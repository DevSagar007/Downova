//! F05 (ADR 0036): site logins in the service. Passwords live only in the OS store, a matching
//! FTP download gets its server's login, other links never do, and a saved login whose password
//! cannot be read holds the download instead of trying it anonymously.

use super::*;
use dm_domain::redact::SecretTransit;
use dm_domain::site_login::{
    RemoveSiteLoginParams, SetSiteLoginParams, SiteLoginPasswordState, SiteLoginScheme,
};
use dm_platform::vault::{MemoryVault, Vault, VaultError};

const PASSWORD: &str = "SYNTHETIC-site-login-pw-5";

fn stored(db: &Path, dir: &Path, vault: Arc<dyn Vault>) -> Core<FakeBackend> {
    core_with(FakeBackend::with_all(CapabilitySupport::Supported), dir)
        .with_vault(vault)
        .with_store(Store::open(db).unwrap())
        .unwrap()
}

fn login(host: &str, password: Option<&str>) -> SetSiteLoginParams {
    SetSiteLoginParams {
        scheme: SiteLoginScheme::Ftp,
        host: host.into(),
        port: 21,
        username: "alice".into(),
        password: password.map(SecretTransit::new),
    }
}

/// The login handed to the backend with each started request, in order.
fn logins_started(core: &Core<FakeBackend>) -> Vec<Option<(String, String)>> {
    core.backend
        .started
        .lock()
        .unwrap()
        .iter()
        .map(|r| {
            r.login
                .as_ref()
                .map(|l| (l.username.clone(), l.password.expose().to_owned()))
        })
        .collect()
}

#[tokio::test]
async fn a_saved_login_is_used_for_its_server_only_and_survives_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("downova.sqlite3");
    let vault: Arc<dyn Vault> = Arc::new(MemoryVault::default());
    {
        let core = stored(&db, dir.path(), Arc::clone(&vault));
        let list = core
            .set_site_login(login("Files.Example.org", Some(PASSWORD)))
            .await
            .unwrap();
        assert_eq!(list.logins.len(), 1);
        assert_eq!(list.logins[0].host, "files.example.org");
        assert_eq!(list.logins[0].password_state, SiteLoginPasswordState::Saved);
        assert!(!serde_json::to_string(&list).unwrap().contains(PASSWORD));
        for (key, url) in [
            ("ext_login_001", "ftp://files.example.org/pub/a.iso"),
            ("ext_login_002", "ftp://other.example.org/b.iso"),
            ("ext_login_003", "https://files.example.org/c.iso"),
        ] {
            core.add(add_params(key, url)).await.unwrap();
        }
        assert_eq!(
            logins_started(&core),
            vec![Some(("alice".into(), PASSWORD.into())), None, None]
        );
    }
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
    let core = stored(&db, dir.path(), vault);
    core.add(add_params(
        "ext_login_004",
        "ftp://FILES.example.org:21/d.iso",
    ))
    .await
    .unwrap();
    assert_eq!(
        logins_started(&core),
        vec![Some(("alice".into(), PASSWORD.into()))]
    );
}

#[tokio::test]
async fn a_new_login_needs_a_password_and_a_refused_store_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("downova.sqlite3");
    let memory = Arc::new(MemoryVault::default());
    let vault: Arc<dyn Vault> = memory.clone();
    let core = stored(&db, dir.path(), vault);
    assert_eq!(
        core.set_site_login(login("h.example", None))
            .await
            .unwrap_err()
            .0
            .kind,
        ErrorKind::InvalidRequest
    );
    *memory.fail.lock().unwrap() = Some(VaultError::Locked);
    let err = core
        .set_site_login(login("h.example", Some(PASSWORD)))
        .await
        .unwrap_err();
    assert_eq!(err.0.kind, ErrorKind::CredentialStoreUnavailable);
    assert!(!format!("{:?}", err.0).contains(PASSWORD));
    assert!(core.list_site_logins().await.logins.is_empty());
}

#[tokio::test]
async fn a_locked_store_holds_downloads_from_that_server_until_the_password_can_be_read() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("downova.sqlite3");
    let memory = Arc::new(MemoryVault::default());
    let vault: Arc<dyn Vault> = memory.clone();
    {
        let core = stored(&db, dir.path(), Arc::clone(&vault));
        core.set_site_login(login("files.example.org", Some(PASSWORD)))
            .await
            .unwrap();
    }
    *memory.fail.lock().unwrap() = Some(VaultError::Locked);
    let core = stored(&db, dir.path(), Arc::clone(&vault));
    let held = core
        .add(add_params("ext_login_005", "ftp://files.example.org/a.iso"))
        .await
        .unwrap();
    assert_eq!(held.phase, Phase::Failed);
    assert_eq!(
        held.error.as_ref().unwrap().kind,
        ErrorKind::CredentialStoreUnavailable
    );
    assert!(logins_started(&core).is_empty(), "never tried anonymously");
    assert_eq!(
        core.list_site_logins().await.logins[0].password_state,
        SiteLoginPasswordState::Unavailable
    );
    // Other servers are unaffected.
    core.add(add_params("ext_login_006", "ftp://other.example.org/b.iso"))
        .await
        .unwrap();
    assert_eq!(logins_started(&core), vec![None]);
    // Unlocked: the next add reads it.
    *memory.fail.lock().unwrap() = None;
    core.add(add_params("ext_login_007", "ftp://files.example.org/c.iso"))
        .await
        .unwrap();
    assert_eq!(
        logins_started(&core).last().cloned().flatten(),
        Some(("alice".into(), PASSWORD.into()))
    );
}

#[tokio::test]
async fn a_removed_login_leaves_the_store_and_its_server_is_anonymous_again() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("downova.sqlite3");
    let vault: Arc<dyn Vault> = Arc::new(MemoryVault::default());
    let core = stored(&db, dir.path(), Arc::clone(&vault));
    let id = core
        .set_site_login(login("files.example.org", Some(PASSWORD)))
        .await
        .unwrap()
        .logins[0]
        .login_id
        .clone();
    // Changing the user name keeps the saved password.
    let mut renamed = login("files.example.org", None);
    renamed.username = "bob".into();
    core.set_site_login(renamed).await.unwrap();
    core.add(add_params("ext_login_008", "ftp://files.example.org/a.iso"))
        .await
        .unwrap();
    assert_eq!(
        logins_started(&core),
        vec![Some(("bob".into(), PASSWORD.into()))]
    );
    let list = core
        .remove_site_login(RemoveSiteLoginParams {
            login_id: id.clone(),
        })
        .await
        .unwrap();
    assert!(list.logins.is_empty());
    assert_eq!(
        vault.get(&format!("site-login-{id}")),
        Err(VaultError::Missing)
    );
    core.add(add_params("ext_login_009", "ftp://files.example.org/b.iso"))
        .await
        .unwrap();
    assert_eq!(logins_started(&core).last().cloned().flatten(), None);
    assert_eq!(
        core.remove_site_login(RemoveSiteLoginParams { login_id: id })
            .await
            .unwrap_err()
            .0
            .kind,
        ErrorKind::InvalidRequest
    );
}
