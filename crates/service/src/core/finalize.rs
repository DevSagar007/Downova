//! Completion journal and safe finalization (B1.3, ADR 0008).
//!
//! A transfer the backend reports `Complete` is not yet a completed job. The service finalizes it
//! through the journal, committing each stage (with the job row) before starting the next:
//!
//! 1. **intent:** the attempt's output is recorded, bound to the job, its attempt, the staging file
//!    the backend owns and the final path;
//! 2. **verified:** the staging file's length and SHA-256 are computed; a trusted expected SHA-256
//!    must match, otherwise the job fails with `CHECKSUM_MISMATCH` and nothing is placed;
//! 3. **placed:** the verified file is placed at the final path without ever replacing anything
//!    ([`place_no_overwrite`]);
//! 4. **complete:** the job becomes `completed` with its verification evidence, in one
//!    transaction with the journal stage; then the staging name is removed and the entry deleted.
//!
//! On restart, [`Core::recover`] reconciles unfinished entries **before** ordinary checkpoint
//! recovery. Every stage re-checks the files it is about to rely on, by SHA-256, never by name or
//! length alone; a file is adopted or cleaned up only when its bytes are the verified ones.
//! Anything else (a changed staging file, a foreign file at the destination, a removed final
//! file) fails the job with an explicit reason and leaves the files as they are.
//!
//! Disk budget (B1.4, ADR 0009): a hard link needs no new data blocks; the copy fallback needs the
//! whole file again, so it is budgeted first. Too little space (or a real `ENOSPC` during the copy)
//! keeps the entry at `verified` and holds the job with the `disk_unavailable` blocker until the
//! budget fits; only the partial copy this operation created is removed.
//!
//! Origin marking (MOTW) and malware scanning are later milestones; nothing here performs them.

use std::io::ErrorKind as IoKind;
use std::path::Path;

use dm_backend_api::{BackendError, DownloadBackend, StagedOutput};
use dm_domain::api::{Verification, VerificationSource};
use dm_domain::{ByteCount, ErrorData, ErrorKind, NextAction, Phase};
use dm_storage::{JournalRow, JournalStage, Placement, StorageError, Store};

use super::{Core, Job, now_ms, trace};
use crate::naming;

/// How a verified file reached its final path.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Placed {
    /// A hard link: atomic, and it fails if the final name exists.
    Linked,
    /// Exclusive create (`create_new`, also atomic against an existing name) plus a synced copy,
    /// where the file system has no hard links.
    Copied,
}

#[derive(Debug)]
pub(super) enum PlaceError {
    /// Something already exists at the final path (possibly created at this very moment).
    Exists,
    /// The volume ran out of space (or quota) while copying.
    Full,
    Io(std::io::Error),
}

/// Why placement did not happen.
enum PlaceFailure {
    /// The job fails with this reason.
    Fatal(ErrorKind, &'static str),
    /// Not enough space: the job waits (B1.4).
    Disk(BackendError),
}

/// Places `staging` at `final_path` without ever replacing an existing file, including one another
/// process creates concurrently: both a hard link and `create_new` fail atomically when the name
/// exists. A copy that fails half-way removes only the file it created itself.
#[cfg(test)]
pub(super) fn place_no_overwrite(staging: &Path, final_path: &Path) -> Result<Placed, PlaceError> {
    if try_link(staging, final_path)? {
        return Ok(Placed::Linked);
    }
    copy_exclusive(staging, final_path)?;
    Ok(Placed::Copied)
}

/// `Ok(true)`: hard-linked. `Ok(false)`: this file system has no hard links (use the copy).
pub(super) fn try_link(staging: &Path, final_path: &Path) -> Result<bool, PlaceError> {
    match std::fs::hard_link(staging, final_path) {
        Ok(()) => {
            sync_parent(final_path);
            Ok(true)
        }
        Err(e) if e.kind() == IoKind::AlreadyExists => Err(PlaceError::Exists),
        Err(_) => Ok(false),
    }
}

/// Creates `final_path` exclusively and copies `staging` into it, synced. On failure removes the
/// file it created (and nothing else).
pub(super) fn copy_exclusive(staging: &Path, final_path: &Path) -> Result<(), PlaceError> {
    let mut out = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(final_path)
        .map_err(|e| match e.kind() {
            IoKind::AlreadyExists => PlaceError::Exists,
            _ => PlaceError::Io(e),
        })?;
    let copied = fault::storage_full("enospc_copy")
        .and_then(|()| std::fs::File::open(staging))
        .and_then(|mut src| std::io::copy(&mut src, &mut out))
        .and_then(|_| out.sync_all());
    if let Err(e) = copied {
        drop(out);
        // We created this name exclusively a moment ago: it is ours to remove.
        let _ = std::fs::remove_file(final_path);
        return Err(match e.kind() {
            IoKind::StorageFull | IoKind::QuotaExceeded => PlaceError::Full,
            _ => PlaceError::Io(e),
        });
    }
    sync_parent(final_path);
    Ok(())
}

/// Makes the new directory entry durable where the platform allows syncing a directory.
fn sync_parent(path: &Path) {
    #[cfg(unix)]
    if let Some(parent) = path.parent()
        && let Ok(dir) = std::fs::File::open(parent)
    {
        let _ = dir.sync_all();
    }
    #[cfg(not(unix))]
    let _ = path;
}

/// Length and lower-case hex SHA-256 of a file.
pub(super) async fn hash_file(path: &Path) -> std::io::Result<(u64, String)> {
    use sha2::{Digest, Sha256};
    let path = path.to_owned();
    tokio::task::spawn_blocking(move || {
        use std::io::Read;
        let mut file = std::fs::File::open(path)?;
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; 256 * 1024];
        let mut len = 0u64;
        loop {
            let n = file.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hasher.update(buf.get(..n).unwrap_or_default());
            len = len.saturating_add(n as u64);
        }
        let hex = hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        Ok((len, hex))
    })
    .await
    .map_err(|_| std::io::Error::other("hash task failed"))?
}

/// Test-only fault injection (B1.3). Compiled into debug builds only, so release packages carry
/// none of it; even then it acts only when `DOWNOVA_FAULT` names a point.
pub(super) mod fault {
    use dm_domain::JobId;

    /// `DOWNOVA_FAULT` names `<point>`: the process aborts here, like a hard kill (no destructors,
    /// no graceful shutdown). Other points: `hold_<point>`, `fail_journal_once`, `no_hard_links`
    /// (forces the copy fallback), `enospc_copy` (the first copy hits "storage full").
    pub(in crate::core) fn crash(point: &str) {
        if active(point) {
            std::process::abort();
        }
    }

    /// `DOWNOVA_FAULT` (a comma-separated list) names `point`. Always false in release builds.
    pub(in crate::core) fn active(point: &str) -> bool {
        #[cfg(debug_assertions)]
        {
            std::env::var("DOWNOVA_FAULT").is_ok_and(|v| v.split(',').any(|p| p.trim() == point))
        }
        #[cfg(not(debug_assertions))]
        {
            let _ = point;
            false
        }
    }

    /// The first time `point` is reached while active: an injected "storage full" I/O error.
    pub(in crate::core) fn storage_full(point: &str) -> std::io::Result<()> {
        #[cfg(debug_assertions)]
        {
            use std::sync::Mutex;
            static FIRED: Mutex<Vec<String>> = Mutex::new(Vec::new());
            if active(point) {
                let mut fired = FIRED
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if !fired.iter().any(|f| f == point) {
                    fired.push(point.to_owned());
                    return Err(std::io::Error::from(std::io::ErrorKind::StorageFull));
                }
            }
        }
        #[cfg(not(debug_assertions))]
        let _ = point;
        Ok(())
    }

    /// `DOWNOVA_FAULT=hold_<point>`: records a `hold` trace event, then waits (at most 60 s) for
    /// the file `<DOWNOVA_CHECKPOINT_TRACE>.release`, so a test can act at exactly this point.
    pub(in crate::core) async fn hold(point: &str, job: &JobId) {
        #[cfg(debug_assertions)]
        if active(&format!("hold_{point}"))
            && let Some(trace) = std::env::var_os("DOWNOVA_CHECKPOINT_TRACE")
        {
            let mut release = std::path::PathBuf::from(trace).into_os_string();
            release.push(".release");
            super::super::trace(&format!("hold_{point}"), job, None, 0);
            let started = std::time::Instant::now();
            while !std::path::Path::new(&release).exists()
                && started.elapsed() < std::time::Duration::from_secs(60)
            {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        }
        #[cfg(not(debug_assertions))]
        let _ = (point, job);
    }

    /// `DOWNOVA_FAULT=fail_journal_once`: the first journal write of the process fails.
    pub(in crate::core) fn journal_write_fails() -> bool {
        #[cfg(debug_assertions)]
        {
            use std::sync::atomic::{AtomicBool, Ordering};
            static FIRED: AtomicBool = AtomicBool::new(false);
            if active("fail_journal_once") && !FIRED.swap(true, Ordering::SeqCst) {
                return true;
            }
        }
        false
    }
}

fn stage_name(stage: JournalStage) -> &'static str {
    match stage {
        JournalStage::Intent => "finalize_intent",
        JournalStage::Verified => "finalize_verified",
        JournalStage::Placed => "finalize_placed",
        JournalStage::Complete => "finalize_complete",
    }
}

impl<B: DownloadBackend> Core<B> {
    /// Finalizes a job whose transfer reported `Complete` (phase `verifying`), or continues an
    /// entry whose last write failed.
    pub(super) async fn finalize(&self, store: &mut Option<Store>, job: &mut Job) {
        if let Some(entry) = job.finalizing.clone() {
            self.run_finalization(store, job, entry, false).await;
            return;
        }
        let Some((attempt_id, handle)) = job
            .attempt
            .as_ref()
            .map(|a| (a.id.clone(), a.handle.clone()))
        else {
            return;
        };
        let (placement, staging) = match self.backend.staged_output(&handle).await {
            Ok(StagedOutput::Staged(path)) => (Placement::Staged, path),
            Ok(StagedOutput::PlacedByEngine(path)) => (Placement::Engine, path),
            Err(e) => {
                self.fail_finalization(store, job, None, e.kind, &e.detail)
                    .await;
                return;
            }
        };
        // The final name is frozen with the intent at the latest (B1.6): a job that never saw
        // response metadata keeps its provisional name. An engine that wrote the destination
        // itself (aria2) wrote it under the staging name, which is then also the final one.
        if job.name_source.is_none() {
            job.name_source = Some(naming::provisional(&job.url).1);
        }
        let final_path = match placement {
            Placement::Staged => job.destination.join(&job.display_name),
            Placement::Engine => staging.clone(),
        };
        let now = now_ms();
        let entry = JournalRow {
            job_id: job.id.clone(),
            attempt_id,
            stage: JournalStage::Intent,
            placement,
            staging_path: staging,
            final_path,
            length: None,
            sha256: None,
            created_at_ms: now,
            updated_at_ms: now,
        };
        if let Some(s) = store.as_mut() {
            let written = if fault::journal_write_fails() {
                Err(StorageError::Sql("injected journal write failure".into()))
            } else {
                s.begin_finalization(&job.row(), &entry)
            };
            if let Err(e) = written {
                // Nothing has been verified or moved: the next poll starts again.
                eprintln!(
                    "dm-service: finalization of job {} not recorded: {e}",
                    job.id
                );
                trace("journal_write_failed", &job.id, Some(&entry.attempt_id), 0);
                return;
            }
        }
        job.finalizing = Some(entry.clone());
        trace(
            stage_name(JournalStage::Intent),
            &job.id,
            Some(&entry.attempt_id),
            0,
        );
        fault::crash("after_intent");
        self.run_finalization(store, job, entry, false).await;
    }

    /// Drives an entry from its recorded stage to the end. `recovering` (after a restart) makes
    /// every stage re-check the files the previous run left behind.
    pub(super) async fn run_finalization(
        &self,
        store: &mut Option<Store>,
        job: &mut Job,
        mut entry: JournalRow,
        recovering: bool,
    ) {
        loop {
            match entry.stage {
                JournalStage::Intent => {
                    let Ok((len, sha)) = hash_file(&entry.staging_path).await else {
                        self.fail_finalization(
                            store,
                            job,
                            Some(&entry),
                            ErrorKind::Internal,
                            "the finished download's file is missing or unreadable; nothing was placed",
                        )
                        .await;
                        return;
                    };
                    if job.total.is_some_and(|t| t.0 != len) {
                        self.discard_bad_output(&entry);
                        self.fail_finalization(
                            store,
                            job,
                            Some(&entry),
                            ErrorKind::InvalidRange,
                            "the finished file has the wrong length; it was not placed",
                        )
                        .await;
                        return;
                    }
                    if job.expected_sha256.as_ref().is_some_and(|e| *e != sha) {
                        self.discard_bad_output(&entry);
                        self.fail_finalization(
                            store,
                            job,
                            Some(&entry),
                            ErrorKind::ChecksumMismatch,
                            "SHA-256 does not match the expected value; the file was not placed",
                        )
                        .await;
                        return;
                    }
                    entry.stage = JournalStage::Verified;
                    entry.length = Some(ByteCount(len));
                    entry.sha256 = Some(sha);
                    if !self.commit_stage(store, job, &entry) {
                        return;
                    }
                    fault::crash("after_verify");
                    fault::hold("before_place", &job.id).await;
                }
                JournalStage::Verified => {
                    let sha = entry.sha256.clone().unwrap_or_default();
                    match self.place(&entry, &sha, recovering).await {
                        Ok(()) => {}
                        Err(PlaceFailure::Fatal(kind, detail)) => {
                            self.fail_finalization(store, job, Some(&entry), kind, detail)
                                .await;
                            return;
                        }
                        Err(PlaceFailure::Disk(e)) => {
                            self.block_finalization(store, job, &entry, &e);
                            return;
                        }
                    }
                    // The file is at the final path but the stage is not recorded yet.
                    fault::crash("after_link");
                    entry.stage = JournalStage::Placed;
                    if !self.commit_stage(store, job, &entry) {
                        return;
                    }
                    fault::crash("after_place");
                }
                JournalStage::Placed => {
                    let sha = entry.sha256.clone().unwrap_or_default();
                    if recovering {
                        match hash_file(&entry.final_path).await {
                            Ok((_, h)) if h == sha => {}
                            Err(e)
                                if e.kind() == IoKind::NotFound
                                    && entry.placement == Placement::Staged
                                    && hash_file(&entry.staging_path)
                                        .await
                                        .is_ok_and(|(_, h)| h == sha) =>
                            {
                                // The final file was removed while the service was down, but
                                // the verified bytes are still staged: place them again.
                                entry.stage = JournalStage::Verified;
                                continue;
                            }
                            _ => {
                                self.fail_finalization(
                                    store,
                                    job,
                                    Some(&entry),
                                    ErrorKind::RemoteChanged,
                                    "the placed file was changed or removed before the download was marked complete",
                                )
                                .await;
                                return;
                            }
                        }
                    }
                    let length = entry.length.unwrap_or(job.downloaded);
                    job.phase = Phase::Completed;
                    job.error = None;
                    job.downloaded = length;
                    job.total = Some(length);
                    job.verification = Some(Verification {
                        sha256: sha,
                        source: if job.expected_sha256.is_some() {
                            VerificationSource::PublisherChecksum
                        } else {
                            VerificationSource::LocalHash
                        },
                    });
                    entry.stage = JournalStage::Complete;
                    if !self.commit_stage(store, job, &entry) {
                        // Not durable: keep presenting the job as finalizing and retry.
                        job.phase = Phase::Verifying;
                        job.verification = None;
                        entry.stage = JournalStage::Placed;
                        return;
                    }
                    fault::crash("after_commit");
                }
                JournalStage::Complete => {
                    self.clean_up(store, job, &entry).await;
                    return;
                }
            }
        }
    }

    /// Stage `verified` → the file at the final path, verified.
    async fn place(
        &self,
        entry: &JournalRow,
        sha: &str,
        recovering: bool,
    ) -> Result<(), PlaceFailure> {
        let fatal = |kind, detail| Err(PlaceFailure::Fatal(kind, detail));
        if entry.placement == Placement::Engine {
            // The engine wrote the final path itself; re-check it after a restart.
            if recovering
                && !hash_file(&entry.final_path)
                    .await
                    .is_ok_and(|(_, h)| h == sha)
            {
                return fatal(
                    ErrorKind::RemoteChanged,
                    "the downloaded file changed or disappeared before it was marked complete",
                );
            }
            return Ok(());
        }
        if recovering {
            match hash_file(&entry.staging_path).await {
                Ok((_, h)) if h == sha => {}
                Ok(_) => {
                    return fatal(
                        ErrorKind::RemoteChanged,
                        "the verified file changed before it was placed; nothing was placed",
                    );
                }
                // The staging name is gone: acceptable only if exactly our bytes are already at
                // the final path (a crash after placing, before it was recorded).
                Err(_) => {
                    return if hash_file(&entry.final_path)
                        .await
                        .is_ok_and(|(_, h)| h == sha)
                    {
                        Ok(())
                    } else {
                        fatal(
                            ErrorKind::Internal,
                            "the verified file disappeared before it was placed",
                        )
                    };
                }
            }
        }
        let (staging, final_path) = (entry.staging_path.clone(), entry.final_path.clone());
        let linked = if fault::active("no_hard_links") {
            Ok(false)
        } else {
            let (s, f) = (staging.clone(), final_path.clone());
            tokio::task::spawn_blocking(move || try_link(&s, &f))
                .await
                .map_err(|_| PlaceFailure::Fatal(ErrorKind::Internal, "placement task failed"))?
        };
        let placed = match linked {
            Ok(true) => Ok(Placed::Linked),
            Ok(false) => {
                // The copy needs the whole file again: budget it first (B1.4).
                let dir = final_path.parent().unwrap_or(Path::new("."));
                let length = entry.length.map_or(0, |l| l.0);
                if let Err(e) = self.space.ensure(dir, length) {
                    return Err(PlaceFailure::Disk(e));
                }
                tokio::task::spawn_blocking(move || copy_exclusive(&staging, &final_path))
                    .await
                    .map_err(|_| PlaceFailure::Fatal(ErrorKind::Internal, "placement task failed"))?
                    .map(|()| Placed::Copied)
            }
            Err(e) => Err(e),
        };
        match placed {
            Ok(Placed::Linked) => Ok(()),
            Ok(Placed::Copied) => {
                // A copy is checked like any other byte stream.
                if hash_file(&entry.final_path)
                    .await
                    .is_ok_and(|(_, h)| h == sha)
                {
                    Ok(())
                } else {
                    let _ = std::fs::remove_file(&entry.final_path);
                    fatal(ErrorKind::Internal, "the copied file did not verify")
                }
            }
            // After a restart the existing file may be our own placement whose record was lost.
            Err(PlaceError::Exists)
                if recovering
                    && hash_file(&entry.final_path)
                        .await
                        .is_ok_and(|(_, h)| h == sha) =>
            {
                Ok(())
            }
            // Occupied by someone else's file: the destination cannot take ours (B1.7; it was
            // reported as INVALID_REQUEST before, which described the client, not the folder).
            Err(PlaceError::Exists) => fatal(
                ErrorKind::DestinationUnwritable,
                "a file already exists at the destination; Downova did not overwrite it (the verified download stays next to it as .downova-part)",
            ),
            Err(PlaceError::Full) => Err(PlaceFailure::Disk(BackendError::new(
                ErrorKind::DiskFull,
                "the destination drive filled up while the finished file was being copied into place; free some space and it is placed by itself (the verified download is kept as .downova-part)",
            ))),
            Err(PlaceError::Io(e)) if dm_backend_api::destination_refused(e.kind()).is_some() => {
                eprintln!(
                    "dm-service: the destination refused the final file: {}",
                    e.kind()
                );
                fatal(
                    ErrorKind::DestinationUnwritable,
                    dm_backend_api::destination_refused(e.kind()).unwrap_or_default(),
                )
            }
            Err(PlaceError::Io(e)) => {
                eprintln!("dm-service: placing a verified file failed: {}", e.kind());
                fatal(
                    ErrorKind::Internal,
                    "the verified file could not be placed at the destination",
                )
            }
        }
    }

    /// Not enough space to place the verified file: the entry stays at `verified` (durably, as
    /// last committed) and the job waits with the disk blocker; the poll loop retries when due.
    fn block_finalization(
        &self,
        store: &mut Option<Store>,
        job: &mut Job,
        entry: &JournalRow,
        e: &BackendError,
    ) {
        job.blockers = super::disk_blocker(e.kind).into_iter().collect();
        job.error = Some(super::disk_error(e));
        let now = tokio::time::Instant::now();
        job.disk_check_at = Some(now.checked_add(super::DISK_RECHECK_INTERVAL).unwrap_or(now));
        job.finalizing = Some(entry.clone());
        super::persist(store, job);
        trace(
            "disk_blocked",
            &job.id,
            Some(&entry.attempt_id),
            job.downloaded.0,
        );
    }

    /// Stage `complete`: removes the staging name (only if its bytes are the verified ones),
    /// forgets the backend attempt and deletes the journal entry. Idempotent.
    async fn clean_up(&self, store: &mut Option<Store>, job: &mut Job, entry: &JournalRow) {
        if entry.placement == Placement::Staged && entry.staging_path != entry.final_path {
            let sha = entry.sha256.as_deref().unwrap_or_default();
            match hash_file(&entry.staging_path).await {
                Ok((_, h)) if h == sha => {
                    let _ = std::fs::remove_file(&entry.staging_path);
                }
                // Already removed by an earlier run.
                Err(e) if e.kind() == IoKind::NotFound => {}
                // Not our verified bytes any more: leave it alone.
                _ => eprintln!(
                    "dm-service: left a changed staging file of job {} in place",
                    job.id
                ),
            }
        }
        if let Some(attempt) = job.attempt.take() {
            let _ = self.backend.remove(&attempt.handle).await;
        }
        if let Some(s) = store.as_mut()
            && let Err(e) = s.finish_finalization(&job.id)
        {
            eprintln!("dm-service: journal of job {} not cleared: {e}", job.id);
            return;
        }
        job.finalizing = None;
        job.stored_attempt = None;
        trace("finalized", &job.id, None, job.downloaded.0);
    }

    /// Commits `entry` with the job row. On failure the in-memory entry stays at its last durable
    /// stage and the next poll continues from there.
    fn commit_stage(&self, store: &mut Option<Store>, job: &mut Job, entry: &JournalRow) -> bool {
        if let Some(s) = store.as_mut() {
            let mut row = job.row();
            row.attempt_id = Some(entry.attempt_id.clone());
            if let Err(e) = s.advance_finalization(&row, entry) {
                eprintln!(
                    "dm-service: finalization of job {} not recorded: {e}",
                    job.id
                );
                trace("journal_write_failed", &job.id, Some(&entry.attempt_id), 0);
                return false;
            }
        }
        job.finalizing = Some(entry.clone());
        trace(
            stage_name(entry.stage),
            &job.id,
            Some(&entry.attempt_id),
            entry.length.map_or(0, |l| l.0),
        );
        true
    }

    /// A bad output (wrong length or checksum) is the attempt's own staging file: delete it so
    /// it is never mistaken for a good download. Engine-placed files are left (ADR 0008).
    fn discard_bad_output(&self, entry: &JournalRow) {
        if entry.placement == Placement::Staged {
            let _ = std::fs::remove_file(&entry.staging_path);
        }
    }

    /// Fails the job with an explicit reason, drops its entry and checkpoint in one transaction,
    /// and forgets the backend attempt. Files are left as they are unless the caller removed its
    /// own bad output.
    async fn fail_finalization(
        &self,
        store: &mut Option<Store>,
        job: &mut Job,
        entry: Option<&JournalRow>,
        kind: ErrorKind,
        detail: &str,
    ) {
        job.fail(ErrorData {
            kind,
            retryable: false,
            next_action: NextAction::None,
            detail: Some(detail.to_owned()),
        });
        if let Some(s) = store.as_mut() {
            let mut row = job.row();
            if let Some(entry) = entry {
                row.attempt_id = Some(entry.attempt_id.clone());
            }
            if let Err(e) = s.abandon_finalization(&row) {
                eprintln!("dm-service: failure of job {} not recorded: {e}", job.id);
            }
        }
        if let Some(attempt) = job.attempt.take() {
            let _ = self.backend.remove(&attempt.handle).await;
        }
        job.finalizing = None;
        job.stored_attempt = None;
        trace("finalize_failed", &job.id, None, 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placement_never_replaces_an_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let staging = dir.path().join("a.bin.downova-part");
        let final_path = dir.path().join("a.bin");
        std::fs::write(&staging, b"verified").unwrap();
        std::fs::write(&final_path, b"user file").unwrap();
        assert!(matches!(
            place_no_overwrite(&staging, &final_path),
            Err(PlaceError::Exists)
        ));
        assert_eq!(std::fs::read(&final_path).unwrap(), b"user file");
        std::fs::remove_file(&final_path).unwrap();
        assert_eq!(
            place_no_overwrite(&staging, &final_path).unwrap(),
            Placed::Linked
        );
        assert_eq!(std::fs::read(&final_path).unwrap(), b"verified");
        assert!(
            staging.exists(),
            "the staging name is removed only after the commit"
        );
    }

    #[test]
    fn concurrent_placements_of_one_name_never_both_succeed() {
        // Many threads race to place different files at the same name: exactly one wins and the
        // winner's bytes are intact; nobody overwrote anybody.
        let dir = tempfile::tempdir().unwrap();
        let final_path = dir.path().join("race.bin");
        let handles: Vec<_> = (0..8u8)
            .map(|i| {
                let staging = dir.path().join(format!("r{i}.downova-part"));
                std::fs::write(&staging, vec![i; 4096]).unwrap();
                let final_path = final_path.clone();
                std::thread::spawn(move || (i, place_no_overwrite(&staging, &final_path)))
            })
            .collect();
        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        let winners: Vec<u8> = results
            .iter()
            .filter(|(_, r)| r.is_ok())
            .map(|(i, _)| *i)
            .collect();
        assert_eq!(winners.len(), 1, "{results:?}");
        assert!(
            results
                .iter()
                .filter(|(_, r)| r.is_err())
                .all(|(_, r)| matches!(r, Err(PlaceError::Exists)))
        );
        assert_eq!(std::fs::read(&final_path).unwrap(), vec![winners[0]; 4096]);
    }

    #[tokio::test]
    async fn hashes_are_lower_case_sha256() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("x");
        std::fs::write(&p, b"abc").unwrap();
        assert_eq!(
            hash_file(&p).await.unwrap(),
            (
                3,
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".to_owned()
            )
        );
    }
}
