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
