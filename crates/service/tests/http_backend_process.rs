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
use dm_domain::{Capability, CapabilitySupport, ErrorKind, NextAction, Phase};
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
    assert_eq!(error.kind, ErrorKind::DestinationUnwritable);
    assert!(!error.retryable);
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
    // CORE-11: the user's step matches the kind, and the job continues by itself afterwards.
    let expected = match blocker {
        dm_domain::Blocker::VolumeMissing => {
            (ErrorKind::VolumeMissing, NextAction::ReconnectVolume)
        }
        _ => (ErrorKind::DiskFull, NextAction::FreeDiskSpace),
    };
    assert_eq!((error.kind, error.next_action), expected);
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

// ------------------------------------------------------------------ B1.5 admission and connection budget

/// The server may take this long to notice a connection the client already dropped.
const TEARDOWN: Duration = Duration::from_millis(100);

struct Many {
    data: tempfile::TempDir,
    downloads: tempfile::TempDir,
    _aux: tempfile::TempDir,
    trace: std::path::PathBuf,
    fixture: Server,
    bodies: Vec<Vec<u8>>,
}

async fn many(prefix: &str, n: usize, len: usize, delay_ms: u64) -> Many {
    let aux = tempfile::tempdir().unwrap();
    let fixture = Server::start().await;
    let bodies: Vec<Vec<u8>> = (0..n)
        .map(|i| {
            let body = dm_test_fixtures::payload_seeded(len, 500 + i as u32);
            let mut r = Route::new(body.clone());
            r.etag = Some(format!("\"{prefix}{i}\""));
            r.slow = Some(Duration::from_millis(delay_ms));
            fixture.set(&format!("{prefix}{i}.bin"), r);
            body
        })
        .collect();
    Many {
        data: tempfile::tempdir().unwrap(),
        downloads: tempfile::tempdir().unwrap(),
        trace: aux.path().join("trace.jsonl"),
        _aux: aux,
        fixture,
        bodies,
    }
}

fn spawn_many(m: &Many) -> Service {
    spawn_traced(m.data.path(), m.downloads.path(), &m.trace)
}

async fn add_all(m: &Many, client: &mut Client, prefix: &str) -> Vec<DownloadSnapshot> {
    let mut jobs = Vec::new();
    for i in 0..m.bodies.len() {
        jobs.push(
            add(
                client,
                &format!("ext-{prefix}-key-{i:04}"),
                &m.fixture.url(&format!("{prefix}{i}.bin")),
                None,
            )
            .await,
        );
    }
    jobs
}

async fn all_exact(m: &Many, client: &mut Client, jobs: &[DownloadSnapshot]) {
    for (i, job) in jobs.iter().enumerate() {
        let done = wait_terminal(client, job.job_id.as_str()).await;
        assert_eq!(done.phase, Phase::Completed, "job {i}: {:?}", done.error);
        let file = std::fs::read(m.downloads.path().join(&done.display_name)).unwrap();
        assert_eq!(sha_hex(&file), sha_hex(&m.bodies[i]), "job {i}");
    }
}

/// A01: five downloads: three transfer, two wait visibly (`queued` + `queue_limit`), then all
/// complete. At most three files are ever on the wire.
#[tokio::test]
async fn a01_only_three_files_transfer_at_once() {
    let m = many("a1-", 5, 3 * MIB / 2, 20).await;
    let _s = spawn_many(&m);
    let mut client = connect(m.data.path()).await;
    let jobs = add_all(&m, &mut client, "a1-").await;
    let list: dm_domain::api::ListDownloadsResult =
        serde_json::from_value(client.call(methods::LIST, json!({})).await.unwrap()).unwrap();
    let waiting: Vec<_> = list
        .items
        .iter()
        .filter(|j| j.blockers == vec![dm_domain::Blocker::QueueLimit])
        .collect();
    assert_eq!(
        waiting.len(),
        2,
        "{:?}",
        list.items.iter().map(|j| &j.blockers).collect::<Vec<_>>()
    );
    assert!(
        waiting
            .iter()
            .all(|j| j.phase == Phase::Queued && j.attempt_id.is_none())
    );
    all_exact(&m, &mut client, &jobs).await;
    let max = m.fixture.max_overlap(TEARDOWN);
    println!(
        "A01 max simultaneous connections: {max} (raw {})",
        m.fixture.max_open_connections()
    );
    assert!(max <= 3, "{max}");
    assert_eq!(max, 3, "three files did run together");
}

/// A02: three segmented downloads from one host share at most eight connections.
#[tokio::test]
async fn a02_segmented_jobs_share_eight_connections_per_host() {
    let m = many("a2-", 3, 6 * MIB, 20).await;
    let _s = spawn_many(&m);
    let mut client = connect(m.data.path()).await;
    let jobs = add_all(&m, &mut client, "a2-").await;
    all_exact(&m, &mut client, &jobs).await;
    let max = m.fixture.max_overlap(TEARDOWN);
    println!(
        "A02 max simultaneous connections: {max} (raw {})",
        m.fixture.max_open_connections()
    );
    assert!(max <= 8, "{max}");
    assert!(max >= 7, "the budget was used: {max}");
}

/// A03: after a hard kill with five recoverable downloads, the restarted service applies the limit
/// before any network activity: at most three files transfer, and every file is byte-exact.
#[tokio::test]
async fn a03_recovered_jobs_respect_the_limit_after_a_restart() {
    let m = many("a3-", 5, 3 * MIB / 2, 40).await;
    let mut s = spawn_many(&m);
    let mut client = connect(m.data.path()).await;
    let jobs = add_all(&m, &mut client, "a3-").await;
    for job in jobs.iter().take(3) {
        wait_checkpoint(&m.trace, job.job_id.as_str(), 0).await;
    }
    hard_kill(&mut s);
    drop(client);
    let restarted_at = Instant::now();
    let _s = spawn_many(&m);
    let mut client = connect(m.data.path()).await;
    all_exact(&m, &mut client, &jobs).await;
    let max = m.fixture.max_overlap_since(Some(restarted_at), TEARDOWN);
    println!("A03 after restart: max simultaneous connections {max}");
    assert!(max <= 3, "{max}");
    let recovered = trace_events(&m.trace)
        .iter()
        .filter(|e| e["event"] == "recovered")
        .count();
    assert_eq!(recovered, 3, "the three that had checkpoints were restored");
}

/// A04: cancelling a waiting download ends it: it is never requested, and freed slots never
/// revive it.
#[tokio::test]
async fn a04_cancel_while_waiting_for_a_slot() {
    let m = many("a4-", 4, 3 * MIB / 2, 20).await;
    let _s = spawn_many(&m);
    let mut client = connect(m.data.path()).await;
    let jobs = add_all(&m, &mut client, "a4-").await;
    let waiting = &jobs[3];
    assert_eq!(waiting.blockers, vec![dm_domain::Blocker::QueueLimit]);
    client
        .call(methods::CANCEL, json!({"job_id": waiting.job_id.as_str()}))
        .await
        .unwrap();
    all_exact(&m, &mut client, &jobs[..3]).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(
        get(&mut client, waiting.job_id.as_str()).await.phase,
        Phase::Cancelled
    );
    assert!(m.fixture.log_for("a4-3.bin").is_empty(), "never requested");
}

// ------------------------------------------------------------------ B1.6 file naming

struct Named {
    data: tempfile::TempDir,
    /// A private folder holding only the download folder, so writes beside it are visible.
    _root: tempfile::TempDir,
    downloads: std::path::PathBuf,
    _aux: tempfile::TempDir,
    trace: std::path::PathBuf,
    fixture: Server,
}

async fn named_setup() -> Named {
    let aux = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let downloads = root.path().join("downloads");
    std::fs::create_dir(&downloads).unwrap();
    Named {
        data: tempfile::tempdir().unwrap(),
        _root: root,
        downloads,
        trace: aux.path().join("trace.jsonl"),
        _aux: aux,
        fixture: Server::start().await,
    }
}

fn disposition_route(body: &[u8], etag: &str, cd: &str) -> Route {
    let mut r = Route::new(body.to_vec());
    r.etag = Some(etag.into());
    r.content_disposition = Some(cd.into());
    r
}

fn listing(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

type NameCheck = Box<dyn Fn(&str) -> bool>;

/// NM01: server-suggested names are advisory: `filename*` wins, Bangla and emoji survive,
/// traversal, separators and reserved names are neutralised, long names are fitted, an existing
/// file is never overwritten, and nothing is written outside the download folder. Contents match
/// by SHA-256.
#[tokio::test]
async fn nm01_advisory_names_are_sanitized_and_never_overwrite() {
    let n = named_setup().await;
    let long_bangla = format!("{}.pdf", "বাংলা".repeat(120));
    let bn = "%E0%A6%AC%E0%A6%BE%E0%A6%82%E0%A6%B2%E0%A6%BE%20%E0%A6%AC%E0%A6%87.pdf";
    let cases: Vec<(&str, String, NameCheck)> = vec![
        (
            "bn",
            format!("attachment; filename=\"fallback.pdf\"; filename*=UTF-8''{bn}"),
            Box::new(|name| name == "বাংলা বই (1).pdf"),
        ),
        (
            "emoji",
            "attachment; filename*=UTF-8''%F0%9F%93%A6%20box.zip".into(),
            Box::new(|name| name == "📦 box.zip"),
        ),
        (
            "trav",
            r#"attachment; filename*=UTF-8''..%2F..%2Fevil.bat; filename="..\\..\\evil.bat""#
                .into(),
            Box::new(|name| name == "_.._evil.bat"),
        ),
        (
            "con",
            "attachment; filename=\"CON.txt. . \"".into(),
            Box::new(|name| name == "_CON.txt"),
        ),
        (
            "long",
            format!("attachment; filename=\"{long_bangla}\""),
            Box::new(|name| {
                name.ends_with(".pdf")
                    && name.starts_with("বাংলা")
                    && if cfg!(windows) {
                        name.encode_utf16().count() <= 231
                    } else {
                        name.len() <= 231
                    }
            }),
        ),
    ];
    std::fs::write(n.downloads.as_path().join("বাংলা বই.pdf"), b"user file").unwrap();
    let parent_before = listing(n.downloads.as_path().parent().unwrap());
    let _s = spawn_traced(n.data.path(), n.downloads.as_path(), &n.trace);
    let mut client = connect(n.data.path()).await;
    let mut jobs = Vec::new();
    for (i, (path, cd, _)) in cases.iter().enumerate() {
        let body = dm_test_fixtures::payload_seeded(200 * 1024, 900 + i as u32);
        n.fixture
            .set(path, disposition_route(&body, &format!("\"nm1{i}\""), cd));
        let url = n.fixture.url(path);
        let job = add(&mut client, &format!("ext-nm01-{i:05}"), &url, None).await;
        jobs.push((job, body));
    }
    for ((job, body), (path, _, expected)) in jobs.iter().zip(&cases) {
        let done = wait_terminal(&mut client, job.job_id.as_str()).await;
        assert_eq!(done.phase, Phase::Completed, "{path}: {:?}", done.error);
        assert!(
            expected(&done.display_name),
            "{path}: {}",
            done.display_name
        );
        let file = std::fs::read(n.downloads.as_path().join(&done.display_name)).unwrap();
        assert_eq!(sha_hex(&file), sha_hex(body), "{path}");
        println!("NM01 {path}: {}", done.display_name);
    }
    assert_eq!(
        std::fs::read(n.downloads.as_path().join("বাংলা বই.pdf")).unwrap(),
        b"user file",
        "the existing file is unchanged"
    );
    assert_eq!(
        listing(n.downloads.as_path().parent().unwrap()),
        parent_before,
        "nothing was created next to the download folder"
    );
    let files = listing(n.downloads.as_path());
    assert_eq!(files.len(), 6, "{files:?}");
    assert!(
        files.iter().all(|f| !f.ends_with(".downova-part")),
        "{files:?}"
    );
}

/// NM02: simultaneous jobs announcing the same name get distinct final names; a stray partial
/// file and an existing file (different case) are avoided and left as they were.
#[tokio::test]
async fn nm02_simultaneous_jobs_with_the_same_name() {
    let n = named_setup().await;
    let stray = n.downloads.as_path().join("same.iso.downova-part");
    std::fs::write(&stray, b"not ours").unwrap();
    std::fs::write(n.downloads.as_path().join("SAME (1).iso"), b"user file").unwrap();
    let _s = spawn_traced(n.data.path(), n.downloads.as_path(), &n.trace);
    let mut client = connect(n.data.path()).await;
    let mut jobs = Vec::new();
    for i in 0..3u32 {
        let body = dm_test_fixtures::payload_seeded(300 * 1024, 950 + i);
        let mut route = disposition_route(
            &body,
            &format!("\"nm2{i}\""),
            "attachment; filename=same.iso",
        );
        route.slow = Some(Duration::from_millis(10));
        n.fixture.set(&format!("s{i}"), route);
        let url = n.fixture.url(&format!("s{i}"));
        let job = add(&mut client, &format!("ext-nm02-{i:05}"), &url, None).await;
        jobs.push((job, body));
    }
    let mut names = Vec::new();
    for (job, body) in &jobs {
        let done = wait_terminal(&mut client, job.job_id.as_str()).await;
        assert_eq!(done.phase, Phase::Completed, "{:?}", done.error);
        let file = std::fs::read(n.downloads.as_path().join(&done.display_name)).unwrap();
        assert_eq!(sha_hex(&file), sha_hex(body), "{}", done.display_name);
        names.push(done.display_name);
    }
    names.sort();
    if cfg!(any(windows, target_os = "macos")) {
        // Case-insensitive volumes: `SAME (1).iso` occupies `same (1).iso`.
        assert_eq!(names, ["same (2).iso", "same (3).iso", "same (4).iso"]);
    } else {
        assert_eq!(names, ["same (1).iso", "same (2).iso", "same (3).iso"]);
    }
    assert_eq!(std::fs::read(&stray).unwrap(), b"not ours");
    assert_eq!(
        std::fs::read(n.downloads.as_path().join("SAME (1).iso")).unwrap(),
        b"user file"
    );
}

/// NM03: the name is frozen once selected. Pause/resume with a different header, then a hard
/// kill and a restart against a changed resource (new ETag, new name, a redirect elsewhere) keep
/// the first name; the partial file is never renamed and the content is the new representation.
#[tokio::test]
async fn nm03_the_name_survives_header_changes_pause_and_restart() {
    let n = named_setup().await;
    let first = payload(3 * MIB / 2);
    let pratham = "attachment; filename*=UTF-8''%E0%A6%AA%E0%A7%8D%E0%A6%B0%E0%A6%A5%E0%A6%AE.bin";
    let mut route = disposition_route(&first, "\"v1\"", pratham);
    route.slow = Some(Duration::from_millis(40));
    n.fixture.set("files/v.bin", route);
    let mut hop = Route::new(Vec::new());
    hop.redirect = Some((302, n.fixture.url("files/v.bin")));
    n.fixture.set("go", hop);
    let mut s = spawn_traced(n.data.path(), n.downloads.as_path(), &n.trace);
    let mut client = connect(n.data.path()).await;
    let job = add(&mut client, "ext-nm03-00001", &n.fixture.url("go"), None).await;
    let id = job.job_id.as_str().to_owned();
    assert_eq!(job.display_name, "go", "provisional name from the URL");
    wait_event(&n.trace, "name_selected", &id).await;
    assert_eq!(get(&mut client, &id).await.display_name, "প্রথম.bin");
    // Pause, change the announced name (same representation), resume.
    client
        .call(methods::PAUSE, json!({"job_id": id}))
        .await
        .unwrap();
    let mut renamed = disposition_route(&first, "\"v1\"", "attachment; filename=second.bin");
    renamed.slow = Some(Duration::from_millis(40));
    n.fixture.set("files/v.bin", renamed);
    client
        .call(methods::RESUME, json!({"job_id": id}))
        .await
        .unwrap();
    wait_checkpoint(&n.trace, &id, 0).await;
    assert_eq!(get(&mut client, &id).await.display_name, "প্রথম.bin");
    // Hard kill; the resource changes completely before the restart.
    hard_kill(&mut s);
    drop(client);
    let second = dm_test_fixtures::payload_seeded(MIB, 77);
    n.fixture.set(
        "files/v.bin",
        disposition_route(&second, "\"v2\"", "attachment; filename=third.bin"),
    );
    let mut elsewhere = Route::new(Vec::new());
    elsewhere.redirect = Some((302, n.fixture.url("other/name.bin")));
    n.fixture.set("go", elsewhere);
    assert!(
        n.downloads.as_path().join("go.downova-part").exists(),
        "the partial file kept its staging name"
    );
    let _s = spawn_traced(n.data.path(), n.downloads.as_path(), &n.trace);
    let mut client = connect(n.data.path()).await;
    let done = wait_terminal(&mut client, &id).await;
    assert_eq!(done.phase, Phase::Completed, "{:?}", done.error);
    assert_eq!(done.display_name, "প্রথম.bin");
    let file = std::fs::read(n.downloads.as_path().join("প্রথম.bin")).unwrap();
    assert_eq!(
        sha_hex(&file),
        sha_hex(&second),
        "the changed resource restarted from zero"
    );
    assert_eq!(listing(n.downloads.as_path()), ["প্রথম.bin"]);
    let selected = trace_events(&n.trace)
        .iter()
        .filter(|e| e["event"] == "name_selected" && e["job_id"] == id.as_str())
        .count();
    assert_eq!(selected, 1, "selected once, never again");
}

/// NM04: a crash during finalization after the name was selected: journal recovery places the
/// verified file under the selected name and removes the staging file.
#[tokio::test]
async fn nm04_journal_recovery_uses_the_selected_name() {
    for fault in ["after_intent", "after_link"] {
        let n = named_setup().await;
        let body = payload(400 * 1024);
        n.fixture.set(
            "j",
            disposition_route(&body, "\"j1\"", "attachment; filename=\"জার্নাল.bin\""),
        );
        let mut service = spawn_fault(n.data.path(), n.downloads.as_path(), &n.trace, fault);
        let mut client = connect(n.data.path()).await;
        let job = add(&mut client, "ext-nm04-00001", &n.fixture.url("j"), None).await;
        let code = exited(&mut service.0, Duration::from_secs(30)).await;
        assert!(code != Some(0), "{fault}: {code:?}");
        drop(client);
        assert!(
            n.downloads.as_path().join("j.downova-part").exists(),
            "{fault}"
        );
        let _s = spawn_traced(n.data.path(), n.downloads.as_path(), &n.trace);
        let mut client = connect(n.data.path()).await;
        let done = wait_terminal(&mut client, job.job_id.as_str()).await;
        assert_eq!(done.phase, Phase::Completed, "{fault}: {:?}", done.error);
        assert_eq!(done.display_name, "জার্নাল.bin", "{fault}");
        let file = std::fs::read(n.downloads.as_path().join("জার্নাল.bin")).unwrap();
        assert_eq!(sha_hex(&file), sha_hex(&body), "{fault}");
        assert_eq!(done.verification.unwrap().sha256, sha_hex(&body));
        assert_eq!(listing(n.downloads.as_path()), ["জার্নাল.bin"], "{fault}");
    }
}

// ---------------------------------------------------------------------------------------------
// B1.7 (CORE-11): failures reach clients with a stable kind, retry flag and next action, through
// the real service process and the controlled fixtures. "Verified" means the fixture produces
// exactly that server behaviour; a status whose meaning the server does not state (403) is a
// documented heuristic.
// ---------------------------------------------------------------------------------------------

/// A failed job's error, after checking that no secret from the signed link leaked anywhere in
/// the snapshot and that the job was not retried automatically (one request).
async fn failed_with(
    client: &mut Client,
    s: &Server,
    path: &str,
    key: &str,
) -> dm_domain::ErrorData {
    let url = format!("{}?token={SECRET}", s.url(path));
    let job = add(client, key, &url, None).await;
    let snap = wait_terminal(client, job.job_id.as_str()).await;
    assert_eq!(snap.phase, Phase::Failed, "{path}: {:?}", snap.error);
    let json = serde_json::to_string(&snap).unwrap();
    assert!(!json.contains(SECRET), "{json}");
    assert_eq!(
        s.log_for(path).len(),
        1,
        "{path}: never retried automatically"
    );
    snap.error.unwrap()
}

async fn paused_mid_transfer(client: &mut Client, url: &str, key: &str) -> String {
    let job = add(client, key, url, None).await;
    let id = job.job_id.to_string();
    let started = Instant::now();
    while get(client, &id).await.downloaded_bytes.0 == 0 {
        assert!(started.elapsed() < Duration::from_secs(20), "no progress");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    client
        .call(methods::PAUSE, json!({"job_id": id}))
        .await
        .unwrap();
    id
}

fn slow(body: Vec<u8>, etag: Option<&str>) -> Route {
    let mut r = Route::new(body);
    r.etag = etag.map(str::to_owned);
    r.slow = Some(Duration::from_millis(5));
    r
}

/// E01–E02: sign-in required, an expired or revoked link and a missing file.
#[tokio::test(flavor = "multi_thread")]
async fn e01_auth_and_expired_links_are_classified_and_not_retried() {
    let data = tempfile::tempdir().unwrap();
    let downloads = tempfile::tempdir().unwrap();
    let _service = Service(spawn_http(data.path(), downloads.path()));
    let mut client = connect(data.path()).await;
    let s = Server::start().await;
    for (path, status, reason) in [
        ("auth", 401, "Unauthorized"),
        ("proxy", 407, "Proxy Authentication Required"),
        ("forbidden", 403, "Forbidden"),
        ("gone", 410, "Gone"),
        ("missing", 404, "Not Found"),
    ] {
        let mut route = Route::new(payload(1024));
        route.script.push_back(Scripted::status(status, reason));
        s.set(path, route);
    }
    for (path, kind, action) in [
        // Verified: the status itself says a sign-in is needed.
        ("auth", ErrorKind::AuthRequired, NextAction::SignIn),
        ("proxy", ErrorKind::AuthRequired, NextAction::SignIn),
        // Heuristic: 403 does not say whether the link expired, was revoked or needs a sign-in;
        // the text names all three. 410 and 404 say the resource is gone.
        (
            "forbidden",
            ErrorKind::LinkRefreshRequired,
            NextAction::RefreshLink,
        ),
        (
            "gone",
            ErrorKind::LinkRefreshRequired,
            NextAction::RefreshLink,
        ),
        (
            "missing",
            ErrorKind::LinkRefreshRequired,
            NextAction::RefreshLink,
        ),
    ] {
        let error = failed_with(&mut client, &s, path, &format!("ext-e01-{path}")).await;
        println!(
            "E01 {path}: {:?} / {:?} / {:?}",
            error.kind, error.next_action, error.detail
        );
        assert_eq!((error.kind, error.next_action), (kind, action), "{path}");
        assert!(!error.retryable, "{path}");
    }
    // Nothing was received, so no final file; each failed job leaves its empty partial file
    // (existing behaviour, recorded in the B1.7 report as a gap).
    for entry in downloads.path().read_dir().unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().to_string_lossy().to_string();
        assert!(name.ends_with(".downova-part"), "{name}");
        assert_eq!(entry.metadata().unwrap().len(), 0, "{name}");
    }
}

/// E03: a server without a strong validator cannot be resumed; the refusal names the restart
/// choice and the job keeps its partial data, paused.
#[tokio::test(flavor = "multi_thread")]
async fn e03_unsupported_resume_is_refused_with_the_restart_choice() {
    let data = tempfile::tempdir().unwrap();
    let downloads = tempfile::tempdir().unwrap();
    let _service = Service(spawn_http(data.path(), downloads.path()));
    let mut client = connect(data.path()).await;
    let s = Server::start().await;
    s.set(
        "noetag",
        slow(dm_test_fixtures::payload_seeded(6 * MIB, 71), None),
    );
    let id = paused_mid_transfer(&mut client, &s.url("noetag"), "ext-e03-key-001").await;
    let refused = client
        .call(methods::RESUME, json!({"job_id": id}))
        .await
        .unwrap_err();
    let dm_platform::ipc::CallError::Rpc(rpc) = &refused else {
        panic!("{refused}")
    };
    let error = rpc.data.clone().unwrap();
    println!("E03 resume refused: {error:?}");
    assert_eq!(error.kind, ErrorKind::BackendCapabilityUnavailable);
    assert_eq!(error.next_action, NextAction::ChooseRestartOrKeep);
    assert!(!error.retryable);
    let after = get(&mut client, &id).await;
    assert_eq!(after.desired_state, dm_domain::DesiredState::Paused);
    assert!(after.downloaded_bytes.0 > 0, "the partial data is kept");
    assert_eq!(s.log_for("noetag").len(), 1, "no resume request sent");
}

/// E04: the file changes while paused and the server ignores If-Range (a 206 of the new
/// version): the change is detected before any byte is appended and the job fails with the
/// restart choice, placing nothing.
#[tokio::test(flavor = "multi_thread")]
async fn e04_a_changed_resource_is_classified_and_nothing_is_placed() {
    let data = tempfile::tempdir().unwrap();
    let downloads = tempfile::tempdir().unwrap();
    let _service = Service(spawn_http(data.path(), downloads.path()));
    let mut client = connect(data.path()).await;
    let s = Server::start().await;
    s.set(
        "changing",
        slow(
            dm_test_fixtures::payload_seeded(6 * MIB, 72),
            Some("\"e1\""),
        ),
    );
    let id = paused_mid_transfer(&mut client, &s.url("changing"), "ext-e04-key-001").await;
    s.update("changing", |r| {
        r.body = std::sync::Arc::new(dm_test_fixtures::payload_seeded(6 * MIB, 73));
        r.etag = Some("\"e2\"".into());
        r.if_range = dm_test_fixtures::Preconditions::Ignore;
        r.slow = None;
    });
    client
        .call(methods::RESUME, json!({"job_id": id}))
        .await
        .unwrap();
    let snap = wait_terminal(&mut client, &id).await;
    let error = snap.error.clone().unwrap();
    println!(
        "E04: {:?} / {:?} / {:?}",
        error.kind, error.next_action, error.detail
    );
    assert_eq!(snap.phase, Phase::Failed);
    assert_eq!(error.kind, ErrorKind::RemoteChanged);
    assert_eq!(error.next_action, NextAction::ChooseRestartOrKeep);
    assert!(!error.retryable);
    assert!(!downloads.path().join(&snap.display_name).exists());
}

/// Makes `dir` refuse new files for this user, for real (no fault injection). Returns false when
/// the platform ignores the restriction (for example when running as root), so the caller can
/// report the case as not run.
fn restrict(dir: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o555)).unwrap();
    }
    #[cfg(windows)]
    {
        // Deny "create files / write data" and "create folders / append data" to Everyone.
        let ok = Command::new("icacls")
            .arg(dir)
            .args(["/deny", "*S-1-1-0:(OI)(CI)(WD,AD)"])
            .stdout(Stdio::null())
            .status()
            .unwrap()
            .success();
        assert!(ok, "icacls /deny failed");
    }
    std::fs::write(dir.join("probe"), b"").is_err()
}

fn unrestrict(dir: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o755));
    }
    #[cfg(windows)]
    {
        let _ = Command::new("icacls")
            .arg(dir)
            .args(["/remove:d", "*S-1-1-0"])
            .stdout(Stdio::null())
            .status();
    }
}

struct Unrestrict<'a>(&'a Path);

impl Drop for Unrestrict<'_> {
    fn drop(&mut self) {
        unrestrict(self.0);
    }
}

/// E06: a download folder this user may not write to, restricted by the operating system itself.
#[tokio::test(flavor = "multi_thread")]
async fn e06_a_folder_without_write_permission_is_classified() {
    let data = tempfile::tempdir().unwrap();
    let downloads = tempfile::tempdir().unwrap();
    let _guard = Unrestrict(downloads.path());
    if !restrict(downloads.path()) {
        println!("E06 not run: this account can write despite the restriction (root?)");
        return;
    }
    let _service = Service(spawn_http(data.path(), downloads.path()));
    let mut client = connect(data.path()).await;
    let s = Server::start().await;
    s.set("f", Route::new(payload(64 * 1024)));
    let job = add(&mut client, "ext-e06-key-001", &s.url("f"), None).await;
    let snap = wait_terminal(&mut client, job.job_id.as_str()).await;
    let error = snap.error.clone().unwrap();
    println!(
        "E06: {:?} / {:?} / {:?}",
        error.kind, error.next_action, error.detail
    );
    assert_eq!(snap.phase, Phase::Failed);
    assert_eq!(error.kind, ErrorKind::DestinationUnwritable);
    assert_eq!(error.next_action, NextAction::None);
    assert!(!error.retryable);
    let detail = error.detail.unwrap();
    assert!(detail.contains("no permission"), "{detail}");
    let folder = downloads.path().to_string_lossy().to_string();
    assert!(!detail.contains(&folder), "no path in the detail: {detail}");
}

// ---------------------------------------------------------------------------------------------
// B1.7 (CORE-04, COR-08): bounded, repeated kill scenarios with final hash checks. The counts
// come from DOWNOVA_KILL_REPS (default 3, so plain `cargo test` stays short); the evidence runs
// recorded in the B1.7 report used larger values. These are process kills (the OS closes the
// process; written data reaches the file system cache), not power-loss tests.
// ---------------------------------------------------------------------------------------------

fn kill_reps() -> usize {
    std::env::var("DOWNOVA_KILL_REPS")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|n: &usize| *n >= 1)
        .unwrap_or(3)
}

/// Trace lines of one job, for failure messages.
fn job_trace(trace: &Path, job: &str) -> Vec<String> {
    trace_events(trace)
        .into_iter()
        .filter(|e| e["job_id"] == job)
        .map(|e| e.to_string())
        .collect()
}

/// Every job the service reports as completed has exactly the expected bytes (never a corrupt
/// "completed" file), and none failed; returns how many are completed. A failure message carries
/// the job's checkpoint trace, so it shows whether a committed checkpoint existed.
async fn completed_files_are_exact(
    client: &mut Client,
    downloads: &Path,
    bodies: &[(&str, &[u8])],
    trace: &Path,
) -> usize {
    let list = client.call(methods::LIST, json!({})).await.unwrap();
    let items: Vec<DownloadSnapshot> = serde_json::from_value(list["items"].clone()).unwrap();
    let mut done = 0;
    for s in items.iter().filter(|s| s.phase == Phase::Completed) {
        let body = bodies
            .iter()
            .find(|(name, _)| s.source_display.contains(name))
            .unwrap()
            .1;
        let file = std::fs::read(downloads.join(&s.display_name)).unwrap();
        assert_eq!(sha_hex(&file), sha_hex(body), "{}", s.display_name);
        done += 1;
    }
    for s in &items {
        assert_ne!(
            s.phase,
            Phase::Failed,
            "{:?}; trace of {}: {:#?}",
            s.error,
            s.job_id,
            job_trace(trace, s.job_id.as_str())
        );
    }
    done
}

/// The kill barrier for R01: returns once every unfinished job has a durable checkpoint for the
/// attempt it is running now — a `checkpoint` (or, right after a restart, `recovered`) trace
/// event of that attempt — and `primary` has one newer than `above`. Completed jobs are excluded
/// only after their files are verified. Returns `primary`'s committed byte count.
async fn wait_all_durable(
    client: &mut Client,
    trace: &Path,
    downloads: &Path,
    bodies: &[(&str, &[u8])],
    primary: &str,
    above: u64,
) -> Option<u64> {
    let started = Instant::now();
    loop {
        let list = client.call(methods::LIST, json!({})).await.unwrap();
        let items: Vec<DownloadSnapshot> = serde_json::from_value(list["items"].clone()).unwrap();
        let events = trace_events(trace);
        let durable = |s: &DownloadSnapshot, newer_than: u64| {
            s.attempt_id.as_ref().is_some_and(|attempt| {
                events.iter().any(|e| {
                    (e["event"] == "checkpoint" || e["event"] == "recovered")
                        && e["job_id"] == s.job_id.as_str()
                        && e["attempt_id"] == attempt.as_str()
                        && e["confirmed"].as_u64().is_some_and(|c| c > newer_than)
                })
            })
        };
        let mut ready = true;
        let mut primary_bytes = None;
        for s in &items {
            match s.phase {
                Phase::Completed => {}
                Phase::Failed | Phase::Cancelled => panic!(
                    "{} ended {:?} before the kill: {:?}; trace: {:#?}",
                    s.job_id,
                    s.phase,
                    s.error,
                    job_trace(trace, s.job_id.as_str())
                ),
                _ if s.job_id.as_str() == primary => {
                    if durable(s, above) {
                        primary_bytes = events
                            .iter()
                            .rev()
                            .find(|e| e["event"] == "checkpoint" && e["job_id"] == primary)
                            .and_then(|e| e["confirmed"].as_u64());
                    } else {
                        ready = false;
                    }
                }
                _ => ready &= durable(s, 0),
            }
        }
        let primary_done = items
            .iter()
            .any(|s| s.job_id.as_str() == primary && s.phase == Phase::Completed);
        if primary_done {
            completed_files_are_exact(client, downloads, bodies, trace).await;
            return None;
        }
        if ready && primary_bytes.is_some() {
            completed_files_are_exact(client, downloads, bodies, trace).await;
            return primary_bytes;
        }
        assert!(
            started.elapsed() < Duration::from_secs(60),
            "no durable checkpoint for every unfinished job: {items:#?}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// R01: a single-connection and a segmented job are killed N times. Each kill waits until every
/// unfinished job has a durable checkpoint in its current attempt (and the single-connection job
/// a newer one than at the previous kill), so every restart must recover both. Each cycle checks
/// that nothing failed and whatever is reported completed is byte-exact; at the end both files
/// match. (A kill before any usable checkpoint is covered separately: it must fail explicitly as
/// interrupted, `jobs_and_idempotency_survive_a_service_kill`.)
#[tokio::test(flavor = "multi_thread")]
async fn r01_repeated_hard_kills_during_transfers_end_exact() {
    let reps = kill_reps();
    let (data, downloads, trace_dir) = (
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
    );
    let trace = trace_dir.path().join("trace.jsonl");
    let fixture = Server::start().await;
    // Slow enough for every cycle: about 2 s between checkpoints plus a restart per kill.
    let delay = 15 + 25 * reps as u64;
    let single = dm_test_fixtures::payload_seeded(3 * MIB / 2, 81);
    let segmented = dm_test_fixtures::payload_seeded(6 * MIB, 82);
    fixture.set("r1-single.bin", slow_route(&single, "\"r1a\"", delay));
    fixture.set("r1-seg.bin", slow_route(&segmented, "\"r1b\"", delay / 2));
    let bodies: [(&str, &[u8]); 2] = [("r1-single.bin", &single), ("r1-seg.bin", &segmented)];

    let mut service = spawn_traced(data.path(), downloads.path(), &trace);
    let mut client = connect(data.path()).await;
    let a = add(
        &mut client,
        "ext-r01-key-0001",
        &fixture.url("r1-single.bin"),
        None,
    )
    .await;
    let b = add(
        &mut client,
        "ext-r01-key-0002",
        &fixture.url("r1-seg.bin"),
        None,
    )
    .await;
    let mut confirmed = 0;
    let mut kills = 0;
    for cycle in 1..=reps {
        let Some(at) = wait_all_durable(
            &mut client,
            &trace,
            downloads.path(),
            &bodies,
            a.job_id.as_str(),
            confirmed,
        )
        .await
        else {
            break;
        };
        confirmed = at;
        hard_kill(&mut service);
        kills += 1;
        drop(client);
        service = spawn_traced(data.path(), downloads.path(), &trace);
        client = connect(data.path()).await;
        let exact = completed_files_are_exact(&mut client, downloads.path(), &bodies, &trace).await;
        println!("R01 cycle {cycle}: killed at checkpoint {confirmed} B; {exact} completed so far");
    }
    let a_done = wait_terminal(&mut client, a.job_id.as_str()).await;
    let b_done = wait_terminal(&mut client, b.job_id.as_str()).await;
    assert_eq!(a_done.phase, Phase::Completed, "{:?}", a_done.error);
    assert_eq!(b_done.phase, Phase::Completed, "{:?}", b_done.error);
    assert_eq!(
        completed_files_are_exact(&mut client, downloads.path(), &bodies, &trace).await,
        2
    );
    let recovered = trace_events(&trace)
        .iter()
        .filter(|e| e["event"] == "recovered")
        .count();
    let parts: Vec<_> = std::fs::read_dir(downloads.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .filter(|n| n.ends_with(".downova-part"))
        .collect();
    assert!(parts.is_empty(), "{parts:?}");
    assert!(kills >= 1, "at least one kill landed mid-transfer");
    println!(
        "R01 {kills} of {reps} kills landed mid-transfer; {recovered} recoveries; both SHA-256 match"
    );
}

/// R02: a crash at every finalization boundary, repeated N times with fresh data each time
/// (5 × N crashes); each restart ends with exactly one correct file and the source is never
/// downloaded twice.
#[tokio::test]
async fn r02_repeated_crashes_at_every_finalization_boundary() {
    let reps = kill_reps();
    let mut crashes = 0;
    for rep in 0..reps {
        for (i, fault) in [
            "after_intent",
            "after_verify",
            "after_link",
            "after_place",
            "after_commit",
        ]
        .into_iter()
        .enumerate()
        {
            let name = format!("r02-{rep}-{i}.bin");
            let f = fin_setup(&name).await;
            let key = format!("ext-r02-{rep:03}-{i:04}");
            let job = crash_at(&f, &key, fault, None).await;
            crashes += 1;
            let (_service, mut client, snap) =
                restart_until_terminal(&f, job.job_id.as_str()).await;
            assert_completed_once(&f, &mut client, &snap, &key, None).await;
            assert_eq!(f.fixture.log_for(&name).len(), 1, "{fault}: one download");
        }
    }
    println!(
        "R02 {crashes} finalization crashes ({reps} × 5 boundaries): every file SHA-256 exact"
    );
}

/// X01 (B2.2, ADR 0012): "Exit and stop downloads" on the real service. The acknowledgement
/// reports the paused job, the process then exits by itself (verified separately from the
/// acknowledgement), the restarted service keeps the job paused and sends nothing, and the
/// user's Resume completes it byte-exact.
#[tokio::test(flavor = "multi_thread")]
async fn x01_exit_and_stop_pauses_durably_and_resume_completes() {
    let (data, downloads, trace_dir) = (
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
    );
    let trace = trace_dir.path().join("trace.jsonl");
    let fixture = Server::start().await;
    let body = dm_test_fixtures::payload_seeded(3 * MIB / 2, 91);
    fixture.set("x1.bin", slow_route(&body, "\"x1\"", 40));
    let mut service = spawn_traced(data.path(), downloads.path(), &trace);
    let mut client = connect(data.path()).await;
    let job = add(
        &mut client,
        "ext-x01-key-0001",
        &fixture.url("x1.bin"),
        None,
    )
    .await;
    let id = job.job_id.as_str();
    wait_checkpoint(&trace, id, 0).await;

    let ack = client
        .call(methods::SHUTDOWN, json!({"mode": "pause_downloads"}))
        .await
        .unwrap();
    assert_eq!(ack, json!({"mode": "pause_downloads", "paused_jobs": 1}));
    // The acknowledgement is not termination: wait for the process itself.
    assert_eq!(
        exited(&mut service.0, Duration::from_secs(30)).await,
        Some(0),
        "the service exits by itself after the acknowledgement"
    );
    let paused_at = last_event(&trace_events(&trace), "paused_for_exit", id)["confirmed"]
        .as_u64()
        .unwrap();
    assert!(paused_at > 0);
    drop(client);
    let requests = fixture.log_for("x1.bin").len();

    let _service = spawn_traced(data.path(), downloads.path(), &trace);
    let mut client = connect(data.path()).await;
    let snap = get(&mut client, id).await;
    assert_eq!(snap.desired_state, dm_domain::DesiredState::Paused);
    assert_eq!(snap.phase, Phase::Queued);
    assert_eq!(
        snap.downloaded_bytes.0, paused_at,
        "the committed checkpoint"
    );
    assert!(snap.error.is_none(), "{:?}", snap.error);
    // Stays paused: nothing is requested from the server until the user resumes.
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(
        get(&mut client, id).await.desired_state,
        dm_domain::DesiredState::Paused
    );
    assert_eq!(fixture.log_for("x1.bin").len(), requests);

    client
        .call(methods::RESUME, json!({"job_id": id}))
        .await
        .unwrap();
    let done = wait_terminal(&mut client, id).await;
    assert_eq!(done.phase, Phase::Completed, "{:?}", done.error);
    let file = std::fs::read(downloads.path().join(&done.display_name)).unwrap();
    assert_eq!(sha_hex(&file), sha_hex(&body));
    let ranges = ranges_since(&fixture, "x1.bin", requests);
    assert_eq!(
        ranges.first(),
        Some(&format!("bytes={paused_at}-")),
        "{ranges:?}"
    );
    println!("X01 paused at {paused_at} B for exit; resumed with {ranges:?}; SHA-256 match");
}
