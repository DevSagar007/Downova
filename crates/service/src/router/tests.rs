#![allow(clippy::unwrap_used)]
use super::*;
use crate::fake::FakeBackend;
use dm_domain::{AttemptId, CapabilitySupport};

fn request(id: &str, engine: Engine) -> TransferRequest {
    TransferRequest {
        attempt_id: AttemptId::parse(id).unwrap(),
        url: "https://example.org/watch".into(),
        headers: vec![],
        session: None,
        destination_dir: std::env::temp_dir(),
        file_name: "watch".into(),
        max_connections: 1,
        rate_limit: None,
        login: None,
        trusted_host_keys: vec![],
        engine,
    }
}

fn both() -> EngineRouter<FakeBackend, FakeBackend> {
    let file = FakeBackend {
        identity: Some("http"),
        ..FakeBackend::with_all(CapabilitySupport::Supported)
    };
    let media = FakeBackend {
        identity: Some("yt-dlp"),
        ..FakeBackend::with_all(CapabilitySupport::Unsupported)
    };
    EngineRouter::new(file, Some((media, Some("2026.10.26".into()))))
}

#[tokio::test]
async fn each_attempt_goes_to_its_engine_and_later_calls_follow_the_handle() {
    let router = both();
    let f = router
        .start(request("att_file0001", Engine::File))
        .await
        .unwrap();
    let m = router
        .start(request("att_media001", Engine::Media))
        .await
        .unwrap();
    // Engine handles stay private: the router hands out its own, distinct per engine.
    assert_ne!(f, m);
    assert!(f.0.starts_with(FILE) && m.0.starts_with(MEDIA));
    router.pause(&m).await.unwrap();
    router.remove(&f).await.unwrap();
    let file_calls = router.file().calls();
    let media_calls = router.media().unwrap().calls();
    assert!(file_calls.iter().any(|c| c.contains("att_file0001")));
    assert!(!file_calls.iter().any(|c| c.contains("att_media001")));
    assert!(media_calls.iter().any(|c| c.contains("att_media001")));
    assert!(!media_calls.iter().any(|c| c.contains("att_file0001")));
    // A handle the router never issued is refused, never guessed.
    assert!(
        router
            .pause(&BackendHandle("att_file0001".into()))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn without_a_media_engine_media_is_refused_and_files_still_work() {
    let router: EngineRouter<FakeBackend, FakeBackend> =
        EngineRouter::new(FakeBackend::with_all(CapabilitySupport::Supported), None);
    let err = router
        .start(request("att_media001", Engine::Media))
        .await
        .unwrap_err();
    assert_eq!(err.kind, ErrorKind::BackendCapabilityUnavailable);
    assert!(router.engine_capabilities(Engine::Media).is_none());
    assert!(router.engine_info(Engine::Media).is_none());
    router
        .start(request("att_file0001", Engine::File))
        .await
        .unwrap();
}

#[tokio::test]
async fn capabilities_and_identity_are_reported_per_engine() {
    let router = both();
    assert_eq!(router.id(), "http");
    let media = router.engine_capabilities(Engine::Media).unwrap();
    assert!(
        media
            .iter()
            .all(|c| c.support == CapabilitySupport::Unsupported)
    );
    assert_eq!(
        router.engine_info(Engine::Media),
        Some(("yt-dlp", Some("2026.10.26".into())))
    );
}

/// F06: the media engine connects directly, so while a proxy is set it refuses to start rather
/// than bypass the proxy. Ordinary files use the proxy as before.
#[tokio::test]
async fn a_set_proxy_stops_media_from_starting_instead_of_bypassing_it() {
    let router = both();
    router
        .set_proxy(Some(ProxyConfig {
            host: "proxy.example".into(),
            port: 8080,
            username: None,
            password: None,
        }))
        .unwrap();
    let err = router
        .start(request("att_media001", Engine::Media))
        .await
        .unwrap_err();
    assert_eq!(err.kind, ErrorKind::BackendCapabilityUnavailable);
    assert!(router.media().unwrap().calls().is_empty());
    router
        .start(request("att_file0001", Engine::File))
        .await
        .unwrap();
    router.set_proxy(None).unwrap();
    router
        .start(request("att_media002", Engine::Media))
        .await
        .unwrap();
}

#[tokio::test]
async fn media_never_restores_from_a_checkpoint() {
    let router = both();
    let cp = DurableCheckpoint {
        backend: "http".into(),
        format_version: 1,
        confirmed: dm_domain::ByteCount(1),
        payload: vec![],
    };
    let err = router
        .restore(request("att_media001", Engine::Media), &cp)
        .await
        .unwrap_err();
    assert_eq!(err.kind, ErrorKind::BackendCapabilityUnavailable);
    assert!(router.media().unwrap().calls().is_empty());
}
