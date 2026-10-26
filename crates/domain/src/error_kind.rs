//! Stable failure classification shared by the service, CLI, UI and diagnostics.

use serde::{Deserialize, Serialize};

/// Stable, user-explainable error kinds (research §17.6). Never encode secrets in variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorKind {
    NetworkOffline,
    AuthRequired,
    LinkRefreshRequired,
    RemoteChanged,
    RangeUnsupported,
    InvalidRange,
    ChecksumMismatch,
    DiskFull,
    VolumeMissing,
    /// The file system refused the destination file: a name that is too long or invalid there,
    /// no permission, or a read-only volume (B1.6). The detail says which.
    DestinationUnwritable,
    HostNotInstalled,
    ScanBlocked,
    BackendCapabilityUnavailable,
    /// The origin is overloaded or rate-limiting (HTTP 429, 503, other 5xx).
    ServerBusy,
    /// TLS could not be established (untrusted certificate, wrong host name, protocol failure).
    TlsHandshakeFailed,
    ServiceUnavailable,
    InvalidRequest,
    IdempotencyConflict,
    Internal,
}

/// The next step offered to the user for a failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NextAction {
    None,
    Retry,
    Wait,
    RefreshLink,
    SignIn,
    FreeDiskSpace,
    ReconnectVolume,
    InstallDesktopApp,
    ChooseRestartOrKeep,
    ContactSupport,
}

/// Error payload carried in JSON-RPC `error.data` and in failed job snapshots.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ErrorData {
    pub kind: ErrorKind,
    pub retryable: bool,
    pub next_action: NextAction,
    /// Sanitized, user-safe detail. Must already be redacted.
    #[serde(default)]
    pub detail: Option<String>,
}

impl ErrorKind {
    /// Default retry classification. Authentication failures and changed resources are never
    /// retried blindly (research §7); the service may still override with server hints.
    pub fn default_retryable(self) -> bool {
        matches!(
            self,
            ErrorKind::NetworkOffline | ErrorKind::ServiceUnavailable | ErrorKind::ServerBusy
        )
    }

    /// The next step offered by default for this kind.
    pub fn default_next_action(self) -> NextAction {
        match self {
            ErrorKind::NetworkOffline | ErrorKind::ServiceUnavailable => NextAction::Retry,
            ErrorKind::ServerBusy => NextAction::Wait,
            ErrorKind::AuthRequired => NextAction::SignIn,
            ErrorKind::LinkRefreshRequired => NextAction::RefreshLink,
            ErrorKind::RemoteChanged | ErrorKind::RangeUnsupported | ErrorKind::InvalidRange => {
                NextAction::ChooseRestartOrKeep
            }
            ErrorKind::DiskFull => NextAction::FreeDiskSpace,
            ErrorKind::VolumeMissing => NextAction::ReconnectVolume,
            ErrorKind::HostNotInstalled => NextAction::InstallDesktopApp,
            ErrorKind::Internal => NextAction::ContactSupport,
            ErrorKind::ChecksumMismatch
            | ErrorKind::ScanBlocked
            | ErrorKind::BackendCapabilityUnavailable
            | ErrorKind::TlsHandshakeFailed
            | ErrorKind::DestinationUnwritable
            | ErrorKind::InvalidRequest
            | ErrorKind::IdempotencyConflict => NextAction::None,
        }
    }

    /// Builds the wire error payload with the default classification.
    pub fn to_error_data(self, detail: impl Into<String>) -> ErrorData {
        ErrorData {
            kind: self,
            retryable: self.default_retryable(),
            next_action: self.default_next_action(),
            detail: Some(detail.into()),
        }
    }

    /// Stable wire name, e.g. `DISK_FULL`.
    pub fn wire_name(self) -> String {
        serde_json::to_value(self)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| "INTERNAL".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_and_changed_resource_are_not_blindly_retryable() {
        assert!(!ErrorKind::AuthRequired.default_retryable());
        assert!(!ErrorKind::RemoteChanged.default_retryable());
        assert!(!ErrorKind::ChecksumMismatch.default_retryable());
        assert!(ErrorKind::NetworkOffline.default_retryable());
    }

    #[test]
    fn wire_names_are_stable() {
        let json = serde_json::to_string(&ErrorKind::BackendCapabilityUnavailable).unwrap();
        assert_eq!(json, "\"BACKEND_CAPABILITY_UNAVAILABLE\"");
        let json = serde_json::to_string(&ErrorKind::DestinationUnwritable).unwrap();
        assert_eq!(json, "\"DESTINATION_UNWRITABLE\"");
        let json = serde_json::to_string(&NextAction::ChooseRestartOrKeep).unwrap();
        assert_eq!(json, "\"choose_restart_or_keep\"");
    }
}
