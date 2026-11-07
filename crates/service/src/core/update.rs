//! Update preparation (ADV09, ADR 0030).
//!
//! The desktop verifies an update first; only then does it ask the service to get ready. The
//! service holds a **bounded update lease** for one operation:
//!
//! 1. It stops admitting transfers; requests that would start one are refused with
//!    `SERVICE_UNAVAILABLE` until the lease ends. Reads and pauses still work.
//! 2. It stops the running transfers and commits their checkpoints **without changing any desired
//!    state**: a manual pause stays paused, a stopped queue stays stopped, and a job that was
//!    running is only *held* (it continues after the update, or when the lease ends).
//! 3. It writes a backup with `VACUUM INTO` from its own connection and checks it (integrity,
//!    schema version, job count, SHA-256).
//! 4. It records the backup's metadata and the `pending.json` marker the next start reads.
//!
//! Any failure in 2–4 releases the lease, lets the held transfers continue, removes what it wrote
//! and returns the error: nothing is installed after a failed preparation. A lease that is not
//! followed by a shutdown expires on its own.

use std::path::{Path, PathBuf};

use dm_backend_api::DownloadBackend;
use dm_domain::api::{
    CancelUpdateParams, PrepareUpdateParams, PrepareUpdateResult, UpdateBackupInfo, is_version,
};
use dm_domain::{ByteCount, DesiredState, ErrorKind, JobId, NextAction};
use serde::{Deserialize, Serialize};

use super::{Core, Pending, ServiceError, State, now_ms, persist, validate_idempotency_key};

/// How long a lease waits for the shutdown that installs the update.
pub const LEASE: std::time::Duration = std::time::Duration::from_secs(600);
/// Backups kept (the newest ones), besides the one being made.
pub const KEEP_BACKUPS: usize = 2;

/// The service's own version: the update's source.
pub const SERVICE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// An active update lease.
#[derive(Debug, Clone)]
pub(crate) struct Lease {
    pub operation_id: String,
    pub expires_at: tokio::time::Instant,
    /// Jobs whose running transfer was stopped for the update (their desired state is unchanged).
    pub held: Vec<JobId>,
    pub result: Option<PrepareUpdateResult>,
}

/// What a backup and the pending marker record (`update/backups/<op>.json`, `update/pending.json`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateRecord {
    pub operation_id: String,
    pub source_version: String,
    pub target_version: String,
    pub schema_version: u32,
    pub jobs: u64,
    pub size_bytes: u64,
    pub sha256: String,
    pub created_at_ms: u64,
    /// The backup was written, synced and checked.
    pub complete: bool,
}

pub fn backups_dir(update_dir: &Path) -> PathBuf {
    update_dir.join("backups")
}

pub fn backup_file(update_dir: &Path, op: &str) -> PathBuf {
    backups_dir(update_dir).join(format!("{op}.sqlite3"))
}

pub fn record_file(update_dir: &Path, op: &str) -> PathBuf {
    backups_dir(update_dir).join(format!("{op}.json"))
}

pub fn pending_file(update_dir: &Path) -> PathBuf {
    update_dir.join("pending.json")
}

/// Writes `value` as JSON to `path` atomically (a synced temporary file, then a rename).
pub fn write_json<T: Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    use std::io::Write;
    let bytes = serde_json::to_vec_pretty(value).map_err(std::io::Error::other)?;
    let tmp = path.with_extension("json.tmp");
    let mut f = std::fs::File::create(&tmp)?;
    f.write_all(&bytes)?;
    f.sync_all()?;
    drop(f);
    std::fs::rename(&tmp, path)
}

pub fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

fn unavailable(why: &str) -> ServiceError {
    ServiceError::new(ErrorKind::ServiceUnavailable, why).with_action(NextAction::Retry)
}

fn failed(why: &str) -> ServiceError {
    ServiceError::new(ErrorKind::Internal, why).with_action(NextAction::Retry)
}

/// Removes all but the newest `keep` complete backups (and their records), never `except`.
fn prune(update_dir: &Path, keep: usize, except: &str) {
    let Ok(entries) = std::fs::read_dir(backups_dir(update_dir)) else {
        return;
    };
    let mut records: Vec<UpdateRecord> = entries
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| read_json::<UpdateRecord>(&e.path()))
        .filter(|r| r.operation_id != except)
        .collect();
    records.sort_by_key(|r| std::cmp::Reverse(r.created_at_ms));
    for old in records.into_iter().skip(keep) {
        let _ = std::fs::remove_file(backup_file(update_dir, &old.operation_id));
        let _ = std::fs::remove_file(record_file(update_dir, &old.operation_id));
    }
}

impl<B: DownloadBackend> Core<B> {
    /// Installs the folder update records and backups go to (`<data>/update`).
    pub fn with_update_dir(mut self, dir: PathBuf) -> Self {
        self.update_dir = Some(dir);
        self
    }

    fn lease(&self) -> std::sync::MutexGuard<'_, Option<Lease>> {
        self.update
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// An update is being prepared: nothing new starts.
    pub(crate) fn update_held(&self) -> bool {
        self.lease().is_some()
    }

    /// Refuses requests that would start a transfer while an update is being prepared.
    pub(crate) fn refuse_while_updating(&self) -> Result<(), ServiceError> {
        if self.update_held() {
            Err(unavailable(
                "an update is being prepared; try again once it is installed or cancelled",
            ))
        } else {
            Ok(())
        }
    }

    /// `service.v1.prepare_update`.
    pub async fn prepare_update(
        &self,
        params: PrepareUpdateParams,
    ) -> Result<PrepareUpdateResult, ServiceError> {
        validate_idempotency_key(&params.operation_id)?;
        if !is_version(&params.target_version) {
            return Err(ServiceError::new(
                ErrorKind::InvalidRequest,
                "target_version must look like 1.2.3",
            ));
        }
        let Some(update_dir) = self.update_dir.clone() else {
            return Err(unavailable("updates need the service's data folder"));
        };
        if self.is_stopping() {
            return Err(unavailable("the service is stopping"));
        }
        {
            let mut lease = self.lease();
            match lease.as_ref() {
                // A repeat of the same preparation answers with what it produced.
                Some(l) if l.operation_id == params.operation_id => {
                    return l
                        .result
                        .clone()
                        .ok_or_else(|| unavailable("this update is still being prepared"));
                }
                Some(_) => {
                    return Err(ServiceError::new(
                        ErrorKind::InvalidRequest,
                        "another update is being prepared",
                    ));
                }
                None => {}
            }
            *lease = Some(Lease {
                operation_id: params.operation_id.clone(),
                expires_at: {
                    let now = tokio::time::Instant::now();
                    now.checked_add(LEASE).unwrap_or(now)
                },
                held: Vec::new(),
                result: None,
            });
        }
        match self.prepare_inner(&params, &update_dir).await {
            Ok(result) => {
                if let Some(l) = self.lease().as_mut() {
                    l.result = Some(result.clone());
                }
                Ok(result)
            }
            Err(e) => {
                let _ = std::fs::remove_file(backup_file(&update_dir, &params.operation_id));
                let _ = std::fs::remove_file(record_file(&update_dir, &params.operation_id));
                let _ = std::fs::remove_file(pending_file(&update_dir));
                self.release_update().await;
                Err(e)
            }
        }
    }

    async fn prepare_inner(
        &self,
        params: &PrepareUpdateParams,
        update_dir: &Path,
    ) -> Result<PrepareUpdateResult, ServiceError> {
        std::fs::create_dir_all(backups_dir(update_dir))
            .map_err(|_| failed("the update folder could not be created"))?;
        let mut guard = self.state.lock().await;
        let State { jobs, store, .. } = &mut *guard;
        // 2. Stop running transfers and commit their checkpoints; desired states stay as they are.
        let mut held = Vec::new();
        for job in jobs.iter_mut() {
            if job.phase.is_terminal() || job.desired_state != DesiredState::Running {
                continue;
            }
            let Some(handle) = job.attempt.as_ref().map(|a| a.handle.clone()) else {
                continue;
            };
            self.backend
                .pause(&handle)
                .await
                .map_err(|_| failed("a transfer could not be stopped for the update"))?;
            // Like a user pause: report the bytes the engine confirmed (flushed) when it stopped,
            // not the last poll, so the held state matches the partial file.
            if let Ok(confirmed) = self.backend.progress(&handle).await {
                job.downloaded = confirmed.completed;
            }
            held.push(job.id.clone());
            if let Some(l) = self.lease().as_mut() {
                l.held.push(job.id.clone());
            }
            match self.backend.checkpoint(&handle).await {
                Ok(_) => {
                    self.take_checkpoint(store, job).await;
                }
                Err(_) => {
                    return Err(failed(
                        "a transfer's progress could not be saved, so the update was not prepared",
                    ));
                }
            }
            persist(store, job);
        }
        // 3. The backup, from this connection.
        let Some(s) = store.as_ref() else {
            return Err(unavailable("the service has no database to back up"));
        };
        let file = backup_file(update_dir, &params.operation_id);
        let check = s.backup_to(&file).map_err(|e| {
            eprintln!("dm-service: update backup failed: {e}");
            failed("the backup could not be written or did not pass its check")
        })?;
        drop(guard);
        #[cfg(debug_assertions)]
        if std::env::var_os("DOWNOVA_TEST_CORRUPT_BACKUP").is_some_and(|v| v == "1") {
            // Process tests: damage the copy after it was written, before it is re-checked.
            let _ = std::fs::write(&file, b"not a database");
        }
        // Checked again from the file itself, as the next start will.
        let again = dm_storage::backup::check_backup(&file)
            .map_err(|_| failed("the backup did not pass its check"))?;
        if again != check {
            return Err(failed("the backup changed after it was written"));
        }
        let now = now_ms();
        let record = UpdateRecord {
            operation_id: params.operation_id.clone(),
            source_version: SERVICE_VERSION.to_owned(),
            target_version: params.target_version.clone(),
            schema_version: check.schema_version,
            jobs: check.jobs,
            size_bytes: check.size_bytes,
            sha256: check.sha256.clone(),
            created_at_ms: now,
            complete: true,
        };
        // 4. The record, then the marker the next start reads.
        write_json(&record_file(update_dir, &params.operation_id), &record)
            .and_then(|()| write_json(&pending_file(update_dir), &record))
            .map_err(|_| failed("the update record could not be written"))?;
        prune(update_dir, KEEP_BACKUPS, &params.operation_id);
        Ok(PrepareUpdateResult {
            operation_id: params.operation_id.clone(),
            source_version: SERVICE_VERSION.to_owned(),
            target_version: params.target_version.clone(),
            backup: UpdateBackupInfo {
                schema_version: check.schema_version,
                jobs: ByteCount(check.jobs),
                size_bytes: ByteCount(check.size_bytes),
                sha256: check.sha256,
                created_at_ms: ByteCount(now),
            },
            held_jobs: u32::try_from(held.len()).unwrap_or(u32::MAX),
            lease_expires_at_ms: ByteCount(
                now.saturating_add(u64::try_from(LEASE.as_millis()).unwrap_or(u64::MAX)),
            ),
        })
    }

    /// `service.v1.cancel_update`: the lease ends, the pending marker goes, held transfers continue.
    pub async fn cancel_update(&self, params: CancelUpdateParams) -> Result<(), ServiceError> {
        let current = self.lease().as_ref().map(|l| l.operation_id.clone());
        match current {
            Some(op) if op == params.operation_id => {
                if let Some(dir) = &self.update_dir {
                    let _ = std::fs::remove_file(pending_file(dir));
                }
                self.release_update().await;
                Ok(())
            }
            Some(_) => Err(ServiceError::new(
                ErrorKind::InvalidRequest,
                "another update is being prepared",
            )),
            // Already ended (expired or cancelled): nothing to do.
            None => Ok(()),
        }
    }

    /// Ends the lease and lets the held transfers continue through admission.
    pub(crate) async fn release_update(&self) {
        let Some(lease) = self.lease().take() else {
            return;
        };
        let mut guard = self.state.lock().await;
        let State {
            jobs,
            store,
            queues,
            ..
        } = &mut *guard;
        for job in jobs.iter_mut().filter(|j| lease.held.contains(&j.id)) {
            if job.desired_state == DesiredState::Running
                && !job.phase.is_terminal()
                && job.attempt.is_some()
            {
                job.queue(Pending::Resume);
            }
        }
        self.admit(jobs, queues, store).await;
    }

    /// Ends an expired lease (poll loop): the update was not installed in time.
    pub(crate) async fn expire_update(&self) {
        let expired = self
            .lease()
            .as_ref()
            .is_some_and(|l| l.expires_at <= tokio::time::Instant::now());
        if expired {
            if let Some(dir) = &self.update_dir {
                let _ = std::fs::remove_file(pending_file(dir));
            }
            self.release_update().await;
        }
    }
}

/// What the last update did, as the next start found it (`update/last.json`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateOutcome {
    pub operation_id: String,
    /// `installed`, `not_installed` or `restored`.
    pub outcome: String,
    pub from_version: String,
    pub to_version: String,
    pub at_ms: u64,
}

/// Why the service could not start its database (`update/recovery.json`). Versions and kinds only:
/// never a path, a URL or a storage error text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryRecord {
    /// `migration_failed`, `newer_schema` or `storage_unavailable`.
    pub kind: String,
    pub service_version: String,
    pub supported_schema: u32,
    pub found_schema: Option<u32>,
    /// The update that was being installed, when there was one.
    pub operation_id: Option<String>,
    /// The newest complete backup this version can open, offered for `--restore-update`.
    pub backup_operation: Option<String>,
    pub at_ms: u64,
}

pub fn last_file(update_dir: &Path) -> PathBuf {
    update_dir.join("last.json")
}

pub fn recovery_file(update_dir: &Path) -> PathBuf {
    update_dir.join("recovery.json")
}

/// After a successful start: closes the pending marker as `installed` (this version is its
/// target) or `not_installed` (the old version came back, e.g. the installer was cancelled), and
/// clears a recovery record. Returns the outcome written, if any.
pub fn accept_pending(update_dir: &Path, version: &str, now: u64) -> Option<UpdateOutcome> {
    let _ = std::fs::remove_file(recovery_file(update_dir));
    let pending: UpdateRecord = read_json(&pending_file(update_dir))?;
    let outcome = UpdateOutcome {
        operation_id: pending.operation_id.clone(),
        outcome: if version == pending.target_version {
            "installed"
        } else {
            "not_installed"
        }
        .into(),
        from_version: pending.source_version.clone(),
        to_version: version.to_owned(),
        at_ms: now,
    };
    if write_json(&last_file(update_dir), &outcome).is_ok() {
        let _ = std::fs::remove_file(pending_file(update_dir));
    }
    Some(outcome)
}

/// The newest complete backup whose file still matches its record and whose schema this version
/// can open.
pub fn usable_backup(update_dir: &Path, supported_schema: u32) -> Option<UpdateRecord> {
    let mut records: Vec<UpdateRecord> = std::fs::read_dir(backups_dir(update_dir))
        .ok()?
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| read_json::<UpdateRecord>(&e.path()))
        .filter(|r| r.complete && r.schema_version <= supported_schema)
        .collect();
    records.sort_by_key(|r| std::cmp::Reverse(r.created_at_ms));
    records.into_iter().find(|r| {
        dm_storage::backup::check_backup(&backup_file(update_dir, &r.operation_id))
            .is_ok_and(|c| c.sha256 == r.sha256)
    })
}

/// Records why the database could not be opened, with the backup that could replace it.
pub fn record_recovery(
    update_dir: &Path,
    error: &dm_storage::StorageError,
    supported_schema: u32,
    now: u64,
) -> RecoveryRecord {
    use dm_storage::StorageError as E;
    let (kind, found) = match error {
        E::Migration { .. } => ("migration_failed", None),
        E::NewerSchema { found, .. } => ("newer_schema", Some(*found)),
        _ => ("storage_unavailable", None),
    };
    let pending: Option<UpdateRecord> = read_json(&pending_file(update_dir));
    let record = RecoveryRecord {
        kind: kind.into(),
        service_version: SERVICE_VERSION.into(),
        supported_schema,
        found_schema: found,
        operation_id: pending.map(|p| p.operation_id),
        backup_operation: usable_backup(update_dir, supported_schema).map(|r| r.operation_id),
        at_ms: now,
    };
    let _ = std::fs::create_dir_all(update_dir);
    let _ = write_json(&recovery_file(update_dir), &record);
    record
}

/// `dm-service --restore-update <operation>`: replaces the database with that update's backup,
/// only when the backup still matches its record, passes the integrity check and has a schema this
/// version can open. The replaced files are set aside under `update/set-aside/`, never deleted.
/// The caller holds the instance lock and has no store open.
pub fn restore(
    update_dir: &Path,
    db: &Path,
    operation_id: &str,
    supported_schema: u32,
    now: u64,
) -> Result<UpdateOutcome, String> {
    validate_idempotency_key(operation_id).map_err(|_| "not an update operation id".to_owned())?;
    let record: UpdateRecord = read_json(&record_file(update_dir, operation_id))
        .ok_or("no complete backup is recorded for this update")?;
    if !record.complete || record.operation_id != operation_id {
        return Err("the backup for this update is not complete".into());
    }
    if record.schema_version > supported_schema {
        return Err("the backup was made by a newer Downova than this one".into());
    }
    let aside = update_dir
        .join("set-aside")
        .join(format!("{operation_id}-{now}"));
    dm_storage::backup::restore_backup(
        db,
        &backup_file(update_dir, operation_id),
        &record.sha256,
        &aside,
    )
    .map_err(|e| format!("the backup could not be restored: {e}"))?;
    let outcome = UpdateOutcome {
        operation_id: operation_id.to_owned(),
        outcome: "restored".into(),
        from_version: SERVICE_VERSION.into(),
        to_version: record.source_version.clone(),
        at_ms: now,
    };
    let _ = write_json(&last_file(update_dir), &outcome);
    let _ = std::fs::remove_file(recovery_file(update_dir));
    let _ = std::fs::remove_file(pending_file(update_dir));
    Ok(outcome)
}
