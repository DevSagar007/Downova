//! F05 (ADR 0035): anonymous FTP through the native engine, against the test FTP server.
//!
//! - F01 anonymous EPSV download, byte-exact.
//! - F02 no EPSV and a PASV reply naming another address: PASV to the control peer, byte-exact.
//! - F03 a cut data connection fails as NETWORK_OFFLINE; the durable checkpoint resumes with
//!   `REST <offset>` to a byte-exact file.
//! - F04 the file changed on the server (MDTM): the resume starts over, byte-exact new content.
//! - F05 a server without MDTM cannot resume (no validator).
//! - F06 a missing file and a refused login are classified (550, 530).
//! - F07 a path that would inject a command is refused before connecting.
//! - F08 an HTTP redirect to ftp:// is not followed.

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
use dm_test_fixtures::ftp::{FtpBehaviour, FtpServer};
use dm_test_fixtures::{Route, Server, payload_seeded};
use sha2::{Digest, Sha256};

const MTIME_A: &str = "20260101120000";
const MTIME_B: &str = "20260202130000";

fn backend() -> HttpBackend {
    HttpBackend::new(HttpConfig {
        connect_timeout: Duration::from_secs(5),
        read_timeout: Duration::from_secs(10),
        ..HttpConfig::default()
    })
    .unwrap()
}

fn request(dir: &Path, att: &str, url: String) -> TransferRequest {
    TransferRequest {
        attempt_id: AttemptId::parse(att).unwrap(),
        url,
        headers: Vec::new(),
        session: None,
        destination_dir: dir.to_owned(),
        file_name: "f.bin".into(),
        // Segmented transfers are an HTTP feature; FTP always uses one connection.
        max_connections: 4,
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

fn staged(dir: &Path) -> Vec<u8> {
    std::fs::read(dir.join(format!("f.bin{PART_SUFFIX}"))).unwrap_or_default()
}

fn failed(state: TransferState) -> (ErrorKind, String) {
    match state {
        TransferState::Failed(e) => (e.kind, e.detail),
        other => panic!("expected a failure, got {other:?}"),
    }
}

fn commands(srv: &FtpServer, verb: &str) -> Vec<String> {
    srv.log()
        .into_iter()
        .filter(|l| l.to_ascii_uppercase().starts_with(verb))
        .collect()
}

#[tokio::test]
async fn f01_anonymous_epsv_download_is_byte_exact() {
    let srv = FtpServer::start(FtpBehaviour::default()).await;
    let body = payload_seeded(1024 * 1024 + 17, 81);
    srv.set("pub/f.bin", body.clone(), MTIME_A);
    let dir = tempfile::tempdir().unwrap();
    let b = backend();
    let h = b
        .start(request(dir.path(), "att_ftp00000001", srv.url("pub/f.bin")))
        .await
        .unwrap();
    assert_eq!(settle(&b, &h).await, TransferState::Complete);
    assert_eq!(sha(&staged(dir.path())), sha(&body));
    assert_eq!(commands(&srv, "USER"), vec!["USER anonymous"]);
    assert_eq!(commands(&srv, "TYPE"), vec!["TYPE I"]);
    assert_eq!(commands(&srv, "EPSV").len(), 1);
    assert!(commands(&srv, "PASV").is_empty());
    assert!(commands(&srv, "REST").is_empty());
    assert_eq!(commands(&srv, "RETR"), vec!["RETR /pub/f.bin"]);
    let p = b.progress(&h).await.unwrap();
    assert_eq!(p.total.map(|t| t.0), Some(body.len() as u64));
}

#[tokio::test]
async fn f02_pasv_fallback_connects_to_the_control_peer_whatever_the_reply_names() {
    let srv = FtpServer::start(FtpBehaviour {
        no_epsv: true,
        pasv_wrong_address: true,
        ..FtpBehaviour::default()
    })
    .await;
    let body = payload_seeded(300 * 1024, 82);
    srv.set("f.bin", body.clone(), MTIME_A);
    let dir = tempfile::tempdir().unwrap();
    let b = backend();
    let h = b
        .start(request(dir.path(), "att_ftp00000002", srv.url("f.bin")))
        .await
        .unwrap();
    assert_eq!(settle(&b, &h).await, TransferState::Complete);
    assert_eq!(sha(&staged(dir.path())), sha(&body));
    assert_eq!(commands(&srv, "EPSV").len(), 1);
    assert_eq!(commands(&srv, "PASV").len(), 1);
}

/// Starts a download the server cuts after `cut` bytes; returns the failed attempt's checkpoint.
async fn cut_download(
    b: &HttpBackend,
    srv: &FtpServer,
    dir: &Path,
    cut: usize,
) -> (BackendHandle, Option<dm_backend_api::DurableCheckpoint>) {
    srv.cut_next_transfer_after(cut);
    let h = b
        .start(request(dir, "att_ftp00000003", srv.url("f.bin")))
        .await
        .unwrap();
    let (kind, _) = failed(settle(b, &h).await);
    assert_eq!(kind, ErrorKind::NetworkOffline);
    let cp = b.checkpoint(&h).await.unwrap();
    (h, cp)
}

#[tokio::test]
async fn f03_a_cut_transfer_resumes_with_rest_to_a_byte_exact_file() {
    let srv = FtpServer::start(FtpBehaviour::default()).await;
    // Checkpoints are published every 4 MiB (or second): cut past the first one.
    let body = payload_seeded(10 * 1024 * 1024 + 3, 83);
    srv.set("f.bin", body.clone(), MTIME_A);
    let dir = tempfile::tempdir().unwrap();
    let b = backend();
    let (_, cp) = cut_download(&b, &srv, dir.path(), 6 * 1024 * 1024).await;
    let cp = cp.expect("a durable checkpoint after the cut");
    let offset = cp.confirmed.0;
    assert!(offset > 0, "bytes before the cut were kept");
    let h = b
        .restore(
            request(dir.path(), "att_ftp00000004", srv.url("f.bin")),
            &cp,
        )
        .await
        .unwrap();
    b.resume(&h).await.unwrap();
    assert_eq!(settle(&b, &h).await, TransferState::Complete);
    assert_eq!(sha(&staged(dir.path())), sha(&body));
    assert_eq!(commands(&srv, "REST"), vec![format!("REST {offset}")]);
}

#[tokio::test]
async fn f04_a_changed_file_restarts_from_zero_instead_of_mixing_versions() {
    let srv = FtpServer::start(FtpBehaviour::default()).await;
    let old = payload_seeded(8 * 1024 * 1024, 84);
    srv.set("f.bin", old, MTIME_A);
    let dir = tempfile::tempdir().unwrap();
    let b = backend();
    let (_, cp) = cut_download(&b, &srv, dir.path(), 6 * 1024 * 1024).await;
    let cp = cp.unwrap();
    let new = payload_seeded(1024 * 1024, 85);
    srv.set("f.bin", new.clone(), MTIME_B);
    let h = b
        .restore(
            request(dir.path(), "att_ftp00000005", srv.url("f.bin")),
            &cp,
        )
        .await
        .unwrap();
    b.resume(&h).await.unwrap();
    assert_eq!(settle(&b, &h).await, TransferState::Complete);
    assert_eq!(sha(&staged(dir.path())), sha(&new), "only the new version");
    assert!(
        commands(&srv, "REST").is_empty(),
        "no append to the old bytes"
    );
}

#[tokio::test]
async fn f05_a_server_without_mdtm_cannot_resume() {
    let srv = FtpServer::start(FtpBehaviour {
        no_mdtm: true,
        ..FtpBehaviour::default()
    })
    .await;
    srv.set("f.bin", payload_seeded(8 * 1024 * 1024, 86), MTIME_A);
    let dir = tempfile::tempdir().unwrap();
    let b = backend();
    let (_, cp) = cut_download(&b, &srv, dir.path(), 6 * 1024 * 1024).await;
    if let Some(cp) = cp {
        let err = match b
            .restore(
                request(dir.path(), "att_ftp00000006", srv.url("f.bin")),
                &cp,
            )
            .await
        {
            Err(e) => e,
            Ok(h) => b.resume(&h).await.unwrap_err(),
        };
        assert_eq!(err.kind, ErrorKind::BackendCapabilityUnavailable);
    }
    assert!(commands(&srv, "REST").is_empty());
}

#[tokio::test]
async fn f06_missing_files_and_refused_logins_are_classified() {
    let srv = FtpServer::start(FtpBehaviour::default()).await;
    let dir = tempfile::tempdir().unwrap();
    let b = backend();
    let h = b
        .start(request(
            dir.path(),
            "att_ftp00000007",
            srv.url("missing.bin"),
        ))
        .await
        .unwrap();
    let (kind, detail) = failed(settle(&b, &h).await);
    assert_eq!(kind, ErrorKind::LinkRefreshRequired, "{detail}");
    assert!(detail.contains("550"));

    let closed = FtpServer::start(FtpBehaviour {
        refuse_login: true,
        ..FtpBehaviour::default()
    })
    .await;
    closed.set("f.bin", payload_seeded(1024, 87), MTIME_A);
    let dir = tempfile::tempdir().unwrap();
    let h = b
        .start(request(dir.path(), "att_ftp00000008", closed.url("f.bin")))
        .await
        .unwrap();
    let (kind, _) = failed(settle(&b, &h).await);
    assert_eq!(kind, ErrorKind::AuthRequired);
    assert!(commands(&closed, "RETR").is_empty());
}

#[tokio::test]
async fn f07_a_path_that_would_inject_a_command_is_refused_before_connecting() {
    let srv = FtpServer::start(FtpBehaviour::default()).await;
    let dir = tempfile::tempdir().unwrap();
    let b = backend();
    let h = b
        .start(request(
            dir.path(),
            "att_ftp00000009",
            srv.url("a.bin%0D%0ADELE%20x"),
        ))
        .await
        .unwrap();
    let (kind, _) = failed(settle(&b, &h).await);
    assert_eq!(kind, ErrorKind::InvalidRequest);
    assert!(srv.log().is_empty(), "nothing reached the server");
}

#[tokio::test]
async fn f08_a_web_link_redirecting_to_ftp_is_not_followed() {
    let ftp = FtpServer::start(FtpBehaviour::default()).await;
    ftp.set("f.bin", payload_seeded(1024, 88), MTIME_A);
    let web = Server::start().await;
    let mut r = Route::new(Vec::new());
    r.redirect = Some((302, ftp.url("f.bin")));
    web.set("go", r);
    let dir = tempfile::tempdir().unwrap();
    let b = backend();
    let h = b
        .start(request(dir.path(), "att_ftp00000010", web.url("go")))
        .await
        .unwrap();
    let (kind, _) = failed(settle(&b, &h).await);
    assert_eq!(kind, ErrorKind::BackendCapabilityUnavailable);
    assert!(ftp.log().is_empty(), "the FTP server was never contacted");
}
