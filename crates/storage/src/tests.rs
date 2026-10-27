#![allow(clippy::unwrap_used, clippy::arithmetic_side_effects)]

use super::*;
use dm_domain::api::{Verification, VerificationSource};
use dm_domain::{ErrorKind, NextAction};

fn job(id: &str, phase: Phase) -> JobRow {
    JobRow {
        id: JobId::parse(id).unwrap(),
        url: "https://example.org/f.zip?sig=SECRET".into(),
        display_name: "f.zip".into(),
        staging_name: "f.zip".into(),
        name_source: None,
        destination: PathBuf::from("downloads"),
        desired_state: DesiredState::Running,
        phase,
        downloaded: ByteCount(0),
        total: Some(ByteCount(u64::from(u32::MAX) * 4)),
        expected_sha256: None,
        error: None,
        attempt_id: None,
        verification: None,
        created_at_ms: 1_000,
        updated_at_ms: 1_000,
    }
}

fn key(k: &str, job_id: &str, at: u64) -> IdempotencyRow {
    IdempotencyRow {
        key: k.into(),
        fingerprint_sha256: "ab".repeat(32),
        job_id: JobId::parse(job_id).unwrap(),
        created_at_ms: at,
    }
}

#[test]
fn a_new_database_is_migrated_and_reopening_is_a_no_op() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(FILE_NAME);
    {
        let store = Store::open(&path).unwrap();
        assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);
    }
    let store = Store::open(&path).unwrap();
    assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);
    assert!(store.load_jobs().unwrap().is_empty());
}

#[test]
fn jobs_and_keys_round_trip_through_a_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(FILE_NAME);
    let mut a = job("job_0000000000000001", Phase::Queued);
    {
        let mut store = Store::open(&path).unwrap();
        store
            .insert_job(&a, Some(&key("ext_key_0001", "job_0000000000000001", 5)))
            .unwrap();
        a.phase = Phase::Failed;
        a.downloaded = ByteCount(1 << 40);
        a.error = Some(ErrorData {
            kind: ErrorKind::ServerBusy,
            retryable: true,
            next_action: NextAction::Wait,
            detail: Some("busy".into()),
        });
        a.updated_at_ms = 2_000;
        store.update_job(&a).unwrap();
    }
    let store = Store::open(&path).unwrap();
    assert_eq!(store.load_jobs().unwrap(), vec![a]);
    assert_eq!(
        store.find_idempotency("ext_key_0001").unwrap(),
        Some(key("ext_key_0001", "job_0000000000000001", 5))
    );
    assert_eq!(store.find_idempotency("ext_key_none").unwrap(), None);
}

#[test]
fn a_job_and_its_key_are_written_together_or_not_at_all() {
    let mut store = Store::open_in_memory().unwrap();
    store
        .insert_job(
            &job("job_0000000000000001", Phase::Queued),
            Some(&key("ext_key_0001", "job_0000000000000001", 1)),
        )
        .unwrap();
    // Same key again for a different job: the key insert fails, so the job is not kept either.
    let err = store.insert_job(
        &job("job_0000000000000002", Phase::Queued),
        Some(&key("ext_key_0001", "job_0000000000000002", 2)),
    );
    assert!(matches!(err, Err(StorageError::Sql(_))));
    assert_eq!(store.load_jobs().unwrap().len(), 1);
}

#[test]
fn updating_an_unknown_job_is_an_error() {
    let mut store = Store::open_in_memory().unwrap();
    assert!(
        store
            .update_job(&job("job_0000000000000009", Phase::Queued))
            .is_err()
    );
}

#[test]
fn old_idempotency_records_are_pruned_but_jobs_kept() {
    let mut store = Store::open_in_memory().unwrap();
    store
        .insert_job(
            &job("job_0000000000000001", Phase::Completed),
            Some(&key("ext_key_old1", "job_0000000000000001", 100)),
        )
        .unwrap();
    store
        .insert_job(
            &job("job_0000000000000002", Phase::Completed),
            Some(&key("ext_key_new1", "job_0000000000000002", 900)),
        )
        .unwrap();
    assert_eq!(store.prune_idempotency(500).unwrap(), 1);
    let keys: Vec<_> = store
        .load_idempotency()
        .unwrap()
        .into_iter()
        .map(|r| r.key)
        .collect();
    assert_eq!(keys, vec!["ext_key_new1".to_owned()]);
    assert_eq!(store.load_jobs().unwrap().len(), 2);
}

#[test]
fn a_second_opener_is_refused_while_the_service_holds_the_database() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(FILE_NAME);
    let _held = Store::open(&path).unwrap();
    let second = Store::open(&path);
    assert!(matches!(second, Err(StorageError::Open(_))), "{second:?}");
}

#[test]
fn a_database_from_a_newer_version_is_refused_and_left_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(FILE_NAME);
    {
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch("CREATE TABLE future (x INTEGER); PRAGMA user_version = 99;")
            .unwrap();
    }
    let before = std::fs::read(&path).unwrap();
    assert_eq!(
        Store::open(&path).unwrap_err(),
        StorageError::NewerSchema {
            found: 99,
            supported: SCHEMA_VERSION
        }
    );
    let conn = Connection::open(&path).unwrap();
    let v: u32 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(v, 99);
    assert_eq!(std::fs::read(&path).unwrap().len(), before.len());
}

#[test]
fn a_failing_migration_rolls_back_completely() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(FILE_NAME);
    {
        // A table named like ours makes migration 1 fail half-way.
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch("CREATE TABLE idempotency_records (x INTEGER);")
            .unwrap();
    }
    let err = Store::open(&path).unwrap_err();
    assert!(
        matches!(err, StorageError::Migration { version: 1, .. }),
        "{err}"
    );
    let conn = Connection::open(&path).unwrap();
    let v: u32 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(v, 0);
    let jobs_table: Option<String> = conn
        .query_row(
            "SELECT name FROM sqlite_master WHERE name = 'jobs'",
            [],
            |r| r.get(0),
        )
        .optional()
        .unwrap();
    assert_eq!(jobs_table, None, "the partial migration was rolled back");
}

#[test]
fn debug_output_never_shows_the_raw_url() {
    let shown = format!("{:?}", job("job_0000000000000001", Phase::Queued));
    assert!(!shown.contains("SECRET"), "{shown}");
}

// ------------------------------------------------------------------ B1.2 resume checkpoints

fn attempt(id: &str) -> AttemptId {
    AttemptId::parse(id).unwrap()
}

fn running(id: &str, att: &str) -> JobRow {
    JobRow {
        phase: Phase::Transferring,
        attempt_id: Some(attempt(att)),
        ..job(id, Phase::Transferring)
    }
}

fn cp(id: &str, att: &str, confirmed: u64) -> CheckpointRow {
    CheckpointRow {
        job_id: JobId::parse(id).unwrap(),
        attempt_id: attempt(att),
        backend_id: "http".into(),
        backend_version: Some("0.1.0".into()),
        format_version: 1,
        confirmed: ByteCount(confirmed),
        payload: b"{\"opaque\":true,\"url\":\"https://h/f?sig=SECRET\"}".to_vec(),
        updated_at_ms: 2_000,
    }
}

#[test]
fn a_version_1_database_is_upgraded_and_keeps_its_jobs_and_keys() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(FILE_NAME);
    {
        // A database exactly as B1.1 wrote it (schema 1).
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(MIGRATIONS[0]).unwrap();
        conn.execute_batch(
            "INSERT INTO jobs VALUES ('job_0000000000000001', 'https://h/f', 'f', 'downloads',
                 'running', 'completed', 5, 5, NULL, NULL, 1, 2);
             INSERT INTO idempotency_records VALUES ('ext_key_0001', 'ab', 'job_0000000000000001', 3);
             PRAGMA user_version = 1;",
        )
        .unwrap();
    }
    let store = Store::open(&path).unwrap();
    assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);
    let jobs = store.load_jobs().unwrap();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].phase, Phase::Completed);
    assert_eq!(jobs[0].attempt_id, None);
    assert!(store.find_idempotency("ext_key_0001").unwrap().is_some());
    assert_eq!(
        store
            .load_checkpoint(&JobId::parse("job_0000000000000001").unwrap())
            .unwrap(),
        None
    );
}

#[test]
fn a_checkpoint_is_committed_with_its_job_and_round_trips() {
    let mut store = Store::open_in_memory().unwrap();
    let mut j = running("job_0000000000000001", "att_0000000000000001");
    store.insert_job(&j, None).unwrap();
    j.downloaded = ByteCount(1 << 20);
    store
        .save_checkpoint(
            &j,
            &cp("job_0000000000000001", "att_0000000000000001", 1 << 20),
        )
        .unwrap();
    assert_eq!(store.load_jobs().unwrap()[0].downloaded, ByteCount(1 << 20));
    let loaded = store.load_checkpoint(&j.id).unwrap().unwrap();
    assert_eq!(
        loaded,
        cp("job_0000000000000001", "att_0000000000000001", 1 << 20)
    );
    assert!(
        !format!("{loaded:?}").contains("SECRET"),
        "payload never in Debug"
    );
    // A newer checkpoint replaces the older one.
    store
        .save_checkpoint(
            &j,
            &cp("job_0000000000000001", "att_0000000000000001", 2 << 20),
        )
        .unwrap();
    assert_eq!(
        store.load_checkpoint(&j.id).unwrap().unwrap().confirmed,
        ByteCount(2 << 20)
    );
}

#[test]
fn checkpoints_from_stale_attempts_or_for_finished_jobs_are_refused() {
    let mut store = Store::open_in_memory().unwrap();
    let j = running("job_0000000000000001", "att_0000000000000002");
    store.insert_job(&j, None).unwrap();
    // An older attempt of the same job.
    assert_eq!(
        store.save_checkpoint(
            &JobRow {
                attempt_id: Some(attempt("att_0000000000000001")),
                ..j.clone()
            },
            &cp("job_0000000000000001", "att_0000000000000001", 10)
        ),
        Err(StorageError::StaleAttempt)
    );
    // The row offered does not name the checkpoint's attempt.
    assert_eq!(
        store.save_checkpoint(&j, &cp("job_0000000000000001", "att_0000000000000001", 10)),
        Err(StorageError::StaleAttempt)
    );
    // Unknown job.
    assert_eq!(
        store.save_checkpoint(
            &running("job_0000000000000009", "att_0000000000000002"),
            &cp("job_0000000000000009", "att_0000000000000002", 10)
        ),
        Err(StorageError::StaleAttempt)
    );
    // Finished job.
    let done = JobRow {
        phase: Phase::Completed,
        ..j.clone()
    };
    store.update_job(&done).unwrap();
    assert_eq!(
        store.save_checkpoint(&j, &cp("job_0000000000000001", "att_0000000000000002", 10)),
        Err(StorageError::StaleAttempt)
    );
    assert_eq!(store.load_checkpoint(&j.id).unwrap(), None);
    // Nothing of the refused writes reached the job row.
    assert_eq!(store.load_jobs().unwrap()[0].phase, Phase::Completed);
}

#[test]
fn finishing_a_job_or_moving_it_to_another_attempt_drops_its_checkpoint() {
    let mut store = Store::open_in_memory().unwrap();
    let j = running("job_0000000000000001", "att_0000000000000001");
    store.insert_job(&j, None).unwrap();
    store
        .save_checkpoint(&j, &cp("job_0000000000000001", "att_0000000000000001", 10))
        .unwrap();
    // Same attempt, still running: kept.
    store.update_job(&j).unwrap();
    assert!(store.load_checkpoint(&j.id).unwrap().is_some());
    // A new attempt (e.g. a retry from zero): the old checkpoint no longer applies.
    store
        .update_job(&JobRow {
            attempt_id: Some(attempt("att_0000000000000002")),
            ..j.clone()
        })
        .unwrap();
    assert!(store.load_checkpoint(&j.id).unwrap().is_none());
    let j2 = JobRow {
        attempt_id: Some(attempt("att_0000000000000002")),
        ..j.clone()
    };
    store
        .save_checkpoint(&j2, &cp("job_0000000000000001", "att_0000000000000002", 10))
        .unwrap();
    store
        .update_job(&JobRow {
            phase: Phase::Cancelled,
            ..j2
        })
        .unwrap();
    assert!(store.load_checkpoint(&j.id).unwrap().is_none());
}

#[test]
fn oversized_checkpoint_payloads_are_refused() {
    let mut store = Store::open_in_memory().unwrap();
    let j = running("job_0000000000000001", "att_0000000000000001");
    store.insert_job(&j, None).unwrap();
    let mut big = cp("job_0000000000000001", "att_0000000000000001", 10);
    big.payload = vec![0; MAX_CHECKPOINT_PAYLOAD + 1];
    assert!(store.save_checkpoint(&j, &big).is_err());
    assert!(store.load_checkpoint(&j.id).unwrap().is_none());
}

#[test]
fn a_restored_job_moves_its_checkpoint_to_the_new_attempt_atomically() {
    let mut store = Store::open_in_memory().unwrap();
    let old = running("job_0000000000000001", "att_0000000000000001");
    store.insert_job(&old, None).unwrap();
    store
        .save_checkpoint(
            &old,
            &cp("job_0000000000000001", "att_0000000000000001", 10),
        )
        .unwrap();
    let new = JobRow {
        attempt_id: Some(attempt("att_0000000000000002")),
        ..old.clone()
    };
    let moved = cp("job_0000000000000001", "att_0000000000000002", 10);
    // Only from the attempt that is actually stored.
    assert_eq!(
        store.rebind_checkpoint(&new, &moved, &attempt("att_0000000000000009")),
        Err(StorageError::StaleAttempt)
    );
    store
        .rebind_checkpoint(&new, &moved, &attempt("att_0000000000000001"))
        .unwrap();
    assert_eq!(store.load_checkpoint(&old.id).unwrap(), Some(moved));
    assert_eq!(
        store.load_jobs().unwrap()[0].attempt_id,
        Some(attempt("att_0000000000000002"))
    );
    // The old attempt can no longer write.
    assert_eq!(
        store.save_checkpoint(
            &old,
            &cp("job_0000000000000001", "att_0000000000000001", 20)
        ),
        Err(StorageError::StaleAttempt)
    );
}

// ------------------------------------------------------------------ B1.3 completion journal

fn entry(job: &str, att: &str, stage: JournalStage) -> JournalRow {
    JournalRow {
        job_id: JobId::parse(job).unwrap(),
        attempt_id: attempt(att),
        stage,
        placement: Placement::Staged,
        staging_path: PathBuf::from("downloads/f.zip.downova-part"),
        final_path: PathBuf::from("downloads/f.zip"),
        length: (stage != JournalStage::Intent).then_some(ByteCount(1 << 33)),
        sha256: (stage != JournalStage::Intent).then(|| "ab".repeat(32)),
        created_at_ms: 10,
        updated_at_ms: 20,
    }
}

#[test]
fn a_version_2_database_is_upgraded_to_the_journal_schema() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(FILE_NAME);
    {
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(MIGRATIONS[0]).unwrap();
        conn.execute_batch(MIGRATIONS[1]).unwrap();
        conn.execute_batch(
            "INSERT INTO jobs (job_id, url, display_name, destination, desired_state, phase,
                               downloaded_bytes, total_bytes, expected_sha256, error_json,
                               created_at_ms, updated_at_ms, attempt_id)
             VALUES ('job_0000000000000001', 'https://h/f', 'f', 'downloads', 'running',
                     'transferring', 5, 9, NULL, NULL, 1, 2, 'att_0000000000000001');
             INSERT INTO resume_checkpoints VALUES ('job_0000000000000001', 'att_0000000000000001',
                     'http', '0.1.0', 1, 5, x'7b7d', 3);
             PRAGMA user_version = 2;",
        )
        .unwrap();
    }
    let store = Store::open(&path).unwrap();
    assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);
    let jobs = store.load_jobs().unwrap();
    assert_eq!(jobs[0].verification, None);
    assert!(store.load_checkpoint(&jobs[0].id).unwrap().is_some());
    assert!(store.load_journal().unwrap().is_empty());
}

#[test]
fn the_journal_walks_its_stages_with_the_job_and_ends_completed() {
    let mut store = Store::open_in_memory().unwrap();
    let j = running("job_0000000000000001", "att_0000000000000001");
    store.insert_job(&j, None).unwrap();
    store
        .save_checkpoint(&j, &cp("job_0000000000000001", "att_0000000000000001", 10))
        .unwrap();
    let verifying = JobRow {
        phase: Phase::Verifying,
        ..j.clone()
    };
    store
        .begin_finalization(
            &verifying,
            &entry(
                "job_0000000000000001",
                "att_0000000000000001",
                JournalStage::Intent,
            ),
        )
        .unwrap();
    // A second intent for the same job is refused.
    assert_eq!(
        store.begin_finalization(
            &verifying,
            &entry(
                "job_0000000000000001",
                "att_0000000000000001",
                JournalStage::Intent
            )
        ),
        Err(StorageError::StaleAttempt)
    );
    for stage in [JournalStage::Verified, JournalStage::Placed] {
        store
            .advance_finalization(
                &verifying,
                &entry("job_0000000000000001", "att_0000000000000001", stage),
            )
            .unwrap();
        assert_eq!(store.load_journal().unwrap()[0].stage, stage);
    }
    let done = JobRow {
        phase: Phase::Completed,
        verification: Some(Verification {
            sha256: "ab".repeat(32),
            source: VerificationSource::PublisherChecksum,
        }),
        ..verifying.clone()
    };
    store
        .advance_finalization(
            &done,
            &entry(
                "job_0000000000000001",
                "att_0000000000000001",
                JournalStage::Complete,
            ),
        )
        .unwrap();
    // Completed in the same transaction: checkpoint gone, evidence kept, entry left for cleanup.
    let loaded = store.load_jobs().unwrap();
    assert_eq!(loaded[0].phase, Phase::Completed);
    assert_eq!(loaded[0].verification, done.verification);
    assert!(store.load_checkpoint(&j.id).unwrap().is_none());
    let pending = store.load_journal().unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].stage, JournalStage::Complete);
    assert_eq!(pending[0].length, Some(ByteCount(1 << 33)));
    store.finish_finalization(&j.id).unwrap();
    assert!(store.load_journal().unwrap().is_empty());
}

#[test]
fn stale_attempts_and_finished_jobs_cannot_write_the_journal() {
    let mut store = Store::open_in_memory().unwrap();
    let j = running("job_0000000000000001", "att_0000000000000002");
    store.insert_job(&j, None).unwrap();
    let old = JobRow {
        attempt_id: Some(attempt("att_0000000000000001")),
        ..j.clone()
    };
    assert_eq!(
        store.begin_finalization(
            &old,
            &entry(
                "job_0000000000000001",
                "att_0000000000000001",
                JournalStage::Intent
            )
        ),
        Err(StorageError::StaleAttempt)
    );
    // Advancing without an entry, or with another attempt's entry, is refused.
    assert_eq!(
        store.advance_finalization(
            &j,
            &entry(
                "job_0000000000000001",
                "att_0000000000000002",
                JournalStage::Verified
            )
        ),
        Err(StorageError::StaleAttempt)
    );
    store
        .begin_finalization(
            &j,
            &entry(
                "job_0000000000000001",
                "att_0000000000000002",
                JournalStage::Intent,
            ),
        )
        .unwrap();
    assert_eq!(
        store.advance_finalization(
            &old,
            &entry(
                "job_0000000000000001",
                "att_0000000000000001",
                JournalStage::Verified
            )
        ),
        Err(StorageError::StaleAttempt)
    );
    // A cancelled job drops its entry, and a late stage cannot revive it.
    let cancelled = JobRow {
        phase: Phase::Cancelled,
        desired_state: DesiredState::Cancelled,
        ..j.clone()
    };
    store.update_job(&cancelled).unwrap();
    assert!(store.load_journal().unwrap().is_empty());
    let late = JobRow {
        phase: Phase::Completed,
        ..j.clone()
    };
    assert_eq!(
        store.advance_finalization(
            &late,
            &entry(
                "job_0000000000000001",
                "att_0000000000000002",
                JournalStage::Complete
            )
        ),
        Err(StorageError::StaleAttempt)
    );
    assert_eq!(store.load_jobs().unwrap()[0].phase, Phase::Cancelled);
}

#[test]
fn abandoning_finalization_fails_the_job_and_drops_entry_and_checkpoint() {
    let mut store = Store::open_in_memory().unwrap();
    let j = running("job_0000000000000001", "att_0000000000000001");
    store.insert_job(&j, None).unwrap();
    store
        .save_checkpoint(&j, &cp("job_0000000000000001", "att_0000000000000001", 10))
        .unwrap();
    store
        .begin_finalization(
            &j,
            &entry(
                "job_0000000000000001",
                "att_0000000000000001",
                JournalStage::Intent,
            ),
        )
        .unwrap();
    store
        .abandon_finalization(&JobRow {
            phase: Phase::Failed,
            ..j.clone()
        })
        .unwrap();
    assert!(store.load_journal().unwrap().is_empty());
    assert!(store.load_checkpoint(&j.id).unwrap().is_none());
    assert_eq!(store.load_jobs().unwrap()[0].phase, Phase::Failed);
}

// ------------------------------------------------------------------ B1.6 file names

#[test]
fn a_version_3_database_keeps_its_names_frozen_as_legacy() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(FILE_NAME);
    {
        let conn = Connection::open(&path).unwrap();
        for m in &MIGRATIONS[..3] {
            conn.execute_batch(m).unwrap();
        }
        conn.execute_batch(
            "INSERT INTO jobs (job_id, url, display_name, destination, desired_state, phase,
                               downloaded_bytes, total_bytes, expected_sha256, error_json,
                               created_at_ms, updated_at_ms, attempt_id)
             VALUES ('job_0000000000000001', 'https://h/f', 'বই (1).pdf', 'downloads', 'running',
                     'transferring', 5, 9, NULL, NULL, 1, 2, 'att_0000000000000001');
             INSERT INTO resume_checkpoints VALUES ('job_0000000000000001', 'att_0000000000000001',
                     'http', '0.1.0', 1, 5, x'7b7d', 3);
             PRAGMA user_version = 3;",
        )
        .unwrap();
    }
    let store = Store::open(&path).unwrap();
    assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);
    let jobs = store.load_jobs().unwrap();
    assert_eq!(jobs[0].display_name, "বই (1).pdf");
    assert_eq!(
        jobs[0].staging_name, "বই (1).pdf",
        "the partial file keeps its name"
    );
    assert_eq!(jobs[0].name_source.as_deref(), Some("legacy"));
    assert!(store.load_checkpoint(&jobs[0].id).unwrap().is_some());
}

#[test]
fn staging_name_and_name_source_round_trip_and_are_validated() {
    let mut store = Store::open_in_memory().unwrap();
    let mut j = job("job_0000000000000001", Phase::Queued);
    j.staging_name = "a.bin".into();
    j.display_name = "Report ২০২৬.pdf".into();
    store.insert_job(&j, None).unwrap();
    assert_eq!(store.load_jobs().unwrap()[0], j);
    j.name_source = Some("filename_ext".into());
    store.update_job(&j).unwrap();
    assert_eq!(store.load_jobs().unwrap()[0], j);
    j.name_source = Some("guessed".into());
    assert!(
        store.update_job(&j).is_err(),
        "unknown sources are refused by the schema"
    );
}

// ------------------------------------------------------------------ B2.3 remove from list (ADR 0013)

#[test]
fn only_finished_jobs_are_removed_and_the_mark_survives_a_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(FILE_NAME);
    let done = JobId::parse("job_0000000000000001").unwrap();
    {
        let mut store = Store::open(&path).unwrap();
        store
            .insert_job(
                &job("job_0000000000000001", Phase::Completed),
                Some(&key("ext_key_0001", "job_0000000000000001", 5)),
            )
            .unwrap();
        for (i, phase) in [Phase::Queued, Phase::Transferring, Phase::Verifying]
            .into_iter()
            .enumerate()
        {
            let id = format!("job_000000000000010{i}");
            store.insert_job(&job(&id, phase), None).unwrap();
            assert!(
                matches!(
                    store.remove_job(&JobId::parse(&id).unwrap(), 50),
                    Err(StorageError::NotRemovable)
                ),
                "{phase:?}"
            );
        }
        assert!(matches!(
            store.remove_job(&JobId::parse("job_0000000000000999").unwrap(), 50),
            Err(StorageError::NotRemovable)
        ));
        assert_eq!(store.remove_job(&done, 100).unwrap(), 100);
        // Repeating keeps the first time.
        assert_eq!(store.remove_job(&done, 200).unwrap(), 100);
        // An ordinary update of the row never undoes the removal.
        store
            .update_job(&job("job_0000000000000001", Phase::Completed))
            .unwrap();
    }
    let store = Store::open(&path).unwrap();
    assert_eq!(store.removed_jobs().unwrap(), vec![(done.clone(), 100)]);
    // The row and its idempotency record are kept (COR-09).
    assert!(store.load_jobs().unwrap().iter().any(|j| j.id == done));
    assert_eq!(
        store
            .find_idempotency("ext_key_0001")
            .unwrap()
            .unwrap()
            .job_id,
        done
    );
}

#[test]
fn a_job_with_a_finalization_entry_is_not_removable() {
    let mut store = Store::open_in_memory().unwrap();
    let j = running("job_0000000000000001", "att_0000000000000001");
    store.insert_job(&j, None).unwrap();
    store
        .begin_finalization(
            &JobRow {
                phase: Phase::Verifying,
                ..j.clone()
            },
            &entry(
                "job_0000000000000001",
                "att_0000000000000001",
                JournalStage::Intent,
            ),
        )
        .unwrap();
    assert!(matches!(
        store.remove_job(&j.id, 1),
        Err(StorageError::NotRemovable)
    ));
}

#[test]
fn tombstones_are_purged_only_after_their_keys_expired() {
    let mut store = Store::open_in_memory().unwrap();
    store
        .insert_job(
            &job("job_0000000000000001", Phase::Failed),
            Some(&key("ext_key_0001", "job_0000000000000001", 5)),
        )
        .unwrap();
    store
        .insert_job(&job("job_0000000000000002", Phase::Cancelled), None)
        .unwrap();
    let keyed = JobId::parse("job_0000000000000001").unwrap();
    let plain = JobId::parse("job_0000000000000002").unwrap();
    store.remove_job(&keyed, 10).unwrap();
    store.remove_job(&plain, 10).unwrap();
    // Too recent: nothing is purged.
    assert_eq!(store.purge_removed(5).unwrap(), 0);
    // Old enough, but the keyed job still has its record: only the other goes.
    assert_eq!(store.purge_removed(20).unwrap(), 1);
    assert_eq!(store.removed_jobs().unwrap(), vec![(keyed.clone(), 10)]);
    // Once its key is pruned, it goes too.
    store.prune_idempotency(6).unwrap();
    assert_eq!(store.purge_removed(20).unwrap(), 1);
    assert!(store.load_jobs().unwrap().is_empty());
}

#[test]
fn a_version_4_database_gains_the_removal_column_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(FILE_NAME);
    {
        let conn = Connection::open(&path).unwrap();
        for m in &MIGRATIONS[..4] {
            conn.execute_batch(m).unwrap();
        }
        conn.execute_batch(
            "INSERT INTO jobs (job_id, url, display_name, destination, desired_state, phase,
                               downloaded_bytes, total_bytes, expected_sha256, error_json,
                               created_at_ms, updated_at_ms, attempt_id, staging_name, name_source)
             VALUES ('job_0000000000000001', 'https://h/f', 'f.bin', 'downloads', 'running',
                     'completed', 9, 9, NULL, NULL, 1, 2, NULL, 'f.bin', 'request_url');
             INSERT INTO idempotency_records VALUES ('ext_key_0001', 'ab', 'job_0000000000000001', 1);
             PRAGMA user_version = 4;",
        )
        .unwrap();
    }
    let mut store = Store::open(&path).unwrap();
    assert_eq!(store.schema_version().unwrap(), 5);
    assert!(store.removed_jobs().unwrap().is_empty());
    let id = JobId::parse("job_0000000000000001").unwrap();
    store.remove_job(&id, 3).unwrap();
    assert_eq!(store.load_jobs().unwrap().len(), 1);
    assert!(store.find_idempotency("ext_key_0001").unwrap().is_some());
}
