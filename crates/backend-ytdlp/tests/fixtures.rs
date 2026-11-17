//! Real pinned yt-dlp, with generated WAV bytes, local HTML, and a throwaway HTTPS CA.
//! No public media site, account, network credentials or FFmpeg is needed.
#![allow(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    clippy::print_stdout
)]
use dm_backend_api::{
    BackendHandle, BackendProgress, DownloadBackend, StagedOutput, TransferRequest, TransferState,
};
use dm_backend_ytdlp::{LaunchConfig, YtdlpBackend, binary};
use dm_domain::{AttemptId, ErrorKind};
use dm_test_fixtures::{Route, Server, tls::TestCa};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::time::{Duration, Instant};

async fn backend(ca: Option<std::path::PathBuf>) -> YtdlpBackend {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let bin = binary::locate(Some(&root))
        .await
        .expect("run pnpm fetch:ytdlp first");
    let mut config = LaunchConfig::new(bin);
    config.ca_file = ca;
    YtdlpBackend::new(config)
}

fn request(dir: &Path, url: String, id: &str) -> TransferRequest {
    TransferRequest {
        attempt_id: AttemptId::parse(id).unwrap(),
        url,
        headers: vec![],
        session: None,
        destination_dir: dir.to_owned(),
        file_name: id.into(),
        max_connections: 4,
        rate_limit: None,
        login: None,
        trusted_host_keys: vec![],
    }
}

fn wav(bytes: usize) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend(b"RIFF");
    out.extend((bytes as u32 + 36).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16u32.to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(8000u32.to_le_bytes());
    out.extend(16000u32.to_le_bytes());
    out.extend(2u16.to_le_bytes());
    out.extend(16u16.to_le_bytes());
    out.extend(b"data");
    out.extend((bytes as u32).to_le_bytes());
    out.extend(dm_test_fixtures::payload_seeded(bytes, 707));
    out
}

async fn settled(backend: &YtdlpBackend, handle: &BackendHandle) -> BackendProgress {
    let start = Instant::now();
    loop {
        let p = backend.progress(handle).await.unwrap();
        if !matches!(p.state, TransferState::Waiting | TransferState::Active) {
            return p;
        }
        assert!(
            start.elapsed() < Duration::from_secs(45),
            "media attempt timed out: {p:?}"
        );
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
}

fn route(body: Vec<u8>, kind: &str) -> Route {
    let mut r = Route::new(body);
    r.content_type = Some(kind.into());
    r
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs yt-dlp; pnpm verify -- --only=ytdlp"]
async fn media_page_and_direct_media_have_exact_bytes_and_staged_output() {
    let server = Server::start().await;
    let body = wav(128 * 1024);
    server.set("sample.wav", route(body.clone(), "audio/wav"));
    server.set("watch", route(b"<html><head><title>Downova fixture</title></head><body><audio controls src='/sample.wav'></audio></body></html>".to_vec(), "text/html"));
    let backend = backend(None).await;
    let dir = tempfile::tempdir().unwrap();
    for (i, link) in ["watch", "sample.wav"].into_iter().enumerate() {
        let req = request(dir.path(), server.url(link), &format!("att_media_{i}"));
        let final_path = dir.path().join(&req.file_name);
        std::fs::write(&final_path, b"existing user file").unwrap();
        let h = backend.start(req).await.unwrap();
        let p = settled(&backend, &h).await;
        assert_eq!(p.state, TransferState::Complete);
        let StagedOutput::Staged(path) = backend.staged_output(&h).await.unwrap() else {
            panic!("must be staged");
        };
        let actual = std::fs::read(path).unwrap();
        assert_eq!(Sha256::digest(&actual), Sha256::digest(&body));
        assert_eq!(std::fs::read(&final_path).unwrap(), b"existing user file");
        assert!(p.naming.is_some());
        assert!(backend.checkpoint(&h).await.unwrap().is_none());
        assert!(
            backend.discard(&h).await.is_err(),
            "completed output is never discarded"
        );
        backend.remove(&h).await.unwrap();
    }
    backend.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs yt-dlp; pnpm verify -- --only=ytdlp"]
async fn https_trust_is_verified_and_invalid_certificates_fail() {
    let ca = TestCa::new("yt-dlp fixture CA");
    let server = Server::start_tls(ca.leaf(&["localhost"])).await;
    server.set("sample.wav", route(wav(4096), "audio/wav"));
    let dir = tempfile::tempdir().unwrap();
    let ca_path = dir.path().join("ca.pem");
    std::fs::write(&ca_path, ca.pem()).unwrap();
    let trusted = backend(Some(ca_path)).await;
    let h = trusted
        .start(request(dir.path(), server.url("sample.wav"), "att_tls_ok"))
        .await
        .unwrap();
    assert_eq!(settled(&trusted, &h).await.state, TransferState::Complete);
    trusted.shutdown().await.unwrap();
    let untrusted = backend(None).await;
    let h = untrusted
        .start(request(dir.path(), server.url("sample.wav"), "att_tls_bad"))
        .await
        .unwrap();
    let TransferState::Failed(e) = settled(&untrusted, &h).await.state else {
        panic!("untrusted TLS must fail");
    };
    assert_eq!(e.kind, ErrorKind::TlsHandshakeFailed);
    untrusted.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs yt-dlp; pnpm verify -- --only=ytdlp"]
async fn pause_stops_writing_and_partial_resume_requires_restart() {
    let server = Server::start().await;
    let mut slow = route(wav(2 * 1024 * 1024), "audio/wav");
    slow.slow = Some(Duration::from_millis(40));
    server.set("slow.wav", slow);
    let backend = backend(None).await;
    let dir = tempfile::tempdir().unwrap();
    let req = request(dir.path(), server.url("slow.wav"), "att_pause");
    let h = backend.start(req.clone()).await.unwrap();
    let start = Instant::now();
    while backend.progress(&h).await.unwrap().completed.0 == 0 {
        assert!(start.elapsed() < Duration::from_secs(30));
        tokio::time::sleep(Duration::from_millis(40)).await;
    }
    backend.pause(&h).await.unwrap();
    let p = backend.progress(&h).await.unwrap();
    assert_eq!(p.state, TransferState::Paused);
    assert!(backend.resume(&h).await.is_err());
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        backend.progress(&h).await.unwrap(),
        p,
        "no late progress after pause acknowledgement"
    );
    backend.discard(&h).await.unwrap();
    backend.discard_output(&req).await.unwrap();
    assert!(std::fs::read_dir(dir.path()).unwrap().next().is_none());
    backend.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs yt-dlp; pnpm verify -- --only=ytdlp"]
async fn truncated_media_never_completes_and_restart_begins_without_range() {
    let server = Server::start().await;
    let mut broken = route(wav(128 * 1024), "audio/wav");
    broken.truncate_after = Some(64 * 1024);
    server.set("broken.wav", broken);
    let backend = backend(None).await;
    let dir = tempfile::tempdir().unwrap();
    let req = request(dir.path(), server.url("broken.wav"), "att_broken");
    let h = backend.start(req.clone()).await.unwrap();
    assert!(matches!(
        settled(&backend, &h).await.state,
        TransferState::Failed(_)
    ));
    assert!(backend.staged_output(&h).await.is_err());
    backend.discard(&h).await.unwrap();
    server.set("broken.wav", route(wav(128 * 1024), "audio/wav"));
    let h = backend.start(req).await.unwrap();
    assert_eq!(settled(&backend, &h).await.state, TransferState::Complete);
    assert!(
        server
            .log_for("broken.wav")
            .iter()
            .all(|r| r.header("range").is_none())
    );
    backend.shutdown().await.unwrap();
}
