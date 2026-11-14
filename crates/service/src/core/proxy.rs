//! F06 basic HTTP proxy (ADR 0034): the saved setting, its password in the OS credential store,
//! and the rule that nothing starts while the saved proxy is not in effect.
//!
//! - The row (`settings.proxy`) holds mode, host, port, user name and whether a password is saved;
//!   the password is only in the store under [`PASSWORD_ID`].
//! - Store calls are bounded (ADR 0032) and never made under the coordinator lock.
//! - [`Core::proxy_gate`] only reads flags, so it is safe under the lock; the password is read
//!   again at user entry points ([`Core::refresh_proxy_password`]) before the lock is taken.

use std::sync::{Arc, MutexGuard};

use dm_backend_api::{DownloadBackend, ProxyConfig, require_capability};
use dm_domain::proxy::{
    PasswordChange, ProxyCarries, ProxyMode, ProxyPasswordState, ProxySettings, SetProxyParams,
};
use dm_domain::redact::Secret;
use dm_domain::{Capability, CapabilitySupport, ErrorKind, NextAction};
use dm_platform::vault::{Vault, VaultError};
use dm_storage::{ProxyRow, Store};

use super::{Core, ServiceError, credentials};

/// The proxy password's id in Downova's namespace of the OS credential store.
pub(crate) const PASSWORD_ID: &str = "proxy-password";

/// Why nothing may start until the user acts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Block {
    /// A proxy is saved but this engine cannot use one.
    Engine,
    /// The saved row could not be read.
    Damaged,
}

#[derive(Debug, Default)]
pub(crate) struct ProxyState {
    row: ProxyRow,
    password: ProxyPasswordState,
    blocked: Option<Block>,
}

/// One bounded read of the saved password. `Ok(None)`: none in the store.
fn read_password(vault: &Arc<dyn Vault>, wait: std::time::Duration) -> Result<Option<Secret>, ()> {
    match credentials::bounded(vault, wait, |v| v.get(PASSWORD_ID)) {
        Some(Ok(bytes)) => String::from_utf8(bytes)
            .map(|s| Some(Secret::new(s)))
            .map_err(|_| ()),
        Some(Err(VaultError::Missing)) => Ok(None),
        _ => Err(()),
    }
}

fn store_refused(what: &str, e: Option<&VaultError>) -> ServiceError {
    let detail = match e {
        Some(e) => format!("the proxy password could not be {what}: {e}"),
        None => format!(
            "the proxy password could not be {what}: the system credential store did not answer in time"
        ),
    };
    ServiceError::new(ErrorKind::CredentialStoreUnavailable, detail).with_action(NextAction::Retry)
}

impl<B: DownloadBackend> Core<B> {
    fn proxy(&self) -> MutexGuard<'_, ProxyState> {
        self.proxy
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Hands `row` (with `password`, when one is known) to the engine and records the outcome.
    /// A refusal for an HTTP proxy blocks every start: never a direct connection instead.
    fn apply_proxy(&self, row: ProxyRow, password: Option<Secret>, state: ProxyPasswordState) {
        let config = (row.mode == ProxyMode::Http).then(|| ProxyConfig {
            host: row.host.clone().unwrap_or_default(),
            port: row.port.unwrap_or_default(),
            username: row.username.clone(),
            password,
        });
        let applied = self.backend.set_proxy(config);
        let blocked = match (&applied, row.mode) {
            (Err(e), ProxyMode::Http) => {
                eprintln!("dm-service: the saved proxy cannot be used: {}", e.detail);
                Some(Block::Engine)
            }
            // Direct on an engine without proxy support: it never had one, nothing to undo.
            _ => None,
        };
        *self.proxy() = ProxyState {
            row,
            password: state,
            blocked,
        };
    }

    /// Startup (from `with_store`, before any recovered transfer starts): applies the saved
    /// setting. A damaged row blocks every start rather than silently going direct.
    pub(super) fn restore_proxy(&self, store: &Store) {
        let row = match store.load_proxy() {
            Ok(Some(row)) => row,
            Ok(None) => return,
            Err(e) => {
                eprintln!("dm-service: the saved proxy setting is unreadable: {e}");
                self.proxy().blocked = Some(Block::Damaged);
                return;
            }
        };
        let (password, state) = if row.password_saved {
            match read_password(&self.vault, self.vault_wait) {
                Ok(Some(p)) => (Some(p), ProxyPasswordState::Saved),
                Ok(None) => (None, ProxyPasswordState::None),
                Err(()) => (None, ProxyPasswordState::Unavailable),
            }
        } else {
            (None, ProxyPasswordState::None)
        };
        self.apply_proxy(row, password, state);
    }

    /// Reads a saved but unreadable password again (bounded, outside every lock), so a store
    /// unlocked since startup takes effect at the next user action.
    pub async fn refresh_proxy_password(&self) {
        let wanted = {
            let p = self.proxy();
            p.row.mode == ProxyMode::Http && p.password == ProxyPasswordState::Unavailable
        };
        if !wanted {
            return;
        }
        let vault = Arc::clone(&self.vault);
        let wait = self.vault_wait;
        let Ok(Ok(found)) = tokio::task::spawn_blocking(move || read_password(&vault, wait)).await
        else {
            return;
        };
        let row = self.proxy().row.clone();
        let state = if found.is_some() {
            ProxyPasswordState::Saved
        } else {
            ProxyPasswordState::None
        };
        self.apply_proxy(row, found, state);
    }

    /// Refuses a start while the saved proxy is not in effect (flags only: safe under the lock).
    pub(super) fn proxy_gate(&self) -> Result<(), ServiceError> {
        let p = self.proxy();
        match p.blocked {
            Some(Block::Engine) => {
                return Err(ServiceError::new(
                    ErrorKind::BackendCapabilityUnavailable,
                    "a proxy is saved, but this download engine cannot use one; turn the proxy off or use the default engine",
                ));
            }
            Some(Block::Damaged) => {
                return Err(ServiceError::new(
                    ErrorKind::Internal,
                    "the saved proxy setting is damaged; save the proxy settings again",
                ));
            }
            None => {}
        }
        if p.row.mode == ProxyMode::Http && p.password == ProxyPasswordState::Unavailable {
            return Err(ServiceError::new(
                ErrorKind::CredentialStoreUnavailable,
                "the proxy password is in the system credential store, which is locked or not answering; unlock it, then try again",
            )
            .with_action(NextAction::Retry));
        }
        Ok(())
    }

    fn proxy_snapshot(&self) -> ProxySettings {
        let p = self.proxy();
        let carries = if p.row.mode == ProxyMode::Http
            && p.blocked.is_none()
            && dm_domain::capability::support_of(&self.capabilities, Capability::HttpProxy)
                == CapabilitySupport::Supported
        {
            vec![ProxyCarries::Http, ProxyCarries::Https]
        } else {
            Vec::new()
        };
        ProxySettings {
            mode: p.row.mode,
            host: p.row.host.clone(),
            port: p.row.port,
            username: p.row.username.clone(),
            password_state: p.password,
            carries,
        }
    }

    /// `proxy.v1.get`.
    pub async fn proxy_settings(&self) -> ProxySettings {
        self.refresh_proxy_password().await;
        self.proxy_snapshot()
    }

    /// `proxy.v1.set`: validate, check the capability, write the password to the OS store, save
    /// the row, then apply (ADR 0034). A refused step leaves the saved row and the engine as they
    /// were.
    pub async fn set_proxy(&self, params: SetProxyParams) -> Result<ProxySettings, ServiceError> {
        params
            .validate()
            .map_err(|why| ServiceError::new(ErrorKind::InvalidRequest, why))?;
        if params.mode == ProxyMode::Http {
            require_capability(&self.capabilities, Capability::HttpProxy)?;
        }
        let saved_before = self.proxy().row.password_saved;
        // Without a user name a saved password is meaningless: it goes too.
        let change = match params.password {
            PasswordChange::Keep if params.username.is_none() && saved_before => {
                PasswordChange::Remove
            }
            other => other,
        };
        let vault = Arc::clone(&self.vault);
        let wait = self.vault_wait;
        let (password, password_saved, state) = match change {
            PasswordChange::Replace(p) => {
                let secret = p.into_secret();
                let bytes = secret.expose().as_bytes().to_vec();
                let put = tokio::task::spawn_blocking(move || {
                    credentials::bounded(&vault, wait, move |v| v.put(PASSWORD_ID, &bytes))
                })
                .await
                .ok()
                .flatten();
                match put {
                    Some(Ok(())) => (Some(secret), true, ProxyPasswordState::Saved),
                    Some(Err(e)) => return Err(store_refused("saved", Some(&e))),
                    None => return Err(store_refused("saved", None)),
                }
            }
            PasswordChange::Remove => {
                if saved_before {
                    let deleted = tokio::task::spawn_blocking(move || {
                        credentials::bounded(&vault, wait, |v| v.delete(PASSWORD_ID))
                    })
                    .await
                    .ok()
                    .flatten();
                    match deleted {
                        Some(Ok(())) => {}
                        Some(Err(e)) => return Err(store_refused("removed", Some(&e))),
                        None => return Err(store_refused("removed", None)),
                    }
                }
                (None, false, ProxyPasswordState::None)
            }
            PasswordChange::Keep if saved_before => {
                match tokio::task::spawn_blocking(move || read_password(&vault, wait))
                    .await
                    .unwrap_or(Err(()))
                {
                    Ok(Some(p)) => (Some(p), true, ProxyPasswordState::Saved),
                    Ok(None) => (None, false, ProxyPasswordState::None),
                    Err(()) => (None, true, ProxyPasswordState::Unavailable),
                }
            }
            PasswordChange::Keep => (None, false, ProxyPasswordState::None),
        };
        let row = ProxyRow {
            mode: params.mode,
            host: params.host,
            port: params.port,
            username: params.username,
            password_saved,
        };
        let mut guard = self.state.lock().await;
        if let Some(store) = guard.store.as_mut() {
            store.save_proxy(&row).map_err(|e| {
                eprintln!("dm-service: proxy setting not saved: {e}");
                ServiceError::new(ErrorKind::Internal, "the proxy setting could not be saved")
            })?;
        }
        self.apply_proxy(row, password, state);
        drop(guard);
        Ok(self.proxy_snapshot())
    }
}
