#![allow(clippy::unwrap_used, clippy::arithmetic_side_effects)]

use super::*;
use dm_domain::{ErrorKind, NextAction};

fn job(id: &str, phase: Phase) -> JobRow {
    JobRow {
        id: JobId::parse(id).unwrap(),
        url: "https://example.org/f.zip?sig=SECRET".into(),
        display_name: "f.zip".into(),
        destination: PathBuf::from("downloads"),
        desired_state: DesiredState::Running,
        phase,
        downloaded: ByteCount(0),
        total: Some(ByteCount(u64::from(u32::MAX) * 4)),
        expected_sha256: None,
        error: None,
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
