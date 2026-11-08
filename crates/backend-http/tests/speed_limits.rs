//! CORE-06 bandwidth limits against a real fixture server (ADR 0024).
//!
//! Throughput is measured from the backend's own progress over a steady-state window that starts
//! after the first second (a fresh bucket starts empty and the first chunk is in flight), and must
//! lie within ±10 % of the limit (CORE-06 acceptance). Every finished file is byte-exact.

#![allow(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    clippy::print_stdout,
    clippy::cast_precision_loss
)]

use std::path::Path;
use std::time::{Duration, Instant};

use dm_backend_api::{BackendHandle, DownloadBackend, TransferRequest, TransferState};
use dm_backend_http::{HttpBackend, HttpConfig, PART_SUFFIX};
use dm_domain::AttemptId;
use dm_test_fixtures::{Route, Server, payload_seeded};
use sha2::{Digest, Sha256};

const KIB: usize = 1024;
const MIB: usize = 1024 * 1024;

fn backend(per_host: usize, read_timeout: Duration) -> HttpBackend {
    HttpBackend::new(HttpConfig {
        connect_timeout: Duration::from_secs(5),
        read_timeout,
        max_connections_per_host: per_host,
        ..HttpConfig::default()
    })
    .unwrap()
}

fn request(
    dir: &Path,
    attempt: &str,
    name: &str,
    url: String,
    conns: u8,
    rate: Option<u64>,
) -> TransferRequest {
    TransferRequest {
        attempt_id: AttemptId::parse(attempt).unwrap(),
        url,
        headers: Vec::new(),
        destination_dir: dir.to_owned(),
        file_name: name.into(),
        max_connections: conns,
        rate_limit: rate,
        session: None,
    }
}

fn route(body: &[u8]) -> Route {
    let mut r = Route::new(body.to_vec());
    r.etag = Some("\"sl\"".into());
    r
}

fn sha(b: &[u8]) -> String {
    Sha256::digest(b)
        .iter()
        .map(|x| format!("{x:02x}"))
        .collect()
}

async fn completed(b: &HttpBackend, h: &BackendHandle) -> u64 {
    b.progress(h).await.unwrap().completed.0
}

/// Bytes per second across `handles` between `from` and `to` after `t0`.
///
/// Progress advances in parts of up to the bucket capacity (a quarter second of the rate, ADR
/// 0024), so two samples at arbitrary moments can each be off by almost a whole part: over a
/// 1.5 s window at 100 kB/s that is up to ±25 kB on 150 kB (B3.7 follow-up: macOS measured
/// 110,486 B/s). Instead the progress is sampled every 5 ms and the rate is taken between the
/// first and the last moment within the window at which the byte count changed: both ends are
/// part completions, so the quantization at the ends cancels and the steady-state ±10 % keeps its
/// meaning. Panics if fewer than two changes were seen (the transfer was not running).
async fn measured_rate(
    b: &HttpBackend,
    handles: &[BackendHandle],
    t0: Instant,
    from: f64,
    to: f64,
) -> f64 {
    tokio::time::sleep_until((t0 + Duration::from_secs_f64(from)).into()).await;
    let end = t0 + Duration::from_secs_f64(to);
    let total = async || {
        let mut n = 0u64;
        for h in handles {
            n += completed(b, h).await;
        }
        n
    };
    let mut last = total().await;
    let mut first_change: Option<(Instant, u64)> = None;
    let mut last_change: Option<(Instant, u64)> = None;
    while Instant::now() < end {
        tokio::time::sleep(Duration::from_millis(5)).await;
        let now = total().await;
        if now != last {
            let at = (Instant::now(), now);
            first_change.get_or_insert(at);
            last_change = Some(at);
            last = now;
        }
    }
    let ((ta, a), (tz, z)) = (
        first_change.expect("no progress in the window"),
        last_change.expect("no progress in the window"),
    );
    assert!(
        tz > ta,
        "at least two progress changes are needed in the window"
    );
    (z - a) as f64 / (tz - ta).as_secs_f64()
}

fn within(rate: f64, limit: u64) -> bool {
    (rate - limit as f64).abs() <= limit as f64 * 0.10
}

async fn settle(b: &HttpBackend, h: &BackendHandle, limit: Duration) -> TransferState {
    let started = Instant::now();
    loop {
        let p = b.progress(h).await.unwrap();
        if !matches!(p.state, TransferState::Active | TransferState::Waiting) {
            return p.state;
        }
        assert!(
            started.elapsed() < limit,
            "did not settle in {limit:?}: {p:?}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn staged(dir: &Path, name: &str) -> Vec<u8> {
    std::fs::read(dir.join(format!("{name}{PART_SUFFIX}"))).unwrap()
}

/// S01: one connection under its own limit.
#[tokio::test]
async fn s01_a_download_limit_holds_within_ten_percent_and_the_file_is_exact() {
    let dir = tempfile::tempdir().unwrap();
    let srv = Server::start().await;
    let body = payload_seeded(2 * MIB, 61);
    srv.set("one.bin", route(&body));
    let b = backend(8, Duration::from_secs(10));
    let limit = 256_000;
    let t0 = Instant::now();
    let h = b
        .start(request(
            dir.path(),
            "att_rate000000001",
            "one.bin",
            srv.url("one.bin"),
            1,
            Some(limit),
        ))
        .await
        .unwrap();
    let rate = measured_rate(&b, std::slice::from_ref(&h), t0, 1.0, 5.0).await;
    println!("S01 measured {rate:.0} B/s for a {limit} B/s limit");
    assert!(within(rate, limit), "S01 {rate:.0} B/s vs {limit}");
    assert_eq!(
        settle(&b, &h, Duration::from_secs(30)).await,
        TransferState::Complete
    );
    assert_eq!(sha(&staged(dir.path(), "one.bin")), sha(&body));
}

/// S02: four segments share one file budget: extra segments never multiply the allowance.
#[tokio::test]
async fn s02_segments_share_the_download_limit() {
    let dir = tempfile::tempdir().unwrap();
    let srv = Server::start().await;
    let body = payload_seeded(3 * MIB, 62);
    srv.set("seg.bin", route(&body));
    let b = backend(8, Duration::from_secs(10));
    let limit = 400_000;
    let t0 = Instant::now();
    let h = b
        .start(request(
            dir.path(),
            "att_rate000000002",
            "seg.bin",
            srv.url("seg.bin"),
            4,
            Some(limit),
        ))
        .await
        .unwrap();
    let rate = measured_rate(&b, std::slice::from_ref(&h), t0, 1.0, 5.0).await;
    let ranged = srv
        .log_for("seg.bin")
        .iter()
        .filter(|r| r.header("range").is_some())
        .count();
    println!("S02 measured {rate:.0} B/s over {ranged} ranged requests for a {limit} B/s limit");
    assert!(ranged >= 2, "the file was segmented");
    assert!(within(rate, limit), "S02 {rate:.0} B/s vs {limit}");
    assert_eq!(
        settle(&b, &h, Duration::from_secs(30)).await,
        TransferState::Complete
    );
    assert_eq!(sha(&staged(dir.path(), "seg.bin")), sha(&body));
}

/// S03: one global budget across 1, 3 and 6 downloads.
#[tokio::test]
async fn s03_one_three_and_six_downloads_share_the_global_limit() {
    for n in [1usize, 3, 6] {
        let dir = tempfile::tempdir().unwrap();
        let srv = Server::start().await;
        let b = backend(8, Duration::from_secs(10));
        let limit = 600_000;
        b.set_global_rate(Some(limit)).unwrap();
        let mut handles = Vec::new();
        let mut bodies = Vec::new();
        let t0 = Instant::now();
        for k in 0..n {
            let name = format!("g{k}.bin");
            let body = payload_seeded(2 * MIB, 70 + k as u32);
            srv.set(&name, route(&body));
            bodies.push((name.clone(), body));
            handles.push(
                b.start(request(
                    dir.path(),
                    &format!("att_glob{n}0000000{k}"),
                    &name,
                    srv.url(&name),
                    1,
                    None,
                ))
                .await
                .unwrap(),
            );
        }
        let rate = measured_rate(&b, &handles, t0, 1.0, 3.5).await;
        println!("S03 n={n}: measured {rate:.0} B/s total for a {limit} B/s global limit");
        assert!(within(rate, limit), "S03 n={n}: {rate:.0} B/s vs {limit}");
        // Lift the limit and let every file finish byte-exact.
        b.set_global_rate(None).unwrap();
        for (h, (name, body)) in handles.iter().zip(&bodies) {
            assert_eq!(
                settle(&b, h, Duration::from_secs(30)).await,
                TransferState::Complete
            );
            assert_eq!(sha(&staged(dir.path(), name)), sha(body), "{name}");
        }
    }
}

/// S04: a live change applies to the running request without reopening it.
#[tokio::test]
async fn s04_a_live_increase_applies_without_a_new_request() {
    let dir = tempfile::tempdir().unwrap();
    let srv = Server::start().await;
    let body = payload_seeded(4 * MIB, 63);
    srv.set("live.bin", route(&body));
    let b = backend(8, Duration::from_secs(10));
    let t0 = Instant::now();
    let h = b
        .start(request(
            dir.path(),
            "att_rate000000004",
            "live.bin",
            srv.url("live.bin"),
            1,
            Some(100_000),
        ))
        .await
        .unwrap();
    let slow = measured_rate(&b, std::slice::from_ref(&h), t0, 1.0, 2.5).await;
    let requests_before = srv.log_for("live.bin").len();
    b.set_rate(&h, Some(400_000)).unwrap();
    let fast = measured_rate(&b, std::slice::from_ref(&h), t0, 3.5, 6.0).await;
    // The transfer was still running when the fast window closed (4 MiB at these rates takes
    // well over 6 s), so both windows measured steady state, not the tail of a finished file.
    assert_eq!(
        b.progress(&h).await.unwrap().state,
        TransferState::Active,
        "still transferring after both windows"
    );
    println!("S04 {slow:.0} B/s, then {fast:.0} B/s after the change");
    assert!(within(slow, 100_000), "before: {slow:.0}");
    assert!(within(fast, 400_000), "after: {fast:.0}");
    assert_eq!(
        srv.log_for("live.bin").len(),
        requests_before,
        "no request was reopened"
    );
    b.set_rate(&h, None).unwrap();
    assert_eq!(
        settle(&b, &h, Duration::from_secs(30)).await,
        TransferState::Complete
    );
    assert_eq!(sha(&staged(dir.path(), "live.bin")), sha(&body));
}

/// S05: waiting for tokens is not a network timeout.
#[tokio::test]
async fn s05_a_low_limit_never_turns_into_a_network_timeout() {
    let dir = tempfile::tempdir().unwrap();
    let srv = Server::start().await;
    let body = payload_seeded(5 * KIB, 64);
    srv.set("low.bin", route(&body));
    // A 1 s read timeout, and 1,024 B/s: about 4 s of mostly token waits.
    let b = backend(8, Duration::from_secs(1));
    let t0 = Instant::now();
    let h = b
        .start(request(
            dir.path(),
            "att_rate000000005",
            "low.bin",
            srv.url("low.bin"),
            1,
            Some(1024),
        ))
        .await
        .unwrap();
    let state = settle(&b, &h, Duration::from_secs(30)).await;
    println!("S05 finished in {:?}: {state:?}", t0.elapsed());
    assert_eq!(state, TransferState::Complete);
    assert!(t0.elapsed() >= Duration::from_secs(3), "the limit applied");
    assert_eq!(sha(&staged(dir.path(), "low.bin")), sha(&body));
}

/// S06: a throttled segment cannot hold the host's last permit for long (ADR 0024 rotation).
#[tokio::test]
async fn s06_a_throttled_segment_yields_the_last_permit_within_seconds() {
    let dir = tempfile::tempdir().unwrap();
    let srv = Server::start().await;
    let big = payload_seeded(2 * MIB, 65);
    let small = payload_seeded(64 * KIB, 66);
    srv.set("throttled.bin", route(&big));
    srv.set("other.bin", route(&small));
    // One permit for the host: the throttled file holds it unless it rotates.
    let b = backend(1, Duration::from_secs(10));
    let throttled = b
        .start(request(
            dir.path(),
            "att_rate000000006",
            "throttled.bin",
            srv.url("throttled.bin"),
            2,
            Some(2048),
        ))
        .await
        .unwrap();
    // Wait until it streams a range (the segmented phase).
    let started = Instant::now();
    while !srv
        .log_for("throttled.bin")
        .iter()
        .any(|r| r.header("range").is_some())
    {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "no ranged request"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let t0 = Instant::now();
    let other = b
        .start(request(
            dir.path(),
            "att_rate000000007",
            "other.bin",
            srv.url("other.bin"),
            1,
            None,
        ))
        .await
        .unwrap();
    let state = settle(&b, &other, Duration::from_secs(20)).await;
    println!(
        "S06 the other file finished {:?} after it started",
        t0.elapsed()
    );
    assert_eq!(state, TransferState::Complete);
    assert!(
        t0.elapsed() < Duration::from_secs(10),
        "rotation did not free the permit in time"
    );
    assert_eq!(sha(&staged(dir.path(), "other.bin")), sha(&small));
    b.remove(&throttled).await.unwrap();
}

/// A raw server that answers each connection with `head` (then `body_prefix`) and then stalls
/// with the connection open, so only the engine's own read timeout can end the transfer.
async fn stalling_server(head: &'static str, body_prefix: Vec<u8>) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            let prefix = body_prefix.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let _ = sock.read(&mut buf).await;
                if !head.is_empty() {
                    let _ = sock.write_all(head.as_bytes()).await;
                    let _ = sock.write_all(&prefix).await;
                    let _ = sock.flush().await;
                }
                tokio::time::sleep(Duration::from_secs(120)).await;
                drop(sock);
            });
        }
    });
    format!("http://{addr}/stall.bin")
}

/// S07: a limit never disables network timeouts. A server that never sends headers, and one
/// that stalls mid-body, each end in `NetworkOffline` shortly after the 1 s read timeout, with a
/// low limit applied.
#[tokio::test]
async fn s07_a_genuinely_stalled_server_still_times_out_under_a_limit() {
    for (case, head, prefix) in [
        ("headers", "", Vec::new()),
        (
            "body",
            "HTTP/1.1 200 OK\r\nContent-Length: 1048576\r\nETag: \"st\"\r\nAccept-Ranges: bytes\r\n\r\n",
            vec![7u8; 4096],
        ),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let url = stalling_server(head, prefix).await;
        let b = backend(8, Duration::from_secs(1));
        let t0 = Instant::now();
        let h = b
            .start(request(
                dir.path(),
                "att_rate000000071",
                "stall.bin",
                url,
                1,
                Some(2048),
            ))
            .await
            .unwrap();
        let state = settle(&b, &h, Duration::from_secs(20)).await;
        let took = t0.elapsed();
        println!("S07 {case}: {state:?} after {took:?}");
        match state {
            TransferState::Failed(e) => {
                assert_eq!(
                    e.kind,
                    dm_domain::ErrorKind::NetworkOffline,
                    "S07 {case}: {e:?}"
                )
            }
            other => panic!("S07 {case}: expected a timeout, got {other:?}"),
        }
        assert!(
            took >= Duration::from_secs(1),
            "S07 {case} ended before the timeout"
        );
        assert!(took < Duration::from_secs(15), "S07 {case} took {took:?}");
    }
}
