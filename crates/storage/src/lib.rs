//! SQLite persistence for `dm-service` (B1.1, ARCH-05, COR-09; ADR 0006).
//!
//! - **Single writer.** Only `dm-service` opens the database, and it does so with
//!   `locking_mode=EXCLUSIVE`: a second process cannot read or write the file while the service
//!   holds it. Clients read through the service API.
//! - **Durability.** WAL with `synchronous=FULL`. The service writes at state changes (a job
//!   accepted, paused, resumed, cancelled, failed, completed), never per packet.
//! - **No segment map.** Resume state belongs to the engine; `jobs` keeps only the job's state, its
//!   current attempt and its last confirmed byte count. Since B1.2 (ADR 0007) `resume_checkpoints`
//!   holds the backend's *opaque* durable checkpoint for the job's current attempt; a checkpoint is
//!   committed together with its job row, refused for a stale attempt, and deleted in the same
//!   transaction that makes the job terminal or moves it to another attempt.
//! - **Completion journal (B1.3, ADR 0008).** `finalization_journal` records each stage of making
//!   a finished transfer final (intent → verified → placed → complete), bound to the job's attempt
//!   and the staging file it owns. Every stage is committed together with the job row; a stale
//!   attempt cannot write one, and cancelling or failing the job drops it.
//! - **Migrations** are forward-only, run in one transaction, and are tracked in
//!   `PRAGMA user_version`. A failed migration rolls back and leaves the original file intact. A
//!   database written by a newer Downova is refused instead of being opened.
//! - **Secrets.** `jobs.url` holds the raw source URL because a later recovery needs it; it is
//!   never logged ([`JobRow`]'s `Debug` redacts it) and never exported. Idempotency records keep
//!   only a SHA-256 of the request fingerprint.

use std::fmt;
use std::path::{Path, PathBuf};

use dm_domain::api::{Verification, VerificationSource};
use dm_domain::redact::redact_url;
use dm_domain::{AttemptId, ByteCount, DesiredState, ErrorData, JobId, Phase};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};

/// Forward-only migrations; index + 1 is the schema version they produce.
const MIGRATIONS: &[&str] = &[
    include_str!("../migrations/0001_jobs_and_idempotency.sql"),
    include_str!("../migrations/0002_resume_checkpoints.sql"),
    include_str!("../migrations/0003_finalization_journal.sql"),
    include_str!("../migrations/0004_file_names.sql"),
];

/// Schema version this build writes.
pub const SCHEMA_VERSION: u32 = 4;

/// Largest checkpoint payload the schema accepts (matches `dm_backend_api::MAX_CHECKPOINT_BYTES`).
pub const MAX_CHECKPOINT_PAYLOAD: usize = 64 * 1024;

/// Database file name inside the per-user data directory.
pub const FILE_NAME: &str = "downova.sqlite3";

/// Storage failure. Messages never contain URLs or other row contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageError {
    /// The file could not be opened or locked (for example, another process holds it).
    Open(String),
    /// The database was written by a newer Downova (schema `found` > `supported`).
    NewerSchema { found: u32, supported: u32 },
    /// A migration failed; the database was left unchanged.
    Migration { version: u32, reason: String },
    /// A stored value could not be decoded.
    Corrupt(String),
    /// A query failed.
    Sql(String),
    /// A checkpoint was offered for an attempt that is no longer the job's current one, or for a
    /// job that is finished or unknown. Nothing was written.
    StaleAttempt,
}

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Open(e) => write!(f, "cannot open the Downova database: {e}"),
            Self::NewerSchema { found, supported } => write!(
                f,
                "the Downova database uses schema {found}, but this version understands up to {supported}; install the newer Downova again or move the database aside"
            ),
            Self::Migration { version, reason } => write!(
                f,
                "upgrading the Downova database to schema {version} failed ({reason}); the database was left unchanged"
            ),
            Self::Corrupt(what) => write!(f, "the Downova database holds an invalid {what}"),
            Self::Sql(e) => write!(f, "database error: {e}"),
            Self::StaleAttempt => {
                write!(
                    f,
                    "checkpoint refused: it belongs to an attempt that is no longer current"
                )
            }
        }
    }
}

impl std::error::Error for StorageError {}

fn sql(e: rusqlite::Error) -> StorageError {
    // With exclusive locking, "busy" or "locked" only happens when another process holds the file.
    match e.sqlite_error_code() {
        Some(rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked) => {
            StorageError::Open(IN_USE.to_owned())
        }
        _ => StorageError::Sql(e.to_string()),
    }
}

const IN_USE: &str = "the database is in use by another process";

/// One persisted job. Live progress is not stored per packet; `downloaded` is the last value the
/// service confirmed at a state change.
#[derive(Clone, PartialEq, Eq)]
pub struct JobRow {
    pub id: JobId,
    /// Raw URL; see the module notes.
    pub url: String,
    /// The final file name: provisional while `name_source` is `None`, frozen afterwards (B1.6).
    pub display_name: String,
    /// The name partial and control files are created under; fixed at acceptance (B1.6).
    pub staging_name: String,
    /// Where the frozen final name came from (`filename_ext`, `filename`, `response_url`,
    /// `request_url`, `default`, `legacy`); `None` while it is provisional.
    pub name_source: Option<String>,
    pub destination: PathBuf,
    pub desired_state: DesiredState,
    pub phase: Phase,
    pub downloaded: ByteCount,
    pub total: Option<ByteCount>,
    pub expected_sha256: Option<String>,
    pub error: Option<ErrorData>,
    /// The service-owned attempt currently responsible for the job's bytes, if any.
    pub attempt_id: Option<AttemptId>,
    /// Verification evidence of a completed job (B1.3).
    pub verification: Option<Verification>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

impl fmt::Debug for JobRow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JobRow")
            .field("id", &self.id)
            .field("url", &redact_url(&self.url))
            .field("display_name", &self.display_name)
            .field("staging_name", &self.staging_name)
            .field("phase", &self.phase)
            .field("desired_state", &self.desired_state)
            .finish_non_exhaustive()
    }
}

/// Stage of the completion journal (B1.3). Each stage is committed before the next begins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum JournalStage {
    /// Finalization started; the staging file is the attempt's complete output.
    Intent,
    /// Length and SHA-256 computed (and matched, if a trusted checksum was given).
    Verified,
    /// The verified file is at the final path.
    Placed,
    /// The job is committed as completed; only cleanup of the staging name may remain.
    Complete,
}

impl JournalStage {
    fn as_str(self) -> &'static str {
        match self {
            Self::Intent => "intent",
            Self::Verified => "verified",
            Self::Placed => "placed",
            Self::Complete => "complete",
        }
    }

    fn parse(s: &str) -> Result<Self, StorageError> {
        Ok(match s {
            "intent" => Self::Intent,
            "verified" => Self::Verified,
            "placed" => Self::Placed,
            "complete" => Self::Complete,
            _ => return Err(StorageError::Corrupt("journal stage".into())),
        })
    }
}

/// Who put the output at the destination (B1.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// The service places the backend's staging file at the final path.
    Staged,
    /// The engine wrote the final path itself (aria2); verification is in place.
    Engine,
}

/// One completion-journal entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalRow {
    pub job_id: JobId,
    pub attempt_id: AttemptId,
    pub stage: JournalStage,
    pub placement: Placement,
    /// The attempt's own output file (equal to `final_path` for `Placement::Engine`).
    pub staging_path: PathBuf,
    pub final_path: PathBuf,
    /// Verified length and SHA-256 (from `Verified` on).
    pub length: Option<ByteCount>,
    pub sha256: Option<String>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

/// A backend's durable checkpoint for one job attempt (B1.2). `payload` is opaque here.
#[derive(Clone, PartialEq, Eq)]
pub struct CheckpointRow {
    pub job_id: JobId,
    pub attempt_id: AttemptId,
    pub backend_id: String,
    pub backend_version: Option<String>,
    pub format_version: u32,
    pub confirmed: ByteCount,
    /// May contain the source URL: never logged.
    pub payload: Vec<u8>,
    pub updated_at_ms: u64,
}

impl fmt::Debug for CheckpointRow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CheckpointRow")
            .field("job_id", &self.job_id)
            .field("attempt_id", &self.attempt_id)
            .field("backend_id", &self.backend_id)
            .field("format_version", &self.format_version)
            .field("confirmed", &self.confirmed)
            .field("payload_len", &self.payload.len())
            .finish_non_exhaustive()
    }
}

/// Idempotency key → job, with a hash of the request fingerprint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdempotencyRow {
    pub key: String,
    pub fingerprint_sha256: String,
    pub job_id: JobId,
    pub created_at_ms: u64,
}

/// The service's database connection.
#[derive(Debug)]
pub struct Store {
    conn: Connection,
}

impl Store {
    /// Opens (creating if needed) and migrates the database at `path`, taking an exclusive lock.
    pub fn open(path: &Path) -> Result<Self, StorageError> {
        let conn = Connection::open(path).map_err(|e| StorageError::Open(e.to_string()))?;
        Self::init(conn, true)
    }

    /// A private in-memory database (tests).
    pub fn open_in_memory() -> Result<Self, StorageError> {
        let conn = Connection::open_in_memory().map_err(|e| StorageError::Open(e.to_string()))?;
        Self::init(conn, false)
    }

    fn init(conn: Connection, file: bool) -> Result<Self, StorageError> {
        // Fail at once instead of waiting if another process holds the file.
        conn.busy_timeout(std::time::Duration::ZERO).map_err(sql)?;
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(sql)?;
        if file {
            conn.pragma_update(None, "locking_mode", "EXCLUSIVE")
                .map_err(sql)?;
            // The first statement that touches the file takes the exclusive lock (WAL needs it
            // before switching modes), so a second opener fails here.
            let mode: String = conn
                .query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))
                .map_err(|e| StorageError::Open(e.to_string()))?;
            if !mode.eq_ignore_ascii_case("wal") {
                return Err(StorageError::Open(format!(
                    "journal mode {mode} instead of WAL"
                )));
            }
            conn.pragma_update(None, "synchronous", "FULL")
                .map_err(sql)?;
            // Take the write lock now; in exclusive locking mode it is kept until the connection
            // closes, so no other process can open the database meanwhile.
            conn.execute_batch("BEGIN EXCLUSIVE; COMMIT;")
                .map_err(sql)?;
        }
        let mut store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    /// Current schema version.
    pub fn schema_version(&self) -> Result<u32, StorageError> {
        self.conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(sql)
    }

    fn migrate(&mut self) -> Result<(), StorageError> {
        let found = self.schema_version()?;
        if found > SCHEMA_VERSION {
            return Err(StorageError::NewerSchema {
                found,
                supported: SCHEMA_VERSION,
            });
        }
        if found == SCHEMA_VERSION {
            return Ok(());
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql)?;
        for (index, script) in MIGRATIONS.iter().enumerate().skip(found as usize) {
            let version = u32::try_from(index).map_or(u32::MAX, |i| i.saturating_add(1));
            tx.execute_batch(script)
                .map_err(|e| StorageError::Migration {
                    version,
                    reason: e.to_string(),
                })?;
        }
        tx.pragma_update(None, "user_version", SCHEMA_VERSION)
            .map_err(sql)?;
        // Dropping `tx` without commit rolls everything back.
        tx.commit().map_err(|e| StorageError::Migration {
            version: SCHEMA_VERSION,
            reason: e.to_string(),
        })
    }

    /// Records a newly accepted job and, when given, its idempotency record in one transaction:
    /// either both are durable or neither is.
    pub fn insert_job(
        &mut self,
        job: &JobRow,
        idempotency: Option<&IdempotencyRow>,
    ) -> Result<(), StorageError> {
        let tx = self.conn.transaction().map_err(sql)?;
        write_job(&tx, job, true)?;
        if let Some(rec) = idempotency {
            tx.execute(
                "INSERT INTO idempotency_records (idempotency_key, fingerprint_sha256, job_id, created_at_ms)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    rec.key,
                    rec.fingerprint_sha256,
                    rec.job_id.as_str(),
                    to_i64(rec.created_at_ms, "timestamp")?
                ],
            )
            .map_err(sql)?;
        }
        tx.commit().map_err(sql)
    }

    /// Writes a job's current state (it must exist). In the same transaction, a checkpoint that no
    /// longer applies (the job is finished, or it belongs to another attempt) is deleted.
    pub fn update_job(&mut self, job: &JobRow) -> Result<(), StorageError> {
        let tx = self.conn.transaction().map_err(sql)?;
        write_job(&tx, job, false)?;
        drop_stale_checkpoint(&tx, job)?;
        drop_abandoned_journal(&tx, job)?;
        tx.commit().map_err(sql)
    }

    /// Starts finalization: the job row (normally phase `verifying`) and the `Intent` entry in one
    /// transaction. Refused with [`StorageError::StaleAttempt`] unless the stored job is unfinished,
    /// its current attempt is the entry's, and no entry exists yet.
    pub fn begin_finalization(
        &mut self,
        job: &JobRow,
        entry: &JournalRow,
    ) -> Result<(), StorageError> {
        let tx = self.conn.transaction().map_err(sql)?;
        if !current_attempt(&tx, job, &entry.attempt_id)?
            || journal_attempt(&tx, &entry.job_id)?.is_some()
        {
            return Err(StorageError::StaleAttempt);
        }
        write_job(&tx, job, false)?;
        tx.execute(
            "INSERT INTO finalization_journal (job_id, attempt_id, stage, placement, staging_path,
                                               final_path, length, sha256, created_at_ms,
                                               updated_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            journal_params(entry)?,
        )
        .map_err(sql)?;
        tx.commit().map_err(sql)
    }

    /// Records the next stage together with the job row. Refused with
    /// [`StorageError::StaleAttempt`] unless an entry of the same attempt exists and the stored job is
    /// unfinished (the `Complete` stage is the one that makes it `completed`).
    pub fn advance_finalization(
        &mut self,
        job: &JobRow,
        entry: &JournalRow,
    ) -> Result<(), StorageError> {
        let tx = self.conn.transaction().map_err(sql)?;
        if !current_attempt(&tx, job, &entry.attempt_id)?
            || journal_attempt(&tx, &entry.job_id)?.as_deref() != Some(entry.attempt_id.as_str())
        {
            return Err(StorageError::StaleAttempt);
        }
        write_job(&tx, job, false)?;
        drop_stale_checkpoint(&tx, job)?;
        tx.execute(
            "UPDATE finalization_journal SET attempt_id = ?2, stage = ?3, placement = ?4,
                    staging_path = ?5, final_path = ?6, length = ?7, sha256 = ?8,
                    created_at_ms = ?9, updated_at_ms = ?10
             WHERE job_id = ?1",
            journal_params(entry)?,
        )
        .map_err(sql)?;
        tx.commit().map_err(sql)
    }

    /// Fails or cancels a job during finalization: the job row and the removal of its entry (and
    /// checkpoint) in one transaction.
    pub fn abandon_finalization(&mut self, job: &JobRow) -> Result<(), StorageError> {
        let tx = self.conn.transaction().map_err(sql)?;
        write_job(&tx, job, false)?;
        drop_stale_checkpoint(&tx, job)?;
        tx.execute(
            "DELETE FROM finalization_journal WHERE job_id = ?1",
            params![job.id.as_str()],
        )
        .map_err(sql)?;
        tx.commit().map_err(sql)
    }

    /// Removes a job's entry once the job is committed as completed and its staging name is gone.
    pub fn finish_finalization(&mut self, job_id: &JobId) -> Result<(), StorageError> {
        self.conn
            .execute(
                "DELETE FROM finalization_journal WHERE job_id = ?1 AND stage = 'complete'",
                params![job_id.as_str()],
            )
            .map(drop)
            .map_err(sql)
    }

    /// Every unfinished journal entry (reconciled at startup before ordinary recovery).
    pub fn load_journal(&self) -> Result<Vec<JournalRow>, StorageError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT job_id, attempt_id, stage, placement, staging_path, final_path, length,
                        sha256, created_at_ms, updated_at_ms
                 FROM finalization_journal ORDER BY created_at_ms, rowid",
            )
            .map_err(sql)?;
        let raw = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, Option<i64>>(6)?,
                    r.get::<_, Option<String>>(7)?,
                    r.get::<_, i64>(8)?,
                    r.get::<_, i64>(9)?,
                ))
            })
            .map_err(sql)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql)?;
        raw.into_iter()
            .map(
                |(
                    job,
                    attempt,
                    stage,
                    placement,
                    staging,
                    final_path,
                    length,
                    sha,
                    created,
                    updated,
                )| {
                    Ok(JournalRow {
                        job_id: JobId::parse(&job)
                            .map_err(|_| StorageError::Corrupt("job id".into()))?,
                        attempt_id: AttemptId::parse(&attempt)
                            .map_err(|_| StorageError::Corrupt("attempt id".into()))?,
                        stage: JournalStage::parse(&stage)?,
                        placement: match placement.as_str() {
                            "staged" => Placement::Staged,
                            "engine" => Placement::Engine,
                            _ => return Err(StorageError::Corrupt("journal placement".into())),
                        },
                        staging_path: PathBuf::from(staging),
                        final_path: PathBuf::from(final_path),
                        length: length
                            .map(|l| to_u64(l, "byte count").map(ByteCount))
                            .transpose()?,
                        sha256: sha,
                        created_at_ms: to_u64(created, "timestamp")?,
                        updated_at_ms: to_u64(updated, "timestamp")?,
                    })
                },
            )
            .collect()
    }

    /// Commits a job row and its attempt's checkpoint together. Refused with
    /// [`StorageError::StaleAttempt`] (nothing written) unless the stored job is unfinished and its
    /// current attempt is `checkpoint.attempt_id`, and `job` names the same attempt.
    pub fn save_checkpoint(
        &mut self,
        job: &JobRow,
        checkpoint: &CheckpointRow,
    ) -> Result<(), StorageError> {
        self.commit_checkpoint(job, checkpoint, &checkpoint.attempt_id)
    }

    /// After a restart: moves a job to its new attempt and stores the checkpoint that attempt was
    /// restored from, in one transaction. Refused with [`StorageError::StaleAttempt`] unless the
    /// stored job is unfinished and its current attempt is still `previous`.
    pub fn rebind_checkpoint(
        &mut self,
        job: &JobRow,
        checkpoint: &CheckpointRow,
        previous: &AttemptId,
    ) -> Result<(), StorageError> {
        self.commit_checkpoint(job, checkpoint, previous)
    }

    fn commit_checkpoint(
        &mut self,
        job: &JobRow,
        checkpoint: &CheckpointRow,
        stored_attempt: &AttemptId,
    ) -> Result<(), StorageError> {
        if checkpoint.payload.len() > MAX_CHECKPOINT_PAYLOAD {
            return Err(StorageError::Corrupt("checkpoint (too large)".into()));
        }
        let tx = self.conn.transaction().map_err(sql)?;
        let stored: Option<(String, Option<String>)> = tx
            .query_row(
                "SELECT phase, attempt_id FROM jobs WHERE job_id = ?1",
                params![checkpoint.job_id.as_str()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(sql)?;
        let current = match stored {
            Some((phase, attempt)) => {
                let phase: Phase = from_enum(&phase, "phase")?;
                !phase.is_terminal()
                    && attempt.as_deref() == Some(stored_attempt.as_str())
                    && job.id == checkpoint.job_id
                    && job.attempt_id.as_ref() == Some(&checkpoint.attempt_id)
                    && !job.phase.is_terminal()
            }
            None => false,
        };
        if !current {
            return Err(StorageError::StaleAttempt);
        }
        write_job(&tx, job, false)?;
        tx.execute(
            "INSERT INTO resume_checkpoints (job_id, attempt_id, backend_id, backend_version,
                                             format_version, confirmed_bytes, payload, updated_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT (job_id) DO UPDATE SET
                 attempt_id = excluded.attempt_id, backend_id = excluded.backend_id,
                 backend_version = excluded.backend_version,
                 format_version = excluded.format_version,
                 confirmed_bytes = excluded.confirmed_bytes, payload = excluded.payload,
                 updated_at_ms = excluded.updated_at_ms",
            params![
                checkpoint.job_id.as_str(),
                checkpoint.attempt_id.as_str(),
                checkpoint.backend_id,
                checkpoint.backend_version,
                checkpoint.format_version,
                to_i64(checkpoint.confirmed.0, "byte count")?,
                checkpoint.payload,
                to_i64(checkpoint.updated_at_ms, "timestamp")?,
            ],
        )
        .map_err(sql)?;
        tx.commit().map_err(sql)
    }

    /// The stored checkpoint of a job, if any.
    pub fn load_checkpoint(&self, job_id: &JobId) -> Result<Option<CheckpointRow>, StorageError> {
        let row: Option<RawCheckpoint> = self
            .conn
            .query_row(
                "SELECT attempt_id, backend_id, backend_version, format_version, confirmed_bytes,
                        payload, updated_at_ms
                 FROM resume_checkpoints WHERE job_id = ?1",
                params![job_id.as_str()],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                        r.get(6)?,
                    ))
                },
            )
            .optional()
            .map_err(sql)?;
        row.map(
            |(attempt, backend_id, backend_version, format, confirmed, payload, updated)| {
                Ok(CheckpointRow {
                    job_id: job_id.clone(),
                    attempt_id: AttemptId::parse(&attempt)
                        .map_err(|_| StorageError::Corrupt("attempt id".into()))?,
                    backend_id,
                    backend_version,
                    format_version: u32::try_from(format)
                        .map_err(|_| StorageError::Corrupt("checkpoint format".into()))?,
                    confirmed: ByteCount(to_u64(confirmed, "byte count")?),
                    payload,
                    updated_at_ms: to_u64(updated, "timestamp")?,
                })
            },
        )
        .transpose()
    }

    /// Deletes a job's checkpoint (if any).
    pub fn delete_checkpoint(&mut self, job_id: &JobId) -> Result<(), StorageError> {
        self.conn
            .execute(
                "DELETE FROM resume_checkpoints WHERE job_id = ?1",
                params![job_id.as_str()],
            )
            .map(drop)
            .map_err(sql)
    }

    /// All jobs, oldest first.
    pub fn load_jobs(&self) -> Result<Vec<JobRow>, StorageError> {
        struct Raw {
            id: String,
            url: String,
            display_name: String,
            destination: String,
            desired: String,
            phase: String,
            done: i64,
            total: Option<i64>,
            sha: Option<String>,
            error: Option<String>,
            attempt: Option<String>,
            created: i64,
            updated: i64,
            verified_sha: Option<String>,
            verified_source: Option<String>,
            staging_name: String,
            name_source: Option<String>,
        }
        let mut stmt = self
            .conn
            .prepare(
                "SELECT job_id, url, display_name, destination, desired_state, phase,
                        downloaded_bytes, total_bytes, expected_sha256, error_json, attempt_id,
                        created_at_ms, updated_at_ms, verified_sha256, verification_source,
                        staging_name, name_source
                 FROM jobs ORDER BY created_at_ms, rowid",
            )
            .map_err(sql)?;
        let raw = stmt
            .query_map([], |r| {
                Ok(Raw {
                    id: r.get(0)?,
                    url: r.get(1)?,
                    display_name: r.get(2)?,
                    destination: r.get(3)?,
                    desired: r.get(4)?,
                    phase: r.get(5)?,
                    done: r.get(6)?,
                    total: r.get(7)?,
                    sha: r.get(8)?,
                    error: r.get(9)?,
                    attempt: r.get(10)?,
                    created: r.get(11)?,
                    updated: r.get(12)?,
                    verified_sha: r.get(13)?,
                    verified_source: r.get(14)?,
                    staging_name: r.get(15)?,
                    name_source: r.get(16)?,
                })
            })
            .map_err(sql)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql)?;
        raw.into_iter()
            .map(|r| {
                Ok(JobRow {
                    id: JobId::parse(&r.id).map_err(|_| StorageError::Corrupt("job id".into()))?,
                    url: r.url,
                    display_name: r.display_name,
                    staging_name: if r.staging_name.is_empty() {
                        return Err(StorageError::Corrupt("staging name".into()));
                    } else {
                        r.staging_name
                    },
                    name_source: r.name_source,
                    destination: PathBuf::from(r.destination),
                    desired_state: from_enum(&r.desired, "desired state")?,
                    phase: from_enum(&r.phase, "phase")?,
                    downloaded: ByteCount(to_u64(r.done, "byte count")?),
                    total: r
                        .total
                        .map(|t| to_u64(t, "byte count").map(ByteCount))
                        .transpose()?,
                    expected_sha256: r.sha,
                    error: r
                        .error
                        .map(|e| {
                            serde_json::from_str(&e)
                                .map_err(|_| StorageError::Corrupt("error record".into()))
                        })
                        .transpose()?,
                    attempt_id: r
                        .attempt
                        .map(|a| {
                            AttemptId::parse(&a)
                                .map_err(|_| StorageError::Corrupt("attempt id".into()))
                        })
                        .transpose()?,
                    verification: match (r.verified_sha, r.verified_source.as_deref()) {
                        (Some(sha256), Some("local_hash")) => Some(Verification {
                            sha256,
                            source: VerificationSource::LocalHash,
                        }),
                        (Some(sha256), Some("publisher_checksum")) => Some(Verification {
                            sha256,
                            source: VerificationSource::PublisherChecksum,
                        }),
                        (None, None) => None,
                        _ => return Err(StorageError::Corrupt("verification record".into())),
                    },
                    created_at_ms: to_u64(r.created, "timestamp")?,
                    updated_at_ms: to_u64(r.updated, "timestamp")?,
                })
            })
            .collect()
    }

    /// All idempotency records.
    pub fn load_idempotency(&self) -> Result<Vec<IdempotencyRow>, StorageError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT idempotency_key, fingerprint_sha256, job_id, created_at_ms
                 FROM idempotency_records",
            )
            .map_err(sql)?;
        let raw = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)?,
                ))
            })
            .map_err(sql)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql)?;
        raw.into_iter()
            .map(|(key, fingerprint_sha256, job_id, created)| {
                Ok(IdempotencyRow {
                    key,
                    fingerprint_sha256,
                    job_id: JobId::parse(&job_id)
                        .map_err(|_| StorageError::Corrupt("job id".into()))?,
                    created_at_ms: to_u64(created, "timestamp")?,
                })
            })
            .collect()
    }

    /// Looks up one idempotency key.
    pub fn find_idempotency(&self, key: &str) -> Result<Option<IdempotencyRow>, StorageError> {
        let row: Option<(String, String, String, i64)> = self
            .conn
            .query_row(
                "SELECT idempotency_key, fingerprint_sha256, job_id, created_at_ms
                 FROM idempotency_records WHERE idempotency_key = ?1",
                params![key],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()
            .map_err(sql)?;
        row.map(|(key, fingerprint_sha256, job_id, created)| {
            Ok(IdempotencyRow {
                key,
                fingerprint_sha256,
                job_id: JobId::parse(&job_id)
                    .map_err(|_| StorageError::Corrupt("job id".into()))?,
                created_at_ms: to_u64(created, "timestamp")?,
            })
        })
        .transpose()
    }

    /// Deletes idempotency records created before `before_ms` (bounded retention, COR-09). The
    /// jobs themselves are kept. Returns the number removed.
    pub fn prune_idempotency(&mut self, before_ms: u64) -> Result<usize, StorageError> {
        self.conn
            .execute(
                "DELETE FROM idempotency_records WHERE created_at_ms < ?1",
                params![to_i64(before_ms, "timestamp")?],
            )
            .map_err(sql)
    }
}

fn write_job(tx: &Transaction<'_>, job: &JobRow, insert: bool) -> Result<(), StorageError> {
    let destination = job
        .destination
        .to_str()
        .ok_or_else(|| StorageError::Corrupt("destination path (not UTF-8)".into()))?;
    let error = job
        .error
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|_| StorageError::Corrupt("error record".into()))?;
    let total = job.total.map(|t| to_i64(t.0, "byte count")).transpose()?;
    let values = params![
        job.id.as_str(),
        job.url,
        job.display_name,
        destination,
        to_enum(&job.desired_state)?,
        to_enum(&job.phase)?,
        to_i64(job.downloaded.0, "byte count")?,
        total,
        job.expected_sha256,
        error,
        to_i64(job.created_at_ms, "timestamp")?,
        to_i64(job.updated_at_ms, "timestamp")?,
        job.attempt_id.as_ref().map(AttemptId::as_str),
        job.verification.as_ref().map(|v| v.sha256.as_str()),
        job.verification.as_ref().map(|v| match v.source {
            VerificationSource::LocalHash => "local_hash",
            VerificationSource::PublisherChecksum => "publisher_checksum",
        }),
        job.staging_name,
        job.name_source,
    ];
    let changed = if insert {
        tx.execute(
            "INSERT INTO jobs (job_id, url, display_name, destination, desired_state, phase,
                               downloaded_bytes, total_bytes, expected_sha256, error_json,
                               created_at_ms, updated_at_ms, attempt_id, verified_sha256,
                               verification_source, staging_name, name_source)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
            values,
        )
    } else {
        tx.execute(
            "UPDATE jobs SET url = ?2, display_name = ?3, destination = ?4, desired_state = ?5,
                             phase = ?6, downloaded_bytes = ?7, total_bytes = ?8,
                             expected_sha256 = ?9, error_json = ?10, created_at_ms = ?11,
                             updated_at_ms = ?12, attempt_id = ?13, verified_sha256 = ?14,
                             verification_source = ?15, staging_name = ?16, name_source = ?17
             WHERE job_id = ?1",
            values,
        )
    }
    .map_err(sql)?;
    if changed == 1 {
        Ok(())
    } else {
        Err(StorageError::Sql(format!("job {} not found", job.id)))
    }
}

/// Deletes a checkpoint that no longer applies to `job`: it is finished, or the checkpoint belongs
/// to a different attempt.
/// Deletes the journal entry of a job that was cancelled or failed (a completed job keeps its
/// `complete` entry until cleanup).
fn drop_abandoned_journal(tx: &Transaction<'_>, job: &JobRow) -> Result<(), StorageError> {
    if matches!(job.phase, Phase::Cancelled | Phase::Failed) {
        tx.execute(
            "DELETE FROM finalization_journal WHERE job_id = ?1",
            params![job.id.as_str()],
        )
        .map_err(sql)?;
    }
    Ok(())
}

/// True when the stored job is unfinished, its current attempt is `attempt`, and `job` (the row
/// about to be written) names the same job and attempt.
fn current_attempt(
    tx: &Transaction<'_>,
    job: &JobRow,
    attempt: &AttemptId,
) -> Result<bool, StorageError> {
    let stored: Option<(String, Option<String>)> = tx
        .query_row(
            "SELECT phase, attempt_id FROM jobs WHERE job_id = ?1",
            params![job.id.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(sql)?;
    Ok(match stored {
        Some((phase, stored_attempt)) => {
            let phase: Phase = from_enum(&phase, "phase")?;
            !phase.is_terminal()
                && stored_attempt.as_deref() == Some(attempt.as_str())
                && job.attempt_id.as_ref() == Some(attempt)
        }
        None => false,
    })
}

/// The attempt of a job's journal entry, if one exists.
fn journal_attempt(tx: &Transaction<'_>, job: &JobId) -> Result<Option<String>, StorageError> {
    tx.query_row(
        "SELECT attempt_id FROM finalization_journal WHERE job_id = ?1",
        params![job.as_str()],
        |r| r.get(0),
    )
    .optional()
    .map_err(sql)
}

fn journal_params(entry: &JournalRow) -> Result<[Box<dyn rusqlite::ToSql>; 10], StorageError> {
    let path = |p: &Path, what: &str| {
        p.to_str()
            .map(str::to_owned)
            .ok_or_else(|| StorageError::Corrupt(format!("{what} (not UTF-8)")))
    };
    Ok([
        Box::new(entry.job_id.as_str().to_owned()),
        Box::new(entry.attempt_id.as_str().to_owned()),
        Box::new(entry.stage.as_str()),
        Box::new(match entry.placement {
            Placement::Staged => "staged",
            Placement::Engine => "engine",
        }),
        Box::new(path(&entry.staging_path, "staging path")?),
        Box::new(path(&entry.final_path, "final path")?),
        Box::new(
            entry
                .length
                .map(|l| to_i64(l.0, "byte count"))
                .transpose()?,
        ),
        Box::new(entry.sha256.clone()),
        Box::new(to_i64(entry.created_at_ms, "timestamp")?),
        Box::new(to_i64(entry.updated_at_ms, "timestamp")?),
    ])
}

/// attempt, backend, backend version, format, confirmed, payload, updated (one stored row).
type RawCheckpoint = (String, String, Option<String>, i64, i64, Vec<u8>, i64);

fn drop_stale_checkpoint(tx: &Transaction<'_>, job: &JobRow) -> Result<(), StorageError> {
    tx.execute(
        "DELETE FROM resume_checkpoints
         WHERE job_id = ?1 AND (?2 OR attempt_id IS NOT ?3)",
        params![
            job.id.as_str(),
            job.phase.is_terminal(),
            job.attempt_id.as_ref().map(AttemptId::as_str)
        ],
    )
    .map(drop)
    .map_err(sql)
}

fn to_i64(v: u64, what: &str) -> Result<i64, StorageError> {
    i64::try_from(v).map_err(|_| StorageError::Corrupt(format!("{what} (out of range)")))
}

fn to_u64(v: i64, what: &str) -> Result<u64, StorageError> {
    u64::try_from(v).map_err(|_| StorageError::Corrupt(format!("{what} (negative)")))
}

/// Enums are stored as their wire names (`"running"`, `"transferring"`, …).
fn to_enum<T: serde::Serialize>(v: &T) -> Result<String, StorageError> {
    match serde_json::to_value(v) {
        Ok(serde_json::Value::String(s)) => Ok(s),
        _ => Err(StorageError::Corrupt("state value".into())),
    }
}

fn from_enum<T: serde::de::DeserializeOwned>(s: &str, what: &str) -> Result<T, StorageError> {
    serde_json::from_value(serde_json::Value::String(s.to_owned()))
        .map_err(|_| StorageError::Corrupt(what.to_owned()))
}

#[cfg(test)]
mod tests;
