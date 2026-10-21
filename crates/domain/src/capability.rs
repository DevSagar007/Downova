//! Backend capability reporting.
//!
//! A backend reports every product capability explicitly. `Unverified` means "documented
//! upstream but not yet proven by this project's fixtures" and must not be presented to users as
//! supported. Missing capabilities are an error ([`ErrorKind::BackendCapabilityUnavailable`]),
//! never a silent fallback.
//!
//! [`ErrorKind::BackendCapabilityUnavailable`]: crate::ErrorKind::BackendCapabilityUnavailable

use serde::{Deserialize, Serialize};

/// Product-level capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    Http,
    Https,
    SegmentedHttp,
    /// Resume guarded by a strong validator (If-Range with a strong ETag or equivalent).
    ResumeStrongValidator,
    SpeedLimitGlobal,
    SpeedLimitPerJob,
    HttpProxy,
    Socks5Proxy,
    PacProxy,
    Http2,
    Ftp,
    Sftp,
    Bittorrent,
    Metalink,
}

impl Capability {
    /// Every capability, so reports can be checked for completeness.
    pub const ALL: [Capability; 14] = [
        Capability::Http,
        Capability::Https,
        Capability::SegmentedHttp,
        Capability::ResumeStrongValidator,
        Capability::SpeedLimitGlobal,
        Capability::SpeedLimitPerJob,
        Capability::HttpProxy,
        Capability::Socks5Proxy,
        Capability::PacProxy,
        Capability::Http2,
        Capability::Ftp,
        Capability::Sftp,
        Capability::Bittorrent,
        Capability::Metalink,
    ];
}

/// Support level of a capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilitySupport {
    Supported,
    Unsupported,
    Unverified,
}

/// One entry of a backend capability report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityReport {
    pub capability: Capability,
    pub support: CapabilitySupport,
    #[serde(default)]
    pub note: Option<String>,
}

/// Looks up a capability in a report. A capability absent from the report counts as unsupported.
pub fn support_of(report: &[CapabilityReport], capability: Capability) -> CapabilitySupport {
    report
        .iter()
        .find(|entry| entry.capability == capability)
        .map_or(CapabilitySupport::Unsupported, |entry| entry.support)
}

/// Returns the capabilities missing from a report; a complete report lists all of them.
pub fn missing_from(report: &[CapabilityReport]) -> Vec<Capability> {
    Capability::ALL
        .into_iter()
        .filter(|cap| !report.iter().any(|entry| entry.capability == *cap))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_capability_is_unsupported_not_assumed() {
        let report = vec![CapabilityReport {
            capability: Capability::Http,
            support: CapabilitySupport::Unverified,
            note: None,
        }];
        assert_eq!(support_of(&report, Capability::Http), CapabilitySupport::Unverified);
        assert_eq!(support_of(&report, Capability::Socks5Proxy), CapabilitySupport::Unsupported);
        let missing = missing_from(&report);
        assert!(!missing.contains(&Capability::Http));
        assert!(missing.contains(&Capability::Socks5Proxy));
    }
}
