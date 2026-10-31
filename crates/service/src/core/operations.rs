//! Explicit job operations (B2.16–B2.19, ADR 0020): Retry, Refresh link and Locate.
//!
//! **Operation identity.** Retry and Refresh link carry a client-chosen `operation_id`. The first
//! request that changes the job records it (kind, job, request fingerprint, outcome) in the same
//! store transaction as the change; a repeat with the same id answers with the recorded outcome and
//! never starts another attempt, also after a lost acknowledgement or a service restart. The same
//! id with a different request is an `IDEMPOTENCY_CONFLICT`. A refusal or a `choice_required`
//! answer records nothing (nothing changed).
//!
//! **Retry** reopens a *failed* job with a fresh attempt. A failed job never keeps a resume point
//! (its checkpoint is dropped at the terminal write), so Retry always starts from zero; when the
//! job holds partial data the client must confirm discarding it (`discard_partial`). The old
//! attempt's handle and partial output are released before the reopen is committed; progress of
//! the old attempt is dropped by the attempt guard. Queue, priority, destination, names and the
//! trusted checksum are kept. The reopen clears the finish time (ADR 0018 amended), so retention
//! counts from the new outcome.
//!
//! **Refresh link** replaces the source of a paused or failed job. Progress is kept only when the
//! backend proves the replacement serves the same content *and* the resource is the same (origin
//! and path) or a trusted SHA-256 will verify the result. Otherwise nothing changes and the answer
//! is `choice_required`. Restart is explicit. Until a decision is committed the job keeps its old
//! source, partial data and checkpoint.
//!
//! **Locate** resolves a job's bound destination for a user-triggered "Open containing folder".

use std::path::Path;

use dm_backend_api::{DownloadBackend, DurableCheckpoint, ReplacementCheck, require_capability};
use dm_domain::api::{
    DownloadSnapshot, LocateResult, ReplaceMode, ReplaceOutcome, ReplaceReason,
    ReplaceSourceParams, ReplaceSourceResult, RetryParams,
};
use dm_domain::{
    AttemptId, Blocker, ByteCount, Capability, DesiredState, ErrorKind, JobId, NextAction, Phase,
};
use dm_storage::{CheckpointRow, OperationRow, Store};

use super::finalize::fault::crash;
use super::{
    Attempt, Core, Job, Pending, ServiceError, State, find_job, new_attempt_id, now_ms, persist,
    queue_running, recovery_error, stopping, trace, url_scheme, validate_idempotency_key,
};

pub(super) const RETRY: &str = "retry";
pub(super) const REPLACE_SOURCE: &str = "replace_source";
const RETRIED: &str = "retried";

fn fingerprint(kind: &str, job: &JobId, fields: &serde_json::Value) -> String {
    use sha2::{Digest, Sha256};
    let canonical = serde_json::json!([kind, job.as_str(), fields]).to_string();
    Sha256::digest(canonical.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn outcome_code(outcome: ReplaceOutcome) -> &'static str {
    match outcome {
        ReplaceOutcome::ProgressKept => "progress_kept",
        ReplaceOutcome::Restarted => "restarted",
        ReplaceOutcome::ChoiceRequired => "choice_required",
    }
}

fn parse_outcome(code: &str) -> ReplaceOutcome {
    match code {
        "progress_kept" => ReplaceOutcome::ProgressKept,
        _ => ReplaceOutcome::Restarted,
    }
}

fn changed() -> ServiceError {
    ServiceError::new(
        ErrorKind::InvalidRequest,
        "the download changed since it was shown; look at it again before repeating the action",
    )
}

fn not_recorded() -> ServiceError {
    ServiceError::new(
        ErrorKind::Internal,
        "the change could not be recorded, so it was not applied",
    )
    .with_action(NextAction::Retry)
}

/// The attempt a client could have seen for `job` (its snapshot's `attempt_id`).
fn seen_attempt(job: &Job) -> Option<AttemptId> {
    job.attempt.as_ref().map(|a| a.id.clone())
}

/// What may be reopened at all: listed, not completed or cancelled, not finalizing.
fn ensure_reopenable(job: &Job, expected: Option<&AttemptId>) -> Result<(), ServiceError> {
    if job.removed_at_ms.is_some() {
        return Err(ServiceError::new(
            ErrorKind::InvalidRequest,
            "this download was removed from the list; add the link again",
        ));
    }
    match job.phase {
        Phase::Completed => {
            return Err(ServiceError::new(
                ErrorKind::InvalidRequest,
                "the download already completed",
            ));
        }
        Phase::Cancelled => {
            return Err(ServiceError::new(
                ErrorKind::InvalidRequest,
                "a cancelled download is not reopened; add the link again",
            ));
        }
        _ => {}
    }
    if job.finalizing.is_some() || job.recover_journal {
        return Err(ServiceError::new(
            ErrorKind::InvalidRequest,
            "the download is being completed; wait until it finishes",
        )
        .with_action(NextAction::Wait));
    }
    if let Some(expected) = expected
        && seen_attempt(job).as_ref() != Some(expected)
    {
        return Err(changed());
    }
    Ok(())
}

impl<B: DownloadBackend> Core<B> {
    fn replay(
        state: &mut State,
        operation_id: &str,
        kind: &str,
        fingerprint: &str,
    ) -> Result<Option<(String, JobId)>, ServiceError> {
        match state.operations.get(operation_id) {
            None => Ok(None),
            Some(op) if op.kind != kind || op.fingerprint_sha256 != fingerprint => {
                Err(ServiceError::new(
                    ErrorKind::IdempotencyConflict,
                    "this operation id was already used for a different request",
                ))
            }
            Some(op) => Ok(Some((op.outcome.clone(), op.job_id.clone()))),
        }
    }

    /// Releases the job's attempt and its partial output (a live handle is discarded; a leftover
    /// of an earlier service run is released by name). Nothing is recorded yet.
    async fn release_partial(&self, job: &mut Job) -> Result<(), ServiceError> {
        let result = match job.attempt.as_ref() {
            Some(attempt) => self.backend.discard(&attempt.handle).await,
            None => {
                let request = self.request_for(job, new_attempt_id());
                self.backend.discard_output(&request).await
            }
        };
        result.map_err(|e| {
            let refusal = ServiceError::from(e);
            if refusal.0.kind == ErrorKind::BackendCapabilityUnavailable {
                refusal
            } else {
                ServiceError::new(
                    refusal.0.kind,
                    "the old partial data could not be released, so nothing was changed",
                )
                .with_action(NextAction::Retry)
            }
        })?;
        job.attempt = None;
        Ok(())
    }

    /// Commits a reopen (Retry or Refresh-link restart) with its operation, then applies it in
    /// memory: unfinished, no attempt, nothing downloaded, finish time cleared.
    fn commit_reopen(
        store: &mut Option<Store>,
        job: &mut Job,
        url: Option<&str>,
        run: bool,
        op: &OperationRow,
    ) -> Result<(), ServiceError> {
        let mut row = job.row();
        if let Some(url) = url {
            url.clone_into(&mut row.url);
        }
        row.attempt_id = None;
        row.phase = Phase::Queued;
        row.error = None;
        row.verification = None;
        row.downloaded = ByteCount(0);
        if run {
            row.desired_state = DesiredState::Running;
        }
        if let Some(s) = store.as_mut() {
            s.reopen_job(&row, op).map_err(|e| {
                eprintln!("dm-service: reopen of job {} not recorded: {e}", job.id);
                not_recorded()
            })?;
        }
        if let Some(url) = url {
            url.clone_into(&mut job.url);
        }
        job.attempt = None;
        job.stored_attempt = None;
        job.recover_from = None;
        job.error = None;
        job.verification = None;
        job.downloaded = ByteCount(0);
        job.durable = 0;
        job.retries = 0;
        job.retry_at = None;
        job.checkpointed_at = None;
        job.disk_failure = None;
        job.disk_check_at = None;
        job.finished_at_ms = None;
        job.pending = None;
        job.phase = Phase::Queued;
        job.blockers
            .retain(|b| matches!(b, Blocker::NameConflict | Blocker::QueueStopped));
        if run {
            job.desired_state = DesiredState::Running;
        }
        if job.desired_state == DesiredState::Running {
            job.queue(Pending::Fresh);
        }
        Ok(())
    }

    /// `downloads.v1.retry` (B2.16).
    pub async fn retry(&self, params: RetryParams) -> Result<DownloadSnapshot, ServiceError> {
        validate_idempotency_key(&params.operation_id)?;
        let fp = fingerprint(
            RETRY,
            &params.job_id,
            &serde_json::json!({ "discard_partial": params.discard_partial }),
        );
        let mut guard = self.state.lock().await;
        if let Some((_, id)) = Self::replay(&mut guard, &params.operation_id, RETRY, &fp)? {
            return Ok(guard.job_mut(&id)?.snapshot());
        }
        if self.is_stopping() {
            return Err(stopping());
        }
        let State {
            jobs,
            store,
            queues,
            operations,
            ..
        } = &mut *guard;
        let job = find_job(jobs, &params.job_id)?;
        ensure_reopenable(job, params.expected_attempt_id.as_ref())?;
        if job.phase != Phase::Failed {
            return Err(ServiceError::new(
                ErrorKind::InvalidRequest,
                "only a failed download can be retried",
            ));
        }
        match job.error.as_ref().map(|e| e.kind) {
            Some(ErrorKind::LinkRefreshRequired) => {
                return Err(ServiceError::new(
                    ErrorKind::InvalidRequest,
                    "the link no longer works; refresh it with a new link instead of retrying",
                )
                .with_action(NextAction::RefreshLink));
            }
            Some(ErrorKind::AuthRequired) => {
                return Err(ServiceError::new(
                    ErrorKind::InvalidRequest,
                    "the server asks for a sign-in, which Downova does not support yet",
                )
                .with_action(NextAction::SignIn));
            }
            _ => {}
        }
        let required = if url_scheme(&job.url)? == "https" {
            Capability::Https
        } else {
            Capability::Http
        };
        require_capability(&self.capabilities, required)?;
        if job.downloaded.0 > 0 && !params.discard_partial {
            return Err(ServiceError::new(
                ErrorKind::InvalidRequest,
                "Retry starts again from the beginning and discards the partial data; confirm to continue",
            )
            .with_action(NextAction::ChooseRestartOrKeep));
        }
        self.release_partial(job).await?;
        // Process tests: a hard kill after the release, before the reopen is recorded.
        crash("after_retry_release");
        let op = OperationRow {
            operation_id: params.operation_id.clone(),
            kind: RETRY.to_owned(),
            job_id: job.id.clone(),
            fingerprint_sha256: fp,
            outcome: RETRIED.to_owned(),
            created_at_ms: now_ms(),
        };
        Self::commit_reopen(store, job, None, true, &op)?;
        job.sync_queue_hold(queue_running(queues, &job.queue_id));
        trace("retried", &job.id, None, 0);
        operations.insert(op.operation_id.clone(), op);
        let id = job.id.clone();
        self.admit(jobs, queues, store).await;
        let snapshot = find_job(jobs, &id)?.snapshot();
        Ok(snapshot)
    }

    /// `downloads.v1.replace_source` (B2.17).
    pub async fn replace_source(
        &self,
        params: ReplaceSourceParams,
    ) -> Result<ReplaceSourceResult, ServiceError> {
        validate_idempotency_key(&params.operation_id)?;
        let scheme = url_scheme(&params.url)?;
        let fp = fingerprint(
            REPLACE_SOURCE,
            &params.job_id,
            &serde_json::json!({ "url": params.url, "mode": params.mode }),
        );
        let mut guard = self.state.lock().await;
        if let Some((outcome, id)) =
            Self::replay(&mut guard, &params.operation_id, REPLACE_SOURCE, &fp)?
        {
            return Ok(ReplaceSourceResult {
                outcome: parse_outcome(&outcome),
                reason: None,
                job: guard.job_mut(&id)?.snapshot(),
            });
        }
        if self.is_stopping() {
            return Err(stopping());
        }
        require_capability(
            &self.capabilities,
            if scheme == "https" {
                Capability::Https
            } else {
                Capability::Http
            },
        )?;
        let State {
            jobs,
            store,
            queues,
            operations,
            ..
        } = &mut *guard;
        let job = find_job(jobs, &params.job_id)?;
        ensure_reopenable(job, params.expected_attempt_id.as_ref())?;
        let stopped = job.phase == Phase::Failed || job.desired_state == DesiredState::Paused;
        if !stopped || job.is_disk_blocked() {
            return Err(ServiceError::new(
                ErrorKind::InvalidRequest,
                "pause the download before replacing its link",
            ));
        }
        let mut op = OperationRow {
            operation_id: params.operation_id.clone(),
            kind: REPLACE_SOURCE.to_owned(),
            job_id: job.id.clone(),
            fingerprint_sha256: fp,
            outcome: String::new(),
            created_at_ms: now_ms(),
        };
        let (outcome, reason) = match params.mode {
            ReplaceMode::Restart => {
                self.release_partial(job).await?;
                crash("after_replace_release");
                op.outcome = outcome_code(ReplaceOutcome::Restarted).to_owned();
                // A failed job runs again; a paused one stays paused (it starts from zero on
                // Resume).
                let run = job.phase == Phase::Failed;
                Self::commit_reopen(store, job, Some(&params.url), run, &op)?;
                job.sync_queue_hold(queue_running(queues, &job.queue_id));
                trace("source_replaced_restart", &job.id, None, 0);
                (ReplaceOutcome::Restarted, None)
            }
            ReplaceMode::KeepIfVerified => {
                match self.keep_progress(store, job, &params.url, &mut op).await? {
                    None => (ReplaceOutcome::ProgressKept, None),
                    Some(reason) => (ReplaceOutcome::ChoiceRequired, Some(reason)),
                }
            }
        };
        if outcome != ReplaceOutcome::ChoiceRequired {
            operations.insert(op.operation_id.clone(), op);
        }
        let id = job.id.clone();
        self.admit(jobs, queues, store).await;
        let snapshot = find_job(jobs, &id)?.snapshot();
        Ok(ReplaceSourceResult {
            outcome,
            reason,
            job: snapshot,
        })
    }

    /// The keep-progress path: `Ok(None)` when the job now continues from the replacement,
    /// `Ok(Some(reason))` when nothing changed and the user must choose.
    async fn keep_progress(
        &self,
        store: &mut Option<Store>,
        job: &mut Job,
        url: &str,
        op: &mut OperationRow,
    ) -> Result<Option<ReplaceReason>, ServiceError> {
        let Some(old) = job
            .attempt
            .as_ref()
            .map(|a| (a.id.clone(), a.handle.clone()))
        else {
            // A failed job (or one whose attempt ended before this service run) has no resume
            // point to keep.
            return Ok(Some(ReplaceReason::NoCheckpoint));
        };
        let cp: DurableCheckpoint = match self.backend.checkpoint(&old.1).await {
            Ok(Some(cp)) if cp.confirmed.0 > 0 => cp,
            _ => return Ok(Some(ReplaceReason::NoCheckpoint)),
        };
        let new_id = new_attempt_id();
        let mut request = self.request_for(job, new_id.clone());
        url.clone_into(&mut request.url);
        let checkpoint = match self.backend.verify_replacement(&request, &cp).await {
            Ok(ReplacementCheck::Compatible {
                checkpoint,
                same_resource,
            }) => {
                if !same_resource && job.expected_sha256.is_none() {
                    return Ok(Some(ReplaceReason::DifferentResource));
                }
                checkpoint
            }
            Ok(ReplacementCheck::Incompatible(reason)) => return Ok(Some(reason)),
            Err(_) => return Ok(Some(ReplaceReason::Unverifiable)),
        };
        // Proven: move the job to a new attempt restored from the rebound checkpoint. The old
        // attempt is released first (its files stay); if the new one cannot start, the old
        // checkpoint is restored so the job keeps its old source.
        self.backend.remove(&old.1).await?;
        job.attempt = None;
        let handle = match self.backend.restore(request, &checkpoint).await {
            Ok(handle) => handle,
            Err(e) => {
                self.reattach(store, job, &cp, &old.0).await;
                eprintln!(
                    "dm-service: replacement of job {} not resumed: {}",
                    job.id,
                    e.kind.wire_name()
                );
                return Ok(Some(ReplaceReason::Unverifiable));
            }
        };
        job.attempt = Some(Attempt {
            id: new_id.clone(),
            handle,
        });
        crash("after_replace_restore");
        let previous_url = std::mem::replace(&mut job.url, url.to_owned());
        op.outcome = outcome_code(ReplaceOutcome::ProgressKept).to_owned();
        let row = CheckpointRow {
            job_id: job.id.clone(),
            attempt_id: new_id.clone(),
            backend_id: checkpoint.backend.clone(),
            backend_version: self.backend_version.clone(),
            format_version: checkpoint.format_version,
            confirmed: checkpoint.confirmed,
            payload: checkpoint.payload.clone(),
            updated_at_ms: now_ms(),
        };
        if let Some(s) = store.as_mut()
            && let Err(e) = s.replace_source_kept(&job.row(), &row, &old.0, op)
        {
            eprintln!(
                "dm-service: replacement of job {} not recorded: {e}",
                job.id
            );
            // Undo: back to the old source and its checkpoint, still paused.
            if let Some(a) = job.attempt.take() {
                let _ = self.backend.remove(&a.handle).await;
            }
            job.url = previous_url;
            self.reattach(store, job, &cp, &old.0).await;
            return Err(not_recorded());
        }
        job.durable = checkpoint.confirmed.0;
        job.downloaded = checkpoint.confirmed;
        job.error = None;
        if job.phase == Phase::Failed {
            job.phase = Phase::Queued;
        }
        trace(
            "source_replaced_kept",
            &job.id,
            Some(&new_id),
            checkpoint.confirmed.0,
        );
        Ok(None)
    }

    /// Restores the job's own (old) checkpoint under a fresh attempt after an aborted
    /// replacement, rebinding the stored checkpoint to it. A failure ends the job explicitly.
    async fn reattach(
        &self,
        store: &mut Option<Store>,
        job: &mut Job,
        cp: &DurableCheckpoint,
        previous: &AttemptId,
    ) {
        let id = new_attempt_id();
        match self
            .backend
            .restore(self.request_for(job, id.clone()), cp)
            .await
        {
            Ok(handle) => {
                job.attempt = Some(Attempt {
                    id: id.clone(),
                    handle,
                });
                let row = CheckpointRow {
                    job_id: job.id.clone(),
                    attempt_id: id,
                    backend_id: cp.backend.clone(),
                    backend_version: self.backend_version.clone(),
                    format_version: cp.format_version,
                    confirmed: cp.confirmed,
                    payload: cp.payload.clone(),
                    updated_at_ms: now_ms(),
                };
                if let Some(s) = store.as_mut()
                    && let Err(e) = s.rebind_checkpoint(&job.row(), &row, previous)
                {
                    eprintln!("dm-service: job {} not rebound: {e}", job.id);
                }
            }
            Err(e) => {
                job.fail(recovery_error(&e));
                persist(store, job);
            }
        }
    }

    /// `downloads.v1.locate` (B2.19): the job's bound folder and file name, from the service's
    /// own record. A completed job names its final file; anything else names the folder only.
    pub async fn locate(&self, id: &JobId) -> Result<LocateResult, ServiceError> {
        let mut state = self.state.lock().await;
        let job = state.job_mut(id)?;
        if job.removed_at_ms.is_some() {
            return Err(ServiceError::new(
                ErrorKind::InvalidRequest,
                "this download was removed from the list",
            ));
        }
        let directory = job.destination.clone();
        let file_name = if job.phase == Phase::Completed {
            job.display_name.clone()
        } else {
            String::new()
        };
        let directory_present = directory.is_dir();
        let file_present =
            !file_name.is_empty() && Path::new(&directory).join(&file_name).is_file();
        let directory = directory.to_str().map(str::to_owned).ok_or_else(|| {
            ServiceError::new(
                ErrorKind::DestinationUnwritable,
                "the download folder path cannot be shown",
            )
        })?;
        Ok(LocateResult {
            directory,
            file_name,
            directory_present,
            file_present,
        })
    }
}
