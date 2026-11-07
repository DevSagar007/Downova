//! OS credential store (SEC-02, ADR 0027): Windows Credential Manager, macOS Keychain, Linux
//! Secret Service, through `keyring-core` and its native stores.
//!
//! The interface is deliberately small: put, get, delete one opaque byte payload under an id in
//! Downova's own namespace. Labels and lookup attributes carry only the namespace and the id, never
//! a value. Every call blocks: callers run it on a blocking thread, never under a lock that other
//! work waits for. There is no fallback store of any kind.

use std::sync::{Arc, OnceLock};

/// Why a vault operation failed. Never carries a secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VaultError {
    /// The store refused access (locked keychain or collection, denied prompt).
    Locked,
    /// No entry under this id.
    Missing,
    /// No usable store on this system (no Secret Service, unsupported platform, store failure).
    Unavailable(String),
    /// The payload is larger than this store accepts.
    TooLarge,
}

impl std::fmt::Display for VaultError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Locked => write!(f, "the system credential store is locked or refused access"),
            Self::Missing => write!(
                f,
                "the saved entry is missing from the system credential store"
            ),
            Self::Unavailable(why) => {
                write!(f, "the system credential store is unavailable ({why})")
            }
            Self::TooLarge => write!(f, "too large for the system credential store"),
        }
    }
}

/// A store of opaque secrets keyed by id. Implementations must keep values out of labels,
/// attributes, logs and errors.
pub trait Vault: Send + Sync + std::fmt::Debug + 'static {
    fn put(&self, id: &str, secret: &[u8]) -> Result<(), VaultError>;
    fn get(&self, id: &str) -> Result<Vec<u8>, VaultError>;
    /// Deleting a missing entry is not an error.
    fn delete(&self, id: &str) -> Result<(), VaultError>;
}

/// Largest payload accepted by every supported store (Windows limits a generic credential's blob to
/// 5 × 512 bytes).
pub const MAX_SECRET_BYTES: usize = 2560;

/// The real OS store, under the service name `Downova/<namespace>` (the namespace separates data
/// folders and test runs).
#[derive(Debug, Clone)]
pub struct OsVault {
    service: String,
}

impl OsVault {
    pub fn new(namespace: &str) -> Self {
        Self {
            service: format!("Downova/{namespace}"),
        }
    }
}

#[cfg(feature = "vault")]
mod imp {
    use super::*;
    use keyring_core::{Entry, Error};

    static STORE: OnceLock<Result<(), String>> = OnceLock::new();

    /// Installs the platform store once per process.
    pub(super) fn ensure_store() -> Result<(), VaultError> {
        STORE
            .get_or_init(|| {
                #[cfg(windows)]
                let store = windows_native_keyring_store::Store::new().map(|s| s as Arc<_>);
                #[cfg(target_os = "macos")]
                let store = apple_native_keyring_store::keychain::Store::new().map(|s| s as Arc<_>);
                #[cfg(target_os = "linux")]
                let store = zbus_secret_service_keyring_store::Store::new().map(|s| s as Arc<_>);
                #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
                let store: Result<Arc<keyring_core::CredentialStore>, Error> = Err(
                    Error::NotSupportedByStore("no credential store on this platform".into()),
                );
                match store {
                    Ok(s) => {
                        keyring_core::set_default_store(s);
                        Ok(())
                    }
                    Err(e) => Err(short(&e)),
                }
            })
            .clone()
            .map_err(VaultError::Unavailable)
    }

    /// A short, value-free description of a store error.
    fn short(e: &Error) -> String {
        match e {
            Error::PlatformFailure(_) => "platform failure".into(),
            Error::NoStorageAccess(_) => "no access".into(),
            Error::NoEntry => "no entry".into(),
            Error::TooLong(what, _) => format!("{what} too long"),
            Error::NotSupportedByStore(_) => "not supported".into(),
            _ => "store error".into(),
        }
    }

    pub(super) fn map(e: Error) -> VaultError {
        match e {
            Error::NoStorageAccess(_) => VaultError::Locked,
            Error::NoEntry => VaultError::Missing,
            Error::TooLong(..) => VaultError::TooLarge,
            other => VaultError::Unavailable(short(&other)),
        }
    }

    pub(super) fn entry(service: &str, id: &str) -> Result<Entry, VaultError> {
        ensure_store()?;
        Entry::new(service, id).map_err(map)
    }
}

#[cfg(feature = "vault")]
impl Vault for OsVault {
    fn put(&self, id: &str, secret: &[u8]) -> Result<(), VaultError> {
        if secret.len() > MAX_SECRET_BYTES {
            return Err(VaultError::TooLarge);
        }
        imp::entry(&self.service, id)?
            .set_secret(secret)
            .map_err(imp::map)
    }
    fn get(&self, id: &str) -> Result<Vec<u8>, VaultError> {
        imp::entry(&self.service, id)?
            .get_secret()
            .map_err(imp::map)
    }
    fn delete(&self, id: &str) -> Result<(), VaultError> {
        match imp::entry(&self.service, id)?.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring_core::Error::NoEntry) => Ok(()),
            Err(e) => Err(imp::map(e)),
        }
    }
}

/// Without the `vault` feature (the desktop and CLI never link the store): always unavailable.
#[cfg(not(feature = "vault"))]
impl Vault for OsVault {
    fn put(&self, _: &str, _: &[u8]) -> Result<(), VaultError> {
        Err(VaultError::Unavailable(
            "not built with a credential store".into(),
        ))
    }
    fn get(&self, _: &str) -> Result<Vec<u8>, VaultError> {
        Err(VaultError::Unavailable(
            "not built with a credential store".into(),
        ))
    }
    fn delete(&self, _: &str) -> Result<(), VaultError> {
        Err(VaultError::Unavailable(
            "not built with a credential store".into(),
        ))
    }
}

/// A process-local, in-memory vault for tests (never persisted, never used in production).
#[derive(Debug, Default)]
pub struct MemoryVault {
    pub entries: std::sync::Mutex<std::collections::HashMap<String, Vec<u8>>>,
    /// When set, every call fails with this error.
    pub fail: std::sync::Mutex<Option<VaultError>>,
}

impl Vault for MemoryVault {
    fn put(&self, id: &str, secret: &[u8]) -> Result<(), VaultError> {
        if let Some(e) = self.fail.lock().ok().and_then(|f| f.clone()) {
            return Err(e);
        }
        if secret.len() > MAX_SECRET_BYTES {
            return Err(VaultError::TooLarge);
        }
        if let Ok(mut m) = self.entries.lock() {
            m.insert(id.to_owned(), secret.to_vec());
        }
        Ok(())
    }
    fn get(&self, id: &str) -> Result<Vec<u8>, VaultError> {
        if let Some(e) = self.fail.lock().ok().and_then(|f| f.clone()) {
            return Err(e);
        }
        self.entries
            .lock()
            .ok()
            .and_then(|m| m.get(id).cloned())
            .ok_or(VaultError::Missing)
    }
    fn delete(&self, id: &str) -> Result<(), VaultError> {
        if let Some(e) = self.fail.lock().ok().and_then(|f| f.clone()) {
            return Err(e);
        }
        if let Ok(mut m) = self.entries.lock() {
            m.remove(id);
        }
        Ok(())
    }
}

/// Keeps `OnceLock` and `Arc` used without the feature.
#[allow(dead_code)]
fn _unused(_: OnceLock<()>, _: Arc<()>) {}

#[cfg(all(test, feature = "vault"))]
mod tests {
    use super::*;

    /// The real OS store with a synthetic secret in an owned, unique namespace; the entry is
    /// deleted at the end. Ignored by default: it touches the user's credential store.
    /// `cargo test -p dm-platform --features vault -- --ignored vault`
    #[test]
    #[ignore = "touches the real OS credential store; run explicitly"]
    fn real_os_vault_round_trip() {
        let ns = format!("test-{}", crate::random_hex(6).unwrap());
        let v = OsVault::new(&ns);
        let id = "cred_0000000000000001";
        let secret = b"SYNTH-VAULT-SECRET-b33";
        assert_eq!(v.get(id), Err(VaultError::Missing));
        v.put(id, secret).unwrap();
        assert_eq!(v.get(id).unwrap(), secret);
        v.put(id, b"replaced").unwrap();
        assert_eq!(v.get(id).unwrap(), b"replaced");
        v.delete(id).unwrap();
        assert_eq!(v.get(id), Err(VaultError::Missing));
        v.delete(id).unwrap();
        assert_eq!(
            v.put(id, &[0u8; MAX_SECRET_BYTES + 1]),
            Err(VaultError::TooLarge)
        );
        println!("real OS vault round trip passed in namespace Downova/{ns}");
    }
}

/// A store that is never available (tests, and an explicit `DOWNOVA_VAULT=off` in debug builds).
#[derive(Debug, Default)]
pub struct OffVault;

impl Vault for OffVault {
    fn put(&self, _: &str, _: &[u8]) -> Result<(), VaultError> {
        Err(VaultError::Unavailable("disabled".into()))
    }
    fn get(&self, _: &str) -> Result<Vec<u8>, VaultError> {
        Err(VaultError::Unavailable("disabled".into()))
    }
    fn delete(&self, _: &str) -> Result<(), VaultError> {
        Err(VaultError::Unavailable("disabled".into()))
    }
}

/// **Debug builds only** (compiled out of release packages): a file-backed test vault, so process
/// tests that restart the service keep their data key without touching the developer's real OS
/// credential store. Selected only by `DOWNOVA_VAULT=file`.
#[cfg(debug_assertions)]
#[derive(Debug)]
pub struct FileVault {
    path: std::path::PathBuf,
}

#[cfg(debug_assertions)]
impl FileVault {
    pub fn new(path: std::path::PathBuf) -> Self {
        Self { path }
    }
    fn read(&self) -> std::collections::BTreeMap<String, String> {
        std::fs::read_to_string(&self.path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }
    fn write(&self, m: &std::collections::BTreeMap<String, String>) -> Result<(), VaultError> {
        let text =
            serde_json::to_string(m).map_err(|_| VaultError::Unavailable("encode".into()))?;
        std::fs::write(&self.path, text).map_err(|_| VaultError::Unavailable("write".into()))
    }
}

#[cfg(debug_assertions)]
impl Vault for FileVault {
    fn put(&self, id: &str, secret: &[u8]) -> Result<(), VaultError> {
        if secret.len() > MAX_SECRET_BYTES {
            return Err(VaultError::TooLarge);
        }
        let mut m = self.read();
        m.insert(
            id.into(),
            secret.iter().map(|b| format!("{b:02x}")).collect(),
        );
        self.write(&m)
    }
    fn get(&self, id: &str) -> Result<Vec<u8>, VaultError> {
        let hex = self.read().remove(id).ok_or(VaultError::Missing)?;
        (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(hex.get(i..i.saturating_add(2)).unwrap_or("zz"), 16))
            .collect::<Result<Vec<u8>, _>>()
            .map_err(|_| VaultError::Unavailable("corrupt".into()))
    }
    fn delete(&self, id: &str) -> Result<(), VaultError> {
        let mut m = self.read();
        m.remove(id);
        self.write(&m)
    }
}
