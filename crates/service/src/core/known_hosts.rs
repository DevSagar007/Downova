//! Known SFTP servers (F05, ADR 0037).
//!
//! - **Trusted** host keys are saved in `settings.known_hosts` and handed to the engine with every
//!   `sftp://` request ([`Core::trusted_keys_for`]); the engine compares the server's key with
//!   them before any password is sent.
//! - **Pending** keys are not stored separately: they are the keys named by downloads that failed
//!   with `HOST_KEY_UNKNOWN` or `HOST_KEY_CHANGED` (their error detail, which survives a restart).
//! - A key is trusted only when it equals a pending one the service observed, and never over a
//!   different trusted key: a changed key needs the old one removed first.

use std::sync::MutexGuard;

use dm_backend_api::DownloadBackend;
use dm_domain::ErrorKind;
use dm_domain::known_host::{
    KnownHost, KnownHostState, ListKnownHostsResult, MAX_KNOWN_HOSTS, RemoveKnownHostParams,
    TrustHostKeyParams, presented_key,
};
use dm_domain::site_login::SiteLoginScheme;
use dm_storage::{KnownHostRow, Store};

use super::site_logins::login_server;
use super::{Core, ServiceError, State};

#[derive(Debug, Default)]
pub(crate) struct KnownHostsState {
    rows: Vec<KnownHostRow>,
    /// The saved list could not be read: every SFTP download is held until it is saved again.
    damaged: bool,
}

/// The lower-case host and port of an `sftp://` link.
fn sftp_server(url: &str) -> Option<(String, u16)> {
    match login_server(url)? {
        (SiteLoginScheme::Sftp, host, port) => Some((host, port)),
        _ => None,
    }
}

impl<B: DownloadBackend> Core<B> {
    fn known_hosts(&self) -> MutexGuard<'_, KnownHostsState> {
        self.known_hosts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Startup (from `with_store`).
    pub(super) fn restore_known_hosts(&self, store: &Store) {
        match store.load_known_hosts() {
            Ok(rows) => {
                *self.known_hosts() = KnownHostsState {
                    rows,
                    damaged: false,
                };
            }
            Err(e) => {
                eprintln!("dm-service: the saved known servers are unreadable: {e}");
                self.known_hosts().damaged = true;
            }
        }
    }

    /// The fingerprints trusted for an `sftp://` link's server; empty for any other link.
    pub(super) fn trusted_keys_for(&self, url: &str) -> Vec<String> {
        let Some((host, port)) = sftp_server(url) else {
            return Vec::new();
        };
        self.known_hosts()
            .rows
            .iter()
            .filter(|r| r.host == host && r.port == port)
            .map(|r| r.fingerprint.clone())
            .collect()
    }

    /// Holds SFTP downloads while the saved list is damaged (never "nothing trusted" by accident).
    pub(super) fn known_hosts_gate(&self, url: &str) -> Result<(), ServiceError> {
        if sftp_server(url).is_some() && self.known_hosts().damaged {
            return Err(ServiceError::new(
                ErrorKind::Internal,
                "the saved known servers are damaged; trust the server again",
            ));
        }
        Ok(())
    }

    /// Trusted rows plus the keys failed downloads observed (pending), without duplicates.
    fn known_hosts_snapshot(&self, state: &State) -> ListKnownHostsResult {
        let rows = self.known_hosts().rows.clone();
        let mut hosts: Vec<KnownHost> = rows
            .iter()
            .map(|r| KnownHost {
                host: r.host.clone(),
                port: r.port,
                algorithm: r.algorithm.clone(),
                fingerprint: r.fingerprint.clone(),
                state: KnownHostState::Trusted,
            })
            .collect();
        for job in &state.jobs {
            let Some(error) = &job.error else { continue };
            if !matches!(
                error.kind,
                ErrorKind::HostKeyUnknown | ErrorKind::HostKeyChanged
            ) {
                continue;
            }
            let (Some((host, port)), Some((algorithm, fingerprint))) = (
                sftp_server(&job.url),
                error.detail.as_deref().and_then(presented_key),
            ) else {
                continue;
            };
            let known = hosts
                .iter()
                .any(|h| h.host == host && h.port == port && h.fingerprint == fingerprint);
            if !known {
                hosts.push(KnownHost {
                    host,
                    port,
                    algorithm,
                    fingerprint,
                    state: KnownHostState::Pending,
                });
            }
        }
        ListKnownHostsResult { hosts }
    }

    /// `known_hosts.v1.list`.
    pub async fn list_known_hosts(&self) -> ListKnownHostsResult {
        let state = self.state.lock().await;
        self.known_hosts_snapshot(&state)
    }

    /// `known_hosts.v1.trust`: only a key a download actually observed for that server, and never
    /// over a different trusted key.
    pub async fn trust_host_key(
        &self,
        params: TrustHostKeyParams,
    ) -> Result<ListKnownHostsResult, ServiceError> {
        params
            .validate()
            .map_err(|why| ServiceError::new(ErrorKind::InvalidRequest, why))?;
        let host = params.host.to_ascii_lowercase();
        let mut guard = self.state.lock().await;
        let listed = self.known_hosts_snapshot(&guard);
        let same_server = |h: &&KnownHost| h.host == host && h.port == params.port;
        if listed
            .hosts
            .iter()
            .filter(same_server)
            .any(|h| h.state == KnownHostState::Trusted && h.fingerprint != params.fingerprint)
        {
            return Err(ServiceError::new(
                ErrorKind::InvalidRequest,
                "a different key is trusted for this server; remove it first if the change is expected",
            ));
        }
        if listed
            .hosts
            .iter()
            .filter(same_server)
            .any(|h| h.state == KnownHostState::Trusted)
        {
            return Ok(listed);
        }
        let Some(pending) = listed
            .hosts
            .iter()
            .filter(same_server)
            .find(|h| h.state == KnownHostState::Pending && h.fingerprint == params.fingerprint)
            .cloned()
        else {
            return Err(ServiceError::new(
                ErrorKind::InvalidRequest,
                "this key was not presented by the server; try the download again to see its key",
            ));
        };
        let mut rows = self.known_hosts().rows.clone();
        if rows.len() >= MAX_KNOWN_HOSTS {
            return Err(ServiceError::new(
                ErrorKind::InvalidRequest,
                "at most 200 servers can be trusted",
            ));
        }
        rows.push(KnownHostRow {
            host: pending.host,
            port: pending.port,
            algorithm: pending.algorithm,
            fingerprint: pending.fingerprint,
        });
        if let Some(store) = guard.store.as_mut() {
            store.save_known_hosts(&rows).map_err(|e| {
                eprintln!("dm-service: known servers not saved: {e}");
                ServiceError::new(ErrorKind::Internal, "the server key could not be saved")
            })?;
        }
        {
            let mut k = self.known_hosts();
            k.rows = rows;
            k.damaged = false;
        }
        Ok(self.known_hosts_snapshot(&guard))
    }

    /// `known_hosts.v1.remove`: forgets the server's trusted key.
    pub async fn remove_known_host(
        &self,
        params: RemoveKnownHostParams,
    ) -> Result<ListKnownHostsResult, ServiceError> {
        params
            .validate()
            .map_err(|why| ServiceError::new(ErrorKind::InvalidRequest, why))?;
        let host = params.host.to_ascii_lowercase();
        let mut guard = self.state.lock().await;
        let mut rows = self.known_hosts().rows.clone();
        rows.retain(|r| !(r.host == host && r.port == params.port));
        if let Some(store) = guard.store.as_mut() {
            store.save_known_hosts(&rows).map_err(|e| {
                eprintln!("dm-service: known servers not saved: {e}");
                ServiceError::new(ErrorKind::Internal, "the server key could not be removed")
            })?;
        }
        {
            let mut k = self.known_hosts();
            k.rows = rows;
            k.damaged = false;
        }
        Ok(self.known_hosts_snapshot(&guard))
    }
}
