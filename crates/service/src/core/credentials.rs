//! Login contexts for signed-in downloads and the data key (SEC-02, BRW-05, ADR 0027).
//!
//! - **Temporary** contexts live only here, in memory, and end with their job or the service.
//! - **Remembered** contexts are also in the OS credential store under their opaque id; SQLite holds
//!   only metadata. Store calls block, so they run on blocking threads and never under the
//!   coordinator lock.
//! - The **data key** (sealing protected URL material in SQLite) is read or created once at startup.
//!
//! Nothing here ever returns a secret to a caller: snapshots, the list and errors carry ids,
//! origins, modes and states only.

use std::collections::HashMap;
use std::sync::Arc;

use dm_domain::api::{
    AuthInfo, BrowserKind, CredentialInfo, CredentialMode, CredentialState, SessionInput,
};
use dm_domain::cookies::{Origin, ScopedCookie, SessionScope};
use dm_domain::{ByteCount, ErrorKind, NextAction};
use dm_platform::vault::{Vault, VaultError};
use dm_storage::{CredentialRow, Store};
use sha2::{Digest, Sha256};

use crate::core::ServiceError;

/// Vault id of the data key.
pub(crate) const DATA_KEY_ID: &str = "data-key";

/// One context in memory.
#[derive(Debug, Clone)]
pub(crate) struct Context {
    pub scope: Option<Arc<SessionScope>>,
    pub info: AuthInfo,
    pub created_at_ms: u64,
    pub expires_at_ms: Option<u64>,
    /// Its download is being added: the job does not exist yet, so the sweep that ends finished
    /// temporary sign-ins must leave it alone.
    pub adding: bool,
}

/// Every known context, by id.
#[derive(Debug, Default)]
pub(crate) struct Registry {
    pub contexts: HashMap<String, Context>,
    /// The data key could be read or created (protected URLs are sealed).
    pub sealing: bool,
    /// Why the store is not usable, when it is not.
    pub store_problem: Option<String>,
    /// The data key exists but the store is locked now: sealed links are kept, not given up.
    pub store_locked: bool,
    pub legacy_plaintext_rows: usize,
}

/// `cred_` + 16 hex of SHA-256(idempotency key | origin): a retried add maps to the same id.
pub(crate) fn credential_id(idempotency_key: &str, origin: &str) -> String {
    let d = Sha256::digest(format!("{idempotency_key}|{origin}").as_bytes());
    let hex: String = d.iter().take(8).map(|b| format!("{b:02x}")).collect();
    format!("cred_{hex}")
}

fn browser_str(b: BrowserKind) -> &'static str {
    match b {
        BrowserKind::Chrome => "chrome",
        BrowserKind::Edge => "edge",
    }
}

fn parse_browser(s: &str) -> BrowserKind {
    if s == "edge" {
        BrowserKind::Edge
    } else {
        BrowserKind::Chrome
    }
}

/// The session's cookies that can ever apply to its origin (domain match), unexpired.
pub(crate) fn scope_from(
    session: &SessionInput,
    now_ms: u64,
) -> Result<SessionScope, ServiceError> {
    let origin = Origin::parse(&session.origin).ok_or_else(|| {
        ServiceError::new(ErrorKind::InvalidRequest, "the session origin is not valid")
    })?;
    let cookies: Vec<ScopedCookie> = session
        .cookies
        .iter()
        .map(|c| ScopedCookie {
            name: c.name.clone(),
            value: c.value.clone().into_secret(),
            domain: c.domain.trim_start_matches('.').to_ascii_lowercase(),
            host_only: c.host_only,
            path: c.path.clone(),
            secure: c.secure,
            expires_at_ms: c.expires_at_ms.map(|e| e.0),
        })
        .collect();
    let scope = SessionScope { origin, cookies };
    let probe = format!("{}/", scope.origin.display());
    // At least one cookie must apply somewhere on this origin (a path cookie counts).
    let applicable = scope.cookies.iter().any(|c| {
        let s = SessionScope {
            origin: scope.origin.clone(),
            cookies: vec![ScopedCookie {
                path: "/".into(),
                ..c.clone()
            }],
        };
        s.cookie_header(&probe, now_ms).is_some()
    });
    if !applicable {
        return Err(ServiceError::new(
            ErrorKind::AuthRequired,
            "the browser offered no current sign-in for this site; sign in there and try again",
        )
        .with_action(NextAction::SignIn));
    }
    Ok(scope)
}

/// The secret bytes of a remembered context (JSON of the cookies, values included). Only ever
/// passed to the OS store.
pub(crate) fn encode_scope(scope: &SessionScope) -> Vec<u8> {
    let cookies: Vec<serde_json::Value> = scope
        .cookies
        .iter()
        .map(|c| {
            serde_json::json!({
                "n": c.name, "v": c.value.expose(), "d": c.domain, "h": c.host_only,
                "p": c.path, "s": c.secure, "e": c.expires_at_ms,
            })
        })
        .collect();
    serde_json::json!({ "o": scope.origin.display(), "c": cookies })
        .to_string()
        .into_bytes()
}

pub(crate) fn decode_scope(bytes: &[u8]) -> Option<SessionScope> {
    let v: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    let origin = Origin::parse(v.get("o")?.as_str()?)?;
    let cookies = v
        .get("c")?
        .as_array()?
        .iter()
        .map(|c| {
            Some(ScopedCookie {
                name: c.get("n")?.as_str()?.to_owned(),
                value: dm_domain::redact::Secret::new(c.get("v")?.as_str()?),
                domain: c.get("d")?.as_str()?.to_owned(),
                host_only: c.get("h")?.as_bool()?,
                path: c.get("p")?.as_str()?.to_owned(),
                secure: c.get("s")?.as_bool()?,
                expires_at_ms: c.get("e").and_then(serde_json::Value::as_u64),
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(SessionScope { origin, cookies })
}

pub(crate) fn vault_state(e: &VaultError) -> CredentialState {
    match e {
        VaultError::Locked => CredentialState::Locked,
        VaultError::Missing => CredentialState::Missing,
        VaultError::Unavailable(_) | VaultError::TooLarge => CredentialState::Unavailable,
    }
}

pub(crate) fn vault_error(e: &VaultError) -> ServiceError {
    match e {
        VaultError::TooLarge => ServiceError::new(
            ErrorKind::CredentialStoreUnavailable,
            "this sign-in is too large for the system credential store; use it without remembering it",
        ),
        other => ServiceError::new(ErrorKind::CredentialStoreUnavailable, other.to_string())
            .with_action(NextAction::Retry),
    }
}

/// Startup (before serving, not under any lock): reads or creates the data key, re-seals legacy
/// rows, reconciles credential rows with the store, and loads remembered contexts.
pub(crate) fn startup(store: &mut Store, vault: &Arc<dyn Vault>) -> Registry {
    let mut reg = Registry::default();
    // 1. The data key.
    let key = match vault.get(DATA_KEY_ID) {
        Ok(k) if k.len() == 32 => Some(k),
        Ok(_) => {
            reg.store_problem = Some("the stored data key is malformed".into());
            None
        }
        Err(VaultError::Missing) => match dm_storage::seal::Sealer::generate_key() {
            Ok(k) => match vault.put(DATA_KEY_ID, &k) {
                Ok(()) => Some(k.to_vec()),
                Err(e) => {
                    reg.store_problem = Some(e.to_string());
                    None
                }
            },
            Err(_) => None,
        },
        Err(e) => {
            reg.store_locked = e == VaultError::Locked;
            reg.store_problem = Some(e.to_string());
            None
        }
    };
    if let Some(k) = key.as_deref().and_then(|k| <[u8; 32]>::try_from(k).ok()) {
        store.set_sealer(&k);
        reg.sealing = true;
        match store.reseal_legacy() {
            Ok((j, c)) if j > 0 || c > 0 => {
                eprintln!("dm-service: re-sealed {j} stored link(s) and {c} checkpoint(s)");
            }
            Ok(_) => {}
            Err(e) => eprintln!("dm-service: re-sealing stored links failed: {e}"),
        }
    } else if let Some(why) = &reg.store_problem {
        eprintln!("dm-service: protected link parts are kept in memory only: {why}");
    }
    reg.legacy_plaintext_rows = store.legacy_plaintext_rows().unwrap_or(0);
    // 2. Credential rows.
    for row in store.load_credentials().unwrap_or_default() {
        let remembered = row.mode == "remembered";
        if row.state == "pending" {
            // A crash between the store write and the job: never leave an unmanaged secret.
            if remembered {
                let _ = vault.delete(&row.credential_id);
            }
            let _ = store.delete_credential(&row.credential_id);
            continue;
        }
        let (scope, state) = if row.state == "forgotten" {
            (None, CredentialState::Forgotten)
        } else if !remembered {
            (None, CredentialState::Missing)
        } else {
            match vault.get(&row.credential_id) {
                Ok(bytes) => match decode_scope(&bytes) {
                    Some(s) => (Some(Arc::new(s)), CredentialState::Available),
                    None => (None, CredentialState::Missing),
                },
                Err(e) => (None, vault_state(&e)),
            }
        };
        reg.contexts.insert(
            row.credential_id.clone(),
            Context {
                adding: false,
                scope,
                info: AuthInfo {
                    credential_id: row.credential_id.clone(),
                    origin: row.origin.clone(),
                    browser: parse_browser(&row.browser),
                    mode: if remembered {
                        CredentialMode::Remembered
                    } else {
                        CredentialMode::Temporary
                    },
                    state,
                },
                created_at_ms: row.created_at_ms,
                expires_at_ms: row.expires_at_ms,
            },
        );
    }
    reg
}

pub(crate) fn row_for(
    info: &AuthInfo,
    profile_id: &str,
    state: &str,
    created: u64,
    expires: Option<u64>,
) -> CredentialRow {
    CredentialRow {
        credential_id: info.credential_id.clone(),
        origin: info.origin.clone(),
        browser: browser_str(info.browser).into(),
        profile_id: profile_id.into(),
        mode: match info.mode {
            CredentialMode::Temporary => "temporary".into(),
            CredentialMode::Remembered => "remembered".into(),
        },
        state: state.into(),
        generation: 1,
        created_at_ms: created,
        expires_at_ms: expires,
    }
}

pub(crate) fn info_list(reg: &Registry, active: &HashMap<String, u32>) -> Vec<CredentialInfo> {
    let mut out: Vec<CredentialInfo> = reg
        .contexts
        .values()
        .map(|c| CredentialInfo {
            credential_id: c.info.credential_id.clone(),
            origin: c.info.origin.clone(),
            browser: c.info.browser,
            mode: c.info.mode,
            state: c.info.state,
            created_at_ms: ByteCount(c.created_at_ms),
            expires_at_ms: c.expires_at_ms.map(ByteCount),
            active_jobs: active.get(&c.info.credential_id).copied().unwrap_or(0),
        })
        .collect();
    out.sort_by(|a, b| {
        a.created_at_ms
            .0
            .cmp(&b.created_at_ms.0)
            .then(a.credential_id.cmp(&b.credential_id))
    });
    out
}
