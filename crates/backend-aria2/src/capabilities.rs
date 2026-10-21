//! Capability report for the aria2 adapter (ARCH-04), decided by ADR 0003 (B0.4).
//!
//! Rule: upstream documentation alone gives `Unverified`; only project fixtures promote an entry
//! to `Supported`, and a fixture that shows unsafe behaviour makes it `Unsupported`. Evidence:
//! docs/reports/b0.4-http-correctness.md. The report depends on the launch configuration, because
//! the `Http` evidence holds only with engine-internal retries disabled.

use dm_domain::{Capability, CapabilityReport, CapabilitySupport};

fn entry(capability: Capability, support: CapabilitySupport, note: &str) -> CapabilityReport {
    CapabilityReport {
        capability,
        support,
        note: Some(note.to_owned()),
    }
}

/// Complete report: every [`Capability`] appears exactly once. `max_tries` is the adapter's
/// aria2 `--max-tries`.
pub fn report(max_tries: u32) -> Vec<CapabilityReport> {
    use Capability as C;
    use CapabilitySupport::{Supported, Unsupported, Unverified};
    let http = if max_tries == 1 {
        entry(
            C::Http,
            Supported,
            "ADR 0003: single connection, no engine-internal retry; B0.4 fixtures R01-R04, L01-L04, C08, N01, S01-S03 safe. Limits: no resume of partial data; unknown-length truncation undetectable without a trusted hash",
        )
    } else {
        entry(
            C::Http,
            Unverified,
            "engine-internal retries enabled: a retry resumes without a validator (B0.4 C01-C04 unsafe)",
        )
    };
    vec![
        http,
        entry(
            C::Https,
            Unverified,
            "B0.4: invalid/self-signed certificates rejected (T02); a controlled trusted-certificate transfer is not proven on WinTLS (T01: --ca-certificate not honoured). Promotion needs T01 evidence per TLS backend",
        ),
        entry(
            C::SegmentedHttp,
            Unsupported,
            "B0.4 C05: segments fetched at different times mix old and new representations; aria2 sends no If-Range",
        ),
        entry(
            C::ResumeStrongValidator,
            Unsupported,
            "B0.4 C01-C04, C06: resume sends a bare Range with no If-Range for strong, weak or missing ETags",
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
            "Pinned Windows build links libssh2 1.11.0 (pre-Terrapin fix); fixtures planned for F05 (B4)",
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
    fn report_is_complete_and_follows_adr_0003() {
        for tries in [1, 5] {
            let report = report(tries);
            assert!(
                missing_from(&report).is_empty(),
                "every capability must be reported"
            );
            assert_eq!(report.len(), Capability::ALL.len(), "no duplicates");
            assert_eq!(
                support_of(&report, Capability::ResumeStrongValidator),
                CapabilitySupport::Unsupported
            );
            assert_eq!(
                support_of(&report, Capability::SegmentedHttp),
                CapabilitySupport::Unsupported
            );
            assert_eq!(
                support_of(&report, Capability::Https),
                CapabilitySupport::Unverified
            );
            assert_eq!(
                support_of(&report, Capability::Http2),
                CapabilitySupport::Unsupported
            );
        }
        assert_eq!(
            support_of(&report(1), Capability::Http),
            CapabilitySupport::Supported
        );
        assert_eq!(
            support_of(&report(5), Capability::Http),
            CapabilitySupport::Unverified
        );
        let supported: Vec<_> = report(1)
            .into_iter()
            .filter(|e| e.support == CapabilitySupport::Supported)
            .map(|e| e.capability)
            .collect();
        assert_eq!(supported, vec![Capability::Http], "only Http is promoted");
    }
}
