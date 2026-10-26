//! B0.4.2 (second increment): validated single-connection pause/resume for `dm-backend-http`
//! (docs/reports/b0.4.2-http-backend.md, "Validated resume").
//!
//! Every case pauses a real transfer once confirmed bytes are on disk, changes the fixture state
//! (or the partial file) while nothing is in flight, resumes, and checks the server's request log
//! to prove which scenario actually happened before judging the outcome. Oracle: SHA-256 of the
//! published file plus a mixed-bytes detector.
//! Since B1.3 (ADR 0008) the backend never publishes: a finished transfer is its synced staging
//! file `out.bin.downova-part`; verification and placement at `out.bin` belong to the service.

#![allow(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    clippy::print_stdout
)]

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dm_backend_api::{BackendHandle, DownloadBackend, TransferRequest, TransferState};
use dm_backend_http::{HttpBackend, HttpConfig, PART_SUFFIX};
use dm_domain::redact::Secret;
use dm_domain::{AttemptId, ErrorKind};
use dm_test_fixtures::{Logged, Preconditions, RangeMode, Route, Server, payload_seeded};
use sha2::{Digest, Sha256};

const MIB: usize = 1024 * 1024;
const ETAG: &str = "\"v1\"";

fn backend() -> HttpBackend {
    HttpBackend::new(HttpConfig {
        connect_timeout: Duration::from_secs(5),
        read_timeout: Duration::from_secs(5),
        ..HttpConfig::default()
    })
    .unwrap()
}

fn request(dir: &Path, attempt: &str, url: String) -> TransferRequest {
    TransferRequest {
        attempt_id: AttemptId::parse(attempt).unwrap(),
        url,
        headers: Vec::new(),
        destination_dir: dir.to_owned(),
        file_name: "out.bin".into(),
        max_connections: 1,
    }
}

fn sha(b: &[u8]) -> Vec<u8> {
    Sha256::digest(b).to_vec()
}

fn part(dir: &Path) -> std::path::PathBuf {
    dir.join(format!("out.bin{PART_SUFFIX}"))
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
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// Version A: 4 MiB, strong ETag, slow enough to pause in the middle.
fn version_a() -> (Vec<u8>, Route) {
    let a = payload_seeded(4 * MIB, 21);
    let mut r = Route::new(a.clone());
    r.etag = Some(ETAG.into());
    r.slow = Some(Duration::from_millis(5));
    (a, r)
}

/// Starts the transfer and pauses once at least `min` confirmed bytes exist. Returns the handle
/// and the paused offset (equal to the partial file's length, asserted).
async fn start_and_pause(
    b: &HttpBackend,
    dir: &Path,
    attempt: &str,
    url: String,
    min: u64,
) -> (BackendHandle, u64) {
    let h = b.start(request(dir, attempt, url)).await.unwrap();
    pause_after(b, &h, dir, min).await
}

async fn pause_after(
    b: &HttpBackend,
    h: &BackendHandle,
    dir: &Path,
    min: u64,
) -> (BackendHandle, u64) {
    let started = Instant::now();
    while b.progress(h).await.unwrap().completed.0 < min {
        assert!(started.elapsed() < Duration::from_secs(20), "no progress");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    b.pause(h).await.unwrap();
    let p = b.progress(h).await.unwrap();
    assert_eq!(p.state, TransferState::Paused);
    let on_disk = std::fs::metadata(part(dir)).unwrap().len();
    assert_eq!(on_disk, p.completed.0, "confirmed bytes are on disk");
    (h.clone(), on_disk)
}

fn mixture(file: &[u8], a: &[u8], b: &[u8]) -> &'static str {
    if file == a {
        "all A"
    } else if file == b {
        "all B"
    } else if file.len() == a.len()
        && file.first() == a.first()
        && file.ends_with(&b[b.len() - 64..])
    {
        "MIXED A/B"
    } else {
        "other"
    }
}

fn report(case: &str, outcome: &str, note: &str) {
    println!("CASE {case:<52} {outcome} {note}");
}

fn resume_request(log: &[Logged], offset: u64) -> &Logged {
    let want = format!("bytes={offset}-");
    log.iter()
        .find(|l| l.header("range") == Some(want.as_str()))
        .unwrap_or_else(|| panic!("no resume request with Range {want}: {log:?}"))
}

// ------------------------------------------------------------------------------------ success

#[tokio::test]
async fn v01_stable_resource_resumes_byte_exact() {
    let s = Server::start().await;
    let (a, r) = version_a();
    s.set("f", r);
    let dir = tempfile::tempdir().unwrap();
    let b = backend();
    let (h, offset) = start_and_pause(&b, dir.path(), "att_v01", s.url("f"), 512 * 1024).await;
    s.update("f", |r| r.slow = None);
    b.resume(&h).await.unwrap();
    let state = settle(&b, &h).await;
    let file = std::fs::read(part(dir.path())).unwrap_or_default();
    let log = s.log_for("f");
    let resumed = resume_request(&log, offset);
    report(
        "V01 stable resource, pause/resume",
        &format!("{state:?}"),
        &format!(
            "(paused at {offset} B; {} requests; If-Range {:?}; sha match {})",
            log.len(),
            resumed.header("if-range"),
            sha(&file) == sha(&a)
        ),
    );
    assert_eq!(state, TransferState::Complete);
    assert_eq!(sha(&file), sha(&a));
    assert_eq!(log.len(), 2);
    assert_eq!(resumed.header("if-range"), Some(ETAG));
    assert_eq!(resumed.header("accept-encoding"), Some("identity"));
    assert!(log[0].header("range").is_none());
}

#[tokio::test]
async fn v02_repeated_pause_resume_and_increasing_offsets() {
    let s = Server::start().await;
    let (a, mut r) = version_a();
    r.slow = Some(Duration::from_millis(10));
    s.set("f", r);
    let dir = tempfile::tempdir().unwrap();
    let b = backend();
    let (h, mut offset) = start_and_pause(&b, dir.path(), "att_v02", s.url("f"), 256 * 1024).await;
    let mut offsets = vec![offset];
    for _ in 0..2 {
        b.resume(&h).await.unwrap();
        let (_, next) = pause_after(&b, &h, dir.path(), offset + 256 * 1024).await;
        assert!(next > offset);
        offset = next;
        offsets.push(offset);
    }
    s.update("f", |r| r.slow = None);
    b.resume(&h).await.unwrap();
    let state = settle(&b, &h).await;
    let file = std::fs::read(part(dir.path())).unwrap_or_default();
    let log = s.log_for("f");
    for o in &offsets {
        resume_request(&log, *o);
    }
    report(
        "V02 three pauses, three resumes",
        &format!("{state:?}"),
        &format!("(offsets {offsets:?}; {} requests)", log.len()),
    );
    assert_eq!(state, TransferState::Complete);
    assert_eq!(sha(&file), sha(&a));
    assert_eq!(log.len(), 4);
}

// ------------------------------------------------------------------------------------ changes

#[tokio::test]
async fn v03_changed_resource_with_new_etag_restarts_cleanly() {
    let s = Server::start().await;
    let (a, r) = version_a();
    s.set("f", r);
    let dir = tempfile::tempdir().unwrap();
    let b = backend();
    let (h, offset) = start_and_pause(&b, dir.path(), "att_v03", s.url("f"), 512 * 1024).await;
    let new = payload_seeded(4 * MIB, 22);
    s.update("f", |r| {
        r.body = Arc::new(new.clone());
        r.etag = Some("\"v2\"".into());
        r.slow = None;
        r.tag = "B".into();
    });
    b.resume(&h).await.unwrap();
    let state = settle(&b, &h).await;
    let file = std::fs::read(part(dir.path())).unwrap_or_default();
    let log = s.log_for("f");
    let resumed = resume_request(&log, offset);
    report(
        "V03 same-length change, new ETag (If-Range honoured)",
        &format!("{state:?}"),
        &format!(
            "({}; resume request served by {:?} as a full 200)",
            mixture(&file, &a, &new),
            resumed.served
        ),
    );
    assert_eq!(
        resumed.served.as_deref(),
        Some("B"),
        "the change happened before the resume request"
    );
    assert_eq!(state, TransferState::Complete);
    assert_eq!(
        file, new,
        "a clean restart onto the new representation, never a splice"
    );
}

#[tokio::test]
async fn v04_ignored_if_range_is_detected_by_the_etag_check() {
    let s = Server::start().await;
    let (a, r) = version_a();
    s.set("f", r);
    let dir = tempfile::tempdir().unwrap();
    let b = backend();
    let (h, offset) = start_and_pause(&b, dir.path(), "att_v04", s.url("f"), 512 * 1024).await;
    let new = payload_seeded(4 * MIB, 23);
    s.update("f", |r| {
        r.body = Arc::new(new.clone());
        r.etag = Some("\"v2\"".into());
        r.if_range = Preconditions::Ignore;
        r.slow = None;
        r.tag = "B".into();
    });
    b.resume(&h).await.unwrap();
    let state = settle(&b, &h).await;
    let log = s.log_for("f");
    let resumed = resume_request(&log, offset);
    let on_disk = std::fs::metadata(part(dir.path())).unwrap().len();
    report(
        "V04 change + server ignores If-Range (206 of v2)",
        &format!("{state:?}"),
        &format!(
            "(served {:?}; partial {on_disk} B of offset {offset})",
            resumed.served
        ),
    );
    assert_eq!(resumed.served.as_deref(), Some("B"));
    assert!(
        matches!(&state, TransferState::Failed(e) if e.kind == ErrorKind::RemoteChanged),
        "{state:?}"
    );
    assert_eq!(on_disk, offset, "nothing appended");
    assert!(!dir.path().join("out.bin").exists(), "nothing published");
    let _ = a;
}

#[tokio::test]
async fn v05_server_lying_about_its_validator_is_a_protocol_limit() {
    // Same ETag for different bytes: no client can detect this without a trusted hash.
    let s = Server::start().await;
    let (a, r) = version_a();
    s.set("f", r);
    let dir = tempfile::tempdir().unwrap();
    let b = backend();
    let (h, offset) = start_and_pause(&b, dir.path(), "att_v05", s.url("f"), 512 * 1024).await;
    let new = payload_seeded(4 * MIB, 24);
    s.update("f", |r| {
        r.body = Arc::new(new.clone());
        r.slow = None;
        r.tag = "B".into();
    });
    b.resume(&h).await.unwrap();
    let state = settle(&b, &h).await;
    let file = std::fs::read(part(dir.path())).unwrap_or_default();
    let log = s.log_for("f");
    let resumed = resume_request(&log, offset);
    report(
        "V05 change with an unchanged (lying) ETag",
        &format!("{state:?}"),
        &format!(
            "({}; only a trusted SHA-256 detects this)",
            mixture(&file, &a, &new)
        ),
    );
    assert_eq!(resumed.served.as_deref(), Some("B"));
    assert_eq!(state, TransferState::Complete);
    assert_eq!(
        mixture(&file, &a, &new),
        "MIXED A/B",
        "documented limitation"
    );
}

#[tokio::test]
async fn v06_weak_or_missing_etag_makes_resume_explicitly_unavailable() {
    for (case, etag) in [("V06 weak ETag", Some("W/\"v1\"")), ("V06 no ETag", None)] {
        let s = Server::start().await;
        let (_, mut r) = version_a();
        r.etag = etag.map(str::to_owned);
        s.set("f", r);
        let dir = tempfile::tempdir().unwrap();
        let b = backend();
        let (h, _) = start_and_pause(&b, dir.path(), "att_v06", s.url("f"), 256 * 1024).await;
        let e = b.resume(&h).await.unwrap_err();
        report(case, &format!("{:?}", e.kind), &format!("({})", e.detail));
        assert_eq!(e.kind, ErrorKind::BackendCapabilityUnavailable);
        assert_eq!(s.log_for("f").len(), 1, "no resume request");
        assert_eq!(b.progress(&h).await.unwrap().state, TransferState::Paused);
    }
}

#[tokio::test]
async fn v07_changed_length_is_rejected() {
    let s = Server::start().await;
    let (_, r) = version_a();
    s.set("f", r);
    let dir = tempfile::tempdir().unwrap();
    let b = backend();
    let (h, offset) = start_and_pause(&b, dir.path(), "att_v07", s.url("f"), 512 * 1024).await;
    s.update("f", |r| {
        r.body = Arc::new(payload_seeded(5 * MIB, 25)); // same ETag, longer
        r.slow = None;
    });
    b.resume(&h).await.unwrap();
    let state = settle(&b, &h).await;
    report(
        "V07 total length changed, same ETag",
        &format!("{state:?}"),
        "",
    );
    resume_request(&s.log_for("f"), offset);
    assert!(
        matches!(&state, TransferState::Failed(e) if e.kind == ErrorKind::RemoteChanged),
        "{state:?}"
    );
    assert_eq!(std::fs::metadata(part(dir.path())).unwrap().len(), offset);
}

// ------------------------------------------------------------------------------------ responses

async fn response_case(
    case: &str,
    change: impl FnOnce(&mut Route),
) -> (TransferState, u64, u64, Vec<u8>, Vec<u8>, usize) {
    let s = Server::start().await;
    let (a, r) = version_a();
    s.set("f", r);
    let dir = tempfile::tempdir().unwrap();
    let b = backend();
    let (h, offset) = start_and_pause(&b, dir.path(), "att_rsp", s.url("f"), 512 * 1024).await;
    s.update("f", |r| {
        r.slow = None;
        change(r);
    });
    b.resume(&h).await.unwrap();
    let state = settle(&b, &h).await;
    let log = s.log_for("f");
    resume_request(&log, offset);
    let on_disk = std::fs::metadata(part(dir.path())).map_or(0, |m| m.len());
    // Only a completed transfer has a result (its staged file); the destination is never
    // written by the backend.
    assert!(!dir.path().join("out.bin").exists());
    let file = if state == TransferState::Complete {
        std::fs::read(part(dir.path())).unwrap_or_default()
    } else {
        Vec::new()
    };
    report(
        case,
        &format!("{state:?}"),
        &format!(
            "(offset {offset}; partial {on_disk} B; {} requests)",
            log.len()
        ),
    );
    (state, offset, on_disk, file, a, log.len())
}

#[tokio::test]
async fn v08_wrong_content_range_is_rejected() {
    let (state, offset, on_disk, file, _, _) = response_case("V08 wrong Content-Range", |r| {
        r.ranges = RangeMode::WrongContentRange
    })
    .await;
    assert!(
        matches!(&state, TransferState::Failed(e) if e.kind == ErrorKind::InvalidRange),
        "{state:?}"
    );
    assert_eq!(on_disk, offset, "nothing appended");
    assert!(file.is_empty(), "nothing published");
}

#[tokio::test]
async fn v09_range_ignored_200_restarts_from_zero() {
    let (state, _, _, file, a, n) =
        response_case("V09 server ignores Range (200, same ETag)", |r| {
            r.ranges = RangeMode::Ignore
        })
        .await;
    assert_eq!(state, TransferState::Complete);
    assert_eq!(
        sha(&file),
        sha(&a),
        "the 200 was written from zero, never appended"
    );
    assert_eq!(n, 2);
}

#[tokio::test]
async fn v10_416_is_an_error_not_a_completion() {
    let (state, offset, on_disk, file, _, _) =
        response_case("V10 416 to the resume request", |r| {
            r.ranges = RangeMode::Always416
        })
        .await;
    assert!(
        matches!(&state, TransferState::Failed(e) if e.kind == ErrorKind::InvalidRange),
        "{state:?}"
    );
    assert_eq!(on_disk, offset);
    assert!(file.is_empty());
}

#[tokio::test]
async fn v11_encoding_change_is_rejected() {
    let (state, offset, on_disk, file, _, _) =
        response_case("V11 206 with Content-Encoding: gzip", |r| {
            r.content_encoding = Some("gzip".into())
        })
        .await;
    assert!(
        matches!(&state, TransferState::Failed(e) if e.kind == ErrorKind::InvalidRequest),
        "{state:?}"
    );
    assert_eq!(on_disk, offset);
    assert!(file.is_empty());
}

#[tokio::test]
async fn v12_resumed_body_cut_short_fails_without_publishing() {
    let (state, offset, on_disk, file, _, _) = response_case("V12 resumed body cut short", |r| {
        r.truncate_after = Some(256 * 1024);
        r.truncate_times = 1;
    })
    .await;
    assert!(
        matches!(&state, TransferState::Failed(e) if e.kind == ErrorKind::NetworkOffline),
        "{state:?}"
    );
    assert!(
        on_disk > offset,
        "validated bytes of the cut 206 were appended"
    );
    assert!(file.is_empty());
}

// ------------------------------------------------------------------------------------ partial file

#[tokio::test]
async fn v13_modified_truncated_or_deleted_partial_is_refused() {
    for (case, damage) in [
        ("V13 partial modified (same length)", 0u8),
        ("V13 partial truncated", 1),
        ("V13 partial deleted", 2),
    ] {
        let s = Server::start().await;
        let (_, r) = version_a();
        s.set("f", r);
        let dir = tempfile::tempdir().unwrap();
        let b = backend();
        let (h, _) = start_and_pause(&b, dir.path(), "att_v13", s.url("f"), 256 * 1024).await;
        let p = part(dir.path());
        match damage {
            0 => {
                let mut bytes = std::fs::read(&p).unwrap();
                bytes[100] ^= 0xFF;
                std::fs::write(&p, bytes).unwrap();
            }
            1 => {
                let f = std::fs::OpenOptions::new().write(true).open(&p).unwrap();
                f.set_len(1000).unwrap();
            }
            _ => std::fs::remove_file(&p).unwrap(),
        }
        let e = b.resume(&h).await.unwrap_err();
        report(case, &format!("{:?}", e.kind), &format!("({})", e.detail));
        assert_eq!(e.kind, ErrorKind::InvalidRange);
        assert_eq!(s.log_for("f").len(), 1, "no request sent");
        assert!(!dir.path().join("out.bin").exists());
    }
}

// ------------------------------------------------------------------------------------ lifecycle

#[tokio::test]
async fn v14_cancel_during_resume_stops_writes() {
    let s = Server::start().await;
    let (_, mut r) = version_a();
    r.slow = Some(Duration::from_millis(10));
    s.set("f", r);
    let dir = tempfile::tempdir().unwrap();
    let b = backend();
    let (h, offset) = start_and_pause(&b, dir.path(), "att_v14", s.url("f"), 256 * 1024).await;
    b.resume(&h).await.unwrap();
    let started = Instant::now();
    while b.progress(&h).await.unwrap().completed.0 < offset + 128 * 1024 {
        assert!(started.elapsed() < Duration::from_secs(20));
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    b.remove(&h).await.unwrap();
    let at_ack = std::fs::metadata(part(dir.path())).unwrap().len();
    tokio::time::sleep(Duration::from_millis(300)).await;
    let later = std::fs::metadata(part(dir.path())).unwrap().len();
    report(
        "V14 cancel during a resumed transfer",
        "Removed",
        &format!("({at_ack} B at ack, {later} B later)"),
    );
    assert!(at_ack > offset);
    assert_eq!(at_ack, later, "no writes after remove returned");
    assert_eq!(
        b.progress(&h).await.unwrap_err().kind,
        ErrorKind::InvalidRequest,
        "cannot be revived"
    );
    assert!(b.resume(&h).await.is_err());
}

#[tokio::test]
async fn v15_resume_goes_to_the_final_resource_with_scoped_headers() {
    let origin = Server::start().await;
    let cdn = Server::start().await;
    let (a, r) = version_a();
    cdn.set("real", r);
    let mut hop = Route::new(Vec::new());
    hop.redirect = Some((302, cdn.url("real")));
    origin.set("start", hop);
    let dir = tempfile::tempdir().unwrap();
    let b = backend();
    let mut req = request(dir.path(), "att_v15", origin.url("start"));
    req.headers.push((
        "Authorization".into(),
        Secret::new(String::from("Bearer SYNTHETIC-SECRET-v15")),
    ));
    let h = b.start(req).await.unwrap();
    let (_, offset) = pause_after(&b, &h, dir.path(), 512 * 1024).await;
    cdn.update("real", |r| r.slow = None);
    b.resume(&h).await.unwrap();
    let state = settle(&b, &h).await;
    let file = std::fs::read(part(dir.path())).unwrap_or_default();
    let log = cdn.log_for("real");
    let resumed = resume_request(&log, offset);
    report(
        "V15 resume after a cross-origin redirect",
        &format!("{state:?}"),
        &format!(
            "(origin requests {}; CDN Authorization {:?})",
            origin.log_for("start").len(),
            resumed.header("authorization")
        ),
    );
    assert_eq!(state, TransferState::Complete);
    assert_eq!(sha(&file), sha(&a));
    assert_eq!(
        origin.log_for("start").len(),
        1,
        "resume goes straight to the final resource"
    );
    assert_eq!(
        resumed.header("if-range"),
        Some(ETAG),
        "the validator belongs to the final resource"
    );
    assert!(
        log.iter().all(|l| l.header("authorization").is_none()),
        "credentials stay with their origin"
    );
}
