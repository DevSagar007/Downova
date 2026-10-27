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
    /// The destination file cannot be written: a name that is too long or invalid there, no
    /// permission, a read-only volume, a path that is not a folder (B1.6), or another file that
    /// appeared at the chosen final name, which is never overwritten (B1.7). The detail says which.
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

impl NextAction {
    /// Every action, in contract order.
    pub const ALL: [NextAction; 10] = [
        NextAction::None,
        NextAction::Retry,
        NextAction::Wait,
        NextAction::RefreshLink,
        NextAction::SignIn,
        NextAction::FreeDiskSpace,
        NextAction::ReconnectVolume,
        NextAction::InstallDesktopApp,
        NextAction::ChooseRestartOrKeep,
        NextAction::ContactSupport,
    ];
}

/// Error payload carried in JSON-RPC `error.data` and in failed job snapshots.
///
/// Locale-neutral: clients choose their own text from `kind` and `next_action` and never parse
/// `detail` (CORE-11, B1.7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ErrorData {
    pub kind: ErrorKind,
    /// The cause is transient: the same request may succeed later without any change. This is
    /// not a promise that the service retries; automatic retries follow
    /// [`crate::retry::is_auto_retryable`] (transfers) or the disk recheck (space, drives).
    pub retryable: bool,
    /// The step the user may take. `none` when there is nothing useful to offer.
    pub next_action: NextAction,
    /// Sanitized, user-safe detail. Must already be redacted.
    #[serde(default)]
    pub detail: Option<String>,
}

impl ErrorKind {
    /// Every kind, in contract order (catalogue completeness tests).
    pub const ALL: [ErrorKind; 19] = [
        ErrorKind::NetworkOffline,
        ErrorKind::AuthRequired,
        ErrorKind::LinkRefreshRequired,
        ErrorKind::RemoteChanged,
        ErrorKind::RangeUnsupported,
        ErrorKind::InvalidRange,
        ErrorKind::ChecksumMismatch,
        ErrorKind::DiskFull,
        ErrorKind::VolumeMissing,
        ErrorKind::DestinationUnwritable,
        ErrorKind::HostNotInstalled,
        ErrorKind::ScanBlocked,
        ErrorKind::BackendCapabilityUnavailable,
        ErrorKind::ServerBusy,
        ErrorKind::TlsHandshakeFailed,
        ErrorKind::ServiceUnavailable,
        ErrorKind::InvalidRequest,
        ErrorKind::IdempotencyConflict,
        ErrorKind::Internal,
    ];

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
