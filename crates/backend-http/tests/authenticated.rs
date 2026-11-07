//! BRW-05 (ADR 0027): a scoped browser session in the native HTTP engine, against a real HTTPS
//! fixture whose CA is trusted only through the explicit extra-roots option.
//!
//! Every request the fixture saw is inspected: which `Cookie` header it carried (domain, path,
//! Secure and expiry rules re-evaluated per URL), and that nothing outside the session's origin was
//! ever requested with, or without, the session.

use std::sync::Arc;
use std::time::{Duration, Instant};

use dm_backend_api::{BackendHandle, DownloadBackend, TransferRequest, TransferState};
use dm_backend_http::{HttpBackend, HttpConfig, PART_SUFFIX};
use dm_domain::cookies::{Origin, ScopedCookie, SessionScope};
use dm_domain::redact::Secret;
use dm_domain::{AttemptId, ErrorKind};
use dm_test_fixtures::{Route, Server, payload_seeded, tls};
use sha2::{Digest, Sha256};

const SECRET: &str = "SYNTH-COOKIE-b33";

fn backend(ca: &tls::TestCa) -> HttpBackend {
    HttpBackend::new(HttpConfig {
        connect_timeout: Duration::from_secs(5),
        read_timeout: Duration::from_secs(10),
        extra_roots: vec![ca.der()],
        ..HttpConfig::default()
    })
    .unwrap()
}

fn cookie(name: &str, domain: &str, path: &str, secure: bool) -> ScopedCookie {
    ScopedCookie {
        name: name.into(),
        value: Secret::new(format!("{name}-{SECRET}")),
        domain: domain.into(),
        host_only: true,
        path: path.into(),
        secure,
        expires_at_ms: None,
    }
}

fn session(origin: &str) -> Arc<SessionScope> {
    let o = Origin::parse(origin).unwrap();
    let host = o.host.clone();
    Arc::new(SessionScope {
        origin: o,
        cookies: vec![
            cookie("root", &host, "/", true),
            cookie("reports", &host, "/reports", true),
            cookie("elsewhere", "other.example", "/", true),
        ],
    })
}

fn request(
    dir: &std::path::Path,
    att: &str,
    url: String,
    conns: u8,
    s: Option<Arc<SessionScope>>,
) -> TransferRequest {
    TransferRequest {
        attempt_id: AttemptId::parse(att).unwrap(),
        url,
        headers: Vec::new(),
        session: s,
        destination_dir: dir.to_owned(),
        file_name: "f.bin".into(),
        max_connections: conns,
        rate_limit: None,
    }
}

async fn settle(b: &HttpBackend, h: &BackendHandle) -> TransferState {
    let t0 = Instant::now();
    loop {
        let p = b.progress(h).await.unwrap();
        if !matches!(p.state, TransferState::Active | TransferState::Waiting) {
            return p.state;
        }
        assert!(
            t0.elapsed() < Duration::from_secs(30),
            "did not settle: {p:?}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn sha(b: &[u8]) -> String {
    Sha256::digest(b)
        .iter()
        .map(|x| format!("{x:02x}"))
        .collect()
}

fn route(body: &[u8]) -> Route {
    let mut r = Route::new(body.to_vec());
    r.etag = Some("\"auth\"".into());
    r
}

async fn tls_server(ca: &tls::TestCa) -> Server {
    Server::start_tls(ca.leaf(&["localhost", "127.0.0.1"])).await
}

/// A01 + A07: the session's applicable cookies reach the resource (byte-exact); another domain's
/// cookie never does; a download without a session sends no cookie.
#[tokio::test]
async fn a01_cookies_follow_domain_and_path_and_public_downloads_send_none() {
    let ca = tls::TestCa::new("Downova Test CA");
    let srv = tls_server(&ca).await;
    let body = payload_seeded(256 * 1024, 31);
    srv.set("reports/f.bin", route(&body));
    srv.set("public.bin", route(&body));
    let dir = tempfile::tempdir().unwrap();
    let b = backend(&ca);
    let url = srv.url_on("127.0.0.1", "reports/f.bin");
    let origin = url.trim_end_matches("/reports/f.bin").to_owned();
    let h = b
        .start(request(
            dir.path(),
            "att_auth00000001",
            url,
            1,
            Some(session(&origin)),
        ))
        .await
        .unwrap();
    assert_eq!(settle(&b, &h).await, TransferState::Complete);
    let staged = std::fs::read(dir.path().join(format!("f.bin{PART_SUFFIX}"))).unwrap();
    assert_eq!(sha(&staged), sha(&body));
    for r in srv.log_for("reports/f.bin") {
        let c = r.header("cookie").unwrap_or_default();
        assert!(c.contains(&format!("reports=reports-{SECRET}")), "{c}");
        assert!(c.contains(&format!("root=root-{SECRET}")), "{c}");
        assert!(!c.contains("elsewhere"), "another domain's cookie: {c}");
    }
    let dir2 = tempfile::tempdir().unwrap();
    let h = b
        .start(request(
            dir2.path(),
            "att_auth00000007",
            srv.url_on("127.0.0.1", "public.bin"),
            1,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(settle(&b, &h).await, TransferState::Complete);
    assert!(
        srv.log_for("public.bin")
            .iter()
            .all(|r| r.header("cookie").is_none())
    );
}

/// A02: a same-origin redirect to another path re-evaluates the cookies: `/reports` no longer
/// applies at `/dl/`.
#[tokio::test]
async fn a02_a_same_origin_redirect_recomputes_cookie_paths() {
    let ca = tls::TestCa::new("Downova Test CA");
    let srv = tls_server(&ca).await;
    let body = payload_seeded(64 * 1024, 32);
    let mut go = Route::new(Vec::new());
    go.redirect = Some((302, "/dl/f.bin".into()));
    srv.set("reports/go", go);
    srv.set("dl/f.bin", route(&body));
    let dir = tempfile::tempdir().unwrap();
    let b = backend(&ca);
    let url = srv.url_on("127.0.0.1", "reports/go");
    let origin = url.trim_end_matches("/reports/go").to_owned();
    let h = b
        .start(request(
            dir.path(),
            "att_auth00000002",
            url,
            1,
            Some(session(&origin)),
        ))
        .await
        .unwrap();
    assert_eq!(settle(&b, &h).await, TransferState::Complete);
    let first = srv.log_for("reports/go");
    assert!(first[0].header("cookie").unwrap().contains("reports="));
    for r in srv.log_for("dl/f.bin") {
        let c = r.header("cookie").unwrap();
        assert!(c.contains("root=") && !c.contains("reports="), "{c}");
    }
}

/// A03 + A04: a redirect to another origin (another host name for the same server) or from https
/// to http stops the attempt; the other origin is never requested at all.
#[tokio::test]
async fn a03_a04_leaving_the_origin_stops_and_leaks_nothing() {
    let ca = tls::TestCa::new("Downova Test CA");
    let srv = tls_server(&ca).await;
    let plain = Server::start().await;
    let body = payload_seeded(32 * 1024, 33);
    plain.set("x.bin", route(&body));
    srv.set("x.bin", route(&body));
    let mut cross = Route::new(Vec::new());
    cross.redirect = Some((302, srv.url_on("localhost", "x.bin")));
    srv.set("cross", cross);
    let mut down = Route::new(Vec::new());
    down.redirect = Some((302, plain.url("x.bin")));
    srv.set("down", down);
    let b = backend(&ca);
    for (path, att) in [("cross", "att_auth00000003"), ("down", "att_auth00000004")] {
        let dir = tempfile::tempdir().unwrap();
        let url = srv.url_on("127.0.0.1", path);
        let origin = url.trim_end_matches(&format!("/{path}")).to_owned();
        let h = b
            .start(request(dir.path(), att, url, 1, Some(session(&origin))))
            .await
            .unwrap();
        match settle(&b, &h).await {
            TransferState::Failed(e) => {
                assert_eq!(e.kind, ErrorKind::AuthScopeUnsupported, "{path}")
            }
            other => panic!("{path}: {other:?}"),
        }
    }
    assert!(
        srv.log_for("x.bin").is_empty(),
        "the other host name was never requested"
    );
    assert!(
        plain.log_for("x.bin").is_empty(),
        "the http origin was never requested"
    );
}

/// A05: a segmented transfer sends the session's cookies on every range request.
#[tokio::test]
async fn a05_every_segment_carries_the_session() {
    let ca = tls::TestCa::new("Downova Test CA");
    let srv = tls_server(&ca).await;
    let body = payload_seeded(8 * 1024 * 1024, 34);
    srv.set("reports/big.bin", route(&body));
    let dir = tempfile::tempdir().unwrap();
    let b = backend(&ca);
    let url = srv.url_on("127.0.0.1", "reports/big.bin");
    let origin = url.trim_end_matches("/reports/big.bin").to_owned();
    let h = b
        .start(request(
            dir.path(),
            "att_auth00000005",
            url,
            4,
            Some(session(&origin)),
        ))
        .await
        .unwrap();
    assert_eq!(settle(&b, &h).await, TransferState::Complete);
    let log = srv.log_for("reports/big.bin");
    assert!(
        log.iter().filter(|r| r.header("range").is_some()).count() >= 2,
        "segmented"
    );
    for r in &log {
        assert!(
            r.header("cookie").is_some_and(|c| c.contains("reports=")),
            "{:?}",
            r.headers
        );
    }
    let staged = std::fs::read(dir.path().join(format!("f.bin{PART_SUFFIX}"))).unwrap();
    assert_eq!(sha(&staged), sha(&body));
}

/// A06: a rejected session is reported as AUTH_REQUIRED after one request (no retry storm).
#[tokio::test]
async fn a06_a_rejected_session_fails_once_as_auth_required() {
    let ca = tls::TestCa::new("Downova Test CA");
    let srv = tls_server(&ca).await;
    let mut r = Route::new(Vec::new());
    r.script
        .push_back(dm_test_fixtures::Scripted::status(401, "Unauthorized"));
    srv.set("reports/locked.bin", r);
    let dir = tempfile::tempdir().unwrap();
    let b = backend(&ca);
    let url = srv.url_on("127.0.0.1", "reports/locked.bin");
    let origin = url.trim_end_matches("/reports/locked.bin").to_owned();
    let h = b
        .start(request(
            dir.path(),
            "att_auth00000006",
            url,
            1,
            Some(session(&origin)),
        ))
        .await
        .unwrap();
    match settle(&b, &h).await {
        TransferState::Failed(e) => {
            assert_eq!(e.kind, ErrorKind::AuthRequired);
            assert!(!e.detail.contains(SECRET));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(srv.log_for("reports/locked.bin").len(), 1);
}

/// A07: pause and resume: the resume request (Range + If-Range) carries the session too, and the
/// file is byte-exact.
#[tokio::test]
async fn a07_a_resumed_request_carries_the_session() {
    let ca = tls::TestCa::new("Downova Test CA");
    let srv = tls_server(&ca).await;
    let body = payload_seeded(4 * 1024 * 1024, 35);
    let mut r = route(&body);
    r.slow = Some(Duration::from_millis(2));
    srv.set("reports/slow.bin", r);
    let dir = tempfile::tempdir().unwrap();
    let b = backend(&ca);
    let url = srv.url_on("127.0.0.1", "reports/slow.bin");
    let origin = url.trim_end_matches("/reports/slow.bin").to_owned();
    let h = b
        .start(request(
            dir.path(),
            "att_auth00000008",
            url,
            1,
            Some(session(&origin)),
        ))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    b.pause(&h).await.unwrap();
    srv.update("reports/slow.bin", |r| r.slow = None);
    b.resume(&h).await.unwrap();
    assert_eq!(settle(&b, &h).await, TransferState::Complete);
    let log = srv.log_for("reports/slow.bin");
    let resumed: Vec<_> = log
        .iter()
        .filter(|r| r.header("if-range").is_some())
        .collect();
    assert!(!resumed.is_empty(), "a validated resume request was made");
    assert!(
        resumed
            .iter()
            .all(|r| r.header("cookie").is_some_and(|c| c.contains("root=")))
    );
    let staged = std::fs::read(dir.path().join(format!("f.bin{PART_SUFFIX}"))).unwrap();
    assert_eq!(sha(&staged), sha(&body));
}
