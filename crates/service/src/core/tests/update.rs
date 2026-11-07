//! ADV09 / ADR 0030: update preparation (lease, quiescence, backup), cancellation, expiry,
//! startup acceptance and recovery with restore, against the fake backend and a real store.

use super::*;
use crate::core::update::{
    LEASE, RecoveryRecord, UpdateRecord, accept_pending, backup_file, pending_file, read_json,
    record_file, record_recovery, recovery_file, restore, usable_backup,
};
use dm_domain::api::{CancelUpdateParams, PrepareUpdateParams};

fn prep(op: &str) -> PrepareUpdateParams {
    PrepareUpdateParams {
        operation_id: op.into(),
        target_version: "9.9.9".into(),
    }
}

fn updating_core(db: &Path, dir: &Path, update: &Path) -> Core<FakeBackend> {
    core_with(FakeBackend::with_all(CapabilitySupport::Supported), dir)
        .with_update_dir(update.to_owned())
        .with_store(Store::open(db).unwrap())
        .unwrap()
}

#[tokio::test(start_paused = true)]
async fn preparing_holds_running_work_without_changing_any_intent() {
    let dir = tempfile::tempdir().unwrap();
    let update = dir.path().join("update");
    let db = dir.path().join("downova.sqlite3");
    let core = updating_core(&db, dir.path(), &update);
    let running = core
        .add(add_params("upd_run_0001", "http://h/run.bin"))
        .await
        .unwrap();
    let paused = core
        .add(add_params("upd_pau_0001", "http://h/pause.bin"))
        .await
        .unwrap();
    core.pause(&paused.job_id).await.unwrap();
    core.backend()
        .set_checkpoint(&handle_of(&running), Ok(Some(durable(4096))));

    let result = core.prepare_update(prep("upd_op_000001")).await.unwrap();
    assert_eq!(result.held_jobs, 1, "only the running transfer is held");
    assert_eq!(result.target_version, "9.9.9");
    assert_eq!(result.backup.jobs.0, 2);
    // Desired states are unchanged: running stays running (held), paused stays paused.
    assert_eq!(
        core.get(&running.job_id).await.unwrap().desired_state,
        DesiredState::Running
    );
    assert_eq!(
        core.get(&paused.job_id).await.unwrap().desired_state,
        DesiredState::Paused
    );
    assert!(
        core.backend()
            .calls()
            .iter()
            .any(|c| c.starts_with("pause"))
    );
    // Nothing new starts: add, resume are refused; reads and pauses work.
    let refused = core
        .add(add_params("upd_new_0001", "http://h/new.bin"))
        .await
        .unwrap_err();
    assert_eq!(refused.0.kind, ErrorKind::ServiceUnavailable);
    assert!(core.resume(&paused.job_id).await.is_err());
    assert_eq!(core.list().await.len(), 2);
    // The backup, its record and the pending marker exist and agree.
    let record: UpdateRecord = read_json(&record_file(&update, "upd_op_000001")).unwrap();
    assert!(record.complete);
    assert_eq!(record.sha256, result.backup.sha256);
    assert_eq!(
        read_json::<UpdateRecord>(&pending_file(&update)).unwrap(),
        record
    );
    let check = dm_storage::backup::check_backup(&backup_file(&update, "upd_op_000001")).unwrap();
    assert_eq!(check.sha256, record.sha256);
    // A repeat of the same preparation answers with the same result; another one is refused.
    assert_eq!(
        core.prepare_update(prep("upd_op_000001")).await.unwrap(),
        result
    );
    assert!(core.prepare_update(prep("upd_op_000002")).await.is_err());

    // Cancel: the lease ends, the marker goes, the held transfer continues; the pause stays.
    core.cancel_update(CancelUpdateParams {
        operation_id: "upd_op_000001".into(),
    })
    .await
    .unwrap();
    assert!(!pending_file(&update).exists());
    let resumes = core
        .backend()
        .calls()
        .iter()
        .filter(|c| c.starts_with("resume"))
        .count();
    assert!(resumes >= 1, "{:?}", core.backend().calls());
    assert_eq!(
        core.get(&paused.job_id).await.unwrap().desired_state,
        DesiredState::Paused
    );
    core.add(add_params("upd_new_0002", "http://h/after.bin"))
        .await
        .unwrap();
}

#[tokio::test(start_paused = true)]
async fn a_stopped_queue_stays_stopped_and_an_expired_lease_gives_the_version_back() {
    let dir = tempfile::tempdir().unwrap();
    let update = dir.path().join("update");
    let core = updating_core(&dir.path().join("downova.sqlite3"), dir.path(), &update);
    let job = core
        .add(add_params("upd_que_0001", "http://h/q.bin"))
        .await
        .unwrap();
    core.set_queue_running(dm_storage::DEFAULT_QUEUE, false)
        .await
        .unwrap();
    core.prepare_update(prep("upd_op_000003")).await.unwrap();
    // The lease runs out without a shutdown: the old version continues as it was.
    tokio::time::advance(LEASE + std::time::Duration::from_secs(1)).await;
    core.poll_once().await;
    assert!(!pending_file(&update).exists());
    let queues = core.queues().await;
    assert!(
        !queues[0].running,
        "the queue the user stopped is still stopped"
    );
    let held = core.get(&job.job_id).await.unwrap();
    assert!(held.blockers.contains(&Blocker::QueueStopped));
    assert_eq!(held.desired_state, DesiredState::Running);
    core.add(add_params("upd_que_0002", "http://h/q2.bin"))
        .await
        .unwrap();
}

#[tokio::test(start_paused = true)]
async fn a_checkpoint_failure_cancels_the_preparation_and_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let update = dir.path().join("update");
    let core = updating_core(&dir.path().join("downova.sqlite3"), dir.path(), &update);
    let job = core
        .add(add_params("upd_cpf_0001", "http://h/c.bin"))
        .await
        .unwrap();
    core.backend().set_checkpoint(
        &handle_of(&job),
        Err(BackendError::new(ErrorKind::Internal, "cannot sync")),
    );
    let err = core
        .prepare_update(prep("upd_op_000004"))
        .await
        .unwrap_err();
    assert_eq!(err.0.kind, ErrorKind::Internal);
    assert!(!pending_file(&update).exists());
    assert!(!backup_file(&update, "upd_op_000004").exists());
    // Released: the transfer was resumed and new work is admitted again.
    assert!(
        core.backend()
            .calls()
            .iter()
            .any(|c| c.starts_with("resume"))
    );
    core.add(add_params("upd_cpf_0002", "http://h/c2.bin"))
        .await
        .unwrap();
}

#[tokio::test]
async fn preparation_needs_a_valid_operation_and_version() {
    let dir = tempfile::tempdir().unwrap();
    let update = dir.path().join("update");
    let core = updating_core(&dir.path().join("downova.sqlite3"), dir.path(), &update);
    for (op, v) in [
        ("short", "1.0.0"),
        ("upd_op_000005", "1.0"),
        ("upd_op_000005", "../x"),
    ] {
        let e = core
            .prepare_update(PrepareUpdateParams {
                operation_id: op.into(),
                target_version: v.into(),
            })
            .await
            .unwrap_err();
        assert_eq!(e.0.kind, ErrorKind::InvalidRequest, "{op} {v}");
    }
    // Without a data folder the service refuses rather than writing elsewhere.
    let no_dir = core_with(
        FakeBackend::with_all(CapabilitySupport::Supported),
        dir.path(),
    );
    assert!(no_dir.prepare_update(prep("upd_op_000006")).await.is_err());
}

#[tokio::test]
async fn startup_accepts_the_target_version_and_closes_an_abandoned_update() {
    let dir = tempfile::tempdir().unwrap();
    let update = dir.path().join("update");
    let core = updating_core(&dir.path().join("downova.sqlite3"), dir.path(), &update);
    core.prepare_update(prep("upd_op_000007")).await.unwrap();
    drop(core);
    let installed = accept_pending(&update, "9.9.9", 1).unwrap();
    assert_eq!(installed.outcome, "installed");
    assert!(!pending_file(&update).exists());
    assert_eq!(
        accept_pending(&update, "9.9.9", 2),
        None,
        "nothing pending any more"
    );

    let core = updating_core(&dir.path().join("downova.sqlite3"), dir.path(), &update);
    core.prepare_update(prep("upd_op_000008")).await.unwrap();
    drop(core);
    let old = accept_pending(&update, "0.0.1", 3).unwrap();
    assert_eq!(old.outcome, "not_installed", "the installer did not run");
}

#[tokio::test]
async fn a_failed_start_is_recorded_and_only_a_matching_checked_backup_is_restored() {
    let dir = tempfile::tempdir().unwrap();
    let update = dir.path().join("update");
    let db = dir.path().join("downova.sqlite3");
    let downloads = dir.path().join("downloads");
    std::fs::create_dir_all(&downloads).unwrap();
    let job;
    {
        let core = updating_core(&db, &downloads, &update);
        job = core
            .add(add_params("upd_rst_0001", "http://h/keep.bin?sig=SECRET"))
            .await
            .unwrap();
        core.pause(&job.job_id).await.unwrap();
        core.prepare_update(prep("upd_op_000009")).await.unwrap();
    }
    std::fs::write(downloads.join("user-file.bin"), b"user data").unwrap();
    // The database becomes unusable (as after a broken migration in a newer build).
    std::fs::write(&db, b"this is not a database").unwrap();
    let e = Store::open(&db).unwrap_err();
    let record: RecoveryRecord = record_recovery(&update, &e, dm_storage::SCHEMA_VERSION, 10);
    assert_eq!(record.operation_id.as_deref(), Some("upd_op_000009"));
    assert_eq!(record.backup_operation.as_deref(), Some("upd_op_000009"));
    assert!(recovery_file(&update).exists());
    let text = std::fs::read_to_string(recovery_file(&update)).unwrap();
    assert!(!text.contains("SECRET") && !text.contains(&*db.to_string_lossy()));

    // A tampered backup is refused and nothing is replaced.
    let file = backup_file(&update, "upd_op_000009");
    let good = std::fs::read(&file).unwrap();
    let mut bad = good.clone();
    let last = bad.len() - 1;
    bad[last] ^= 0x55;
    std::fs::write(&file, &bad).unwrap();
    assert!(usable_backup(&update, dm_storage::SCHEMA_VERSION).is_none());
    assert!(
        restore(
            &update,
            &db,
            "upd_op_000009",
            dm_storage::SCHEMA_VERSION,
            11
        )
        .is_err()
    );
    assert_eq!(std::fs::read(&db).unwrap(), b"this is not a database");
    // An unknown operation, or one with a newer schema, is refused.
    assert!(
        restore(
            &update,
            &db,
            "upd_op_unknown",
            dm_storage::SCHEMA_VERSION,
            12
        )
        .is_err()
    );
    std::fs::write(&file, &good).unwrap();
    assert!(
        restore(&update, &db, "upd_op_000009", 0, 13).is_err(),
        "newer schema than supported"
    );

    // The matching backup restores; the broken file is set aside, not deleted.
    let outcome = restore(
        &update,
        &db,
        "upd_op_000009",
        dm_storage::SCHEMA_VERSION,
        14,
    )
    .unwrap();
    assert_eq!(outcome.outcome, "restored");
    assert!(!recovery_file(&update).exists());
    let aside = update
        .join("set-aside")
        .join("upd_op_000009-14")
        .join("downova.sqlite3");
    assert_eq!(std::fs::read(aside).unwrap(), b"this is not a database");
    // The restored database opens; the job, its pause and its sealed link are back; user files
    // were never touched.
    let core = updating_core(&db, &downloads, &update);
    let back = core.get(&job.job_id).await.unwrap();
    assert_eq!(back.desired_state, DesiredState::Paused);
    assert_eq!(
        std::fs::read(downloads.join("user-file.bin")).unwrap(),
        b"user data"
    );
}
