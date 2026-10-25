//! B0.4 HTTP correctness matrix against a real aria2c (docs/reports/b0.4-http-correctness.md).
//!
//! Oracle: the SHA-256 of the completed file against the expected payload, plus a prefix/suffix
//! check that exposes mixed old/new bytes when sizes match.
//!
//! - `SafeSuccess` — engine reported Complete and the bytes are exactly the expected payload.
//! - `SafeFailure` — engine reported a failure (no false completion), with its classified kind.
//! - `Unsafe`      — engine reported Complete with wrong/mixed bytes (or leaked a credential).
//!
//! Two configurations are exercised:
//! - **engine** (`max_tries = 5`): characterizes raw aria2 behaviour with internal retries;
//! - **production** (adapter defaults, `max_tries = 1`, no credential headers): what dm-service
//!   actually runs.
//!
//! Every test asserts the *observed, documented* outcome. Unsafe engine outcomes are asserted as
//! unsafe on purpose — characterization tests that fail loudly if aria2's behaviour changes.
//!
//! Needs aria2c: `#[ignore]`d in plain `cargo test`, run by `pnpm verify` (group `aria2`).

#![allow(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    clippy::print_stdout
)]

mod support;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use dm_backend_api::{BackendHandle, DownloadBackend, TransferRequest, TransferState};
use dm_backend_aria2::binary::TlsBackend;
use dm_backend_aria2::{Aria2Backend, LaunchConfig, binary};
use dm_domain::redact::Secret;
use dm_domain::{AttemptId, ErrorKind};
use sha2::{Digest, Sha256};
use support::{Preconditions, RangeMode, Route, Scripted, Server, payload, payload_seeded};

const MIB: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Outcome {
    SafeSuccess,
    SafeFailure(ErrorKind),
    Unsafe(String),
    StillRunning,
}

struct Harness {
    backend: Aria2Backend,
    downloads: tempfile::TempDir,
    _work: tempfile::TempDir,
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

async fn harness_in(downloads: tempfile::TempDir, f: impl FnOnce(&mut LaunchConfig)) -> Harness {
    let bin = binary::locate(Some(&root()))
        .await
        .expect("aria2c must be available");
    let work = tempfile::tempdir().unwrap();
    let mut config = LaunchConfig::new(bin, work.path());
    config.retry_wait_secs = 0; // timing only; retry *count* is the production default unless overridden
    config.min_split_size = "1M".into();
    f(&mut config);
    let backend = Aria2Backend::launch(config).await.unwrap();
    Harness {
        backend,
        downloads,
        _work: work,
    }
}

/// Production adapter configuration (what dm-service runs).
async fn production() -> Harness {
    harness_in(tempfile::tempdir().unwrap(), |_| {}).await
}

/// Raw engine characterization: internal retries enabled.
async fn engine() -> Harness {
    harness_in(tempfile::tempdir().unwrap(), |c| c.max_tries = 5).await
}

fn request(h: &Harness, attempt: &str, url: String, conns: u8) -> TransferRequest {
    TransferRequest {
        attempt_id: AttemptId::parse(attempt).unwrap(),
        url,
        headers: Vec::new(),
        destination_dir: h.downloads.path().to_owned(),
        file_name: "out.bin".into(),
        max_connections: conns,
    }
}

async fn settle(h: &Harness, handle: &BackendHandle, limit: Duration) -> TransferState {
    let started = Instant::now();
    loop {
        let p = h.backend.progress(handle).await.unwrap();
        if !matches!(p.state, TransferState::Active | TransferState::Waiting)
            || started.elapsed() > limit
        {
            return p.state;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn sha(b: &[u8]) -> Vec<u8> {
    Sha256::digest(b).to_vec()
}

fn classify(state: &TransferState, file: &Path, expected: &[u8]) -> Outcome {
    match state {
        TransferState::Complete => {
            let got = std::fs::read(file).unwrap_or_default();
            if sha(&got) == sha(expected) {
                Outcome::SafeSuccess
            } else {
                Outcome::Unsafe(format!(
                    "completed with wrong bytes (len {} vs {})",
                    got.len(),
                    expected.len()
                ))
            }
        }
        TransferState::Failed(e) => Outcome::SafeFailure(e.kind),
        TransferState::Active | TransferState::Waiting => Outcome::StillRunning,
        other => Outcome::Unsafe(format!("unexpected terminal state {other:?}")),
    }
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
        "MIXED: starts with A, ends with B"
    } else {
        "other"
    }
}

fn report(case: &str, outcome: &Outcome, note: &str) {
    println!("CASE {case:<44} {outcome:?} {note}");
}

async fn run(
    h: &Harness,
    attempt: &str,
    url: String,
    conns: u8,
    expected: &[u8],
) -> (Outcome, Vec<u8>) {
    let handle = h
        .backend
        .start(request(h, attempt, url, conns))
        .await
        .unwrap();
    let state = settle(h, &handle, Duration::from_secs(45)).await;
    let path = h.downloads.path().join("out.bin");
    (
        classify(&state, &path, expected),
        std::fs::read(&path).unwrap_or_default(),
    )
}

// ---------------------------------------------------------------- ranges (production config)

async fn range_case(case: &str, mode: RangeMode, conns: u8) -> Outcome {
    let s = Server::start().await;
    let body = payload(4 * MIB);
    let mut r = Route::new(body.clone());
    r.ranges = mode;
    s.set("f", r);
    let h = production().await;
    let (o, _) = run(&h, "att_rng", s.url("f"), conns, &body).await;
    let ranged = s
        .log_for("f")
        .iter()
        .filter(|l| l.header("range").is_some())
        .count();
    report(case, &o, &format!("({ranged} ranged requests)"));
    h.backend.shutdown().await.unwrap();
    o
}

#[tokio::test]
#[ignore = "needs aria2c; run via `pnpm verify` (group aria2)"]
async fn r01_correct_206() {
    assert_eq!(
        range_case("R01 correct 206, 4 connections", RangeMode::Honour, 4).await,
        Outcome::SafeSuccess
    );
    assert_eq!(
        range_case("R01b correct 206, 1 connection", RangeMode::Honour, 1).await,
        Outcome::SafeSuccess
    );
}

#[tokio::test]
#[ignore = "needs aria2c; run via `pnpm verify` (group aria2)"]
async fn r02_range_ignored_200() {
    assert_eq!(
        range_case(
            "R02 range ignored (200), 4 connections",
            RangeMode::Ignore,
            4
        )
        .await,
        Outcome::SafeSuccess
    );
}

#[tokio::test]
#[ignore = "needs aria2c; run via `pnpm verify` (group aria2)"]
async fn r03_wrong_content_range() {
    assert_eq!(
        range_case(
            "R03 wrong Content-Range, 4 connections",
            RangeMode::WrongContentRange,
            4
        )
        .await,
        Outcome::SafeSuccess
    );
}

#[tokio::test]
#[ignore = "needs aria2c; run via `pnpm verify` (group aria2)"]
async fn r04_always_416() {
    assert_eq!(
        range_case(
            "R04 416 on every Range, 4 connections",
            RangeMode::Always416,
            4
        )
        .await,
        Outcome::SafeSuccess
    );
}

// ---------------------------------------------------------------- lengths

async fn truncated_once(h: &Harness, case: &str) -> Outcome {
    let s = Server::start().await;
    let body = payload(3 * MIB);
    let mut r = Route::new(body.clone());
    r.truncate_after = Some(MIB);
    r.truncate_times = 1;
    s.set("f", r);
    let (o, _) = run(h, "att_l01", s.url("f"), 1, &body).await;
    let reqs = s.log_for("f");
    let if_range = reqs.iter().any(|l| l.header("if-range").is_some());
    report(
        case,
        &o,
        &format!("({} requests, If-Range sent: {if_range})", reqs.len()),
    );
    o
}

#[tokio::test]
#[ignore = "needs aria2c; run via `pnpm verify` (group aria2)"]
async fn l01_truncated_once() {
    let h = engine().await;
    assert_eq!(
        truncated_once(&h, "L01 truncated once [engine max-tries 5]").await,
        Outcome::SafeSuccess
    );
    h.backend.shutdown().await.unwrap();
    let h = production().await;
    assert_eq!(
        truncated_once(&h, "L01 truncated once [production]").await,
        Outcome::SafeFailure(ErrorKind::NetworkOffline)
    );
    h.backend.shutdown().await.unwrap();
}

#[tokio::test]
#[ignore = "needs aria2c; run via `pnpm verify` (group aria2)"]
async fn l02_unknown_length() {
    let s = Server::start().await;
    let body = payload(2 * MIB);
    let mut r = Route::new(body.clone());
    r.content_length = false;
    r.ranges = RangeMode::Ignore;
    s.set("f", r);
    let h = production().await;
    let (o, _) = run(&h, "att_l02", s.url("f"), 1, &body).await;
    report("L02 unknown length (close-delimited)", &o, "");
    assert_eq!(o, Outcome::SafeSuccess);
    h.backend.shutdown().await.unwrap();
}

#[tokio::test]
#[ignore = "needs aria2c; run via `pnpm verify` (group aria2)"]
async fn l03_unknown_length_truncated() {
    let s = Server::start().await;
    let body = payload(2 * MIB);
    let mut r = Route::new(body.clone());
    r.content_length = false;
    r.ranges = RangeMode::Ignore;
    r.truncate_after = Some(MIB);
    s.set("f", r);
    let h = production().await;
    let (o, _) = run(&h, "att_l03", s.url("f"), 1, &body).await;
    report(
        "L03 unknown length, cut short",
        &o,
        "(protocol-inherent; only a trusted hash detects it)",
    );
    assert!(matches!(o, Outcome::Unsafe(_)), "{o:?}");
    h.backend.shutdown().await.unwrap();
}

#[tokio::test]
#[ignore = "needs aria2c; run via `pnpm verify` (group aria2)"]
async fn l04_zero_bytes() {
    let s = Server::start().await;
    s.set("f", Route::new(Vec::new()));
    let h = production().await;
    let (o, file) = run(&h, "att_l04", s.url("f"), 1, &[]).await;
    report(
        "L04 zero-byte file",
        &o,
        &format!("(file len {})", file.len()),
    );
    assert_eq!(o, Outcome::SafeSuccess);
    h.backend.shutdown().await.unwrap();
}

// ---------------------------------------------------------------- changed resources

/// What the server actually served in a changed-resource case: the scenario is only proven when
/// both representations were served to the transfer.
struct Served {
    requests: usize,
    a: usize,
    b: usize,
    /// Ranged requests answered by version B (the continuation or another segment).
    b_ranged: usize,
}

impl Served {
    fn of(log: &[support::Logged], b_tag: &str) -> Self {
        let tag = |l: &support::Logged| l.served.clone().unwrap_or_default();
        Self {
            requests: log.len(),
            a: log.iter().filter(|l| tag(l) == "A").count(),
            b: log.iter().filter(|l| tag(l) == b_tag).count(),
            b_ranged: log
                .iter()
                .filter(|l| tag(l) == b_tag && l.header("range").is_some())
                .count(),
        }
    }

    fn proven(&self) -> bool {
        self.a > 0 && self.b_ranged > 0
    }
}

impl std::fmt::Display for Served {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} requests: original x{}, changed x{} ({} ranged)",
            self.requests, self.a, self.b, self.b_ranged
        )
    }
}

/// Version A (cut once at 1 MiB, slow so that parallel segments open) for the **first request
/// only**; every later request gets version B (same length). The switch is a fixture state
/// transition applied atomically with request accounting, not a test-side timing guess.
async fn changed_mid_transfer(
    h: &Harness,
    case: &str,
    etag_a: Option<&str>,
    etag_b: Option<&str>,
    conns: u8,
) -> (Outcome, Served) {
    let s = Server::start().await;
    let a = payload_seeded(3 * MIB, 1);
    let b = payload_seeded(3 * MIB, 2);
    let mut ra = Route::new(a.clone());
    ra.etag = etag_a.map(str::to_owned);
    ra.truncate_after = Some(MIB);
    ra.truncate_times = 1;
    ra.slow = Some(Duration::from_millis(5));
    let mut rb = Route::new(b.clone());
    rb.etag = etag_b.map(str::to_owned);
    rb.tag = "B".into();
    s.set("f", ra.then_after(1, rb));
    let handle = h
        .backend
        .start(request(h, "att_chg", s.url("f"), conns))
        .await
        .unwrap();
    let state = settle(h, &handle, Duration::from_secs(45)).await;
    let path = h.downloads.path().join("out.bin");
    let file = std::fs::read(&path).unwrap_or_default();
    let reqs = s.log_for("f");
    let served = Served::of(&reqs, "B");
    let if_range = reqs
        .iter()
        .filter(|l| l.header("if-range").is_some())
        .count();
    let o = match classify(&state, &path, &a) {
        Outcome::Unsafe(_) if file == b => Outcome::SafeSuccess, // a clean restart onto B is fine
        other => other,
    };
    report(
        case,
        &o,
        &format!(
            "({}; {served}; If-Range sent {if_range}x; engine {})",
            mixture(&file, &a, &b),
            h.backend.version()
        ),
    );
    (o, served)
}

#[tokio::test]
#[ignore = "needs aria2c; run via `pnpm verify` (group aria2)"]
async fn c01_to_c04_changed_resource_engine_mixes_versions() {
    for (case, ea, eb) in [
        (
            "C01 changed, strong ETag v1->v2 [engine]",
            Some("\"v1\""),
            Some("\"v2\""),
        ),
        (
            "C02 changed, ETag unchanged (lying) [engine]",
            Some("\"v1\""),
            Some("\"v1\""),
        ),
        (
            "C03 changed, weak ETag [engine]",
            Some("W/\"v1\""),
            Some("W/\"v2\""),
        ),
        ("C04 changed, no ETag [engine]", None, None),
    ] {
        let h = engine().await;
        let (o, served) = changed_mid_transfer(&h, case, ea, eb, 1).await;
        assert!(served.proven(), "{case}: scenario not exercised ({served})");
        assert!(
            matches!(o, Outcome::Unsafe(_)),
            "{case}: expected the engine to mix versions, got {o:?}"
        );
        h.backend.shutdown().await.unwrap();
    }
}

#[tokio::test]
#[ignore = "needs aria2c; run via `pnpm verify` (group aria2)"]
async fn c05_changed_during_segmented_transfer_engine() {
    let h = engine().await;
    let (o, served) = changed_mid_transfer(
        &h,
        "C05 changed, 4 connections [engine]",
        Some("\"v1\""),
        Some("\"v2\""),
        4,
    )
    .await;
    // Proven first: version A served the first request, version B served at least one segment.
    assert!(served.proven(), "C05 scenario not exercised ({served})");
    // aria2 compares no validator in any version, so B segments are spliced into A.
    assert!(matches!(o, Outcome::Unsafe(_)), "{o:?} ({served})");
    h.backend.shutdown().await.unwrap();
}

#[tokio::test]
#[ignore = "needs aria2c; run via `pnpm verify` (group aria2)"]
async fn c06_changed_between_pause_and_resume_engine() {
    let s = Server::start().await;
    let a = payload_seeded(3 * MIB, 3);
    let b = payload_seeded(3 * MIB, 4);
    let mut r = Route::new(a.clone());
    r.slow = Some(Duration::from_millis(20));
    s.set("f", r);
    let h = production().await; // pause/resume is engine behaviour regardless of max-tries
    let handle = h
        .backend
        .start(request(&h, "att_c06", s.url("f"), 1))
        .await
        .unwrap();
    // Pause only once bytes have arrived (bounded), so the resume is a real continuation.
    let started = Instant::now();
    while h
        .backend
        .progress(&handle)
        .await
        .is_ok_and(|p| p.completed.0 < 256 * 1024)
    {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "transfer never started"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    h.backend.pause(&handle).await.unwrap();
    let paused_at = h.backend.progress(&handle).await.unwrap().completed;
    s.update("f", |r| {
        r.body = Arc::new(b.clone());
        r.etag = Some("\"v2\"".into());
        r.slow = None;
        r.tag = "B".into();
    });
    h.backend.resume(&handle).await.unwrap();
    let state = settle(&h, &handle, Duration::from_secs(45)).await;
    let path = h.downloads.path().join("out.bin");
    let file = std::fs::read(&path).unwrap_or_default();
    let o = classify(&state, &path, &a);
    let reqs = s.log_for("f");
    let served = Served::of(&reqs, "B");
    let if_range = reqs.iter().any(|l| l.header("if-range").is_some());
    report(
        "C06 changed between pause/resume [engine]",
        &o,
        &format!(
            "(paused at {} B; {}; {served}; If-Range sent: {if_range}) — dm-service refuses this resume",
            paused_at.0,
            mixture(&file, &a, &b)
        ),
    );
    assert!(served.proven(), "C06 scenario not exercised ({served})");
    assert!(matches!(o, Outcome::Unsafe(_)), "{o:?}");
    h.backend.shutdown().await.unwrap();
}

#[tokio::test]
#[ignore = "needs aria2c; run via `pnpm verify` (group aria2)"]
async fn c07_content_encoding_changes_mid_transfer() {
    // Defined representation: the identity bytes `a`. The first response (identity) is cut at
    // 1 MiB; every later request is answered by a gzip-encoded representation of the same
    // content, so the continuation is guaranteed to see the encoding switch.
    let s = Server::start().await;
    let a = payload(2 * MIB);
    let mut identity = Route::new(a.clone());
    identity.truncate_after = Some(MIB);
    identity.truncate_times = 1;
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    gz.write_all(&a).unwrap();
    let mut encoded = Route::new(gz.finish().unwrap());
    encoded.content_encoding = Some("gzip".into());
    encoded.tag = "gzip".into();
    s.set("f", identity.then_after(1, encoded));
    let h = engine().await;
    let handle = h
        .backend
        .start(request(&h, "att_c07", s.url("f"), 1))
        .await
        .unwrap();
    let state = settle(&h, &handle, Duration::from_secs(45)).await;
    let path = h.downloads.path().join("out.bin");
    let file = std::fs::read(&path).unwrap_or_default();
    let o = classify(&state, &path, &a);
    let reqs = s.log_for("f");
    let served = Served::of(&reqs, "gzip");
    report(
        "C07 Content-Encoding changes mid-transfer [engine]",
        &o,
        &format!(
            "({served}; file {} B, identity match {}; engine {})",
            file.len(),
            file == a,
            h.backend.version()
        ),
    );
    // The scenario must have happened: identity first, then an encoded continuation.
    assert!(served.proven(), "C07 scenario not exercised ({served})");
    // Safe means: a failure, or completion with exactly the identity bytes. Never anything else.
    match &o {
        Outcome::SafeFailure(_) => {}
        Outcome::SafeSuccess => {
            assert_eq!(file, a, "completed bytes are the identity representation")
        }
        other => panic!("C07 unsafe: {other:?} ({served})"),
    }
    h.backend.shutdown().await.unwrap();
}

#[tokio::test]
#[ignore = "needs aria2c; run via `pnpm verify` (group aria2)"]
async fn c08_changed_resource_production_config_fails_safely() {
    let h = production().await;
    let (o, served) = changed_mid_transfer(
        &h,
        "C08 changed mid-transfer [production]",
        Some("\"v1\""),
        Some("\"v2\""),
        1,
    )
    .await;
    // With max-tries 1 the cut first response is the only request: B is never fetched.
    assert_eq!(served.requests, 1, "{served}");
    assert_eq!(
        o,
        Outcome::SafeFailure(ErrorKind::NetworkOffline),
        "production config must not resume silently"
    );
    h.backend.shutdown().await.unwrap();
}

// ---------------------------------------------------------------- status codes (production)

async fn status_case(case: &str, script: Vec<Scripted>) -> Outcome {
    let s = Server::start().await;
    let body = payload(MIB);
    let mut r = Route::new(body.clone());
    r.script = script.into();
    s.set("f", r);
    let h = production().await;
    let start = Instant::now();
    let (o, file) = run(&h, "att_st", s.url("f"), 1, &body).await;
    report(
        case,
        &o,
        &format!(
            "({} requests in {:?}; file {} B)",
            s.log_for("f").len(),
            start.elapsed(),
            file.len()
        ),
    );
    h.backend.shutdown().await.unwrap();
    o
}

#[tokio::test]
#[ignore = "needs aria2c; run via `pnpm verify` (group aria2)"]
async fn s01_503_retry_after() {
    let o = status_case(
        "S01 503 + Retry-After: 30",
        vec![Scripted::status(503, "Service Unavailable").header("Retry-After", "30")],
    )
    .await;
    assert_eq!(o, Outcome::SafeFailure(ErrorKind::ServerBusy));
}

#[tokio::test]
#[ignore = "needs aria2c; run via `pnpm verify` (group aria2)"]
async fn s02_429_retry_after() {
    let o = status_case(
        "S02 429 + Retry-After: 30",
        vec![Scripted::status(429, "Too Many Requests").header("Retry-After", "30")],
    )
    .await;
    assert_eq!(o, Outcome::SafeFailure(ErrorKind::ServerBusy));
}

#[tokio::test]
#[ignore = "needs aria2c; run via `pnpm verify` (group aria2)"]
async fn s03_401_403_410() {
    for (code, reason, kind) in [
        (401, "Unauthorized", ErrorKind::AuthRequired),
        (403, "Forbidden", ErrorKind::LinkRefreshRequired),
        (410, "Gone", ErrorKind::LinkRefreshRequired),
    ] {
        let o = status_case(
            &format!("S03 {code} {reason}"),
            vec![Scripted::status(code, reason); 4],
        )
        .await;
        assert_eq!(o, Outcome::SafeFailure(kind), "{code}");
    }
}

// ---------------------------------------------------------------- redirects & credentials

const REDIRECT_SECRET: &str = "SYNTHETIC-SECRET-b04-redirect";

async fn redirect_case(
    h: &Harness,
    case: &str,
    cross_origin: bool,
    with_credentials: bool,
) -> (Result<Outcome, ErrorKind>, bool) {
    let origin = Server::start().await;
    let target = if cross_origin {
        Server::start().await
    } else {
        origin.clone()
    };
    let body = payload(MIB);
    target.set("real", Route::new(body.clone()));
    let mut hop = Route::new(Vec::new());
    hop.redirect = Some((302, target.url("real")));
    origin.set("start", hop);
    let mut req = request(h, "att_redir", origin.url("start"), 1);
    if with_credentials {
        req.headers.push((
            "Authorization".into(),
            Secret::new(format!("Bearer {REDIRECT_SECRET}")),
        ));
        req.headers.push((
            "Cookie".into(),
            Secret::new(format!("session={REDIRECT_SECRET}")),
        ));
    }
    let handle = match h.backend.start(req).await {
        Ok(handle) => handle,
        Err(e) => {
            report(
                case,
                &Outcome::SafeFailure(e.kind),
                "(refused before any request)",
            );
            return (Err(e.kind), false);
        }
    };
    let state = settle(h, &handle, Duration::from_secs(45)).await;
    let o = classify(&state, &h.downloads.path().join("out.bin"), &body);
    let leaked = target.log_for("real").iter().any(|l| {
        [l.header("authorization"), l.header("cookie")]
            .into_iter()
            .flatten()
            .any(|v| v.contains(REDIRECT_SECRET))
    });
    report(
        case,
        &o,
        &format!("(credentials seen by redirect target: {leaked})"),
    );
    (Ok(o), leaked)
}

#[tokio::test]
#[ignore = "needs aria2c; run via `pnpm verify` (group aria2)"]
async fn d01_same_origin_redirect_without_credentials() {
    let h = production().await;
    let (o, _) = redirect_case(&h, "D01 302 same origin [production]", false, false).await;
    assert_eq!(o, Ok(Outcome::SafeSuccess));
    h.backend.shutdown().await.unwrap();
}

#[tokio::test]
#[ignore = "needs aria2c; run via `pnpm verify` (group aria2)"]
async fn d02_cross_origin_redirect_leaks_engine_headers() {
    let h = harness_in(tempfile::tempdir().unwrap(), |c| {
        c.forward_credential_headers_unsafe = true
    })
    .await;
    let (o, leaked) =
        redirect_case(&h, "D02 302 cross origin with creds [engine]", true, true).await;
    assert_eq!(o, Ok(Outcome::SafeSuccess));
    assert!(
        leaked,
        "characterization: aria2 forwards --header values to other origins"
    );
    h.backend.shutdown().await.unwrap();
}

#[tokio::test]
#[ignore = "needs aria2c; run via `pnpm verify` (group aria2)"]
async fn d03_production_refuses_credential_headers() {
    let h = production().await;
    let (o, leaked) = redirect_case(&h, "D03 credentials refused [production]", true, true).await;
    assert_eq!(o, Err(ErrorKind::BackendCapabilityUnavailable));
    assert!(!leaked);
    h.backend.shutdown().await.unwrap();
}

#[tokio::test]
#[ignore = "needs aria2c; run via `pnpm verify` (group aria2)"]
async fn d04_http_to_https_redirect_bypasses_the_https_gate() {
    // aria2 follows http -> https redirects; the service's Https gate sees only the first URL.
    // Certificate checks still apply (here: self-signed -> refused before any HTTP request).
    let origin = Server::start().await;
    let target = Server::start_tls(support::tls::self_signed(&["localhost", "127.0.0.1"])).await;
    let body = payload(MIB);
    target.set("real", Route::new(body.clone()));
    let mut hop = Route::new(Vec::new());
    hop.redirect = Some((302, target.url("real")));
    origin.set("start", hop);
    let h = production().await;
    let (o, _) = run(&h, "att_d04", origin.url("start"), 1, &body).await;
    report(
        "D04 http -> https redirect [production]",
        &o,
        &format!(
            "(TLS target saw {} HTTP requests)",
            target.log_for("real").len()
        ),
    );
    assert_eq!(o, Outcome::SafeFailure(ErrorKind::TlsHandshakeFailed));
    assert!(target.log_for("real").is_empty());
    h.backend.shutdown().await.unwrap();
}

#[tokio::test]
#[ignore = "needs aria2c; run via `pnpm verify` (group aria2)"]
async fn d05_userinfo_is_not_sent_to_a_cross_origin_redirect_target() {
    let origin = Server::start().await;
    let target = Server::start().await;
    let body = payload(MIB);
    target.set("real", Route::new(body.clone()));
    let mut hop = Route::new(Vec::new());
    hop.redirect = Some((302, target.url("real")));
    origin.set("start", hop);
    let url =
        origin
            .url("start")
            .replacen("http://", &format!("http://user:{REDIRECT_SECRET}@"), 1);
    let h = production().await;
    let (o, _) = run(&h, "att_d05", url, 1, &body).await;
    let origin_auth = origin
        .log_for("start")
        .iter()
        .any(|l| l.header("authorization").is_some());
    let leaked = target
        .log_for("real")
        .iter()
        .any(|l| l.header("authorization").is_some());
    report(
        "D05 userinfo URL, cross-origin redirect [production]",
        &o,
        &format!(
            "(origin got Authorization: {origin_auth}; redirect target got Authorization: {leaked})"
        ),
    );
    assert_eq!(o, Outcome::SafeSuccess);
    assert!(origin_auth, "userinfo becomes Basic auth for the origin");
    assert!(!leaked, "userinfo must not follow a cross-origin redirect");
    h.backend.shutdown().await.unwrap();
}

// ---------------------------------------------------------------- If-Match through --header (B0.4.1)
//
// Question: can a strong-ETag `If-Match` passed through the existing per-download header
// interface make aria2's validator-less resume and segmentation safe? Every case runs with
// engine-internal retries enabled (max-tries 5), which is the resume path we would want to
// unlock, and with the ETag supplied up front, as a preflight would. The fixture evaluates
// If-Match with strong comparison unless told to ignore it.

const ETAG_A: &str = "\"v1\"";
const ETAG_B: &str = "\"v2\"";

fn if_match_request(
    h: &Harness,
    attempt: &str,
    url: String,
    conns: u8,
    etag: &str,
) -> TransferRequest {
    let mut req = request(h, attempt, url, conns);
    req.headers
        .push(("If-Match".into(), Secret::new(etag.to_owned())));
    req
}

struct IfMatchRun {
    outcome: Outcome,
    requests: usize,
    with_if_match: usize,
    /// Requests answered by the post-transition state (tag "after").
    served: Served,
}

/// Version A (ETag v1) serves the first request only and cuts it at 1 MiB; every later request is
/// answered by the route as changed by `after` (a fixture state transition, tag "after").
async fn if_match_case(
    case: &str,
    conns: u8,
    precondition: Preconditions,
    after: impl FnOnce(&mut Route, Vec<u8>),
) -> IfMatchRun {
    let s = Server::start().await;
    let a = payload_seeded(3 * MIB, 11);
    let b = payload_seeded(3 * MIB, 12);
    let mut r = Route::new(a.clone());
    r.etag = Some(ETAG_A.into());
    r.truncate_after = Some(MIB);
    r.truncate_times = 1;
    r.slow = Some(Duration::from_millis(5));
    r.if_match = precondition;
    let mut next = r.clone();
    next.truncate_after = None;
    next.slow = None;
    next.tag = "after".into();
    after(&mut next, b.clone());
    s.set("f", r.then_after(1, next));
    let h = engine().await;
    let handle = h
        .backend
        .start(if_match_request(&h, "att_im", s.url("f"), conns, ETAG_A))
        .await
        .unwrap();
    let state = settle(&h, &handle, Duration::from_secs(45)).await;
    let path = h.downloads.path().join("out.bin");
    let file = std::fs::read(&path).unwrap_or_default();
    let outcome = classify(&state, &path, &a);
    let reqs = s.log_for("f");
    let run = IfMatchRun {
        requests: reqs.len(),
        with_if_match: reqs
            .iter()
            .filter(|l| l.header("if-match") == Some(ETAG_A))
            .count(),
        served: Served::of(&reqs, "after"),
        outcome,
    };
    report(
        case,
        &run.outcome,
        &format!(
            "({}; {} requests, {} with If-Match; {})",
            mixture(&file, &a, &b),
            run.requests,
            run.with_if_match,
            run.served
        ),
    );
    assert!(
        run.served.proven(),
        "{case}: scenario not exercised ({})",
        run.served
    );
    h.backend.shutdown().await.unwrap();
    run
}

fn change_to_b(r: &mut Route, b: Vec<u8>) {
    r.body = Arc::new(b);
    r.etag = Some(ETAG_B.into());
    r.slow = None;
}

#[tokio::test]
#[ignore = "needs aria2c; run via `pnpm verify` (group aria2)"]
async fn m01_if_match_unchanged_interrupted_resumes_exactly() {
    let run = if_match_case(
        "M01 If-Match, unchanged, cut once, internal resume",
        1,
        Preconditions::Honour,
        |r, _| r.slow = None,
    )
    .await;
    assert_eq!(run.outcome, Outcome::SafeSuccess);
    assert_eq!(
        run.with_if_match, run.requests,
        "every request carries If-Match"
    );
}

#[tokio::test]
#[ignore = "needs aria2c; run via `pnpm verify` (group aria2)"]
async fn m02_if_match_same_length_change_fails_with_412() {
    let run = if_match_case(
        "M02 If-Match, same-length change, 1 conn",
        1,
        Preconditions::Honour,
        change_to_b,
    )
    .await;
    // Safe, but the 412 is not identifiable: on a resumed request aria2 reports only
    // "No URI available." (code 22, no status), which classifies as INVALID_RANGE.
    assert_eq!(run.outcome, Outcome::SafeFailure(ErrorKind::InvalidRange));
}

#[tokio::test]
#[ignore = "needs aria2c; run via `pnpm verify` (group aria2)"]
async fn m03_if_match_change_during_segmented_transfer() {
    let run = if_match_case(
        "M03 If-Match, same-length change, 4 conns",
        4,
        Preconditions::Honour,
        change_to_b,
    )
    .await;
    assert!(
        !matches!(run.outcome, Outcome::Unsafe(_)),
        "segments must not mix: {:?}",
        run.outcome
    );
    assert_eq!(
        run.with_if_match, run.requests,
        "every segment request carries If-Match"
    );
}

#[tokio::test]
#[ignore = "needs aria2c; run via `pnpm verify` (group aria2)"]
async fn m04_ignored_if_match_still_mixes_versions() {
    let run = if_match_case(
        "M04 If-Match IGNORED by server, same-length change",
        1,
        Preconditions::Ignore,
        change_to_b,
    )
    .await;
    // aria2 never checks the response ETag, so nothing on the client side notices.
    assert!(
        matches!(run.outcome, Outcome::Unsafe(_)),
        "{:?}",
        run.outcome
    );
    assert_eq!(run.with_if_match, run.requests);
}

#[tokio::test]
#[ignore = "needs aria2c; run via `pnpm verify` (group aria2)"]
async fn m05_if_match_wrong_content_range() {
    let run = if_match_case(
        "M05 If-Match, wrong Content-Range on resume",
        1,
        Preconditions::Honour,
        |r, _| {
            r.slow = None;
            r.ranges = RangeMode::WrongContentRange;
        },
    )
    .await;
    assert!(
        !matches!(run.outcome, Outcome::Unsafe(_)),
        "{:?}",
        run.outcome
    );
}

#[tokio::test]
#[ignore = "needs aria2c; run via `pnpm verify` (group aria2)"]
async fn m06_if_match_412_on_the_first_request() {
    // The representation changed between a preflight and the transfer.
    let s = Server::start().await;
    let b = payload_seeded(MIB, 13);
    let mut r = Route::new(b.clone());
    r.etag = Some(ETAG_B.into());
    s.set("f", r);
    let h = engine().await;
    let handle = h
        .backend
        .start(if_match_request(&h, "att_m06", s.url("f"), 1, ETAG_A))
        .await
        .unwrap();
    let state = settle(&h, &handle, Duration::from_secs(30)).await;
    let o = classify(&state, &h.downloads.path().join("out.bin"), &b);
    report(
        "M06 If-Match, ETag already changed",
        &o,
        &format!("({} requests)", s.log_for("f").len()),
    );
    assert_eq!(o, Outcome::SafeFailure(ErrorKind::RemoteChanged));
    h.backend.shutdown().await.unwrap();
}

#[tokio::test]
#[ignore = "needs aria2c; run via `pnpm verify` (group aria2)"]
async fn m07_if_match_change_between_pause_and_resume() {
    let s = Server::start().await;
    let a = payload_seeded(3 * MIB, 14);
    let b = payload_seeded(3 * MIB, 15);
    let mut r = Route::new(a.clone());
    r.etag = Some(ETAG_A.into());
    r.slow = Some(Duration::from_millis(20));
    s.set("f", r);
    let h = engine().await;
    let handle = h
        .backend
        .start(if_match_request(&h, "att_m07", s.url("f"), 1, ETAG_A))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(800)).await;
    h.backend.pause(&handle).await.unwrap();
    s.update("f", |r| change_to_b(r, b.clone()));
    h.backend.resume(&handle).await.unwrap();
    let state = settle(&h, &handle, Duration::from_secs(45)).await;
    let path = h.downloads.path().join("out.bin");
    let file = std::fs::read(&path).unwrap_or_default();
    let o = classify(&state, &path, &a);
    report(
        "M07 If-Match, change between pause/resume",
        &o,
        &format!("({})", mixture(&file, &a, &b)),
    );
    // Safe failure; the status is lost on the resumed request, as in M02.
    assert_eq!(o, Outcome::SafeFailure(ErrorKind::InvalidRange));
    h.backend.shutdown().await.unwrap();
}

#[tokio::test]
#[ignore = "needs aria2c; run via `pnpm verify` (group aria2)"]
async fn m08_weak_etag_cannot_be_used_with_if_match() {
    // Strong comparison: a weak ETag never matches, so even an unchanged file fails.
    let s = Server::start().await;
    let a = payload(MIB);
    let mut r = Route::new(a.clone());
    r.etag = Some("W/\"v1\"".into());
    s.set("f", r);
    let h = engine().await;
    let handle = h
        .backend
        .start(if_match_request(&h, "att_m08", s.url("f"), 1, "W/\"v1\""))
        .await
        .unwrap();
    let state = settle(&h, &handle, Duration::from_secs(30)).await;
    let o = classify(&state, &h.downloads.path().join("out.bin"), &a);
    report("M08 If-Match with a weak ETag, unchanged", &o, "");
    assert_eq!(o, Outcome::SafeFailure(ErrorKind::RemoteChanged));
    h.backend.shutdown().await.unwrap();
}

#[tokio::test]
#[ignore = "needs aria2c; run via `pnpm verify` (group aria2)"]
async fn m09_if_match_follows_a_cross_origin_redirect() {
    // A CDN hop has its own ETag: the origin's If-Match is forwarded and fails there.
    let origin = Server::start().await;
    let cdn = Server::start().await;
    let a = payload(MIB);
    let mut real = Route::new(a.clone());
    real.etag = Some("\"cdn-7\"".into());
    cdn.set("real", real);
    let mut hop = Route::new(Vec::new());
    hop.redirect = Some((302, cdn.url("real")));
    origin.set("start", hop);
    let h = engine().await;
    let handle = h
        .backend
        .start(if_match_request(
            &h,
            "att_m09",
            origin.url("start"),
            1,
            ETAG_A,
        ))
        .await
        .unwrap();
    let state = settle(&h, &handle, Duration::from_secs(30)).await;
    let o = classify(&state, &h.downloads.path().join("out.bin"), &a);
    let forwarded = cdn
        .log_for("real")
        .iter()
        .any(|l| l.header("if-match").is_some());
    report(
        "M09 If-Match across a cross-origin redirect",
        &o,
        &format!("(If-Match forwarded to the CDN: {forwarded})"),
    );
    assert!(forwarded);
    assert_eq!(o, Outcome::SafeFailure(ErrorKind::RemoteChanged));
    h.backend.shutdown().await.unwrap();
}

#[tokio::test]
#[ignore = "needs aria2c; run via `pnpm verify` (group aria2)"]
async fn m10_if_match_unchanged_segmented_succeeds() {
    let s = Server::start().await;
    let a = payload_seeded(4 * MIB, 16);
    let mut r = Route::new(a.clone());
    r.etag = Some(ETAG_A.into());
    s.set("f", r);
    let h = engine().await;
    let handle = h
        .backend
        .start(if_match_request(&h, "att_m10", s.url("f"), 4, ETAG_A))
        .await
        .unwrap();
    let state = settle(&h, &handle, Duration::from_secs(30)).await;
    let o = classify(&state, &h.downloads.path().join("out.bin"), &a);
    let reqs = s.log_for("f");
    let ranged = reqs.iter().filter(|l| l.header("range").is_some()).count();
    let with = reqs
        .iter()
        .filter(|l| l.header("if-match") == Some(ETAG_A))
        .count();
    report(
        "M10 If-Match, unchanged, 4 conns",
        &o,
        &format!(
            "({} requests, {ranged} ranged, {with} with If-Match)",
            reqs.len()
        ),
    );
    assert_eq!(o, Outcome::SafeSuccess);
    assert_eq!(with, reqs.len());
    h.backend.shutdown().await.unwrap();
}

// ---------------------------------------------------------------- filenames

#[tokio::test]
#[ignore = "needs aria2c; run via `pnpm verify` (group aria2)"]
async fn n01_content_disposition_is_ignored_for_naming() {
    let s = Server::start().await;
    let body = payload(256 * 1024);
    let mut r = Route::new(body.clone());
    r.content_disposition =
        Some("attachment; filename=\"evil.exe\"; filename*=UTF-8''..%2F..%2F%E0%A6%AC%E0%A6%87%2Fevil.exe".into());
    s.set("f", r);
    let h = production().await;
    let (o, _) = run(&h, "att_n01", s.url("f"), 1, &body).await;
    let names: Vec<String> = std::fs::read_dir(h.downloads.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    let escaped = h
        .downloads
        .path()
        .parent()
        .unwrap()
        .join("evil.exe")
        .exists();
    report(
        "N01 Content-Disposition filename* (unsafe)",
        &o,
        &format!("(files: {names:?}; escaped: {escaped})"),
    );
    assert_eq!(o, Outcome::SafeSuccess);
    assert_eq!(
        names,
        vec!["out.bin".to_owned()],
        "service-chosen name only"
    );
    assert!(!escaped);
    h.backend.shutdown().await.unwrap();
}

// ---------------------------------------------------------------- TLS

fn write_ca(dir: &Path, pem: &str) -> PathBuf {
    let path = dir.join("test-ca.pem");
    std::fs::write(&path, pem).unwrap();
    path
}

async fn tls_backend() -> TlsBackend {
    binary::locate(Some(&root())).await.unwrap().tls
}

/// Launches with a CA bundle. `None` when the build cannot honour CA files and the adapter
/// (correctly) refuses to launch — asserted here so the refusal itself is evidence (B0.4.1).
async fn harness_with_ca(ca_path: &Path) -> Option<Harness> {
    let bin = binary::locate(Some(&root())).await.unwrap();
    let work = tempfile::tempdir().unwrap();
    let mut config = LaunchConfig::new(bin.clone(), work.path());
    config.retry_wait_secs = 0;
    config.ca_certificate = Some(ca_path.to_owned());
    match Aria2Backend::launch(config).await {
        Ok(backend) => {
            assert!(
                bin.tls.honours_ca_file(),
                "launch must refuse an ignored CA bundle"
            );
            Some(Harness {
                backend,
                downloads: tempfile::tempdir().unwrap(),
                _work: work,
            })
        }
        Err(e) => {
            assert!(
                !bin.tls.honours_ca_file(),
                "unexpected launch failure: {e:?}"
            );
            assert_eq!(e.kind, ErrorKind::BackendCapabilityUnavailable);
            None
        }
    }
}

#[tokio::test]
#[ignore = "needs aria2c; run via `pnpm verify` (group aria2)"]
async fn t00_tls_backend_is_identified() {
    let tls = tls_backend().await;
    println!(
        "CASE T00 TLS backend of this aria2c              {}",
        tls.describe()
    );
    if cfg!(windows) {
        // The pinned Windows build: no OpenSSL/GnuTLS, SChannel via the OS (REL-05 audit).
        assert_eq!(tls, TlsBackend::Platform);
    }
    assert_ne!(tls, TlsBackend::None, "every supported build has HTTPS");
}

#[tokio::test]
#[ignore = "needs aria2c; run via `pnpm verify` (group aria2)"]
async fn t01_https_trusted_via_ca_certificate() {
    let ca = support::tls::TestCa::new("Downova Test CA");
    let s = Server::start_tls(ca.leaf(&["localhost", "127.0.0.1"])).await;
    let body = payload(MIB);
    s.set("f", Route::new(body.clone()));
    let ca_dir = tempfile::tempdir().unwrap();
    let ca_path = write_ca(ca_dir.path(), &ca.pem());
    let tls = tls_backend().await;
    let Some(h) = harness_with_ca(&ca_path).await else {
        report(
            "T01 HTTPS, test CA via --ca-certificate",
            &Outcome::SafeFailure(ErrorKind::BackendCapabilityUnavailable),
            &format!(
                "(not testable: {} ignores CA files; launch refused instead of silently ignoring it)",
                tls.describe()
            ),
        );
        return;
    };
    let (o, _) = run(&h, "att_t01", s.url("f"), 1, &body).await;
    report(
        "T01 HTTPS, test CA via --ca-certificate",
        &o,
        &format!("(TLS backend: {})", tls.describe()),
    );
    assert_eq!(
        o,
        Outcome::SafeSuccess,
        "a build that honours CA files must trust the supplied CA"
    );
    h.backend.shutdown().await.unwrap();
}

#[tokio::test]
#[ignore = "needs aria2c; run via `pnpm verify` (group aria2)"]
async fn t02_https_self_signed_rejected() {
    let s = Server::start_tls(support::tls::self_signed(&["localhost", "127.0.0.1"])).await;
    let body = payload(MIB);
    s.set("f", Route::new(body.clone()));
    let h = production().await;
    let (o, _) = run(&h, "att_t02", s.url("f"), 1, &body).await;
    report(
        "T02 HTTPS, self-signed",
        &o,
        &format!("(HTTP requests reached server: {})", s.log_for("f").len()),
    );
    assert_eq!(o, Outcome::SafeFailure(ErrorKind::TlsHandshakeFailed));
    assert!(
        s.log_for("f").is_empty(),
        "no HTTP request over an untrusted connection"
    );
    h.backend.shutdown().await.unwrap();
}

#[tokio::test]
#[ignore = "needs aria2c; run via `pnpm verify` (group aria2)"]
async fn t03_https_wrong_hostname_rejected() {
    let ca = support::tls::TestCa::new("Downova Test CA");
    let s = Server::start_tls(ca.leaf(&["other.invalid"])).await;
    let body = payload(MIB);
    s.set("f", Route::new(body.clone()));
    let ca_dir = tempfile::tempdir().unwrap();
    let ca_path = write_ca(ca_dir.path(), &ca.pem());
    let tls = tls_backend().await;
    let Some(h) = harness_with_ca(&ca_path).await else {
        report(
            "T03 HTTPS, CA-signed but wrong host name",
            &Outcome::SafeFailure(ErrorKind::BackendCapabilityUnavailable),
            &format!(
                "(not testable: needs a trusted CA, {} ignores CA files)",
                tls.describe()
            ),
        );
        return;
    };
    let (o, _) = run(&h, "att_t03", s.url("f"), 1, &body).await;
    report(
        "T03 HTTPS, CA-signed but wrong host name",
        &o,
        &format!("(TLS backend: {})", tls.describe()),
    );
    assert_eq!(o, Outcome::SafeFailure(ErrorKind::TlsHandshakeFailed));
    assert!(s.log_for("f").is_empty());
    h.backend.shutdown().await.unwrap();
}

// ---------------------------------------------------------------- process failure

fn pid_alive(pid: u32) -> bool {
    if cfg!(windows) {
        let out = std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).contains(&pid.to_string())
    } else {
        std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .status()
            .is_ok_and(|s| s.success())
    }
}

/// "running", "zombie" (exited but not reaped by its parent) or "gone". A zombie still answers
/// `kill -0`, so [`pid_alive`] deliberately treats it as alive: an unreaped engine is a defect.
fn process_state(pid: u32) -> &'static str {
    if cfg!(windows) {
        return if pid_alive(pid) { "running" } else { "gone" };
    }
    let out = std::process::Command::new("ps")
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
        std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/F"])
            .output()
            .unwrap();
    } else {
        std::process::Command::new("kill")
            .args(["-9", &pid.to_string()])
            .status()
            .unwrap();
    }
}

#[tokio::test]
#[ignore = "needs aria2c; run via `pnpm verify` (group aria2)"]
async fn k01_k02_engine_killed_then_resumed_from_control_file() {
    let s = Server::start().await;
    let body = payload(4 * MIB);
    let mut r = Route::new(body.clone());
    r.slow = Some(Duration::from_millis(15));
    s.set("f", r);
    let h = production().await;
    let dl = h.downloads.path().to_owned();
    let pid = h.backend.pid().unwrap();
    let handle = h
        .backend
        .start(request(&h, "att_k01", s.url("f"), 1))
        .await
        .unwrap();
    // Kill only once bytes are flowing (bounded), not after a fixed delay.
    let started = Instant::now();
    while s.log_for("f").is_empty()
        || h.backend
            .progress(&handle)
            .await
            .is_ok_and(|p| p.completed.0 == 0)
    {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "transfer never started"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    kill_pid(pid);
    // Bounded synchronization on the adapter's exit monitor: Some = exited *and reaped*.
    let exit = h.backend.wait_exit(Duration::from_secs(10)).await;
    let state_after_exit = process_state(pid);
    let after = h.backend.progress(&handle).await;
    let file = std::fs::read(dl.join("out.bin")).unwrap_or_default();
    // Written bytes must never contradict the payload (unwritten regions may be zero/absent).
    let consistent =
        file.len() <= body.len() && file.iter().zip(body.iter()).all(|(f, b)| *f == 0 || f == b);
    let control = dl.join("out.bin.aria2").exists();
    println!(
        "CASE K01 engine killed mid-transfer                  progress={:?} partial={}B consistent={consistent} control_file={control} exit={exit:?} process={state_after_exit}",
        after.as_ref().map(|p| &p.state).map_err(|e| e.kind),
        file.len(),
    );
    assert!(exit.is_some(), "the adapter observes the engine exit");
    assert_eq!(
        after.unwrap_err().kind,
        ErrorKind::ServiceUnavailable,
        "engine loss is reported"
    );
    assert!(control, "an unfinished transfer keeps its control file");
    assert!(consistent, "partial bytes never contradict the payload");
    assert!(!pid_alive(pid), "no orphan engine");

    // K02: a new engine on the same directory and file name resumes from the control file.
    let h2 = harness_in(h.downloads, |_| {}).await;
    let before = s.log().len();
    let (o, _) = run(&h2, "att_k02", s.url("f"), 1, &body).await;
    let resumed_with_range = s.log()[before..].iter().any(|l| {
        l.header("range")
            .is_some_and(|r| !r.starts_with("bytes=0-"))
    });
    report(
        "K02 new engine resumes from control file",
        &o,
        &format!("(resumed with non-zero Range: {resumed_with_range})"),
    );
    assert_eq!(
        o,
        Outcome::SafeSuccess,
        "crash-resume of an unchanged resource must be byte-exact"
    );
    h2.backend.shutdown().await.unwrap();
}

#[tokio::test]
#[ignore = "needs aria2c; run via `pnpm verify` (group aria2)"]
async fn k03_existing_file_without_control_file_is_never_appended_to() {
    let s = Server::start().await;
    let body = payload(2 * MIB);
    s.set("f", Route::new(body.clone()));
    let h = production().await;
    let unrelated = payload_seeded(MIB, 99);
    std::fs::write(h.downloads.path().join("out.bin"), &unrelated).unwrap();
    let (o, file) = run(&h, "att_k03", s.url("f"), 1, &body).await;
    report(
        "K03 pre-existing file, no control file",
        &o,
        &format!("(user file unchanged: {})", file == unrelated),
    );
    assert_eq!(
        o,
        Outcome::SafeFailure(ErrorKind::InvalidRequest),
        "must refuse, not append"
    );
    assert_eq!(file, unrelated, "user file untouched");
    h.backend.shutdown().await.unwrap();
}
