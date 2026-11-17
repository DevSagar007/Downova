//! Coordinator + real yt-dlp: normal Add URL contract, journal, checksum and collision policy.
//! No IPC is needed, so this can run in an environment that forbids Unix sockets.
#![allow(clippy::unwrap_used, clippy::arithmetic_side_effects)]
use dm_backend_ytdlp::{LaunchConfig, YtdlpBackend, binary};
use dm_domain::{Phase, api::AddDownloadParams};
use dm_service::core::{Core, Profiles};
use dm_test_fixtures::{Route, Server};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::time::{Duration, Instant};

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs yt-dlp; pnpm verify -- --only=ytdlp"]
async fn add_media_page_finalizes_verified_bytes_and_never_overwrites_a_file() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let binary = binary::locate(Some(&root)).await.unwrap();
    let backend = YtdlpBackend::new(LaunchConfig::new(binary));
    let dir = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let mut profiles = Profiles::new();
    profiles.insert("downloads".into(), dir.path().to_owned());
    let core = Core::new(backend, None, profiles)
        .with_store(dm_storage::Store::open(&data.path().join("test.sqlite3")).unwrap())
        .unwrap();
    let server = Server::start().await;
    // Small valid PCM WAV, generated locally; no FFmpeg or public site dependency.
    let mut wav = b"RIFF".to_vec();
    wav.extend(8228u32.to_le_bytes());
    wav.extend(b"WAVEfmt ");
    wav.extend(16u32.to_le_bytes());
    wav.extend(1u16.to_le_bytes());
    wav.extend(1u16.to_le_bytes());
    wav.extend(8000u32.to_le_bytes());
    wav.extend(16000u32.to_le_bytes());
    wav.extend(2u16.to_le_bytes());
    wav.extend(16u16.to_le_bytes());
    wav.extend(b"data");
    wav.extend(8192u32.to_le_bytes());
    wav.extend(vec![0; 8192]);
    let mut audio = Route::new(wav.clone());
    audio.content_type = Some("audio/wav".into());
    server.set("sample.wav", audio);
    let mut html = Route::new(b"<html><head><title>My media</title></head><body><audio src='/sample.wav'></audio></body></html>".to_vec());
    html.content_type = Some("text/html".into());
    server.set("watch", html);
    // Keep a real file at the eventual response title to test the service's naming policy.
    std::fs::write(dir.path().join("My media.wav"), b"keep me").unwrap();
    let sha: String = Sha256::digest(&wav)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let params: AddDownloadParams = serde_json::from_value(serde_json::json!({
        "idempotency_key":"media-coordinator-1", "source":{"url":server.url("watch")},
        "destination_profile_id":"downloads", "expected_sha256":sha
    }))
    .unwrap();
    let added = core.add(params.clone()).await.unwrap();
    assert_eq!(
        core.add(params).await.unwrap().job_id,
        added.job_id,
        "same Add URL key is idempotent"
    );
    let start = Instant::now();
    let done = loop {
        core.poll_once().await;
        let snap = core.get(&added.job_id).await.unwrap();
        if matches!(
            snap.phase,
            Phase::Completed | Phase::Failed | Phase::Cancelled
        ) {
            break snap;
        }
        assert!(start.elapsed() < Duration::from_secs(45));
        tokio::time::sleep(Duration::from_millis(30)).await;
    };
    assert_eq!(done.phase, Phase::Completed, "{:?}", done.error);
    assert_ne!(done.display_name, "My media.wav");
    assert_eq!(
        std::fs::read(dir.path().join("My media.wav")).unwrap(),
        b"keep me"
    );
    assert_eq!(
        std::fs::read(dir.path().join(&done.display_name)).unwrap(),
        wav
    );
    assert_eq!(done.total_bytes.unwrap().0, 8236);
}
