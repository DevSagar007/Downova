//! F05 (ADR 0037): SFTP through the native engine, against the test SFTP server.
//!
//! - S01 an untrusted host key stops before any password is sent (HOST_KEY_UNKNOWN, key named).
//! - S02 a trusted key downloads byte-exact (absolute and home-relative paths).
//! - S03 a changed key stops before any password is sent (HOST_KEY_CHANGED).
//! - S04 a wrong password, or no site login, is AUTH_REQUIRED.
//! - S05 a cut transfer resumes from the checkpoint offset to a byte-exact file.
//! - S06 a changed file (mtime) restarts from zero.
//! - S07 a missing file is LINK_REFRESH_REQUIRED.

#![allow(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    clippy::print_stdout
)]

use std::path::Path;
use std::time::{Duration, Instant};

use dm_backend_api::{
    BackendHandle, DownloadBackend, DurableCheckpoint, Login, TransferRequest, TransferState,
};
use dm_backend_http::{HttpBackend, HttpConfig, PART_SUFFIX};
use dm_domain::redact::Secret;
use dm_domain::{AttemptId, ErrorKind};
use dm_test_fixtures::payload_seeded;
use dm_test_fixtures::sftp::{SftpServer, host_fingerprint};
use sha2::{Digest, Sha256};

const PASSWORD: &str = "SYNTHETIC-sftp-pw-37";

fn backend() -> HttpBackend {
    HttpBackend::new(HttpConfig {
        connect_timeout: Duration::from_secs(5),
        read_timeout: Duration::from_secs(10),
        ..HttpConfig::default()
    })
    .unwrap()
}

fn request(dir: &Path, att: &str, url: String, trusted: &[String]) -> TransferRequest {
    TransferRequest {
        attempt_id: AttemptId::parse(att).unwrap(),
        url,
        headers: Vec::new(),
        session: None,
        destination_dir: dir.to_owned(),
        file_name: "f.bin".into(),
        max_connections: 4,
        rate_limit: None,
        login: Some(Login {
            username: "alice".into(),
            password: Secret::new(PASSWORD),
        }),
        trusted_host_keys: trusted.to_vec(),
        engine: Default::default(),
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
            t0.elapsed() < Duration::from_secs(60),
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

fn staged(dir: &Path) -> Vec<u8> {
    std::fs::read(dir.join(format!("f.bin{PART_SUFFIX}"))).unwrap_or_default()
}

fn failed(state: TransferState) -> (ErrorKind, String) {
    match state {
        TransferState::Failed(e) => (e.kind, e.detail),
        other => panic!("expected a failure, got {other:?}"),
    }
}

async fn server() -> SftpServer {
    SftpServer::start(1, "alice", PASSWORD).await
}

#[tokio::test]
async fn s01_an_untrusted_host_key_stops_before_any_password_is_sent() {
    let srv = server().await;
    srv.set("f.bin", payload_seeded(4096, 91), 1_700_000_000);
    let dir = tempfile::tempdir().unwrap();
    let b = backend();
    let h = b
        .start(request(
            dir.path(),
            "att_sftp0000001",
            srv.url("f.bin"),
            &[],
        ))
        .await
        .unwrap();
    let (kind, detail) = failed(settle(&b, &h).await);
    assert_eq!(kind, ErrorKind::HostKeyUnknown);
    assert!(
        detail.contains(&format!("server key ssh-ed25519 {}", srv.fingerprint())),
        "{detail}"
    );
    assert_eq!(
        srv.sign_ins(),
        0,
        "a password was sent to an untrusted server"
    );
    assert!(!detail.contains(PASSWORD));
}

#[tokio::test]
async fn s02_a_trusted_key_downloads_byte_exact() {
    let srv = server().await;
    let body = payload_seeded(1024 * 1024 + 5, 92);
    srv.set("srv/f.bin", body.clone(), 1_700_000_000);
    srv.set("home/f.bin", body.clone(), 1_700_000_000);
    let trusted = vec![srv.fingerprint().to_owned()];
    for (att, path) in [
        ("att_sftp0000002", "srv/f.bin"),
        // `/~/` is the home folder (the test server's root): a relative request.
        ("att_sftp0000003", "~/home/f.bin"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let b = backend();
        let h = b
            .start(request(dir.path(), att, srv.url(path), &trusted))
            .await
            .unwrap();
        assert_eq!(settle(&b, &h).await, TransferState::Complete, "{path}");
        assert_eq!(sha(&staged(dir.path())), sha(&body));
    }
}

#[tokio::test]
async fn s03_a_changed_host_key_stops_before_any_password_is_sent() {
    let srv = SftpServer::start(2, "alice", PASSWORD).await;
    srv.set("f.bin", payload_seeded(4096, 93), 1_700_000_000);
    let dir = tempfile::tempdir().unwrap();
    let b = backend();
    let h = b
        .start(request(
            dir.path(),
            "att_sftp0000004",
            srv.url("f.bin"),
            &[host_fingerprint(1)],
        ))
        .await
        .unwrap();
    let (kind, detail) = failed(settle(&b, &h).await);
    assert_eq!(kind, ErrorKind::HostKeyChanged);
    assert!(detail.contains(srv.fingerprint()), "{detail}");
    assert_eq!(srv.sign_ins(), 0);
}

#[tokio::test]
async fn s04_a_wrong_password_or_no_site_login_is_auth_required() {
    let srv = server().await;
    srv.set("f.bin", payload_seeded(4096, 94), 1_700_000_000);
    let trusted = vec![srv.fingerprint().to_owned()];
    let b = backend();
    let dir = tempfile::tempdir().unwrap();
    let mut wrong = request(dir.path(), "att_sftp0000005", srv.url("f.bin"), &trusted);
    wrong.login = Some(Login {
        username: "alice".into(),
        password: Secret::new("wrong"),
    });
    let h = b.start(wrong).await.unwrap();
    let (kind, detail) = failed(settle(&b, &h).await);
    assert_eq!(kind, ErrorKind::AuthRequired);
    assert!(
        detail.contains("site login") && !detail.contains("wrong"),
        "{detail}"
    );

    let dir = tempfile::tempdir().unwrap();
    let mut none = request(dir.path(), "att_sftp0000006", srv.url("f.bin"), &trusted);
    none.login = None;
    let h = b.start(none).await.unwrap();
    let (kind, detail) = failed(settle(&b, &h).await);
    assert_eq!(kind, ErrorKind::AuthRequired);
    assert!(detail.contains("add a site login"), "{detail}");
}

async fn cut(
    b: &HttpBackend,
    srv: &SftpServer,
    dir: &Path,
    trusted: &[String],
) -> DurableCheckpoint {
    // Checkpoints are published every 4 MiB (or second): cut well past the first one.
    srv.cut_next_read_after(6 * 1024 * 1024);
    let h = b
        .start(request(dir, "att_sftp0000007", srv.url("f.bin"), trusted))
        .await
        .unwrap();
    let (kind, _) = failed(settle(b, &h).await);
    assert_eq!(kind, ErrorKind::NetworkOffline);
    b.checkpoint(&h)
        .await
        .unwrap()
        .expect("a durable checkpoint after the cut")
}

#[tokio::test]
async fn s05_a_cut_transfer_resumes_from_the_checkpoint_byte_exact() {
    let srv = server().await;
    let body = payload_seeded(10 * 1024 * 1024 + 9, 95);
    srv.set("f.bin", body.clone(), 1_700_000_000);
    let trusted = vec![srv.fingerprint().to_owned()];
    let dir = tempfile::tempdir().unwrap();
    let b = backend();
    let cp = cut(&b, &srv, dir.path(), &trusted).await;
    let offset = cp.confirmed.0;
    assert!(offset > 0);
    let h = b
        .restore(
            request(dir.path(), "att_sftp0000008", srv.url("f.bin"), &trusted),
            &cp,
        )
        .await
        .unwrap();
    b.resume(&h).await.unwrap();
    assert_eq!(settle(&b, &h).await, TransferState::Complete);
    assert_eq!(sha(&staged(dir.path())), sha(&body));
    assert_eq!(srv.reads_from().last().copied(), Some(offset));
}

#[tokio::test]
async fn s06_a_changed_file_restarts_from_zero() {
    let srv = server().await;
    srv.set("f.bin", payload_seeded(8 * 1024 * 1024, 96), 1_700_000_000);
    let trusted = vec![srv.fingerprint().to_owned()];
    let dir = tempfile::tempdir().unwrap();
    let b = backend();
    let cp = cut(&b, &srv, dir.path(), &trusted).await;
    let new = payload_seeded(8 * 1024 * 1024, 97);
    srv.set("f.bin", new.clone(), 1_700_000_999);
    let h = b
        .restore(
            request(dir.path(), "att_sftp0000009", srv.url("f.bin"), &trusted),
            &cp,
        )
        .await
        .unwrap();
    b.resume(&h).await.unwrap();
    assert_eq!(settle(&b, &h).await, TransferState::Complete);
    assert_eq!(sha(&staged(dir.path())), sha(&new));
    assert_eq!(srv.reads_from().last().copied(), Some(0));
}

#[tokio::test]
async fn s07_a_missing_file_is_link_refresh_required() {
    let srv = server().await;
    let trusted = vec![srv.fingerprint().to_owned()];
    let dir = tempfile::tempdir().unwrap();
    let b = backend();
    let h = b
        .start(request(
            dir.path(),
            "att_sftp0000010",
            srv.url("missing.bin"),
            &trusted,
        ))
        .await
        .unwrap();
    let (kind, _) = failed(settle(&b, &h).await);
    assert_eq!(kind, ErrorKind::LinkRefreshRequired);
}
