//! aria2 sidecar adapter (B0.1) implementing [`dm_backend_api::DownloadBackend`].
//!
//! Security and ownership choices (docs/architecture.md §4, §10):
//! - RPC listens on 127.0.0.1 only, on a random free port, with a random 256-bit secret.
//! - The secret is passed through a private `--conf-path` file that is deleted as soon as aria2
//!   answers, not on the command line (other local users can read process arguments on Linux).
//! - `--save-session` is **not** used: aria2 writes per-download options, including `header`
//!   (cookies, Authorization), into session files. Resume relies on aria2's `.aria2` control
//!   files plus the service's own job records.
//! - `--no-netrc`, `--allow-overwrite=false`, `--auto-file-renaming=false`: never pick up ambient
//!   credentials and never overwrite or silently rename user files.
//! - `--stop-with-process=<our pid>`: aria2 exits if the owning process dies (no orphans).
//! - Handles returned to the service are attempt IDs; aria2 GIDs never leave this crate.

pub mod binary;
pub mod capabilities;
pub mod errors;
mod rpc;

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use dm_backend_api::{
    BackendError, BackendHandle, BackendProgress, DownloadBackend, TransferRequest, TransferState,
};
use dm_domain::redact::Secret;
use dm_domain::{ByteCount, CapabilityReport, ErrorKind};
use serde_json::{Value, json};
use tokio::process::Child;

pub use binary::{Aria2Binary, PINNED_VERSION};
use rpc::RpcClient;

/// Launch options. Defaults are conservative starting values (research §15.4), not tuned optima.
#[derive(Debug, Clone)]
pub struct LaunchConfig {
    pub binary: Aria2Binary,
    /// Private directory for the adapter's transient files. Created if missing.
    pub work_dir: PathBuf,
    /// aria2 `--max-tries` (0 = unlimited is never used). **Default 1** (B0.4, ADR 0003): any
    /// engine-internal retry resumes with a bare `Range` request and no validator, which mixed
    /// old and new bytes in fixtures C01–C05. Retries are to become fresh service attempts (COR-04, B1); until then a failure is final.
    /// `Http` is reported `supported` only when this is 1.
    pub max_tries: u32,
    /// aria2 `--retry-wait` in seconds.
    pub retry_wait_secs: u32,
    /// aria2 `--min-split-size` (aria2 accepts 1M–1024M).
    pub min_split_size: String,
    /// Extra trusted CA bundle (PEM) for HTTPS (`--ca-certificate`). Certificate verification
    /// itself is never disabled.
    pub ca_certificate: Option<PathBuf>,
    /// Forward credential headers (Authorization, Cookie, …) to the engine. **Default false**:
    /// aria2 re-sends every `--header` to redirect targets on other origins (fixture D02), so
    /// credentialed transfers are refused until redirect scoping exists (BRW-05). Only the
    /// characterization fixtures set this to true; the service never does.
    pub forward_credential_headers_unsafe: bool,
}

impl LaunchConfig {
    pub fn new(binary: Aria2Binary, work_dir: impl Into<PathBuf>) -> Self {
        Self {
            binary,
            work_dir: work_dir.into(),
            max_tries: 1,
            retry_wait_secs: 2,
            min_split_size: "20M".to_owned(),
            ca_certificate: None,
            forward_credential_headers_unsafe: false,
        }
    }
}

/// A running aria2 process owned by this adapter.
#[derive(Debug)]
pub struct Aria2Backend {
    child: tokio::sync::Mutex<Child>,
    pid: Option<u32>,
    rpc: RpcClient,
    binary: Aria2Binary,
    /// attempt ID → aria2 GID. The GID is private to this adapter.
    attempts: Mutex<HashMap<String, String>>,
    max_tries: u32,
    forward_credential_headers: bool,
}

const CONF_FILE: &str = "aria2-rpc.conf";

impl Aria2Backend {
    /// Starts aria2 and waits until its RPC endpoint answers.
    pub async fn launch(config: LaunchConfig) -> Result<Self, BackendError> {
        fs::create_dir_all(&config.work_dir).map_err(io("create work dir"))?;
        let port = free_loopback_port()?;
        let secret = random_secret()?;
        let conf_path = config.work_dir.join(CONF_FILE);
        write_private_file(&conf_path, &format!("rpc-secret={}\n", secret.expose()))?;

        let child = tokio::process::Command::new(&config.binary.path)
            .args(launch_args(&config, &conf_path, port))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn();
        let child = match child {
            Ok(child) => child,
            Err(e) => {
                let _ = fs::remove_file(&conf_path);
                return Err(BackendError::new(
                    ErrorKind::Internal,
                    format!("spawn aria2c: {e}"),
                ));
            }
        };

        let pid = child.id();
        let backend = Self {
            child: tokio::sync::Mutex::new(child),
            pid,
            rpc: RpcClient::new(port, &secret)?,
            binary: config.binary,
            attempts: Mutex::new(HashMap::new()),
            max_tries: config.max_tries,
            forward_credential_headers: config.forward_credential_headers_unsafe,
        };
        let ready = backend.wait_ready(Duration::from_secs(10)).await;
        // The secret file is only needed until aria2 has read it.
        let _ = fs::remove_file(&conf_path);
        ready?;
        Ok(backend)
    }

    /// Version reported by the running binary.
    pub fn version(&self) -> &str {
        &self.binary.version
    }

    /// OS process ID of the running aria2c (for diagnostics and fault-injection tests).
    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    /// The binary this backend runs.
    pub fn binary(&self) -> &Aria2Binary {
        &self.binary
    }

    /// Asks aria2 to exit, then kills it if it does not stop in time.
    pub async fn shutdown(&self) -> Result<(), BackendError> {
        let _ = self.rpc.call("aria2.forceShutdown", vec![]).await;
        let mut child = self.child.lock().await;
        if tokio::time::timeout(Duration::from_secs(5), child.wait())
            .await
            .is_err()
        {
            child
                .kill()
                .await
                .map_err(|e| BackendError::new(ErrorKind::Internal, format!("kill aria2c: {e}")))?;
        }
        Ok(())
    }

    async fn wait_ready(&self, limit: Duration) -> Result<(), BackendError> {
        let started = Instant::now();
        loop {
            if self.rpc.call("aria2.getVersion", vec![]).await.is_ok() {
                return Ok(());
            }
            if let Ok(Some(status)) = self.child.lock().await.try_wait() {
                return Err(BackendError::new(
                    ErrorKind::Internal,
                    format!("aria2c exited during startup ({status})"),
                ));
            }
            if started.elapsed() > limit {
                return Err(BackendError::new(
                    ErrorKind::Internal,
                    "aria2c RPC did not start",
                ));
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    fn gid(&self, handle: &BackendHandle) -> Result<String, BackendError> {
        self.attempts
            .lock()
            .map_err(|_| BackendError::new(ErrorKind::Internal, "attempt map poisoned"))?
            .get(&handle.0)
            .cloned()
            .ok_or_else(|| BackendError::new(ErrorKind::InvalidRequest, "unknown attempt"))
    }

    async fn status_of(&self, gid: &str) -> Result<Value, BackendError> {
        let keys = json!([
            "status",
            "totalLength",
            "completedLength",
            "connections",
            "errorCode",
            "errorMessage"
        ]);
        self.rpc
            .call("aria2.tellStatus", vec![json!(gid), keys])
            .await
    }
}

impl DownloadBackend for Aria2Backend {
    fn id(&self) -> &'static str {
        "aria2"
    }

    fn capabilities(&self) -> Vec<CapabilityReport> {
        capabilities::report(self.max_tries)
    }

    async fn start(&self, request: TransferRequest) -> Result<BackendHandle, BackendError> {
        validate_file_name(&request.file_name)?;
        if !self.forward_credential_headers
            && request
                .headers
                .iter()
                .any(|(name, _)| dm_domain::redact::is_sensitive_header(name))
        {
            return Err(BackendError::new(
                ErrorKind::BackendCapabilityUnavailable,
                "credentialed transfers are not supported: the engine forwards headers to other origins on redirect",
            ));
        }
        let attempt = request.attempt_id.as_str().to_owned();
        if self.gid(&BackendHandle(attempt.clone())).is_ok() {
            return Err(BackendError::new(
                ErrorKind::InvalidRequest,
                "attempt already started",
            ));
        }
        let connections = request.max_connections.max(1).to_string();
        let headers: Vec<String> = request
            .headers
            .iter()
            .map(|(name, value)| format!("{name}: {}", value.expose()))
            .collect();
        let options = json!({
            "dir": request.destination_dir.to_string_lossy(),
            "out": request.file_name,
            "split": connections,
            "max-connection-per-server": connections,
            "header": headers,
        });
        let gid = self
            .rpc
            .call("aria2.addUri", vec![json!([request.url]), options])
            .await?
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| BackendError::new(ErrorKind::Internal, "aria2 addUri: no GID"))?;
        self.attempts
            .lock()
            .map_err(|_| BackendError::new(ErrorKind::Internal, "attempt map poisoned"))?
            .insert(attempt.clone(), gid);
        Ok(BackendHandle(attempt))
    }

    async fn pause(&self, handle: &BackendHandle) -> Result<(), BackendError> {
        let gid = self.gid(handle)?;
        self.rpc
            .call("aria2.forcePause", vec![json!(gid)])
            .await
            .map(drop)
    }

    async fn resume(&self, handle: &BackendHandle) -> Result<(), BackendError> {
        let gid = self.gid(handle)?;
        self.rpc
            .call("aria2.unpause", vec![json!(gid)])
            .await
            .map(drop)
    }

    async fn remove(&self, handle: &BackendHandle) -> Result<(), BackendError> {
        let gid = self.gid(handle)?;
        // forceRemove fails for already-stopped downloads; that is fine, the result is removed below.
        let _ = self.rpc.call("aria2.forceRemove", vec![json!(gid)]).await;
        let started = Instant::now();
        loop {
            let status = self.status_of(&gid).await?;
            let state = status
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if matches!(state, "removed" | "complete" | "error") {
                break;
            }
            if started.elapsed() > Duration::from_secs(10) {
                return Err(BackendError::new(
                    ErrorKind::Internal,
                    "aria2 did not stop transfer",
                ));
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let _ = self
            .rpc
            .call("aria2.removeDownloadResult", vec![json!(gid)])
            .await;
        self.attempts
            .lock()
            .map_err(|_| BackendError::new(ErrorKind::Internal, "attempt map poisoned"))?
            .remove(&handle.0);
        Ok(())
    }

    async fn progress(&self, handle: &BackendHandle) -> Result<BackendProgress, BackendError> {
        let gid = self.gid(handle)?;
        let status = self.status_of(&gid).await?;
        parse_progress(&status)
    }
}

/// Translates a `tellStatus` reply.
fn parse_progress(status: &Value) -> Result<BackendProgress, BackendError> {
    let field = |name: &str| status.get(name).and_then(Value::as_str).unwrap_or("0");
    let number = |name: &str| {
        ByteCount::parse_decimal(field(name))
            .map_err(|_| BackendError::new(ErrorKind::Internal, format!("aria2 bad {name}")))
    };
    let state = match field("status") {
        "active" => TransferState::Active,
        "waiting" => TransferState::Waiting,
        "paused" => TransferState::Paused,
        "complete" => TransferState::Complete,
        "removed" => TransferState::Removed,
        "error" => {
            let code = field("errorCode").parse::<u32>().unwrap_or(1);
            // errorMessage is used only to extract a status number; it is never forwarded.
            let message = status.get("errorMessage").and_then(Value::as_str);
            TransferState::Failed(errors::classify(code, message))
        }
        _ => {
            return Err(BackendError::new(
                ErrorKind::Internal,
                "aria2 unknown status",
            ));
        }
    };
    let completed = number("completedLength")?;
    let total = number("totalLength")?;
    // aria2 reports 0 before the size is known; only a finished transfer can truly be empty.
    let total = (total.0 > 0 || state == TransferState::Complete).then_some(total);
    let connections = field("connections").parse::<u32>().unwrap_or(0);
    Ok(BackendProgress {
        state,
        completed,
        total,
        connections,
    })
}

fn launch_args(config: &LaunchConfig, conf_path: &Path, port: u16) -> Vec<String> {
    let mut args = vec![
        format!("--conf-path={}", conf_path.display()),
        "--enable-rpc=true".into(),
        "--rpc-listen-all=false".into(),
        format!("--rpc-listen-port={port}"),
        "--rpc-allow-origin-all=false".into(),
        format!("--stop-with-process={}", std::process::id()),
        "--no-netrc=true".into(),
        "--allow-overwrite=false".into(),
        "--auto-file-renaming=false".into(),
        // Never append to a pre-existing file that has no aria2 control file (B0.4 K03).
        "--continue=false".into(),
        "--conditional-get=false".into(),
        "--remote-time=false".into(),
        "--check-certificate=true".into(),
        "--file-allocation=none".into(),
        format!("--max-tries={}", config.max_tries),
        format!("--retry-wait={}", config.retry_wait_secs),
        format!("--min-split-size={}", config.min_split_size),
        "--summary-interval=0".into(),
        "--console-log-level=error".into(),
        "--quiet=true".into(),
        "--enable-color=false".into(),
    ];
    if let Some(ca) = &config.ca_certificate {
        args.push(format!("--ca-certificate={}", ca.display()));
    }
    args
}

fn validate_file_name(name: &str) -> Result<(), BackendError> {
    let bad = name.is_empty()
        || name == "."
        || name == ".."
        || name.contains(['/', '\\'])
        || name.chars().any(char::is_control);
    if bad {
        return Err(BackendError::new(
            ErrorKind::InvalidRequest,
            "invalid file name",
        ));
    }
    Ok(())
}

fn free_loopback_port() -> Result<u16, BackendError> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).map_err(io("bind port"))?;
    let port = listener.local_addr().map_err(io("read port"))?.port();
    Ok(port)
}

fn random_secret() -> Result<Secret, BackendError> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes)
        .map_err(|e| BackendError::new(ErrorKind::Internal, format!("random secret: {e}")))?;
    Ok(Secret::new(
        bytes.iter().map(|b| format!("{b:02x}")).collect::<String>(),
    ))
}

/// Writes a file readable only by the current user (mode 0600 on Unix; on Windows the default
/// per-user ACL of the work directory applies).
fn write_private_file(path: &Path, contents: &str) -> Result<(), BackendError> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(io("create conf file"))?;
    file.write_all(contents.as_bytes())
        .map_err(io("write conf file"))
}

fn io(what: &'static str) -> impl Fn(std::io::Error) -> BackendError {
    move |e| BackendError::new(ErrorKind::Internal, format!("{what}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_cannot_escape_the_destination() {
        for bad in ["", ".", "..", "a/b.zip", "a\\b.zip", "a\nb"] {
            assert!(validate_file_name(bad).is_err(), "{bad:?}");
        }
        assert!(validate_file_name("বাংলা ফাইল.zip").is_ok());
    }

    #[test]
    fn unknown_size_is_none_until_complete() {
        let p = parse_progress(&json!({
            "status": "active", "totalLength": "0", "completedLength": "0", "connections": "1"
        }))
        .unwrap();
        assert_eq!(p.total, None);
        let done = parse_progress(&json!({
            "status": "complete", "totalLength": "0", "completedLength": "0"
        }))
        .unwrap();
        assert_eq!(done.total, Some(ByteCount(0)));
    }

    #[test]
    fn error_status_is_classified_without_message() {
        let p = parse_progress(&json!({
            "status": "error", "totalLength": "10", "completedLength": "3", "errorCode": "24"
        }))
        .unwrap();
        assert_eq!(
            p.state,
            TransferState::Failed(BackendError::new(
                ErrorKind::AuthRequired,
                "authorization failed (HTTP 401)"
            ))
        );
    }

    #[test]
    fn secret_is_never_on_the_command_line() {
        let config = LaunchConfig::new(
            Aria2Binary {
                path: PathBuf::from("aria2c"),
                version: PINNED_VERSION.into(),
            },
            "work",
        );
        let args = launch_args(&config, Path::new("work/aria2-rpc.conf"), 6800);
        assert!(args.iter().all(|a| !a.starts_with("--rpc-secret")));
        assert!(args.iter().all(|a| !a.starts_with("--save-session")));
        assert!(args.contains(&"--rpc-listen-all=false".to_owned()));
        assert!(args.contains(&"--allow-overwrite=false".to_owned()));
        assert!(args.contains(&"--continue=false".to_owned()));
        assert!(
            args.contains(&"--max-tries=1".to_owned()),
            "production default: no engine-internal resume"
        );
    }
}
