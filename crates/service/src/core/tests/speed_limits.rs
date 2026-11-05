//! CORE-06 in the service (ADR 0024): persist, gate, apply, restore.
use super::*;
use dm_domain::api::{SetJobSpeedLimitParams, SpeedLimitSettings};

fn calls(core: &Core<FakeBackend>) -> Vec<String> {
    core.backend().calls.lock().unwrap().clone()
}
fn global(bps: Option<u64>) -> SpeedLimitSettings {
    SpeedLimitSettings {
        global_bytes_per_second: bps.map(ByteCount),
    }
}

#[tokio::test]
async fn the_global_limit_is_saved_applied_and_restored_before_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("downova.sqlite");
    {
        let core = reopen(
            &db,
            dir.path(),
            FakeBackend::with_all(CapabilitySupport::Supported),
        );
        assert_eq!(
            core.speed_limit_settings().await,
            global(None),
            "unlimited by default"
        );
        core.set_speed_limit(global(Some(1_048_576))).await.unwrap();
        assert_eq!(core.speed_limit_settings().await, global(Some(1_048_576)));
        assert!(calls(&core).contains(&"global_rate Some(1048576)".to_owned()));
    }
    let core = reopen(
        &db,
        dir.path(),
        FakeBackend::with_all(CapabilitySupport::Supported),
    );
    assert_eq!(core.speed_limit_settings().await, global(Some(1_048_576)));
    // Applied while the store was attached, before anything could start or recover.
    assert_eq!(
        calls(&core).first().map(String::as_str),
        Some("global_rate Some(1048576)")
    );
    core.set_speed_limit(global(None)).await.unwrap();
    assert!(calls(&core).contains(&"global_rate None".to_owned()));
}

#[tokio::test]
async fn an_unsupported_engine_refuses_a_limit_but_accepts_unlimited() {
    let dir = tempfile::tempdir().unwrap();
    let fake =
        FakeBackend::supported_except(Capability::SpeedLimitGlobal, CapabilitySupport::Unverified);
    let core = core_with(fake, dir.path());
    let e = core.set_speed_limit(global(Some(4096))).await.unwrap_err();
    assert_eq!(e.0.kind, ErrorKind::BackendCapabilityUnavailable);
    assert_eq!(core.speed_limit_settings().await, global(None));
    core.set_speed_limit(global(None)).await.unwrap();

    let fake =
        FakeBackend::supported_except(Capability::SpeedLimitPerJob, CapabilitySupport::Unsupported);
    let core = core_with(fake, dir.path());
    let mut p = add_params("speed-refused-0001", "http://h/a.bin");
    p.speed_limit_bytes_per_second = Some(ByteCount(4096));
    let e = core.add(p).await.unwrap_err();
    assert_eq!(e.0.kind, ErrorKind::BackendCapabilityUnavailable);
    assert!(core.list().await.is_empty(), "a refused add leaves no job");
}

#[tokio::test]
async fn a_refused_save_changes_neither_the_stored_nor_the_effective_limit() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("downova.sqlite");
    {
        let core = reopen(
            &db,
            dir.path(),
            FakeBackend::with_all(CapabilitySupport::Supported),
        );
        core.set_speed_limit(global(Some(500_000))).await.unwrap();
    }
    rusqlite::Connection::open(&db)
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER refuse_speed BEFORE UPDATE ON settings \
             WHEN NEW.key = 'speed_limit_global_bps' BEGIN SELECT RAISE(ABORT, 'refused'); END;",
        )
        .unwrap();
    let core = reopen(
        &db,
        dir.path(),
        FakeBackend::with_all(CapabilitySupport::Supported),
    );
    let before = calls(&core).len();
    let e = core
        .set_speed_limit(global(Some(2_000_000)))
        .await
        .unwrap_err();
    assert_eq!(e.0.kind, ErrorKind::Internal);
    assert_eq!(core.speed_limit_settings().await, global(Some(500_000)));
    assert_eq!(
        calls(&core).len(),
        before,
        "nothing was applied to the engine"
    );
}

#[tokio::test]
async fn a_download_limit_reaches_its_attempt_changes_live_and_survives_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("downova.sqlite");
    let job_id;
    {
        let core = reopen(
            &db,
            dir.path(),
            FakeBackend::with_all(CapabilitySupport::Supported),
        );
        let mut p = add_params("speed-job-0001", "http://h/limited.bin");
        p.speed_limit_bytes_per_second = Some(ByteCount(262_144));
        let s = core.add(p).await.unwrap();
        job_id = s.job_id.clone();
        assert_eq!(s.speed_limit_bytes_per_second, Some(ByteCount(262_144)));
        let started = core.backend().started.lock().unwrap().clone();
        assert_eq!(
            started.last().unwrap().rate_limit,
            Some(262_144),
            "applied from the first byte"
        );
        // Live change on the running attempt, then removal.
        let s = core
            .set_job_speed_limit(SetJobSpeedLimitParams {
                job_id: job_id.clone(),
                bytes_per_second: Some(ByteCount(8192)),
            })
            .await
            .unwrap();
        assert_eq!(s.speed_limit_bytes_per_second, Some(ByteCount(8192)));
        let attempt = s.attempt_id.unwrap();
        assert!(calls(&core).contains(&format!("rate {} Some(8192)", attempt.as_str())));
        // Removal applies live too (only the global limit remains), and a new value again.
        let removed = core
            .set_job_speed_limit(SetJobSpeedLimitParams {
                job_id: job_id.clone(),
                bytes_per_second: None,
            })
            .await
            .unwrap();
        assert_eq!(removed.speed_limit_bytes_per_second, None);
        assert!(calls(&core).contains(&format!("rate {} None", attempt.as_str())));
        core.set_job_speed_limit(SetJobSpeedLimitParams {
            job_id: job_id.clone(),
            bytes_per_second: Some(ByteCount(8192)),
        })
        .await
        .unwrap();
    }
    // After a restart the job's own limit is back with it (here the fake backend has no
    // checkpoint, so the job is restored as interrupted; its stored limit still reads back).
    let core = reopen(
        &db,
        dir.path(),
        FakeBackend::with_all(CapabilitySupport::Supported),
    );
    let s = core.get(&job_id).await.unwrap();
    assert_eq!(
        s.speed_limit_bytes_per_second,
        Some(ByteCount(8192)),
        "restored with the job"
    );
}

#[tokio::test]
async fn a_finished_download_keeps_its_limit_and_unknown_jobs_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let core = core_with(
        FakeBackend::with_all(CapabilitySupport::Supported),
        dir.path(),
    );
    let s = core
        .add(add_params("speed-done-0001", "http://h/done.bin"))
        .await
        .unwrap();
    core.cancel(&s.job_id).await.unwrap();
    let e = core
        .set_job_speed_limit(SetJobSpeedLimitParams {
            job_id: s.job_id.clone(),
            bytes_per_second: Some(ByteCount(4096)),
        })
        .await
        .unwrap_err();
    assert_eq!(e.0.kind, ErrorKind::InvalidRequest);
    let e = core
        .set_job_speed_limit(SetJobSpeedLimitParams {
            job_id: JobId::parse("job_doesnotexist").unwrap(),
            bytes_per_second: None,
        })
        .await
        .unwrap_err();
    assert_eq!(e.0.kind, ErrorKind::InvalidRequest);
}

#[test]
fn old_requests_keep_their_fingerprint_and_a_limit_makes_a_different_request() {
    let plain = add_params("fp-0001", "http://h/a.bin");
    let mut with_queue = plain.clone();
    with_queue.queue_id = Some("main".into());
    let mut limited = with_queue.clone();
    limited.speed_limit_bytes_per_second = Some(ByteCount(4096));
    // Unchanged for requests without a limit: the CORE-06 element is absent, not null.
    assert!(
        !serde_json::to_string(&plain)
            .unwrap()
            .contains("speed_limit")
    );
    assert_ne!(
        fingerprint_sha256(&with_queue),
        fingerprint_sha256(&limited)
    );
    assert_eq!(
        fingerprint_sha256(&with_queue),
        fingerprint_sha256(&with_queue.clone())
    );
}

#[tokio::test]
async fn replaying_a_key_with_a_different_limit_is_a_conflict() {
    let dir = tempfile::tempdir().unwrap();
    let core = core_with(
        FakeBackend::with_all(CapabilitySupport::Supported),
        dir.path(),
    );
    core.add(add_params("fp-replay-0001", "http://h/r.bin"))
        .await
        .unwrap();
    let mut again = add_params("fp-replay-0001", "http://h/r.bin");
    again.speed_limit_bytes_per_second = Some(ByteCount(4096));
    let e = core.add(again).await.unwrap_err();
    assert_eq!(e.0.kind, ErrorKind::IdempotencyConflict);
}

#[tokio::test]
async fn a_recovered_transfer_is_restored_with_its_limit_before_any_body_byte() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("downova.sqlite3");
    {
        let core = stored_core(&db, dir.path());
        core.set_speed_limit(global(Some(65_536))).await.unwrap();
        let mut p = add_params("speed-recover-01", "http://h/rec.bin");
        p.speed_limit_bytes_per_second = Some(ByteCount(16_384));
        let s = core.add(p).await.unwrap();
        core.backend()
            .set_checkpoint(&handle_of(&s), Ok(Some(durable(4096))));
        core.pause(&s.job_id).await.unwrap();
    }
    let core = reopen(
        &db,
        dir.path(),
        FakeBackend::with_all(CapabilitySupport::Supported),
    );
    // The global limit was in the engine before recovery ran…
    assert_eq!(
        calls(&core).first().map(String::as_str),
        Some("global_rate Some(65536)")
    );
    assert_eq!(core.recover().await, 1);
    // …and the restored attempt's request carries the job's own limit.
    let restored = core.backend().restored.lock().unwrap().clone();
    assert_eq!(restored.len(), 1);
    assert_eq!(restored[0].0.rate_limit, Some(16_384));
}
