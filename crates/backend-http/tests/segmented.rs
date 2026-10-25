//! B0.4.3: validated segmented downloads in `dm-backend-http`
//! (docs/reports/b0.4.2-http-backend.md, "Validated segmentation").
//!
//! Version changes are fixture state transitions (`Route::then_after`): the first `n` requests
//! (probe and some segments) see version A, every later request sees the replacement. The request
//! log (method, `Range`, `If-Range`, which representation served it) proves each scenario before
//! the outcome is judged. Oracle: SHA-256 of the published file plus a mixed-bytes detector.

#![allow(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    clippy::print_stdout
)]

use std::path::Path;
use std::time::{Duration, Instant};

use dm_backend_api::{BackendHandle, DownloadBackend, TransferRequest, TransferState};
use dm_backend_http::{HttpBackend, HttpConfig, PART_SUFFIX};
use dm_domain::{AttemptId, ErrorKind};
use dm_test_fixtures::{Logged, Preconditions, RangeMode, Route, Server, payload_seeded};
use sha2::{Digest, Sha256};

const MIB: usize = 1024 * 1024;
const LEN: usize = 8 * MIB;
const ETAG: &str = "\"v1\"";

fn backend() -> HttpBackend {
    HttpBackend::new(HttpConfig {
        connect_timeout: Duration::from_secs(5),
        read_timeout: Duration::from_secs(5),
        ..HttpConfig::default()
    })
    .unwrap()
}

fn request(dir: &Path, attempt: &str, url: String, conns: u8) -> TransferRequest {
    TransferRequest {
        attempt_id: AttemptId::parse(attempt).unwrap(),
        url,
        headers: Vec::new(),
        destination_dir: dir.to_owned(),
        file_name: "out.bin".into(),
        max_connections: conns,
    }
}

fn sha(b: &[u8]) -> Vec<u8> {
    Sha256::digest(b).to_vec()
}

fn part(dir: &Path) -> std::path::PathBuf {
    dir.join(format!("out.bin{PART_SUFFIX}"))
}

fn version(seed: u32, tag: &str, etag: &str) -> (Vec<u8>, Route) {
    let body = payload_seeded(LEN, seed);
    let mut r = Route::new(body.clone());
    r.etag = Some(etag.into());
    r.tag = tag.into();
    (body, r)
}

async fn settle(b: &HttpBackend, h: &BackendHandle) -> TransferState {
    let started = Instant::now();
    loop {
        let p = b.progress(h).await.unwrap();
        if !matches!(p.state, TransferState::Active | TransferState::Waiting) {
            return p.state;
        }
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "did not settle: {p:?}"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

fn ranged(log: &[Logged]) -> Vec<&Logged> {
    log.iter().filter(|l| l.header("range").is_some()).collect()
}

fn mixture(file: &[u8], a: &[u8], b: &[u8]) -> &'static str {
    if file == a {
        "all A"
    } else if file == b {
        "all B"
    } else if file.len() == a.len()
        && (0..file.len())
            .step_by(4096)
            .any(|i| file[i..].starts_with(&a[i..i + 64]))
        && (0..file.len())
            .step_by(4096)
            .any(|i| file[i..].starts_with(&b[i..i + 64]))
    {
        "MIXED A/B"
    } else {
        "other"
    }
}

fn report(case: &str, outcome: &str, note: &str) {
    println!("CASE {case:<56} {outcome} {note}");
}

async fn run(route: Route, conns: u8) -> (TransferState, Server, tempfile::TempDir) {
    let s = Server::start().await;
    s.set("f", route);
    let dir = tempfile::tempdir().unwrap();
    let b = backend();
    let h = b
        .start(request(dir.path(), "att_seg", s.url("f"), conns))
        .await
        .unwrap();
    let state = settle(&b, &h).await;
    (state, s, dir)
}

fn published(dir: &Path) -> Vec<u8> {
    std::fs::read(dir.join("out.bin")).unwrap_or_default()
}

// ------------------------------------------------------------------------------------ success

#[tokio::test]
async fn g01_stable_resource_downloads_in_validated_segments() {
    let (a, r) = version(1, "A", ETAG);
    let (state, s, dir) = run(r, 4).await;
    let log = s.log_for("f");
    let ranges: Vec<&str> = ranged(&log)
        .iter()
        .map(|l| l.header("range").unwrap())
        .collect();
    report(
        "G01 stable 8 MiB, 4 connections",
        &format!("{state:?}"),
        &format!("({} requests; ranges {ranges:?})", log.len()),
    );
    assert_eq!(state, TransferState::Complete);
    assert_eq!(sha(&published(dir.path())), sha(&a));
    assert!(!part(dir.path()).exists());
    // Proof of segmentation: a probe plus four distinct, non-overlapping validated ranges.
    assert_eq!(log.len(), 5);
    assert!(log[0].header("range").is_none(), "probe");
    let mut sorted = ranges.clone();
    sorted.sort_by_key(|r| {
        r.trim_start_matches("bytes=")
            .split('-')
            .next()
            .unwrap()
            .parse::<u64>()
            .unwrap()
    });
    assert_eq!(
        sorted,
        vec![
            "bytes=0-2097151",
            "bytes=2097152-4194303",
            "bytes=4194304-6291455",
            "bytes=6291456-8388607"
        ]
    );
    for l in ranged(&log) {
        assert_eq!(l.header("if-range"), Some(ETAG));
        assert_eq!(l.header("accept-encoding"), Some("identity"));
    }
}

#[tokio::test]
async fn g02_unsuitable_resources_stay_on_one_connection() {
    for (case, etag, len, conns) in [
        ("G02 one connection requested", Some(ETAG), LEN, 1u8),
        ("G02 small file (1.5 MiB)", Some(ETAG), MIB + MIB / 2, 4),
        ("G02 no ETag", None, LEN, 4),
        ("G02 weak ETag", Some("W/\"v1\""), LEN, 4),
    ] {
        let body = payload_seeded(len, 2);
        let mut r = Route::new(body.clone());
        r.etag = etag.map(str::to_owned);
        let (state, s, dir) = run(r, conns).await;
        let log = s.log_for("f");
        report(
            case,
            &format!("{state:?}"),
            &format!("({} requests, {} ranged)", log.len(), ranged(&log).len()),
        );
        assert_eq!(state, TransferState::Complete, "{case}");
        assert_eq!(sha(&published(dir.path())), sha(&body), "{case}");
        assert_eq!(
            log.len(),
            1,
            "{case}: single connection, no probe-and-split"
        );
        assert!(ranged(&log).is_empty(), "{case}");
    }
}

#[tokio::test]
async fn g03_unexpected_200_with_ranges_ignored_restarts_on_one_connection() {
    let (a, mut r) = version(3, "A", ETAG);
    r.ranges = RangeMode::Ignore;
    let (state, s, dir) = run(r, 4).await;
    let log = s.log_for("f");
    report(
        "G03 server ignores Range (200 to segments)",
        &format!("{state:?}"),
        &format!("({} requests, {} ranged)", log.len(), ranged(&log).len()),
    );
    assert!(!ranged(&log).is_empty(), "segments were requested");
    assert_eq!(state, TransferState::Complete);
    assert_eq!(
        sha(&published(dir.path())),
        sha(&a),
        "the 200 was never spliced"
    );
    assert!(
        log.last().unwrap().header("range").is_none(),
        "final request: whole file"
    );
}

// ------------------------------------------------------------------------------------ changes

#[tokio::test]
async fn g04_change_during_parallel_transfer_restarts_onto_one_version() {
    let (a, ra) = version(4, "A", ETAG);
    let (b, rb) = version(5, "B", "\"v2\"");
    // Probe and the first two segment requests see A; the rest see B (If-Range honoured → 200).
    let (state, s, dir) = run(ra.then_after(3, rb), 4).await;
    let log = s.log_for("f");
    let file = published(dir.path());
    let served: Vec<_> = log
        .iter()
        .map(|l| {
            (
                l.served.clone().unwrap_or_default(),
                l.header("range").is_some(),
            )
        })
        .collect();
    report(
        "G04 same-length change mid-transfer, If-Range honoured",
        &format!("{state:?}"),
        &format!("({}; served {served:?})", mixture(&file, &a, &b)),
    );
    assert!(
        served.iter().any(|(t, r)| t == "A" && *r) && served.iter().any(|(t, r)| t == "B" && *r),
        "both versions reached segments"
    );
    assert_eq!(state, TransferState::Complete);
    assert_eq!(file, b, "restarted onto the new version; never mixed");
}

#[tokio::test]
async fn g05_ignored_if_range_is_caught_by_the_etag_check() {
    let (a, ra) = version(6, "A", ETAG);
    let (b, mut rb) = version(7, "B", "\"v2\"");
    rb.if_range = Preconditions::Ignore;
    let (state, s, dir) = run(ra.then_after(3, rb), 4).await;
    let log = s.log_for("f");
    report(
        "G05 change + If-Range ignored (206 of v2)",
        &format!("{state:?}"),
        &format!("({} requests)", log.len()),
    );
    assert!(
        log.iter()
            .any(|l| l.served.as_deref() == Some("B") && l.header("range").is_some())
    );
    assert!(
        matches!(&state, TransferState::Failed(e) if e.kind == ErrorKind::RemoteChanged),
        "{state:?}"
    );
    assert!(published(dir.path()).is_empty(), "nothing published");
    let _ = (a, b);
}

#[tokio::test]
async fn g06_lying_validator_is_a_protocol_limit() {
    let (a, ra) = version(8, "A", ETAG);
    let (b, rb) = version(9, "B", ETAG); // different bytes, same strong ETag
    let (state, s, dir) = run(ra.then_after(3, rb), 4).await;
    let file = published(dir.path());
    let log = s.log_for("f");
    report(
        "G06 change with an unchanged (lying) ETag",
        &format!("{state:?}"),
        &format!(
            "({}; only a trusted SHA-256 detects this)",
            mixture(&file, &a, &b)
        ),
    );
    assert!(
        log.iter()
            .any(|l| l.served.as_deref() == Some("B") && l.header("range").is_some())
    );
    assert_eq!(state, TransferState::Complete);
    assert_eq!(mixture(&file, &a, &b), "MIXED A/B", "documented limitation");
}

// ------------------------------------------------------------------------------------ responses

async fn response_case(
    case: &str,
    change: impl FnOnce(&mut Route),
) -> (TransferState, Server, tempfile::TempDir) {
    let (_, ra) = version(10, "A", ETAG);
    let mut rb = ra.clone();
    rb.tag = "changed".into();
    change(&mut rb);
    let (state, s, dir) = run(ra.then_after(1, rb), 4).await;
    let log = s.log_for("f");
    let hit = log
        .iter()
        .filter(|l| l.served.as_deref() == Some("changed") && l.header("range").is_some())
        .count();
    report(
        case,
        &format!("{state:?}"),
        &format!(
            "({} requests, {hit} ranged requests saw the change)",
            log.len()
        ),
    );
    assert!(hit > 0, "{case}: scenario not exercised");
    assert!(
        published(dir.path()).is_empty(),
        "{case}: nothing published"
    );
    (state, s, dir)
}

#[tokio::test]
async fn g07_wrong_content_range_is_rejected() {
    let (state, _, _) = response_case("G07 wrong Content-Range", |r| {
        r.ranges = RangeMode::WrongContentRange
    })
    .await;
    assert!(
        matches!(&state, TransferState::Failed(e) if e.kind == ErrorKind::InvalidRange),
        "{state:?}"
    );
}

#[tokio::test]
async fn g08_changed_total_length_is_rejected() {
    let (state, _, _) = response_case("G08 total length changed, same ETag", |r| {
        r.body = std::sync::Arc::new(payload_seeded(LEN + MIB, 10));
    })
    .await;
    assert!(
        matches!(&state, TransferState::Failed(e) if e.kind == ErrorKind::RemoteChanged),
        "{state:?}"
    );
}

#[tokio::test]
async fn g09_encoding_change_is_rejected() {
    let (state, _, _) = response_case("G09 206 with Content-Encoding: gzip", |r| {
        r.content_encoding = Some("gzip".into())
    })
    .await;
    assert!(
        matches!(&state, TransferState::Failed(e) if e.kind == ErrorKind::InvalidRequest),
        "{state:?}"
    );
}

#[tokio::test]
async fn g10_416_to_a_segment_is_rejected() {
    let (state, _, _) = response_case("G10 416 to segment requests", |r| {
        r.ranges = RangeMode::Always416
    })
    .await;
    assert!(
        matches!(&state, TransferState::Failed(e) if e.kind == ErrorKind::InvalidRange),
        "{state:?}"
    );
}

#[tokio::test]
async fn g11_interrupted_segment_fails_and_keeps_confirmed_bytes() {
    let (state, _, dir) = response_case("G11 one segment cut short", |r| {
        r.truncate_after = Some(256 * 1024);
        r.truncate_times = 1;
    })
    .await;
    assert!(
        matches!(&state, TransferState::Failed(e) if e.kind == ErrorKind::NetworkOffline),
        "{state:?}"
    );
    assert!(part(dir.path()).exists(), "partial kept until discarded");
}

// ------------------------------------------------------------------------------------ lifecycle

async fn wait_completed(b: &HttpBackend, h: &BackendHandle, min: u64) {
    let started = Instant::now();
    while b.progress(h).await.unwrap().completed.0 < min {
        assert!(started.elapsed() < Duration::from_secs(20), "no progress");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

#[tokio::test]
async fn g12_pause_resume_across_segments_is_byte_exact() {
    let (a, mut r) = version(12, "A", ETAG);
    r.slow = Some(Duration::from_millis(10));
    let s = Server::start().await;
    s.set("f", r);
    let dir = tempfile::tempdir().unwrap();
    let b = backend();
    let h = b
        .start(request(dir.path(), "att_g12", s.url("f"), 4))
        .await
        .unwrap();
    let mut pauses = Vec::new();
    for round in 1..=2u64 {
        wait_completed(&b, &h, round * MIB as u64).await;
        b.pause(&h).await.unwrap();
        let p = b.progress(&h).await.unwrap();
        assert_eq!(p.state, TransferState::Paused);
        let requests = s.log_for("f").len();
        let snapshot = std::fs::read(part(dir.path())).unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(
            std::fs::read(part(dir.path())).unwrap(),
            snapshot,
            "no writes while paused"
        );
        assert_eq!(s.log_for("f").len(), requests, "no requests while paused");
        assert_eq!(
            b.progress(&h).await.unwrap().completed,
            p.completed,
            "confirmed bytes stable"
        );
        assert_eq!(
            snapshot.len(),
            LEN,
            "file is preallocated; progress counts confirmed bytes only"
        );
        assert!(p.completed.0 < LEN as u64);
        pauses.push(p.completed.0);
        b.resume(&h).await.unwrap();
    }
    s.update("f", |r| r.slow = None);
    let state = settle(&b, &h).await;
    let log = s.log_for("f");
    let continuation: Vec<&str> = ranged(&log)
        .iter()
        .map(|l| l.header("range").unwrap())
        .filter(|r| {
            ![
                "bytes=0-2097151",
                "bytes=2097152-4194303",
                "bytes=4194304-6291455",
                "bytes=6291456-8388607",
            ]
            .contains(r)
        })
        .collect();
    report(
        "G12 two pauses and resumes, 4 segments",
        &format!("{state:?}"),
        &format!("(paused at {pauses:?}; continuation ranges {continuation:?})"),
    );
    assert_eq!(state, TransferState::Complete);
    assert_eq!(sha(&published(dir.path())), sha(&a));
    assert!(
        continuation.len() >= 4,
        "resumed ranges start inside segments"
    );
    assert!(
        ranged(&log)
            .iter()
            .all(|l| l.header("if-range") == Some(ETAG))
    );
}

#[tokio::test]
async fn g13_modified_or_truncated_partial_blocks_segmented_resume() {
    for (case, truncate) in [
        ("G13 partial modified inside a segment", false),
        ("G13 partial truncated", true),
    ] {
        let (_, mut r) = version(13, "A", ETAG);
        r.slow = Some(Duration::from_millis(10));
        let s = Server::start().await;
        s.set("f", r);
        let dir = tempfile::tempdir().unwrap();
        let b = backend();
        let h = b
            .start(request(dir.path(), "att_g13", s.url("f"), 4))
            .await
            .unwrap();
        wait_completed(&b, &h, MIB as u64).await;
        b.pause(&h).await.unwrap();
        let requests = s.log_for("f").len();
        let p = part(dir.path());
        if truncate {
            std::fs::OpenOptions::new()
                .write(true)
                .open(&p)
                .unwrap()
                .set_len(1000)
                .unwrap();
        } else {
            // Byte 10 of the second segment is confirmed (each segment has progressed).
            let mut bytes = std::fs::read(&p).unwrap();
            bytes[2 * MIB + 10] ^= 0xFF;
            std::fs::write(&p, bytes).unwrap();
        }
        let e = b.resume(&h).await.unwrap_err();
        report(case, &format!("{:?}", e.kind), &format!("({})", e.detail));
        assert_eq!(e.kind, ErrorKind::InvalidRange);
        assert_eq!(s.log_for("f").len(), requests, "no request sent");
    }
}

#[tokio::test]
async fn g14_cancel_during_segments_stops_every_worker() {
    let (_, mut r) = version(14, "A", ETAG);
    r.slow = Some(Duration::from_millis(10));
    let s = Server::start().await;
    s.set("f", r);
    let dir = tempfile::tempdir().unwrap();
    let b = backend();
    let h = b
        .start(request(dir.path(), "att_g14", s.url("f"), 4))
        .await
        .unwrap();
    wait_completed(&b, &h, MIB as u64).await;
    b.remove(&h).await.unwrap();
    let at_ack = std::fs::read(part(dir.path())).unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    let later = std::fs::read(part(dir.path())).unwrap();
    report(
        "G14 cancel during a segmented transfer",
        "Removed",
        &format!("({} ranged requests)", ranged(&s.log_for("f")).len()),
    );
    assert_eq!(at_ack, later, "no worker writes after remove returned");
    assert!(!dir.path().join("out.bin").exists());
    assert_eq!(
        b.progress(&h).await.unwrap_err().kind,
        ErrorKind::InvalidRequest,
        "cannot be revived"
    );
}

#[tokio::test]
async fn g15_cancel_racing_finalization_never_publishes_a_partial_file() {
    // Rounds slow the tail differently so the cancel lands before, during and after the last
    // writes and the publish. Both outcomes must occur for the race to count as exercised.
    let mut outcomes = (0, 0);
    for round in 0..8u64 {
        let (a, mut r) = version(15, "A", ETAG);
        r.slow = (round > 0).then(|| Duration::from_millis(round * 2));
        let s = Server::start().await;
        s.set("f", r);
        let dir = tempfile::tempdir().unwrap();
        let b = backend();
        let h = b
            .start(request(
                dir.path(),
                &format!("att_g15{round}"),
                s.url("f"),
                4,
            ))
            .await
            .unwrap();
        if round == 0 {
            // Deterministic side: a remove after completion never deletes or revives the
            // published file.
            assert_eq!(settle(&b, &h).await, TransferState::Complete);
        } else {
            // Racing side: the cancel lands while the last bytes, the flush or the publish are
            // in flight; the result must be atomic either way.
            wait_completed(&b, &h, (LEN - MIB / 2) as u64).await;
        }
        b.remove(&h).await.unwrap();
        let file = published(dir.path());
        report(
            &format!("G15 cancel near completion, round {round}"),
            if file.is_empty() {
                "not published"
            } else {
                "published"
            },
            "",
        );
        if file.is_empty() {
            outcomes.1 += 1;
            assert!(!dir.path().join("out.bin").exists());
        } else {
            outcomes.0 += 1;
            assert!(
                !part(dir.path()).exists(),
                "published files leave no partial behind"
            );
        }
        assert!(
            file.is_empty() || sha(&file) == sha(&a),
            "either nothing or the complete, correct file"
        );
    }
    println!(
        "G15 outcomes: published {}, cancelled before publishing {}",
        outcomes.0, outcomes.1
    );
    assert!(
        outcomes.0 > 0 && outcomes.1 > 0,
        "both sides of the race were exercised: {outcomes:?}"
    );
}

#[tokio::test]
async fn g16_destination_appearing_during_segments_is_not_overwritten() {
    let (_, mut r) = version(16, "A", ETAG);
    r.slow = Some(Duration::from_millis(2));
    let s = Server::start().await;
    s.set("f", r);
    let dir = tempfile::tempdir().unwrap();
    let b = backend();
    let h = b
        .start(request(dir.path(), "att_g16", s.url("f"), 4))
        .await
        .unwrap();
    wait_completed(&b, &h, 1).await;
    std::fs::write(dir.path().join("out.bin"), b"user file").unwrap();
    let state = settle(&b, &h).await;
    report(
        "G16 destination appears during segments",
        &format!("{state:?}"),
        "",
    );
    assert!(
        matches!(&state, TransferState::Failed(e) if e.kind == ErrorKind::InvalidRequest),
        "{state:?}"
    );
    assert_eq!(
        std::fs::read(dir.path().join("out.bin")).unwrap(),
        b"user file"
    );
}
