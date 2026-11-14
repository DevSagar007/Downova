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
            Supported,
            "B0.4.3: only with a strong ETag and a known length; at most 4 contiguous segments of at least 1 MiB; every segment request carries Range + If-Range + identity and its 206 must match the exact range, ETag, total length and Content-Length; any 200 restarts the whole file on one connection; any other violation stops all workers and publishes nothing; per-segment SHA-256 checkpoints for pause/resume (tests/segmented.rs G01-G16). A server that keeps the same ETag for different bytes cannot be detected without a trusted hash (G06)",
        ),
        entry(
            C::ResumeStrongValidator,
            Supported,
            "B0.4.2 (second increment): single connection; strong ETag + known length required (weak/missing refused); partial file re-verified by length and SHA-256; Range + If-Range to the final resource; 206 appended only with exact Content-Range, same ETag and length, identity encoding; a 200 restarts from zero (tests/validated_resume.rs V01-V15). A server that keeps the same ETag for different bytes cannot be detected without a trusted hash (V05)",
        ),
        entry(
            C::DurableResume,
            Supported,
            "B1.2 (ADR 0007): writers sync before publishing confirmed state; versioned, size-bounded checkpoints bound to the destination; import re-verifies every confirmed region by SHA-256, rebuilds hashers from disk and truncates unconfirmed single-connection tails; continuation uses the same validated Range/If-Range path (tests/durable_resume.rs R01-R08, service process tests). Transfers without a strong ETag and length are not resumable",
        ),
        entry(
            C::SpeedLimitGlobal,
            Supported,
            "B3.1 (CORE-06, ADR 0024): one token bucket shared by every download, applied before each write in bounded parts; live changes wake waiters; waits never run the read timeout (rate.rs unit tests; tests/speed_limits.rs steady-state throughput within ±10 % for 1, 3 and 6 downloads)",
        ),
        entry(
            C::SpeedLimitPerJob,
            Supported,
            "B3.1 (CORE-06, ADR 0024): one token bucket per download shared by all of its segments, coupled with the global one (tests/speed_limits.rs: 4 segments, live change, low rate without false timeouts)",
        ),
        entry(
            C::HttpProxy,
            Supported,
            "F06 (ADR 0034): one configured HTTP proxy for http and https (CONNECT), Basic sign-in; no system proxy, SOCKS5 or PAC; never a direct fallback (tests/proxy.rs: byte-exact through the proxy, 407 and refused tunnels, an unreachable proxy never reaches the origin)",
        ),
        entry(C::Socks5Proxy, Unsupported, "not implemented"),
        entry(C::PacProxy, Unsupported, "not implemented"),
        entry(C::Http2, Unsupported, "HTTP/1.1 only (ALPN http/1.1)"),
        entry(
            C::Ftp,
            Supported,
            "F05 (ADR 0035): anonymous FTP, passive data to the control peer only (EPSV, then PASV), SIZE/MDTM identity, REST resume only with an unchanged MDTM, no FTPS (tests/ftp.rs F01-F08)",
        ),
        entry(C::Sftp, Unsupported, "not implemented yet (F05 next step)"),
        entry(C::Bittorrent, Unsupported, "HTTP(S) backend only"),
        entry(C::Metalink, Unsupported, "HTTP(S) backend only"),
        entry(
            C::AuthenticatedGet,
            Supported,
            "B3.3 (BRW-05, ADR 0027): a scoped browser session for one origin; the Cookie header is computed per request (domain, path, Secure, expiry) for the probe, every redirect hop, every segment and every resume; a URL outside the origin (another host or port, https→http) stops the attempt with AUTH_SCOPE_UNSUPPORTED (tests/authenticated.rs)",
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use dm_domain::capability::missing_from;

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
                Capability::SegmentedHttp,
                Capability::ResumeStrongValidator,
                Capability::DurableResume,
                // CORE-06 (B3.1): tests/speed_limits.rs S01–S06 and rate.rs.
                Capability::SpeedLimitGlobal,
                Capability::SpeedLimitPerJob,
                // F06 (ADR 0034): tests/proxy.rs P01–P06.
                Capability::HttpProxy,
                // F05 (ADR 0035): tests/ftp.rs F01–F08.
                Capability::Ftp,
                // BRW-05 (B3.3): tests/authenticated.rs A01–A07.
                Capability::AuthenticatedGet
            ],
            "only entries whose acceptance tests pass"
        );
    }
}
