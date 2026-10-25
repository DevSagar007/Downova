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
    let mut service = spawn_http(data.path(), downloads.path());
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
        CapabilitySupport::Unsupported
    );
    assert_eq!(
        support_of(caps, Capability::ResumeStrongValidator),
        CapabilitySupport::Unsupported
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
    assert_eq!(reqs.len(), 2);
    assert!(
        started.elapsed() >= Duration::from_secs(1),
        "Retry-After honoured"
    );
    assert!(reqs.iter().all(|l| l.header("range").is_none()));

    // 3. Connection cut halfway once: a fresh attempt from zero, never a resume.
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
    let at_ack = std::fs::metadata(&part).map_or(0, |m| m.len());
    tokio::time::sleep(Duration::from_millis(300)).await;
    let later = std::fs::metadata(&part).map_or(0, |m| m.len());
    println!(
        "cancel: phase {:?}, partial {at_ack} B at ack, {later} B later",
        cancelled.phase
    );
    assert_eq!(cancelled.phase, Phase::Cancelled);
    assert_eq!(at_ack, later, "no writes after cancel was acknowledged");
    assert!(!downloads.path().join(&snap.display_name).exists());

    client.call(methods::SHUTDOWN, json!({})).await.unwrap();
    assert_eq!(wait_exit(&mut service, Duration::from_secs(20)), Some(0));
}

#[test]
fn backend_flag_is_validated() {
    for args in [
        ["--backend", "curl"].as_slice(),
        ["--backend", "http", "--aria2", "x"].as_slice(),
    ] {
        let data = tempfile::tempdir().unwrap();
        let status = Command::new(SERVICE)
            .args(args)
            .arg("--data-dir")
            .arg(data.path())
            .stderr(Stdio::null())
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(2), "{args:?}");
    }
}
