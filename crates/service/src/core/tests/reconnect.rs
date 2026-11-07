//! BRW-05 reconnect (ADR 0028): a signed-in download that lost its sign-in is held with its
//! checkpoint, and a fresh sign-in attached to that very job continues it. The fake engine stands
//! in for the HTTP engine (whose restore re-verifies data, tested in dm-backend-http and the
//! process tests); a memory vault stands in for the OS store.
use super::*;
use dm_domain::api::{
    AddAuthenticatedParams, BrowserKind, CookieInput, CredentialMode, CredentialState,
    ForgetCredentialParams, ReconnectParams, ReconnectableParams, SessionInput,
};
use dm_domain::redact::SecretTransit;
use dm_platform::vault::{MemoryVault, Vault, VaultError};

const SECRET: &str = "SYNTH-RECONNECT-b33";
const FRESH: &str = "SYNTH-FRESH-b33";
const ORIGIN: &str = "https://files.example.org";
const PROFILE: &str = "0123456789abcdef";

fn session(value: &str, remember: bool) -> SessionInput {
    SessionInput {
        origin: ORIGIN.into(),
        browser: BrowserKind::Chrome,
        profile_id: PROFILE.into(),
        cookies: vec![CookieInput {
            name: "sid".into(),
            value: SecretTransit::new(value),
            domain: "files.example.org".into(),
            host_only: true,
            path: "/".into(),
            secure: true,
            expires_at_ms: None,
        }],
        remember,
    }
}

fn add(key: &str, remember: bool) -> AddAuthenticatedParams {
    AddAuthenticatedParams {
        add: add_params(key, &format!("{ORIGIN}/reports/q3.pdf?sig={SECRET}")),
        session: session(SECRET, remember),
    }
}

fn reconnect(job: &DownloadSnapshot, op: &str, value: &str, resume: bool) -> ReconnectParams {
    ReconnectParams {
        job_id: job.job_id.clone(),
        operation_id: op.into(),
        expected_credential_id: job.auth.as_ref().unwrap().credential_id.clone(),
        session: session(value, false),
        resume,
    }
}

fn stored(db: &Path, dir: &Path, vault: Arc<dyn Vault>) -> Core<FakeBackend> {
    core_with(FakeBackend::with_all(CapabilitySupport::Supported), dir)
        .with_vault(vault)
        .with_store(Store::open(db).unwrap())
        .unwrap()
}

/// Lets `job`'s attempt confirm `bytes` durably (one poll takes the checkpoint).
async fn checkpointed(core: &Core<FakeBackend>, job: &DownloadSnapshot, bytes: u64) {
    let handle = BackendHandle(job.attempt_id.clone().unwrap().as_str().to_owned());
    core.backend()
        .set_progress(&handle, progress(TransferState::Active, bytes, None));
    core.backend().set_checkpoint(
        &handle,
        Ok(Some(dm_backend_api::DurableCheckpoint {
            backend: "fake".into(),
            format_version: 1,
            confirmed: ByteCount(bytes),
            payload: b"{\"v\":1}".to_vec(),
        })),
    );
    core.poll_once().await;
}

/// The cookie the last restored or started attempt carries.
fn last_cookie(core: &Core<FakeBackend>, url: &str) -> Option<String> {
    let b = core.backend();
    let restored = b.restored.lock().unwrap().last().map(|(r, _)| r.clone());
    let started = b.started.lock().unwrap().last().cloned();
    restored
        .or(started)
        .and_then(|r| r.session)
        .and_then(|s| s.cookie_header(url, 0))
        .map(|c| c.expose().to_owned())
}

fn held(s: &DownloadSnapshot) -> bool {
    s.blockers.contains(&Blocker::SignInRequired)
}

/// A temporary signed-in job with durable progress, then a restart: held with its checkpoint.
async fn restarted_temporary(
    db: &Path,
    dir: &Path,
    vault: Arc<MemoryVault>,
) -> (Core<FakeBackend>, DownloadSnapshot) {
    let id = {
        let core = stored(db, dir, vault.clone());
        let s = core
            .add_authenticated(add("rc-temp-0001", false))
            .await
            .unwrap();
        checkpointed(&core, &s, 4096).await;
        s.job_id
    };
    let core = stored(db, dir, vault);
    core.recover().await;
    let s = core.get(&id).await.unwrap();
    (core, s)
}

#[tokio::test]
async fn a_temporary_sign_in_lost_with_a_restart_is_held_and_reconnected_on_the_same_job() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("downova.sqlite3");
    let vault = Arc::new(MemoryVault::default());
    let (core, s) = restarted_temporary(&db, dir.path(), vault.clone()).await;
    // Held, not failed, nothing restored without the session (never silently public).
    assert!(held(&s), "{s:?}");
    assert_eq!(s.phase, Phase::Queued);
    assert_eq!(
        s.downloaded_bytes,
        ByteCount(4096),
        "it shows the kept (durable) part"
    );
    assert_eq!(s.auth.as_ref().unwrap().state, CredentialState::Missing);
    assert_eq!(s.error.as_ref().unwrap().next_action, NextAction::SignIn);
    assert!(core.backend().restored.lock().unwrap().is_empty());
    assert!(core.backend().started.lock().unwrap().is_empty());
    // It is offered to this browser profile for this origin only.
    let offered = core
        .reconnectable(ReconnectableParams {
            origin: ORIGIN.into(),
            browser: BrowserKind::Chrome,
            profile_id: PROFILE.into(),
        })
        .await
        .unwrap();
    assert_eq!(offered.downloads.len(), 1);
    assert_eq!(offered.downloads[0].job_id, s.job_id);
    for (origin, browser, profile) in [
        ("https://other.example.org", BrowserKind::Chrome, PROFILE),
        (ORIGIN, BrowserKind::Edge, PROFILE),
        (ORIGIN, BrowserKind::Chrome, "fedcba9876543210"),
    ] {
        let none = core
            .reconnectable(ReconnectableParams {
                origin: origin.into(),
                browser,
                profile_id: profile.into(),
            })
            .await
            .unwrap();
        assert!(none.downloads.is_empty(), "{origin} {browser:?} {profile}");
    }
    // Reconnect: same job, same history, continued from its checkpoint with the fresh sign-in.
    let r = core
        .reconnect(reconnect(&s, "rc-op-000001", FRESH, false))
        .await
        .unwrap();
    assert_eq!(r.job_id, s.job_id);
    assert!(!held(&r));
    assert_eq!(r.auth.as_ref().unwrap().state, CredentialState::Available);
    assert_ne!(
        r.auth.as_ref().unwrap().credential_id,
        s.auth.as_ref().unwrap().credential_id
    );
    let restored = core.backend().restored.lock().unwrap().clone();
    assert_eq!(restored.len(), 1, "restored once from its checkpoint");
    assert_eq!(
        restored[0].1.confirmed,
        ByteCount(4096),
        "the partial data is kept"
    );
    let url = format!("{ORIGIN}/reports/q3.pdf?sig={SECRET}");
    assert_eq!(restored[0].0.url, url, "the sealed link came back");
    assert_eq!(last_cookie(&core, &url), Some(format!("sid={FRESH}")));
    // The old temporary context is retired.
    let list = core.list_credentials().await.credentials;
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].credential_id, r.auth.unwrap().credential_id);
}

#[tokio::test]
async fn an_expired_remembered_sign_in_is_held_once_and_reconnect_and_resume_continues() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("downova.sqlite3");
    let vault = Arc::new(MemoryVault::default());
    let core = stored(&db, dir.path(), vault.clone());
    let s = core
        .add_authenticated(add("rc-exp-0001", true))
        .await
        .unwrap();
    checkpointed(&core, &s, 8192).await;
    // The site rejects the session mid-transfer.
    let handle = BackendHandle(s.attempt_id.clone().unwrap().as_str().to_owned());
    core.backend().set_progress(
        &handle,
        progress(
            TransferState::Failed(BackendError::new(ErrorKind::AuthRequired, "401")),
            8192,
            None,
        ),
    );
    core.poll_once().await;
    core.poll_once().await;
    let h = core.get(&s.job_id).await.unwrap();
    assert!(held(&h), "{h:?}");
    assert_ne!(
        h.phase,
        Phase::Failed,
        "never failed: the checkpoint is kept"
    );
    assert_eq!(h.auth.as_ref().unwrap().state, CredentialState::Expired);
    assert_eq!(
        core.backend().started.lock().unwrap().len(),
        1,
        "no retry storm"
    );
    // The user also paused it meanwhile; "Reconnect and resume" is the explicit wish to run.
    core.pause(&s.job_id).await.unwrap();
    let r = core
        .reconnect(reconnect(&h, "rc-op-000002", FRESH, true))
        .await
        .unwrap();
    assert_eq!(r.desired_state, DesiredState::Running);
    let restored = core.backend().restored.lock().unwrap().clone();
    assert_eq!(restored.last().unwrap().1.confirmed, ByteCount(8192));
    let url = format!("{ORIGIN}/reports/q3.pdf?sig={SECRET}");
    assert_eq!(last_cookie(&core, &url), Some(format!("sid={FRESH}")));
    // The expired remembered entry was retired from the OS store; the new one is temporary.
    let ids: Vec<String> = vault.entries.lock().unwrap().keys().cloned().collect();
    assert_eq!(ids, vec!["data-key".to_owned()]);
}

#[tokio::test]
async fn reconnect_keeps_a_manual_pause_and_a_stopped_queue() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("downova.sqlite3");
    let vault = Arc::new(MemoryVault::default());
    let id = {
        let core = stored(&db, dir.path(), vault.clone());
        let s = core
            .add_authenticated(add("rc-pause-001", false))
            .await
            .unwrap();
        checkpointed(&core, &s, 4096).await;
        core.pause(&s.job_id).await.unwrap();
        s.job_id
    };
    let core = stored(&db, dir.path(), vault.clone());
    core.recover().await;
    let s = core.get(&id).await.unwrap();
    assert!(held(&s));
    let r = core
        .reconnect(reconnect(&s, "rc-op-000003", FRESH, false))
        .await
        .unwrap();
    assert_eq!(
        r.desired_state,
        DesiredState::Paused,
        "reconnect alone never resumes"
    );
    assert!(!held(&r));
    // Restored (paused, from its checkpoint) but not running.
    assert!(
        core.backend()
            .calls()
            .iter()
            .all(|c| !c.starts_with("resume "))
    );
    // A stopped queue keeps a resumed job waiting.
    core.set_queue_running(DEFAULT_QUEUE, false).await.unwrap();
    let again = core.resume(&id).await.unwrap();
    assert!(again.blockers.contains(&Blocker::QueueStopped), "{again:?}");
}

#[tokio::test]
async fn reconnect_is_bound_to_its_job_origin_profile_and_generation() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("downova.sqlite3");
    let vault = Arc::new(MemoryVault::default());
    let (core, s) = restarted_temporary(&db, dir.path(), vault.clone()).await;
    let public = core
        .add(add_params(
            "rc-public-01",
            "https://files.example.org/public.bin",
        ))
        .await
        .unwrap();
    // A public download never gains a session.
    let mut p = reconnect(&s, "rc-op-000010", FRESH, false);
    p.job_id = public.job_id.clone();
    assert_eq!(
        core.reconnect(p).await.unwrap_err().0.kind,
        ErrorKind::InvalidRequest
    );
    // Another site.
    let mut p = reconnect(&s, "rc-op-000011", FRESH, false);
    p.session.origin = "https://other.example.org".into();
    p.session.cookies[0].domain = "other.example.org".into();
    assert_eq!(
        core.reconnect(p).await.unwrap_err().0.kind,
        ErrorKind::AuthScopeUnsupported
    );
    // Another browser profile, another browser.
    let mut p = reconnect(&s, "rc-op-000012", FRESH, false);
    p.session.profile_id = "fedcba9876543210".into();
    assert_eq!(
        core.reconnect(p).await.unwrap_err().0.kind,
        ErrorKind::AuthScopeUnsupported
    );
    let mut p = reconnect(&s, "rc-op-000013", FRESH, false);
    p.session.browser = BrowserKind::Edge;
    assert_eq!(
        core.reconnect(p).await.unwrap_err().0.kind,
        ErrorKind::AuthScopeUnsupported
    );
    // A cookie that does not apply to the download's link.
    let mut p = reconnect(&s, "rc-op-000014", FRESH, false);
    p.session.cookies[0].path = "/elsewhere".into();
    assert_eq!(
        core.reconnect(p).await.unwrap_err().0.kind,
        ErrorKind::AuthRequired
    );
    // Nothing changed so far.
    assert!(held(&core.get(&s.job_id).await.unwrap()));
    // Stale generation: after one reconnect, a request listed before it is refused.
    core.reconnect(reconnect(&s, "rc-op-000015", FRESH, false))
        .await
        .unwrap();
    let stale = core
        .reconnect(reconnect(&s, "rc-op-000016", FRESH, false))
        .await
        .unwrap_err();
    assert_eq!(stale.0.kind, ErrorKind::InvalidRequest);
    // Completed and cancelled downloads are never reopened.
    let done = core.cancel(&s.job_id).await.unwrap();
    let mut p = reconnect(&done, "rc-op-000017", FRESH, false);
    p.expected_credential_id = done.auth.unwrap().credential_id;
    assert_eq!(
        core.reconnect(p).await.unwrap_err().0.kind,
        ErrorKind::InvalidRequest
    );
}

#[tokio::test]
async fn a_repeated_reconnect_changes_nothing_twice_and_a_reused_id_is_a_conflict() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("downova.sqlite3");
    let vault = Arc::new(MemoryVault::default());
    let (core, s) = restarted_temporary(&db, dir.path(), vault.clone()).await;
    let first = core
        .reconnect(reconnect(&s, "rc-op-000020", FRESH, false))
        .await
        .unwrap();
    let restored = core.backend().restored.lock().unwrap().len();
    // A lost acknowledgement: the browser repeats with the same id and re-read cookie values.
    let again = core
        .reconnect(reconnect(&s, "rc-op-000020", "SYNTH-REREAD-b33", false))
        .await
        .unwrap();
    assert_eq!(again.auth, first.auth, "the same sign-in, unchanged");
    assert_eq!(core.backend().restored.lock().unwrap().len(), restored);
    let url = format!("{ORIGIN}/reports/q3.pdf?sig={SECRET}");
    assert_eq!(last_cookie(&core, &url), Some(format!("sid={FRESH}")));
    // The same id for another choice (or another job) is a conflict, never applied.
    let conflict = core
        .reconnect(reconnect(&s, "rc-op-000020", FRESH, true))
        .await
        .unwrap_err();
    assert_eq!(conflict.0.kind, ErrorKind::IdempotencyConflict);
    // Also after a restart (the operation is recorded with the change).
    drop(core);
    let core = stored(&db, dir.path(), vault.clone());
    core.recover().await;
    let replay = core
        .reconnect(reconnect(&s, "rc-op-000020", "SYNTH-REREAD-b33", false))
        .await
        .unwrap();
    assert_eq!(replay.job_id, s.job_id);
}

#[tokio::test]
async fn store_and_database_failures_leave_the_old_state_and_no_new_secret() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("downova.sqlite3");
    let vault = Arc::new(MemoryVault::default());
    let (core, s) = restarted_temporary(&db, dir.path(), vault.clone()).await;
    // Remember refused by a locked store: refused, never downgraded to a temporary sign-in.
    *vault.fail.lock().unwrap() = Some(VaultError::Locked);
    let mut p = reconnect(&s, "rc-op-000030", FRESH, false);
    p.session.remember = true;
    let e = core.reconnect(p).await.unwrap_err();
    assert_eq!(e.0.kind, ErrorKind::CredentialStoreUnavailable);
    *vault.fail.lock().unwrap() = None;
    let after = core.get(&s.job_id).await.unwrap();
    assert!(held(&after));
    assert_eq!(after.auth, s.auth, "the job keeps its old reference");
    // Too large for the store: also refused (not silently temporary).
    *vault.fail.lock().unwrap() = Some(VaultError::TooLarge);
    let mut p = reconnect(&s, "rc-op-000031", FRESH, false);
    p.session.remember = true;
    assert_eq!(
        core.reconnect(p).await.unwrap_err().0.kind,
        ErrorKind::CredentialStoreUnavailable
    );
    *vault.fail.lock().unwrap() = None;
    drop(core);
    // The database refuses the change: the new store entry is removed again.
    rusqlite::Connection::open(&db)
        .unwrap()
        .execute_batch("CREATE TRIGGER refuse_ops BEFORE INSERT ON operations BEGIN SELECT RAISE(ABORT, 'refused'); END;")
        .unwrap();
    let core = stored(&db, dir.path(), vault.clone());
    core.recover().await;
    let mut p = reconnect(&s, "rc-op-000032", FRESH, false);
    p.session.remember = true;
    assert!(core.reconnect(p).await.is_err());
    let ids: Vec<String> = vault.entries.lock().unwrap().keys().cloned().collect();
    assert_eq!(
        ids,
        vec!["data-key".to_owned()],
        "no new remembered entry left behind"
    );
    let after = core.get(&s.job_id).await.unwrap();
    assert!(held(&after), "{after:?}");
    assert_eq!(
        after.auth.unwrap().credential_id,
        s.auth.unwrap().credential_id
    );
    drop(core);
    // Nothing secret reached the database folder.
    for e in std::fs::read_dir(dir.path()).unwrap() {
        let p = e.unwrap().path();
        if p.is_file() {
            let bytes = std::fs::read(&p).unwrap();
            for secret in [FRESH, SECRET] {
                assert!(
                    !bytes.windows(secret.len()).any(|w| w == secret.as_bytes()),
                    "{} holds {secret}",
                    p.display()
                );
            }
        }
    }
}

/// A vault whose `put` waits until the test lets it go, so a cancel can race the reconnect.
#[derive(Debug)]
struct GatedVault {
    inner: MemoryVault,
    gate: std::sync::Mutex<Option<std::sync::mpsc::Receiver<()>>>,
    entered: std::sync::Mutex<Option<std::sync::mpsc::Sender<()>>>,
}

impl Vault for GatedVault {
    fn put(&self, id: &str, secret: &[u8]) -> Result<(), VaultError> {
        if id != dm_platform_key() {
            if let Some(tx) = self.entered.lock().unwrap().take() {
                let _ = tx.send(());
            }
            if let Some(rx) = self.gate.lock().unwrap().take() {
                let _ = rx.recv();
            }
        }
        self.inner.put(id, secret)
    }
    fn get(&self, id: &str) -> Result<Vec<u8>, VaultError> {
        self.inner.get(id)
    }
    fn delete(&self, id: &str) -> Result<(), VaultError> {
        self.inner.delete(id)
    }
}

fn dm_platform_key() -> &'static str {
    "data-key"
}

#[tokio::test]
async fn a_cancel_or_forget_racing_a_reconnect_wins_and_leaves_no_entry() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("downova.sqlite3");
    let (go_tx, go_rx) = std::sync::mpsc::channel();
    let (in_tx, in_rx) = std::sync::mpsc::channel();
    let vault = Arc::new(GatedVault {
        inner: MemoryVault::default(),
        gate: std::sync::Mutex::new(Some(go_rx)),
        entered: std::sync::Mutex::new(Some(in_tx)),
    });
    let core = Arc::new(stored(&db, dir.path(), vault.clone()));
    let s = core
        .add_authenticated(add("rc-race-0001", false))
        .await
        .unwrap();
    checkpointed(&core, &s, 4096).await;
    core.forget_credential(ForgetCredentialParams {
        credential_id: s.auth.clone().unwrap().credential_id,
    })
    .await
    .unwrap();
    let h = core.get(&s.job_id).await.unwrap();
    assert!(held(&h), "forget holds the job: {h:?}");
    assert_eq!(h.desired_state, DesiredState::Paused);
    // Forget released the attempt that carried the old session: resuming does not run it.
    core.resume(&s.job_id).await.unwrap();
    assert!(held(&core.get(&s.job_id).await.unwrap()));
    // A remembered reconnect waits in the store while the user cancels the download.
    let mut p = reconnect(&h, "rc-op-000040", FRESH, true);
    p.session.remember = true;
    let racing = {
        let core = Arc::clone(&core);
        tokio::spawn(async move { core.reconnect(p).await })
    };
    tokio::task::spawn_blocking(move || in_rx.recv().unwrap())
        .await
        .unwrap();
    core.cancel(&s.job_id).await.unwrap();
    go_tx.send(()).unwrap();
    let e = racing.await.unwrap().unwrap_err();
    assert_eq!(e.0.kind, ErrorKind::InvalidRequest, "{e:?}");
    let ids: Vec<String> = vault
        .inner
        .entries
        .lock()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    assert_eq!(
        ids,
        vec!["data-key".to_owned()],
        "the racing entry was removed"
    );
    assert_eq!(core.get(&s.job_id).await.unwrap().phase, Phase::Cancelled);
}

#[tokio::test]
async fn a_failed_signed_in_download_gets_its_sign_in_back_and_retry_decides_the_restart() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("downova.sqlite3");
    let vault = Arc::new(MemoryVault::default());
    let core = stored(&db, dir.path(), vault.clone());
    let s = core
        .add_authenticated(add("rc-fail-0001", false))
        .await
        .unwrap();
    // A legacy terminal failure (before holds existed): failed with AUTH_REQUIRED.
    let handle = BackendHandle(s.attempt_id.clone().unwrap().as_str().to_owned());
    core.backend().set_progress(
        &handle,
        progress(
            TransferState::Failed(BackendError::new(ErrorKind::Internal, "x")),
            0,
            None,
        ),
    );
    core.poll_once().await;
    {
        let mut state = core.state.lock().await;
        let job = state.job_mut(&s.job_id).unwrap();
        job.fail(ErrorKind::AuthRequired.to_error_data("401"));
        job.auth.as_mut().unwrap().state = CredentialState::Expired;
    }
    if let Some(c) = core
        .creds()
        .contexts
        .get_mut(&s.auth.clone().unwrap().credential_id)
    {
        c.scope = None;
        c.info.state = CredentialState::Expired;
    }
    let f = core.get(&s.job_id).await.unwrap();
    assert_eq!(f.phase, Phase::Failed);
    // Retry without a sign-in is refused with the way on.
    let e = core
        .retry(dm_domain::api::RetryParams {
            job_id: s.job_id.clone(),
            operation_id: "rc-retry-0001".into(),
            discard_partial: true,
            expected_attempt_id: None,
        })
        .await
        .unwrap_err();
    assert_eq!(e.0.next_action, NextAction::SignIn);
    let r = core
        .reconnect(reconnect(&f, "rc-op-000050", FRESH, true))
        .await
        .unwrap();
    assert_eq!(
        r.phase,
        Phase::Failed,
        "a failed download is restarted only by Retry"
    );
    let retried = core
        .retry(dm_domain::api::RetryParams {
            job_id: s.job_id.clone(),
            operation_id: "rc-retry-0002".into(),
            discard_partial: true,
            expected_attempt_id: None,
        })
        .await
        .unwrap();
    assert_ne!(retried.phase, Phase::Failed);
    let url = format!("{ORIGIN}/reports/q3.pdf?sig={SECRET}");
    assert_eq!(last_cookie(&core, &url), Some(format!("sid={FRESH}")));
    let _ = CredentialMode::Temporary;
}

#[tokio::test]
async fn a_repeated_signed_in_add_never_changes_the_accepted_sign_in() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("downova.sqlite3");
    let vault = Arc::new(MemoryVault::default());
    let core = stored(&db, dir.path(), vault.clone());
    let s = core
        .add_authenticated(add("rc-add-00001", true))
        .await
        .unwrap();
    // A lost acknowledgement: the browser repeats with the same key and re-read cookie values.
    let mut again = add("rc-add-00001", true);
    again.session.cookies[0].value = SecretTransit::new(FRESH);
    let r = core.add_authenticated(again).await.unwrap();
    assert_eq!(r.job_id, s.job_id);
    assert_eq!(r.auth, s.auth);
    let url = format!("{ORIGIN}/reports/q3.pdf?sig={SECRET}");
    assert_eq!(last_cookie(&core, &url), Some(format!("sid={SECRET}")));
    let id = s.auth.unwrap().credential_id;
    let stored_secret = vault.entries.lock().unwrap().get(&id).cloned().unwrap();
    let text = String::from_utf8_lossy(&stored_secret).into_owned();
    assert!(
        text.contains(SECRET) && !text.contains(FRESH),
        "the remembered entry is unchanged"
    );
    assert_eq!(core.list_credentials().await.credentials.len(), 1);
}
