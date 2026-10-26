//! Exercises the real `dm-service` binary as a separate process (B0.2, extended in B0.4 and B1.1).
//!
//! Tests that need aria2c are `#[ignore]`d in plain `cargo test` and run by `pnpm verify`
//! (group `aria2`), which reports UNAVAILABLE when no aria2c binary exists.

#![allow(
    clippy::unwrap_used,
    clippy::print_stdout,
    clippy::arithmetic_side_effects
)]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use dm_domain::api::{DownloadSnapshot, HelloResult, ListDownloadsResult, methods};
use dm_domain::capability::support_of;
use dm_domain::{Capability, CapabilitySupport, ErrorKind, Phase};
use dm_platform::ipc::Client;
use serde_json::json;
use sha2::{Digest, Sha256};

const SERVICE: &str = env!("CARGO_BIN_EXE_dm-service");
const MIB: usize = 1024 * 1024;

fn aria2() -> Option<PathBuf> {
    let exe = format!("aria2c{}", std::env::consts::EXE_SUFFIX);
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    std::env::var_os("DM_ARIA2_PATH")
        .map(PathBuf::from)
        .into_iter()
        .chain([root.join("tools/aria2").join(&exe)])
        .chain(
            std::env::var_os("PATH")
                .iter()
                .flat_map(std::env::split_paths)
                .map(|d| d.join(&exe)),
        )
        .find(|p| p.is_file())
}

/// A service started by this test. Dropping it (on success, panic or timeout) kills and reaps
/// the process if it is still running, so a failed assertion can never leave a service that keeps
/// `target/debug/dm-service` (and its database) locked for later builds and tests. Only this
/// child is touched; graceful-shutdown assertions still run first where a test makes them.
struct Service(Child);

impl std::ops::Deref for Service {
    type Target = Child;
    fn deref(&self) -> &Child {
        &self.0
    }
}

impl std::ops::DerefMut for Service {
    fn deref_mut(&mut self) -> &mut Child {
        &mut self.0
    }
}

impl Drop for Service {
    fn drop(&mut self) {
        if matches!(self.0.try_wait(), Ok(None)) {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}

fn spawn(data: &Path, downloads: &Path, aria2: &Path) -> Service {
    Service(
        Command::new(SERVICE)
            .arg("--data-dir")
            .arg(data)
            .arg("--download-dir")
            .arg(downloads)
            .args(["--backend", "aria2", "--aria2"])
            .arg(aria2)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    )
}

async fn wait_for_endpoint(data: &Path) -> Client {
    let started = Instant::now();
    loop {
        if let Ok(client) = Client::connect(data).await {
            return client;
        }
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "service did not come up"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn wait_exit(child: &mut Child, limit: Duration) -> Option<i32> {
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status.code();
        }
        if started.elapsed() > limit {
            let _ = child.kill();
            return None;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn payload(len: usize) -> Vec<u8> {
    let mut state: u32 = 0x2545_F491;
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            (state & 0xFF) as u8
        })
        .collect()
}

fn sha_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Minimal controlled HTTP fixture: `/file` fast, `/slow` ~1 MiB/s; 200 + Content-Length.
async fn fixture(body: Vec<u8>) -> std::net::SocketAddr {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let body = std::sync::Arc::new(body);
    tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = listener.accept().await else {
                return;
            };
            let body = body.clone();
            tokio::spawn(async move {
                let mut head = Vec::new();
                let mut b = [0u8; 1];
                while !head.ends_with(b"\r\n\r\n") {
                    if s.read(&mut b).await.unwrap_or(0) == 0 {
                        return;
                    }
                    head.push(b[0]);
                }
                let slow = String::from_utf8_lossy(&head).starts_with("GET /slow");
                let hdr = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                if s.write_all(hdr.as_bytes()).await.is_err() {
                    return;
                }
                for chunk in body.chunks(16 * 1024) {
                    if s.write_all(chunk).await.is_err() {
                        return;
                    }
                    if slow {
                        tokio::time::sleep(Duration::from_millis(15)).await;
                    }
                }
            });
        }
    });
    addr
}

fn child_pids(parent: u32) -> Vec<u32> {
    let out = if cfg!(windows) {
        Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                &format!(
                    "(Get-CimInstance Win32_Process -Filter 'ParentProcessId={parent}').ProcessId"
                ),
            ])
            .output()
            .unwrap()
    } else {
        Command::new("pgrep")
            .args(["-P", &parent.to_string()])
            .output()
            .unwrap()
    };
    String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .filter_map(|p| p.parse().ok())
        .collect()
}

fn pid_alive(pid: u32) -> bool {
    if cfg!(windows) {
        let out = Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).contains(&pid.to_string())
    } else {
        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .status()
            .is_ok_and(|s| s.success())
    }
}

/// "running", "zombie" (exited, not reaped by its parent) or "gone". `pid_alive` treats a zombie
/// as alive on purpose: an unreaped engine means the service never observed its exit.
fn process_state(pid: u32) -> &'static str {
    if cfg!(windows) {
        return if pid_alive(pid) { "running" } else { "gone" };
    }
    let out = Command::new("ps")
        .args(["-o", "stat=", "-p", &pid.to_string()])
        .output()
        .unwrap();
    let stat = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    if stat.is_empty() {
        "gone"
    } else if stat.starts_with('Z') {
        "zombie"
    } else {
        "running"
    }
}

fn kill_pid(pid: u32) {
    if cfg!(windows) {
        Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/F"])
            .output()
            .unwrap();
    } else {
        Command::new("kill")
            .args(["-9", &pid.to_string()])
            .status()
            .unwrap();
    }
}

fn add_params(key: &str, url: &str, sha: Option<&str>) -> serde_json::Value {
    json!({
        "idempotency_key": key,
        "source": {"url": url},
        "destination_profile_id": "downloads",
        "expected_size_bytes": null,
        "expected_sha256": sha
    })
}

async fn get(client: &mut Client, id: &str) -> DownloadSnapshot {
    serde_json::from_value(
        client
            .call(methods::GET, json!({"job_id": id}))
            .await
            .unwrap(),
    )
    .unwrap()
}

async fn wait_terminal(client: &mut Client, id: &str, limit: Duration) -> DownloadSnapshot {
    let started = Instant::now();
    loop {
        let s = get(client, id).await;
        if s.phase.is_terminal() || started.elapsed() > limit {
            return s;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[test]
fn missing_aria2_is_a_startup_failure() {
    let data = tempfile::tempdir().unwrap();
    let mut child = spawn(data.path(), data.path(), &data.path().join("no-aria2c"));
    assert_eq!(wait_exit(&mut child, Duration::from_secs(20)), Some(4));
}

#[test]
fn bad_arguments_exit_with_2() {
    let status = Command::new(SERVICE)
        .arg("--bogus")
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(2));
}

#[tokio::test]
#[ignore = "needs aria2c; run via `pnpm verify` (group aria2)"]
async fn real_service_single_instance_capabilities_and_https_gate() {
    let aria2 = aria2().expect("aria2c must be available");
    let data = tempfile::tempdir().unwrap();
    let downloads = tempfile::tempdir().unwrap();
    let mut service = spawn(data.path(), downloads.path(), &aria2);
    let mut client = wait_for_endpoint(data.path()).await;

    let hello: HelloResult =
        serde_json::from_value(client.call(methods::HELLO, json!({})).await.unwrap()).unwrap();
    let caps = &hello.backend.capabilities;
    println!(
        "hello: backend {} {:?}",
        hello.backend.id, hello.backend.version
    );
    assert_eq!(hello.backend.id, "aria2");
    // ADR 0003 (B0.4): Http promoted; the rest stays explicit.
    assert_eq!(
        support_of(caps, Capability::Http),
        CapabilitySupport::Supported
    );
    assert_eq!(
        support_of(caps, Capability::Https),
        CapabilitySupport::Unverified
    );
    assert_eq!(
        support_of(caps, Capability::SegmentedHttp),
        CapabilitySupport::Unsupported
    );
    assert_eq!(
        support_of(caps, Capability::ResumeStrongValidator),
        CapabilitySupport::Unsupported
    );

    let mut second = spawn(data.path(), downloads.path(), &aria2);
    assert_eq!(wait_exit(&mut second, Duration::from_secs(20)), Some(3));

    // HTTPS stays gated: refused, no job, no secret echoed.
    let refused = client
        .call(
            methods::ADD,
            add_params(
                "process-https-1",
                "https://127.0.0.1:9/f.bin?token=SYNTHETIC-SECRET",
                None,
            ),
        )
        .await
        .unwrap_err();
    println!("https add refused: {refused}");
    assert_eq!(refused.kind(), ErrorKind::BackendCapabilityUnavailable);
    assert!(!refused.to_string().contains("SYNTHETIC-SECRET"));
    let list: ListDownloadsResult =
        serde_json::from_value(client.call(methods::LIST, json!({})).await.unwrap()).unwrap();
    assert!(list.items.is_empty());

    let unknown = client
        .call(methods::GET, json!({"job_id": "job_unknown"}))
        .await
        .unwrap_err();
    assert_eq!(unknown.kind(), ErrorKind::InvalidRequest);

    client.call(methods::SHUTDOWN, json!({})).await.unwrap();
    assert_eq!(wait_exit(&mut service, Duration::from_secs(20)), Some(0));
    assert!(
        !dm_platform::endpoint::endpoint_file(data.path()).exists(),
        "endpoint file removed"
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs aria2c; run via `pnpm verify` (group aria2)"]
async fn real_http_download_through_the_service_is_sha256_verified() {
    let aria2 = aria2().expect("aria2c must be available");
    let body = payload(3 * MIB);
    let addr = fixture(body.clone()).await;
    let data = tempfile::tempdir().unwrap();
    let downloads = tempfile::tempdir().unwrap();
    let mut service = spawn(data.path(), downloads.path(), &aria2);
    let mut client = wait_for_endpoint(data.path()).await;

    // Trusted hash supplied: the service verifies it before reporting Completed.
    let url = format!("http://{addr}/file/data.bin");
    let snap: DownloadSnapshot = serde_json::from_value(
        client
            .call(
                methods::ADD,
                add_params("process-dl-1", &url, Some(&sha_hex(&body))),
            )
            .await
            .unwrap(),
    )
    .unwrap();
    let done = wait_terminal(&mut client, snap.job_id.as_str(), Duration::from_secs(60)).await;
    let file = std::fs::read(downloads.path().join(&done.display_name)).unwrap();
    println!(
        "download: phase {:?}, {} bytes, sha256 {}",
        done.phase,
        file.len(),
        sha_hex(&file)
    );
    assert_eq!(done.phase, Phase::Completed);
    assert_eq!(done.total_bytes.map(|b| b.0), Some(body.len() as u64));
    assert_eq!(
        sha_hex(&file),
        sha_hex(&body),
        "file bytes equal the fixture payload"
    );

    // A wrong trusted hash is never reported as completed.
    let bad: DownloadSnapshot = serde_json::from_value(
        client
            .call(
                methods::ADD,
                add_params("process-dl-2", &url, Some(&"0".repeat(64))),
            )
            .await
            .unwrap(),
    )
    .unwrap();
    let failed = wait_terminal(&mut client, bad.job_id.as_str(), Duration::from_secs(60)).await;
    println!(
        "wrong hash: phase {:?}, error {:?}",
        failed.phase,
        failed.error.as_ref().map(|e| e.kind)
    );
    assert_eq!(failed.phase, Phase::Failed);
    assert_eq!(failed.error.unwrap().kind, ErrorKind::ChecksumMismatch);

    client.call(methods::SHUTDOWN, json!({})).await.unwrap();
    assert_eq!(wait_exit(&mut service, Duration::from_secs(20)), Some(0));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs aria2c; run via `pnpm verify` (group aria2)"]
async fn engine_killed_during_a_service_transfer_is_reported_honestly() {
    let aria2 = aria2().expect("aria2c must be available");
    let body = payload(6 * MIB);
    let addr = fixture(body.clone()).await;
    let data = tempfile::tempdir().unwrap();
    let downloads = tempfile::tempdir().unwrap();
    let mut service = spawn(data.path(), downloads.path(), &aria2);
    let mut client = wait_for_endpoint(data.path()).await;
    let engine_pid = *child_pids(service.id())
        .first()
        .expect("aria2c child of dm-service");

    let snap: DownloadSnapshot = serde_json::from_value(
        client
            .call(
                methods::ADD,
                add_params("process-kill-1", &format!("http://{addr}/slow/k.bin"), None),
            )
            .await
            .unwrap(),
    )
    .unwrap();
    // Kill only once bytes are flowing (bounded), not after a fixed delay.
    let started = Instant::now();
    while get(&mut client, snap.job_id.as_str())
        .await
        .downloaded_bytes
        .0
        == 0
    {
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "transfer never started"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    kill_pid(engine_pid);
    // The service reports the loss only after its exit monitor has seen (and reaped) the engine.
    let after = wait_terminal(&mut client, snap.job_id.as_str(), Duration::from_secs(20)).await;
    let engine_state = process_state(engine_pid);
    let file_len =
        std::fs::metadata(downloads.path().join(&after.display_name)).map_or(0, |m| m.len());
    println!(
        "engine killed: phase {:?}, error {:?}, file {} B of {}, engine process {engine_state}",
        after.phase,
        after.error.as_ref().map(|e| (e.kind, e.detail.clone())),
        file_len,
        body.len()
    );
    assert_eq!(
        after.phase,
        Phase::Failed,
        "never Completed after engine loss"
    );
    assert_eq!(after.error.unwrap().kind, ErrorKind::ServiceUnavailable);
    assert!(file_len < body.len() as u64, "no false completion");
    assert!(!pid_alive(engine_pid));

    client.call(methods::SHUTDOWN, json!({})).await.unwrap();
    assert_eq!(wait_exit(&mut service, Duration::from_secs(20)), Some(0));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs aria2c; run via `pnpm verify` (group aria2)"]
async fn service_killed_during_a_transfer_leaves_no_orphan_and_no_false_state() {
    let aria2 = aria2().expect("aria2c must be available");
    let body = payload(6 * MIB);
    let addr = fixture(body.clone()).await;
    let data = tempfile::tempdir().unwrap();
    let downloads = tempfile::tempdir().unwrap();
    let mut service = spawn(data.path(), downloads.path(), &aria2);
    let mut client = wait_for_endpoint(data.path()).await;
    let engine_pid = *child_pids(service.id())
        .first()
        .expect("aria2c child of dm-service");
    let snap: DownloadSnapshot = serde_json::from_value(
        client
            .call(
                methods::ADD,
                add_params("process-kill-2", &format!("http://{addr}/slow/s.bin"), None),
            )
            .await
            .unwrap(),
    )
    .unwrap();
    tokio::time::sleep(Duration::from_millis(1500)).await;
    service.kill().unwrap();
    let _ = service.wait();

    // aria2 runs with --stop-with-process=<service pid>: it must exit on its own.
    let started = Instant::now();
    while pid_alive(engine_pid) && started.elapsed() < Duration::from_secs(15) {
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    let part = downloads.path().join(&snap.display_name);
    let file_len = std::fs::metadata(&part).map_or(0, |m| m.len());
    let control = downloads
        .path()
        .join(format!("{}.aria2", snap.display_name))
        .exists();
    println!(
        "service killed: engine alive {}, partial {} B, control file {control}",
        pid_alive(engine_pid),
        file_len
    );
    assert!(
        !pid_alive(engine_pid),
        "no orphan engine after the service dies"
    );
    assert!(
        file_len < body.len() as u64 || control,
        "no complete-looking file without its control file"
    );

    // Restart (B1.1): jobs and idempotency records are durable. The original job comes back
    // under the same ID as an explicit interruption, never as transferring or (falsely)
    // completed; continuing it is B1.2. The guard reaps the restarted service if anything below
    // fails.
    let mut restarted = spawn(data.path(), downloads.path(), &aria2);
    let mut client = wait_for_endpoint(data.path()).await;
    let list: ListDownloadsResult =
        serde_json::from_value(client.call(methods::LIST, json!({})).await.unwrap()).unwrap();
    println!(
        "after restart: {} job(s): {:?}",
        list.items.len(),
        list.items.iter().map(|j| j.phase).collect::<Vec<_>>()
    );
    assert_eq!(list.items.len(), 1, "exactly the original job survives");
    let job = &list.items[0];
    assert_eq!(job.job_id, snap.job_id);
    assert_eq!(job.phase, Phase::Failed);
    assert_ne!(job.phase, Phase::Completed);
    assert_eq!(
        job.error.as_ref().and_then(|e| e.detail.as_deref()),
        Some(dm_service::core::INTERRUPTED)
    );

    // The original add, repeated with its key (a retry after the lost acknowledgement), returns
    // that job; changing the payload under the same key still conflicts.
    let url = format!("http://{addr}/slow/s.bin");
    let again: DownloadSnapshot = serde_json::from_value(
        client
            .call(methods::ADD, add_params("process-kill-2", &url, None))
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        again.job_id, snap.job_id,
        "same key → same job after a restart"
    );
    let conflict = client
        .call(
            methods::ADD,
            add_params("process-kill-2", &format!("{url}?changed"), None),
        )
        .await
        .unwrap_err();
    let dm_platform::ipc::CallError::Rpc(rpc) = conflict else {
        panic!("expected an RPC error, got {conflict:?}");
    };
    assert_eq!(rpc.data.unwrap().kind, ErrorKind::IdempotencyConflict);
    let list: ListDownloadsResult =
        serde_json::from_value(client.call(methods::LIST, json!({})).await.unwrap()).unwrap();
    assert_eq!(list.items.len(), 1, "neither call created a job");

    client.call(methods::SHUTDOWN, json!({})).await.unwrap();
    assert_eq!(wait_exit(&mut restarted, Duration::from_secs(20)), Some(0));
}

/// Fixture whose first request per path fails: `/busy/…` answers 503 + Retry-After: 1,
/// `/cut/…` closes the connection halfway through. Later requests get the full body. Returns
/// the request heads seen, for checking `Range` on the retry.
async fn flaky_fixture(
    body: Vec<u8>,
) -> (
    std::net::SocketAddr,
    std::sync::Arc<std::sync::Mutex<Vec<String>>>,
) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let body = std::sync::Arc::new(body);
    let log = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let seen = log.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = listener.accept().await else {
                return;
            };
            let (body, log) = (body.clone(), seen.clone());
            tokio::spawn(async move {
                let mut head = Vec::new();
                let mut b = [0u8; 1];
                while !head.ends_with(b"\r\n\r\n") {
                    if s.read(&mut b).await.unwrap_or(0) == 0 {
                        return;
                    }
                    head.push(b[0]);
                }
                let head = String::from_utf8_lossy(&head).into_owned();
                let path = head
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or_default()
                    .to_owned();
                let first = {
                    let mut log = log.lock().unwrap();
                    let first = !log
                        .iter()
                        .any(|h| h.split_whitespace().nth(1) == Some(&path));
                    log.push(head);
                    first
                };
                if first && path.starts_with("/busy/") {
                    let _ = s
                        .write_all(b"HTTP/1.1 503 Service Unavailable\r\nRetry-After: 1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                        .await;
                    return;
                }
                let hdr = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                if s.write_all(hdr.as_bytes()).await.is_err() {
                    return;
                }
                let limit = if first && path.starts_with("/cut/") {
                    body.len() / 2
                } else {
                    body.len()
                };
                let _ = s.write_all(&body[..limit]).await;
                let _ = s.shutdown().await;
            });
        }
    });
    (addr, log)
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs aria2c; run via `pnpm verify` (group aria2)"]
async fn transient_failures_are_retried_by_the_service_from_zero() {
    let aria2 = aria2().expect("aria2c must be available");
    let body = payload(2 * MIB);
    let (addr, log) = flaky_fixture(body.clone()).await;
    let data = tempfile::tempdir().unwrap();
    let downloads = tempfile::tempdir().unwrap();
    let mut service = spawn(data.path(), downloads.path(), &aria2);
    let mut client = wait_for_endpoint(data.path()).await;

    for (key, path) in [("process-retry-503", "busy"), ("process-retry-cut", "cut")] {
        let url = format!("http://{addr}/{path}/data.bin");
        let snap: DownloadSnapshot = serde_json::from_value(
            client
                .call(methods::ADD, add_params(key, &url, Some(&sha_hex(&body))))
                .await
                .unwrap(),
        )
        .unwrap();
        let first_attempt = snap.attempt_id.clone();
        let done = wait_terminal(&mut client, snap.job_id.as_str(), Duration::from_secs(60)).await;
        let file = std::fs::read(downloads.path().join(&done.display_name)).unwrap_or_default();
        let requests: Vec<String> = log
            .lock()
            .unwrap()
            .iter()
            .filter(|h| h.contains(&format!("/{path}/")))
            .cloned()
            .collect();
        let ranged = requests
            .iter()
            .filter(|h| h.to_ascii_lowercase().contains("\nrange:"))
            .count();
        println!(
            "retry {path}: phase {:?}, {} requests, {ranged} with Range, sha256 match {}",
            done.phase,
            requests.len(),
            sha_hex(&file) == sha_hex(&body)
        );
        assert_eq!(done.phase, Phase::Completed, "{path}: {:?}", done.error);
        assert_eq!(sha_hex(&file), sha_hex(&body));
        assert_eq!(requests.len(), 2, "{path}: one failed attempt, one retry");
        assert_eq!(
            ranged, 0,
            "{path}: the retry starts from zero, never a validator-less resume"
        );
        assert_ne!(done.attempt_id, first_attempt, "{path}: a new attempt ID");
        assert!(
            !downloads
                .path()
                .join(format!("{}.aria2", done.display_name))
                .exists()
        );
    }

    client.call(methods::SHUTDOWN, json!({})).await.unwrap();
    assert_eq!(wait_exit(&mut service, Duration::from_secs(20)), Some(0));
}
