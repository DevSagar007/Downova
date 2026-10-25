//! Finding or starting the per-user download service (B0.5, shared since B0.6).
//!
//! Used by every client that may need the service when it is not running: the desktop shell and
//! the browser native host. Clients never link the service itself; they start the `dm-service`
//! executable that ships next to them (Tauri `externalBin` sidecars are installed beside the main
//! binary) and talk to it only over the private IPC endpoint.
//!
//! 1. Read the private endpoint and send `service.v1.hello` (bounded). A compatible service is
//!    reused; an incompatible API version is reported and never replaced.
//! 2. Otherwise start the given `dm-service` **detached** (its own process group / no console), so
//!    closing the client never stops downloads.
//! 3. Wait a bounded time for `hello`. Concurrent starts are settled by the service's single-
//!    instance lock (a second copy exits with code 3); a stale endpoint file is replaced by the
//!    service that wins.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use dm_domain::api::{API_VERSION, HelloResult, methods};
use serde_json::json;

use crate::ipc::Client;

/// How long a single hello attempt may take.
pub const HELLO_TIMEOUT: Duration = Duration::from_secs(3);

/// What a client found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Readiness {
    /// A compatible service answered `hello`.
    Ready(HelloResult),
    /// A service answered with a different API version; it was left running.
    Incompatible(u32),
    /// No service is reachable; the reason is safe to show.
    NotRunning(String),
}

/// One bounded `hello` against the endpoint recorded in `data_dir`.
pub async fn probe(data_dir: &Path) -> Readiness {
    let attempt = async {
        let mut client = Client::connect(data_dir).await.ok()?;
        let hello = client.call(methods::HELLO, json!({})).await.ok()?;
        serde_json::from_value::<HelloResult>(hello).ok()
    };
    match tokio::time::timeout(HELLO_TIMEOUT, attempt).await {
        Ok(Some(hello)) if hello.api_version == API_VERSION => Readiness::Ready(hello),
        Ok(Some(hello)) => Readiness::Incompatible(hello.api_version),
        _ => Readiness::NotRunning("the download service is not running".into()),
    }
}

/// The `dm-service` installed next to the running executable, if any.
pub fn sibling_service() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let path = exe
        .parent()?
        .join(format!("dm-service{}", std::env::consts::EXE_SUFFIX));
    path.is_file().then_some(path)
}

/// Starts `path` detached from the caller; returns a slot that receives its exit code if it ends
/// while the caller still runs (for example 3: another instance won the race).
pub fn spawn_detached(path: &Path) -> std::io::Result<Arc<Mutex<Option<i32>>>> {
    let mut cmd = Command::new(path);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Its own process group: signals aimed at the caller's group do not reach it.
        cmd.process_group(0);
    }
    let mut child = cmd.spawn()?;
    let exit = Arc::new(Mutex::new(None));
    let slot = Arc::clone(&exit);
    // Reap the child if it exits while the caller lives (no zombie on Unix). The thread does not
    // keep a running service alive or tie it to the caller.
    std::thread::spawn(move || {
        if let Ok(status) = child.wait()
            && let Ok(mut guard) = slot.lock()
        {
            *guard = status.code();
        }
    });
    Ok(exit)
}

/// Reuses a compatible service, or starts `service` (when given) and waits up to `start_timeout`.
pub async fn connect_or_start(
    data_dir: &Path,
    service: Option<&Path>,
    start_timeout: Duration,
) -> Readiness {
    match probe(data_dir).await {
        Readiness::NotRunning(_) => {}
        other => return other,
    }
    let Some(path) = service else {
        return Readiness::NotRunning(
            "the download service is not running and no dm-service was found next to this program"
                .into(),
        );
    };
    let exit = match spawn_detached(path) {
        Ok(exit) => exit,
        Err(e) => {
            return Readiness::NotRunning(format!(
                "could not start the download service: {}",
                e.kind()
            ));
        }
    };
    let started = Instant::now();
    loop {
        match probe(data_dir).await {
            Readiness::NotRunning(_) => {}
            other => return other,
        }
        // Exit code 3 means another instance holds the lock: keep waiting for it to answer.
        let code = exit.lock().ok().and_then(|g| *g);
        if let Some(code) = code
            && code != 3
        {
            return Readiness::NotRunning(format!(
                "the download service exited during startup (code {code})"
            ));
        }
        if started.elapsed() > start_timeout {
            return Readiness::NotRunning(format!(
                "the download service did not answer within {} s",
                start_timeout.as_secs()
            ));
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn no_service_and_no_executable_is_not_running_without_waiting() {
        let dir = tempfile::tempdir().unwrap();
        let started = Instant::now();
        let r = connect_or_start(dir.path(), None, Duration::from_secs(20)).await;
        assert!(matches!(r, Readiness::NotRunning(_)), "{r:?}");
        assert!(started.elapsed() < HELLO_TIMEOUT + Duration::from_secs(1));
    }

    #[tokio::test]
    async fn an_executable_that_exits_is_reported_within_the_bound() {
        let dir = tempfile::tempdir().unwrap();
        // A program that exits at once with a non-3 code stands in for a failing service.
        let exe = if cfg!(windows) {
            PathBuf::from(std::env::var("ComSpec").unwrap_or_else(|_| "cmd.exe".into()))
        } else {
            PathBuf::from("/bin/false")
        };
        if !exe.exists() {
            return;
        }
        let r = connect_or_start(dir.path(), Some(&exe), Duration::from_secs(10)).await;
        assert!(matches!(r, Readiness::NotRunning(_)), "{r:?}");
    }
}
