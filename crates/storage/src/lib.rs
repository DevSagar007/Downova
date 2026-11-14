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
//! - **Queues, rules and settings (B2.8–B2.11, ADR 0015, ADR 0017).** `queues` holds the
//!   persistent queues (the default `main` always exists); every job row carries its queue,
//!   priority and manual position. `destination_rules` maps file-type categories to folders and
//!   `settings` holds the collision policy. A queue with jobs is never deleted unless its jobs move
//!   to another queue in the same transaction; no job or file is ever deleted with a queue.
//! - **Secrets.** `jobs.url` holds the raw source URL because a later recovery needs it; it is
//!   never logged ([`JobRow`]'s `Debug` redacts it) and never exported. Idempotency records keep
//!   only a SHA-256 of the request fingerprint.

use std::fmt;
use std::path::{Path, PathBuf};

use dm_domain::api::{DestinationRule, Verification, VerificationSource};
use dm_domain::policy::{CollisionPolicy, FileCategory, Priority};
use dm_domain::redact::redact_url;
use dm_domain::{AttemptId, ByteCount, DesiredState, ErrorData, JobId, Phase};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};

pub mod backup;
pub mod seal;
use seal::Sealer;

/// Forward-only migrations; index + 1 is the schema version they produce.
const MIGRATIONS: &[&str] = &[
    include_str!("../migrations/0001_jobs_and_idempotency.sql"),
    include_str!("../migrations/0002_resume_checkpoints.sql"),
    include_str!("../migrations/0003_finalization_journal.sql"),
    include_str!("../migrations/0004_file_names.sql"),
    include_str!("../migrations/0005_removed_jobs.sql"),
    include_str!("../migrations/0006_queues_rules_names.sql"),
    include_str!("../migrations/0007_history_retention.sql"),
    include_str!("../migrations/0008_operations.sql"),
    include_str!("../migrations/0009_notifications.sql"),
    include_str!("../migrations/0010_speed_limits.sql"),
    include_str!("../migrations/0011_queue_schedules.sql"),
    include_str!("../migrations/0012_attachment_scan.sql"),
    include_str!("../migrations/0013_credentials_sealed_urls.sql"),
    include_str!("../migrations/0014_reconnect_operations.sql"),
];

/// Schema version this build writes.
pub const SCHEMA_VERSION: u32 = 14;

/// The default queue (B2.11). It always exists and cannot be deleted.
pub const DEFAULT_QUEUE: &str = "main";

/// Largest checkpoint payload the schema accepts (matches `dm_backend_api::MAX_CHECKPOINT_BYTES`).
pub const MAX_CHECKPOINT_PAYLOAD: usize = 64 * 1024;

/// Longest accepted history retention period (B2.12).
pub const MAX_RETENTION_DAYS: u32 = 3650;

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
    /// Only a completed, failed or cancelled job without a finalization entry can be removed from
    /// the list (ADR 0013). Nothing was written.
    NotRemovable,
    /// The queue still has jobs and no target queue was given, or it is the default queue, or the
    /// target does not exist (ADR 0017). Nothing was written.
    QueueNotDeletable,
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
            Self::NotRemovable => {
                f.write_str("only a finished download can be removed from the list")
            }
            Self::QueueNotDeletable => f.write_str(
                "the queue cannot be deleted: it is the default queue, still has downloads, or the target queue does not exist",
            ),
        }
    }
}

impl std::error::Error for StorageError {}

pub(crate) fn sql(e: rusqlite::Error) -> StorageError {
    // With exclusive locking, "busy" or "locked" only happens when another process holds the file.
    match e.sqlite_error_code() {
        Some(rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked) => {
            StorageError::Open(IN_USE.to_owned())
        }
        _ => StorageError::Sql(e.to_string()),
    }
}

const IN_USE: &str = "the database is in use by another process";

/// The saved proxy setting (F06, ADR 0034), stored as JSON under `settings.proxy`. It never holds
/// the password: `password_saved` only says one is in the OS credential store.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProxyRow {
    pub mode: dm_domain::proxy::ProxyMode,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub username: Option<String>,
    pub password_saved: bool,
}

impl ProxyRow {
    /// The same rules as `proxy.v1.set`: a damaged row is refused, never half-applied.
    pub fn is_valid(&self) -> bool {
        use dm_domain::proxy::{ProxyMode, valid_proxy_host, valid_proxy_username};
        self.host.as_deref().is_none_or(valid_proxy_host)
            && self.port != Some(0)
            && self.username.as_deref().is_none_or(valid_proxy_username)
            && (!self.password_saved || self.username.is_some())
            && (self.mode == ProxyMode::Direct || (self.host.is_some() && self.port.is_some()))
    }
}

/// One persisted job. Live progress is not stored per packet; `downloaded` is the last value the
/// service confirmed at a state change.
/// A queue's stored schedule and its durable evaluation state (CORE-08).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduleRow {
    pub queue_id: String,
    pub schedule_json: String,
    pub evaluated_ms: Option<u64>,
    pub catch_up_for_ms: Option<u64>,
    pub catch_up_from_ms: Option<u64>,
    pub catch_up_until_ms: Option<u64>,
}

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
    /// Queue membership, priority and manual position (B2.11).
    pub queue_id: String,
    pub priority: Priority,
    pub queue_position: i64,
    /// Destination category it was filed under (B2.8); `None` for older jobs.
    pub category: Option<FileCategory>,
    /// False while an ask-on-collision job waits for the user to confirm its final name.
    pub name_confirmed: bool,
    /// The attachment check's evidence (F07/E06, ADR 0026); `None`: none recorded.
    pub scan: Option<dm_domain::api::ScanRecord>,
    /// The login context of a signed-in download (BRW-05, ADR 0027); `None`: public.
    pub credential_id: Option<String>,
    /// Set when loading: the stored URL is only the display form, because its protected part is
    /// sealed and the data key is unavailable (or the material was never persisted). Ignored on write.
    pub url_unavailable: bool,
}

/// A login context's stored metadata (ADR 0027). Never a secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialRow {
    pub credential_id: String,
    pub origin: String,
    /// `chrome` or `edge`.
    pub browser: String,
    pub profile_id: String,
    /// `temporary` or `remembered`.
    pub mode: String,
    /// `pending`, `active` or `forgotten`.
    pub state: String,
    pub generation: u32,
    pub created_at_ms: u64,
    pub expires_at_ms: Option<u64>,
}

/// Whether a URL carries material a display form drops (userinfo, a query or a fragment).
pub fn url_has_protected_parts(url: &str) -> bool {
    redact_url(url) != url || url.contains('#')
}

/// The display form of a signed-in download's URL: its origin only (ADR 0028).
pub fn signed_in_display(url: &str) -> String {
    let redacted = redact_url(url);
    let (scheme, rest) = redacted
        .split_once("://")
        .unwrap_or(("https", redacted.as_str()));
    let rest = rest.split(['?', '#']).next().unwrap_or_default();
    let host = rest.split('/').next().unwrap_or_default();
    // ADR 0028: no path segment at all. A signed-in link's last segment can be the token itself
    // (`/dl/<token>`); the file name is shown as the download's name, never from the link.
    format!("{scheme}://{host}/…")
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
    /// The attachment check of this placement started (ADR 0026); `None`: not started.
    pub scan_started_ms: Option<u64>,
    /// The placed file's identity, recorded with `Placed` (ADR 0026).
    pub final_file_id: Option<String>,
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

/// A durable explicit job operation (B2.16/B2.17, ADR 0020): Retry or Refresh link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationRow {
    pub operation_id: String,
    /// `retry` or `replace_source`.
    pub kind: String,
    pub job_id: JobId,
    /// SHA-256 of what was asked (kind, job and request fields), lower-case hex.
    pub fingerprint_sha256: String,
    /// The stable outcome code the first request answered with.
    pub outcome: String,
    pub created_at_ms: u64,
}

/// One persisted queue (B2.11).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueRow {
    pub queue_id: String,
    pub name: String,
    pub position: u32,
    pub max_active: u32,
    pub running: bool,
    pub created_at_ms: u64,
}

/// The service's database connection.
#[derive(Debug)]
pub struct Store {
    conn: Connection,
    /// The data key for protected URL material (ADR 0027); `None`: nothing is sealed or opened.
    sealer: Option<Sealer>,
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
        let mut store = Self { conn, sealer: None };
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
        // Debug builds only (ADR 0030 tests): fail after the scripts ran, before the commit, to
        // prove that a failed migration leaves the database exactly as it was.
        #[cfg(debug_assertions)]
        if std::env::var_os("DOWNOVA_TEST_FAIL_MIGRATION").is_some_and(|v| v == "1") {
            return Err(StorageError::Migration {
                version: SCHEMA_VERSION,
                reason: "test: injected migration failure".into(),
            });
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
        self.insert_job_with_speed_limit(job, idempotency, None)
    }

    /// [`Store::insert_job`] plus the job's own bandwidth limit (CORE-06), all in one transaction:
    /// the job, its idempotency record and its limit are durable together or not at all.
    pub fn insert_job_with_speed_limit(
        &mut self,
        job: &JobRow,
        idempotency: Option<&IdempotencyRow>,
        speed_limit_bps: Option<u64>,
    ) -> Result<(), StorageError> {
        let tx = self.conn.transaction().map_err(sql)?;
        write_job(&tx, self.sealer.as_ref(), job, true)?;
        if let Some(bps) = speed_limit_bps {
            tx.execute(
                "UPDATE jobs SET speed_limit_bps = ?2 WHERE job_id = ?1",
                params![job.id.as_str(), to_i64(bps, "speed limit")?],
            )
            .map_err(sql)?;
        }
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
        write_job(&tx, self.sealer.as_ref(), job, false)?;
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
        write_job(&tx, self.sealer.as_ref(), job, false)?;
        tx.execute(
            "INSERT INTO finalization_journal (job_id, attempt_id, stage, placement, staging_path,
                                               final_path, length, sha256, created_at_ms,
                                               updated_at_ms, scan_started_ms, final_file_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
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
        write_job(&tx, self.sealer.as_ref(), job, false)?;
        drop_stale_checkpoint(&tx, job)?;
        tx.execute(
            "UPDATE finalization_journal SET attempt_id = ?2, stage = ?3, placement = ?4,
                    staging_path = ?5, final_path = ?6, length = ?7, sha256 = ?8,
                    created_at_ms = ?9, updated_at_ms = ?10, scan_started_ms = ?11,
                    final_file_id = ?12
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
        write_job(&tx, self.sealer.as_ref(), job, false)?;
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

    /// Installs the data key (ADR 0027). From now on protected URL material is sealed on write
    /// and opened on read.
    pub fn set_sealer(&mut self, key: &[u8; 32]) {
        self.sealer = Some(Sealer::new(key));
    }

    pub fn has_sealer(&self) -> bool {
        self.sealer.is_some()
    }

    /// ADR 0032: whether the job has a sealed checkpoint that cannot be opened because no data key
    /// is loaded (the progress exists and comes back once the key does).
    pub fn checkpoint_needs_key(&self, job_id: &JobId) -> Result<bool, StorageError> {
        if self.sealer.is_some() {
            return Ok(false);
        }
        let sealed: Option<i64> = self
            .conn
            .query_row(
                "SELECT sealed FROM resume_checkpoints WHERE job_id = ?1",
                params![job_id.as_str()],
                |r| r.get(0),
            )
            .optional()
            .map_err(sql)?;
        Ok(sealed == Some(1))
    }

    /// Re-seals rows written before migration 13 (or while the key was unavailable): every job
    /// whose plain `url` still carries protected material, and every unsealed checkpoint. One row
    /// per transaction: a failure leaves that row as it was. Then a WAL checkpoint and a VACUUM, so
    /// the live file no longer holds the old pages (best effort; older backups are out of reach).
    /// Returns (jobs re-sealed, checkpoints re-sealed).
    pub fn reseal_legacy(&mut self) -> Result<(usize, usize), StorageError> {
        if self.sealer.is_none() {
            return Ok((0, 0));
        }
        let jobs: Vec<(String, String, Option<String>)> = {
            let mut stmt = self
                .conn
                .prepare("SELECT job_id, url, credential_id FROM jobs WHERE url_sealed IS NULL")
                .map_err(sql)?;
            stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                .map_err(sql)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(sql)?
        };
        let mut sealed_jobs = 0usize;
        for (id, url, credential) in jobs {
            if credential.is_none() && !url_has_protected_parts(&url) {
                continue;
            }
            if url.contains("REDACTED") || url.ends_with("/…") {
                continue; // already only a display form: nothing left to protect
            }
            let Some(s) = self.sealer.as_ref() else { break };
            let Ok(blob) = s.seal(&format!("{id}|url"), url.as_bytes()) else {
                continue;
            };
            let display = if credential.is_some() {
                signed_in_display(&url)
            } else {
                redact_url(&url)
            };
            let tx = self.conn.transaction().map_err(sql)?;
            tx.execute(
                "UPDATE jobs SET url = ?2, url_sealed = ?3 WHERE job_id = ?1 AND url_sealed IS NULL",
                params![id, display, blob],
            )
            .map_err(sql)?;
            tx.commit().map_err(sql)?;
            sealed_jobs = sealed_jobs.saturating_add(1);
        }
        let cps: Vec<(String, Vec<u8>)> = {
            let mut stmt = self
                .conn
                .prepare("SELECT job_id, payload FROM resume_checkpoints WHERE sealed = 0")
                .map_err(sql)?;
            stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                .map_err(sql)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(sql)?
        };
        let mut sealed_cps = 0usize;
        for (id, payload) in cps {
            let Some(s) = self.sealer.as_ref() else { break };
            let Ok(blob) = s.seal(&format!("{id}|checkpoint"), &payload) else {
                continue;
            };
            if blob.len() > MAX_CHECKPOINT_PAYLOAD {
                continue;
            }
            let tx = self.conn.transaction().map_err(sql)?;
            tx.execute(
                "UPDATE resume_checkpoints SET payload = ?2, sealed = 1 WHERE job_id = ?1 AND sealed = 0",
                params![id, blob],
            )
            .map_err(sql)?;
            tx.commit().map_err(sql)?;
            sealed_cps = sealed_cps.saturating_add(1);
        }
        if sealed_jobs > 0 || sealed_cps > 0 {
            let _ = self
                .conn
                .execute_batch("PRAGMA wal_checkpoint(TRUNCATE); VACUUM;");
        }
        Ok((sealed_jobs, sealed_cps))
    }

    /// Rows that still hold protected material in the clear (diagnostics: legacy rows when the
    /// key is unavailable).
    pub fn legacy_plaintext_rows(&self) -> Result<usize, StorageError> {
        let urls: Vec<(String, Option<String>)> = {
            let mut stmt = self
                .conn
                .prepare("SELECT url, credential_id FROM jobs WHERE url_sealed IS NULL")
                .map_err(sql)?;
            stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                .map_err(sql)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(sql)?
        };
        let jobs = urls
            .iter()
            .filter(|(u, c)| {
                !u.contains("REDACTED")
                    && !u.ends_with("/…")
                    && (c.is_some() || url_has_protected_parts(u))
            })
            .count();
        let cps: i64 = self
            .conn
            .query_row(
                "SELECT count(*) FROM resume_checkpoints WHERE sealed = 0",
                [],
                |r| r.get(0),
            )
            .map_err(sql)?;
        Ok(jobs.saturating_add(usize::try_from(cps).unwrap_or(0)))
    }

    /// Records a login context's metadata (ADR 0027). Replacing an existing id keeps its row and
    /// bumps nothing: the same idempotency key maps to the same id.
    pub fn upsert_credential(&mut self, c: &CredentialRow) -> Result<(), StorageError> {
        upsert_credential_in(&self.conn, c)
    }

    /// Reconnect (ADR 0028): binds a fresh credential to a job in one transaction: the new
    /// credential row (active), the job with its new reference, the operation, and the retired
    /// credential row when no job references it any more. Refused (nothing written) when the
    /// stored job is removed or no longer references `expected` (a concurrent change).
    pub fn reconnect_job(
        &mut self,
        job: &JobRow,
        credential: &CredentialRow,
        operation: &OperationRow,
        expected: &str,
    ) -> Result<(), StorageError> {
        let tx = self.conn.transaction().map_err(sql)?;
        let current: Option<Option<String>> = tx
            .query_row(
                "SELECT credential_id FROM jobs WHERE job_id = ?1 AND removed_at_ms IS NULL",
                params![job.id.as_str()],
                |r| r.get(0),
            )
            .optional()
            .map_err(sql)?;
        if current.flatten().as_deref() != Some(expected) {
            return Err(StorageError::StaleAttempt);
        }
        upsert_credential_in(&tx, credential)?;
        write_job(&tx, self.sealer.as_ref(), job, false)?;
        insert_operation(&tx, operation)?;
        tx.execute(
            "DELETE FROM credentials WHERE credential_id = ?1
               AND NOT EXISTS (SELECT 1 FROM jobs WHERE credential_id = ?1)",
            params![expected],
        )
        .map_err(sql)?;
        tx.commit().map_err(sql)
    }

    pub fn set_credential_state(&mut self, id: &str, state: &str) -> Result<(), StorageError> {
        let n = self
            .conn
            .execute(
                "UPDATE credentials SET state = ?2 WHERE credential_id = ?1",
                params![id, state],
            )
            .map_err(sql)?;
        if n == 1 {
            Ok(())
        } else {
            Err(StorageError::Corrupt("no such credential".into()))
        }
    }

    /// Removes a credential row that no job references (a failed add).
    pub fn delete_credential(&mut self, id: &str) -> Result<(), StorageError> {
        self.conn
            .execute(
                "DELETE FROM credentials WHERE credential_id = ?1
                   AND NOT EXISTS (SELECT 1 FROM jobs WHERE credential_id = ?1)",
                params![id],
            )
            .map(drop)
            .map_err(sql)
    }

    pub fn load_credentials(&self) -> Result<Vec<CredentialRow>, StorageError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT credential_id, origin, browser, profile_id, mode, state, generation,
                        created_at_ms, expires_at_ms
                 FROM credentials ORDER BY created_at_ms, credential_id",
            )
            .map_err(sql)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, i64>(6)?,
                    r.get::<_, i64>(7)?,
                    r.get::<_, Option<i64>>(8)?,
                ))
            })
            .map_err(sql)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql)?;
        rows.into_iter()
            .map(
                |(id, origin, browser, profile, mode, state, gen_, created, expires)| {
                    Ok(CredentialRow {
                        credential_id: id,
                        origin,
                        browser,
                        profile_id: profile,
                        mode,
                        state,
                        generation: u32::try_from(gen_)
                            .map_err(|_| StorageError::Corrupt("generation".into()))?,
                        created_at_ms: to_u64(created, "timestamp")?,
                        expires_at_ms: expires.map(|e| to_u64(e, "timestamp")).transpose()?,
                    })
                },
            )
            .collect()
    }

    /// Every unfinished journal entry (reconciled at startup before ordinary recovery).
    pub fn load_journal(&self) -> Result<Vec<JournalRow>, StorageError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT job_id, attempt_id, stage, placement, staging_path, final_path, length,
                        sha256, created_at_ms, updated_at_ms, scan_started_ms, final_file_id
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
                    r.get::<_, Option<i64>>(10)?,
                    r.get::<_, Option<String>>(11)?,
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
                    scan_started,
                    file_id,
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
                        scan_started_ms: scan_started
                            .map(|t| to_u64(t, "timestamp"))
                            .transpose()?,
                        final_file_id: file_id,
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
        write_job(&tx, self.sealer.as_ref(), job, false)?;
        upsert_checkpoint(&tx, self.sealer.as_ref(), checkpoint)?;
        tx.commit().map_err(sql)
    }

    /// The stored checkpoint of a job, if any.
    pub fn load_checkpoint(&self, job_id: &JobId) -> Result<Option<CheckpointRow>, StorageError> {
        let row: Option<StoredCheckpoint> = self
            .conn
            .query_row(
                "SELECT attempt_id, backend_id, backend_version, format_version, confirmed_bytes,
                        payload, updated_at_ms, sealed
                 FROM resume_checkpoints WHERE job_id = ?1",
                params![job_id.as_str()],
                |r| {
                    let sealed: i64 = r.get(7)?;
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        (r.get::<_, Vec<u8>>(5)?, sealed == 1),
                        r.get(6)?,
                    ))
                },
            )
            .optional()
            .map_err(sql)?;
        // ADR 0027: a sealed payload without the key is unusable here (no checkpoint).
        let row: Option<RawCheckpoint> = match row {
            Some((a, b, c, d, e, (payload, true), g)) => self
                .sealer
                .as_ref()
                .and_then(|s| {
                    s.open(&format!("{}|checkpoint", job_id.as_str()), &payload)
                        .ok()
                })
                .map(|open| (a, b, c, d, e, open, g)),
            Some((a, b, c, d, e, (payload, false), g)) => Some((a, b, c, d, e, payload, g)),
            None => None,
        };
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
            queue_id: String,
            priority: i64,
            queue_position: i64,
            category: Option<String>,
            name_confirmed: i64,
            scan: Option<String>,
            credential_id: Option<String>,
            url_sealed: Option<Vec<u8>>,
        }
        let mut stmt = self
            .conn
            .prepare(
                "SELECT job_id, url, display_name, destination, desired_state, phase,
                        downloaded_bytes, total_bytes, expected_sha256, error_json, attempt_id,
                        created_at_ms, updated_at_ms, verified_sha256, verification_source,
                        staging_name, name_source, queue_id, priority, queue_position,
                        category, name_confirmed, scan_json, credential_id, url_sealed
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
                    queue_id: r.get(17)?,
                    priority: r.get(18)?,
                    queue_position: r.get(19)?,
                    category: r.get(20)?,
                    name_confirmed: r.get(21)?,
                    scan: r.get(22)?,
                    credential_id: r.get(23)?,
                    url_sealed: r.get(24)?,
                })
            })
            .map_err(sql)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql)?;
        raw.into_iter()
            .map(|r| {
                // ADR 0027: the full URL comes back only through the data key.
                let opened = r.url_sealed.as_ref().and_then(|blob| {
                    self.sealer
                        .as_ref()
                        .and_then(|s| s.open(&format!("{}|url", r.id), blob).ok())
                        .and_then(|b| String::from_utf8(b).ok())
                });
                let url_unavailable = opened.is_none()
                    && (r.url_sealed.is_some()
                        || r.credential_id.is_some()
                        || r.url.contains("REDACTED")
                        || r.url.ends_with("/…"));
                Ok(JobRow {
                    id: JobId::parse(&r.id).map_err(|_| StorageError::Corrupt("job id".into()))?,
                    url: opened.unwrap_or(r.url),
                    credential_id: r.credential_id,
                    url_unavailable,
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
                    scan: r
                        .scan
                        .map(|s| {
                            serde_json::from_str(&s)
                                .map_err(|_| StorageError::Corrupt("scan record".into()))
                        })
                        .transpose()?,
                    created_at_ms: to_u64(r.created, "timestamp")?,
                    updated_at_ms: to_u64(r.updated, "timestamp")?,
                    queue_id: r.queue_id,
                    priority: Priority::from_i64(r.priority)
                        .ok_or_else(|| StorageError::Corrupt("priority".into()))?,
                    queue_position: r.queue_position,
                    category: r
                        .category
                        .map(|c| {
                            FileCategory::parse(&c)
                                .ok_or_else(|| StorageError::Corrupt("category".into()))
                        })
                        .transpose()?,
                    name_confirmed: r.name_confirmed != 0,
                })
            })
            .collect()
    }

    /// All queues, in list order.
    pub fn load_queues(&self) -> Result<Vec<QueueRow>, StorageError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT queue_id, name, position, max_active, running, created_at_ms
                 FROM queues ORDER BY position, created_at_ms, queue_id",
            )
            .map_err(sql)?;
        let raw = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, i64>(5)?,
                ))
            })
            .map_err(sql)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql)?;
        raw.into_iter()
            .map(|(queue_id, name, position, max_active, running, created)| {
                Ok(QueueRow {
                    queue_id,
                    name,
                    position: u32::try_from(position)
                        .map_err(|_| StorageError::Corrupt("queue position".into()))?,
                    max_active: u32::try_from(max_active)
                        .map_err(|_| StorageError::Corrupt("queue limit".into()))?,
                    running: running != 0,
                    created_at_ms: to_u64(created, "timestamp")?,
                })
            })
            .collect()
    }

    /// Inserts or replaces one queue's row.
    pub fn save_queue(&mut self, queue: &QueueRow) -> Result<(), StorageError> {
        self.conn
            .execute(
                "INSERT INTO queues (queue_id, name, position, max_active, running, created_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT (queue_id) DO UPDATE SET name = ?2, position = ?3, max_active = ?4,
                                                      running = ?5",
                params![
                    queue.queue_id,
                    queue.name,
                    i64::from(queue.position),
                    i64::from(queue.max_active),
                    i64::from(queue.running),
                    to_i64(queue.created_at_ms, "timestamp")?
                ],
            )
            .map(drop)
            .map_err(sql)
    }

    /// Sets or removes (`None`) a queue's schedule (CORE-08). Any catch-up state is cleared and
    /// the evaluation time is reset to `evaluated_ms`, so an edit can never turn a past occurrence
    /// into a "missed" one.
    pub fn save_queue_schedule(
        &mut self,
        queue_id: &str,
        schedule_json: Option<&str>,
        evaluated_ms: u64,
    ) -> Result<(), StorageError> {
        let changed = self
            .conn
            .execute(
                "UPDATE queues SET schedule_json = ?2, schedule_evaluated_ms = ?3,
                        catch_up_for_ms = NULL, catch_up_from_ms = NULL, catch_up_until_ms = NULL
                 WHERE queue_id = ?1",
                params![queue_id, schedule_json, to_i64(evaluated_ms, "timestamp")?],
            )
            .map_err(sql)?;
        if changed == 1 {
            Ok(())
        } else {
            Err(StorageError::Corrupt("no such queue".into()))
        }
    }

    /// Records when the schedules were last evaluated (written sparingly: at shutdown and every
    /// few minutes), for missed-window detection after a restart.
    pub fn save_schedules_evaluated(&mut self, evaluated_ms: u64) -> Result<(), StorageError> {
        self.conn
            .execute(
                "UPDATE queues SET schedule_evaluated_ms = ?1 WHERE schedule_json IS NOT NULL",
                params![to_i64(evaluated_ms, "timestamp")?],
            )
            .map(drop)
            .map_err(sql)
    }

    /// Opens a catch-up window, durably, before it takes effect (ADR 0025 §5).
    pub fn save_catch_up(
        &mut self,
        queue_id: &str,
        for_ms: u64,
        from_ms: u64,
        until_ms: u64,
    ) -> Result<(), StorageError> {
        let changed = self
            .conn
            .execute(
                "UPDATE queues SET catch_up_for_ms = ?2, catch_up_from_ms = ?3, catch_up_until_ms = ?4
                 WHERE queue_id = ?1 AND schedule_json IS NOT NULL",
                params![
                    queue_id,
                    to_i64(for_ms, "timestamp")?,
                    to_i64(from_ms, "timestamp")?,
                    to_i64(until_ms, "timestamp")?
                ],
            )
            .map_err(sql)?;
        if changed == 1 {
            Ok(())
        } else {
            Err(StorageError::Corrupt("no such scheduled queue".into()))
        }
    }

    /// Every queue's schedule and its durable evaluation state.
    pub fn load_queue_schedules(&self) -> Result<Vec<ScheduleRow>, StorageError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT queue_id, schedule_json, schedule_evaluated_ms, catch_up_for_ms,
                        catch_up_from_ms, catch_up_until_ms
                 FROM queues WHERE schedule_json IS NOT NULL ORDER BY position",
            )
            .map_err(sql)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<i64>>(2)?,
                    r.get::<_, Option<i64>>(3)?,
                    r.get::<_, Option<i64>>(4)?,
                    r.get::<_, Option<i64>>(5)?,
                ))
            })
            .map_err(sql)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql)?;
        let ms = |v: Option<i64>| v.map(|n| to_u64(n, "timestamp")).transpose();
        rows.into_iter()
            .map(|(queue_id, json, evaluated, cf, cfrom, cuntil)| {
                Ok(ScheduleRow {
                    queue_id,
                    schedule_json: json,
                    evaluated_ms: ms(evaluated)?,
                    catch_up_for_ms: ms(cf)?,
                    catch_up_from_ms: ms(cfrom)?,
                    catch_up_until_ms: ms(cuntil)?,
                })
            })
            .collect()
    }

    /// Deletes a queue. Its jobs (removed ones included) move to `move_to` in the same
    /// transaction, after the target's last position; without `move_to` a queue that still has
    /// any job is refused. The default queue is never deleted. Returns the jobs moved.
    pub fn delete_queue(
        &mut self,
        queue_id: &str,
        move_to: Option<&str>,
    ) -> Result<usize, StorageError> {
        if queue_id == DEFAULT_QUEUE || move_to == Some(queue_id) {
            return Err(StorageError::QueueNotDeletable);
        }
        let tx = self.conn.transaction().map_err(sql)?;
        let exists = |id: &str| -> Result<bool, StorageError> {
            tx.query_row(
                "SELECT 1 FROM queues WHERE queue_id = ?1",
                params![id],
                |_| Ok(()),
            )
            .optional()
            .map(|r| r.is_some())
            .map_err(sql)
        };
        if !exists(queue_id)? {
            return Err(StorageError::QueueNotDeletable);
        }
        let members: i64 = tx
            .query_row(
                "SELECT count(*) FROM jobs WHERE queue_id = ?1",
                params![queue_id],
                |r| r.get(0),
            )
            .map_err(sql)?;
        let moved = match move_to {
            None if members > 0 => return Err(StorageError::QueueNotDeletable),
            None => 0,
            Some(target) => {
                if !exists(target)? {
                    return Err(StorageError::QueueNotDeletable);
                }
                let base: i64 = tx
                    .query_row(
                        "SELECT coalesce(max(queue_position), 0) FROM jobs WHERE queue_id = ?1",
                        params![target],
                        |r| r.get(0),
                    )
                    .map_err(sql)?;
                tx.execute(
                    "UPDATE jobs SET queue_id = ?2, queue_position = ?3 + queue_position
                     WHERE queue_id = ?1",
                    params![queue_id, target, base],
                )
                .map_err(sql)?
            }
        };
        tx.execute("DELETE FROM queues WHERE queue_id = ?1", params![queue_id])
            .map_err(sql)?;
        tx.commit().map_err(sql)?;
        Ok(moved)
    }

    /// Writes several jobs' states in one transaction (reordering, moving between queues).
    pub fn update_jobs(&mut self, jobs: &[JobRow]) -> Result<(), StorageError> {
        let tx = self.conn.transaction().map_err(sql)?;
        for job in jobs {
            write_job(&tx, self.sealer.as_ref(), job, false)?;
            drop_stale_checkpoint(&tx, job)?;
            drop_abandoned_journal(&tx, job)?;
        }
        tx.commit().map_err(sql)
    }

    /// When each finished job first became completed, failed or cancelled (B2.12, ADR 0018).
    pub fn finished_times(&self) -> Result<Vec<(JobId, u64)>, StorageError> {
        let mut stmt = self
            .conn
            .prepare("SELECT job_id, finished_at_ms FROM jobs WHERE finished_at_ms IS NOT NULL")
            .map_err(sql)?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
            .map_err(sql)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql)?;
        rows.into_iter()
            .map(|(id, at)| {
                Ok((
                    JobId::parse(&id).map_err(|_| StorageError::Corrupt("job id".into()))?,
                    to_u64(at, "timestamp")?,
                ))
            })
            .collect()
    }

    /// The history retention period in days (B2.12); `None` keeps history indefinitely. Stored
    /// apart from `save_settings`, so saving destination rules never changes it.
    pub fn load_retention(&self) -> Result<Option<u32>, StorageError> {
        let value: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM settings WHERE key = 'history_retention_days'",
                [],
                |r| r.get(0),
            )
            .optional()
            .map_err(sql)?;
        value
            .map(|v| {
                v.parse::<u32>()
                    .ok()
                    .filter(|d| (1..=MAX_RETENTION_DAYS).contains(d))
                    .ok_or_else(|| StorageError::Corrupt("history retention".into()))
            })
            .transpose()
    }

    pub fn save_retention(&mut self, days: Option<u32>) -> Result<(), StorageError> {
        match days {
            None => self
                .conn
                .execute(
                    "DELETE FROM settings WHERE key = 'history_retention_days'",
                    [],
                )
                .map(drop),
            Some(d) => self
                .conn
                .execute(
                    "INSERT INTO settings (key, value) VALUES ('history_retention_days', ?1)
                     ON CONFLICT (key) DO UPDATE SET value = ?1",
                    params![d.to_string()],
                )
                .map(drop),
        }
        .map_err(sql)
    }

    /// Destination rules and the collision policy (B2.8).
    pub fn load_settings(&self) -> Result<(Vec<DestinationRule>, CollisionPolicy), StorageError> {
        let mut stmt = self
            .conn
            .prepare("SELECT category, directory FROM destination_rules ORDER BY category")
            .map_err(sql)?;
        let raw = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(sql)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql)?;
        let rules = raw
            .into_iter()
            .map(|(c, directory)| {
                Ok(DestinationRule {
                    category: FileCategory::parse(&c)
                        .ok_or_else(|| StorageError::Corrupt("category".into()))?,
                    directory,
                })
            })
            .collect::<Result<Vec<_>, StorageError>>()?;
        let policy: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM settings WHERE key = 'on_collision'",
                [],
                |r| r.get(0),
            )
            .optional()
            .map_err(sql)?;
        let policy = match policy.as_deref() {
            None | Some("rename") => CollisionPolicy::Rename,
            Some("ask") => CollisionPolicy::Ask,
            Some(_) => return Err(StorageError::Corrupt("collision policy".into())),
        };
        Ok((rules, policy))
    }

    /// Replaces the destination rules and the collision policy in one transaction.
    pub fn save_settings(
        &mut self,
        rules: &[DestinationRule],
        policy: CollisionPolicy,
    ) -> Result<(), StorageError> {
        let tx = self.conn.transaction().map_err(sql)?;
        tx.execute("DELETE FROM destination_rules", [])
            .map_err(sql)?;
        for rule in rules {
            tx.execute(
                "INSERT INTO destination_rules (category, directory) VALUES (?1, ?2)",
                params![rule.category.as_str(), rule.directory],
            )
            .map_err(sql)?;
        }
        tx.execute(
            "INSERT INTO settings (key, value) VALUES ('on_collision', ?1)
             ON CONFLICT (key) DO UPDATE SET value = ?1",
            params![match policy {
                CollisionPolicy::Rename => "rename",
                CollisionPolicy::Ask => "ask",
            }],
        )
        .map_err(sql)?;
        tx.commit().map_err(sql)
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
    /// Removes a finished job from the list (ADR 0013): marks its row, never deletes it, and never
    /// touches files. Refused with [`StorageError::NotRemovable`] unless the stored job is completed,
    /// failed or cancelled with no finalization entry. Marking an already removed job keeps its
    /// first timestamp. Returns that timestamp.
    pub fn remove_job(&mut self, job_id: &JobId, at_ms: u64) -> Result<u64, StorageError> {
        let tx = self.conn.transaction().map_err(sql)?;
        let stored: Option<(String, Option<i64>)> = tx
            .query_row(
                "SELECT phase, removed_at_ms FROM jobs WHERE job_id = ?1",
                params![job_id.as_str()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(sql)?;
        let Some((phase, removed)) = stored else {
            return Err(StorageError::NotRemovable);
        };
        if let Some(at) = removed {
            return to_u64(at, "timestamp");
        }
        let phase: Phase = from_enum(&phase, "phase")?;
        if !phase.is_terminal() || journal_attempt(&tx, job_id)?.is_some() {
            return Err(StorageError::NotRemovable);
        }
        tx.execute(
            "UPDATE jobs SET removed_at_ms = ?2 WHERE job_id = ?1",
            params![job_id.as_str(), to_i64(at_ms, "timestamp")?],
        )
        .map_err(sql)?;
        tx.commit().map_err(sql)?;
        Ok(at_ms)
    }

    /// Jobs removed from the list, with when.
    pub fn removed_jobs(&self) -> Result<Vec<(JobId, u64)>, StorageError> {
        let mut stmt = self
            .conn
            .prepare("SELECT job_id, removed_at_ms FROM jobs WHERE removed_at_ms IS NOT NULL")
            .map_err(sql)?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
            .map_err(sql)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql)?;
        rows.into_iter()
            .map(|(id, at)| {
                Ok((
                    JobId::parse(&id).map_err(|_| StorageError::Corrupt("job id".into()))?,
                    to_u64(at, "timestamp")?,
                ))
            })
            .collect()
    }

    /// Deletes tombstones removed before `before_ms` that no idempotency record still references
    /// (records are pruned first, after their own retention). Returns how many.
    pub fn purge_removed(&mut self, before_ms: u64) -> Result<usize, StorageError> {
        self.conn
            .execute(
                "DELETE FROM jobs WHERE removed_at_ms IS NOT NULL AND removed_at_ms < ?1
                   AND job_id NOT IN (SELECT job_id FROM idempotency_records)
                   AND job_id NOT IN (SELECT job_id FROM operations)",
                params![to_i64(before_ms, "timestamp")?],
            )
            .map_err(sql)
    }

    pub fn prune_idempotency(&mut self, before_ms: u64) -> Result<usize, StorageError> {
        let before = to_i64(before_ms, "timestamp")?;
        let operations = self
            .conn
            .execute(
                "DELETE FROM operations WHERE created_at_ms < ?1",
                params![before],
            )
            .map_err(sql)?;
        self.conn
            .execute(
                "DELETE FROM idempotency_records WHERE created_at_ms < ?1",
                params![before],
            )
            .map(|n| n.saturating_add(operations))
            .map_err(sql)
    }

    /// Each job's notification decision (CORE-09, ADR 0022): `sent` or `skipped`; jobs without a
    /// decision are omitted.
    pub fn notify_states(&self) -> Result<Vec<(JobId, String)>, StorageError> {
        let mut stmt = self
            .conn
            .prepare("SELECT job_id, notify_state FROM jobs WHERE notify_state IS NOT NULL")
            .map_err(sql)?;
        let raw = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(sql)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql)?;
        raw.into_iter()
            .map(|(id, state)| {
                Ok((
                    JobId::parse(&id).map_err(|_| StorageError::Corrupt("job id".into()))?,
                    state,
                ))
            })
            .collect()
    }

    /// Records the notification decision for a terminal job. Refused (nothing written) unless the
    /// stored job is terminal and has no decision yet, so a decision is made at most once per
    /// outcome. Returns whether it was recorded.
    pub fn decide_notification(&mut self, job: &JobId, state: &str) -> Result<bool, StorageError> {
        if !matches!(state, "sent" | "skipped") {
            return Err(StorageError::Corrupt("notification state".into()));
        }
        self.conn
            .execute(
                "UPDATE jobs SET notify_state = ?2
                 WHERE job_id = ?1 AND notify_state IS NULL
                   AND phase IN ('completed', 'failed', 'cancelled')",
                params![job.as_str(), state],
            )
            .map(|n| n == 1)
            .map_err(sql)
    }

    /// Missing on older profiles: keep the default global limit. Other settings never reset it.
    pub fn load_concurrency(&self) -> Result<Option<u32>, StorageError> {
        let value: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM settings WHERE key = 'max_active_downloads'",
                [],
                |r| r.get(0),
            )
            .optional()
            .map_err(sql)?;
        value
            .map(|v| {
                v.parse::<u32>()
                    .ok()
                    .filter(|n| (1..=dm_domain::api::MAX_ACTIVE_DOWNLOADS).contains(n))
                    .ok_or_else(|| StorageError::Corrupt("active download limit".into()))
            })
            .transpose()
    }

    pub fn save_concurrency(&mut self, n: u32) -> Result<(), StorageError> {
        if !(1..=dm_domain::api::MAX_ACTIVE_DOWNLOADS).contains(&n) {
            return Err(StorageError::Corrupt("active download limit".into()));
        }
        self.conn
            .execute(
                "INSERT INTO settings (key, value) VALUES ('max_active_downloads', ?1)
             ON CONFLICT (key) DO UPDATE SET value = ?1",
                params![n.to_string()],
            )
            .map(|_| ())
            .map_err(sql)
    }

    /// The global bandwidth limit (CORE-06), bytes per second; `None` = unlimited (also when absent
    /// on older profiles). Other settings never reset it.
    pub fn load_global_speed_limit(&self) -> Result<Option<u64>, StorageError> {
        let value: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM settings WHERE key = 'speed_limit_global_bps'",
                [],
                |r| r.get(0),
            )
            .optional()
            .map_err(sql)?;
        match value.as_deref() {
            None | Some("") => Ok(None),
            Some(v) => v
                .parse::<u64>()
                .ok()
                .filter(|n| {
                    (dm_domain::api::MIN_SPEED_LIMIT..=dm_domain::api::MAX_SPEED_LIMIT).contains(n)
                })
                .map(Some)
                .ok_or_else(|| StorageError::Corrupt("global speed limit".into())),
        }
    }

    /// The saved proxy setting (F06, ADR 0034); `None` before one was ever saved (direct). The
    /// row never holds the password, only whether one is in the OS credential store.
    pub fn load_proxy(&self) -> Result<Option<ProxyRow>, StorageError> {
        let value: Option<String> = self
            .conn
            .query_row("SELECT value FROM settings WHERE key = 'proxy'", [], |r| {
                r.get(0)
            })
            .optional()
            .map_err(sql)?;
        value
            .map(|v| {
                serde_json::from_str::<ProxyRow>(&v)
                    .ok()
                    .filter(ProxyRow::is_valid)
                    .ok_or_else(|| StorageError::Corrupt("proxy setting".into()))
            })
            .transpose()
    }

    pub fn save_proxy(&mut self, row: &ProxyRow) -> Result<(), StorageError> {
        if !row.is_valid() {
            return Err(StorageError::Corrupt("proxy setting".into()));
        }
        let value = serde_json::to_string(row)
            .map_err(|_| StorageError::Corrupt("proxy setting".into()))?;
        self.conn
            .execute(
                "INSERT INTO settings (key, value) VALUES ('proxy', ?1)
             ON CONFLICT (key) DO UPDATE SET value = ?1",
                params![value],
            )
            .map(|_| ())
            .map_err(sql)
    }

    /// Saves the global limit; `None` (unlimited) is stored as an empty value.
    pub fn save_global_speed_limit(&mut self, bps: Option<u64>) -> Result<(), StorageError> {
        if bps.is_some_and(|n| {
            !(dm_domain::api::MIN_SPEED_LIMIT..=dm_domain::api::MAX_SPEED_LIMIT).contains(&n)
        }) {
            return Err(StorageError::Corrupt("global speed limit".into()));
        }
        self.conn
            .execute(
                "INSERT INTO settings (key, value) VALUES ('speed_limit_global_bps', ?1)
             ON CONFLICT (key) DO UPDATE SET value = ?1",
                params![bps.map(|n| n.to_string()).unwrap_or_default()],
            )
            .map(|_| ())
            .map_err(sql)
    }

    /// Sets one job's own limit (CORE-06); `None` removes it. The job must exist.
    pub fn set_job_speed_limit(
        &mut self,
        job: &JobId,
        bps: Option<u64>,
    ) -> Result<(), StorageError> {
        let value = bps.map(|n| to_i64(n, "speed limit")).transpose()?;
        let changed = self
            .conn
            .execute(
                "UPDATE jobs SET speed_limit_bps = ?2 WHERE job_id = ?1",
                params![job.as_str(), value],
            )
            .map_err(sql)?;
        if changed == 1 {
            Ok(())
        } else {
            Err(StorageError::Corrupt("no such job".into()))
        }
    }

    /// Every job's own limit (jobs without one are absent).
    pub fn load_job_speed_limits(&self) -> Result<Vec<(JobId, u64)>, StorageError> {
        let mut stmt = self
            .conn
            .prepare("SELECT job_id, speed_limit_bps FROM jobs WHERE speed_limit_bps IS NOT NULL")
            .map_err(sql)?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
            .map_err(sql)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql)?;
        rows.into_iter()
            .map(|(id, bps)| {
                Ok((
                    JobId::parse(&id).map_err(|_| StorageError::Corrupt("job id".into()))?,
                    u64::try_from(bps).map_err(|_| StorageError::Corrupt("speed limit".into()))?,
                ))
            })
            .collect()
    }

    /// Notification settings (CORE-09): `(enabled, locale)`; off and English by default.
    pub fn load_notifications(&self) -> Result<(bool, String), StorageError> {
        let get = |key: &str| -> Result<Option<String>, StorageError> {
            self.conn
                .query_row(
                    "SELECT value FROM settings WHERE key = ?1",
                    params![key],
                    |r| r.get(0),
                )
                .optional()
                .map_err(sql)
        };
        let enabled = get("notifications_enabled")?.is_some_and(|v| v == "1");
        let locale = get("notifications_locale")?
            .filter(|l| matches!(l.as_str(), "en" | "bn"))
            .unwrap_or_else(|| "en".to_owned());
        Ok((enabled, locale))
    }

    pub fn save_notifications(&mut self, enabled: bool, locale: &str) -> Result<(), StorageError> {
        if !matches!(locale, "en" | "bn") {
            return Err(StorageError::Corrupt("notification locale".into()));
        }
        let tx = self.conn.transaction().map_err(sql)?;
        for (key, value) in [
            ("notifications_enabled", if enabled { "1" } else { "0" }),
            ("notifications_locale", locale),
        ] {
            tx.execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT (key) DO UPDATE SET value = ?2",
                params![key, value],
            )
            .map_err(sql)?;
        }
        tx.commit().map_err(sql)
    }

    /// Every stored operation (loaded once at startup).
    pub fn load_operations(&self) -> Result<Vec<OperationRow>, StorageError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT operation_id, kind, job_id, fingerprint_sha256, outcome, created_at_ms
                 FROM operations",
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
                    r.get::<_, i64>(5)?,
                ))
            })
            .map_err(sql)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql)?;
        raw.into_iter()
            .map(
                |(operation_id, kind, job_id, fingerprint_sha256, outcome, created)| {
                    Ok(OperationRow {
                        operation_id,
                        kind,
                        job_id: JobId::parse(&job_id)
                            .map_err(|_| StorageError::Corrupt("job id".into()))?,
                        fingerprint_sha256,
                        outcome,
                        created_at_ms: to_u64(created, "timestamp")?,
                    })
                },
            )
            .collect()
    }

    /// Explicit Retry or Refresh-link restart (ADR 0020): writes the reopened job (unfinished, no
    /// checkpoint), clears its finish time and records the operation, in one transaction. This is
    /// the only path that clears `finished_at_ms`; ordinary writes never do. Refused with
    /// [`StorageError::StaleAttempt`] when the stored job is removed (attempt guards are the
    /// service's, under its coordinator lock).
    pub fn reopen_job(
        &mut self,
        job: &JobRow,
        operation: &OperationRow,
    ) -> Result<(), StorageError> {
        let tx = self.conn.transaction().map_err(sql)?;
        guard_operation_target(&tx, &job.id, None)?;
        write_job(&tx, self.sealer.as_ref(), job, false)?;
        // The reopened job gets a new outcome: its finish time and notification decision restart.
        tx.execute(
            "UPDATE jobs SET finished_at_ms = NULL, notify_state = NULL WHERE job_id = ?1 AND ?2",
            params![job.id.as_str(), !job.phase.is_terminal()],
        )
        .map_err(sql)?;
        drop_stale_checkpoint(&tx, job)?;
        drop_abandoned_journal(&tx, job)?;
        insert_operation(&tx, operation)?;
        tx.commit().map_err(sql)
    }

    /// Refresh link with kept progress (ADR 0020): the job (new source, new attempt) and the
    /// checkpoint rebound to the replacement, plus the operation, in one transaction. Until this
    /// commits, the stored job keeps its old source and checkpoint.
    pub fn replace_source_kept(
        &mut self,
        job: &JobRow,
        checkpoint: &CheckpointRow,
        previous: &AttemptId,
        operation: &OperationRow,
    ) -> Result<(), StorageError> {
        if checkpoint.payload.len() > MAX_CHECKPOINT_PAYLOAD {
            return Err(StorageError::Corrupt("checkpoint (too large)".into()));
        }
        let tx = self.conn.transaction().map_err(sql)?;
        guard_operation_target(&tx, &job.id, Some(previous))?;
        if job.phase.is_terminal() || job.attempt_id.as_ref() != Some(&checkpoint.attempt_id) {
            return Err(StorageError::StaleAttempt);
        }
        write_job(&tx, self.sealer.as_ref(), job, false)?;
        upsert_checkpoint(&tx, self.sealer.as_ref(), checkpoint)?;
        insert_operation(&tx, operation)?;
        tx.commit().map_err(sql)
    }
}

fn upsert_checkpoint(
    tx: &Transaction<'_>,
    sealer: Option<&Sealer>,
    checkpoint: &CheckpointRow,
) -> Result<(), StorageError> {
    // ADR 0027: the payload holds the final URL; sealed whenever the key is available. A sealed
    // payload over the limit is refused (no durable checkpoint), never stored in the clear.
    let (payload, sealed) = match sealer {
        Some(s) => (
            s.seal(
                &format!("{}|checkpoint", checkpoint.job_id.as_str()),
                &checkpoint.payload,
            )
            .map_err(|_| StorageError::Corrupt("checkpoint could not be sealed".into()))?,
            1i64,
        ),
        None => (checkpoint.payload.clone(), 0i64),
    };
    if payload.len() > MAX_CHECKPOINT_PAYLOAD {
        return Err(StorageError::Corrupt("checkpoint (too large)".into()));
    }
    tx.execute(
        "INSERT INTO resume_checkpoints (job_id, attempt_id, backend_id, backend_version,
                                         format_version, confirmed_bytes, payload, updated_at_ms,
                                         sealed)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT (job_id) DO UPDATE SET
             attempt_id = excluded.attempt_id, backend_id = excluded.backend_id,
             backend_version = excluded.backend_version,
             format_version = excluded.format_version,
             confirmed_bytes = excluded.confirmed_bytes, payload = excluded.payload,
             updated_at_ms = excluded.updated_at_ms, sealed = excluded.sealed",
        params![
            checkpoint.job_id.as_str(),
            checkpoint.attempt_id.as_str(),
            checkpoint.backend_id,
            checkpoint.backend_version,
            checkpoint.format_version,
            to_i64(checkpoint.confirmed.0, "byte count")?,
            payload,
            to_i64(checkpoint.updated_at_ms, "timestamp")?,
            sealed,
        ],
    )
    .map(drop)
    .map_err(sql)
}

/// The stored job exists, is not removed and (when `previous` is given) is still on that attempt;
/// otherwise the operation is stale.
fn guard_operation_target(
    tx: &Transaction<'_>,
    job: &JobId,
    previous: Option<&AttemptId>,
) -> Result<(), StorageError> {
    let stored: Option<(Option<String>, Option<i64>)> = tx
        .query_row(
            "SELECT attempt_id, removed_at_ms FROM jobs WHERE job_id = ?1",
            params![job.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(sql)?;
    match stored {
        Some((attempt, None))
            if previous.is_none_or(|p| attempt.as_deref() == Some(p.as_str())) =>
        {
            Ok(())
        }
        _ => Err(StorageError::StaleAttempt),
    }
}

fn upsert_credential_in(
    conn: &rusqlite::Connection,
    c: &CredentialRow,
) -> Result<(), StorageError> {
    conn.execute(
        "INSERT INTO credentials (credential_id, origin, browser, profile_id, mode, state,
                                          generation, created_at_ms, expires_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT (credential_id) DO UPDATE SET state = excluded.state,
                     mode = excluded.mode, expires_at_ms = excluded.expires_at_ms",
        params![
            c.credential_id,
            c.origin,
            c.browser,
            c.profile_id,
            c.mode,
            c.state,
            i64::from(c.generation),
            to_i64(c.created_at_ms, "timestamp")?,
            c.expires_at_ms
                .map(|e| to_i64(e, "timestamp"))
                .transpose()?,
        ],
    )
    .map(drop)
    .map_err(sql)
}

fn insert_operation(tx: &Transaction<'_>, op: &OperationRow) -> Result<(), StorageError> {
    tx.execute(
        "INSERT INTO operations (operation_id, kind, job_id, fingerprint_sha256, outcome, created_at_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            op.operation_id,
            op.kind,
            op.job_id.as_str(),
            op.fingerprint_sha256,
            op.outcome,
            to_i64(op.created_at_ms, "timestamp")?
        ],
    )
    .map(drop)
    .map_err(sql)
}

fn write_job(
    tx: &Transaction<'_>,
    sealer: Option<&Sealer>,
    job: &JobRow,
    insert: bool,
) -> Result<(), StorageError> {
    // SEC-01 (ADR 0027): protected URL material is stored only sealed; the plain column holds the
    // display form. Without the key the protected part is not persisted at all (memory only), and
    // an update of the same link keeps the existing sealed copy: a store that is only locked now
    // must not cost the link (it comes back once the key is readable again).
    let protect = job.credential_id.is_some() || url_has_protected_parts(&job.url);
    let (stored_url, url_sealed): (String, Option<Vec<u8>>) = if protect {
        let display = if job.credential_id.is_some() {
            signed_in_display(&job.url)
        } else {
            redact_url(&job.url)
        };
        let sealed = sealer
            .map(|s| s.seal(&format!("{}|url", job.id.as_str()), job.url.as_bytes()))
            .transpose()
            .map_err(|_| StorageError::Corrupt("url could not be sealed".into()))?;
        (display, sealed)
    } else {
        (job.url.clone(), None)
    };
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
    let scan = job
        .scan
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|_| StorageError::Corrupt("scan record".into()))?;
    let values = params![
        job.id.as_str(),
        stored_url,
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
        job.queue_id,
        job.priority.as_i64(),
        job.queue_position,
        job.category.map(FileCategory::as_str),
        i64::from(job.name_confirmed),
        scan,
        job.credential_id,
        url_sealed,
    ];
    let changed = if insert {
        tx.execute(
            "INSERT INTO jobs (job_id, url, display_name, destination, desired_state, phase,
                               downloaded_bytes, total_bytes, expected_sha256, error_json,
                               created_at_ms, updated_at_ms, attempt_id, verified_sha256,
                               verification_source, staging_name, name_source, queue_id,
                               priority, queue_position, category, name_confirmed, scan_json,
                               credential_id, url_sealed)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17,
                     ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25)",
            values,
        )
    } else {
        tx.execute(
            "UPDATE jobs SET url = ?2, display_name = ?3, destination = ?4, desired_state = ?5,
                             phase = ?6, downloaded_bytes = ?7, total_bytes = ?8,
                             expected_sha256 = ?9, error_json = ?10, created_at_ms = ?11,
                             updated_at_ms = ?12, attempt_id = ?13, verified_sha256 = ?14,
                             verification_source = ?15, staging_name = ?16, name_source = ?17,
                             queue_id = ?18, priority = ?19, queue_position = ?20,
                             category = ?21, name_confirmed = ?22, scan_json = ?23,
                             credential_id = ?24,
                             url_sealed = CASE WHEN ?25 IS NULL AND url = ?2 AND credential_id IS ?24
                                               THEN url_sealed ELSE ?25 END
             WHERE job_id = ?1",
            values,
        )
    }
    .map_err(sql)?;
    if changed != 1 {
        return Err(StorageError::Sql(format!("job {} not found", job.id)));
    }
    // B2.12: the first terminal write stamps the finish time; later writes never change it.
    tx.execute(
        "UPDATE jobs SET finished_at_ms = ?2
         WHERE job_id = ?1 AND finished_at_ms IS NULL
           AND phase IN ('completed', 'failed', 'cancelled')",
        params![job.id.as_str(), to_i64(job.updated_at_ms, "timestamp")?],
    )
    .map_err(sql)?;
    Ok(())
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

fn journal_params(entry: &JournalRow) -> Result<[Box<dyn rusqlite::ToSql>; 12], StorageError> {
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
        Box::new(
            entry
                .scan_started_ms
                .map(|t| to_i64(t, "timestamp"))
                .transpose()?,
        ),
        Box::new(entry.final_file_id.clone()),
    ])
}

/// attempt, backend, backend version, format, confirmed, payload, updated (one stored row).
/// A checkpoint row as read: the payload with its "sealed" flag.
type StoredCheckpoint = (
    String,
    String,
    Option<String>,
    i64,
    i64,
    (Vec<u8>, bool),
    i64,
);
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
