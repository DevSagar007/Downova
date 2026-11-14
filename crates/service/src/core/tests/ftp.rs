//! F05 (ADR 0035): the service accepts anonymous ftp:// downloads, gated on the Ftp capability.

use super::*;

#[tokio::test]
async fn ftp_links_are_added_when_the_engine_supports_them_and_refused_otherwise() {
    let dir = tempfile::tempdir().unwrap();
    let core = core_with(
        FakeBackend::with_all(CapabilitySupport::Supported),
        dir.path(),
    );
    let job = core
        .add(add_params(
            "ext_ftp_000001",
            "ftp://ftp.example.org/pub/a.iso",
        ))
        .await
        .unwrap();
    assert_ne!(job.phase, Phase::Failed);
    {
        let started = core.backend.started.lock().unwrap();
        assert_eq!(started.len(), 1);
        assert_eq!(started[0].url, "ftp://ftp.example.org/pub/a.iso");
    }

    let no_ftp = core_with(
        FakeBackend::supported_except(Capability::Ftp, CapabilitySupport::Unsupported),
        dir.path(),
    );
    let err = no_ftp
        .add(add_params(
            "ext_ftp_000002",
            "ftp://ftp.example.org/pub/a.iso",
        ))
        .await
        .unwrap_err();
    assert_eq!(err.0.kind, ErrorKind::BackendCapabilityUnavailable);
    assert!(no_ftp.backend.started.lock().unwrap().is_empty());
}

#[tokio::test]
async fn ftp_links_with_credentials_or_other_schemes_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let core = core_with(
        FakeBackend::with_all(CapabilitySupport::Supported),
        dir.path(),
    );
    for (key, url) in [
        ("ext_ftp_000003", "ftp://user:pw@ftp.example.org/a.iso"),
        ("ext_ftp_000004", "ftp://user@ftp.example.org/a.iso"),
        ("ext_ftp_000005", "sftp://ftp.example.org/a.iso"),
        ("ext_ftp_000006", "ftp:///a.iso"),
    ] {
        let err = core.add(add_params(key, url)).await.unwrap_err();
        assert_eq!(err.0.kind, ErrorKind::InvalidRequest, "{url}");
        assert!(
            !format!("{:?}", err.0).contains("pw@"),
            "the link is never echoed"
        );
    }
    assert!(core.backend.started.lock().unwrap().is_empty());
}
