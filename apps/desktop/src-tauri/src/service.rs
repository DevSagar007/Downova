//! Service readiness bridge (B0.5): find the per-user `dm-service`, or start the packaged one.
//!
//! 1. Read the private endpoint and send `service.v1.hello`. A compatible service is reused; an
//!    incompatible API version is reported and never replaced.
//! 2. Otherwise start the `dm-service` that ships next to this executable (Tauri `externalBin`
//!    sidecars are installed beside the main binary on every platform). It runs detached from the
//!    window, so closing the UI does not stop downloads.
//! 3. Wait a bounded time for `hello`. Concurrent starts are settled by the service's own
//!    single-instance lock (a second copy exits with code 3); a stale endpoint file is replaced by
//!    the service that wins.
//!
//! The result has the shape of the UI's `ConnectionState` (apps/desktop/src/services).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use dm_domain::api::{API_VERSION, HelloResult, methods};
use dm_domain::{ErrorData, ErrorKind};
use dm_platform::ipc::Client;
use serde_json::{Value, json};

/// How long a single hello attempt may take.
const HELLO_TIMEOUT: Duration = Duration::from_secs(3);
/// How long to wait for a started service to answer.
const START_TIMEOUT: Duration = Duration::from_secs(20);

/// What one hello attempt found.
enum Probe {
    Compatible(HelloResult),
    Incompatible(u32),
    Absent,
}

async fn probe(data_dir: &Path) -> Probe {
    let attempt = async {
        let mut client = Client::connect(data_dir).await.ok()?;
        let hello = client.call(methods::HELLO, json!({})).await.ok()?;
        serde_json::from_value::<HelloResult>(hello).ok()
    };
    match tokio::time::timeout(HELLO_TIMEOUT, attempt).await {
        Ok(Some(hello)) if hello.api_version == API_VERSION => Probe::Compatible(hello),
        Ok(Some(hello)) => Probe::Incompatible(hello.api_version),
        _ => Probe::Absent,
    }
}

fn unavailable(reason: &str, kind: ErrorKind, detail: String) -> Value {
    let error: ErrorData = kind.to_error_data(detail);
    json!({"status": "unavailable", "reason": reason, "error": error})
}

fn connected(hello: &HelloResult) -> Value {
    json!({"status": "connected", "hello": hello})
}

/// The packaged service binary beside this executable.
pub fn sidecar_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let path = exe
        .parent()?
        .join(format!("dm-service{}", std::env::consts::EXE_SUFFIX));
    path.is_file().then_some(path)
}

/// Starts the service detached from the window; returns a slot that receives its exit code if
/// it ends while the shell is running (for example 3: another instance won the race).
fn spawn_detached(path: &Path) -> std::io::Result<Arc<Mutex<Option<i32>>>> {
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
        // Its own process group: terminal or window-manager signals to the shell do not reach it.
        cmd.process_group(0);
    }
    let mut child = cmd.spawn()?;
    let exit = Arc::new(Mutex::new(None));
    let slot = Arc::clone(&exit);
    // Reap the child if it exits while the shell lives (no zombie on Unix). Dropping the thread
    // at shell exit leaves a running service untouched.
    std::thread::spawn(move || {
        if let Ok(status) = child.wait()
            && let Ok(mut guard) = slot.lock()
        {
            *guard = status.code();
        }
    });
    Ok(exit)
}

/// Serialises concurrent `service_connect` calls inside one shell process.
static CONNECTING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Finds or starts the service. Never throws: every outcome is a `ConnectionState`.
pub async fn connect() -> Value {
    let _guard = CONNECTING.lock().await;
    let data_dir = match dm_platform::paths::data_dir() {
        Ok(dir) => dir,
        Err(_) => {
            return unavailable(
                "not_running",
                ErrorKind::ServiceUnavailable,
                "no per-user data directory".into(),
            );
        }
    };
    match probe(&data_dir).await {
        Probe::Compatible(hello) => return connected(&hello),
        Probe::Incompatible(version) => {
            return unavailable(
                "incompatible_version",
                ErrorKind::ServiceUnavailable,
                format!(
                    "the running service speaks API v{version}; this app needs v{API_VERSION}. Quit the other Downova version first."
                ),
            );
        }
        Probe::Absent => {}
    }
    let Some(path) = sidecar_path() else {
        return unavailable(
            "not_running",
            ErrorKind::ServiceUnavailable,
            "the download service is not running and no packaged dm-service was found next to the app".into(),
        );
    };
    let exit = match spawn_detached(&path) {
        Ok(exit) => exit,
        Err(e) => {
            return unavailable(
                "not_running",
                ErrorKind::ServiceUnavailable,
                format!("could not start the download service: {}", e.kind()),
            );
        }
    };
    let started = Instant::now();
    loop {
        match probe(&data_dir).await {
            Probe::Compatible(hello) => return connected(&hello),
            Probe::Incompatible(version) => {
                return unavailable(
                    "incompatible_version",
                    ErrorKind::ServiceUnavailable,
                    format!("another service with API v{version} answered"),
                );
            }
            Probe::Absent => {}
        }
        // Exit code 3 means another instance holds the lock: keep waiting for it to answer.
        let code = exit.lock().ok().and_then(|g| *g);
        if let Some(code) = code
            && code != 3
        {
            return unavailable(
                "not_running",
                ErrorKind::ServiceUnavailable,
                format!("the download service exited during startup (code {code})"),
            );
        }
        if started.elapsed() > START_TIMEOUT {
            return unavailable(
                "not_running",
                ErrorKind::ServiceUnavailable,
                format!(
                    "the download service did not answer within {} s",
                    START_TIMEOUT.as_secs()
                ),
            );
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}
