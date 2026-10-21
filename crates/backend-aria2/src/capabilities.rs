//! Capability report for the aria2 adapter (ARCH-04).
//!
//! Rule: upstream documentation alone gives `Unverified`; only project fixtures promote an entry to
//! `Supported` (and that promotion happens with the B0 go/no-go, ADR 0003). Known gaps are
//! `Unsupported` with the reason, so features can refuse them explicitly.

use dm_domain::{Capability, CapabilityReport, CapabilitySupport};

fn entry(capability: Capability, support: CapabilitySupport, note: &str) -> CapabilityReport {
    CapabilityReport {
        capability,
        support,
        note: Some(note.to_owned()),
    }
}

/// Complete report: every [`Capability`] appears exactly once.
pub fn report() -> Vec<CapabilityReport> {
    use Capability as C;
    use CapabilitySupport::{Unsupported, Unverified};
    vec![
        entry(
            C::Http,
            Unverified,
            "B0.1 fixtures pass locally; promotion pending ADR 0003",
        ),
        entry(
            C::Https,
            Unverified,
            "Documented upstream; no TLS fixture yet",
        ),
        entry(
            C::SegmentedHttp,
            Unverified,
            "B0.1 fixtures pass locally; range-ignore/changed-resource matrix pending (B0.4)",
        ),
        entry(
            C::ResumeStrongValidator,
            Unsupported,
            "aria2 conditional-get is If-Modified-Since and is ignored with a control file; no If-Range",
        ),
        entry(
            C::SpeedLimitGlobal,
            Unverified,
            "Documented upstream (max-overall-download-limit)",
        ),
        entry(
            C::SpeedLimitPerJob,
            Unverified,
            "Documented upstream (max-download-limit)",
        ),
        entry(
            C::HttpProxy,
            Unverified,
            "Documented upstream (HTTP proxy, Basic auth only)",
        ),
        entry(
            C::Socks5Proxy,
            Unsupported,
            "Not in aria2's documented feature list",
        ),
        entry(
            C::PacProxy,
            Unsupported,
            "Not in aria2's documented feature list",
        ),
        entry(C::Http2, Unsupported, "aria2 documents HTTP/1.1 only"),
        entry(
            C::Ftp,
            Unverified,
            "Documented upstream; fixtures planned for F05 (B4)",
        ),
        entry(
            C::Sftp,
            Unverified,
            "Depends on the build's libssh2; fixtures planned for F05 (B4)",
        ),
        entry(
            C::Bittorrent,
            Unverified,
            "Documented upstream; product scope F11 (B6)",
        ),
        entry(
            C::Metalink,
            Unverified,
            "Documented upstream; product scope F12 (B6)",
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use dm_domain::capability::{missing_from, support_of};

    #[test]
    fn report_is_complete_and_never_claims_unproven_support() {
        let report = report();
        assert!(
            missing_from(&report).is_empty(),
            "every capability must be reported"
        );
        assert_eq!(report.len(), Capability::ALL.len(), "no duplicates");
        assert!(
            report
                .iter()
                .all(|e| e.support != CapabilitySupport::Supported),
            "nothing is Supported until fixtures + ADR 0003 promote it"
        );
        assert_eq!(
            support_of(&report, Capability::ResumeStrongValidator),
            CapabilitySupport::Unsupported
        );
        assert_eq!(
            support_of(&report, Capability::Http2),
            CapabilitySupport::Unsupported
        );
    }
}
