//! A minimal SFTP server for engine tests (F05, ADR 0037).
//!
//! - Host key: a fixed Ed25519 key from a seed, so its fingerprint is stable; another seed gives
//!   a "changed" key.
//! - Sign-in: one user name and password.
//! - Files: in memory; `stat`/`fstat`, `open` (read), `read`, `close`, plus `realpath`.
//! - `cut_next_read_after(n)` drops the connection once `n` bytes of the next file were served.
//!
//! Counts sign-in attempts so tests can prove a password never left before the key was checked.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use russh::keys::ssh_key::private::Ed25519Keypair;
use russh::keys::{HashAlg, PrivateKey};
use russh::server::{Auth, Msg, Server as _, Session};
use russh::{Channel, ChannelId};
use russh_sftp::protocol::{
    Attrs, Data, File, FileAttributes, Handle, Name, Status, StatusCode, Version,
};
use tokio::net::TcpListener;

/// One file the server offers.
#[derive(Debug, Clone)]
pub struct SftpFile {
    pub body: Vec<u8>,
    /// Modification time in seconds since the epoch.
    pub mtime: u32,
}

#[derive(Debug, Default)]
struct Shared {
    files: HashMap<String, SftpFile>,
    cut_next_after: Option<u64>,
    sign_ins: usize,
    reads_from: Vec<u64>,
}

fn lock(m: &Mutex<Shared>) -> std::sync::MutexGuard<'_, Shared> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// A running test SFTP server on 127.0.0.1.
pub struct SftpServer {
    port: u16,
    fingerprint: String,
    shared: Arc<Mutex<Shared>>,
    task: tokio::task::JoinHandle<()>,
}

impl std::fmt::Debug for SftpServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SftpServer")
            .field("port", &self.port)
            .finish()
    }
}

impl Drop for SftpServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// The host key for `seed`.
pub fn host_key(seed: u8) -> PrivateKey {
    PrivateKey::from(Ed25519Keypair::from_seed(&[seed; 32]))
}

/// `SHA256:<base64>` of the host key for `seed`, as clients print it.
pub fn host_fingerprint(seed: u8) -> String {
    host_key(seed)
        .public_key()
        .fingerprint(HashAlg::Sha256)
        .to_string()
}

impl SftpServer {
    /// Starts a server with the host key for `seed`, accepting `user` / `password`.
    pub async fn start(seed: u8, user: &str, password: &str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind sftp");
        let port = listener.local_addr().expect("sftp address").port();
        let shared = Arc::new(Mutex::new(Shared::default()));
        let config = Arc::new(russh::server::Config {
            keys: vec![host_key(seed)],
            auth_rejection_time: std::time::Duration::from_millis(10),
            auth_rejection_time_initial: Some(std::time::Duration::from_millis(0)),
            ..russh::server::Config::default()
        });
        let mut server = Server {
            shared: Arc::clone(&shared),
            user: user.to_owned(),
            password: password.to_owned(),
        };
        let task = tokio::spawn(async move {
            let _ = server.run_on_socket(config, &listener).await;
        });
        Self {
            port,
            fingerprint: host_fingerprint(seed),
            shared,
            task,
        }
    }

    /// `sftp://127.0.0.1:<port>/<path>`.
    pub fn url(&self, path: &str) -> String {
        format!(
            "sftp://127.0.0.1:{}/{}",
            self.port,
            path.trim_start_matches('/')
        )
    }

    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    pub fn set(&self, path: &str, body: Vec<u8>, mtime: u32) {
        lock(&self.shared).files.insert(
            format!("/{}", path.trim_start_matches('/')),
            SftpFile { body, mtime },
        );
    }

    /// The next file read is cut (the connection drops) once `bytes` bytes were served.
    pub fn cut_next_read_after(&self, bytes: u64) {
        lock(&self.shared).cut_next_after = Some(bytes);
    }

    /// Password sign-in attempts the server received.
    pub fn sign_ins(&self) -> usize {
        lock(&self.shared).sign_ins
    }

    /// The offset of the first read of each opened file.
    pub fn reads_from(&self) -> Vec<u64> {
        lock(&self.shared).reads_from.clone()
    }
}

#[derive(Clone)]
struct Server {
    shared: Arc<Mutex<Shared>>,
    user: String,
    password: String,
}

impl russh::server::Server for Server {
    type Handler = SshSession;

    fn new_client(&mut self, _: Option<std::net::SocketAddr>) -> Self::Handler {
        SshSession {
            server: self.clone(),
            channels: HashMap::new(),
        }
    }
}

struct SshSession {
    server: Server,
    channels: HashMap<ChannelId, Channel<Msg>>,
}

impl russh::server::Handler for SshSession {
    type Error = russh::Error;

    async fn auth_password(&mut self, user: &str, password: &str) -> Result<Auth, Self::Error> {
        lock(&self.server.shared).sign_ins += 1;
        if user == self.server.user && password == self.server.password {
            Ok(Auth::Accept)
        } else {
            Ok(Auth::reject())
        }
    }

    async fn channel_open_session(
        &mut self,
        channel: Channel<Msg>,
        reply: russh::server::ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        self.channels.insert(channel.id(), channel);
        reply.accept().await;
        Ok(())
    }

    async fn subsystem_request(
        &mut self,
        channel_id: ChannelId,
        name: &str,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        match (name, self.channels.remove(&channel_id)) {
            ("sftp", Some(channel)) => {
                session.channel_success(channel_id)?;
                let handler = Sftp {
                    shared: Arc::clone(&self.server.shared),
                    open: HashMap::new(),
                    next: AtomicUsize::new(0),
                };
                russh_sftp::server::run(channel.into_stream(), handler).await;
            }
            _ => session.channel_failure(channel_id)?,
        }
        Ok(())
    }
}

struct Sftp {
    shared: Arc<Mutex<Shared>>,
    /// Open handles: path, and whether this file's first read was recorded.
    open: HashMap<String, (String, bool)>,
    next: AtomicUsize,
}

/// The home folder is the root: a relative path `x` is `/x`.
fn absolute(path: &str) -> String {
    if path.starts_with('/') {
        path.to_owned()
    } else {
        format!("/{path}")
    }
}

fn attrs_of(file: &SftpFile) -> FileAttributes {
    FileAttributes {
        size: Some(file.body.len() as u64),
        mtime: Some(file.mtime),
        // A regular file, rw-r--r--.
        permissions: Some(0o100_644),
        ..FileAttributes::default()
    }
}

impl russh_sftp::server::Handler for Sftp {
    type Error = StatusCode;

    fn unimplemented(&self) -> Self::Error {
        StatusCode::OpUnsupported
    }

    async fn init(
        &mut self,
        _version: u32,
        _extensions: HashMap<String, String>,
    ) -> Result<Version, Self::Error> {
        Ok(Version::new())
    }

    async fn realpath(&mut self, id: u32, path: String) -> Result<Name, Self::Error> {
        Ok(Name {
            id,
            files: vec![File::dummy(path)],
        })
    }

    async fn stat(&mut self, id: u32, path: String) -> Result<Attrs, Self::Error> {
        let s = lock(&self.shared);
        let file = s
            .files
            .get(&absolute(&path))
            .ok_or(StatusCode::NoSuchFile)?;
        Ok(Attrs {
            id,
            attrs: attrs_of(file),
        })
    }

    async fn lstat(&mut self, id: u32, path: String) -> Result<Attrs, Self::Error> {
        self.stat(id, path).await
    }

    async fn fstat(&mut self, id: u32, handle: String) -> Result<Attrs, Self::Error> {
        let path = self
            .open
            .get(&handle)
            .map(|(p, _)| p.clone())
            .ok_or(StatusCode::Failure)?;
        self.stat(id, path).await
    }

    async fn open(
        &mut self,
        id: u32,
        filename: String,
        _pflags: russh_sftp::protocol::OpenFlags,
        _attrs: FileAttributes,
    ) -> Result<Handle, Self::Error> {
        let filename = absolute(&filename);
        if !lock(&self.shared).files.contains_key(&filename) {
            return Err(StatusCode::NoSuchFile);
        }
        let handle = format!("h{}", self.next.fetch_add(1, Ordering::SeqCst));
        self.open.insert(handle.clone(), (filename, false));
        Ok(Handle { id, handle })
    }

    async fn read(
        &mut self,
        id: u32,
        handle: String,
        offset: u64,
        len: u32,
    ) -> Result<Data, Self::Error> {
        let (path, recorded) = self
            .open
            .get_mut(&handle)
            .map(|(p, r)| (p.clone(), r))
            .ok_or(StatusCode::Failure)?;
        let mut s = lock(&self.shared);
        if !*recorded {
            *recorded = true;
            s.reads_from.push(offset);
        }
        if let Some(cut) = s.cut_next_after
            && offset >= cut
        {
            s.cut_next_after = None;
            return Err(StatusCode::ConnectionLost);
        }
        let file = s.files.get(&path).ok_or(StatusCode::NoSuchFile)?;
        let start = usize::try_from(offset).map_err(|_| StatusCode::Failure)?;
        if start >= file.body.len() {
            return Err(StatusCode::Eof);
        }
        let end = start.saturating_add(len as usize).min(file.body.len());
        Ok(Data {
            id,
            data: file.body.get(start..end).unwrap_or_default().to_vec(),
        })
    }

    async fn close(&mut self, id: u32, handle: String) -> Result<Status, Self::Error> {
        self.open.remove(&handle);
        Ok(Status {
            id,
            status_code: StatusCode::Ok,
            error_message: "Ok".into(),
            language_tag: "en-US".into(),
        })
    }
}
