//! Reconnect an existing signed-in download (BRW-05, ADR 0028).
//!
//! A signed-in download whose sign-in is gone (expired at the site, a temporary one lost with a
//! restart, forgotten, or locked away) is *held* with `sign_in_required`, keeping its checkpoint
//! and partial data. The user attaches a fresh sign-in from the browser to **that** job:
//!
//! - **Binding.** The request names the job, the origin, the browser profile and the job's
//!   current credential (`expected_credential_id`, which changes with every reconnect). Anything
//!   else (another site, another profile, a stale or replayed request) is refused; nothing is
//!   matched by name or display link.
//! - **Idempotency.** `operation_id` is recorded with the change in one transaction. A repeat (a
//!   lost acknowledgement) answers with the job as it is and changes nothing a second time, even
//!   when the browser read different cookie values for it; the fingerprint holds no secret. The
//!   same id for a different job, origin, profile or choice is an `IDEMPOTENCY_CONFLICT`.
//! - **Consistency.** A remembered sign-in goes to the OS store first (outside every lock); the
//!   job is checked again and the database records the new credential, the job's reference, the
//!   operation and the retired row together. Any failure removes the new store entry and leaves
//!   the old state as it was. A refused remembered sign-in is never turned into a temporary one.
//! - **Shared contexts.** The old context is retired (memory, row, store entry) only when no
//!   other download still references it; those keep it, and their own state, unchanged.
//! - **Continuing.** Reconnect alone keeps the desired state (a manual pause stays paused, a
//!   stopped queue stays stopped). "Reconnect and resume" also asks to run. The job continues
//!   from its checkpoint through the engine's restore, which re-verifies the partial data and
//!   the strong validator; a changed resource is refused there, never spliced. A failed download
//!   only gets its sign-in back: starting it again is the existing Retry (with its confirmation).

use std::sync::Arc;

use dm_backend_api::DownloadBackend;
use dm_domain::api::{
    AuthInfo, CredentialMode, CredentialState, DownloadSnapshot, ReconnectCandidate,
    ReconnectParams, ReconnectableParams, ReconnectableResult,
};
use dm_domain::cookies::Origin;
use dm_domain::{
    Blocker, Capability, CapabilitySupport, DesiredState, ErrorKind, NextAction, Phase,
};
use dm_storage::{OperationRow, Store};

use super::operations::fingerprint;
use super::{
    Core, Job, Pending, ServiceError, State, credentials, find_job, now_ms, persist, queue_running,
    stopping, support_of, trace, validate_idempotency_key,
};

pub(super) const RECONNECT: &str = "reconnect";
const RECONNECTED: &str = "reconnected";

fn refuse(kind: ErrorKind, why: &str) -> ServiceError {
    ServiceError::new(kind, why)
}

/// Whether `job` waits for a reconnect: held, or its context has no usable session.
fn needs_reconnect(job: &Job, usable: bool) -> bool {
    job.is_sign_in_held() || !usable || (job.phase == Phase::Failed && is_auth_error(job))
}

fn is_auth_error(job: &Job) -> bool {
    job.error.as_ref().is_some_and(|e| {
        matches!(
            e.kind,
            ErrorKind::AuthRequired | ErrorKind::CredentialStoreUnavailable
        )
    })
}

impl<B: DownloadBackend> Core<B> {
    /// `credentials.v1.reconnectable`: this origin's unfinished signed-in downloads that the
    /// requesting browser profile may reconnect. Metadata only.
    pub async fn reconnectable(
        &self,
        params: ReconnectableParams,
    ) -> Result<ReconnectableResult, ServiceError> {
        let origin = Origin::parse(&params.origin)
            .filter(|o| o.scheme == "https")
            .ok_or_else(|| {
                refuse(
                    ErrorKind::AuthScopeUnsupported,
                    "only https sites can reconnect a download",
                )
            })?;
        // Copied first: the registry lock is never held while waiting for the state lock.
        let contexts: Vec<(String, String, AuthInfo, bool)> = self
            .creds()
            .contexts
            .values()
            .map(|c| {
                (
                    c.info.credential_id.clone(),
                    c.profile_id.clone(),
                    c.info.clone(),
                    c.scope.is_some(),
                )
            })
            .collect();
        let state = self.state.lock().await;
        let mut downloads = Vec::new();
        for job in state.jobs.iter() {
            if job.removed_at_ms.is_some()
                || job.url_lost
                || matches!(job.phase, Phase::Completed | Phase::Cancelled)
                || job.finalizing.is_some()
            {
                continue;
            }
            let Some(id) = job.credential_id.as_ref() else {
                continue;
            };
            let Some((_, profile, info, usable)) = contexts.iter().find(|c| &c.0 == id) else {
                continue;
            };
            if Origin::parse(&info.origin).as_ref() != Some(&origin)
                || info.browser != params.browser
                || profile != &params.profile_id
                || !needs_reconnect(job, *usable)
            {
                continue;
            }
            let snapshot = job.snapshot();
            downloads.push(ReconnectCandidate {
                job_id: snapshot.job_id,
                display_name: snapshot.display_name,
                phase: snapshot.phase,
                desired_state: snapshot.desired_state,
                downloaded_bytes: snapshot.downloaded_bytes,
                total_bytes: snapshot.total_bytes,
                auth: snapshot.auth.unwrap_or_else(|| info.clone()),
            });
        }
        Ok(ReconnectableResult { downloads })
    }

    /// The checks a reconnect needs against the job as it is now (run twice: before the store
    /// call and again, under the lock, before anything is committed).
    fn check_reconnect(&self, job: &Job, params: &ReconnectParams) -> Result<(), ServiceError> {
        if job.removed_at_ms.is_some() {
            return Err(refuse(
                ErrorKind::InvalidRequest,
                "this download was removed from the list",
            ));
        }
        match job.phase {
            Phase::Completed => {
                return Err(refuse(
                    ErrorKind::InvalidRequest,
                    "the download already completed",
                ));
            }
            Phase::Cancelled => {
                return Err(refuse(
                    ErrorKind::InvalidRequest,
                    "a cancelled download is not reopened; start it again from the browser",
                ));
            }
            _ => {}
        }
        if job.finalizing.is_some()
            || job.recover_journal
            || matches!(
                job.phase,
                Phase::Verifying | Phase::Scanning | Phase::Finalizing
            )
        {
            return Err(refuse(
                ErrorKind::InvalidRequest,
                "the download is being completed; wait until it finishes",
            )
            .with_action(NextAction::Wait));
        }
        let Some(current) = job.credential_id.as_deref() else {
            return Err(refuse(
                ErrorKind::InvalidRequest,
                "this download was not started with a sign-in",
            ));
        };
        if current != params.expected_credential_id {
            // Reconnected or changed since the browser listed it: a stale or replayed request.
            return Err(refuse(
                ErrorKind::InvalidRequest,
                "this download changed since it was listed; choose it again",
            ));
        }
        if job.url_lost {
            return Err(refuse(
                ErrorKind::LinkRefreshRequired,
                "this download's link could not be kept (the system credential store was unavailable), so a new sign-in cannot continue it; start it again from the browser. Its partial data stays until you remove the download",
            )
            .with_action(NextAction::SignIn));
        }
        let session_origin = Origin::parse(&params.session.origin);
        if session_origin.is_none() || Origin::parse(&job.url) != session_origin {
            return Err(refuse(
                ErrorKind::AuthScopeUnsupported,
                "this download belongs to another site than the sign-in",
            ));
        }
        let reg = self.creds();
        let ctx = reg.contexts.get(current);
        if ctx.is_some_and(|c| {
            c.info.browser != params.session.browser || c.profile_id != params.session.profile_id
        }) {
            return Err(refuse(
                ErrorKind::AuthScopeUnsupported,
                "reconnect it from the same browser profile it was started in",
            ));
        }
        Ok(())
    }

    /// `downloads.v1.reconnect` (ADR 0028).
    pub async fn reconnect(
        &self,
        params: ReconnectParams,
    ) -> Result<DownloadSnapshot, ServiceError> {
        validate_idempotency_key(&params.operation_id)?;
        params
            .session
            .validate()
            .map_err(|why| ServiceError::new(ErrorKind::InvalidRequest, why))?;
        if support_of(&self.capabilities, Capability::AuthenticatedGet)
            != CapabilitySupport::Supported
        {
            return Err(refuse(
                ErrorKind::BackendCapabilityUnavailable,
                "signed-in downloads need the default download engine",
            ));
        }
        let remember = params.session.remember;
        let fp = fingerprint(
            RECONNECT,
            &params.job_id,
            &serde_json::json!({
                "expected": params.expected_credential_id,
                "origin": params.session.origin,
                "browser": params.session.browser,
                "profile": params.session.profile_id,
                "remember": remember,
                "resume": params.resume,
            }),
        );
        // 1. A repeat answers as things are; otherwise the job is checked and its link read.
        let url = {
            let mut guard = self.state.lock().await;
            if let Some((_, id)) = Self::replay(&mut guard, &params.operation_id, RECONNECT, &fp)? {
                return Ok(guard.job_mut(&id)?.snapshot());
            }
            if self.is_stopping() {
                return Err(stopping());
            }
            let job = find_job(&mut guard.jobs, &params.job_id)?;
            self.check_reconnect(job, &params)?;
            job.url.clone()
        };
        let now = now_ms();
        let scope = credentials::scope_from(&params.session, now)?;
        if scope.cookie_header(&url, now).is_none() {
            return Err(refuse(
                ErrorKind::AuthRequired,
                "the browser offered no sign-in that applies to this download; sign in on the site and try again",
            )
            .with_action(NextAction::SignIn));
        }
        let new_id = credentials::credential_id(&params.operation_id, &params.session.origin);
        let info = AuthInfo {
            credential_id: new_id.clone(),
            origin: scope.origin.display(),
            browser: params.session.browser,
            mode: if remember {
                CredentialMode::Remembered
            } else {
                CredentialMode::Temporary
            },
            state: CredentialState::Available,
        };
        let expires = scope.cookies.iter().filter_map(|c| c.expires_at_ms).min();
        let scope = Arc::new(scope);
        // 2. A remembered sign-in goes to the OS store first, outside every lock. Refused (locked,
        //    unavailable, too large): nothing changes and the choice is not downgraded.
        if remember {
            let vault = Arc::clone(&self.vault);
            let (vid, bytes) = (new_id.clone(), credentials::encode_scope(&scope));
            let put = tokio::task::spawn_blocking(move || vault.put(&vid, &bytes))
                .await
                .map_err(|_| {
                    ServiceError::new(ErrorKind::Internal, "the credential task failed")
                })?;
            if let Err(e) = put {
                return Err(credentials::vault_error(&e));
            }
        }
        let undo = |vault: &Arc<dyn dm_platform::vault::Vault>| {
            if remember {
                let _ = vault.delete(&new_id);
            }
        };
        // 3. Checked again under the lock (a cancel, removal, forget or another reconnect may
        //    have happened meanwhile), then committed in one transaction.
        let mut guard = self.state.lock().await;
        match Self::replay(&mut guard, &params.operation_id, RECONNECT, &fp) {
            Ok(Some((_, id))) => return Ok(guard.job_mut(&id)?.snapshot()),
            Ok(None) => {}
            Err(e) => {
                drop(guard);
                undo(&self.vault);
                return Err(e);
            }
        }
        let State {
            jobs,
            store,
            queues,
            operations,
            ..
        } = &mut *guard;
        let checked = find_job(jobs, &params.job_id).and_then(|job| {
            self.check_reconnect(job, &params)?;
            Ok(job.id.clone())
        });
        let job_id = match checked {
            Ok(id) => id,
            Err(e) => {
                drop(guard);
                undo(&self.vault);
                return Err(e);
            }
        };
        let op = OperationRow {
            operation_id: params.operation_id.clone(),
            kind: RECONNECT.to_owned(),
            job_id: job_id.clone(),
            fingerprint_sha256: fp,
            outcome: RECONNECTED.to_owned(),
            created_at_ms: now,
        };
        let row = credentials::row_for(&info, &params.session.profile_id, "active", now, expires);
        let old = params.expected_credential_id.clone();
        let committed = {
            let job = find_job(jobs, &job_id)?;
            let mut job_row = job.row();
            job_row.credential_id = Some(new_id.clone());
            match store.as_mut() {
                Some(s) => s.reconnect_job(&job_row, &row, &op, &old),
                None => Ok(()),
            }
        };
        if let Err(e) = committed {
            eprintln!("dm-service: reconnect of job {job_id} not recorded: {e}");
            drop(guard);
            undo(&self.vault);
            return Err(ServiceError::new(
                ErrorKind::Internal,
                "the reconnect could not be recorded, so nothing was changed",
            )
            .with_action(NextAction::Retry));
        }
        self.creds().contexts.insert(
            new_id.clone(),
            credentials::Context {
                scope: Some(Arc::clone(&scope)),
                info: info.clone(),
                profile_id: params.session.profile_id.clone(),
                created_at_ms: now,
                expires_at_ms: expires,
                adding: false,
            },
        );
        operations.insert(op.operation_id.clone(), op);
        {
            let job = find_job(jobs, &job_id)?;
            job.credential_id = Some(new_id);
            job.auth = Some(info);
            self.release_sign_in_hold(
                store,
                job,
                params.resume,
                queue_running(queues, &job.queue_id),
            )
            .await;
        }
        let still_used = jobs
            .iter()
            .any(|j| j.credential_id.as_deref() == Some(old.as_str()));
        self.admit(jobs, queues, store).await;
        let snapshot = find_job(jobs, &job_id)?.snapshot();
        drop(guard);
        // 4. The old context is retired only when no other download uses it.
        if !still_used {
            let retired = self.creds().contexts.remove(&old);
            if retired.is_some_and(|c| c.info.mode == CredentialMode::Remembered) {
                let vault = Arc::clone(&self.vault);
                let _ = tokio::task::spawn_blocking(move || vault.delete(&old)).await;
            }
        }
        Ok(snapshot)
    }

    /// After a reconnect: the hold ends and the job continues as its desired state says. A live
    /// attempt (which carries the old session) is first released into a checkpoint. The partial
    /// data comes back only through the engine's restore, which re-verifies it.
    async fn release_sign_in_hold(
        &self,
        store: &mut Option<Store>,
        job: &mut Job,
        resume: bool,
        queue_running: bool,
    ) {
        if job.attempt.is_some() && !job.phase.is_terminal() {
            // Park it first: its engine transfer still holds the previous session.
            let error = self.sign_in_error(job);
            self.hold_for_sign_in(store, job, error).await;
        }
        job.blockers.retain(|b| *b != Blocker::SignInRequired);
        if job.phase == Phase::Failed {
            // Starting a failed download again is Retry's decision (it confirms discarding).
            persist(store, job);
            trace("reconnected", &job.id, None, job.durable);
            return;
        }
        if is_auth_error(job) {
            job.error = None;
        }
        if resume {
            job.desired_state = DesiredState::Running;
        }
        job.sync_queue_hold(queue_running);
        match job.recover_from.take() {
            Some(cp) => {
                self.restore_from_checkpoint(store, job, cp).await;
            }
            None if job.desired_state == DesiredState::Running => job.queue(Pending::Fresh),
            None => {}
        }
        persist(store, job);
        trace("reconnected", &job.id, None, job.durable);
    }
}
