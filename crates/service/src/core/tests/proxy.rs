//! F06 (ADR 0034): the proxy setting in the service. The password lives only in the OS store, a
//! restart applies the saved proxy before anything starts, and nothing starts while the saved
//! proxy is not in effect.

use super::*;
use dm_domain::proxy::{
    PasswordChange, ProxyCarries, ProxyMode, ProxyPasswordState, ProxySettings, SetProxyParams,
};
use dm_domain::redact::SecretTransit;
use dm_platform::vault::{MemoryVault, Vault, VaultError};

const PASSWORD: &str = "SYNTHETIC-proxy-pw-77";

fn stored(db: &Path, dir: &Path, fake: FakeBackend, vault: Arc<dyn Vault>) -> Core<FakeBackend> {
    core_with(fake, dir)
        .with_vault(vault)
        .with_store(Store::open(db).unwrap())
        .unwrap()
}

fn http(password: PasswordChange) -> SetProxyParams {
    SetProxyParams {
        mode: ProxyMode::Http,
        host: Some("proxy.example.test".into()),
        port: Some(3128),
        username: Some("alice".into()),
        password,
    }
}

fn last_proxy(core: &Core<FakeBackend>) -> Option<dm_backend_api::ProxyConfig> {
    core.backend
        .proxies
        .lock()
        .unwrap()
        .last()
        .cloned()
        .flatten()
}

fn starts(core: &Core<FakeBackend>) -> usize {
    core.backend
        .calls
        .lock()
        .unwrap()
        .iter()
        .filter(|c| c.starts_with("start"))
        .count()
}

fn all() -> FakeBackend {
    FakeBackend::with_all(CapabilitySupport::Supported)
}

#[tokio::test]
async fn a_saved_proxy_keeps_its_password_only_in_the_store_and_comes_back_after_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("downova.sqlite3");
    let vault: Arc<dyn Vault> = Arc::new(MemoryVault::default());
    {
        let core = stored(&db, dir.path(), all(), Arc::clone(&vault));
        assert_eq!(core.proxy_settings().await, ProxySettings::default());
        let got = core
            .set_proxy(http(PasswordChange::Replace(SecretTransit::new(PASSWORD))))
            .await
            .unwrap();
        assert_eq!(got.mode, ProxyMode::Http);
        assert_eq!(got.password_state, ProxyPasswordState::Saved);
        assert_eq!(got.carries, vec![ProxyCarries::Http, ProxyCarries::Https]);
        let applied = last_proxy(&core).unwrap();
        assert_eq!(applied.password.unwrap().expose(), PASSWORD);
        assert!(!serde_json::to_string(&got).unwrap().contains(PASSWORD));
        // Keeping the password on a re-save; the engine still gets it.
        let mut keep = http(PasswordChange::Keep);
        keep.port = Some(8080);
        let got = core.set_proxy(keep).await.unwrap();
        assert_eq!(got.port, Some(8080));
        assert_eq!(got.password_state, ProxyPasswordState::Saved);
        assert!(last_proxy(&core).unwrap().password.is_some());
    }
    // Nothing in the database file holds the password.
    for entry in std::fs::read_dir(dir.path()).unwrap() {
        let path = entry.unwrap().path();
        if path.is_file() {
            let bytes = std::fs::read(&path).unwrap();
            assert!(
                !bytes
                    .windows(PASSWORD.len())
                    .any(|w| w == PASSWORD.as_bytes()),
                "{}",
                path.display()
            );
        }
    }
    assert_eq!(vault.get("proxy-password").unwrap(), PASSWORD.as_bytes());
    // A restart applies it (with the password) before anything starts.
    let core = stored(&db, dir.path(), all(), Arc::clone(&vault));
    let applied = last_proxy(&core).unwrap();
    assert_eq!(
        (applied.host.as_str(), applied.port),
        ("proxy.example.test", 8080)
    );
    assert_eq!(applied.password.unwrap().expose(), PASSWORD);
    // Removing the password deletes it from the store; direct mode keeps the fields.
    let mut direct = http(PasswordChange::Remove);
    direct.mode = ProxyMode::Direct;
    let got = core.set_proxy(direct).await.unwrap();
    assert_eq!(got.mode, ProxyMode::Direct);
    assert_eq!(got.host.as_deref(), Some("proxy.example.test"));
    assert_eq!(got.password_state, ProxyPasswordState::None);
    assert!(got.carries.is_empty());
    assert_eq!(vault.get("proxy-password"), Err(VaultError::Missing));
    assert!(last_proxy(&core).is_none(), "direct mode hands no proxy");
}

#[tokio::test]
async fn a_store_that_refuses_the_password_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("downova.sqlite3");
    let memory = Arc::new(MemoryVault::default());
    let vault: Arc<dyn Vault> = memory.clone();
    let core = stored(&db, dir.path(), all(), vault);
    *memory.fail.lock().unwrap() = Some(VaultError::Locked);
    let err = core
        .set_proxy(http(PasswordChange::Replace(SecretTransit::new(PASSWORD))))
        .await
        .unwrap_err();
    assert_eq!(err.0.kind, ErrorKind::CredentialStoreUnavailable);
    assert!(!format!("{:?}", err.0).contains(PASSWORD));
    assert_eq!(core.proxy_settings().await, ProxySettings::default());
    assert!(core.backend.proxies.lock().unwrap().is_empty());
    // Invalid input is refused before any store call.
    let mut no_user = http(PasswordChange::Replace(SecretTransit::new(PASSWORD)));
    no_user.username = None;
    assert_eq!(
        core.set_proxy(no_user).await.unwrap_err().0.kind,
        ErrorKind::InvalidRequest
    );
}

#[tokio::test]
async fn a_locked_store_at_startup_holds_every_start_until_the_password_can_be_read() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("downova.sqlite3");
    let memory = Arc::new(MemoryVault::default());
    let vault: Arc<dyn Vault> = memory.clone();
    {
        let core = stored(&db, dir.path(), all(), Arc::clone(&vault));
        core.set_proxy(http(PasswordChange::Replace(SecretTransit::new(PASSWORD))))
            .await
            .unwrap();
    }
    *memory.fail.lock().unwrap() = Some(VaultError::Locked);
    let core = stored(&db, dir.path(), all(), Arc::clone(&vault));
    // The engine has the proxy (never direct), but without the password.
    assert!(last_proxy(&core).unwrap().password.is_none());
    assert_eq!(
        core.proxy_settings().await.password_state,
        ProxyPasswordState::Unavailable
    );
    let held = core
        .add(add_params("ext_proxy_0001", "http://h/a.bin"))
        .await
        .unwrap();
    assert_eq!(starts(&core), 0, "started without the proxy password");
    assert_eq!(held.phase, Phase::Failed);
    assert_eq!(
        held.error.as_ref().unwrap().kind,
        ErrorKind::CredentialStoreUnavailable
    );
    // Unlocked: the next user action reads it and the download starts through the proxy.
    *memory.fail.lock().unwrap() = None;
    let started = core
        .add(add_params("ext_proxy_0002", "http://h/b.bin"))
        .await
        .unwrap();
    assert_ne!(started.phase, Phase::Failed);
    assert_eq!(starts(&core), 1);
    assert_eq!(
        last_proxy(&core).unwrap().password.unwrap().expose(),
        PASSWORD
    );
}

#[tokio::test]
async fn an_engine_without_proxy_support_refuses_the_setting_and_never_goes_direct() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("downova.sqlite3");
    let vault: Arc<dyn Vault> = Arc::new(MemoryVault::default());
    let no_proxy =
        || FakeBackend::supported_except(Capability::HttpProxy, CapabilitySupport::Unsupported);
    {
        let core = stored(&db, dir.path(), no_proxy(), Arc::clone(&vault));
        let err = core
            .set_proxy(http(PasswordChange::Keep))
            .await
            .unwrap_err();
        assert_eq!(err.0.kind, ErrorKind::BackendCapabilityUnavailable);
        // Direct mode is always accepted.
        let mut direct = http(PasswordChange::Keep);
        direct.mode = ProxyMode::Direct;
        core.set_proxy(direct).await.unwrap();
    }
    // A proxy saved with the default engine, then the service runs with one that has none.
    {
        let core = stored(&db, dir.path(), all(), Arc::clone(&vault));
        core.set_proxy(http(PasswordChange::Keep)).await.unwrap();
    }
    let core = stored(&db, dir.path(), no_proxy(), vault);
    let held = core
        .add(add_params("ext_proxy_0003", "http://h/c.bin"))
        .await
        .unwrap();
    assert_eq!(
        starts(&core),
        0,
        "a direct connection instead of the saved proxy"
    );
    assert_eq!(
        held.error.as_ref().unwrap().kind,
        ErrorKind::BackendCapabilityUnavailable
    );
    assert!(core.proxy_settings().await.carries.is_empty());
}

#[tokio::test]
async fn a_damaged_proxy_row_holds_every_start() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("downova.sqlite3");
    {
        let store = Store::open(&db).unwrap();
        drop(store);
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('proxy', '{\"mode\":\"http\"}')",
            [],
        )
        .unwrap();
    }
    let core = stored(&db, dir.path(), all(), Arc::new(MemoryVault::default()));
    let held = core
        .add(add_params("ext_proxy_0004", "http://h/d.bin"))
        .await
        .unwrap();
    assert_eq!(starts(&core), 0);
    assert_eq!(held.phase, Phase::Failed);
}
