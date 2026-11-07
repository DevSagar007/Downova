//! SEC-01/SEC-02/BRW-05 (ADR 0027): signed-in downloads in the service, with a memory vault (or a
//! failing one) standing in for the OS credential store. Synthetic secrets are searched for in
//! snapshots, events, the database and WAL files, diagnostics and the credential list.
use super::*;
use dm_domain::api::{
    AddAuthenticatedParams, BrowserKind, CookieInput, CredentialMode, CredentialState,
    DiagnosticsPreviewParams, ForgetCredentialParams, SessionInput,
};
use dm_domain::redact::SecretTransit;
use dm_platform::vault::{MemoryVault, Vault, VaultError};

const SECRET: &str = "SYNTH-SESSION-b33";
const ORIGIN: &str = "https://files.example.org";

fn session(remember: bool) -> SessionInput {
    SessionInput {
        origin: ORIGIN.into(),
        browser: BrowserKind::Edge,
        profile_id: "0123456789abcdef".into(),
        cookies: vec![
            CookieInput {
                name: "sid".into(),
                value: SecretTransit::new(SECRET),
                domain: "files.example.org".into(),
                host_only: true,
                path: "/".into(),
                secure: true,
                expires_at_ms: None,
            },
            CookieInput {
                name: "csrf".into(),
                value: SecretTransit::new(format!("{SECRET}-csrf")),
                domain: "files.example.org".into(),
                host_only: true,
                path: "/reports".into(),
                secure: true,
                expires_at_ms: None,
            },
        ],
        remember,
    }
}

fn params(key: &str, remember: bool) -> AddAuthenticatedParams {
    AddAuthenticatedParams {
        add: add_params(
            key,
            &format!("{ORIGIN}/reports/{SECRET}/q3.pdf?sig={SECRET}"),
        ),
        session: session(remember),
    }
}

fn core_v(dir: &Path, vault: Arc<dyn Vault>) -> Core<FakeBackend> {
    core_with(FakeBackend::with_all(CapabilitySupport::Supported), dir).with_vault(vault)
}

fn stored(db: &Path, dir: &Path, vault: Arc<dyn Vault>) -> Core<FakeBackend> {
    core_v(dir, vault)
        .with_store(Store::open(db).unwrap())
        .unwrap()
}

/// Every byte of the database folder (database, WAL, shared memory) is free of the secret.
fn assert_no_secret_on_disk(dir: &Path) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_file() {
            let bytes = std::fs::read(&p).unwrap();
            assert!(
                !bytes.windows(SECRET.len()).any(|w| w == SECRET.as_bytes()),
                "{} holds the secret",
                p.display()
            );
        }
    }
}

#[tokio::test]
async fn a_signed_in_download_uses_its_session_and_never_shows_it() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("downova.sqlite3");
    let vault = Arc::new(MemoryVault::default());
    let core = stored(&db, dir.path(), vault.clone());
    let mut events = core.subscribe().await.events;
    let s = core
        .add_authenticated(params("auth-ok-00001", false))
        .await
        .unwrap();
    let auth = s.auth.clone().unwrap();
    assert_eq!(
        (auth.mode, auth.state, auth.origin.as_str()),
        (
            CredentialMode::Temporary,
            CredentialState::Available,
            ORIGIN
        )
    );
    // The engine got the session for this attempt.
    let req = core
        .backend()
        .started
        .lock()
        .unwrap()
        .last()
        .cloned()
        .unwrap();
    let scope = req.session.expect("session passed to the attempt");
    assert_eq!(scope.origin.display(), ORIGIN);
    assert_eq!(
        scope
            .cookie_header(&format!("{ORIGIN}/reports/x"), 0)
            .unwrap()
            .expose(),
        format!("csrf={SECRET}-csrf; sid={SECRET}")
    );
    // Nothing secret anywhere observable.
    let shown = serde_json::to_string(&s).unwrap();
    assert!(!shown.contains(SECRET), "{shown}");
    assert!(!s.source_display.contains(SECRET));
    while let Ok(e) = events.try_recv() {
        assert!(!format!("{e:?}").contains(SECRET));
    }
    let list = core.list_credentials().await;
    assert_eq!(list.credentials.len(), 1);
    assert!(!serde_json::to_string(&list).unwrap().contains(SECRET));
    let diag = core
        .diagnostics_preview(DiagnosticsPreviewParams::default())
        .await
        .unwrap();
    assert!(!diag.content.contains(SECRET));
    assert!(
        diag.content.contains("\"link_sealing_active\": true"),
        "{}",
        diag.content
    );
    // A temporary sign-in never reaches the vault; only the data key does.
    assert_eq!(
        vault.entries.lock().unwrap().keys().collect::<Vec<_>>(),
        vec!["data-key"]
    );
    drop(core);
    assert_no_secret_on_disk(dir.path());
}

#[tokio::test]
async fn scope_and_capability_are_checked_before_anything_is_kept() {
    let dir = tempfile::tempdir().unwrap();
    // Another origin, or http, is refused.
    let core = core_v(dir.path(), Arc::new(MemoryVault::default()));
    let mut p = params("auth-scope-001", false);
    p.add.source.url = "https://cdn.example.org/q3.pdf".into();
    assert_eq!(
        core.add_authenticated(p).await.unwrap_err().0.kind,
        ErrorKind::AuthScopeUnsupported
    );
    let mut p = params("auth-scope-002", false);
    p.add.source.url = "http://files.example.org/q3.pdf".into();
    assert_eq!(
        core.add_authenticated(p).await.unwrap_err().0.kind,
        ErrorKind::AuthScopeUnsupported
    );
    // No cookie that can apply to the site.
    let mut p = params("auth-scope-003", false);
    for c in &mut p.session.cookies {
        c.domain = "other.example".into();
    }
    assert_eq!(
        core.add_authenticated(p).await.unwrap_err().0.kind,
        ErrorKind::AuthRequired
    );
    // An engine without the capability refuses.
    let core = core_with(
        FakeBackend::supported_except(Capability::AuthenticatedGet, CapabilitySupport::Unsupported),
        dir.path(),
    );
    let e = core
        .add_authenticated(params("auth-scope-004", false))
        .await
        .unwrap_err();
    assert_eq!(e.0.kind, ErrorKind::BackendCapabilityUnavailable);
    assert!(core.list().await.is_empty());
    assert!(core.backend().started.lock().unwrap().is_empty());
}

#[tokio::test]
async fn remembering_needs_the_store_and_failures_leave_no_secret_behind() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("downova.sqlite3");
    // Locked store: refused, nothing kept.
    let locked = Arc::new(MemoryVault::default());
    *locked.fail.lock().unwrap() = Some(VaultError::Locked);
    let core = stored(&db, dir.path(), locked.clone());
    let e = core
        .add_authenticated(params("auth-lock-0001", true))
        .await
        .unwrap_err();
    assert_eq!(e.0.kind, ErrorKind::CredentialStoreUnavailable);
    assert!(core.list().await.is_empty());
    assert!(core.list_credentials().await.credentials.is_empty());
    drop(core);
    // Store success, then the database refuses the job: the store entry and the row are removed.
    let vault = Arc::new(MemoryVault::default());
    let core = stored(&db, dir.path(), vault.clone());
    rusqlite::Connection::open(&db)
        .unwrap()
        .execute_batch("CREATE TRIGGER refuse_jobs BEFORE INSERT ON jobs BEGIN SELECT RAISE(ABORT, 'refused'); END;")
        .unwrap_or(()); // the service holds the database; see below
    drop(core);
    rusqlite::Connection::open(&db)
        .unwrap()
        .execute_batch("CREATE TRIGGER refuse_jobs BEFORE INSERT ON jobs BEGIN SELECT RAISE(ABORT, 'refused'); END;")
        .unwrap();
    let core = stored(&db, dir.path(), vault.clone());
    assert!(
        core.add_authenticated(params("auth-sqlf-0001", true))
            .await
            .is_err()
    );
    let ids: Vec<String> = vault.entries.lock().unwrap().keys().cloned().collect();
    assert_eq!(
        ids,
        vec!["data-key".to_owned()],
        "the remembered sign-in was removed again"
    );
    assert!(core.list_credentials().await.credentials.is_empty());
    drop(core);
    assert_no_secret_on_disk(dir.path());
}

#[tokio::test]
async fn a_lost_acknowledgement_retry_returns_the_same_job_and_one_entry() {
    let dir = tempfile::tempdir().unwrap();
    let vault = Arc::new(MemoryVault::default());
    let core = core_v(dir.path(), vault.clone());
    let a = core
        .add_authenticated(params("auth-retry-001", true))
        .await
        .unwrap();
    let b = core
        .add_authenticated(params("auth-retry-001", true))
        .await
        .unwrap();
    assert_eq!(a.job_id, b.job_id);
    assert_eq!(core.list().await.len(), 1);
    let creds = core.list_credentials().await.credentials;
    assert_eq!(creds.len(), 1);
    assert_eq!(
        vault.entries.lock().unwrap().len(),
        2,
        "the data key and one sign-in"
    );
}

#[tokio::test]
async fn restarts_restore_remembered_sign_ins_and_hold_temporary_ones() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("downova.sqlite3");
    let vault = Arc::new(MemoryVault::default());
    let (temp, kept) = {
        let core = stored(&db, dir.path(), vault.clone());
        let t = core
            .add_authenticated(params("auth-rest-0001", false))
            .await
            .unwrap();
        core.pause(&t.job_id).await.unwrap();
        let k = core
            .add_authenticated(params("auth-rest-0002", true))
            .await
            .unwrap();
        // A durable checkpoint, so the paused job is resumable after the restart.
        let handle = BackendHandle(k.attempt_id.clone().unwrap().as_str().to_owned());
        core.backend()
            .set_progress(&handle, progress(TransferState::Active, 2048, None));
        core.backend().set_checkpoint(
            &handle,
            Ok(Some(dm_backend_api::DurableCheckpoint {
                backend: "fake".into(),
                format_version: 1,
                confirmed: ByteCount(2048),
                payload: format!("{{\"url\":\"{ORIGIN}/reports/{SECRET}/q3.pdf?sig={SECRET}\"}}")
                    .into_bytes(),
            })),
        );
        core.poll_once().await;
        core.pause(&k.job_id).await.unwrap();
        (t.job_id, k.job_id)
    };
    assert_no_secret_on_disk(dir.path());
    let core = stored(&db, dir.path(), vault.clone());
    core.recover().await;
    let t = core.get(&temp).await.unwrap();
    assert_eq!(t.auth.unwrap().state, CredentialState::Missing);
    let k = core.get(&kept).await.unwrap();
    assert_eq!(k.auth.as_ref().unwrap().state, CredentialState::Available);
    // The remembered one was restored with its session and the full (unsealed) link.
    let req = core
        .backend()
        .restored
        .lock()
        .unwrap()
        .last()
        .map(|(r, _)| r.clone())
        .expect("restored from its checkpoint");
    assert!(
        req.session.is_some(),
        "the restored attempt carries the sign-in"
    );
    assert!(
        req.url.contains(&format!("sig={SECRET}")),
        "the sealed link came back"
    );
    // The temporary one cannot start without a new browser action.
    let e = core.resume(&temp).await;
    let t = core.get(&temp).await.unwrap();
    assert!(
        e.is_err()
            || t.error
                .as_ref()
                .is_some_and(|e| e.next_action == NextAction::SignIn),
        "{e:?} {t:?}"
    );
    drop(core);
    // A locked store at the next start: the remembered one reports it and does not start.
    let locked = Arc::new(MemoryVault::default());
    *locked.entries.lock().unwrap() = vault.entries.lock().unwrap().clone();
    *locked.fail.lock().unwrap() = Some(VaultError::Locked);
    let core = stored(&db, dir.path(), locked);
    let k = core.get(&kept).await.unwrap();
    assert_eq!(k.auth.unwrap().state, CredentialState::Locked);
    assert_eq!(
        k.error.as_ref().map(|e| (e.kind, e.next_action)),
        Some((ErrorKind::CredentialStoreUnavailable, NextAction::Retry)),
        "a locked store asks for an unlock, not a new sign-in"
    );
    drop(core);
    // Unlocked again: the sealed link was kept through the locked start and the job goes on.
    let core = stored(&db, dir.path(), vault.clone());
    core.recover().await;
    let _ = core.resume(&kept).await;
    core.poll_once().await;
    let b = core.backend();
    let seen: Vec<String> = b
        .started
        .lock()
        .unwrap()
        .iter()
        .map(|r| r.url.clone())
        .chain(
            b.restored
                .lock()
                .unwrap()
                .iter()
                .map(|(r, _)| r.url.clone()),
        )
        .collect();
    assert!(
        seen.iter().any(|u| u.contains(&format!("sig={SECRET}"))),
        "the link survived the locked start: {seen:?}"
    );
}

#[tokio::test]
async fn forgetting_a_sign_in_in_use_pauses_its_download_and_empties_the_store() {
    let dir = tempfile::tempdir().unwrap();
    let vault = Arc::new(MemoryVault::default());
    let core = core_v(dir.path(), vault.clone());
    let s = core
        .add_authenticated(params("auth-forget-01", true))
        .await
        .unwrap();
    let id = s.auth.unwrap().credential_id;
    let list = core
        .forget_credential(ForgetCredentialParams {
            credential_id: id.clone(),
        })
        .await
        .unwrap();
    assert_eq!(list.credentials[0].state, CredentialState::Forgotten);
    assert!(!vault.entries.lock().unwrap().contains_key(&id));
    let after = core.get(&s.job_id).await.unwrap();
    assert_eq!(after.desired_state, DesiredState::Paused);
    assert_eq!(after.auth.unwrap().state, CredentialState::Forgotten);
    let before = core.backend().started.lock().unwrap().len();
    let _ = core.resume(&s.job_id).await;
    core.poll_once().await;
    assert_eq!(
        core.backend().started.lock().unwrap().len(),
        before,
        "never restarted without its sign-in"
    );
}

#[tokio::test]
async fn a_crash_between_store_and_database_is_cleaned_up_at_the_next_start() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("downova.sqlite3");
    let vault = Arc::new(MemoryVault::default());
    {
        let mut store = Store::open(&db).unwrap();
        store
            .upsert_credential(&dm_storage::CredentialRow {
                credential_id: "cred_00000000000000aa".into(),
                origin: ORIGIN.into(),
                browser: "chrome".into(),
                profile_id: "0123456789abcdef".into(),
                mode: "remembered".into(),
                state: "pending".into(),
                generation: 1,
                created_at_ms: 1,
                expires_at_ms: None,
            })
            .unwrap();
    }
    vault
        .put("cred_00000000000000aa", SECRET.as_bytes())
        .unwrap();
    let core = stored(&db, dir.path(), vault.clone());
    assert!(core.list_credentials().await.credentials.is_empty());
    assert!(
        !vault
            .entries
            .lock()
            .unwrap()
            .contains_key("cred_00000000000000aa")
    );
}

#[tokio::test]
async fn without_a_store_public_downloads_still_work_and_protected_links_stay_in_memory() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("downova.sqlite3");
    let off: Arc<dyn Vault> = Arc::new(dm_platform::vault::OffVault);
    let id = {
        let core = stored(&db, dir.path(), Arc::clone(&off));
        let s = core
            .add(add_params(
                "auth-off-00001",
                &format!("https://cdn.example.org/a.zip?token={SECRET}"),
            ))
            .await
            .unwrap();
        let req = core
            .backend()
            .started
            .lock()
            .unwrap()
            .last()
            .cloned()
            .unwrap();
        assert!(req.url.contains(SECRET), "works now, from memory");
        core.pause(&s.job_id).await.unwrap();
        s.job_id
    };
    assert_no_secret_on_disk(dir.path());
    let core = stored(&db, dir.path(), off);
    let s = core.get(&id).await.unwrap();
    assert_eq!(s.phase, Phase::Failed);
    assert_eq!(
        s.error.unwrap().next_action,
        NextAction::RefreshLink,
        "an explicit way on"
    );
}
