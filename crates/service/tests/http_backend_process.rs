//! The real `dm-service --backend http` process (B0.4.2, ADR 0004) against the shared controlled
//! fixtures. Needs no external engine binary, so it runs in plain `cargo test` on every OS.

#![allow(
    clippy::unwrap_used,
    clippy::print_stdout,
    clippy::arithmetic_side_effects
)]

use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use dm_domain::api::{DownloadSnapshot, HelloResult, methods};
use dm_domain::capability::support_of;
use dm_domain::{Capability, CapabilitySupport, ErrorKind, Phase};
use dm_platform::ipc::Client;
use dm_test_fixtures::{Route, Scripted, Server, payload, tls};
use serde_json::json;
use sha2::{Digest, Sha256};

const SERVICE: &str = env!("CARGO_BIN_EXE_dm-service");
const MIB: usize = 1024 * 1024;
const SECRET: &str = "SYNTHETIC-SECRET-b042-svc";

/// Kills the service if the test panics, so a failed assertion cannot leave it running (and keep
/// the test harness's pipes open).
struct Service(Child);

impl Drop for Service {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn spawn_http(data: &Path, downloads: &Path) -> Child {
    Command::new(SERVICE)
        .args(["--backend", "http", "--data-dir"])
        .arg(data)
        .arg("--download-dir")
        .arg(downloads)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}

async fn connect(data: &Path) -> Client {
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

fn sha_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

async fn add(client: &mut Client, key: &str, url: &str, sha: Option<&str>) -> DownloadSnapshot {
    serde_json::from_value(
        client
            .call(
                methods::ADD,
                json!({
                    "idempotency_key": key,
                    "source": {"url": url},
                    "destination_profile_id": "downloads",
                    "expected_size_bytes": null,
                    "expected_sha256": sha
                }),
            )
            .await
            .unwrap(),
    )
    .unwrap()
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

async fn wait_terminal(client: &mut Client, id: &str) -> DownloadSnapshot {
    let started = Instant::now();
    loop {
        let s = get(client, id).await;
        if s.phase.is_terminal() || started.elapsed() > Duration::from_secs(60) {
            return s;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn http_backend_service_end_to_end() {
    let data = tempfile::tempdir().unwrap();
    let downloads = tempfile::tempdir().unwrap();
    let mut service = Service(spawn_http(data.path(), downloads.path()));
    let mut client = connect(data.path()).await;

    // Backend selection and the honest capability report.
    let hello: HelloResult =
        serde_json::from_value(client.call(methods::HELLO, json!({})).await.unwrap()).unwrap();
    let caps = &hello.backend.capabilities;
    println!(
        "hello: backend {} {:?}",
        hello.backend.id, hello.backend.version
    );
    assert_eq!(hello.backend.id, "http");
    assert_eq!(
        support_of(caps, Capability::Http),
        CapabilitySupport::Supported
    );
    assert_eq!(
        support_of(caps, Capability::Https),
        CapabilitySupport::Supported
    );
    assert_eq!(
        support_of(caps, Capability::SegmentedHttp),
        CapabilitySupport::Supported
    );
    assert_eq!(
        support_of(caps, Capability::ResumeStrongValidator),
        CapabilitySupport::Supported
    );

    let s = Server::start().await;
    let body = payload(3 * MIB);

    // 1. A plain download with a trusted hash; the token never appears in the snapshot.
    s.set("plain", Route::new(body.clone()));
    let snap = add(
        &mut client,
        "http-svc-1",
        &format!("{}?token={SECRET}", s.url("plain")),
        Some(&sha_hex(&body)),
    )
    .await;
    assert!(!snap.source_display.contains(SECRET));
    let done = wait_terminal(&mut client, snap.job_id.as_str()).await;
    let file = std::fs::read(downloads.path().join(&done.display_name)).unwrap();
    println!(
        "plain: phase {:?}, {} bytes, sha256 {}",
        done.phase,
        file.len(),
        sha_hex(&file)
    );
    assert_eq!(done.phase, Phase::Completed);
    assert_eq!(sha_hex(&file), sha_hex(&body));
    // SegmentedHttp is supported: the service asked for 4 connections; 3 MiB → 3 validated segments.
    let reqs = s.log_for("plain");
    let ranged: Vec<_> = reqs
        .iter()
        .filter(|l| l.header("range").is_some())
        .collect();
    println!("plain: {} requests, {} ranged", reqs.len(), ranged.len());
    assert_eq!(ranged.len(), 3);
    assert!(
        ranged
            .iter()
            .all(|l| l.header("if-range") == Some("\"v1\""))
    );

    // 2. 503 with Retry-After: 1 once: the service waits at least that long, retries from zero.
    let mut busy = Route::new(body.clone());
    busy.script
        .push_back(Scripted::status(503, "Service Unavailable").header("Retry-After", "1"));
    s.set("busy", busy);
    let started = Instant::now();
    let snap = add(
        &mut client,
        "http-svc-2",
        &s.url("busy"),
        Some(&sha_hex(&body)),
    )
    .await;
    let done = wait_terminal(&mut client, snap.job_id.as_str()).await;
    let reqs = s.log_for("busy");
    println!(
        "busy: phase {:?}, {} requests, {:?} elapsed, ranged {}",
        done.phase,
        reqs.len(),
        started.elapsed(),
        reqs.iter().any(|l| l.header("range").is_some())
    );
    assert_eq!(done.phase, Phase::Completed);
    assert_eq!(
        sha_hex(&std::fs::read(downloads.path().join(&done.display_name)).unwrap()),
        sha_hex(&body)
    );
    assert!(
        started.elapsed() >= Duration::from_secs(1),
        "Retry-After honoured"
    );
    // The 503 answered the first probe; the retry probed again and then fetched validated segments.
    assert!(reqs[0].header("range").is_none() && reqs[1].header("range").is_none());
    assert!(
        reqs.iter()
            .filter(|l| l.header("range").is_some())
            .all(|l| l.header("if-range").is_some())
    );

    // 3. Connection cut halfway once (1.5 MiB: below the segmentation threshold, one connection):
    //    a fresh attempt from zero, never a resume.
    let body = payload(MIB + MIB / 2);
    let mut cut = Route::new(body.clone());
    cut.truncate_after = Some(MIB);
    cut.truncate_times = 1;
    s.set("cut", cut);
    let snap = add(
        &mut client,
        "http-svc-3",
        &s.url("cut"),
        Some(&sha_hex(&body)),
    )
    .await;
    let done = wait_terminal(&mut client, snap.job_id.as_str()).await;
    let reqs = s.log_for("cut");
    println!("cut: phase {:?}, {} requests", done.phase, reqs.len());
    assert_eq!(done.phase, Phase::Completed);
    assert_eq!(reqs.len(), 2);
    assert!(reqs.iter().all(|l| l.header("range").is_none()));
    assert_eq!(
        sha_hex(&std::fs::read(downloads.path().join(&done.display_name)).unwrap()),
        sha_hex(&body)
    );

    // 4. HTTPS is accepted by the gate; an untrusted certificate fails honestly (production trust,
    //    no extra roots), never completes.
    let tls_server = Server::start_tls(tls::self_signed(&["localhost", "127.0.0.1"])).await;
    tls_server.set("f", Route::new(payload(1024)));
    let snap = add(&mut client, "http-svc-4", &tls_server.url("f"), None).await;
    let done = wait_terminal(&mut client, snap.job_id.as_str()).await;
    println!(
        "untrusted https: phase {:?}, error {:?}",
        done.phase,
        done.error.as_ref().map(|e| e.kind)
    );
    assert_eq!(done.phase, Phase::Failed);
    assert_eq!(done.error.unwrap().kind, ErrorKind::TlsHandshakeFailed);
    assert!(tls_server.log_for("f").is_empty());

    // 5. Cancel during a slow transfer: acknowledged after the backend stopped writing.
    let mut slow = Route::new(payload(8 * MIB));
    slow.slow = Some(Duration::from_millis(10));
    s.set("slow", slow);
    let snap = add(&mut client, "http-svc-5", &s.url("slow"), None).await;
    let started = Instant::now();
    while get(&mut client, snap.job_id.as_str())
        .await
        .downloaded_bytes
        .0
        == 0
    {
        assert!(started.elapsed() < Duration::from_secs(20));
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let cancelled: DownloadSnapshot = serde_json::from_value(
        client
            .call(methods::CANCEL, json!({"job_id": snap.job_id}))
            .await
            .unwrap(),
    )
    .unwrap();
    let part = downloads
        .path()
        .join(format!("{}.downova-part", snap.display_name));
    // Segmented: the partial file is preallocated, so compare contents, not sizes.
    let at_ack = std::fs::read(&part).unwrap_or_default();
    tokio::time::sleep(Duration::from_millis(300)).await;
    let later = std::fs::read(&part).unwrap_or_default();
    println!(
        "cancel: phase {:?}, partial unchanged after ack {}",
        cancelled.phase,
        at_ack == later
    );
    assert_eq!(cancelled.phase, Phase::Cancelled);
    assert!(at_ack == later, "no writes after cancel was acknowledged");
    assert!(!downloads.path().join(&snap.display_name).exists());

    // 6. Validated pause/resume through the service (B0.4.2, second increment).
    let resumed_body = dm_test_fixtures::payload_seeded(6 * MIB, 31);
    let mut slow = Route::new(resumed_body.clone());
    slow.slow = Some(Duration::from_millis(5));
    s.set("resumable", slow);
    let snap = add(
        &mut client,
        "http-svc-6",
        &s.url("resumable"),
        Some(&sha_hex(&resumed_body)),
    )
    .await;
    let id = snap.job_id.clone();
    let started = Instant::now();
    while get(&mut client, id.as_str()).await.downloaded_bytes.0 < 512 * 1024 {
        assert!(started.elapsed() < Duration::from_secs(20));
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let paused: DownloadSnapshot = serde_json::from_value(
        client
            .call(methods::PAUSE, json!({"job_id": id}))
            .await
            .unwrap(),
    )
    .unwrap();
    let part = downloads
        .path()
        .join(format!("{}.downova-part", snap.display_name));
    // Segmented and preallocated: compare contents; progress is the confirmed byte count.
    let at_pause_bytes = std::fs::read(&part).unwrap();
    let at_pause = paused.downloaded_bytes.0;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        std::fs::read(&part).unwrap() == at_pause_bytes,
        "paused: no writes"
    );
    assert!(at_pause > 0 && at_pause < resumed_body.len() as u64);
    s.update("resumable", |r| r.slow = None);
    client
        .call(methods::RESUME, json!({"job_id": id}))
        .await
        .unwrap();
    let done = wait_terminal(&mut client, id.as_str()).await;
    let file = std::fs::read(downloads.path().join(&done.display_name)).unwrap();
    let reqs = s.log_for("resumable");
    let ranged: Vec<_> = reqs.iter().filter_map(|l| l.header("range")).collect();
    println!(
        "pause/resume: paused {:?} at {at_pause} B, phase {:?}, {} requests, ranges {ranged:?}, sha match {}",
        paused.desired_state,
        done.phase,
        reqs.len(),
        sha_hex(&file) == sha_hex(&resumed_body)
    );
    assert_eq!(done.phase, Phase::Completed);
    assert_eq!(sha_hex(&file), sha_hex(&resumed_body));
    // 6 MiB with 4 connections: 4 segments of 1.5 MiB; after the resume, continuation ranges
    // start inside segments (not on a segment boundary).
    let boundaries = [
        "bytes=0-1572863",
        "bytes=1572864-3145727",
        "bytes=3145728-4718591",
        "bytes=4718592-6291455",
    ];
    let continuations: Vec<_> = ranged.iter().filter(|r| !boundaries.contains(r)).collect();
    assert!(!continuations.is_empty(), "a real continuation: {ranged:?}");
    assert!(
        reqs.iter()
            .filter(|l| l.header("range").is_some())
            .all(|l| l.header("if-range").is_some())
    );

    // 7. Weak ETag: the resume is refused explicitly and the job stays paused.
    let mut weak = Route::new(dm_test_fixtures::payload_seeded(6 * MIB, 32));
    weak.etag = Some("W/\"w1\"".into());
    weak.slow = Some(Duration::from_millis(5));
    s.set("weak", weak);
    let snap = add(&mut client, "http-svc-7", &s.url("weak"), None).await;
    let id = snap.job_id.clone();
    let started = Instant::now();
    while get(&mut client, id.as_str()).await.downloaded_bytes.0 == 0 {
        assert!(started.elapsed() < Duration::from_secs(20));
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    client
        .call(methods::PAUSE, json!({"job_id": id}))
        .await
        .unwrap();
    let refused = client
        .call(methods::RESUME, json!({"job_id": id}))
        .await
        .unwrap_err();
    let after = get(&mut client, id.as_str()).await;
    println!(
        "weak ETag resume: {refused}; desired {:?}",
        after.desired_state
    );
    assert_eq!(refused.kind(), ErrorKind::BackendCapabilityUnavailable);
    assert_eq!(after.desired_state, dm_domain::DesiredState::Paused);
    assert_eq!(s.log_for("weak").len(), 1, "no resume request sent");

    client.call(methods::SHUTDOWN, json!({})).await.unwrap();
    assert_eq!(wait_exit(&mut service.0, Duration::from_secs(20)), Some(0));
}

#[test]
fn backend_flags_are_validated_with_clear_errors() {
    for args in [
        ["--backend", "curl"].as_slice(),
        ["--backend"].as_slice(),
        ["--backend", "http", "--aria2", "x"].as_slice(),
        ["--aria2", "x"].as_slice(),
        ["--backend", "http", "--backend", "aria2"].as_slice(),
        ["--backend", "aria2", "--backend", "aria2"].as_slice(),
    ] {
        let data = tempfile::tempdir().unwrap();
        let out = Command::new(SERVICE)
            .args(args)
            .arg("--data-dir")
            .arg(data.path())
            .output()
            .unwrap();
        let err = String::from_utf8_lossy(&out.stderr);
        println!(
            "{args:?} → exit {:?}: {}",
            out.status.code(),
            err.lines().next().unwrap_or("")
        );
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        assert!(
            err.contains("usage: dm-service"),
            "{args:?}: the error shows the usage"
        );
    }
}

/// A process environment in which no aria2c can be found (no PATH entries, no override, a
/// working directory without tools/aria2).
fn without_aria2(cmd: &mut Command, cwd: &Path) {
    cmd.env_remove("DM_ARIA2_PATH")
        .env("PATH", cwd)
        .current_dir(cwd);
}

#[test]
fn explicit_aria2_without_an_executable_fails_and_never_falls_back() {
    let data = tempfile::tempdir().unwrap();
    let mut cmd = Command::new(SERVICE);
    cmd.args(["--backend", "aria2", "--data-dir"])
        .arg(data.path());
    without_aria2(&mut cmd, data.path());
    let out = cmd.output().unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    println!(
        "--backend aria2 without aria2c → exit {:?}: {}",
        out.status.code(),
        err.trim()
    );
    assert_eq!(
        out.status.code(),
        Some(4),
        "startup failure, not a silent switch to http"
    );
    assert!(err.contains("aria2c unavailable"));
}

#[tokio::test(flavor = "multi_thread")]
async fn default_backend_is_http_and_needs_no_aria2() {
    let data = tempfile::tempdir().unwrap();
    let downloads = tempfile::tempdir().unwrap();
    let mut cmd = Command::new(SERVICE);
    cmd.arg("--data-dir")
        .arg(data.path())
        .arg("--download-dir")
        .arg(downloads.path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    without_aria2(&mut cmd, data.path());
    let mut service = Service(cmd.spawn().unwrap());
    let mut client = connect(data.path()).await;
    let hello: HelloResult =
        serde_json::from_value(client.call(methods::HELLO, json!({})).await.unwrap()).unwrap();
    println!(
        "default backend: {} {:?}",
        hello.backend.id, hello.backend.version
    );
    assert_eq!(hello.backend.id, "http");

    let s = Server::start().await;
    let body = dm_test_fixtures::payload_seeded(5 * MIB, 51);
    s.set("default.bin", Route::new(body.clone()));
    let snap = add(
        &mut client,
        "default-backend-1",
        &s.url("default.bin"),
        Some(&sha_hex(&body)),
    )
    .await;
    let done = wait_terminal(&mut client, snap.job_id.as_str()).await;
    let file = std::fs::read(downloads.path().join(&done.display_name)).unwrap();
    let ranged = s
        .log_for("default.bin")
        .iter()
        .filter(|l| l.header("range").is_some())
        .count();
    println!(
        "default backend download: phase {:?}, sha256 {}, {ranged} validated segments",
        done.phase,
        sha_hex(&file)
    );
    assert_eq!(done.phase, Phase::Completed);
    assert_eq!(sha_hex(&file), sha_hex(&body));
    assert!(ranged >= 2, "the default backend segments large files");

    client.call(methods::SHUTDOWN, json!({})).await.unwrap();
    assert_eq!(wait_exit(&mut service.0, Duration::from_secs(20)), Some(0));
}

/// B1.1 (COR-09, CORE-04 first step): jobs and idempotency records survive a hard kill of the
/// real service. Same key after the restart → same job and no second row; a different payload →
/// conflict; the unfinished job is reported as interrupted; the completed one keeps its state.
#[tokio::test]
async fn jobs_and_idempotency_survive_a_service_kill() {
    let data = tempfile::tempdir().unwrap();
    let downloads = tempfile::tempdir().unwrap();
    let fixture = Server::start().await;
    let small = payload(256 * 1024);
    fixture.set("done.bin", Route::new(small.clone()));
    let mut slow = Route::new(payload(8 * MIB));
    slow.slow = Some(Duration::from_millis(40));
    fixture.set("slow.bin", slow);
    let (done_url, slow_url) = (fixture.url("done.bin"), fixture.url("slow.bin"));

    let mut service = Service(spawn_http(data.path(), downloads.path()));
    let mut client = connect(data.path()).await;
    let done = add(&mut client, "ext-kill-done-01", &done_url, None).await;
    assert_eq!(
        wait_terminal(&mut client, done.job_id.as_str()).await.phase,
        Phase::Completed
    );
    let running = add(&mut client, "ext-kill-slow-01", &slow_url, None).await;
    // Hard kill (no shutdown request) while the slow transfer is still under way.
    service.0.kill().unwrap();
    service.0.wait().unwrap();
    drop(client);

    let _service = Service(spawn_http(data.path(), downloads.path()));
    let mut client = connect(data.path()).await;
    let again = add(&mut client, "ext-kill-slow-01", &slow_url, None).await;
    assert_eq!(
        again.job_id, running.job_id,
        "same key after a restart → same job"
    );
    let list = client.call(methods::LIST, json!({})).await.unwrap();
    assert_eq!(
        list["items"].as_array().unwrap().len(),
        2,
        "no second job: {list}"
    );
    let conflict = client
        .call(
            methods::ADD,
            json!({
                "idempotency_key": "ext-kill-slow-01",
                "source": {"url": format!("{slow_url}?other")},
                "destination_profile_id": "downloads",
                "expected_size_bytes": null,
                "expected_sha256": null
            }),
        )
        .await
        .unwrap_err();
    let dm_platform::ipc::CallError::Rpc(rpc) = conflict else {
        panic!("expected an RPC error, got {conflict:?}");
    };
    assert_eq!(rpc.data.unwrap().kind, ErrorKind::IdempotencyConflict);

    let interrupted = get(&mut client, running.job_id.as_str()).await;
    assert_eq!(interrupted.phase, Phase::Failed);
    assert_eq!(
        interrupted.error.unwrap().detail.as_deref(),
        Some(dm_service::core::INTERRUPTED)
    );
    let kept = get(&mut client, done.job_id.as_str()).await;
    assert_eq!(kept.phase, Phase::Completed);
    let file = std::fs::read(downloads.path().join(&kept.display_name)).unwrap();
    assert_eq!(sha_hex(&file), sha_hex(&small));
    println!(
        "kill/restart: same job {} after restart; interrupted job reported; completed job kept",
        again.job_id
    );
}

/// The database is the service's alone: a second service for the same data directory exits
/// (instance lock), and the database file is not opened by anything else meanwhile.
#[tokio::test]
async fn the_database_is_created_in_the_data_directory() {
    let data = tempfile::tempdir().unwrap();
    let downloads = tempfile::tempdir().unwrap();
    let _service = Service(spawn_http(data.path(), downloads.path()));
    let _client = connect(data.path()).await;
    assert!(data.path().join(dm_storage::FILE_NAME).exists());
    assert!(dm_storage::Store::open(&data.path().join(dm_storage::FILE_NAME)).is_err());
}

// ------------------------------------------------------------------ B1.2 crash recovery

/// A traced service: committed checkpoints and recovery outcomes are appended to `trace`
/// (`DOWNOVA_CHECKPOINT_TRACE`), the deterministic barrier these tests wait on.
fn spawn_traced(data: &Path, downloads: &Path, trace: &Path) -> Service {
    Service(
        Command::new(SERVICE)
            .args(["--backend", "http", "--data-dir"])
            .arg(data)
            .arg("--download-dir")
            .arg(downloads)
            .env("DOWNOVA_CHECKPOINT_TRACE", trace)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    )
}

fn trace_events(trace: &Path) -> Vec<serde_json::Value> {
    std::fs::read_to_string(trace)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

/// Waits for a committed checkpoint of `job` covering more than zero bytes (and more than
/// `above`); returns (attempt, confirmed).
async fn wait_checkpoint(trace: &Path, job: &str, above: u64) -> (String, u64) {
    let started = Instant::now();
    loop {
        if let Some(e) = trace_events(trace).into_iter().rev().find(|e| {
            e["event"] == "checkpoint"
                && e["job_id"] == job
                && e["confirmed"].as_u64().is_some_and(|c| c > above)
        }) {
            return (
                e["attempt_id"].as_str().unwrap().to_owned(),
                e["confirmed"].as_u64().unwrap(),
            );
        }
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "no checkpoint for {job}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn last_event<'a>(events: &'a [serde_json::Value], kind: &str, job: &str) -> &'a serde_json::Value {
    events
        .iter()
        .rev()
        .find(|e| e["event"] == kind && e["job_id"] == job)
        .unwrap_or_else(|| panic!("no {kind} event for {job}: {events:?}"))
}

fn hard_kill(service: &mut Service) {
    service.0.kill().unwrap();
    service.0.wait().unwrap();
}

fn slow_route(body: &[u8], etag: &str, delay_ms: u64) -> Route {
    let mut r = Route::new(body.to_vec());
    r.etag = Some(etag.into());
    r.slow = Some(Duration::from_millis(delay_ms));
    r
}

fn ranges_since(fixture: &Server, path: &str, from: usize) -> Vec<String> {
    fixture
        .log_for(path)
        .iter()
        .skip(from)
        .filter_map(|l| l.header("range").map(str::to_owned))
        .collect()
}

/// K01: hard kill after a nonzero checkpoint (single connection); the restarted service continues
/// from the checkpoint (not from the end of the partial file, which may hold unconfirmed bytes
/// written after it) and the file's SHA-256 matches. Same key afterwards → same job.
#[tokio::test]
async fn k01_single_connection_recovers_after_a_hard_kill() {
    let (data, downloads, trace_dir) = (
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
    );
    let trace = trace_dir.path().join("trace.jsonl");
    let fixture = Server::start().await;
    let body = payload(3 * MIB / 2);
    fixture.set("k1.bin", slow_route(&body, "\"k1\"", 40));
    let url = fixture.url("k1.bin");

    let mut service = spawn_traced(data.path(), downloads.path(), &trace);
    let mut client = connect(data.path()).await;
    let job = add(&mut client, "ext-k01-key-0001", &url, None).await;
    let (old_attempt, _) = wait_checkpoint(&trace, job.job_id.as_str(), 0).await;
    let part = downloads
        .path()
        .join(format!("{}.downova-part", job.display_name));
    // Barrier for "crash between a data write and the metadata commit": the backend has written
    // (and synced) bytes the service has not committed yet. Kill exactly then.
    let committed = |trace: &Path| {
        last_event(&trace_events(trace), "checkpoint", job.job_id.as_str())["confirmed"]
            .as_u64()
            .unwrap()
    };
    let started = Instant::now();
    while std::fs::metadata(&part).unwrap().len() <= committed(&trace) {
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "file never ran ahead"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    hard_kill(&mut service);
    drop(client);
    let confirmed = committed(&trace);
    let on_disk = std::fs::metadata(&part).unwrap().len();
    assert!(
        on_disk > confirmed,
        "bytes past the committed checkpoint: {on_disk} vs {confirmed}"
    );
    let seen = fixture.log_for("k1.bin").len();

    let _service = spawn_traced(data.path(), downloads.path(), &trace);
    let mut client = connect(data.path()).await;
    let done = wait_terminal(&mut client, job.job_id.as_str()).await;
    assert_eq!(done.phase, Phase::Completed, "{:?}", done.error);
    let file = std::fs::read(downloads.path().join(&done.display_name)).unwrap();
    assert_eq!(sha_hex(&file), sha_hex(&body));
    let events = trace_events(&trace);
    let recovered = last_event(&events, "recovered", job.job_id.as_str());
    assert_eq!(recovered["confirmed"].as_u64(), Some(confirmed));
    assert_ne!(recovered["attempt_id"].as_str(), Some(old_attempt.as_str()));
    let ranges = ranges_since(&fixture, "k1.bin", seen);
    assert_eq!(
        ranges.first(),
        Some(&format!("bytes={confirmed}-")),
        "{ranges:?}"
    );
    let again = add(&mut client, "ext-k01-key-0001", &url, None).await;
    assert_eq!(again.job_id, job.job_id);
    let list = client.call(methods::LIST, json!({})).await.unwrap();
    assert_eq!(list["items"].as_array().unwrap().len(), 1);
    println!(
        "K01 checkpoint {confirmed} B, partial on disk {on_disk} B at the kill, continued with {ranges:?}"
    );
}

/// K02: the same for a segmented transfer: every segment continues from its own checkpoint.
#[tokio::test]
async fn k02_segmented_recovers_after_a_hard_kill() {
    let (data, downloads, trace_dir) = (
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
    );
    let trace = trace_dir.path().join("trace.jsonl");
    let fixture = Server::start().await;
    let body = payload(6 * MIB);
    fixture.set("k2.bin", slow_route(&body, "\"k2\"", 40));
    let url = fixture.url("k2.bin");

    let mut service = spawn_traced(data.path(), downloads.path(), &trace);
    let mut client = connect(data.path()).await;
    let job = add(&mut client, "ext-k02-key-0001", &url, None).await;
    let (_, confirmed) = wait_checkpoint(&trace, job.job_id.as_str(), 0).await;
    hard_kill(&mut service);
    drop(client);
    let seen = fixture.log_for("k2.bin").len();

    let _service = spawn_traced(data.path(), downloads.path(), &trace);
    let mut client = connect(data.path()).await;
    let done = wait_terminal(&mut client, job.job_id.as_str()).await;
    assert_eq!(done.phase, Phase::Completed, "{:?}", done.error);
    let file = std::fs::read(downloads.path().join(&done.display_name)).unwrap();
    assert_eq!(sha_hex(&file), sha_hex(&body));
    let ranges = ranges_since(&fixture, "k2.bin", seen);
    let starts: Vec<u64> = (0..4u64).map(|i| i * (6 * MIB as u64) / 4).collect();
    assert!(
        ranges.iter().any(|r| {
            let first: u64 = r["bytes=".len()..]
                .split('-')
                .next()
                .unwrap()
                .parse()
                .unwrap();
            !starts.contains(&first)
        }),
        "continued inside a segment: {ranges:?}"
    );
    println!("K02 checkpoint {confirmed} B, continued with {ranges:?}");
}

/// K03: a job the user paused stays paused after a kill and restart (no request is sent), and
/// resuming it later continues from the paused checkpoint.
#[tokio::test]
async fn k03_a_paused_job_stays_paused_after_a_kill() {
    let (data, downloads, trace_dir) = (
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
    );
    let trace = trace_dir.path().join("trace.jsonl");
    let fixture = Server::start().await;
    let body = payload(3 * MIB / 2);
    fixture.set("k3.bin", slow_route(&body, "\"k3\"", 40));
    let url = fixture.url("k3.bin");

    let mut service = spawn_traced(data.path(), downloads.path(), &trace);
    let mut client = connect(data.path()).await;
    let job = add(&mut client, "ext-k03-key-0001", &url, None).await;
    let (_, first) = wait_checkpoint(&trace, job.job_id.as_str(), 0).await;
    client
        .call(methods::PAUSE, json!({"job_id": job.job_id.as_str()}))
        .await
        .unwrap();
    // The pause commits its own checkpoint (bytes synced at the pause).
    let (_, paused_at) = wait_checkpoint(&trace, job.job_id.as_str(), first).await;
    hard_kill(&mut service);
    drop(client);
    let seen = fixture.log_for("k3.bin").len();

    let _service = spawn_traced(data.path(), downloads.path(), &trace);
    let mut client = connect(data.path()).await;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let snap = get(&mut client, job.job_id.as_str()).await;
    assert_eq!(snap.desired_state, dm_domain::DesiredState::Paused);
    assert!(!snap.phase.is_terminal(), "{:?}", snap.phase);
    assert_eq!(snap.downloaded_bytes.0, paused_at);
    assert_eq!(
        fixture.log_for("k3.bin").len(),
        seen,
        "nothing sent while paused"
    );
    client
        .call(methods::RESUME, json!({"job_id": job.job_id.as_str()}))
        .await
        .unwrap();
    let done = wait_terminal(&mut client, job.job_id.as_str()).await;
    assert_eq!(done.phase, Phase::Completed, "{:?}", done.error);
    let file = std::fs::read(downloads.path().join(&done.display_name)).unwrap();
    assert_eq!(sha_hex(&file), sha_hex(&body));
    let ranges = ranges_since(&fixture, "k3.bin", seen);
    assert_eq!(ranges.first(), Some(&format!("bytes={paused_at}-")));
}

/// K04: the resource changed while the service was down. The validated continuation gets a `200`
/// (If-Range mismatch), restarts from zero, and the file is the new representation, never a splice.
#[tokio::test]
async fn k04_a_changed_resource_restarts_cleanly_after_a_kill() {
    let (data, downloads, trace_dir) = (
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
    );
    let trace = trace_dir.path().join("trace.jsonl");
    let fixture = Server::start().await;
    let a = payload(3 * MIB / 2);
    let b: Vec<u8> = a.iter().map(|x| x ^ 0x5a).collect();
    fixture.set("k4.bin", slow_route(&a, "\"k4a\"", 40));
    let url = fixture.url("k4.bin");

    let mut service = spawn_traced(data.path(), downloads.path(), &trace);
    let mut client = connect(data.path()).await;
    let job = add(&mut client, "ext-k04-key-0001", &url, None).await;
    wait_checkpoint(&trace, job.job_id.as_str(), 0).await;
    hard_kill(&mut service);
    drop(client);
    let mut changed = Route::new(b.clone());
    changed.etag = Some("\"k4b\"".into());
    fixture.set("k4.bin", changed);

    let _service = spawn_traced(data.path(), downloads.path(), &trace);
    let mut client = connect(data.path()).await;
    let done = wait_terminal(&mut client, job.job_id.as_str()).await;
    assert_eq!(done.phase, Phase::Completed, "{:?}", done.error);
    let file = std::fs::read(downloads.path().join(&done.display_name)).unwrap();
    assert_eq!(sha_hex(&file), sha_hex(&b), "whole new representation");
}

/// K05: the partial file was changed while the service was down: recovery is refused with an
/// explicit reason, the partial file is left as it is, and no destination file appears.
#[tokio::test]
async fn k05_a_modified_partial_file_is_refused_after_a_kill() {
    let (data, downloads, trace_dir) = (
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
    );
    let trace = trace_dir.path().join("trace.jsonl");
    let fixture = Server::start().await;
    let body = payload(3 * MIB / 2);
    fixture.set("k5.bin", slow_route(&body, "\"k5\"", 40));
    let url = fixture.url("k5.bin");

    let mut service = spawn_traced(data.path(), downloads.path(), &trace);
    let mut client = connect(data.path()).await;
    let job = add(&mut client, "ext-k05-key-0001", &url, None).await;
    wait_checkpoint(&trace, job.job_id.as_str(), 0).await;
    hard_kill(&mut service);
    drop(client);
    let part = downloads
        .path()
        .join(format!("{}.downova-part", job.display_name));
    let mut bytes = std::fs::read(&part).unwrap();
    bytes[100] ^= 0xff;
    std::fs::write(&part, &bytes).unwrap();

    let _service = spawn_traced(data.path(), downloads.path(), &trace);
    let mut client = connect(data.path()).await;
    let snap = wait_terminal(&mut client, job.job_id.as_str()).await;
    assert_eq!(snap.phase, Phase::Failed);
    let detail = snap.error.unwrap().detail.unwrap();
    assert!(
        detail.starts_with(dm_service::core::RECOVERY_FAILED),
        "{detail}"
    );
    assert!(detail.contains("modified"), "{detail}");
    assert_eq!(
        std::fs::read(&part).unwrap(),
        bytes,
        "partial file untouched"
    );
    assert!(!downloads.path().join(&snap.display_name).exists());
}

/// K06: a stored checkpoint of an unknown format version (or otherwise unusable) is refused
/// explicitly after the restart.
#[tokio::test]
async fn k06_an_incompatible_stored_checkpoint_is_refused() {
    let (data, downloads, trace_dir) = (
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
    );
    let trace = trace_dir.path().join("trace.jsonl");
    let fixture = Server::start().await;
    let body = payload(3 * MIB / 2);
    fixture.set("k6.bin", slow_route(&body, "\"k6\"", 40));
    let url = fixture.url("k6.bin");

    let mut service = spawn_traced(data.path(), downloads.path(), &trace);
    let mut client = connect(data.path()).await;
    let job = add(&mut client, "ext-k06-key-0001", &url, None).await;
    wait_checkpoint(&trace, job.job_id.as_str(), 0).await;
    hard_kill(&mut service);
    drop(client);
    {
        // The service is down, so the database can be opened to damage the stored checkpoint.
        let mut store = dm_storage::Store::open(&data.path().join(dm_storage::FILE_NAME)).unwrap();
        let row = store
            .load_jobs()
            .unwrap()
            .into_iter()
            .find(|j| j.id == job.job_id)
            .unwrap();
        let mut cp = store.load_checkpoint(&job.job_id).unwrap().unwrap();
        cp.format_version = 99;
        store.save_checkpoint(&row, &cp).unwrap();
    }

    let _service = spawn_traced(data.path(), downloads.path(), &trace);
    let mut client = connect(data.path()).await;
    let snap = wait_terminal(&mut client, job.job_id.as_str()).await;
    assert_eq!(snap.phase, Phase::Failed);
    let error = snap.error.unwrap();
    assert_eq!(error.kind, ErrorKind::BackendCapabilityUnavailable);
    assert!(
        error
            .detail
            .unwrap()
            .starts_with(dm_service::core::RECOVERY_FAILED)
    );
}

/// K07: a graceful shutdown during a transfer checkpoints it; the next start continues it.
#[tokio::test]
async fn k07_a_graceful_shutdown_mid_transfer_continues_after_the_restart() {
    let (data, downloads, trace_dir) = (
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
    );
    let trace = trace_dir.path().join("trace.jsonl");
    let fixture = Server::start().await;
    let body = payload(3 * MIB / 2);
    fixture.set("k7.bin", slow_route(&body, "\"k7\"", 40));
    let url = fixture.url("k7.bin");

    let mut service = spawn_traced(data.path(), downloads.path(), &trace);
    let mut client = connect(data.path()).await;
    let job = add(&mut client, "ext-k07-key-0001", &url, None).await;
    let started = Instant::now();
    while get(&mut client, job.job_id.as_str())
        .await
        .downloaded_bytes
        .0
        < 64 * 1024
    {
        assert!(started.elapsed() < Duration::from_secs(20));
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    client.call(methods::SHUTDOWN, json!({})).await.unwrap();
    assert_eq!(wait_exit(&mut service.0, Duration::from_secs(20)), Some(0));
    drop(client);
    let events = trace_events(&trace);
    let at_shutdown = last_event(&events, "checkpoint", job.job_id.as_str())["confirmed"]
        .as_u64()
        .unwrap();
    let seen = fixture.log_for("k7.bin").len();

    let _service = spawn_traced(data.path(), downloads.path(), &trace);
    let mut client = connect(data.path()).await;
    let done = wait_terminal(&mut client, job.job_id.as_str()).await;
    assert_eq!(done.phase, Phase::Completed, "{:?}", done.error);
    let file = std::fs::read(downloads.path().join(&done.display_name)).unwrap();
    assert_eq!(sha_hex(&file), sha_hex(&body));
    let ranges = ranges_since(&fixture, "k7.bin", seen);
    assert_eq!(ranges.first(), Some(&format!("bytes={at_shutdown}-")));
}

// ------------------------------------------------------------------ B1.3 completion journal

/// A traced service with an optional fault point (`DOWNOVA_FAULT`, debug builds only).
fn spawn_fault(data: &Path, downloads: &Path, trace: &Path, fault: &str) -> Service {
    Service(
        Command::new(SERVICE)
            .args(["--backend", "http", "--data-dir"])
            .arg(data)
            .arg("--download-dir")
            .arg(downloads)
            .env("DOWNOVA_CHECKPOINT_TRACE", trace)
            .env("DOWNOVA_FAULT", fault)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    )
}

struct Fin {
    data: tempfile::TempDir,
    downloads: tempfile::TempDir,
    _trace_dir: tempfile::TempDir,
    trace: std::path::PathBuf,
    fixture: Server,
    body: Vec<u8>,
    url: String,
}

async fn fin_setup(name: &str) -> Fin {
    let trace_dir = tempfile::tempdir().unwrap();
    let trace = trace_dir.path().join("trace.jsonl");
    let fixture = Server::start().await;
    let body = payload(512 * 1024);
    let mut route = Route::new(body.clone());
    route.etag = Some("\"f1\"".into());
    fixture.set(name, route);
    let url = fixture.url(name);
    Fin {
        data: tempfile::tempdir().unwrap(),
        downloads: tempfile::tempdir().unwrap(),
        _trace_dir: trace_dir,
        trace,
        fixture,
        body,
        url,
    }
}

fn has_event(trace: &Path, kind: &str, job: &str) -> bool {
    trace_events(trace)
        .iter()
        .any(|e| e["event"] == kind && e["job_id"] == job)
}

/// Waits for a child to exit without blocking the runtime (the fixture server shares it).
async fn exited(child: &mut Child, limit: Duration) -> Option<i32> {
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status.code();
        }
        if started.elapsed() > limit {
            let _ = child.kill();
            let _ = child.wait();
            panic!("the service did not exit within {limit:?}");
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// Starts a service with `fault`, adds the fixture, and waits until the process died at it.
async fn crash_at(f: &Fin, key: &str, fault: &str, sha: Option<&str>) -> DownloadSnapshot {
    let mut service = spawn_fault(f.data.path(), f.downloads.path(), &f.trace, fault);
    let mut client = connect(f.data.path()).await;
    let job = add(&mut client, key, &f.url, sha).await;
    let code = exited(&mut service.0, Duration::from_secs(30)).await;
    assert!(
        code != Some(0),
        "the service should have died at {fault}: {code:?}"
    );
    job
}

/// Restarts normally and waits for the job to be terminal.
async fn restart_until_terminal(f: &Fin, job: &str) -> (Service, Client, DownloadSnapshot) {
    let service = spawn_traced(f.data.path(), f.downloads.path(), &f.trace);
    let mut client = connect(f.data.path()).await;
    let snap = wait_terminal(&mut client, job).await;
    (service, client, snap)
}

fn final_and_staged(f: &Fin, snap: &DownloadSnapshot) -> (std::path::PathBuf, std::path::PathBuf) {
    (
        f.downloads.path().join(&snap.display_name),
        f.downloads
            .path()
            .join(format!("{}.downova-part", snap.display_name)),
    )
}

/// `sha` repeats the original add exactly (same key, same payload).
async fn assert_completed_once(
    f: &Fin,
    client: &mut Client,
    snap: &DownloadSnapshot,
    key: &str,
    sha: Option<&str>,
) {
    assert_eq!(snap.phase, Phase::Completed, "{:?}", snap.error);
    let (final_path, staged) = final_and_staged(f, snap);
    assert_eq!(
        sha_hex(&std::fs::read(&final_path).unwrap()),
        sha_hex(&f.body)
    );
    assert!(!staged.exists(), "staging name cleaned up");
    let v = snap.verification.as_ref().expect("verification evidence");
    assert_eq!(v.sha256, sha_hex(&f.body));
    let again = add(client, key, &f.url, sha).await;
    assert_eq!(again.job_id, snap.job_id, "same key → same job");
    let list = client.call(methods::LIST, json!({})).await.unwrap();
    assert_eq!(list["items"].as_array().unwrap().len(), 1);
}

/// F01–F04 + after_link: a hard crash at every stage boundary of finalization; the restart
/// reconciles the journal and ends with exactly the right file, once.
#[tokio::test]
async fn f01_a_crash_at_each_finalization_boundary_is_reconciled() {
    for (i, (fault, staged_before, final_before)) in [
        ("after_intent", true, false),
        ("after_verify", true, false),
        ("after_link", true, true),
        ("after_place", true, true),
        ("after_commit", true, true),
    ]
    .into_iter()
    .enumerate()
    {
        let f = fin_setup(&format!("f01-{i}.bin")).await;
        let key = format!("ext-f01-key-{i:04}");
        let job = crash_at(&f, &key, fault, None).await;
        assert!(
            has_event(&f.trace, "finalize_intent", job.job_id.as_str()),
            "{fault}: finalization had started"
        );
        let (final_path, staged) = final_and_staged(&f, &job);
        assert_eq!(staged.exists(), staged_before, "{fault}: staged file");
        assert_eq!(final_path.exists(), final_before, "{fault}: final file");
        let (_service, mut client, snap) = restart_until_terminal(&f, job.job_id.as_str()).await;
        assert!(
            has_event(&f.trace, "journal_reconcile", job.job_id.as_str()),
            "{fault}: reconciled at startup"
        );
        assert_completed_once(&f, &mut client, &snap, &key, None).await;
        assert_eq!(
            snap.verification.as_ref().unwrap().source,
            dm_domain::api::VerificationSource::LocalHash,
            "no trusted checksum was given"
        );
        assert_eq!(
            f.fixture.log_for(&format!("f01-{i}.bin")).len(),
            1,
            "{fault}: the file was never downloaded again"
        );
        println!("F01 {fault}: reconciled, SHA-256 match");
    }
}

/// F02: a trusted checksum is required to match; the evidence says so.
#[tokio::test]
async fn f02_a_trusted_checksum_match_is_recorded_as_such() {
    let f = fin_setup("f02.bin").await;
    let sha = sha_hex(&f.body);
    let job = crash_at(&f, "ext-f02-key-0001", "after_verify", Some(&sha)).await;
    let (_service, mut client, snap) = restart_until_terminal(&f, job.job_id.as_str()).await;
    assert_completed_once(&f, &mut client, &snap, "ext-f02-key-0001", Some(&sha)).await;
    assert_eq!(
        snap.verification.unwrap().source,
        dm_domain::api::VerificationSource::PublisherChecksum
    );
}

/// F03: an expected-checksum mismatch never completes and never places the file.
#[tokio::test]
async fn f03_a_checksum_mismatch_never_completes() {
    let f = fin_setup("f03.bin").await;
    let _service = spawn_traced(f.data.path(), f.downloads.path(), &f.trace);
    let mut client = connect(f.data.path()).await;
    let job = add(
        &mut client,
        "ext-f03-key-0001",
        &f.url,
        Some(&"0".repeat(64)),
    )
    .await;
    let snap = wait_terminal(&mut client, job.job_id.as_str()).await;
    assert_eq!(snap.phase, Phase::Failed);
    assert_eq!(
        snap.error.as_ref().unwrap().kind,
        ErrorKind::ChecksumMismatch
    );
    assert!(snap.verification.is_none());
    let (final_path, staged) = final_and_staged(&f, &snap);
    assert!(!final_path.exists(), "never placed");
    assert!(!staged.exists(), "the mismatching bytes were discarded");
}

/// F04: a file created at the destination while finalization is under way (held exactly before
/// placement) is never overwritten; the job fails explicitly and the verified bytes stay staged.
#[tokio::test]
async fn f04_a_destination_created_concurrently_is_never_overwritten() {
    let f = fin_setup("f04.bin").await;
    let _service = spawn_fault(
        f.data.path(),
        f.downloads.path(),
        &f.trace,
        "hold_before_place",
    );
    let mut client = connect(f.data.path()).await;
    let job = add(&mut client, "ext-f04-key-0001", &f.url, None).await;
    let started = Instant::now();
    while !has_event(&f.trace, "hold_before_place", job.job_id.as_str()) {
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "never reached placement"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let (final_path, staged) = final_and_staged(&f, &job);
    std::fs::write(&final_path, b"another program's file").unwrap();
    std::fs::write(format!("{}.release", f.trace.display()), b"").unwrap();
    let snap = wait_terminal(&mut client, job.job_id.as_str()).await;
    assert_eq!(snap.phase, Phase::Failed);
    let error = snap.error.unwrap();
    assert_eq!(error.kind, ErrorKind::InvalidRequest);
    assert!(error.detail.unwrap().contains("did not overwrite"));
    assert_eq!(
        std::fs::read(&final_path).unwrap(),
        b"another program's file"
    );
    assert_eq!(sha_hex(&std::fs::read(&staged).unwrap()), sha_hex(&f.body));
}

/// F05: files changed while the service was down are never adopted: a modified staging file
/// (after `verified`) and a modified final file (after `placed`) fail explicitly and are left
/// untouched; a final file that was merely removed is placed again from the intact staging file.
#[tokio::test]
async fn f05_missing_or_modified_files_after_a_crash() {
    // Modified staging file.
    let f = fin_setup("f05a.bin").await;
    let job = crash_at(&f, "ext-f05-key-000a", "after_verify", None).await;
    let (final_path, staged) = final_and_staged(&f, &job);
    let mut bytes = std::fs::read(&staged).unwrap();
    bytes[7] ^= 0xff;
    std::fs::write(&staged, &bytes).unwrap();
    let (_s, _c, snap) = restart_until_terminal(&f, job.job_id.as_str()).await;
    assert_eq!(snap.phase, Phase::Failed);
    assert!(snap.error.unwrap().detail.unwrap().contains("changed"));
    assert!(!final_path.exists(), "never placed");
    assert_eq!(std::fs::read(&staged).unwrap(), bytes, "left as found");

    // Modified final file.
    let f = fin_setup("f05b.bin").await;
    let job = crash_at(&f, "ext-f05-key-000b", "after_place", None).await;
    let (final_path, _) = final_and_staged(&f, &job);
    std::fs::write(&final_path, b"edited by someone").unwrap();
    let (_s, _c, snap) = restart_until_terminal(&f, job.job_id.as_str()).await;
    assert_eq!(snap.phase, Phase::Failed);
    assert_eq!(std::fs::read(&final_path).unwrap(), b"edited by someone");

    // Removed final file, intact staging file: placed again.
    let f = fin_setup("f05c.bin").await;
    let job = crash_at(&f, "ext-f05-key-000c", "after_place", None).await;
    let (final_path, _) = final_and_staged(&f, &job);
    std::fs::remove_file(&final_path).unwrap();
    let (_s, mut client, snap) = restart_until_terminal(&f, job.job_id.as_str()).await;
    assert_completed_once(&f, &mut client, &snap, "ext-f05-key-000c", None).await;
}

/// F06: a cancel that arrives while finalization holds the job waits for it; it can neither
/// revive nor half-cancel a completed download.
#[tokio::test]
async fn f06_cancel_during_finalization_cannot_corrupt_the_outcome() {
    let f = fin_setup("f06.bin").await;
    let _service = spawn_fault(
        f.data.path(),
        f.downloads.path(),
        &f.trace,
        "hold_before_place",
    );
    let mut client = connect(f.data.path()).await;
    let job = add(&mut client, "ext-f06-key-0001", &f.url, None).await;
    let started = Instant::now();
    while !has_event(&f.trace, "hold_before_place", job.job_id.as_str()) {
        assert!(started.elapsed() < Duration::from_secs(30));
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let data = f.data.path().to_owned();
    let id = job.job_id.as_str().to_owned();
    let cancel = tokio::spawn(async move {
        let mut c = connect(&data).await;
        c.call(methods::CANCEL, json!({"job_id": id})).await
    });
    tokio::time::sleep(Duration::from_millis(300)).await;
    std::fs::write(format!("{}.release", f.trace.display()), b"").unwrap();
    let cancelled = cancel.await.unwrap();
    let snap = wait_terminal(&mut client, job.job_id.as_str()).await;
    // Finalization was already placing: it completes, and the cancel is refused rather than
    // leaving a cancelled job with a placed file.
    assert_eq!(snap.phase, Phase::Completed);
    assert!(cancelled.is_err(), "{cancelled:?}");
    let (final_path, staged) = final_and_staged(&f, &snap);
    assert_eq!(
        sha_hex(&std::fs::read(&final_path).unwrap()),
        sha_hex(&f.body)
    );
    assert!(!staged.exists());
}

/// F07: a failed journal write is retried; nothing is verified or placed until it succeeds.
#[tokio::test]
async fn f07_a_journal_write_failure_is_retried_safely() {
    let f = fin_setup("f07.bin").await;
    let _service = spawn_fault(
        f.data.path(),
        f.downloads.path(),
        &f.trace,
        "fail_journal_once",
    );
    let mut client = connect(f.data.path()).await;
    let job = add(&mut client, "ext-f07-key-0001", &f.url, None).await;
    let snap = wait_terminal(&mut client, job.job_id.as_str()).await;
    assert!(has_event(
        &f.trace,
        "journal_write_failed",
        job.job_id.as_str()
    ));
    assert_completed_once(&f, &mut client, &snap, "ext-f07-key-0001", None).await;
}

/// F08: repeated crashes during reconciliation converge: crash after `verified`, crash again
/// while reconciling (after placing), then a clean start completes exactly once.
#[tokio::test]
async fn f08_repeated_restarts_converge() {
    let f = fin_setup("f08.bin").await;
    let job = crash_at(&f, "ext-f08-key-0001", "after_verify", None).await;
    let mut again = spawn_fault(f.data.path(), f.downloads.path(), &f.trace, "after_place");
    let code = exited(&mut again.0, Duration::from_secs(30)).await;
    assert!(code != Some(0), "died again while reconciling: {code:?}");
    let (_s, mut client, snap) = restart_until_terminal(&f, job.job_id.as_str()).await;
    let reconciles = trace_events(&f.trace)
        .iter()
        .filter(|e| e["event"] == "journal_reconcile")
        .count();
    assert_eq!(reconciles, 2, "reconciled at both restarts");
    assert_completed_once(&f, &mut client, &snap, "ext-f08-key-0001", None).await;
    // A further restart has nothing left to reconcile.
    drop((_s, client));
    let _s = spawn_traced(f.data.path(), f.downloads.path(), &f.trace);
    let mut client = connect(f.data.path()).await;
    let snap = get(&mut client, job.job_id.as_str()).await;
    assert_eq!(snap.phase, Phase::Completed);
    assert_eq!(
        trace_events(&f.trace)
            .iter()
            .filter(|e| e["event"] == "journal_reconcile")
            .count(),
        2
    );
}

// ------------------------------------------------------------------ B1.4 disk budget

/// Plenty of space for the test probe (1 TB).
const PLENTY: &str = "1000000000000";

struct Disk {
    data: tempfile::TempDir,
    downloads: tempfile::TempDir,
    _aux: tempfile::TempDir,
    trace: std::path::PathBuf,
    free: std::path::PathBuf,
    fixture: Server,
    body: Vec<u8>,
    url: String,
    path: String,
}

async fn disk_setup(name: &str, len: usize, route: impl FnOnce(&mut Route)) -> Disk {
    let aux = tempfile::tempdir().unwrap();
    let trace = aux.path().join("trace.jsonl");
    let free = aux.path().join("free");
    std::fs::write(&free, PLENTY).unwrap();
    let fixture = Server::start().await;
    let body = payload(len);
    let mut r = Route::new(body.clone());
    r.etag = Some("\"dk\"".into());
    route(&mut r);
    fixture.set(name, r);
    let url = fixture.url(name);
    Disk {
        data: tempfile::tempdir().unwrap(),
        downloads: tempfile::tempdir().unwrap(),
        _aux: aux,
        trace,
        free,
        fixture,
        body,
        url,
        path: name.to_owned(),
    }
}

/// A service whose free-space probe reads \`d.free\` (debug builds), with optional faults.
fn spawn_disk(d: &Disk, fault: &str) -> Service {
    Service(
        Command::new(SERVICE)
            .args(["--backend", "http", "--data-dir"])
            .arg(d.data.path())
            .arg("--download-dir")
            .arg(d.downloads.path())
            .env("DOWNOVA_CHECKPOINT_TRACE", &d.trace)
            .env("DOWNOVA_TEST_FREE_SPACE_FILE", &d.free)
            .env("DOWNOVA_FAULT", fault)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    )
}

async fn wait_event(trace: &Path, kind: &str, job: &str) -> serde_json::Value {
    let started = Instant::now();
    loop {
        if let Some(e) = trace_events(trace)
            .into_iter()
            .find(|e| e["event"] == kind && e["job_id"] == job)
        {
            return e;
        }
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "no {kind} event for {job}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn assert_disk_blocked(client: &mut Client, job: &str, blocker: dm_domain::Blocker) {
    let snap = get(client, job).await;
    assert!(!snap.phase.is_terminal(), "{:?}", snap.phase);
    assert_eq!(snap.blockers, vec![blocker]);
    let error = snap.error.expect("an explicit reason");
    assert!(error.retryable);
    assert!(matches!(
        error.kind,
        ErrorKind::DiskFull | ErrorKind::VolumeMissing
    ));
}

async fn assert_exact(d: &Disk, client: &mut Client, job: &str) -> DownloadSnapshot {
    let snap = wait_terminal(client, job).await;
    assert_eq!(snap.phase, Phase::Completed, "{:?}", snap.error);
    assert!(snap.blockers.is_empty());
    let file = std::fs::read(d.downloads.path().join(&snap.display_name)).unwrap();
    assert_eq!(sha_hex(&file), sha_hex(&d.body));
    assert!(
        !d.downloads
            .path()
            .join(format!("{}.downova-part", snap.display_name))
            .exists()
    );
    snap
}

fn ranges(d: &Disk, from: usize) -> Vec<String> {
    ranges_since(&d.fixture, &d.path, from)
}

/// DK01: a known size that does not fit waits before any body byte is written, then completes once
/// the space is there.
#[tokio::test]
async fn dk01_a_known_size_waits_for_space_then_completes() {
    let d = disk_setup("dk01.bin", 2 * MIB, |_| {}).await;
    std::fs::write(&d.free, "0").unwrap();
    let _s = spawn_disk(&d, "");
    let mut client = connect(d.data.path()).await;
    let job = add(&mut client, "ext-dk01-key-001", &d.url, None).await;
    wait_event(&d.trace, "disk_blocked", job.job_id.as_str()).await;
    assert_disk_blocked(
        &mut client,
        job.job_id.as_str(),
        dm_domain::Blocker::DiskUnavailable,
    )
    .await;
    let snap = get(&mut client, job.job_id.as_str()).await;
    assert_eq!(snap.desired_state, dm_domain::DesiredState::Running);
    assert_eq!(
        snap.error.unwrap().next_action,
        dm_domain::NextAction::FreeDiskSpace
    );
    std::fs::write(&d.free, PLENTY).unwrap();
    assert_exact(&d, &mut client, job.job_id.as_str()).await;
}

/// DK02: an unknown-length transfer that runs out of space stops; with nothing resumable it starts
/// again from zero once space is back (no Range is ever used for it).
#[tokio::test]
async fn dk02_an_unknown_length_transfer_running_out_of_space() {
    let d = disk_setup("dk02.bin", 3 * MIB, |r| {
        r.content_length = false;
        r.etag = None;
        r.slow = Some(Duration::from_millis(20));
    })
    .await;
    let _s = spawn_disk(&d, "");
    let mut client = connect(d.data.path()).await;
    let job = add(&mut client, "ext-dk02-key-001", &d.url, None).await;
    let started = Instant::now();
    while get(&mut client, job.job_id.as_str())
        .await
        .downloaded_bytes
        .0
        < 256 * 1024
    {
        assert!(started.elapsed() < Duration::from_secs(20));
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    std::fs::write(&d.free, "0").unwrap();
    wait_event(&d.trace, "disk_blocked", job.job_id.as_str()).await;
    assert_disk_blocked(
        &mut client,
        job.job_id.as_str(),
        dm_domain::Blocker::DiskUnavailable,
    )
    .await;
    std::fs::write(&d.free, PLENTY).unwrap();
    let snap = assert_exact(&d, &mut client, job.job_id.as_str()).await;
    assert_eq!(snap.total_bytes.map(|t| t.0), Some(d.body.len() as u64));
    assert!(
        ranges(&d, 0).is_empty(),
        "never resumed without a validator"
    );
}

/// DK03: a real "disk full" while writing (injected once), single and segmented: the job waits
/// with its durable checkpoint, then continues from it and ends byte-exact.
#[tokio::test]
async fn dk03_disk_full_while_writing_keeps_progress() {
    for (name, len) in [("dk03a.bin", 3 * MIB / 2), ("dk03b.bin", 6 * MIB)] {
        let d = disk_setup(name, len, |r| r.slow = Some(Duration::from_millis(40))).await;
        let _s = spawn_disk(&d, "enospc_write");
        let mut client = connect(d.data.path()).await;
        let job = add(&mut client, &format!("ext-{name}-key"), &d.url, None).await;
        let blocked = wait_event(&d.trace, "disk_blocked", job.job_id.as_str()).await;
        let confirmed = blocked["confirmed"].as_u64().unwrap();
        assert!(confirmed > 0, "{name}: a durable checkpoint existed");
        let seen = d.fixture.log_for(name).len();
        let snap = assert_exact(&d, &mut client, job.job_id.as_str()).await;
        let continued = ranges(&d, seen);
        assert!(
            !continued.is_empty(),
            "{name}: continued with ranges: {continued:?}"
        );
        println!(
            "DK03 {name}: blocked at {confirmed} B, continued with {continued:?}, {} B final",
            snap.downloaded_bytes.0
        );
    }
}

/// DK04: "disk full" from the durable sync itself.
#[tokio::test]
async fn dk04_disk_full_during_sync_keeps_progress() {
    let d = disk_setup("dk04.bin", 3 * MIB / 2, |r| {
        r.slow = Some(Duration::from_millis(40))
    })
    .await;
    let _s = spawn_disk(&d, "enospc_sync");
    let mut client = connect(d.data.path()).await;
    let job = add(&mut client, "ext-dk04-key-001", &d.url, None).await;
    let blocked = wait_event(&d.trace, "disk_blocked", job.job_id.as_str()).await;
    assert!(blocked["confirmed"].as_u64().unwrap() > 0);
    let seen = d.fixture.log_for("dk04.bin").len();
    assert_exact(&d, &mut client, job.job_id.as_str()).await;
    assert_eq!(
        ranges(&d, seen).first().map(String::as_str),
        Some(format!("bytes={}-", blocked["confirmed"]).as_str())
    );
}

/// DK05: the copy fallback (forced) does not fit at finalization; the job waits at `verified`
/// with its journal entry, survives a hard kill, and completes by copy once space is back.
#[tokio::test]
async fn dk05_finalization_copy_waits_for_space_across_a_restart() {
    let d = disk_setup("dk05.bin", 512 * 1024, |_| {}).await;
    let mut s = spawn_disk(&d, "no_hard_links,hold_before_place");
    let mut client = connect(d.data.path()).await;
    let job = add(&mut client, "ext-dk05-key-001", &d.url, None).await;
    wait_event(&d.trace, "hold_before_place", job.job_id.as_str()).await;
    std::fs::write(&d.free, "0").unwrap();
    std::fs::write(format!("{}.release", d.trace.display()), b"").unwrap();
    wait_event(&d.trace, "disk_blocked", job.job_id.as_str()).await;
    let snap = get(&mut client, job.job_id.as_str()).await;
    assert_eq!(snap.phase, Phase::Verifying);
    assert_eq!(snap.blockers, vec![dm_domain::Blocker::DiskUnavailable]);
    let (final_path, staged) = (
        d.downloads.path().join(&snap.display_name),
        d.downloads
            .path()
            .join(format!("{}.downova-part", snap.display_name)),
    );
    assert!(!final_path.exists(), "nothing placed");
    assert_eq!(sha_hex(&std::fs::read(&staged).unwrap()), sha_hex(&d.body));
    hard_kill(&mut s);
    drop(client);
    std::fs::write(&d.free, PLENTY).unwrap();
    let _s = spawn_disk(&d, "no_hard_links");
    let mut client = connect(d.data.path()).await;
    let snap = assert_exact(&d, &mut client, job.job_id.as_str()).await;
    assert!(has_event(
        &d.trace,
        "journal_reconcile",
        job.job_id.as_str()
    ));
    assert_eq!(
        d.fixture.log_for("dk05.bin").len(),
        1,
        "never downloaded again"
    );
    assert!(snap.verification.is_some());
}

/// DK06: the copy fallback hits "disk full" half-way: only the partial copy is removed, the
/// staged file is kept, and the next attempt places it.
#[tokio::test]
async fn dk06_disk_full_during_the_copy_fallback() {
    let d = disk_setup("dk06.bin", 512 * 1024, |_| {}).await;
    let _s = spawn_disk(&d, "no_hard_links,enospc_copy");
    let mut client = connect(d.data.path()).await;
    let job = add(&mut client, "ext-dk06-key-001", &d.url, None).await;
    wait_event(&d.trace, "disk_blocked", job.job_id.as_str()).await;
    assert_exact(&d, &mut client, job.job_id.as_str()).await;
}

/// DK07: a job waiting for space keeps its checkpoint across a hard kill and continues from it.
#[tokio::test]
async fn dk07_a_waiting_job_keeps_its_checkpoint_across_a_restart() {
    let d = disk_setup("dk07.bin", 3 * MIB / 2, |r| {
        r.slow = Some(Duration::from_millis(40))
    })
    .await;
    let mut s = spawn_disk(&d, "enospc_write");
    let mut client = connect(d.data.path()).await;
    let job = add(&mut client, "ext-dk07-key-001", &d.url, None).await;
    // Once something is durable, keep it waiting: no space from now on (the injected write
    // failure then stops it).
    wait_checkpoint(&d.trace, job.job_id.as_str(), 0).await;
    std::fs::write(&d.free, "0").unwrap();
    let blocked = wait_event(&d.trace, "disk_blocked", job.job_id.as_str()).await;
    let confirmed = blocked["confirmed"].as_u64().unwrap();
    assert!(confirmed > 0);
    hard_kill(&mut s);
    drop(client);
    let seen = d.fixture.log_for("dk07.bin").len();
    std::fs::write(&d.free, PLENTY).unwrap();
    let _s = spawn_disk(&d, "");
    let mut client = connect(d.data.path()).await;
    assert_exact(&d, &mut client, job.job_id.as_str()).await;
    assert_eq!(
        ranges(&d, seen).first().map(String::as_str),
        Some(format!("bytes={confirmed}-").as_str())
    );
}

/// DK08: a manual pause while waiting for space stays effective after the space returns.
#[tokio::test]
async fn dk08_a_manual_pause_outranks_restored_space() {
    let d = disk_setup("dk08.bin", 2 * MIB, |_| {}).await;
    std::fs::write(&d.free, "0").unwrap();
    let _s = spawn_disk(&d, "");
    let mut client = connect(d.data.path()).await;
    let job = add(&mut client, "ext-dk08-key-001", &d.url, None).await;
    wait_event(&d.trace, "disk_blocked", job.job_id.as_str()).await;
    client
        .call(methods::PAUSE, json!({"job_id": job.job_id.as_str()}))
        .await
        .unwrap();
    let seen = d.fixture.log_for("dk08.bin").len();
    std::fs::write(&d.free, PLENTY).unwrap();
    tokio::time::sleep(Duration::from_millis(3000)).await;
    let snap = get(&mut client, job.job_id.as_str()).await;
    assert_eq!(snap.desired_state, dm_domain::DesiredState::Paused);
    assert!(!snap.phase.is_terminal());
    assert_eq!(
        d.fixture.log_for("dk08.bin").len(),
        seen,
        "nothing sent while paused"
    );
    client
        .call(methods::RESUME, json!({"job_id": job.job_id.as_str()}))
        .await
        .unwrap();
    assert_exact(&d, &mut client, job.job_id.as_str()).await;
}

/// DK09: cancelling a job that waits for space is final; restored space does not revive it.
#[tokio::test]
async fn dk09_cancel_while_waiting_for_space_is_final() {
    let d = disk_setup("dk09.bin", 2 * MIB, |_| {}).await;
    std::fs::write(&d.free, "0").unwrap();
    let _s = spawn_disk(&d, "");
    let mut client = connect(d.data.path()).await;
    let job = add(&mut client, "ext-dk09-key-001", &d.url, None).await;
    wait_event(&d.trace, "disk_blocked", job.job_id.as_str()).await;
    client
        .call(methods::CANCEL, json!({"job_id": job.job_id.as_str()}))
        .await
        .unwrap();
    let seen = d.fixture.log_for("dk09.bin").len();
    std::fs::write(&d.free, PLENTY).unwrap();
    tokio::time::sleep(Duration::from_millis(3000)).await;
    let snap = get(&mut client, job.job_id.as_str()).await;
    assert_eq!(snap.phase, Phase::Cancelled);
    assert_eq!(d.fixture.log_for("dk09.bin").len(), seen);
    assert!(!d.downloads.path().join(&snap.display_name).exists());
}

/// DK10: when free space cannot be determined the check is advisory: the download proceeds
/// (the real "disk full" path still protects it), and no value is invented.
#[tokio::test]
async fn dk10_an_unknown_free_space_is_advisory() {
    let d = disk_setup("dk10.bin", 512 * 1024, |_| {}).await;
    std::fs::write(&d.free, "error").unwrap();
    let _s = spawn_disk(&d, "");
    let mut client = connect(d.data.path()).await;
    let job = add(&mut client, "ext-dk10-key-001", &d.url, None).await;
    assert_exact(&d, &mut client, job.job_id.as_str()).await;
    assert!(!has_event(&d.trace, "disk_blocked", job.job_id.as_str()));
}

/// DK11: a missing destination volume holds the job with `volume_missing` until it is back.
#[tokio::test]
async fn dk11_a_missing_volume_waits_until_it_returns() {
    let d = disk_setup("dk11.bin", 512 * 1024, |_| {}).await;
    std::fs::write(&d.free, "missing").unwrap();
    let _s = spawn_disk(&d, "");
    let mut client = connect(d.data.path()).await;
    let job = add(&mut client, "ext-dk11-key-001", &d.url, None).await;
    wait_event(&d.trace, "disk_blocked", job.job_id.as_str()).await;
    assert_disk_blocked(
        &mut client,
        job.job_id.as_str(),
        dm_domain::Blocker::VolumeMissing,
    )
    .await;
    let snap = get(&mut client, job.job_id.as_str()).await;
    assert_eq!(
        snap.error.unwrap().next_action,
        dm_domain::NextAction::ReconnectVolume
    );
    std::fs::write(&d.free, PLENTY).unwrap();
    assert_exact(&d, &mut client, job.job_id.as_str()).await;
}
