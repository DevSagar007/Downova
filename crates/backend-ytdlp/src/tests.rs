#![allow(clippy::unwrap_used)]
use super::*;
use dm_domain::{AttemptId, capability::missing_from, redact::Secret};

fn request(dir: &Path) -> TransferRequest {
    TransferRequest {
        attempt_id: AttemptId::parse("att_media_test").unwrap(),
        url: "https://example.test/watch?v=1".into(),
        headers: vec![],
        session: None,
        destination_dir: dir.to_owned(),
        file_name: "watch".into(),
        max_connections: 4,
        rate_limit: None,
        login: None,
        trusted_host_keys: vec![],
    }
}

#[test]
fn capabilities_are_complete_and_do_not_inherit_upstream_promises() {
    let report = capabilities();
    assert!(missing_from(&report).is_empty());
    assert_eq!(report.len(), Capability::ALL.len());
    for entry in report {
        assert_eq!(
            entry.support,
            if matches!(entry.capability, Capability::Http | Capability::Https) {
                CapabilitySupport::Supported
            } else {
                CapabilitySupport::Unsupported
            }
        );
    }
}

#[test]
fn credentials_options_and_batch_injection_are_refused() {
    let mut r = request(Path::new("/tmp"));
    r.url.push_str("\nhttps://example.test/other");
    assert_eq!(validate(&r).unwrap_err().kind, ErrorKind::InvalidRequest);
    r.url = "file:///etc/passwd".into();
    assert!(validate(&r).is_err());
    r.url = "https://user:password@example.test/watch".into();
    assert_eq!(
        validate(&r).unwrap_err().kind,
        ErrorKind::BackendCapabilityUnavailable
    );
    r.url = "https://example.test/watch".into();
    r.headers.push(("Cookie".into(), Secret::new("PRIVATE")));
    assert_eq!(
        validate(&r).unwrap_err().kind,
        ErrorKind::BackendCapabilityUnavailable
    );
    r.headers.clear();
    r.rate_limit = Some(1024);
    assert!(validate(&r).is_err());
    r.rate_limit = None;
    r.file_name = "../outside".into();
    assert!(validate(&r).is_err());
}

#[test]
fn diagnostics_never_echo_the_url_or_cookie() {
    for raw in [
        "ERROR https://example.test/watch?token=PRIVATE HTTP Error 403 Cookie: SECRET",
        "https://example.test/?token=PRIVATE HTTP Error 429",
        "PRIVATE Certificate Verify Failed",
        "PRIVATE Unsupported URL",
        "PRIVATE unexpected failure",
    ] {
        let e = classify(raw);
        assert!(!e.detail.contains("PRIVATE") && !e.detail.contains("SECRET"));
        assert!(!e.detail.contains("https://"));
    }
    assert_eq!(classify("HTTP Error 429").kind, ErrorKind::ServerBusy);
    assert_eq!(
        classify("HTTP Error 404").kind,
        ErrorKind::LinkRefreshRequired
    );
}

#[test]
fn progress_is_not_completion_and_naming_is_only_advisory() {
    let progress = Mutex::new(BackendProgress {
        state: TransferState::Active,
        completed: ByteCount(0),
        total: None,
        connections: 1,
        naming: None,
    });
    parse_line(
        "downova-progress:{\"status\":\"finished\",\"downloaded_bytes\":42,\"total_bytes\":42}",
        &progress,
    );
    assert_eq!(snapshot(&progress).unwrap().state, TransferState::Active);
    parse_line(
        "downova-meta:{\"title\":\"A/B % video\",\"ext\":\"mp4\"}",
        &progress,
    );
    assert_eq!(
        snapshot(&progress)
            .unwrap()
            .naming
            .unwrap()
            .url_file_name
            .unwrap(),
        "A%2FB%20%25%20video.mp4"
    );
    parse_line(
        "downova-meta:{\"title\":\"video\",\"ext\":\"../exe\"}",
        &progress,
    );
    assert!(
        !snapshot(&progress)
            .unwrap()
            .naming
            .unwrap()
            .url_file_name
            .unwrap()
            .contains("exe")
    );
}

#[test]
fn partial_cleanup_is_bound_to_the_staging_name() {
    let dir = tempfile::tempdir().unwrap();
    let r = request(dir.path());
    let work = work_dir(&r).unwrap();
    assert_eq!(work.parent(), Some(dir.path()));
    std::fs::write(dir.path().join("keep.mp4"), b"existing").unwrap();
    std::fs::create_dir(&work).unwrap();
    std::fs::write(work.join("media.part"), b"partial").unwrap();
    cleanup(&work).unwrap();
    assert_eq!(
        std::fs::read(dir.path().join("keep.mp4")).unwrap(),
        b"existing"
    );
}
