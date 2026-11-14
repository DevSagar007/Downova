//! A minimal FTP server for engine tests (F05, ADR 0035).
//!
//! Supports what a download needs: `USER`/`PASS` (anonymous, or refused with 530), `TYPE`,
//! `SIZE`, `MDTM`, `EPSV`, `PASV`, `REST`, `RETR`, `QUIT`, plus harmless `SYST`/`FEAT`/`PWD`/
//! `NOOP`/`OPTS`. Behaviour switches let tests remove `EPSV` or `MDTM`, make `PASV` name a wrong
//! address, refuse the login, or cut the next data transfer short. Every command line is logged.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

/// One file the server offers.
#[derive(Debug, Clone)]
pub struct FtpFile {
    pub body: Vec<u8>,
    /// `MDTM` answer, `YYYYMMDDHHMMSS`.
    pub modified: String,
}

/// Switches for the server's behaviour (all off: a well-behaved anonymous server).
#[derive(Debug, Clone, Default)]
pub struct FtpBehaviour {
    /// `EPSV` answers 502 (the client must use `PASV`).
    pub no_epsv: bool,
    /// `MDTM` answers 502.
    pub no_mdtm: bool,
    /// `PASV` names 10.255.255.1 instead of the server's address.
    pub pasv_wrong_address: bool,
    /// `PASS` answers 530.
    pub refuse_login: bool,
}

#[derive(Debug, Default)]
struct Shared {
    files: HashMap<String, FtpFile>,
    log: Vec<String>,
    /// Bytes the next `RETR` sends before closing the data connection with 426.
    cut_next_after: Option<usize>,
}

/// A running test FTP server on 127.0.0.1.
#[derive(Debug)]
pub struct FtpServer {
    port: u16,
    shared: Arc<Mutex<Shared>>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for FtpServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn lock(m: &Mutex<Shared>) -> std::sync::MutexGuard<'_, Shared> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl FtpServer {
    pub async fn start(behaviour: FtpBehaviour) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind ftp");
        let port = listener.local_addr().expect("ftp address").port();
        let shared = Arc::new(Mutex::new(Shared::default()));
        let s = Arc::clone(&shared);
        let behaviour = Arc::new(behaviour);
        let task = tokio::spawn(async move {
            while let Ok((conn, _)) = listener.accept().await {
                let s = Arc::clone(&s);
                let b = Arc::clone(&behaviour);
                tokio::spawn(async move {
                    let _ = session(conn, &b, &s).await;
                });
            }
        });
        Self { port, shared, task }
    }

    /// `ftp://127.0.0.1:<port>/<path>`.
    pub fn url(&self, path: &str) -> String {
        format!(
            "ftp://127.0.0.1:{}/{}",
            self.port,
            path.trim_start_matches('/')
        )
    }

    pub fn set(&self, path: &str, body: Vec<u8>, modified: &str) {
        lock(&self.shared).files.insert(
            format!("/{}", path.trim_start_matches('/')),
            FtpFile {
                body,
                modified: modified.to_owned(),
            },
        );
    }

    /// The next `RETR` sends only `bytes` bytes, then closes the data connection with 426.
    pub fn cut_next_transfer_after(&self, bytes: usize) {
        lock(&self.shared).cut_next_after = Some(bytes);
    }

    /// Every command line received, in order.
    pub fn log(&self) -> Vec<String> {
        lock(&self.shared).log.clone()
    }
}

async fn reply(w: &mut tokio::net::tcp::OwnedWriteHalf, line: &str) -> std::io::Result<()> {
    w.write_all(format!("{line}\r\n").as_bytes()).await
}

async fn session(conn: TcpStream, b: &FtpBehaviour, shared: &Mutex<Shared>) -> std::io::Result<()> {
    let (read, mut write) = conn.into_split();
    let mut lines = BufReader::new(read).lines();
    reply(&mut write, "220 test FTP ready").await?;
    let mut user = String::new();
    let mut data: Option<TcpListener> = None;
    let mut rest: usize = 0;
    while let Some(line) = lines.next_line().await? {
        lock(shared).log.push(line.clone());
        let (cmd, arg) = line.split_once(' ').unwrap_or((line.as_str(), ""));
        let arg = arg.to_owned();
        match cmd.to_ascii_uppercase().as_str() {
            "USER" => {
                user = arg;
                reply(&mut write, "331 password please").await?;
            }
            "PASS" => {
                if b.refuse_login || user != "anonymous" {
                    reply(&mut write, "530 login incorrect").await?;
                } else {
                    reply(&mut write, "230 logged in").await?;
                }
            }
            "TYPE" | "OPTS" | "NOOP" => reply(&mut write, "200 ok").await?,
            "SYST" => reply(&mut write, "215 UNIX Type: L8").await?,
            "FEAT" => reply(&mut write, "211 no features").await?,
            "PWD" => reply(&mut write, "257 \"/\"").await?,
            "SIZE" => {
                let size = lock(shared).files.get(&arg).map(|f| f.body.len());
                match size {
                    Some(n) => reply(&mut write, &format!("213 {n}")).await?,
                    None => reply(&mut write, "550 no such file").await?,
                }
            }
            "MDTM" => {
                if b.no_mdtm {
                    reply(&mut write, "502 not implemented").await?;
                    continue;
                }
                let modified = lock(shared).files.get(&arg).map(|f| f.modified.clone());
                match modified {
                    Some(m) => reply(&mut write, &format!("213 {m}")).await?,
                    None => reply(&mut write, "550 no such file").await?,
                }
            }
            "EPSV" => {
                if b.no_epsv {
                    reply(&mut write, "502 not implemented").await?;
                    continue;
                }
                let listener = TcpListener::bind("127.0.0.1:0").await?;
                let port = listener.local_addr()?.port();
                data = Some(listener);
                reply(
                    &mut write,
                    &format!("229 Entering Extended Passive Mode (|||{port}|)"),
                )
                .await?;
            }
            "PASV" => {
                let listener = TcpListener::bind("127.0.0.1:0").await?;
                let port = listener.local_addr()?.port();
                data = Some(listener);
                let host = if b.pasv_wrong_address {
                    "10,255,255,1"
                } else {
                    "127,0,0,1"
                };
                reply(
                    &mut write,
                    &format!(
                        "227 Entering Passive Mode ({host},{},{})",
                        port >> 8,
                        port & 0xff
                    ),
                )
                .await?;
            }
            "REST" => {
                rest = arg.parse().unwrap_or(0);
                reply(&mut write, "350 restarting").await?;
            }
            "RETR" => {
                let file = lock(shared).files.get(&arg).cloned();
                let Some(file) = file else {
                    reply(&mut write, "550 no such file").await?;
                    continue;
                };
                let Some(listener) = data.take() else {
                    reply(&mut write, "425 use PASV or EPSV first").await?;
                    continue;
                };
                reply(&mut write, "150 opening data connection").await?;
                let accepted =
                    tokio::time::timeout(std::time::Duration::from_secs(10), listener.accept())
                        .await;
                let Ok(Ok((mut conn, _))) = accepted else {
                    reply(&mut write, "425 no data connection").await?;
                    continue;
                };
                let from = rest.min(file.body.len());
                rest = 0;
                let cut = lock(shared).cut_next_after.take();
                let end = cut.map_or(file.body.len(), |n| {
                    from.saturating_add(n).min(file.body.len())
                });
                let sent = conn
                    .write_all(file.body.get(from..end).unwrap_or_default())
                    .await;
                let _ = conn.shutdown().await;
                drop(conn);
                if cut.is_some() || sent.is_err() {
                    reply(&mut write, "426 connection closed; transfer aborted").await?;
                } else {
                    reply(&mut write, "226 transfer complete").await?;
                }
            }
            "QUIT" => {
                reply(&mut write, "221 bye").await?;
                return Ok(());
            }
            _ => reply(&mut write, "502 not implemented").await?,
        }
    }
    Ok(())
}
