//! B2.16/B2.17/B2.19 explicit operations (ADR 0020) against the fake backend: Retry eligibility,
//! fresh attempts, operation replay and conflicts, attempt guards, stale callbacks, admission,
//! retention after a retry, Refresh link proofs and choices, and Locate.

use super::*;
use dm_backend_api::ReplacementCheck;
use dm_domain::api::{
    HistorySettings, ReplaceMode, ReplaceOutcome, ReplaceReason, ReplaceSourceParams, RetryParams,
};

const DAY: u64 = 24 * 60 * 60 * 1000;

fn supported(dir: &Path) -> Core<FakeBackend> {
    core_with(FakeBackend::with_all(CapabilitySupport::Supported), dir)
}

fn retry(job: &DownloadSnapshot, op: &str, discard: bool) -> RetryParams {
    RetryParams {
        job_id: job.job_id.clone(),
        operation_id: op.into(),
        expected_attempt_id: None,
        discard_partial: discard,
    }
}

fn replace(job: &DownloadSnapshot, op: &str, url: &str, mode: ReplaceMode) -> ReplaceSourceParams {
    ReplaceSourceParams {
        job_id: job.job_id.clone(),
        operation_id: op.into(),
        url: url.into(),
        mode,
        expected_attempt_id: None,
    }
}

/// A job that failed with `kind` after `bytes` bytes.
async fn failed(
    core: &Core<FakeBackend>,
    key: &str,
    kind: ErrorKind,
    bytes: u64,
) -> DownloadSnapshot {
    let job = core
        .add(add_params(
            &format!("ext_{key}"),
            &format!("http://h/{key}.bin"),
        ))
        .await
        .unwrap();
    // Progress first, so the job holds partial data when it fails.
    core.backend().set_progress(
        &handle_of(&job),
        progress(TransferState::Active, bytes, Some(4096)),
    );
    core.poll_once().await;
    core.backend().set_progress(
        &handle_of(&job),
        progress(
            TransferState::Failed(BackendError::new(kind, "x")),
            bytes,
            Some(4096),
        ),
    );
    core.poll_once().await;
    let done = core.get(&job.job_id).await.unwrap();
    assert_eq!(done.phase, Phase::Failed);
    done
}

fn starts(core: &Core<FakeBackend>) -> usize {
    core.backend().started.lock().unwrap().len()
}

#[tokio::test]
async fn retry_confirms_discarding_partial_data_then_starts_one_fresh_attempt() {
    let dir = tempfile::tempdir().unwrap();
    let core = supported(dir.path());
    let job = failed(&core, "rt-0001", ErrorKind::RemoteChanged, 1024).await;
    let old = job.attempt_id.clone().unwrap();
    // Partial data exists and no resume point can be kept: the client must confirm.
    let refused = core
        .retry(retry(&job, "op_rt_000001", false))
        .await
        .unwrap_err();
    assert_eq!(refused.0.next_action, NextAction::ChooseRestartOrKeep);
    assert_eq!(starts(&core), 1, "nothing started");
    let retried = core.retry(retry(&job, "op_rt_000002", true)).await.unwrap();
    assert_eq!(
        retried.job_id, job.job_id,
        "the same logical job, not a new one"
    );
    assert_ne!(retried.attempt_id.as_ref(), Some(&old), "a fresh attempt");
    assert_eq!(retried.phase, Phase::Queued);
    assert_eq!(retried.downloaded_bytes, ByteCount(0));
    assert!(retried.error.is_none());
    assert!(core.backend().calls().contains(&format!("discard {old}")));
    assert_eq!(starts(&core), 2);
    // Double click / lost acknowledgement: the same operation answers again, no third attempt.
    let again = core.retry(retry(&job, "op_rt_000002", true)).await.unwrap();
    assert_eq!(again.attempt_id, retried.attempt_id);
    assert_eq!(starts(&core), 2);
    // The same id for a different request conflicts; a new id finds nothing failed.
    let conflict = core
        .retry(retry(&job, "op_rt_000002", false))
        .await
        .unwrap_err();
    assert_eq!(conflict.0.kind, ErrorKind::IdempotencyConflict);
    let not_failed = core
        .retry(retry(&job, "op_rt_000003", true))
        .await
        .unwrap_err();
    assert_eq!(not_failed.0.kind, ErrorKind::InvalidRequest);
    assert_eq!(starts(&core), 2);
}

#[tokio::test]
async fn retry_keeps_queue_priority_names_destination_and_trusted_checksum() {
    let dir = tempfile::tempdir().unwrap();
    let core = supported(dir.path());
    let mut params = add_params("ext_rt-keep01", "http://h/keep.bin");
    params.expected_sha256 = Some("A".repeat(64));
    let job = core.add(params).await.unwrap();
    core.set_priority(&job.job_id, Priority::High)
        .await
        .unwrap();
    core.backend().set_progress(
        &handle_of(&job),
        progress(
            TransferState::Failed(BackendError::new(ErrorKind::ChecksumMismatch, "x")),
            0,
            None,
        ),
    );
    core.poll_once().await;
    let before = core.get(&job.job_id).await.unwrap();
    let retried = core
        .retry(retry(&before, "op_keep_0001", false))
        .await
        .unwrap();
    assert_eq!(retried.priority, Some(Priority::High));
    assert_eq!(retried.queue_id, before.queue_id);
    assert_eq!(retried.display_name, before.display_name);
    assert!(retried.checksum_expected, "the trusted checksum is kept");
    let started = core.backend().started.lock().unwrap().clone();
    assert_eq!(started[0].destination_dir, started[1].destination_dir);
    assert_eq!(started[0].file_name, started[1].file_name);
}

#[tokio::test]
async fn retry_refuses_completed_cancelled_removed_and_link_or_sign_in_failures() {
    let dir = tempfile::tempdir().unwrap();
    let core = supported(dir.path());
    let expired = failed(&core, "rt-exp01", ErrorKind::LinkRefreshRequired, 0).await;
    let e = core
        .retry(retry(&expired, "op_exp_00001", true))
        .await
        .unwrap_err();
    assert_eq!(e.0.next_action, NextAction::RefreshLink);
    let auth = failed(&core, "rt-auth1", ErrorKind::AuthRequired, 0).await;
    let e = core
        .retry(retry(&auth, "op_auth_0001", true))
        .await
        .unwrap_err();
    assert_eq!(e.0.next_action, NextAction::SignIn);
    let running = core
        .add(add_params("ext_rt-run01", "http://h/run.bin"))
        .await
        .unwrap();
    core.cancel(&running.job_id).await.unwrap();
    let e = core
        .retry(retry(&running, "op_can_00001", true))
        .await
        .unwrap_err();
    assert_eq!(e.0.kind, ErrorKind::InvalidRequest);
    let removed = failed(&core, "rt-rem01", ErrorKind::RemoteChanged, 0).await;
    core.remove(&removed.job_id).await.unwrap();
    let e = core
        .retry(retry(&removed, "op_rem_00001", true))
        .await
        .unwrap_err();
    assert!(e.0.detail.unwrap().contains("removed"));
}

#[tokio::test]
async fn a_stale_attempt_guard_refuses_and_old_attempt_callbacks_are_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let core = supported(dir.path());
    let job = failed(&core, "rt-grd01", ErrorKind::RemoteChanged, 0).await;
    let old = job.attempt_id.clone().unwrap();
    let mut guarded = retry(&job, "op_grd_00001", false);
    guarded.expected_attempt_id = Some(AttemptId::parse("att_00000000000000ff").unwrap());
    assert_eq!(
        core.retry(guarded).await.unwrap_err().0.kind,
        ErrorKind::InvalidRequest
    );
    let mut guarded = retry(&job, "op_grd_00002", false);
    guarded.expected_attempt_id = Some(old.clone());
    let retried = core.retry(guarded).await.unwrap();
    // A late "complete" from the old attempt cannot finish the retried job.
    core.deliver(
        &job.job_id,
        &old,
        progress(TransferState::Complete, 4096, Some(4096)),
    )
    .await;
    let now = core.get(&job.job_id).await.unwrap();
    assert_eq!(now.phase, Phase::Queued);
    assert_eq!(now.attempt_id, retried.attempt_id);
}

#[tokio::test]
async fn a_retried_job_waits_for_admission_and_a_stopped_queue() {
    let dir = tempfile::tempdir().unwrap();
    let core = supported(dir.path()).with_max_active(1);
    let job = failed(&core, "rt-adm01", ErrorKind::RemoteChanged, 0).await;
    let other = failed(&core, "rt-adm02", ErrorKind::RemoteChanged, 0).await;
    let busy = core
        .add(add_params("ext_rt-busy1", "http://h/busy.bin"))
        .await
        .unwrap();
    assert!(
        busy.attempt_id.is_some(),
        "the other job holds the only slot"
    );
    let retried = core
        .retry(retry(&job, "op_adm_00001", false))
        .await
        .unwrap();
    assert!(retried.attempt_id.is_none());
    assert!(retried.blockers.contains(&Blocker::QueueLimit));
    core.cancel(&busy.job_id).await.unwrap();
    core.poll_once().await;
    assert!(core.get(&job.job_id).await.unwrap().attempt_id.is_some());
    // A stopped queue holds a retried job without changing the user's intent.
    core.set_queue_running(DEFAULT_QUEUE, false).await.unwrap();
    let held = core
        .retry(retry(&other, "op_adm_00002", false))
        .await
        .unwrap();
    assert!(held.blockers.contains(&Blocker::QueueStopped));
    assert_eq!(held.desired_state, DesiredState::Running);
}

#[tokio::test]
async fn retention_counts_from_the_retried_outcome_not_the_old_failure() {
    let dir = tempfile::tempdir().unwrap();
    let core = supported(dir.path());
    core.set_history(HistorySettings {
        retention_days: Some(7),
    })
    .await
    .unwrap();
    let job = failed(&core, "rt-ret01", ErrorKind::RemoteChanged, 0).await;
    // The failure is old enough to expire.
    core.state
        .lock()
        .await
        .job_mut(&job.job_id)
        .unwrap()
        .finished_at_ms = Some(0);
    let retried = core
        .retry(retry(&job, "op_ret_00001", false))
        .await
        .unwrap();
    std::fs::write(staged(dir.path(), &retried), b"done").unwrap();
    core.backend().set_progress(
        &handle_of(&retried),
        progress(TransferState::Complete, 4, Some(4)),
    );
    core.poll_once().await;
    assert_eq!(core.get(&job.job_id).await.unwrap().phase, Phase::Completed);
    let now = now_ms();
    let swept = core.sweep_history_at(now, 100).await;
    assert_eq!(
        swept.removed, 0,
        "a newly completed retry never expires at once"
    );
    assert!(core.list().await.iter().any(|j| j.job_id == job.job_id));
    let swept = core.sweep_history_at(now + 8 * DAY, 100).await;
    assert_eq!(swept.removed, 1);
}

#[tokio::test]
async fn a_retry_operation_survives_a_restart_and_replays_without_a_new_attempt() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("downova.sqlite3");
    let job_id;
    {
        let core = stored_core(&db, dir.path());
        let job = failed(&core, "rt-dur01", ErrorKind::RemoteChanged, 0).await;
        job_id = job.job_id.clone();
        core.retry(retry(&job, "op_dur_00001", false))
            .await
            .unwrap();
    }
    let core = stored_core(&db, dir.path());
    let snap = core.get(&job_id).await.unwrap();
    let replayed = core
        .retry(retry(&snap, "op_dur_00001", false))
        .await
        .unwrap();
    assert_eq!(replayed.job_id, job_id);
    assert_eq!(
        starts(&core),
        0,
        "the restart's admission is the only start path"
    );
    let conflict = core
        .retry(retry(&snap, "op_dur_00001", true))
        .await
        .unwrap_err();
    assert_eq!(conflict.0.kind, ErrorKind::IdempotencyConflict);
}

#[tokio::test]
async fn retry_after_a_restart_releases_the_leftover_partial_by_name() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("downova.sqlite3");
    let job_id;
    {
        let core = stored_core(&db, dir.path());
        job_id = failed(&core, "rt-left1", ErrorKind::RemoteChanged, 512)
            .await
            .job_id;
    }
    let core = stored_core(&db, dir.path());
    let snap = core.get(&job_id).await.unwrap();
    assert!(snap.attempt_id.is_none());
    core.backend()
        .discard_output_error
        .lock()
        .unwrap()
        .replace(BackendError::new(
            ErrorKind::BackendCapabilityUnavailable,
            "cannot",
        ));
    let refused = core
        .retry(retry(&snap, "op_left_0001", true))
        .await
        .unwrap_err();
    assert_eq!(refused.0.kind, ErrorKind::BackendCapabilityUnavailable);
    assert_eq!(
        core.get(&job_id).await.unwrap().phase,
        Phase::Failed,
        "unchanged"
    );
    core.retry(retry(&snap, "op_left_0002", true))
        .await
        .unwrap();
    assert!(
        core.backend()
            .calls()
            .iter()
            .any(|c| c.starts_with("discard_output "))
    );
}

// ------------------------------------------------------------------ Refresh link (B2.17)

/// A paused job with a durable checkpoint of `confirmed` bytes.
async fn paused_with_progress(core: &Core<FakeBackend>, key: &str, sha: bool) -> DownloadSnapshot {
    let mut params = add_params(
        &format!("ext_{key}"),
        &format!("https://h/{key}.bin?sig=old"),
    );
    if sha {
        params.expected_sha256 = Some("b".repeat(64));
    }
    let job = core.add(params).await.unwrap();
    let handle = handle_of(&job);
    core.backend()
        .set_progress(&handle, progress(TransferState::Active, 2048, Some(4096)));
    core.backend()
        .set_checkpoint(&handle, Ok(Some(checkpoint(2048))));
    core.poll_once().await;
    core.pause(&job.job_id).await.unwrap()
}

fn checkpoint(confirmed: u64) -> DurableCheckpoint {
    DurableCheckpoint {
        backend: "fake".into(),
        format_version: 1,
        confirmed: ByteCount(confirmed),
        payload: b"{\"etag\":\"\\\"v1\\\"\"}".to_vec(),
    }
}

fn compatible(same_resource: bool) -> ReplacementCheck {
    ReplacementCheck::Compatible {
        checkpoint: DurableCheckpoint {
            payload: b"rebound".to_vec(),
            ..checkpoint(2048)
        },
        same_resource,
    }
}

#[tokio::test]
async fn a_proven_same_resource_replacement_keeps_progress_under_a_new_attempt() {
    let dir = tempfile::tempdir().unwrap();
    let core = supported(dir.path());
    let job = paused_with_progress(&core, "rs-keep1", false).await;
    *core.backend().replacement.lock().unwrap() = Some(compatible(true));
    let r = core
        .replace_source(replace(
            &job,
            "op_rs_keep01",
            "https://h/rs-keep1.bin?sig=new",
            ReplaceMode::KeepIfVerified,
        ))
        .await
        .unwrap();
    assert_eq!(r.outcome, ReplaceOutcome::ProgressKept);
    assert_eq!(r.job.downloaded_bytes, ByteCount(2048));
    assert_eq!(
        r.job.desired_state,
        DesiredState::Paused,
        "still the user's pause"
    );
    assert_ne!(r.job.attempt_id, job.attempt_id);
    // The new attempt was restored from the rebound checkpoint with the new link.
    let restored = core.backend().restored.lock().unwrap().clone();
    assert_eq!(restored.last().unwrap().1.payload, b"rebound".to_vec());
    assert!(restored.last().unwrap().0.url.ends_with("sig=new"));
    // Snapshots and the result never carry the signature.
    assert!(!serde_json::to_string(&r).unwrap().contains("sig=new"));
    // A repeat answers with the recorded outcome, nothing new restored.
    let again = core
        .replace_source(replace(
            &job,
            "op_rs_keep01",
            "https://h/rs-keep1.bin?sig=new",
            ReplaceMode::KeepIfVerified,
        ))
        .await
        .unwrap();
    assert_eq!(again.outcome, ReplaceOutcome::ProgressKept);
    assert_eq!(
        core.backend().restored.lock().unwrap().len(),
        restored.len()
    );
}

#[tokio::test]
async fn another_resource_keeps_progress_only_with_a_trusted_checksum() {
    let dir = tempfile::tempdir().unwrap();
    let core = supported(dir.path());
    let plain = paused_with_progress(&core, "rs-oth01", false).await;
    *core.backend().replacement.lock().unwrap() = Some(compatible(false));
    let r = core
        .replace_source(replace(
            &plain,
            "op_rs_oth001",
            "https://mirror/x.bin",
            ReplaceMode::KeepIfVerified,
        ))
        .await
        .unwrap();
    assert_eq!(r.outcome, ReplaceOutcome::ChoiceRequired);
    assert_eq!(r.reason, Some(ReplaceReason::DifferentResource));
    assert_eq!(r.job.attempt_id, plain.attempt_id, "nothing changed");
    let trusted = paused_with_progress(&core, "rs-oth02", true).await;
    let r = core
        .replace_source(replace(
            &trusted,
            "op_rs_oth002",
            "https://mirror/y.bin",
            ReplaceMode::KeepIfVerified,
        ))
        .await
        .unwrap();
    assert_eq!(r.outcome, ReplaceOutcome::ProgressKept);
}

#[tokio::test]
async fn an_unproven_replacement_changes_nothing_and_restart_is_explicit() {
    let dir = tempfile::tempdir().unwrap();
    let core = supported(dir.path());
    let job = paused_with_progress(&core, "rs-mis01", false).await;
    for reason in [
        ReplaceReason::ContentDiffers,
        ReplaceReason::ValidatorChanged,
        ReplaceReason::ValidatorMissing,
        ReplaceReason::RangeIgnored,
        ReplaceReason::LengthChanged,
    ] {
        *core.backend().replacement.lock().unwrap() = Some(ReplacementCheck::Incompatible(reason));
        let r = core
            .replace_source(replace(
                &job,
                "op_rs_mis001",
                "https://h/other.bin",
                ReplaceMode::KeepIfVerified,
            ))
            .await
            .unwrap();
        assert_eq!(
            (r.outcome, r.reason),
            (ReplaceOutcome::ChoiceRequired, Some(reason))
        );
        assert_eq!(r.job.downloaded_bytes, ByteCount(2048));
        assert_eq!(r.job.attempt_id, job.attempt_id);
    }
    assert!(
        !core
            .backend()
            .calls()
            .iter()
            .any(|c| c.starts_with("discard"))
    );
    // Restart from zero with the new link: explicit, discards only this job's partial data.
    let r = core
        .replace_source(replace(
            &job,
            "op_rs_mis002",
            "https://h/other.bin",
            ReplaceMode::Restart,
        ))
        .await
        .unwrap();
    assert_eq!(r.outcome, ReplaceOutcome::Restarted);
    assert_eq!(r.job.downloaded_bytes, ByteCount(0));
    assert_eq!(
        r.job.desired_state,
        DesiredState::Paused,
        "a paused job stays paused"
    );
    assert!(r.job.attempt_id.is_none());
    assert_eq!(r.job.source_display, "https://h/other.bin");
    let old = job.attempt_id.clone().unwrap();
    assert!(core.backend().calls().contains(&format!("discard {old}")));
    // Resuming starts afresh from the new link.
    let resumed = core.resume(&job.job_id).await.unwrap();
    assert!(resumed.attempt_id.is_some());
    let started = core.backend().started.lock().unwrap().clone();
    assert_eq!(started.last().unwrap().url, "https://h/other.bin");
}

#[tokio::test]
async fn refresh_link_needs_a_stopped_job_and_a_failed_job_cannot_keep_progress() {
    let dir = tempfile::tempdir().unwrap();
    let core = supported(dir.path());
    let running = core
        .add(add_params("ext_rs-run01", "http://h/run.bin"))
        .await
        .unwrap();
    let e = core
        .replace_source(replace(
            &running,
            "op_rs_run001",
            "http://h/x.bin",
            ReplaceMode::Restart,
        ))
        .await
        .unwrap_err();
    assert!(e.0.detail.unwrap().contains("pause"));
    let expired = failed(&core, "rs-exp01", ErrorKind::LinkRefreshRequired, 300).await;
    let r = core
        .replace_source(replace(
            &expired,
            "op_rs_exp001",
            "http://h/new.bin",
            ReplaceMode::KeepIfVerified,
        ))
        .await
        .unwrap();
    assert_eq!(r.reason, Some(ReplaceReason::NoCheckpoint));
    let r = core
        .replace_source(replace(
            &expired,
            "op_rs_exp002",
            "http://h/new.bin",
            ReplaceMode::Restart,
        ))
        .await
        .unwrap();
    assert_eq!(r.outcome, ReplaceOutcome::Restarted);
    assert_eq!(
        r.job.desired_state,
        DesiredState::Running,
        "a failed job runs again"
    );
    assert!(r.job.attempt_id.is_some());
    // Non-http links are refused without echoing them.
    let e = core
        .replace_source(replace(
            &expired,
            "op_rs_exp003",
            "ftp://secret@h/x",
            ReplaceMode::Restart,
        ))
        .await
        .unwrap_err();
    assert!(!e.0.detail.unwrap().contains("secret"));
}

#[tokio::test]
async fn a_failed_restore_of_the_replacement_keeps_the_old_source() {
    let dir = tempfile::tempdir().unwrap();
    let core = supported(dir.path());
    let job = paused_with_progress(&core, "rs-rst01", false).await;
    *core.backend().replacement.lock().unwrap() = Some(compatible(true));
    *core.backend().restore_error.lock().unwrap() =
        Some(BackendError::new(ErrorKind::InvalidRange, "changed"));
    let r = core
        .replace_source(replace(
            &job,
            "op_rs_rst001",
            "https://h/rs-rst01.bin?sig=n",
            ReplaceMode::KeepIfVerified,
        ))
        .await
        .unwrap();
    assert_eq!(r.outcome, ReplaceOutcome::ChoiceRequired);
    // The reattach also failed (scripted), so the job ends explicitly instead of silently.
    assert_eq!(r.job.phase, Phase::Failed);
    assert!(r.job.source_display.contains("rs-rst01.bin"));
}

#[tokio::test]
async fn locate_answers_the_bound_folder_and_the_file_only_once_completed() {
    let dir = tempfile::tempdir().unwrap();
    let core = supported(dir.path());
    let job = core
        .add(add_params("ext_loc00001", "http://h/loc.bin"))
        .await
        .unwrap();
    let at = core.locate(&job.job_id).await.unwrap();
    assert_eq!(std::path::PathBuf::from(&at.directory), dir.path());
    assert!(at.directory_present);
    assert_eq!(at.file_name, "");
    std::fs::write(staged(dir.path(), &job), b"ok").unwrap();
    core.backend().set_progress(
        &handle_of(&job),
        progress(TransferState::Complete, 2, Some(2)),
    );
    core.poll_once().await;
    let at = core.locate(&job.job_id).await.unwrap();
    assert_eq!(at.file_name, "loc.bin");
    assert!(at.file_present);
    std::fs::remove_file(dir.path().join("loc.bin")).unwrap();
    assert!(!core.locate(&job.job_id).await.unwrap().file_present);
}
