//! B1.5: per-host connection budget in `dm-backend-http` (ADR 0010). The fixture server counts the
//! TCP connections that are open at the same time; that observed maximum is the oracle.

#![allow(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    clippy::print_stdout
)]

use std::path::Path;
use std::time::{Duration, Instant};

use dm_backend_api::{BackendHandle, DownloadBackend, TransferRequest, TransferState};
use dm_backend_http::{HttpBackend, HttpConfig, PART_SUFFIX};
use dm_domain::AttemptId;
use dm_test_fixtures::{Route, Server, payload_seeded};
use sha2::{Digest, Sha256};

const MIB: usize = 1024 * 1024;

/// How long the server may take to notice a connection the client already dropped (see
/// `Server::max_overlap`).
const TEARDOWN: Duration = Duration::from_millis(100);

fn backend(per_host: usize) -> HttpBackend {
    HttpBackend::new(HttpConfig {
        connect_timeout: Duration::from_secs(5),
        read_timeout: Duration::from_secs(10),
        max_connections_per_host: per_host,
        ..HttpConfig::default()
    })
    .unwrap()
}

fn request(dir: &Path, attempt: &str, name: &str, url: String, connections: u8) -> TransferRequest {
    TransferRequest {
        attempt_id: AttemptId::parse(attempt).unwrap(),
        url,
        headers: Vec::new(),
        destination_dir: dir.to_owned(),
        file_name: name.into(),
        max_connections: connections,
    }
}

fn staged(dir: &Path, name: &str) -> Vec<u8> {
    std::fs::read(dir.join(format!("{name}{PART_SUFFIX}"))).unwrap()
}

fn sha(b: &[u8]) -> String {
    Sha256::digest(b)
        .iter()
        .map(|x| format!("{x:02x}"))
        .collect()
}

fn slow(body: &[u8], delay_ms: u64) -> Route {
    let mut r = Route::new(body.to_vec());
    r.etag = Some("\"cb\"".into());
    r.slow = Some(Duration::from_millis(delay_ms));
    r
}

async fn settle(b: &HttpBackend, h: &BackendHandle) -> TransferState {
    let started = Instant::now();
    loop {
        let p = b.progress(h).await.unwrap();
        if !matches!(p.state, TransferState::Active | TransferState::Waiting) {
            return p.state;
        }
        assert!(
            started.elapsed() < Duration::from_secs(120),
            "did not settle: {p:?}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn wait_requests(srv: &Server, path: &str, n: usize) {
    let started = Instant::now();
    while srv.log_for(path).len() < n {
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "{path}: fewer than {n} requests"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// C05: a late file receives body bytes before the earlier large file finishes, even with
/// just one host slot. Both completed files must remain byte-exact.
#[tokio::test]
async fn c05_a_late_file_makes_progress_before_the_earlier_segments_finish() {
    let dir = tempfile::tempdir().unwrap();
    let srv = Server::start().await;
    let body = payload_seeded(32 * MIB, 51);
    srv.set("early.bin", slow(&body, 2));
    srv.set("late.bin", slow(&body, 2));
    let b = backend(1);
    let early = b
        .start(request(
            dir.path(),
            "att_fair000000001",
            "early.bin",
            srv.url("early.bin"),
            4,
        ))
        .await
        .unwrap();
    wait_requests(&srv, "early.bin", 2).await;
    let late = b
        .start(request(
            dir.path(),
            "att_fair000000002",
            "late.bin",
            srv.url("late.bin"),
            4,
        ))
        .await
        .unwrap();
    let started = Instant::now();
    loop {
        let p = b.progress(&late).await.unwrap();
        if p.completed.0 > 0 {
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "late file received no bytes: {p:?}"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_ne!(
        b.progress(&early).await.unwrap().state,
        TransferState::Complete,
        "late file was starved until the earlier file finished"
    );
    assert_eq!(settle(&b, &early).await, TransferState::Complete);
    assert_eq!(settle(&b, &late).await, TransferState::Complete);
    assert_eq!(sha(&staged(dir.path(), "early.bin")), sha(&body));
    assert_eq!(sha(&staged(dir.path(), "late.bin")), sha(&body));
    // The client-side measure (request permits held at once) is exact; the server-side one also
    // counts a connection the client already closed until the server notices (TEARDOWN).
    let permits = b.peak_requests_per_host();
    let overlap = srv.max_overlap(TEARDOWN);
    assert!(permits <= 1, "request permits held at once: {permits}");
    assert!(
        overlap <= 1,
        "server-observed overlap {overlap} (permits {permits})"
    );
}

/// C06: six files share the default eight host connections; all six receive data before any
/// finishes. This also exercises more logical files than the old global cap.
#[tokio::test]
async fn c06_six_files_progress_together_without_exceeding_eight_host_connections() {
    let dir = tempfile::tempdir().unwrap();
    let srv = Server::start().await;
    let bodies: Vec<_> = (0..6).map(|i| payload_seeded(16 * MIB, 60 + i)).collect();
    for (i, body) in bodies.iter().enumerate() {
        srv.set(&format!("six{i}.bin"), slow(body, 4));
    }
    let b = backend(8);
    let mut handles = Vec::new();
    for i in 0..6 {
        handles.push(
            b.start(request(
                dir.path(),
                &format!("att_six00000000{i}"),
                &format!("six{i}.bin"),
                srv.url(&format!("six{i}.bin")),
                4,
            ))
            .await
            .unwrap(),
        );
    }
    let started = Instant::now();
    loop {
        let mut progress = Vec::new();
        for h in &handles {
            progress.push(b.progress(h).await.unwrap());
        }
        if progress.iter().all(|p| p.completed.0 > 0) {
            assert!(
                progress.iter().all(|p| p.state != TransferState::Complete),
                "an earlier file finished before all six had progressed"
            );
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "some files did not receive bytes: {progress:?}"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    for (i, h) in handles.iter().enumerate() {
        assert_eq!(settle(&b, h).await, TransferState::Complete);
        assert_eq!(
            sha(&staged(dir.path(), &format!("six{i}.bin"))),
            sha(&bodies[i])
        );
    }
    // The client-side measure (request permits held at once) is exact; the server-side one also
    // counts a connection the client already closed until the server notices (TEARDOWN).
    let permits = b.peak_requests_per_host();
    let overlap = srv.max_overlap(TEARDOWN);
    assert!(permits <= 8, "request permits held at once: {permits}");
    println!(
        "C06 permits {permits} server overlap {overlap} raw {}",
        srv.max_open_connections()
    );
    assert_eq!(
        permits, 8,
        "the whole host budget was used under contention"
    );
    assert!(
        overlap <= 8,
        "server-observed overlap {overlap} (permits {permits})"
    );
}

/// C07: under steady contention every file still exports a durable checkpoint while it runs.
/// Rotating requests (ADR 0023) end after about 1 MiB, before the periodic sync's 4 MiB or
/// 1 s, so the synced state must be published when a request yields; otherwise an abrupt stop
/// loses all progress (found by process test A05 on Windows). Files stay byte-exact.
#[tokio::test]
async fn c07_rotating_segments_publish_durable_checkpoints_under_contention() {
    let dir = tempfile::tempdir().unwrap();
    let srv = Server::start().await;
    // 16 MiB like C06: still 24 segment requests for 8 slots, without the read-timeout risk of a
    // heavier load when the whole suite runs in parallel on a small machine.
    let bodies: Vec<_> = (0..6).map(|i| payload_seeded(16 * MIB, 70 + i)).collect();
    for (i, body) in bodies.iter().enumerate() {
        srv.set(&format!("cp{i}.bin"), slow(body, 4));
    }
    let b = backend(8);
    let mut handles = Vec::new();
    for i in 0..6 {
        handles.push(
            b.start(request(
                dir.path(),
                &format!("att_cp000000000{i}"),
                &format!("cp{i}.bin"),
                srv.url(&format!("cp{i}.bin")),
                4,
            ))
            .await
            .unwrap(),
        );
    }
    let started = Instant::now();
    let mut confirmed = Vec::new();
    loop {
        confirmed.clear();
        let mut running = true;
        for h in &handles {
            running &= b.progress(h).await.unwrap().state != TransferState::Complete;
            confirmed.push(
                b.checkpoint(h)
                    .await
                    .unwrap()
                    .map_or(0, |cp| cp.confirmed.0),
            );
        }
        assert!(
            running,
            "a file completed before every file exported a checkpoint: {confirmed:?}"
        );
        if confirmed.iter().all(|&c| c > 0) {
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "files without a durable checkpoint under contention: {confirmed:?}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    for (i, h) in handles.iter().enumerate() {
        assert_eq!(settle(&b, h).await, TransferState::Complete);
        assert_eq!(
            sha(&staged(dir.path(), &format!("cp{i}.bin"))),
            sha(&bodies[i])
        );
    }
    // The client-side measure (request permits held at once) is exact; the server-side one also
    // counts a connection the client already closed until the server notices (TEARDOWN).
    let permits = b.peak_requests_per_host();
    let overlap = srv.max_overlap(TEARDOWN);
    assert!(permits <= 8, "request permits held at once: {permits}");
    println!(
        "C07 permits {permits} server overlap {overlap} raw {}",
        srv.max_open_connections()
    );
    assert_eq!(
        permits, 8,
        "the whole host budget was used under contention"
    );
    assert!(
        overlap <= 8,
        "server-observed overlap {overlap} (permits {permits})"
    );
}

/// C08: a stream without range support is never interrupted to share the host (ADR 0023). Alone,
/// such a file takes a probe, range checks answered with 200, then one plain stream. With four
/// segmented files contending for the same host it takes the same plain requests (the probe and
/// one uninterrupted stream: no rotation, no restart) and completes byte-exact; the host never
/// holds more than eight request permits.
#[tokio::test]
async fn c08_a_stream_without_ranges_is_never_rotated_under_contention() {
    let dir = tempfile::tempdir().unwrap();
    let srv = Server::start().await;
    let plain_body = payload_seeded(8 * MIB, 80);
    let mut plain = slow(&plain_body, 4);
    plain.ranges = dm_test_fixtures::RangeMode::Ignore;
    srv.set("plain.bin", plain.clone());
    srv.set("alone.bin", plain);
    // The uncontended reference: the same route downloaded on its own.
    let alone = backend(8);
    let h = alone
        .start(request(
            dir.path(),
            "att_alone0000000",
            "alone.bin",
            srv.url("alone.bin"),
            4,
        ))
        .await
        .unwrap();
    assert_eq!(settle(&alone, &h).await, TransferState::Complete);
    // Range checks may differ (alone, all four segment workers ask at once and each is answered
    // with 200; under contention fewer get a slot before the fallback). The invariant is the plain
    // requests: the probe and exactly one uninterrupted stream. Rotation would add more.
    let plain_gets = |path: &str| {
        srv.log_for(path)
            .iter()
            .filter(|l| l.header("range").is_none())
            .count()
    };
    let alone_requests = plain_gets("/alone.bin");
    let bodies: Vec<_> = (0..4).map(|i| payload_seeded(16 * MIB, 81 + i)).collect();
    for (i, body) in bodies.iter().enumerate() {
        srv.set(&format!("seg{i}.bin"), slow(body, 4));
    }
    let b = backend(8);
    let mut handles = Vec::new();
    for i in 0..4 {
        handles.push(
            b.start(request(
                dir.path(),
                &format!("att_seg00000000{i}"),
                &format!("seg{i}.bin"),
                srv.url(&format!("seg{i}.bin")),
                4,
            ))
            .await
            .unwrap(),
        );
    }
    let plain_handle = b
        .start(request(
            dir.path(),
            "att_plain0000000",
            "plain.bin",
            srv.url("plain.bin"),
            4,
        ))
        .await
        .unwrap();
    assert_eq!(settle(&b, &plain_handle).await, TransferState::Complete);
    for (i, h) in handles.iter().enumerate() {
        assert_eq!(settle(&b, h).await, TransferState::Complete);
        assert_eq!(
            sha(&staged(dir.path(), &format!("seg{i}.bin"))),
            sha(&bodies[i])
        );
    }
    assert_eq!(sha(&staged(dir.path(), "plain.bin")), sha(&plain_body));
    let plain_requests = plain_gets("/plain.bin");
    let rotated: usize = (0..4)
        .map(|i| srv.log_for(&format!("/seg{i}.bin")).len())
        .sum();
    println!(
        "C08 plain (no Range) requests alone {alone_requests}, contended {plain_requests}; segmented requests {rotated}"
    );
    assert_eq!(alone_requests, 2, "probe + one stream when uncontended");
    assert_eq!(
        plain_requests, alone_requests,
        "the non-range stream took extra requests under contention"
    );
    assert!(
        rotated > 4 * 5,
        "the segmented files did rotate (contention was real)"
    );
    assert!(b.peak_requests_per_host() <= 8);
}

/// C01: three segmented files (4 connections each) from one host never use more than 8
/// connections together, but do use the whole budget; every file is byte-exact.
#[tokio::test]
async fn c01_three_segmented_files_share_eight_connections_per_host() {
    let dir = tempfile::tempdir().unwrap();
    let srv = Server::start().await;
    let bodies: Vec<Vec<u8>> = (0..3u32).map(|i| payload_seeded(8 * MIB, 10 + i)).collect();
    for (i, body) in bodies.iter().enumerate() {
        srv.set(&format!("c{i}.bin"), slow(body, 8));
    }
    let b = backend(8);
    let mut handles = Vec::new();
    for i in 0..3 {
        let name = format!("c{i}.bin");
        handles.push(
            b.start(request(
                dir.path(),
                &format!("att_c01000000000{i}"),
                &name,
                srv.url(&name),
                4,
            ))
            .await
            .unwrap(),
        );
    }
    for h in &handles {
        assert_eq!(settle(&b, h).await, TransferState::Complete);
    }
    for (i, body) in bodies.iter().enumerate() {
        assert_eq!(sha(&staged(dir.path(), &format!("c{i}.bin"))), sha(body));
    }
    let raw = srv.max_open_connections();
    let max = srv.max_overlap(TEARDOWN);
    println!(
        "C01 max simultaneous connections to the host: {max} (raw, including teardown tails: {raw})"
    );
    assert!(max <= 8, "the per-host budget was exceeded: {max}");
    assert!(
        max >= 7,
        "the budget was not used (scenario not exercised): {max}"
    );
}

/// C02: two host names are two budgets (here 2 each), even on the same server.
#[tokio::test]
async fn c02_separate_hosts_have_separate_budgets() {
    let dir = tempfile::tempdir().unwrap();
    let srv = Server::start().await;
    let body = payload_seeded(4 * MIB, 20);
    srv.set("s.bin", slow(&body, 8));
    let b = backend(2);
    let mut handles = Vec::new();
    for (i, host) in ["127.0.0.1", "localhost"].into_iter().enumerate() {
        let name = format!("h{i}.bin");
        handles.push(
            b.start(request(
                dir.path(),
                &format!("att_c02000000000{i}"),
                &name,
                srv.url_on(host, "s.bin"),
                4,
            ))
            .await
            .unwrap(),
        );
    }
    for h in &handles {
        assert_eq!(settle(&b, h).await, TransferState::Complete);
    }
    let max = srv.max_overlap(TEARDOWN);
    println!("C02 max simultaneous connections (two hosts, budget 2 each): {max}");
    assert!(max <= 4, "{max}");
    assert!(max >= 3, "both hosts' budgets were used: {max}");
    for i in 0..2 {
        assert_eq!(sha(&staged(dir.path(), &format!("h{i}.bin"))), sha(&body));
    }
}

/// C03: a redirect into a saturated host waits for that host's slot, and while it waits it does
/// not hold the slot of the host it was redirected from.
#[tokio::test]
async fn c03_a_redirect_to_a_saturated_host_waits_without_holding_the_first_host() {
    let dir = tempfile::tempdir().unwrap();
    let srv = Server::start().await;
    let body = payload_seeded(MIB, 30);
    srv.set("target.bin", slow(&body, 20));
    let mut redirect = Route::new(Vec::new());
    redirect.redirect = Some((302, srv.url_on("localhost", "target.bin")));
    srv.set("hop.bin", redirect);
    srv.set("direct.bin", Route::new(payload_seeded(64 * 1024, 31)));
    let b = backend(1);
    // Saturate "localhost".
    let busy = b
        .start(request(
            dir.path(),
            "att_c030000000001",
            "busy.bin",
            srv.url_on("localhost", "target.bin"),
            1,
        ))
        .await
        .unwrap();
    wait_requests(&srv, "target.bin", 1).await;
    // 127.0.0.1 → redirect → localhost: must wait for localhost.
    let hop = b
        .start(request(
            dir.path(),
            "att_c030000000002",
            "hop.bin",
            srv.url("hop.bin"),
            1,
        ))
        .await
        .unwrap();
    wait_requests(&srv, "hop.bin", 1).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        srv.log_for("target.bin").len(),
        1,
        "the redirected request waits"
    );
    // 127.0.0.1's single slot is free again: a direct request proceeds meanwhile.
    let direct = b
        .start(request(
            dir.path(),
            "att_c030000000003",
            "direct.bin",
            srv.url("direct.bin"),
            1,
        ))
        .await
        .unwrap();
    assert_eq!(settle(&b, &direct).await, TransferState::Complete);
    assert_eq!(
        b.progress(&hop).await.unwrap().completed.0,
        0,
        "still waiting for localhost"
    );
    assert_eq!(settle(&b, &busy).await, TransferState::Complete);
    assert_eq!(settle(&b, &hop).await, TransferState::Complete);
    assert_eq!(sha(&staged(dir.path(), "hop.bin")), sha(&body));
}

/// C04: pausing, removing and a failing transfer each release their slot (budget 1).
#[tokio::test]
async fn c04_pause_remove_and_failure_release_their_slots() {
    let dir = tempfile::tempdir().unwrap();
    let srv = Server::start().await;
    let body = payload_seeded(2 * MIB, 40);
    srv.set("a.bin", slow(&body, 20));
    srv.set("b.bin", slow(&body, 20));
    let mut cut = Route::new(body.clone());
    cut.truncate_after = Some(256 * 1024);
    srv.set("cut.bin", cut);
    srv.set("c.bin", Route::new(body.clone()));
    let b = backend(1);
    let a = b
        .start(request(
            dir.path(),
            "att_c040000000001",
            "a.bin",
            srv.url("a.bin"),
            1,
        ))
        .await
        .unwrap();
    wait_requests(&srv, "a.bin", 1).await;
    let wb = b
        .start(request(
            dir.path(),
            "att_c040000000002",
            "b.bin",
            srv.url("b.bin"),
            1,
        ))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(srv.log_for("b.bin").is_empty(), "b waits for a's slot");
    // Pause a → b gets the slot.
    b.pause(&a).await.unwrap();
    wait_requests(&srv, "b.bin", 1).await;
    // Remove b (cancel) → a can resume.
    b.remove(&wb).await.unwrap();
    b.resume(&a).await.unwrap();
    assert_eq!(settle(&b, &a).await, TransferState::Complete);
    assert_eq!(sha(&staged(dir.path(), "a.bin")), sha(&body));
    // A failing transfer releases its slot too.
    let f = b
        .start(request(
            dir.path(),
            "att_c040000000003",
            "cut.bin",
            srv.url("cut.bin"),
            1,
        ))
        .await
        .unwrap();
    assert!(matches!(settle(&b, &f).await, TransferState::Failed(_)));
    let c = b
        .start(request(
            dir.path(),
            "att_c040000000004",
            "c.bin",
            srv.url("c.bin"),
            1,
        ))
        .await
        .unwrap();
    assert_eq!(settle(&b, &c).await, TransferState::Complete);
    let max = srv.max_overlap(TEARDOWN);
    assert!(max <= 1, "{max} (raw {})", srv.max_open_connections());
}
