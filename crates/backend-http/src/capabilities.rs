//! Capability report for the Rust HTTP backend (ARCH-04, ADR 0004).
//!
//! Every entry starts `Unverified` or `Unsupported`; an entry becomes `Supported` only with the
//! acceptance evidence named in its note (docs/reports/b0.4.2-http-backend.md).

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
    use CapabilitySupport::{Supported, Unsupported};
    vec![
        entry(
            C::Http,
            Supported,
            "B0.4.2: single connection, whole-file transfers; identity encoding, exact Content-Length, manual redirects with per-hop scheme gate and origin-scoped credentials (tests/http_backend.rs H, S, D, X, Z cases). No resume of partial data; unknown-length truncation is undetectable without a trusted size or hash (H05)",
        ),
        entry(
            C::Https,
            Supported,
            "B0.4.2: rustls (ring) with the platform verifier; a fixture CA is trusted only as an explicit extra root (T01); self-signed, wrong host name and an unsupplied CA are rejected (T02-T04). Certificate verification is never disabled",
        ),
        entry(
            C::SegmentedHttp,
            Unsupported,
            "not implemented in this increment; requests for more than one connection are refused",
        ),
        entry(
            C::ResumeStrongValidator,
            Supported,
            "B0.4.2 (second increment): single connection; strong ETag + known length required (weak/missing refused); partial file re-verified by length and SHA-256; Range + If-Range to the final resource; 206 appended only with exact Content-Range, same ETag and length, identity encoding; a 200 restarts from zero (tests/validated_resume.rs V01-V15). A server that keeps the same ETag for different bytes cannot be detected without a trusted hash (V05)",
        ),
        entry(C::SpeedLimitGlobal, Unsupported, "not implemented"),
        entry(C::SpeedLimitPerJob, Unsupported, "not implemented"),
        entry(
            C::HttpProxy,
            Unsupported,
            "proxies are disabled (no system proxy, no configuration); never a silent direct fallback",
        ),
        entry(C::Socks5Proxy, Unsupported, "not implemented"),
        entry(C::PacProxy, Unsupported, "not implemented"),
        entry(C::Http2, Unsupported, "HTTP/1.1 only (ALPN http/1.1)"),
        entry(C::Ftp, Unsupported, "HTTP(S) backend only"),
        entry(C::Sftp, Unsupported, "HTTP(S) backend only"),
        entry(C::Bittorrent, Unsupported, "HTTP(S) backend only"),
        entry(C::Metalink, Unsupported, "HTTP(S) backend only"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use dm_domain::capability::{missing_from, support_of};

    #[test]
    fn report_is_complete_and_promotes_only_proven_entries() {
        let r = report();
        assert!(missing_from(&r).is_empty());
        assert_eq!(r.len(), Capability::ALL.len());
        let supported: Vec<_> = r
            .iter()
            .filter(|e| e.support == CapabilitySupport::Supported)
            .map(|e| e.capability)
            .collect();
        assert_eq!(
            supported,
            vec![
                Capability::Http,
                Capability::Https,
                Capability::ResumeStrongValidator
            ],
            "only entries whose acceptance tests pass"
        );
        assert_eq!(
            support_of(&r, Capability::SegmentedHttp),
            CapabilitySupport::Unsupported
        );
    }
}
