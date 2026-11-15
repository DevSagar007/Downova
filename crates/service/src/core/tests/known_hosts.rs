//! F05 (ADR 0037): SFTP in the service. Site logins and trusted host keys reach the engine, a key a
//! failed download observed can be trusted (only that key), and a changed key is never trusted
//! over the old one.

use super::*;
use crate::fake::progress;
use dm_domain::known_host::{KnownHostState, RemoveKnownHostParams, TrustHostKeyParams};
use dm_domain::redact::SecretTransit;
use dm_domain::site_login::{SetSiteLoginParams, SiteLoginScheme};
use dm_platform::vault::{MemoryVault, Vault};

const FP_A: &str = "SHA256:47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU";
const FP_B: &str = "SHA256:uNiVztksCsDhcc0u9e8BujQXVUpKZIDTMczCvj3tD2s";

fn stored(db: &Path, dir: &Path, vault: Arc<dyn Vault>) -> Core<FakeBackend> {
    core_with(FakeBackend::with_all(CapabilitySupport::Supported), dir)
        .with_vault(vault)
        .with_store(Store::open(db).unwrap())
        .unwrap()
}

/// Makes the latest started download fail with a host-key error naming `fp`.
async fn fail_with_key(
    core: &Core<FakeBackend>,
    snap: &DownloadSnapshot,
    kind: ErrorKind,
    fp: &str,
) {
    let handle = handle_of(snap);
    core.backend.set_progress(
        &handle,
        progress(
            TransferState::Failed(BackendError::new(
                kind,
                format!("this SFTP server's key is not trusted yet (server key ssh-ed25519 {fp})"),
            )),
            0,
            None,
        ),
    );
    core.poll_once().await;
}

fn last_started(core: &Core<FakeBackend>) -> TransferRequest {
    core.backend
        .started
        .lock()
        .unwrap()
        .last()
        .cloned()
        .unwrap()
}

fn trust(fp: &str) -> TrustHostKeyParams {
    TrustHostKeyParams {
        host: "files.example.org".into(),
        port: 22,
        fingerprint: fp.into(),
    }
}

#[tokio::test]
async fn an_observed_key_can_be_trusted_and_then_reaches_the_engine_after_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("downova.sqlite3");
    let vault: Arc<dyn Vault> = Arc::new(MemoryVault::default());
    {
        let core = stored(&db, dir.path(), Arc::clone(&vault));
        core.set_site_login(SetSiteLoginParams {
            scheme: SiteLoginScheme::Sftp,
            host: "files.example.org".into(),
            port: 22,
            username: "alice".into(),
            password: Some(SecretTransit::new("SYNTHETIC-sftp-pw")),
        })
        .await
        .unwrap();
        let snap = core
            .add(add_params(
                "ext_sftp_0001",
                "sftp://files.example.org/srv/a.iso",
            ))
            .await
            .unwrap();
        let first = last_started(&core);
        assert_eq!(first.login.as_ref().unwrap().username, "alice");
        assert!(first.trusted_host_keys.is_empty());

        fail_with_key(&core, &snap, ErrorKind::HostKeyUnknown, FP_A).await;
        let list = core.list_known_hosts().await;
        assert_eq!(list.hosts.len(), 1);
        assert_eq!(list.hosts[0].state, KnownHostState::Pending);
        assert_eq!(list.hosts[0].fingerprint, FP_A);

        // Only the observed key can be trusted.
        assert_eq!(
            core.trust_host_key(trust(FP_B)).await.unwrap_err().0.kind,
            ErrorKind::InvalidRequest
        );
        let list = core.trust_host_key(trust(FP_A)).await.unwrap();
        assert_eq!(list.hosts.len(), 1);
        assert_eq!(list.hosts[0].state, KnownHostState::Trusted);
    }
    let core = stored(&db, dir.path(), vault);
    core.add(add_params(
        "ext_sftp_0002",
        "sftp://FILES.example.org:22/srv/b.iso",
    ))
    .await
    .unwrap();
    let request = last_started(&core);
    assert_eq!(request.trusted_host_keys, vec![FP_A.to_owned()]);
    assert_eq!(
        request.login.unwrap().password.expose(),
        "SYNTHETIC-sftp-pw"
    );
    // Other links never carry host keys.
    core.add(add_params("ext_sftp_0003", "ftp://files.example.org/a.iso"))
        .await
        .unwrap();
    assert!(last_started(&core).trusted_host_keys.is_empty());
}

#[tokio::test]
async fn a_changed_key_is_never_trusted_over_the_old_one() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("downova.sqlite3");
    let core = stored(&db, dir.path(), Arc::new(MemoryVault::default()));
    let snap = core
        .add(add_params(
            "ext_sftp_0004",
            "sftp://files.example.org/a.iso",
        ))
        .await
        .unwrap();
    fail_with_key(&core, &snap, ErrorKind::HostKeyUnknown, FP_A).await;
    core.trust_host_key(trust(FP_A)).await.unwrap();

    let snap = core
        .add(add_params(
            "ext_sftp_0005",
            "sftp://files.example.org/b.iso",
        ))
        .await
        .unwrap();
    fail_with_key(&core, &snap, ErrorKind::HostKeyChanged, FP_B).await;
    let list = core.list_known_hosts().await;
    assert!(
        list.hosts
            .iter()
            .any(|h| h.fingerprint == FP_A && h.state == KnownHostState::Trusted)
    );
    assert!(
        list.hosts
            .iter()
            .any(|h| h.fingerprint == FP_B && h.state == KnownHostState::Pending)
    );
    let refused = core.trust_host_key(trust(FP_B)).await.unwrap_err();
    assert_eq!(refused.0.kind, ErrorKind::InvalidRequest);
    assert!(format!("{:?}", refused.0).contains("remove it first"));

    // Removing the old key on purpose, then trusting the new one.
    let list = core
        .remove_known_host(RemoveKnownHostParams {
            host: "files.example.org".into(),
            port: 22,
        })
        .await
        .unwrap();
    assert!(
        list.hosts
            .iter()
            .all(|h| h.state == KnownHostState::Pending)
    );
    core.trust_host_key(trust(FP_B)).await.unwrap();
    core.add(add_params(
        "ext_sftp_0006",
        "sftp://files.example.org/c.iso",
    ))
    .await
    .unwrap();
    assert_eq!(last_started(&core).trusted_host_keys, vec![FP_B.to_owned()]);
}

#[tokio::test]
async fn sftp_links_with_credentials_or_without_the_capability_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let core = core_with(
        FakeBackend::with_all(CapabilitySupport::Supported),
        dir.path(),
    );
    let err = core
        .add(add_params(
            "ext_sftp_0007",
            "sftp://u:pw@files.example.org/a.iso",
        ))
        .await
        .unwrap_err();
    assert_eq!(err.0.kind, ErrorKind::InvalidRequest);
    let no_sftp = core_with(
        FakeBackend::supported_except(Capability::Sftp, CapabilitySupport::Unsupported),
        dir.path(),
    );
    let err = no_sftp
        .add(add_params(
            "ext_sftp_0008",
            "sftp://files.example.org/a.iso",
        ))
        .await
        .unwrap_err();
    assert_eq!(err.0.kind, ErrorKind::BackendCapabilityUnavailable);
}
