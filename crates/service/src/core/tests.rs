use super::*;
use crate::fake::{FakeBackend, progress};
use dm_domain::api::DownloadSource;

fn core_with(fake: FakeBackend, dir: &Path) -> Core<FakeBackend> {
    let mut profiles = Profiles::new();
    profiles.insert("downloads".into(), dir.to_owned());
    Core::new(fake, Some("test".into()), profiles)
}

fn add_params(key: &str, url: &str) -> AddDownloadParams {
    AddDownloadParams {
        idempotency_key: key.into(),
        source: DownloadSource { url: url.into() },
        destination_profile_id: "downloads".into(),
        expected_size_bytes: None,
        expected_sha256: None,
    }
}

fn handle_of(snapshot: &DownloadSnapshot) -> BackendHandle {
    BackendHandle(snapshot.attempt_id.clone().unwrap().as_str().to_owned())
}

#[tokio::test]
async fn ids_are_service_owned_and_snapshots_are_redacted() {
    let dir = tempfile::tempdir().unwrap();
    let core = core_with(
        FakeBackend::with_all(CapabilitySupport::Supported),
        dir.path(),
    );
    let snap = core
        .add(add_params(
            "key-00000001",
            "https://cdn.example.org/a/b%20c.zip?token=SYNTHETIC-SECRET",
        ))
        .await
        .unwrap();
    assert!(snap.job_id.as_str().starts_with("job_"));
    assert!(
        snap.attempt_id
            .as_ref()
            .unwrap()
            .as_str()
            .starts_with("att_")
    );
    assert_eq!(snap.display_name, "b c.zip");
    assert!(!snap.source_display.contains("SYNTHETIC-SECRET"));
    assert!(!format!("{snap:?}").contains("SYNTHETIC-SECRET"));
}

#[tokio::test]
async fn same_key_same_request_returns_the_same_job() {
    let dir = tempfile::tempdir().unwrap();
    let core = core_with(
        FakeBackend::with_all(CapabilitySupport::Supported),
        dir.path(),
    );
    let first = core
        .add(add_params("retry-key-1", "http://h/x.bin"))
        .await
        .unwrap();
    let again = core
        .add(add_params("retry-key-1", "http://h/x.bin"))
        .await
        .unwrap();
    assert_eq!(first.job_id, again.job_id);
    assert_eq!(core.list().await.len(), 1);
    assert_eq!(
        core.backend()
            .calls()
            .iter()
            .filter(|c| c.starts_with("start"))
            .count(),
        1
    );
}

#[tokio::test]
async fn same_key_different_request_is_a_conflict() {
    let dir = tempfile::tempdir().unwrap();
    let core = core_with(
        FakeBackend::with_all(CapabilitySupport::Supported),
        dir.path(),
    );
    core.add(add_params("conflict-key", "http://h/x.bin"))
        .await
        .unwrap();
    let err = core
        .add(add_params("conflict-key", "http://h/other.bin"))
        .await
        .unwrap_err();
    assert_eq!(err.0.kind, ErrorKind::IdempotencyConflict);
    assert_eq!(core.list().await.len(), 1);
}

#[tokio::test]
async fn unverified_http_is_refused_and_leaves_no_job_or_key() {
    let dir = tempfile::tempdir().unwrap();
    let core = core_with(
        FakeBackend::supported_except(Capability::Http, CapabilitySupport::Unverified),
        dir.path(),
    );
    let err = core
        .add(add_params("gated-key-1", "http://h/x.bin"))
        .await
        .unwrap_err();
    assert_eq!(err.0.kind, ErrorKind::BackendCapabilityUnavailable);
    assert!(core.list().await.is_empty());
    assert!(
        core.backend().calls().is_empty(),
        "backend must not be called"
    );
    // HTTPS is gated separately.
    assert!(
        core.add(add_params("gated-key-2", "https://h/x.bin"))
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn invalid_requests_are_rejected_without_echoing_the_url() {
    let dir = tempfile::tempdir().unwrap();
    let core = core_with(
        FakeBackend::with_all(CapabilitySupport::Supported),
        dir.path(),
    );
    for (key, url) in [
        ("bad-scheme-1", "ftp://h/x?token=SECRET"),
        ("bad-scheme-2", "file:///etc/passwd"),
        ("bad-host-01", "http:///x"),
    ] {
        let err = core.add(add_params(key, url)).await.unwrap_err();
        assert_eq!(err.0.kind, ErrorKind::InvalidRequest);
        assert!(!err.0.detail.unwrap_or_default().contains("SECRET"));
    }
    assert_eq!(
        core.add(add_params("short", "http://h/x"))
            .await
            .unwrap_err()
            .0
            .kind,
        ErrorKind::InvalidRequest
    );
    let mut unknown_profile = add_params("profile-key", "http://h/x");
    unknown_profile.destination_profile_id = "nope".into();
    assert_eq!(
        core.add(unknown_profile).await.unwrap_err().0.kind,
        ErrorKind::InvalidRequest
    );
    let missing = JobId::parse("job_doesnotexist").unwrap();
    assert_eq!(
        core.get(&missing).await.unwrap_err().0.kind,
        ErrorKind::InvalidRequest
    );
}

#[tokio::test]
async fn segmentation_is_used_only_when_supported() {
    let dir = tempfile::tempdir().unwrap();
    let full = core_with(
        FakeBackend::with_all(CapabilitySupport::Supported),
        dir.path(),
    );
    full.add(add_params("seg-key-01", "http://h/a.bin"))
        .await
        .unwrap();
    assert!(full.backend().calls()[0].ends_with("conns=4"));
    let limited = core_with(
        FakeBackend::supported_except(Capability::SegmentedHttp, CapabilitySupport::Unverified),
        dir.path(),
    );
    limited
        .add(add_params("seg-key-02", "http://h/a.bin"))
        .await
        .unwrap();
    assert!(limited.backend().calls()[0].ends_with("conns=1"));
}

#[tokio::test]
async fn pause_and_resume_without_partial_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let core = core_with(
        FakeBackend::with_all(CapabilitySupport::Supported),
        dir.path(),
    );
    let snap = core
        .add(add_params("pause-key-1", "http://h/a.bin"))
        .await
        .unwrap();
    let paused = core.pause(&snap.job_id).await.unwrap();
    assert_eq!(paused.desired_state, DesiredState::Paused);
    let resumed = core.resume(&snap.job_id).await.unwrap();
    assert_eq!(resumed.desired_state, DesiredState::Running);
    let calls = core.backend().calls();
    assert!(
        calls.iter().any(|c| c.starts_with("pause"))
            && calls.iter().any(|c| c.starts_with("resume"))
    );
}

#[tokio::test]
async fn resume_with_partial_bytes_needs_strong_validator() {
    let dir = tempfile::tempdir().unwrap();
    let core = core_with(
        FakeBackend::supported_except(
            Capability::ResumeStrongValidator,
            CapabilitySupport::Unsupported,
        ),
        dir.path(),
    );
    let snap = core
        .add(add_params("resume-key-1", "http://h/a.bin"))
        .await
        .unwrap();
    let handle = handle_of(&snap);
    core.backend()
        .set_progress(&handle, progress(TransferState::Active, 512, Some(1024)));
    core.poll_once().await;
    core.pause(&snap.job_id).await.unwrap();
    let err = core.resume(&snap.job_id).await.unwrap_err();
    assert_eq!(err.0.kind, ErrorKind::BackendCapabilityUnavailable);
    assert_eq!(err.0.next_action, NextAction::ChooseRestartOrKeep);
    assert!(
        !core
            .backend()
            .calls()
            .iter()
            .any(|c| c.starts_with("resume"))
    );
    assert_eq!(
        core.get(&snap.job_id).await.unwrap().desired_state,
        DesiredState::Paused
    );
}

/// The gate must use the engine's current byte count, not the last poll: a pause between two
/// polls would otherwise let a validator-less resume through (B0.4 C06).
#[tokio::test]
async fn resume_gate_uses_fresh_engine_progress_not_the_last_poll() {
    let dir = tempfile::tempdir().unwrap();
    let core = core_with(
        FakeBackend::supported_except(
            Capability::ResumeStrongValidator,
            CapabilitySupport::Unsupported,
        ),
        dir.path(),
    );
    let snap = core
        .add(add_params("resume-key-fresh", "http://h/a.bin"))
        .await
        .unwrap();
    let handle = handle_of(&snap);
    // Bytes arrived after the last poll (no poll_once here).
    core.backend()
        .set_progress(&handle, progress(TransferState::Active, 4096, Some(8192)));
    core.pause(&snap.job_id).await.unwrap();
    let err = core.resume(&snap.job_id).await.unwrap_err();
    assert_eq!(err.0.kind, ErrorKind::BackendCapabilityUnavailable);
    assert!(
        !core
            .backend()
            .calls()
            .iter()
            .any(|c| c.starts_with("resume"))
    );
    assert_eq!(
        core.get(&snap.job_id).await.unwrap().downloaded_bytes,
        ByteCount(4096),
        "the snapshot reflects the bytes the gate saw"
    );
}

#[tokio::test]
async fn cancel_removes_the_transfer_and_late_events_are_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let core = core_with(
        FakeBackend::with_all(CapabilitySupport::Supported),
        dir.path(),
    );
    let snap = core
        .add(add_params("cancel-key-1", "http://h/a.bin"))
        .await
        .unwrap();
    let attempt = snap.attempt_id.clone().unwrap();
    let cancelled = core.cancel(&snap.job_id).await.unwrap();
    assert_eq!(cancelled.phase, Phase::Cancelled);
    assert_eq!(cancelled.attempt_id, None, "attempt invalidated");
    assert!(
        core.backend()
            .calls()
            .iter()
            .any(|c| c.starts_with("remove"))
    );
    // A late engine event for the old attempt must not revive the job.
    core.deliver(
        &snap.job_id,
        &attempt,
        progress(TransferState::Complete, 1024, Some(1024)),
    )
    .await;
    assert_eq!(
        core.get(&snap.job_id).await.unwrap().phase,
        Phase::Cancelled
    );
    // Cancel is idempotent; pause/resume on a cancelled job are refused.
    assert_eq!(
        core.cancel(&snap.job_id).await.unwrap().phase,
        Phase::Cancelled
    );
    assert!(core.resume(&snap.job_id).await.is_err());
}

#[tokio::test]
async fn completion_requires_length_and_expected_hash() {
    let dir = tempfile::tempdir().unwrap();
    let core = core_with(
        FakeBackend::with_all(CapabilitySupport::Supported),
        dir.path(),
    );
    let body = b"hello downova";
    // sha256("hello downova")
    let good = {
        use sha2::{Digest, Sha256};
        Sha256::digest(body)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    };
    let mut params = add_params("hash-key-01", "http://h/ok.txt");
    params.expected_sha256 = Some(good);
    let ok = core.add(params).await.unwrap();
    std::fs::write(dir.path().join(&ok.display_name), body).unwrap();
    core.backend().set_progress(
        &handle_of(&ok),
        progress(TransferState::Complete, 13, Some(13)),
    );
    core.poll_once().await;
    assert_eq!(core.get(&ok.job_id).await.unwrap().phase, Phase::Completed);

    let mut params = add_params("hash-key-02", "http://h/bad.txt");
    params.expected_sha256 = Some("0".repeat(64));
    let bad = core.add(params).await.unwrap();
    std::fs::write(dir.path().join(&bad.display_name), body).unwrap();
    core.backend().set_progress(
        &handle_of(&bad),
        progress(TransferState::Complete, 13, Some(13)),
    );
    core.poll_once().await;
    let failed = core.get(&bad.job_id).await.unwrap();
    assert_eq!(failed.phase, Phase::Failed);
    assert_eq!(failed.error.unwrap().kind, ErrorKind::ChecksumMismatch);
}

#[test]
fn file_names_are_sanitized_and_unique() {
    assert_eq!(file_name_from_url("http://h/"), "download");
    assert_eq!(
        file_name_from_url("http://h/a/..%2F..%2Fevil.exe"),
        ".._.._evil.exe"
    );
    assert_eq!(file_name_from_url("http://h/CON.txt"), "_CON.txt");
    assert_eq!(
        file_name_from_url("http://h/%E0%A6%AC%E0%A6%87.pdf?sig=x"),
        "বই.pdf"
    );
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.zip"), b"x").unwrap();
    assert_eq!(unique_file_name(dir.path(), "a.zip", &[]), "a (1).zip");
    assert_eq!(
        unique_file_name(dir.path(), "a.zip", &["a (1).zip".into()]),
        "a (2).zip"
    );
}

#[tokio::test]
async fn input_errors_are_reported_before_the_capability_gate() {
    let dir = tempfile::tempdir().unwrap();
    let core = core_with(
        FakeBackend::supported_except(Capability::Http, CapabilitySupport::Unverified),
        dir.path(),
    );
    let mut bad_profile = add_params("order-key-1", "http://h/x.bin");
    bad_profile.destination_profile_id = "nope".into();
    let err = core.add(bad_profile).await.unwrap_err();
    assert_eq!(err.0.kind, ErrorKind::InvalidRequest);
    assert_eq!(err.0.detail.as_deref(), Some("unknown destination profile"));

    let mut bad_hash = add_params("order-key-2", "http://h/x.bin");
    bad_hash.expected_sha256 = Some("xyz".into());
    assert_eq!(
        core.add(bad_hash).await.unwrap_err().0.kind,
        ErrorKind::InvalidRequest
    );

    // A valid request is then refused by the gate.
    let gated = core
        .add(add_params("order-key-3", "http://h/x.bin"))
        .await
        .unwrap_err();
    assert_eq!(gated.0.kind, ErrorKind::BackendCapabilityUnavailable);
}

// ---------------------------------------------------------------- automatic retry (COR-04, B0.4.1)

fn failed(kind: ErrorKind, retry_after: Option<u64>) -> BackendProgress {
    let mut e = BackendError::new(kind, "test failure");
    if let Some(secs) = retry_after {
        e = e.with_retry_after(std::time::Duration::from_secs(secs));
    }
    progress(TransferState::Failed(e), 1024, Some(4096))
}

fn starts(core: &Core<FakeBackend>) -> usize {
    core.backend()
        .calls()
        .iter()
        .filter(|c| c.starts_with("start"))
        .count()
}

async fn advance(secs: u64) {
    tokio::time::advance(std::time::Duration::from_secs(secs)).await;
}

#[tokio::test(start_paused = true)]
async fn server_busy_waits_for_retry_after_then_starts_a_fresh_attempt() {
    let dir = tempfile::tempdir().unwrap();
    let core = core_with(
        FakeBackend::with_all(CapabilitySupport::Supported),
        dir.path(),
    );
    let snap = core
        .add(add_params("retry-busy-1", "http://h/a.bin"))
        .await
        .unwrap();
    let first = handle_of(&snap);
    core.backend()
        .set_progress(&first, failed(ErrorKind::ServerBusy, Some(30)));
    core.poll_once().await;

    let s = core.get(&snap.job_id).await.unwrap();
    assert_eq!(s.phase, Phase::Queued, "not failed: a retry is scheduled");
    assert_eq!(s.blockers, vec![Blocker::RateLimitCooldown]);
    assert_eq!(s.attempt_id, None);
    assert_eq!(s.downloaded_bytes, ByteCount(0), "partial discarded");
    assert_eq!(s.error.as_ref().unwrap().kind, ErrorKind::ServerBusy);
    assert!(
        core.backend()
            .calls()
            .contains(&format!("discard {}", first.0)),
        "the failed attempt's partial output is discarded before any retry"
    );

    advance(29).await;
    core.poll_once().await;
    assert_eq!(starts(&core), 1, "never earlier than Retry-After");

    advance(2).await;
    core.poll_once().await;
    assert_eq!(starts(&core), 2);
    let s = core.get(&snap.job_id).await.unwrap();
    assert!(s.blockers.is_empty());
    assert_ne!(s.attempt_id.unwrap().as_str(), first.0, "a new attempt ID");
}

#[tokio::test(start_paused = true)]
async fn network_failure_uses_bounded_backoff_and_then_gives_up() {
    let dir = tempfile::tempdir().unwrap();
    let core = core_with(
        FakeBackend::with_all(CapabilitySupport::Supported),
        dir.path(),
    )
    .with_retry_policy(RetryPolicy {
        max_retries: 2,
        ..RetryPolicy::default()
    });
    let snap = core
        .add(add_params("retry-net-1", "http://h/a.bin"))
        .await
        .unwrap();
    for round in 0..2u64 {
        let current = handle_of(&core.get(&snap.job_id).await.unwrap());
        core.backend()
            .set_progress(&current, failed(ErrorKind::NetworkOffline, None));
        core.poll_once().await;
        let s = core.get(&snap.job_id).await.unwrap();
        assert_eq!(s.phase, Phase::Queued);
        assert!(
            s.blockers.is_empty(),
            "no rate-limit blocker for network errors"
        );
        // base 2 s doubled per retry, equal jitter: at most 2 s, then at most 4 s.
        advance(2 << round).await;
        core.poll_once().await;
        assert_eq!(starts(&core), usize::try_from(round + 2).unwrap());
    }
    let current = handle_of(&core.get(&snap.job_id).await.unwrap());
    core.backend()
        .set_progress(&current, failed(ErrorKind::NetworkOffline, None));
    core.poll_once().await;
    let s = core.get(&snap.job_id).await.unwrap();
    assert_eq!(
        s.phase,
        Phase::Failed,
        "bounded: gives up after max_retries"
    );
    assert_eq!(s.error.unwrap().kind, ErrorKind::NetworkOffline);
}

#[tokio::test(start_paused = true)]
async fn permanent_failures_and_overlong_retry_after_are_not_retried() {
    let dir = tempfile::tempdir().unwrap();
    let core = core_with(
        FakeBackend::with_all(CapabilitySupport::Supported),
        dir.path(),
    );
    for (key, progress) in [
        ("retry-auth-1", failed(ErrorKind::AuthRequired, None)),
        ("retry-link-1", failed(ErrorKind::LinkRefreshRequired, None)),
        ("retry-chg-01", failed(ErrorKind::RemoteChanged, None)),
        ("retry-long-1", failed(ErrorKind::ServerBusy, Some(86_400))),
    ] {
        let snap = core
            .add(add_params(key, &format!("http://h/{key}.bin")))
            .await
            .unwrap();
        core.backend().set_progress(&handle_of(&snap), progress);
        core.poll_once().await;
        let s = core.get(&snap.job_id).await.unwrap();
        assert_eq!(s.phase, Phase::Failed, "{key}");
        assert!(s.blockers.is_empty());
    }
    assert!(
        !core
            .backend()
            .calls()
            .iter()
            .any(|c| c.starts_with("discard")),
        "a failure that is not retried keeps its partial output"
    );
}

#[tokio::test(start_paused = true)]
async fn pause_and_cancel_during_a_cooldown_are_respected() {
    let dir = tempfile::tempdir().unwrap();
    let core = core_with(
        FakeBackend::with_all(CapabilitySupport::Supported),
        dir.path(),
    );
    let paused = core
        .add(add_params("retry-pause-1", "http://h/p.bin"))
        .await
        .unwrap();
    let cancelled = core
        .add(add_params("retry-cancel-1", "http://h/c.bin"))
        .await
        .unwrap();
    for snap in [&paused, &cancelled] {
        core.backend()
            .set_progress(&handle_of(snap), failed(ErrorKind::ServerBusy, Some(10)));
    }
    core.poll_once().await;
    core.pause(&paused.job_id).await.unwrap();
    core.cancel(&cancelled.job_id).await.unwrap();
    // Resume before the cool-down ends: no immediate start.
    advance(5).await;
    core.resume(&paused.job_id).await.unwrap();
    assert_eq!(starts(&core), 2, "resume does not skip the cool-down");
    core.pause(&paused.job_id).await.unwrap();

    advance(10).await;
    core.poll_once().await;
    assert_eq!(
        starts(&core),
        2,
        "paused and cancelled jobs are not restarted"
    );
    let c = core.get(&cancelled.job_id).await.unwrap();
    assert_eq!(c.phase, Phase::Cancelled);
    assert!(c.blockers.is_empty());

    core.resume(&paused.job_id).await.unwrap();
    core.poll_once().await;
    assert_eq!(
        starts(&core),
        3,
        "resumed after the cool-down: started by the poller"
    );
}

#[tokio::test]
async fn pause_reports_the_bytes_the_backend_confirmed() {
    let dir = tempfile::tempdir().unwrap();
    let core = core_with(
        FakeBackend::with_all(CapabilitySupport::Supported),
        dir.path(),
    );
    let snap = core
        .add(add_params("pause-confirmed-1", "http://h/a.bin"))
        .await
        .unwrap();
    // Bytes confirmed by the engine after the last poll.
    core.backend().set_progress(
        &handle_of(&snap),
        progress(TransferState::Paused, 720_896, Some(6_291_456)),
    );
    let paused = core.pause(&snap.job_id).await.unwrap();
    assert_eq!(paused.desired_state, DesiredState::Paused);
    assert_eq!(paused.downloaded_bytes, ByteCount(720_896));
}

// ------------------------------------------------------------------ durable storage (B1.1)

fn stored_core(db: &Path, downloads: &Path) -> Core<FakeBackend> {
    core_with(
        FakeBackend::with_all(CapabilitySupport::Supported),
        downloads,
    )
    .with_store(Store::open(db).unwrap())
    .unwrap()
}

#[tokio::test]
async fn after_a_restart_the_same_key_returns_the_same_job_and_a_new_payload_conflicts() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("downova.sqlite3");
    let first = {
        let core = stored_core(&db, dir.path());
        core.add(add_params("ext_restart_0001", "http://h/x.bin"))
            .await
            .unwrap()
        // The acknowledgement is "lost": the core (service) goes away here.
    };
    let core = stored_core(&db, dir.path());
    let again = core
        .add(add_params("ext_restart_0001", "http://h/x.bin"))
        .await
        .unwrap();
    assert_eq!(again.job_id, first.job_id);
    assert_eq!(core.list().await.len(), 1, "one job, not two");
    let conflict = core
        .add(add_params("ext_restart_0001", "http://h/other.bin"))
        .await
        .unwrap_err();
    assert_eq!(conflict.0.kind, ErrorKind::IdempotencyConflict);
}

#[tokio::test]
async fn an_unfinished_job_is_restored_as_interrupted_with_a_reason() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("downova.sqlite3");
    let (running, done) = {
        let core = stored_core(&db, dir.path());
        let running = core
            .add(add_params("ext_running_001", "http://h/running.bin"))
            .await
            .unwrap();
        let done = core
            .add(add_params("ext_done_00001", "http://h/done.bin"))
            .await
            .unwrap();
        std::fs::write(dir.path().join("done.bin"), b"0123456789abc").unwrap();
        core.backend().set_progress(
            &handle_of(&done),
            progress(TransferState::Complete, 13, Some(13)),
        );
        core.poll_once().await;
        assert_eq!(
            core.get(&done.job_id).await.unwrap().phase,
            Phase::Completed
        );
        (running, done)
    };
    let core = stored_core(&db, dir.path());
    let r = core.get(&running.job_id).await.unwrap();
    assert_eq!(r.phase, Phase::Failed);
    assert_eq!(r.attempt_id, None);
    assert_eq!(r.error.unwrap().detail.as_deref(), Some(INTERRUPTED));
    let d = core.get(&done.job_id).await.unwrap();
    assert_eq!(d.phase, Phase::Completed);
    assert_eq!(d.downloaded_bytes, ByteCount(13));
    // The interruption itself is durable.
    drop(core);
    let core = stored_core(&db, dir.path());
    assert_eq!(
        core.get(&running.job_id).await.unwrap().phase,
        Phase::Failed
    );
}

#[tokio::test]
async fn pause_and_cancel_survive_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("downova.sqlite3");
    let (paused, cancelled) = {
        let core = stored_core(&db, dir.path());
        let a = core
            .add(add_params("ext_pause_0001", "http://h/a.bin"))
            .await
            .unwrap();
        let b = core
            .add(add_params("ext_cancel_001", "http://h/b.bin"))
            .await
            .unwrap();
        core.pause(&a.job_id).await.unwrap();
        core.cancel(&b.job_id).await.unwrap();
        (a.job_id, b.job_id)
    };
    let core = stored_core(&db, dir.path());
    let a = core.get(&paused).await.unwrap();
    // Paused but unfinished: interrupted, and the user's pause is still recorded.
    assert_eq!(a.desired_state, DesiredState::Paused);
    assert_eq!(a.phase, Phase::Failed);
    let b = core.get(&cancelled).await.unwrap();
    assert_eq!(b.phase, Phase::Cancelled);
    assert_eq!(b.desired_state, DesiredState::Cancelled);
}

#[tokio::test]
async fn the_database_never_holds_the_raw_fingerprint() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("downova.sqlite3");
    {
        let core = stored_core(&db, dir.path());
        core.add(add_params(
            "ext_secret_001",
            "https://h/f.zip?token=SYNTHETIC-SECRET",
        ))
        .await
        .unwrap();
    }
    let store = Store::open(&db).unwrap();
    let rec = store.find_idempotency("ext_secret_001").unwrap().unwrap();
    assert_eq!(rec.fingerprint_sha256.len(), 64);
    assert!(!rec.fingerprint_sha256.contains("SYNTHETIC"));
}

#[tokio::test]
async fn a_job_that_cannot_be_recorded_is_not_started() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_in_memory().unwrap();
    // Occupy the key in the database only, so the insert fails.
    store
        .insert_job(
            &dm_storage::JobRow {
                id: JobId::parse("job_00000000000000ff").unwrap(),
                url: "http://h/other".into(),
                display_name: "other".into(),
                destination: dir.path().to_owned(),
                desired_state: DesiredState::Running,
                phase: Phase::Completed,
                downloaded: ByteCount(0),
                total: None,
                expected_sha256: None,
                error: None,
                attempt_id: None,
                created_at_ms: now_ms(),
                updated_at_ms: now_ms(),
            },
            None,
        )
        .unwrap();
    let core = core_with(
        FakeBackend::with_all(CapabilitySupport::Supported),
        dir.path(),
    )
    .with_store(store)
    .unwrap();
    {
        let mut state = core.state.lock().await;
        state.idempotency.clear();
        let store = state.store.as_mut().unwrap();
        store
            .insert_job(
                &dm_storage::JobRow {
                    id: JobId::parse("job_00000000000000fe").unwrap(),
                    url: "http://h/x".into(),
                    display_name: "x".into(),
                    destination: dir.path().to_owned(),
                    desired_state: DesiredState::Running,
                    phase: Phase::Completed,
                    downloaded: ByteCount(0),
                    total: None,
                    expected_sha256: None,
                    error: None,
                    attempt_id: None,
                    created_at_ms: now_ms(),
                    updated_at_ms: now_ms(),
                },
                Some(&IdempotencyRow {
                    key: "ext_taken_0001".into(),
                    fingerprint_sha256: "00".repeat(32),
                    job_id: JobId::parse("job_00000000000000fe").unwrap(),
                    created_at_ms: now_ms(),
                }),
            )
            .unwrap();
    }
    let err = core
        .add(add_params("ext_taken_0001", "http://h/new.bin"))
        .await
        .unwrap_err();
    assert_eq!(err.0.kind, ErrorKind::Internal);
    assert!(
        !core
            .backend()
            .calls()
            .iter()
            .any(|c| c.starts_with("start")),
        "nothing was started: {:?}",
        core.backend().calls()
    );
}

// ------------------------------------------------------------------ durable checkpoints (B1.2)

fn durable(confirmed: u64) -> DurableCheckpoint {
    DurableCheckpoint {
        backend: "fake".into(),
        format_version: 1,
        confirmed: ByteCount(confirmed),
        payload: format!("{{\"confirmed\":{confirmed}}}").into_bytes(),
    }
}

fn reopen(db: &Path, dir: &Path, fake: FakeBackend) -> Core<FakeBackend> {
    core_with(fake, dir)
        .with_store(Store::open(db).unwrap())
        .unwrap()
}

#[tokio::test]
async fn a_paused_job_is_restored_from_its_checkpoint_and_stays_paused() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("downova.sqlite3");
    let (job, old_attempt) = {
        let core = stored_core(&db, dir.path());
        let s = core
            .add(add_params("ext_paused_001", "http://h/p.bin"))
            .await
            .unwrap();
        core.backend()
            .set_checkpoint(&handle_of(&s), Ok(Some(durable(4096))));
        let paused = core.pause(&s.job_id).await.unwrap();
        assert_eq!(paused.desired_state, DesiredState::Paused);
        (s.job_id, s.attempt_id.unwrap())
    };
    let core = reopen(
        &db,
        dir.path(),
        FakeBackend::with_all(CapabilitySupport::Supported),
    );
    assert_eq!(core.recover().await, 1);
    let restored = core.backend().restored.lock().unwrap().clone();
    assert_eq!(restored.len(), 1);
    assert_eq!(
        restored[0].1,
        durable(4096),
        "the stored checkpoint, unchanged"
    );
    let snap = core.get(&job).await.unwrap();
    let new_attempt = snap.attempt_id.clone().unwrap();
    assert_ne!(new_attempt, old_attempt, "a new service-owned attempt");
    assert_eq!(restored[0].0.attempt_id, new_attempt);
    assert_eq!(snap.desired_state, DesiredState::Paused);
    assert_eq!(snap.downloaded_bytes, ByteCount(4096));
    assert!(
        !core
            .backend()
            .calls()
            .iter()
            .any(|c| c.starts_with("resume")),
        "a paused job is not transferred automatically"
    );
    // A late event from the old attempt cannot touch the recovered job.
    core.deliver(
        &job,
        &old_attempt,
        progress(TransferState::Complete, 9, Some(9)),
    )
    .await;
    assert_ne!(core.get(&job).await.unwrap().phase, Phase::Completed);
    // The checkpoint now belongs to the new attempt, so another restart restores it again.
    drop(core);
    let core = reopen(
        &db,
        dir.path(),
        FakeBackend::with_all(CapabilitySupport::Supported),
    );
    assert_eq!(core.recover().await, 1);
}

#[tokio::test]
async fn a_running_job_with_a_checkpoint_continues_after_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("downova.sqlite3");
    let job = {
        let core = stored_core(&db, dir.path());
        let s = core
            .add(add_params("ext_running_02", "http://h/r.bin"))
            .await
            .unwrap();
        core.backend()
            .set_checkpoint(&handle_of(&s), Ok(Some(durable(1 << 20))));
        core.poll_once().await; // the first periodic checkpoint is due at once
        s.job_id
    };
    let core = reopen(
        &db,
        dir.path(),
        FakeBackend::with_all(CapabilitySupport::Supported),
    );
    assert_eq!(core.recover().await, 1);
    let snap = core.get(&job).await.unwrap();
    assert_eq!(snap.desired_state, DesiredState::Running);
    assert_ne!(snap.phase, Phase::Failed);
    let calls = core.backend().calls();
    assert!(calls.iter().any(|c| c.starts_with("restore")), "{calls:?}");
    assert!(calls.iter().any(|c| c.starts_with("resume")), "{calls:?}");
    // Durable idempotency still returns the one job.
    let again = core
        .add(add_params("ext_running_02", "http://h/r.bin"))
        .await
        .unwrap();
    assert_eq!(again.job_id, job);
    assert_eq!(core.list().await.len(), 1);
}

#[tokio::test]
async fn a_refused_restore_fails_the_job_explicitly_and_drops_the_checkpoint() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("downova.sqlite3");
    let job = {
        let core = stored_core(&db, dir.path());
        let s = core
            .add(add_params("ext_refused_01", "http://h/x.bin"))
            .await
            .unwrap();
        core.backend()
            .set_checkpoint(&handle_of(&s), Ok(Some(durable(10))));
        core.poll_once().await;
        s.job_id
    };
    let fake = FakeBackend::with_all(CapabilitySupport::Supported);
    *fake.restore_error.lock().unwrap() = Some(BackendError::new(
        ErrorKind::InvalidRange,
        "the partial file was modified after the last saved resume point",
    ));
    let core = reopen(&db, dir.path(), fake);
    assert_eq!(core.recover().await, 0);
    let snap = core.get(&job).await.unwrap();
    assert_eq!(snap.phase, Phase::Failed);
    let error = snap.error.unwrap();
    assert_eq!(error.kind, ErrorKind::InvalidRange);
    assert!(error.detail.unwrap().starts_with(RECOVERY_FAILED));
    assert!(
        !core
            .backend()
            .calls()
            .iter()
            .any(|c| c.starts_with("start"))
    );
    drop(core);
    let store = Store::open(&db).unwrap();
    assert!(store.load_checkpoint(&job).unwrap().is_none());
}

#[tokio::test]
async fn without_durable_resume_a_stored_checkpoint_is_not_used() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("downova.sqlite3");
    let job = {
        let core = stored_core(&db, dir.path());
        let s = core
            .add(add_params("ext_nodurable1", "http://h/x.bin"))
            .await
            .unwrap();
        core.backend()
            .set_checkpoint(&handle_of(&s), Ok(Some(durable(10))));
        core.poll_once().await;
        s.job_id
    };
    let core = reopen(
        &db,
        dir.path(),
        FakeBackend::supported_except(Capability::DurableResume, CapabilitySupport::Unsupported),
    );
    assert_eq!(core.recover().await, 0);
    let snap = core.get(&job).await.unwrap();
    assert_eq!(snap.phase, Phase::Failed);
    assert_eq!(snap.error.unwrap().detail.as_deref(), Some(INTERRUPTED));
    assert!(
        !core
            .backend()
            .calls()
            .iter()
            .any(|c| c.starts_with("restore"))
    );
}

#[tokio::test]
async fn checkpoints_are_committed_only_when_durable_due_and_growing() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("downova.sqlite3");
    let core = stored_core(&db, dir.path());
    let s = core
        .add(add_params("ext_periodic_1", "http://h/x.bin"))
        .await
        .unwrap();
    let h = handle_of(&s);
    // A backend failure is not a checkpoint.
    core.backend().set_checkpoint(
        &h,
        Err(BackendError::new(ErrorKind::Internal, "sync failed")),
    );
    core.poll_once().await;
    {
        let state = core.state.lock().await;
        assert_eq!(state.jobs[0].durable, 0);
        assert!(
            state
                .store
                .as_ref()
                .unwrap()
                .load_checkpoint(&s.job_id)
                .unwrap()
                .is_none()
        );
    }
    // Interval not yet elapsed: nothing new even though the backend has a checkpoint.
    core.backend().set_checkpoint(&h, Ok(Some(durable(100))));
    core.poll_once().await;
    assert_eq!(core.state.lock().await.jobs[0].durable, 0);
    // Due: committed.
    core.state.lock().await.jobs[0].checkpointed_at = None;
    core.poll_once().await;
    assert_eq!(core.state.lock().await.jobs[0].durable, 100);
    {
        let state = core.state.lock().await;
        let stored = state
            .store
            .as_ref()
            .unwrap()
            .load_checkpoint(&s.job_id)
            .unwrap()
            .unwrap();
        assert_eq!(stored.confirmed, ByteCount(100));
        assert_eq!(Some(stored.attempt_id), s.attempt_id);
    }
}

#[tokio::test]
async fn cancelled_and_completed_jobs_never_restart() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("downova.sqlite3");
    {
        let core = stored_core(&db, dir.path());
        let a = core
            .add(add_params("ext_cancel_b12", "http://h/a.bin"))
            .await
            .unwrap();
        core.backend()
            .set_checkpoint(&handle_of(&a), Ok(Some(durable(10))));
        core.poll_once().await;
        core.cancel(&a.job_id).await.unwrap();
    }
    let core = reopen(
        &db,
        dir.path(),
        FakeBackend::with_all(CapabilitySupport::Supported),
    );
    assert_eq!(core.recover().await, 0);
    assert_eq!(core.list().await[0].phase, Phase::Cancelled);
    assert!(
        core.backend().calls().is_empty(),
        "{:?}",
        core.backend().calls()
    );
}

#[tokio::test]
async fn a_graceful_shutdown_checkpoints_running_jobs_which_continue_afterwards() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("downova.sqlite3");
    let job = {
        let core = stored_core(&db, dir.path());
        let s = core
            .add(add_params("ext_shutdown01", "http://h/s.bin"))
            .await
            .unwrap();
        core.backend()
            .set_checkpoint(&handle_of(&s), Ok(Some(durable(777))));
        core.prepare_shutdown().await;
        let calls = core.backend().calls();
        assert!(calls.iter().any(|c| c.starts_with("pause")), "{calls:?}");
        assert_eq!(
            core.get(&s.job_id).await.unwrap().desired_state,
            DesiredState::Running,
            "shutdown is not a user pause"
        );
        s.job_id
    };
    let core = reopen(
        &db,
        dir.path(),
        FakeBackend::with_all(CapabilitySupport::Supported),
    );
    assert_eq!(core.recover().await, 1);
    assert_eq!(
        core.get(&job).await.unwrap().downloaded_bytes,
        ByteCount(777)
    );
    assert!(
        core.backend()
            .calls()
            .iter()
            .any(|c| c.starts_with("resume"))
    );
}
