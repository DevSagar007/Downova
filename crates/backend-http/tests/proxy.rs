//! F06 (ADR 0034): the native engine through one HTTP proxy.
//!
//! - P01 http through the proxy, byte-exact, with Basic sign-in; the origin never sees the
//!   proxy's credentials.
//! - P02 https through CONNECT, byte-exact.
//! - P03 a wrong password: 407 on a forwarded request and on a tunnel → AUTH_REQUIRED.
//! - P04 a refused tunnel → NETWORK_OFFLINE, nothing reaches the origin.
//! - P05 an unreachable proxy never falls back to a direct connection.
//! - P06 switching back to direct (and on again) applies to the next transfer.

#![allow(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    clippy::print_stdout
)]

use std::time::{Duration, Instant};

use dm_backend_api::{BackendHandle, DownloadBackend, ProxyConfig, TransferRequest, TransferState};
use dm_backend_http::{HttpBackend, HttpConfig, PART_SUFFIX};
use dm_domain::redact::Secret;
use dm_domain::{AttemptId, ErrorKind};
use dm_test_fixtures::proxy::{Proxy, ProxyBehaviour, base64};
use dm_test_fixtures::{Route, Server, payload_seeded, tls};
use sha2::{Digest, Sha256};

const PASSWORD: &str = "SYNTHETIC-proxy-pw-41";

fn backend(ca: Option<&tls::TestCa>) -> HttpBackend {
    HttpBackend::new(HttpConfig {
        connect_timeout: Duration::from_secs(5),
        read_timeout: Duration::from_secs(10),
        extra_roots: ca.map(|c| vec![c.der()]).unwrap_or_default(),
        ..HttpConfig::default()
    })
    .unwrap()
}

fn config(port: u16, password: Option<&str>) -> ProxyConfig {
    ProxyConfig {
        host: "127.0.0.1".into(),
        port,
        username: password.map(|_| "alice".to_owned()),
        password: password.map(Secret::new),
    }
}

fn signed_in() -> ProxyBehaviour {
    ProxyBehaviour {
        credentials: Some(("alice".into(), PASSWORD.into())),
        refuse_connect: false,
    }
}

fn request(dir: &std::path::Path, att: &str, url: String) -> TransferRequest {
    TransferRequest {
        attempt_id: AttemptId::parse(att).unwrap(),
        url,
        headers: Vec::new(),
        session: None,
        destination_dir: dir.to_owned(),
        file_name: "f.bin".into(),
        max_connections: 1,
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

fn staged(dir: &std::path::Path) -> Vec<u8> {
    std::fs::read(dir.join(format!("f.bin{PART_SUFFIX}"))).unwrap()
}

fn failed_kind(state: TransferState) -> (ErrorKind, String) {
    match state {
        TransferState::Failed(e) => (e.kind, e.detail),
        other => panic!("expected a failure, got {other:?}"),
    }
}

#[tokio::test]
async fn p01_http_goes_through_the_proxy_byte_exact_and_the_origin_never_sees_its_sign_in() {
    let srv = Server::start().await;
    let body = payload_seeded(512 * 1024, 61);
    srv.set("f.bin", Route::new(body.clone()));
    let proxy = Proxy::start(signed_in()).await;
    let b = backend(None);
    b.set_proxy(Some(config(proxy.port(), Some(PASSWORD))))
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let h = b
        .start(request(dir.path(), "att_proxy0000001", srv.url("f.bin")))
        .await
        .unwrap();
    assert_eq!(settle(&b, &h).await, TransferState::Complete);
    assert_eq!(sha(&staged(dir.path())), sha(&body));
    assert!(proxy.forwarded() >= 1, "nothing went through the proxy");
    let expected = format!("Basic {}", base64(format!("alice:{PASSWORD}").as_bytes()));
    assert!(proxy.auth_headers().iter().all(|a| *a == expected));
    assert!(!srv.log().is_empty());
    for r in srv.log() {
        assert!(r.header("proxy-authorization").is_none());
    }
    // The configuration's Debug never shows the password.
    assert!(!format!("{:?}", config(1, Some(PASSWORD))).contains(PASSWORD));
}

#[tokio::test]
async fn p02_https_goes_through_a_connect_tunnel_byte_exact() {
    let ca = tls::TestCa::new("Downova Test CA");
    let srv = Server::start_tls(ca.leaf(&["localhost", "127.0.0.1"])).await;
    let body = payload_seeded(384 * 1024, 62);
    srv.set("f.bin", Route::new(body.clone()));
    let proxy = Proxy::start(signed_in()).await;
    let b = backend(Some(&ca));
    b.set_proxy(Some(config(proxy.port(), Some(PASSWORD))))
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let h = b
        .start(request(
            dir.path(),
            "att_proxy0000002",
            srv.url_on("127.0.0.1", "f.bin"),
        ))
        .await
        .unwrap();
    assert_eq!(settle(&b, &h).await, TransferState::Complete);
    assert_eq!(sha(&staged(dir.path())), sha(&body));
    assert!(proxy.tunnels() >= 1, "no CONNECT tunnel was opened");
    assert_eq!(proxy.forwarded(), 0);
}

#[tokio::test]
async fn p03_a_wrong_password_is_auth_required_for_http_and_https() {
    let ca = tls::TestCa::new("Downova Test CA");
    let plain = Server::start().await;
    plain.set("f.bin", Route::new(payload_seeded(4096, 63)));
    let secure = Server::start_tls(ca.leaf(&["localhost", "127.0.0.1"])).await;
    secure.set("f.bin", Route::new(payload_seeded(4096, 64)));
    let proxy = Proxy::start(signed_in()).await;
    let b = backend(Some(&ca));
    b.set_proxy(Some(config(proxy.port(), Some("wrong"))))
        .unwrap();
    for (att, url) in [
        ("att_proxy0000003", plain.url("f.bin")),
        ("att_proxy0000004", secure.url_on("127.0.0.1", "f.bin")),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let h = b
            .start(request(dir.path(), att, url.clone()))
            .await
            .unwrap();
        let (kind, detail) = failed_kind(settle(&b, &h).await);
        assert_eq!(kind, ErrorKind::AuthRequired, "{url}: {detail}");
    }
    assert!(plain.log().is_empty() && secure.log().is_empty());
    assert!(proxy.refused() >= 2);
}

#[tokio::test]
async fn p04_a_refused_tunnel_fails_and_nothing_reaches_the_origin() {
    let ca = tls::TestCa::new("Downova Test CA");
    let srv = Server::start_tls(ca.leaf(&["localhost", "127.0.0.1"])).await;
    srv.set("f.bin", Route::new(payload_seeded(4096, 65)));
    let proxy = Proxy::start(ProxyBehaviour {
        credentials: None,
        refuse_connect: true,
    })
    .await;
    let b = backend(Some(&ca));
    b.set_proxy(Some(config(proxy.port(), None))).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let h = b
        .start(request(
            dir.path(),
            "att_proxy0000005",
            srv.url_on("127.0.0.1", "f.bin"),
        ))
        .await
        .unwrap();
    let (kind, detail) = failed_kind(settle(&b, &h).await);
    assert_eq!(kind, ErrorKind::NetworkOffline);
    assert!(detail.contains("proxy refused"), "{detail}");
    assert!(srv.log().is_empty());
}

#[tokio::test]
async fn p05_an_unreachable_proxy_never_falls_back_to_a_direct_connection() {
    let srv = Server::start().await;
    srv.set("f.bin", Route::new(payload_seeded(4096, 66)));
    // A port that was just free: nothing listens there.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let b = backend(None);
    b.set_proxy(Some(config(port, None))).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let h = b
        .start(request(dir.path(), "att_proxy0000006", srv.url("f.bin")))
        .await
        .unwrap();
    let (kind, _) = failed_kind(settle(&b, &h).await);
    assert_eq!(kind, ErrorKind::NetworkOffline);
    assert!(srv.log().is_empty(), "the origin was reached directly");
}

#[tokio::test]
async fn p06_switching_off_and_on_applies_to_the_next_transfer() {
    let srv = Server::start().await;
    let body = payload_seeded(64 * 1024, 67);
    srv.set("f.bin", Route::new(body.clone()));
    let proxy = Proxy::start(ProxyBehaviour::default()).await;
    let b = backend(None);
    b.set_proxy(Some(config(proxy.port(), None))).unwrap();
    b.set_proxy(None).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let h = b
        .start(request(dir.path(), "att_proxy0000007", srv.url("f.bin")))
        .await
        .unwrap();
    assert_eq!(settle(&b, &h).await, TransferState::Complete);
    assert_eq!(proxy.forwarded(), 0, "direct mode still used the proxy");
    b.set_proxy(Some(config(proxy.port(), None))).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let h = b
        .start(request(dir.path(), "att_proxy0000008", srv.url("f.bin")))
        .await
        .unwrap();
    assert_eq!(settle(&b, &h).await, TransferState::Complete);
    assert_eq!(sha(&staged(dir.path())), sha(&body));
    assert!(proxy.forwarded() >= 1);
}
