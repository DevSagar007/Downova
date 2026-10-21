//! Replaceable download-backend boundary.
//!
//! The service talks to a backend only through [`DownloadBackend`]. Backend handles (for aria2,
//! the GID) are opaque [`BackendHandle`]s owned by the service's attempt mapping; they are never
//! exposed as job IDs, persisted as the job identity, or shown in the UI. Public APIs never pass
//! arbitrary backend options through.

use std::fmt;
use std::future::Future;

use dm_domain::capability::support_of;
use dm_domain::redact::Secret;
use dm_domain::{AttemptId, ByteCount, Capability, CapabilityReport, CapabilitySupport, ErrorKind};

/// Opaque backend-specific transfer handle (e.g. an aria2 GID). Scoped to one attempt.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BackendHandle(pub String);

/// A validated request to start one transfer attempt.
#[derive(Debug, Clone)]
pub struct TransferRequest {
    pub attempt_id: AttemptId,
    /// Raw source URL; may contain secrets. Never log without `redact_url`.
    pub url: String,
    /// Credentials resolved from the OS credential store for this attempt only.
    pub headers: Vec<(String, Secret)>,
    pub destination_dir: std::path::PathBuf,
    pub file_name: String,
    pub max_connections: u8,
}

/// Live, non-durable progress reported by a backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackendProgress {
    pub completed: ByteCount,
    pub total: Option<ByteCount>,
}

/// Backend failure, already classified into a stable [`ErrorKind`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendError {
    pub kind: ErrorKind,
    /// Sanitized detail; must not contain URLs with tokens, cookies or headers.
    pub detail: String,
}

impl fmt::Display for BackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.detail)
    }
}

impl std::error::Error for BackendError {}

/// A transfer engine behind the service. Implementations: `dm-backend-aria2` (planned, B0).
pub trait DownloadBackend: Send + Sync {
    /// Stable backend identifier, e.g. `"aria2"`.
    fn id(&self) -> &'static str;

    /// Complete capability report. Capabilities not proven by fixtures must be `Unverified`.
    fn capabilities(&self) -> Vec<CapabilityReport>;

    /// Starts one attempt and returns the backend's handle for it.
    fn start(
        &self,
        request: TransferRequest,
    ) -> impl Future<Output = Result<BackendHandle, BackendError>> + Send;

    /// Pauses a transfer, keeping engine-owned resume state.
    fn pause(&self, handle: &BackendHandle) -> impl Future<Output = Result<(), BackendError>> + Send;

    /// Removes a transfer. After this returns, the backend emits no further progress for it.
    fn remove(&self, handle: &BackendHandle)
    -> impl Future<Output = Result<(), BackendError>> + Send;

    /// Reads live progress (not a durability checkpoint).
    fn progress(
        &self,
        handle: &BackendHandle,
    ) -> impl Future<Output = Result<BackendProgress, BackendError>> + Send;
}

/// Gates a feature on an explicitly supported capability.
///
/// `Unsupported`, `Unverified` and absent capabilities are refused with
/// `BACKEND_CAPABILITY_UNAVAILABLE`; there is never a silent fallback (for example, an
/// unsupported proxy profile must not turn into a direct connection).
pub fn require_capability(
    report: &[CapabilityReport],
    capability: Capability,
) -> Result<(), BackendError> {
    match support_of(report, capability) {
        CapabilitySupport::Supported => Ok(()),
        other => Err(BackendError {
            kind: ErrorKind::BackendCapabilityUnavailable,
            detail: format!("{capability:?} is {other:?} for this backend"),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(capability: Capability, support: CapabilitySupport) -> CapabilityReport {
        CapabilityReport { capability, support, note: None }
    }

    #[test]
    fn only_supported_capabilities_pass() {
        let caps = vec![
            report(Capability::Http, CapabilitySupport::Supported),
            report(Capability::Http2, CapabilitySupport::Unverified),
            report(Capability::Socks5Proxy, CapabilitySupport::Unsupported),
        ];
        assert!(require_capability(&caps, Capability::Http).is_ok());
        for refused in [Capability::Http2, Capability::Socks5Proxy, Capability::PacProxy] {
            let err = require_capability(&caps, refused).unwrap_err();
            assert_eq!(err.kind, ErrorKind::BackendCapabilityUnavailable);
        }
    }
}
