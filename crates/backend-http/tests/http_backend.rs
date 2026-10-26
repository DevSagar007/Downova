//! B0.4.2 acceptance matrix for `dm-backend-http` (docs/reports/b0.4.2-http-backend.md).
//!
//! Backend-independent safety requirements, run against the shared controlled fixture server
//! (`dm-test-fixtures`) with a SHA-256 oracle. Unlike the aria2 characterization tests, these
//! need no external binary, so they run in plain `cargo test` on every OS.
//!
//! Before promotion the tests ran with `scheme_gate` = Http and Https supported (the candidate
//! report); `gate_follows_the_report` proves the gate itself with a report where Https is not.
//! Since B1.3 (ADR 0008) the backend never publishes: a finished transfer is its synced staging
//! file `out.bin.downova-part`; verification and placement at `out.bin` belong to the service.

#![allow(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    clippy::print_stdout
)]

use std::path::Path;
use std::time::{Duration, Instant};

use dm_backend_api::{BackendHandle, DownloadBackend, TransferRequest, TransferState};
use dm_backend_http::{HttpBackend, HttpConfig, PART_SUFFIX};
use dm_domain::redact::Secret;
use dm_domain::{AttemptId, Capability, CapabilityReport, CapabilitySupport, ErrorKind};
use dm_test_fixtures::{Route, Scripted, Server, payload, tls};
use sha2::{Digest, Sha256};

const MIB: usize = 1024 * 1024;
const SECRET: &str = "SYNTHETIC-SECRET-b042";

#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    SafeSuccess,
    SafeFailure(ErrorKind),
    Unsafe(String),
}

fn gate(http: CapabilitySupport, https: CapabilitySupport) -> Vec<CapabilityReport> {
    [(Capability::Http, http), (Capability::Https, https)]
        .into_iter()
        .map(|(capability, support)| CapabilityReport {
            capability,
            support,
            note: None,
        })
        .collect()
}

/// `ca`: the fixture CA, passed only through the explicit extra-roots option (never the OS store).
fn backend_with(ca: Option<&tls::TestCa>, gate_: Vec<CapabilityReport>) -> HttpBackend {
    HttpBackend::new(HttpConfig {
        extra_roots: ca.map(|c| vec![c.der()]).unwrap_or_default(),
        connect_timeout: Duration::from_secs(5),
        read_timeout: Duration::from_secs(5),
        scheme_gate: gate_,
        ..HttpConfig::default()
    })
    .unwrap()
}

fn backend() -> HttpBackend {
    use CapabilitySupport::Supported;
    backend_with(None, gate(Supported, Supported))
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

async fn settle(b: &HttpBackend, h: &BackendHandle) -> TransferState {
    let started = Instant::now();
    loop {
        let p = b.progress(h).await.unwrap();
        if !matches!(p.state, TransferState::Active | TransferState::Waiting) {
            return p.state;
        }
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "transfer did not settle: {p:?}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn sha(b: &[u8]) -> Vec<u8> {
    Sha256::digest(b).to_vec()
}

fn classify(state: &TransferState, dir: &Path, expected: &[u8]) -> Outcome {
    match state {
        TransferState::Complete => {
            let got = std::fs::read(dir.join(format!("out.bin{PART_SUFFIX}"))).unwrap_or_default();
            if sha(&got) == sha(expected) {
                Outcome::SafeSuccess
            } else {
                Outcome::Unsafe(format!(
                    "completed with wrong bytes ({} of {})",
                    got.len(),
                    expected.len()
                ))
            }
        }
        TransferState::Failed(e) => Outcome::SafeFailure(e.kind),
        other => Outcome::Unsafe(format!("unexpected state {other:?}")),
    }
}

fn report(case: &str, o: &Outcome, note: &str) {
    println!("CASE {case:<48} {o:?} {note}");
}

/// Runs one transfer to completion and returns (outcome, final state).
async fn run(
    b: &HttpBackend,
    dir: &Path,
    attempt: &str,
    req: TransferRequest,
    expected: &[u8],
) -> (Outcome, TransferState) {
    let _ = attempt;
    let h = b.start(req).await.unwrap();
    let state = settle(b, &h).await;
    (classify(&state, dir, expected), state)
}

fn part_exists(dir: &Path) -> bool {
    dir.join(format!("out.bin{PART_SUFFIX}")).exists()
}

// ------------------------------------------------------------------------------------ lengths

#[tokio::test]
async fn h01_known_length() {
    let s = Server::start().await;
    let body = payload(3 * MIB);
    s.set("f", Route::new(body.clone()));
    let dir = tempfile::tempdir().unwrap();
    let b = backend();
    let (o, _) = run(
        &b,
        dir.path(),
        "att_h01",
        request(dir.path(), "att_h01", s.url("f")),
        &body,
    )
    .await;
    let log = s.log_for("f");
    report(
        "H01 known length, 200",
        &o,
        &format!(
            "({} request, Accept-Encoding {:?})",
            log.len(),
            log[0].header("accept-encoding")
        ),
    );
    assert_eq!(o, Outcome::SafeSuccess);
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].header("accept-encoding"), Some("identity"));
    assert!(log[0].header("range").is_none());
    assert!(part_exists(dir.path()), "the finished file stays staged");
    assert!(
        !dir.path().join("out.bin").exists(),
        "the backend never places it at the destination"
    );
}

#[tokio::test]
async fn h02_unknown_length() {
    let s = Server::start().await;
    let body = payload(2 * MIB);
    let mut r = Route::new(body.clone());
    r.content_length = false;
    s.set("f", r);
    let dir = tempfile::tempdir().unwrap();
    let (o, _) = run(
        &backend(),
        dir.path(),
        "att_h02",
        request(dir.path(), "att_h02", s.url("f")),
        &body,
    )
    .await;
    report("H02 unknown length (close-delimited)", &o, "");
    assert_eq!(o, Outcome::SafeSuccess);
}

#[tokio::test]
async fn h03_zero_bytes() {
    let s = Server::start().await;
    s.set("f", Route::new(Vec::new()));
    let dir = tempfile::tempdir().unwrap();
    let (o, _) = run(
        &backend(),
        dir.path(),
        "att_h03",
        request(dir.path(), "att_h03", s.url("f")),
        &[],
    )
    .await;
    report("H03 zero-byte file", &o, "");
    assert_eq!(o, Outcome::SafeSuccess);
    assert_eq!(
        std::fs::metadata(dir.path().join(format!("out.bin{PART_SUFFIX}")))
            .unwrap()
            .len(),
        0
    );
}

#[tokio::test]
async fn h04_truncated_known_length_fails_without_hidden_retry() {
    let s = Server::start().await;
    let body = payload(3 * MIB);
    let mut r = Route::new(body.clone());
    r.truncate_after = Some(MIB);
    s.set("f", r);
    let dir = tempfile::tempdir().unwrap();
    let (o, state) = run(
        &backend(),
        dir.path(),
        "att_h04",
        request(dir.path(), "att_h04", s.url("f")),
        &body,
    )
    .await;
    report(
        "H04 body cut at 1 MiB of 3 MiB",
        &o,
        &format!("({} request)", s.log_for("f").len()),
    );
    assert_eq!(
        o,
        Outcome::SafeFailure(ErrorKind::NetworkOffline),
        "{state:?}"
    );
    assert_eq!(s.log_for("f").len(), 1, "no hidden backend retry");
    assert!(!dir.path().join("out.bin").exists(), "nothing published");
    assert!(part_exists(dir.path()), "partial kept until discarded");
}

#[tokio::test]
async fn h05_unknown_length_truncated_is_a_protocol_limitation() {
    let s = Server::start().await;
    let body = payload(2 * MIB);
    let mut r = Route::new(body.clone());
    r.content_length = false;
    r.truncate_after = Some(MIB);
    s.set("f", r);
    let dir = tempfile::tempdir().unwrap();
    let (o, _) = run(
        &backend(),
        dir.path(),
        "att_h05",
        request(dir.path(), "att_h05", s.url("f")),
        &body,
    )
    .await;
    report(
        "H05 unknown length, cut short",
        &o,
        "(indistinguishable from a complete body; needs a trusted size or hash)",
    );
    // Documented limitation: the backend cannot know. The service's expected size/SHA-256 check
    // is the only protection (crates/service `verify`).
    assert!(matches!(o, Outcome::Unsafe(_)), "{o:?}");
}

#[tokio::test]
async fn h06_encoded_body_is_refused() {
    let s = Server::start().await;
    let body = payload(MIB);
    let mut r = Route::new(body.clone());
    r.content_encoding = Some("gzip".into());
    s.set("f", r);
    let dir = tempfile::tempdir().unwrap();
    let (o, _) = run(
        &backend(),
        dir.path(),
        "att_h06",
        request(dir.path(), "att_h06", s.url("f")),
        &body,
    )
    .await;
    report("H06 Content-Encoding: gzip despite identity", &o, "");
    assert_eq!(o, Outcome::SafeFailure(ErrorKind::InvalidRequest));
    assert!(!dir.path().join("out.bin").exists());
}

// ------------------------------------------------------------------------------------ statuses

async fn status_case(case: &str, script: Scripted) -> (Outcome, TransferState, usize) {
    let s = Server::start().await;
    let mut r = Route::new(payload(1024));
    r.script.push_back(script);
    s.set("f", r);
    let dir = tempfile::tempdir().unwrap();
    let mut req = request(
        dir.path(),
        "att_st",
        format!("{}?token={SECRET}", s.url("f")),
    );
    req.headers.push((
        "Authorization".into(),
        Secret::new(format!("Bearer {SECRET}")),
    ));
    let (o, state) = run(&backend(), dir.path(), "att_st", req, &[]).await;
    let n = s.log_for("f").len();
    report(case, &o, &format!("({n} request)"));
    assert!(
        !format!("{state:?}").contains(SECRET),
        "no secret in errors"
    );
    (o, state, n)
}

#[tokio::test]
async fn s01_statuses_are_classified_and_never_retried_by_the_backend() {
    for (code, reason, kind) in [
        (401, "Unauthorized", ErrorKind::AuthRequired),
        (403, "Forbidden", ErrorKind::LinkRefreshRequired),
        (404, "Not Found", ErrorKind::LinkRefreshRequired),
        (410, "Gone", ErrorKind::LinkRefreshRequired),
        (412, "Precondition Failed", ErrorKind::RemoteChanged),
        (416, "Range Not Satisfiable", ErrorKind::InvalidRange),
        (500, "Internal Server Error", ErrorKind::ServerBusy),
    ] {
        let (o, _, n) = status_case(
            &format!("S01 {code} {reason}"),
            Scripted::status(code, reason),
        )
        .await;
        assert_eq!(o, Outcome::SafeFailure(kind), "{code}");
        assert_eq!(n, 1, "{code}: the service owns retries");
    }
}

#[tokio::test]
async fn s02_retry_after_reaches_the_service() {
    for (code, reason, value, secs) in [
        (503, "Service Unavailable", "30", 30),
        (429, "Too Many Requests", "7", 7),
    ] {
        let (o, state, _) = status_case(
            &format!("S02 {code} + Retry-After: {value}"),
            Scripted::status(code, reason).header("Retry-After", value),
        )
        .await;
        assert_eq!(o, Outcome::SafeFailure(ErrorKind::ServerBusy));
        let TransferState::Failed(e) = state else {
            unreachable!()
        };
        assert_eq!(e.retry_after, Some(Duration::from_secs(secs)), "{code}");
    }
}

// ------------------------------------------------------------------------------------ redirects

struct RedirectRun {
    outcome: Outcome,
    origin: Server,
    target: Server,
}

async fn redirect(case: &str, cross_origin: bool, userinfo: bool) -> RedirectRun {
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
    let dir = tempfile::tempdir().unwrap();
    let mut url = origin.url("start");
    if userinfo {
        url = url.replacen("http://", &format!("http://user:{SECRET}@"), 1);
    }
    let mut req = request(dir.path(), "att_rd", url);
    req.headers.push((
        "Authorization".into(),
        Secret::new(format!("Bearer {SECRET}")),
    ));
    req.headers
        .push(("Cookie".into(), Secret::new(format!("session={SECRET}"))));
    req.headers
        .push(("If-Match".into(), Secret::new(String::from("\"v1\""))));
    req.headers
        .push(("X-Client-Hint".into(), Secret::new(String::from("ok"))));
    let (outcome, _) = run(&backend(), dir.path(), "att_rd", req, &body).await;
    let leaked = target.log_for("real").iter().any(|l| {
        l.header("authorization").is_some()
            || l.header("cookie").is_some()
            || l.header("if-match").is_some()
    });
    report(
        case,
        &outcome,
        &format!("(scoped headers seen by redirect target: {leaked})"),
    );
    RedirectRun {
        outcome,
        origin,
        target,
    }
}

#[tokio::test]
async fn d01_same_origin_redirect_keeps_scoped_headers() {
    let r = redirect("D01 302 same origin with credentials", false, false).await;
    assert_eq!(r.outcome, Outcome::SafeSuccess);
    let real = r.target.log_for("real");
    assert!(real[0].header("authorization").is_some() && real[0].header("if-match").is_some());
}

#[tokio::test]
async fn d02_cross_origin_redirect_drops_credentials_and_validators() {
    let r = redirect("D02 302 cross origin with credentials", true, false).await;
    assert_eq!(r.outcome, Outcome::SafeSuccess);
    let start = r.origin.log_for("start");
    assert!(
        start[0].header("authorization").unwrap().contains(SECRET),
        "origin gets them"
    );
    assert!(start[0].header("cookie").is_some() && start[0].header("if-match").is_some());
    let real = r.target.log_for("real");
    assert!(real[0].header("authorization").is_none());
    assert!(real[0].header("cookie").is_none());
    assert!(real[0].header("if-match").is_none());
    assert_eq!(
        real[0].header("x-client-hint"),
        Some("ok"),
        "unscoped headers still sent"
    );
}

#[tokio::test]
async fn d03_userinfo_is_not_sent_cross_origin() {
    let r = redirect("D03 userinfo URL, 302 cross origin", true, true).await;
    assert_eq!(r.outcome, Outcome::SafeSuccess);
    assert!(
        r.target
            .log_for("real")
            .iter()
            .all(|l| l.header("authorization").is_none())
    );
}

#[tokio::test]
async fn d04_http_to_https_hop_is_gated() {
    use CapabilitySupport::{Supported, Unverified};
    let origin = Server::start().await;
    let ca = tls::TestCa::new("Downova Test CA");
    let target = Server::start_tls(ca.leaf(&["localhost", "127.0.0.1"])).await;
    target.set("real", Route::new(payload(1024)));
    let mut hop = Route::new(Vec::new());
    hop.redirect = Some((302, target.url("real")));
    origin.set("start", hop);
    let dir = tempfile::tempdir().unwrap();
    let b = backend_with(Some(&ca), gate(Supported, Unverified));
    let (o, _) = run(
        &b,
        dir.path(),
        "att_d04",
        request(dir.path(), "att_d04", origin.url("start")),
        &[],
    )
    .await;
    report(
        "D04 http -> https while Https is unverified",
        &o,
        &format!("(TLS target saw {} requests)", target.log_for("real").len()),
    );
    assert_eq!(
        o,
        Outcome::SafeFailure(ErrorKind::BackendCapabilityUnavailable)
    );
    assert!(target.log_for("real").is_empty());
}

#[tokio::test]
async fn d05_redirect_to_another_scheme_and_loops_are_refused() {
    let s = Server::start().await;
    let mut ftp = Route::new(Vec::new());
    ftp.redirect = Some((302, "ftp://127.0.0.1/x".into()));
    s.set("ftp", ftp);
    let mut file = Route::new(Vec::new());
    file.redirect = Some((301, "file:///etc/passwd".into()));
    s.set("file", file);
    let mut lp = Route::new(Vec::new());
    lp.redirect = Some((307, s.url("loop")));
    s.set("loop", lp);
    let dir = tempfile::tempdir().unwrap();
    let b = backend();
    for (i, (path, kind)) in [
        ("ftp", ErrorKind::BackendCapabilityUnavailable),
        ("file", ErrorKind::BackendCapabilityUnavailable),
        ("loop", ErrorKind::InvalidRequest),
    ]
    .into_iter()
    .enumerate()
    {
        let mut req = request(dir.path(), &format!("att_d05{i}"), s.url(path));
        req.file_name = format!("d05-{path}.bin");
        let h = b.start(req).await.unwrap();
        let state = settle(&b, &h).await;
        report(
            &format!("D05 redirect to {path}"),
            &Outcome::SafeFailure(match &state {
                TransferState::Failed(e) => e.kind,
                _ => ErrorKind::Internal,
            }),
            "",
        );
        assert!(
            matches!(&state, TransferState::Failed(e) if e.kind == kind),
            "{path}: {state:?}"
        );
    }
    assert_eq!(
        s.log_for("loop").len(),
        11,
        "1 request + 10 redirects, then refused"
    );
}

// ------------------------------------------------------------------------------------ TLS

#[tokio::test]
async fn t01_trusted_https_via_explicit_extra_root() {
    let ca = tls::TestCa::new("Downova Test CA");
    let s = Server::start_tls(ca.leaf(&["localhost", "127.0.0.1"])).await;
    let body = payload(2 * MIB);
    s.set("f", Route::new(body.clone()));
    let dir = tempfile::tempdir().unwrap();
    use CapabilitySupport::Supported;
    let b = backend_with(Some(&ca), gate(Supported, Supported));
    let t0 = Instant::now();
    let (o, state) = run(
        &b,
        dir.path(),
        "att_t01",
        request(dir.path(), "att_t01", s.url("f")),
        &body,
    )
    .await;
    report(
        "T01 HTTPS, test CA as an explicit extra root",
        &o,
        &format!("({:?})", t0.elapsed()),
    );
    assert_eq!(o, Outcome::SafeSuccess, "{state:?}");
}

async fn tls_failure(case: &str, identity: tls::Identity, ca: Option<&tls::TestCa>) {
    let s = Server::start_tls(identity).await;
    s.set("f", Route::new(payload(1024)));
    let dir = tempfile::tempdir().unwrap();
    use CapabilitySupport::Supported;
    let b = backend_with(ca, gate(Supported, Supported));
    let (o, state) = run(
        &b,
        dir.path(),
        "att_tls",
        request(dir.path(), "att_tls", s.url("f")),
        &[],
    )
    .await;
    let detail = match &state {
        TransferState::Failed(e) => e.detail.clone(),
        _ => String::new(),
    };
    report(
        case,
        &o,
        &format!(
            "({detail}; HTTP requests reached server: {})",
            s.log_for("f").len()
        ),
    );
    assert_eq!(o, Outcome::SafeFailure(ErrorKind::TlsHandshakeFailed));
    assert!(
        s.log_for("f").is_empty(),
        "no HTTP request over an untrusted connection"
    );
}

#[tokio::test]
async fn t02_self_signed_is_rejected() {
    tls_failure(
        "T02 HTTPS, self-signed",
        tls::self_signed(&["localhost", "127.0.0.1"]),
        None,
    )
    .await;
}

#[tokio::test]
async fn t03_wrong_host_name_is_rejected_even_with_a_trusted_ca() {
    let ca = tls::TestCa::new("Downova Test CA");
    tls_failure(
        "T03 HTTPS, trusted CA, wrong host name",
        ca.leaf(&["other.invalid"]),
        Some(&ca),
    )
    .await;
}

#[tokio::test]
async fn t04_fixture_ca_is_not_trusted_without_the_explicit_root() {
    let ca = tls::TestCa::new("Downova Test CA");
    tls_failure(
        "T04 HTTPS, test CA not supplied (production trust)",
        ca.leaf(&["localhost", "127.0.0.1"]),
        None,
    )
    .await;
}

// ------------------------------------------------------------------------------------ control

#[tokio::test]
async fn x01_remove_stops_network_and_disk_before_acknowledging() {
    let s = Server::start().await;
    let body = payload(8 * MIB);
    let mut r = Route::new(body.clone());
    r.slow = Some(Duration::from_millis(10));
    s.set("f", r);
    let dir = tempfile::tempdir().unwrap();
    let b = backend();
    let h = b
        .start(request(dir.path(), "att_x01", s.url("f")))
        .await
        .unwrap();
    let started = Instant::now();
    while b.progress(&h).await.unwrap().completed.0 < 256 * 1024 {
        assert!(started.elapsed() < Duration::from_secs(10));
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    b.remove(&h).await.unwrap();
    let part = dir.path().join(format!("out.bin{PART_SUFFIX}"));
    let at_ack = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    tokio::time::sleep(Duration::from_millis(300)).await;
    let later = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    report(
        "X01 remove during transfer",
        &Outcome::SafeFailure(ErrorKind::InvalidRequest),
        &format!("(partial {at_ack} B at ack, {later} B 300 ms later)"),
    );
    assert_eq!(at_ack, later, "no disk writes after remove returned");
    assert!(at_ack < body.len() as u64);
    assert!(!dir.path().join("out.bin").exists(), "nothing published");
    assert_eq!(
        b.progress(&h).await.unwrap_err().kind,
        ErrorKind::InvalidRequest,
        "attempt gone"
    );
}

#[tokio::test]
async fn x02_pause_stops_activity_and_flushes_confirmed_bytes() {
    let s = Server::start().await;
    let mut r = Route::new(payload(8 * MIB));
    r.slow = Some(Duration::from_millis(10));
    s.set("f", r);
    let dir = tempfile::tempdir().unwrap();
    let b = backend();
    let h = b
        .start(request(dir.path(), "att_x02", s.url("f")))
        .await
        .unwrap();
    let started = Instant::now();
    while b.progress(&h).await.unwrap().completed.0 == 0 {
        assert!(started.elapsed() < Duration::from_secs(10));
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    b.pause(&h).await.unwrap();
    let p = b.progress(&h).await.unwrap();
    assert_eq!(p.state, TransferState::Paused);
    let on_disk = std::fs::metadata(dir.path().join(format!("out.bin{PART_SUFFIX}")))
        .unwrap()
        .len();
    assert_eq!(
        on_disk, p.completed.0,
        "the partial file holds exactly the reported bytes"
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    let later = std::fs::metadata(dir.path().join(format!("out.bin{PART_SUFFIX}")))
        .unwrap()
        .len();
    report(
        "X02 pause during a transfer",
        &Outcome::SafeSuccess,
        &format!("(paused at {} B; {later} B 300 ms later)", p.completed.0),
    );
    assert_eq!(later, on_disk, "no writes while paused");
    assert_eq!(s.log_for("f").len(), 1, "no new request while paused");
    // Validated resume itself is covered by tests/validated_resume.rs.
}

#[tokio::test]
async fn x03_discard_deletes_only_the_attempt_partial() {
    let s = Server::start().await;
    let body = payload(2 * MIB);
    let mut r = Route::new(body.clone());
    r.truncate_after = Some(MIB);
    s.set("f", r);
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("unrelated.txt"), b"keep me").unwrap();
    let b = backend();
    let h = b
        .start(request(dir.path(), "att_x03", s.url("f")))
        .await
        .unwrap();
    settle(&b, &h).await;
    assert!(part_exists(dir.path()));
    b.discard(&h).await.unwrap();
    assert!(!part_exists(dir.path()), "own partial deleted");
    assert_eq!(
        std::fs::read(dir.path().join("unrelated.txt")).unwrap(),
        b"keep me"
    );

    // A completed transfer is never discarded.
    s.set("g", Route::new(body.clone()));
    let mut req = request(dir.path(), "att_x03b", s.url("g"));
    req.file_name = "done.bin".into();
    let h = b.start(req).await.unwrap();
    assert_eq!(settle(&b, &h).await, TransferState::Complete);
    assert_eq!(
        b.discard(&h).await.unwrap_err().kind,
        ErrorKind::InvalidRequest
    );
    assert_eq!(
        sha(&std::fs::read(dir.path().join(format!("done.bin{PART_SUFFIX}"))).unwrap()),
        sha(&body),
        "the staged result is kept for the service"
    );
    report(
        "X03 discard after failure; discard after completion",
        &Outcome::SafeSuccess,
        "",
    );
}

#[tokio::test]
async fn x04_destination_collisions_never_overwrite() {
    let s = Server::start().await;
    let body = payload(4 * MIB);
    let mut slow = Route::new(body.clone());
    slow.slow = Some(Duration::from_millis(5));
    s.set("f", slow);
    let dir = tempfile::tempdir().unwrap();
    let b = backend();

    // Existing destination: refused before any request.
    std::fs::write(dir.path().join("out.bin"), b"user file").unwrap();
    let e = b
        .start(request(dir.path(), "att_x04a", s.url("f")))
        .await
        .unwrap_err();
    assert_eq!(e.kind, ErrorKind::InvalidRequest);
    assert_eq!(
        std::fs::read(dir.path().join("out.bin")).unwrap(),
        b"user file"
    );
    std::fs::remove_file(dir.path().join("out.bin")).unwrap();

    // Existing partial file of the same name: refused, untouched.
    std::fs::write(dir.path().join(format!("out.bin{PART_SUFFIX}")), b"other").unwrap();
    let e = b
        .start(request(dir.path(), "att_x04b", s.url("f")))
        .await
        .unwrap_err();
    assert_eq!(e.kind, ErrorKind::InvalidRequest);
    std::fs::remove_file(dir.path().join(format!("out.bin{PART_SUFFIX}"))).unwrap();

    // Destination appears during the transfer: the backend finishes into its staging file and
    // never touches the destination (placement and its collision check belong to the service).
    let h = b
        .start(request(dir.path(), "att_x04c", s.url("f")))
        .await
        .unwrap();
    std::fs::write(dir.path().join("out.bin"), b"appeared meanwhile").unwrap();
    let state = settle(&b, &h).await;
    report(
        "X04 destination collisions",
        &Outcome::SafeFailure(ErrorKind::InvalidRequest),
        "(staged; the destination is never written by the backend)",
    );
    assert_eq!(state, TransferState::Complete);
    assert!(part_exists(dir.path()));
    assert_eq!(
        std::fs::read(dir.path().join("out.bin")).unwrap(),
        b"appeared meanwhile"
    );
    assert_eq!(
        s.log_for("f").len(),
        1,
        "only the third start reached the server"
    );
}

#[tokio::test]
async fn x05_caller_supplied_range_headers_are_refused() {
    // Ranges and their validators belong to the backend (resume, segments; B0.4.3 tests in
    // tests/segmented.rs). A caller cannot inject them.
    let dir = tempfile::tempdir().unwrap();
    let b = backend();
    for (i, name) in ["Range", "If-Range"].into_iter().enumerate() {
        let mut req = request(
            dir.path(),
            &format!("att_x05{i}"),
            "http://127.0.0.1:9/x".into(),
        );
        req.headers
            .push((name.into(), Secret::new(String::from("bytes=100-"))));
        assert_eq!(
            b.start(req).await.unwrap_err().kind,
            ErrorKind::BackendCapabilityUnavailable,
            "{name}"
        );
    }
    assert!(
        std::fs::read_dir(dir.path()).unwrap().next().is_none(),
        "no files created"
    );
}

#[tokio::test]
async fn gate_follows_the_report() {
    use CapabilitySupport::{Supported, Unverified};
    let dir = tempfile::tempdir().unwrap();
    let b = backend_with(None, gate(Supported, Unverified));
    let e = b
        .start(request(
            dir.path(),
            "att_g1",
            "https://127.0.0.1:9/x".into(),
        ))
        .await
        .unwrap_err();
    assert_eq!(e.kind, ErrorKind::BackendCapabilityUnavailable);
    let b = backend_with(None, gate(Unverified, Unverified));
    let e = b
        .start(request(dir.path(), "att_g2", "http://127.0.0.1:9/x".into()))
        .await
        .unwrap_err();
    assert_eq!(e.kind, ErrorKind::BackendCapabilityUnavailable);
}

// ------------------------------------------------------------------------------------ secrets

#[tokio::test]
async fn z01_synthetic_secrets_never_appear_in_errors_or_debug_output() {
    let dir = tempfile::tempdir().unwrap();
    let b = backend();
    let mut req = request(
        dir.path(),
        "att_z01",
        format!("http://user:{SECRET}@127.0.0.1:9/f?token={SECRET}"),
    );
    req.headers
        .push(("Cookie".into(), Secret::new(format!("s={SECRET}"))));
    let debug_req = format!("{req:?}");
    let h = b.start(req).await.unwrap();
    let state = settle(&b, &h).await;
    let dumped = format!("{state:?} {:?} {b:?}", b.progress(&h).await.unwrap());
    report(
        "Z01 secrets in URL, userinfo and headers",
        &Outcome::SafeFailure(match &state {
            TransferState::Failed(e) => e.kind,
            _ => ErrorKind::Internal,
        }),
        "(connection refused)",
    );
    assert!(matches!(state, TransferState::Failed(_)));
    assert!(!dumped.contains(SECRET), "{dumped}");
    assert!(
        !debug_req.contains(&format!("s={SECRET}")),
        "header values are Secret"
    );
}
