# ADR 0003 — aria2 go/no-go: conditional go for single-connection HTTP

- **Status:** Accepted
- **Date:** 2026-10-21
- **Evidence:** [B0.4 HTTP correctness report](../reports/b0.4-http-correctness.md); [B0.1 report](../reports/b0.1-aria2-fixtures.md)
- **Related:** ADR 0001 (aria2 as the first engine), ADR 0002 (`dm-backend-api`)

## Context

ADR 0001 chose aria2 1.37.0 as the first `DownloadBackend` on the condition that a fixture gate (B0.4) shows where it is
safe. Until now every aria2 capability was `unverified`, so `dm-service` refused all adds against the real engine.

B0.4 ran a scripted HTTP/HTTPS fixture matrix against the pinned Windows build, with SHA-256 oracles and a mixed-bytes
detector. The findings that shape this decision:

1. **No If-Range, ever.** aria2 resumes (internal retries, pause/resume, control-file resume) with a bare `Range`
   request. When the resource changes between requests it splices old and new bytes and reports `complete`
   (C01–C06, with strong, weak, lying and missing ETags alike). This is the unsafe outcome the project exists to avoid.
2. **Internal retries are the main resume path.** With `--max-tries=1` there is no internal resume; a mid-transfer cut
   becomes an honest failure (C08, L01 production).
3. **`--continue=true` appends to any same-named file** without a control file. With `--continue=false` aria2 refuses
   (K03).
4. **`--header` values are sent to every redirect target**, including other origins (D02).
5. **Retry-After is ignored**; 429 and 503 end the attempt (S01, S02).
6. **TLS:** the pinned Windows build uses WinTLS (SChannel) and the OS trust store. It rejects self-signed and
   wrong-host certificates (T02, T03). `--ca-certificate` is not honoured on WinTLS, so trusted success against a
   controlled CA cannot be shown on Windows without modifying the OS store, which the tests never do (T01).
7. **Bundled libraries are dated:** libssh2 1.11.0, expat 2.5.0, c-ares 1.19.1 (see the report's audit).

## Decision

Conditional go. aria2 stays the first engine, behind adapter defaults that remove the unsafe paths, and capabilities are
promoted one by one:

| Capability | Status | Condition / reason |
|---|---|---|
| `Http` | **supported** | Only while the adapter runs with `--max-tries=1`, `--continue=false`, a single connection, no credential headers, and the service refuses partial resume (`ResumeStrongValidator` unsupported). `capabilities::report` derives this from the launch config: any other `max_tries` reports `unverified`. |
| `Https` | **unverified** | Rejection is proven (T02, T03); trusted success is not proven on the Windows WinTLS build. Promote per TLS backend only after T01 passes on that build (OpenSSL/GnuTLS on Linux CI) and the bundled-library audit is resolved. |
| `SegmentedHttp` | **unsupported** | C05: a change during a segmented transfer is spliced silently. The service uses one connection. |
| `ResumeStrongValidator` | **unsupported** | C01–C04, C06: aria2 sends no If-Range and does not compare validators. The service therefore never resumes a partial file; it restarts or fails. |

Adapter defaults in `crates/backend-aria2/src/lib.rs` (B0.4):

- `max_tries = 1` (was 5); `--continue=false` (was true);
- `start()` refuses `Authorization`, `Cookie`, `Proxy-Authorization` and similar headers with
  `BACKEND_CAPABILITY_UNAVAILABLE` unless `forward_credential_headers_unsafe` is set (tests only);
- error classification reads only the `status=NNN` digits and the TLS/interruption markers from aria2's
  `errorMessage`, never the message itself (it contains the raw URL).

Not chosen:

- **Engine rewrite or replacement now.** Single-connection HTTP is safe under the conditions above, and the failures are
  known and contained by gates.
- **Promoting `SegmentedHttp` or resume with a wrapper-only check.** A length or prefix check cannot detect a
  same-length change in a segment that was never compared. Only per-request validator checks (If-Range) can.

## Requirements for a full go

Each is a separate, tracked change:

1. **Validated resume and segment consistency** (COR-01, COR-02, CORE-02): patch aria2 to send `If-Range` with a strong
   validator and fail on `200`, or move ranged transfers to a custom transport. Re-run C01–C06 and expect safe outcomes.
2. **Redirect-scoped credentials** (BRW-05, SEC-01): an engine patch or a transport that drops credentials on
   cross-origin redirects. Re-run D02 and expect no leak.
3. **Retry and backoff in the service** (COR-04, B1): honour `Retry-After`, with bounded backoff, as new attempts.
4. **Rebuilt sidecar before release** (REL-05): aria2 built with current libssh2 (≥ 1.11.1), expat (≥ 2.6.3) and
   c-ares (≥ 1.27.0), with recorded provenance and a TLS backend decision per OS.
5. **HTTPS evidence per TLS backend** (CORE-01): T01 passing on each shipped build.
6. **Durable recovery** (B1): SQLite job records; reconciliation of engine state after a service crash.

## Consequences

- `dm add http://…` now runs end to end through `dm-service` and aria2 (SHA-256 verified in
  `crates/cli/tests/real_service.rs` and `crates/service/tests/process.rs`). `https://` is still refused honestly.
- Interrupted transfers fail with `NETWORK_OFFLINE` instead of resuming; the user retries from zero. This costs
  bandwidth on unreliable networks until requirement 1 lands, which is the right trade-off over silent corruption.
- Credentialed browser downloads (BRW-05) remain blocked on requirement 2. URL userinfo is not forwarded across origins (D05).
- **Known gap:** aria2 follows `http://` → `https://` redirects (D04), so an HTTP job can complete over HTTPS although
  `Https` is unverified. Certificate verification still applies (self-signed is refused), and aria2 has no option to
  stop following redirects. Closing this needs the same engine patch or transport as requirement 2.
- New error kinds `SERVER_BUSY` and `TLS_HANDSHAKE_FAILED` are part of the contract.
- The B0.1 report's statement that the pinned build bundles OpenSSL is corrected: the archive contains a
  `LICENSE.OpenSSL` file, but the binary uses WinTLS.
