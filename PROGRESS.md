# PROGRESS — handoff

_Last updated: 2026-10-25 (Windows 11 Pro 10.0.26300 x64, Node 26.7.0, pnpm 12.14.0 via npx, rustc 1.99.1, MSVC Build Tools 17.14)_

## Current milestone
- **B0.1** (aria2 adapter): done.
- **B0.2** (service + IPC): done; CI #2 (`73fe021`) green on all 4 runners.
- **B0.3** (`dm` CLI): done; `371c23a`; CI #3 green on all 4 runners.
- **B0.4** (HTTP correctness + ADR 0003): done; committed as `aee52e3`. CI result for `aee52e3` not checked from this machine (`gh` is not installed).
- **B0.4.1** (engine remediation): done locally on Windows; **not committed, no CI run**.

**B0 gate:** aria2 conditional go ([ADR 0003](docs/decisions/0003-aria2-go-no-go.md)); resume/segmentation cannot be made safe through aria2's interface, so a Rust HTTP backend prototype is decided ([ADR 0004](docs/decisions/0004-rust-http-backend-prototype.md)).

## Completed
### Earlier
- Foundation (91 requirements, IDM-style UI with 16 vitest tests, contracts, rules, skills); Rust 1.99.1 + VS Build Tools (user-approved); ADR 0002; CI on 4 OSes.
- **B0.1–B0.3:** aria2 adapter, `dm-service` with user-only IPC, `dm` CLI.
- **B0.4 (`aee52e3`):** fixture server + HTTP/HTTPS matrix, adapter defaults (`max-tries 1`, `--continue=false`, credential headers refused), classification, `SERVER_BUSY`/`TLS_HANDSHAKE_FAILED`, Http promoted, review fixes (fresh resume gate, last `status=`), D04/D05. [Report](docs/reports/b0.4-http-correctness.md).

### This session — B0.4.1 (evidence kept separate from B0.4)
- **HTTPS investigated separately from resume.**
  - `binary::TlsBackend` + `parse_tls`: the TLS backend is read from `aria2c --version`; the pinned build is **WinTLS (SChannel, OS trust store)** (T00).
  - Cause of T01: upstream `WinTLSContext::addTrustedCACertFile` logs "TLS CA bundle files are not supported" and returns false.
  - Fix: `Aria2Backend::launch` refuses a CA bundle that the build would ignore (`BACKEND_CAPABILITY_UNAVAILABLE`); the `Https` note names the TLS backend. T01/T03 assert the refusal on WinTLS and keep the success/host-name expectations for OpenSSL/GnuTLS builds.
  - `Https` stays unverified for aria2.
- **If-Match evaluation (M01–M10):** strong-ETag `If-Match` via the existing header interface. Conforming servers make changes fail safely (412), unchanged resume and 4-connection transfers succeed, **but a server that ignores If-Match still yields a spliced "complete" file (M04, unsafe)**; aria2 drops the status on resumed requests (M02/M07), forwards If-Match to cross-origin redirects (M09), and weak ETags are unusable (M08). Verdict: `ResumeStrongValidator` and `SegmentedHttp` stay **unsupported**. `status=412` now maps to `REMOTE_CHANGED`. Fixture server: `Preconditions::{Honour, Ignore}`.
- **Service-owned retry (COR-04):**
  - `dm_domain::retry`: policy (transient kinds only, 3 retries, Retry-After honoured exactly up to 15 min, else 2 s × 2ⁿ capped 60 s with equal jitter), `parse_retry_after` (delta-seconds, IMF-fixdate).
  - `BackendError.retry_after`; new trait method `DownloadBackend::discard` (removes the transfer and deletes its partial + engine resume state), implemented for aria2 and the fake.
  - `Core`: failed attempt → discard → `queued` with `rate_limit_cooldown` (429/503) → fresh attempt when due; pause/resume/cancel respect the cool-down.
  - aria2 cannot see `Retry-After`, so with aria2 only backoff applies.
- **ADR 0004:** prototype `dm-backend-http` (Tokio, reqwest, rustls) behind `DownloadBackend`; per-capability promotion after the same matrix.

## Checks actually run (B0.4.1, Windows 11, 2026-10-25)
| Command | Result |
|---|---|
| `cargo test -p dm-backend-aria2 --test http_correctness -- --ignored --nocapture t0` | PASSED 4/4 (T00 WinTLS; T01/T03 launch refused; T02 rejected) |
| `cargo test -p dm-backend-aria2 --test http_correctness -- --ignored --nocapture m0` (+ `m10`) | PASSED 10/10, identical outcomes in two consecutive runs |
| `cargo test -p dm-domain --lib retry` | PASSED 6/6 |
| `cargo test -p dm-service --lib` | PASSED 19/19 (4 new retry tests with paused time) |
| `cargo test -p dm-service --test process -- --ignored --nocapture transient` | PASSED: 503 and a mid-body cut each → 2 requests, 0 with `Range`, completed, SHA-256 match |
| `pnpm verify:strict` | see below |
| GitHub CI for B0.4.1 | **not run** (uncommitted) |

## Honest limitations
- **HTTPS is not usable with aria2 yet** (unverified). On Windows, trusted success needs a CA in the OS store, which tests never add.
- **No resume and no segmentation with aria2** (unsafe against servers that ignore conditions). Interrupted transfers are retried from zero automatically (at most 3 times).
- Retry-After is honoured only when a backend reports it; aria2 never does, so 429/503 with aria2 get backoff.
- An `http://` job that redirects to `https://` is followed by aria2 although `Https` is unverified (D04).
- Credentialed downloads are refused (aria2 forwards headers across origins).
- Unknown-length truncation (L03) is indistinguishable from success without a trusted hash.
- Jobs, retries and idempotency are in memory; durable recovery is B1. The desktop UI is not connected (B2).
- Only Windows ran these checks.

## Blockers and open decisions
- **REL-05:** the pinned aria2 bundles libssh2 1.11.0, expat 2.5.0 and c-ares 1.19.1, which predate published security fixes.
- ADR 0004 adds reqwest/rustls/hyper: licence and advisory review needed when the crate is created.
- Open decisions: project licence (REL-03); `downova.com` ownership if needed for signing.
- pnpm is not installed globally (`npx pnpm@12.14.0`); `gh` is not installed, so CI status is not visible here.

## Exact next task
1. Commit B0.4.1 and push (user's go-ahead) so CI runs the matrix on all four runners; record T01/T03 on the Linux GnuTLS build.
2. **B0.4.2 (skill `dm-engine`, ADR 0004):** create `crates/backend-http` (`dm-backend-http`) implementing `DownloadBackend` with single-connection HTTP first; make the fixture matrix backend-generic; run R/L/C/M/S/D/N/T/K against it; promote capabilities individually.
3. Then B0.5 packaging smoke, B0.6 browser spike, B1 (SQLite, durable retries and idempotency, recovery).
