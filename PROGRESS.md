# PROGRESS — handoff

_Last updated: 2026-10-21 (Windows 11 Pro 10.0.26300 x64, Node 26.7.0, pnpm 12.14.0 via npx, rustc 1.99.1, MSVC Build Tools 17.14)_

## Current milestone
- **B0.1** (aria2 adapter): done.
- **B0.2** (service + IPC): done; CI #2 (`73fe021`) green on all 4 runners.
- **B0.3** (`dm` CLI): done; committed as `371c23a`; **CI #3 for `371c23a` succeeded on all 4 runners** (windows-latest, macos-latest, ubuntu-22.04, ubuntu-24.04).
- **B0.4** (HTTP correctness + ADR 0003): done locally on Windows; **not committed, no CI run yet**.

**B0 gate result: conditional go** ([ADR 0003](docs/decisions/0003-aria2-go-no-go.md)). Next: **B0.5 packaging smoke**, then B0.6 browser spike, then **B1** ([docs/roadmap.md](docs/roadmap.md)).

## Completed
### Earlier sessions
- Foundation: documentation (91 requirements), the IDM-style desktop UI (16 vitest tests), contracts, rules, skills.
- Rust 1.99.1 and VS Build Tools 17.14 installed (user-approved). ADR 0002. CI on 4 OSes.
- **B0.1:** aria2 1.37.0 adapter and 8 fixtures ([report](docs/reports/b0.1-aria2-fixtures.md)).
- **B0.2 (`73fe021`):** `dm-platform` user-only IPC; `dm-service` (single instance, JSON-RPC, in-memory jobs and idempotency, capability gates, attempt guards).
- **B0.3 (`371c23a`):** `dm` CLI (`crates/cli`): args, hello handshake, idempotent add with one retry, human/JSON output with redaction, exit codes 0–4; 19 tests + 1 real-service test.

### This session — B0.4 (evidence kept separate from B0.3)
- **Fixture server** (`crates/backend-aria2/tests/support/{mod,tls}.rs`): scriptable HTTP/HTTPS routes (range modes, ETag, Content-Length on/off, truncation, Content-Encoding, Content-Disposition, redirects, scripted statuses with Retry-After, slow bodies, live body swaps) with a request log. TLS via rustls + rcgen (test CA, leaf, self-signed).
- **Matrix** (`crates/backend-aria2/tests/http_correctness.rs`): 27 tests, SHA-256 oracle plus a mixed-bytes detector, each case classified safe success / safe failure / unsafe / not tested. Results: [B0.4 report](docs/reports/b0.4-http-correctness.md).
- **Adapter fixes** (`crates/backend-aria2/src`):
  - `max_tries` default 5 → **1** (removes aria2's unvalidated internal resume; C08 now fails safely);
  - `--continue=true` → **false** (never appends to a foreign file; K03);
  - credential headers refused unless `forward_credential_headers_unsafe` (tests only), because aria2 forwards them cross-origin (D02/D03);
  - `classify(code, message)`: 401/403/410/429/5xx behind code 22, 503 (29), TLS handshake, interruption ("No URI available") — numbers and markers only, the message is never forwarded;
  - `ca_certificate` launch option; `pid()` accessor for kill tests;
  - `capabilities::report(max_tries)`: **Http supported** only with max-tries 1.
- **Contract:** `ErrorKind` + `SERVER_BUSY`, `TLS_HANDSHAKE_FAILED` (schema, generated TS, `crates/domain/src/error_kind.rs`).
- **Review fixes:** stale resume gate (`resume` now reads the engine's current progress before deciding; regression test failed before the fix); `status=` parsing now uses the last occurrence (a URL query could change the classification); new D04 (http→https redirect bypasses the Https gate, certificate checks still apply) and D05 (userinfo not sent cross-origin).
- **Service process tests** (`crates/service/tests/process.rs`, 6): capabilities + https gate, a real SHA-256-verified HTTP download, engine killed mid-transfer (honest failure, partial kept, no false completion), service killed mid-transfer (no orphan aria2c).
- **CLI real-service test:** https refused (exit 4, no job/file/secret); **`dm add http://<fixture>` → completed, SHA-256 match**.
- `scripts/verify.mjs`: new `aria2` step for `http_correctness`.
- Docs: ADR 0003, B0.4 report, B0.1 report correction (no OpenSSL), requirements (COR-02/03/04, E04 → in-progress), traceability, setup, testing, architecture, roadmap, README.

## Checks actually run (B0.4, Windows 11, 2026-10-21)
| Command | Result |
|---|---|
| `cargo test -p dm-backend-aria2 --locked --test http_correctness -- --ignored --nocapture` | PASSED 27/27 (asserts the documented outcome, including the unsafe engine cases) |
| `cargo test -p dm-service --test process -- --ignored --nocapture` | PASSED 6/6 (real download `sha256` match) |
| `cargo test -p dm-cli --test real_service -- --ignored --nocapture` | PASSED 1/1: `dm add` 2 MiB → completed, sha256 `4d8a372d…7441b` = payload |
| `pnpm verify:strict` | **15 passed, 0 failed, 0 unavailable** (structure, contracts, format, lint, typecheck ×2, vitest, vite build, cargo fmt, clippy `-D warnings`, cargo test, aria2 fixtures, http_correctness, service process, CLI real-service) |
| GitHub CI for B0.4 | **not run** (uncommitted) |

## Honest limitations
- **HTTPS is not usable yet** (Unverified): rejection of bad certificates is proven, trusted success is not provable on the Windows WinTLS build without changing the OS trust store.
- **No resume.** Any interruption fails with `NETWORK_OFFLINE`; the user restarts from zero. Validated resume needs an aria2 patch (If-Range) or a custom transport.
- **Single connection only** (SegmentedHttp unsupported).
- An `http://` job that redirects to `https://` is followed by aria2 although `Https` is unverified (D04; certificate checks still apply).
- **Credentialed downloads are refused** (aria2 forwards headers across origins).
- **429/503 are not retried**; Retry-After is ignored by aria2 and the service has no retry loop yet (B1).
- **Unknown-length truncation (L03) is indistinguishable from success** without a trusted hash; this is protocol-inherent.
- K02 resumed from a control file but the partial had not been flushed (disk cache lost on kill), so a non-zero-offset crash resume was not exercised.
- Idempotency and jobs are in-memory; durable recovery is B1. Cross-user IPC denial is untested. The desktop UI is not connected (B2).
- Only Windows has run the B0.4 checks. macOS/Linux behaviour (and T01 on OpenSSL/GnuTLS builds) is unverified until CI runs.

## Blockers and open decisions
- **REL-05:** the pinned aria2 bundles libssh2 1.11.0, expat 2.5.0 and c-ares 1.19.1, which predate published security fixes. A rebuilt sidecar is required before any release.
- Open decisions: project license (REL-03); `downova.com` ownership if needed for signing.
- pnpm is not installed globally. Use `npx pnpm@12.14.0`.

## Exact next task
1. Commit B0.4 and push so CI runs the matrix on all four runners (needs the user's go-ahead); record T01 on the Linux OpenSSL/GnuTLS build.
2. **B0.5** packaging smoke on each OS (Tauri bundle starts, sidecar launches), then **B0.6** browser spike (public GET only; credentialed capture stays blocked by ADR 0003).
3. **B1 (skill `dm-engine`)**, in order:
   - `dm-storage` SQLite (service is the only writer): durable jobs, attempts and idempotency records (COR-08, COR-09);
   - service-side retry policy with backoff and Retry-After (COR-04);
   - crash recovery and engine-state reconciliation;
   - plan the If-Range engine patch or custom transport needed for resume and segmentation (ADR 0003, requirement 1).
