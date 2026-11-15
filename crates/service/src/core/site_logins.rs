//! Site logins (F05, ADR 0036): one user name and password per FTP server.
//!
//! - The list (`settings.site_logins`) holds no password; each password is only in the OS
//!   credential store under `site-login-<login_id>`.
//! - Passwords the service could read are kept in memory, so building a request never calls the
//!   store. Store calls are bounded (ADR 0032) and never made under the coordinator lock.
//! - A download from a server whose saved password cannot be read now is held
//!   ([`Core::site_login_gate`]) rather than tried anonymously; the password is read again at user
//!   entry points ([`Core::refresh_site_logins`]).

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, MutexGuard};

use dm_backend_api::{DownloadBackend, Login};
use dm_domain::redact::Secret;
use dm_domain::site_login::{
    FTP_PORT, ListSiteLoginsResult, MAX_SITE_LOGINS, RemoveSiteLoginParams, SetSiteLoginParams,
    SiteLogin, SiteLoginPasswordState, SiteLoginScheme, site_login_id, valid_site_login_id,
};
use dm_domain::{ErrorKind, NextAction};
use dm_platform::vault::{Vault, VaultError};
use dm_storage::{SiteLoginRow, Store};

use super::{Core, ServiceError, credentials};

fn vault_id(login_id: &str) -> String {
    format!("site-login-{login_id}")
}

#[derive(Debug, Default)]
pub(crate) struct SiteLoginsState {
    rows: Vec<SiteLoginRow>,
    passwords: HashMap<String, Secret>,
    /// Saved, but the store could not give the password when last asked.
    unavailable: HashSet<String>,
    /// The saved list could not be read: every FTP download is held until it is saved again.
    damaged: bool,
}

/// One bounded read of a saved password. `Ok(None)`: not in the store.
fn read_password(
    vault: &Arc<dyn Vault>,
    wait: std::time::Duration,
    login_id: &str,
) -> Result<Option<Secret>, ()> {
    let id = vault_id(login_id);
    match credentials::bounded(vault, wait, move |v| v.get(&id)) {
        Some(Ok(bytes)) => String::from_utf8(bytes)
            .map(|s| Some(Secret::new(s)))
            .map_err(|_| ()),
        Some(Err(VaultError::Missing)) => Ok(None),
        _ => Err(()),
    }
}

/// Reads each id's password; after the first failure the rest are not tried (a locked or silent
/// store would only make every read wait in turn).
fn read_all(
    vault: &Arc<dyn Vault>,
    wait: std::time::Duration,
    ids: &[String],
) -> (HashMap<String, Secret>, HashSet<String>) {
    let mut found = HashMap::new();
    let mut unavailable = HashSet::new();
    let mut failed = false;
    for id in ids {
        if failed {
            unavailable.insert(id.clone());
            continue;
        }
        match read_password(vault, wait, id) {
            Ok(Some(p)) => {
                found.insert(id.clone(), p);
            }
            // A row without a stored password: treated like an unreadable one (held, never
            // anonymous), until the login is saved again.
            Ok(None) => {
                unavailable.insert(id.clone());
            }
            Err(()) => {
                failed = true;
                unavailable.insert(id.clone());
            }
        }
    }
    (found, unavailable)
}

fn store_refused(what: &str, e: Option<&VaultError>) -> ServiceError {
    let detail = match e {
        Some(e) => format!("the site login password could not be {what}: {e}"),
        None => format!(
            "the site login password could not be {what}: the system credential store did not answer in time"
        ),
    };
    ServiceError::new(ErrorKind::CredentialStoreUnavailable, detail).with_action(NextAction::Retry)
}

/// `ftp`, lower-case host and port of an `ftp://` link; `None` for any other link.
fn ftp_server(url: &str) -> Option<(String, u16)> {
    let rest = url
        .get(..6)?
        .eq_ignore_ascii_case("ftp://")
        .then(|| url.get(6..))??;
    let authority = rest.split(['/', '?', '#']).next()?;
    if authority.contains('@') || authority.is_empty() {
        return None;
    }
    let (host, port) = if let Some(end) = authority.strip_prefix('[').and_then(|a| a.find(']')) {
        // [IPv6]:port
        let host = authority.get(..end.saturating_add(2))?;
        let port = authority.get(end.saturating_add(2)..)?;
        (host, port.strip_prefix(':'))
    } else {
        match authority.rsplit_once(':') {
            Some((h, p)) => (h, Some(p)),
            None => (authority, None),
        }
    };
    let port = match port {
        Some(p) => p.parse().ok()?,
        None => FTP_PORT,
    };
    Some((host.to_ascii_lowercase(), port))
}

impl<B: DownloadBackend> Core<B> {
    fn site_logins(&self) -> MutexGuard<'_, SiteLoginsState> {
        self.site_logins
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Startup (from `with_store`, before any recovered transfer starts).
    pub(super) fn restore_site_logins(&self, store: &Store) {
        let rows = match store.load_site_logins() {
            Ok(rows) => rows,
            Err(e) => {
                eprintln!("dm-service: the saved site logins are unreadable: {e}");
                self.site_logins().damaged = true;
                return;
            }
        };
        let ids: Vec<String> = rows.iter().map(|r| r.login_id.clone()).collect();
        let (passwords, unavailable) = read_all(&self.vault, self.vault_wait, &ids);
        *self.site_logins() = SiteLoginsState {
            rows,
            passwords,
            unavailable,
            damaged: false,
        };
    }

    /// Reads passwords that could not be read before (bounded, outside every lock).
    pub async fn refresh_site_logins(&self) {
        let ids: Vec<String> = self.site_logins().unavailable.iter().cloned().collect();
        if ids.is_empty() {
            return;
        }
        let vault = Arc::clone(&self.vault);
        let wait = self.vault_wait;
        let Ok((found, still)) =
            tokio::task::spawn_blocking(move || read_all(&vault, wait, &ids)).await
        else {
            return;
        };
        let mut s = self.site_logins();
        for (id, p) in found {
            // Only for logins still in the list (one may have been removed meanwhile).
            if s.rows.iter().any(|r| r.login_id == id) {
                s.unavailable.remove(&id);
                s.passwords.insert(id, p);
            }
        }
        s.unavailable.retain(|id| still.contains(id));
    }

    /// The site login for an `ftp://` link, when one is saved and its password is known.
    pub(super) fn login_for(&self, url: &str) -> Option<Login> {
        let (host, port) = ftp_server(url)?;
        let id = site_login_id(SiteLoginScheme::Ftp, &host, port);
        let s = self.site_logins();
        let row = s.rows.iter().find(|r| r.login_id == id)?;
        let password = s.passwords.get(&id)?.clone();
        Some(Login {
            username: row.username.clone(),
            password,
        })
    }

    /// Holds a start while the server's saved login cannot be used (flags only: safe under the
    /// lock). A server without a saved login is anonymous (ADR 0035).
    pub(super) fn site_login_gate(&self, url: &str) -> Result<(), ServiceError> {
        let Some((host, port)) = ftp_server(url) else {
            return Ok(());
        };
        let s = self.site_logins();
        if s.damaged {
            return Err(ServiceError::new(
                ErrorKind::Internal,
                "the saved site logins are damaged; save the site logins again",
            ));
        }
        let id = site_login_id(SiteLoginScheme::Ftp, &host, port);
        if s.unavailable.contains(&id) {
            return Err(ServiceError::new(
                ErrorKind::CredentialStoreUnavailable,
                "this server's site login password is in the system credential store, which is locked or not answering; unlock it, then try again",
            )
            .with_action(NextAction::Retry));
        }
        Ok(())
    }

    fn site_logins_snapshot(&self) -> ListSiteLoginsResult {
        let s = self.site_logins();
        ListSiteLoginsResult {
            logins: s
                .rows
                .iter()
                .map(|r| SiteLogin {
                    login_id: r.login_id.clone(),
                    scheme: r.scheme,
                    host: r.host.clone(),
                    port: r.port,
                    username: r.username.clone(),
                    password_state: if s.passwords.contains_key(&r.login_id) {
                        SiteLoginPasswordState::Saved
                    } else {
                        SiteLoginPasswordState::Unavailable
                    },
                })
                .collect(),
        }
    }

    /// `site_logins.v1.list`.
    pub async fn list_site_logins(&self) -> ListSiteLoginsResult {
        self.refresh_site_logins().await;
        self.site_logins_snapshot()
    }

    /// `site_logins.v1.set`: validate, write the password to the OS store, then save the list.
    /// A refused step changes nothing.
    pub async fn set_site_login(
        &self,
        params: SetSiteLoginParams,
    ) -> Result<ListSiteLoginsResult, ServiceError> {
        params
            .validate()
            .map_err(|why| ServiceError::new(ErrorKind::InvalidRequest, why))?;
        let host = params.host.to_ascii_lowercase();
        let id = site_login_id(params.scheme, &host, params.port);
        let (exists, count) = {
            let s = self.site_logins();
            (s.rows.iter().any(|r| r.login_id == id), s.rows.len())
        };
        if !exists && count >= MAX_SITE_LOGINS {
            return Err(ServiceError::new(
                ErrorKind::InvalidRequest,
                "at most 100 site logins can be saved",
            ));
        }
        let new_password = match params.password {
            Some(p) => {
                let secret = p.into_secret();
                let bytes = secret.expose().as_bytes().to_vec();
                let vault = Arc::clone(&self.vault);
                let wait = self.vault_wait;
                let key = vault_id(&id);
                let put = tokio::task::spawn_blocking(move || {
                    credentials::bounded(&vault, wait, move |v| v.put(&key, &bytes))
                })
                .await
                .ok()
                .flatten();
                match put {
                    Some(Ok(())) => Some(secret),
                    Some(Err(e)) => return Err(store_refused("saved", Some(&e))),
                    None => return Err(store_refused("saved", None)),
                }
            }
            None if exists => None,
            None => {
                return Err(ServiceError::new(
                    ErrorKind::InvalidRequest,
                    "a new site login needs a password",
                ));
            }
        };
        let row = SiteLoginRow {
            login_id: id.clone(),
            scheme: params.scheme,
            host,
            port: params.port,
            username: params.username,
        };
        let mut rows = self.site_logins().rows.clone();
        match rows.iter_mut().find(|r| r.login_id == id) {
            Some(existing) => *existing = row,
            None => rows.push(row),
        }
        let mut guard = self.state.lock().await;
        if let Some(store) = guard.store.as_mut() {
            store.save_site_logins(&rows).map_err(|e| {
                eprintln!("dm-service: site logins not saved: {e}");
                ServiceError::new(ErrorKind::Internal, "the site login could not be saved")
            })?;
        }
        {
            let mut s = self.site_logins();
            s.rows = rows;
            s.damaged = false;
            if let Some(p) = new_password {
                s.unavailable.remove(&id);
                s.passwords.insert(id, p);
            }
        }
        drop(guard);
        Ok(self.site_logins_snapshot())
    }

    /// `site_logins.v1.remove`: the row goes first, then the stored password (a failure there is
    /// logged; the password is no longer used).
    pub async fn remove_site_login(
        &self,
        params: RemoveSiteLoginParams,
    ) -> Result<ListSiteLoginsResult, ServiceError> {
        if !valid_site_login_id(&params.login_id) {
            return Err(ServiceError::new(
                ErrorKind::InvalidRequest,
                "invalid login id",
            ));
        }
        let mut rows = self.site_logins().rows.clone();
        let before = rows.len();
        rows.retain(|r| r.login_id != params.login_id);
        if rows.len() == before {
            return Err(ServiceError::new(
                ErrorKind::InvalidRequest,
                "no such site login",
            ));
        }
        let mut guard = self.state.lock().await;
        if let Some(store) = guard.store.as_mut() {
            store.save_site_logins(&rows).map_err(|e| {
                eprintln!("dm-service: site logins not saved: {e}");
                ServiceError::new(ErrorKind::Internal, "the site login could not be removed")
            })?;
        }
        {
            let mut s = self.site_logins();
            s.rows = rows;
            s.passwords.remove(&params.login_id);
            s.unavailable.remove(&params.login_id);
        }
        drop(guard);
        let vault = Arc::clone(&self.vault);
        let wait = self.vault_wait;
        let key = vault_id(&params.login_id);
        let deleted = tokio::task::spawn_blocking(move || {
            credentials::bounded(&vault, wait, move |v| v.delete(&key))
        })
        .await
        .ok()
        .flatten();
        if !matches!(deleted, Some(Ok(()))) {
            eprintln!(
                "dm-service: a removed site login's password is still in the credential store"
            );
        }
        Ok(self.site_logins_snapshot())
    }
}

#[cfg(test)]
mod unit {
    use super::ftp_server;

    #[test]
    fn servers_are_read_from_ftp_links_only() {
        assert_eq!(
            ftp_server("ftp://Files.Example.org/pub/a.iso"),
            Some(("files.example.org".into(), 21))
        );
        assert_eq!(ftp_server("FTP://h:2121/a"), Some(("h".into(), 2121)));
        assert_eq!(
            ftp_server("ftp://[2001:db8::1]:2121/a"),
            Some(("[2001:db8::1]".into(), 2121))
        );
        assert_eq!(
            ftp_server("ftp://[2001:db8::1]/a"),
            Some(("[2001:db8::1]".into(), 21))
        );
        assert_eq!(ftp_server("https://h/a"), None);
        assert_eq!(ftp_server("ftp://u:p@h/a"), None);
        assert_eq!(ftp_server("ftp://h:notaport/a"), None);
    }
}
