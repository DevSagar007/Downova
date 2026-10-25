# PROGRESS — handoff

_Last updated: 2026-10-25 (Windows 11 Pro 10.0.26300 x64, Node 26.7.0, pnpm 12.14.0 via npx, rustc 1.99.1, MSVC Build Tools 17.14)_

## Current milestone
- **B0.1–B0.3:** done (aria2 adapter, `dm-service` with user-only IPC, `dm` CLI). CI #2, #3 green.
- **B0.4** (`aee52e3`) and **B0.4.1** (`c5dc042`): CI runs #4 and #5 failed on macOS and both Ubuntu runners.
- **Cross-platform CI fixes** (`3e37105`): **CI run #6 green on all 4 runners**.
- **B0.4.2 first increment** — Rust HTTP backend `dm-backend-http` (`d0d040b`): **CI run #7 green on all 4 runners**
  (windows-latest, macos-latest, ubuntu-22.04, ubuntu-24.04; public job conclusions, logs not read).
- **B0.4.2 second increment** — validated single-connection pause/resume: committed as `4daa6ea`; verified on **Windows**;
  **CI run #8 in progress** at the time of writing.

Decisions: aria2 conditional go ([ADR 0003](docs/decisions/0003-aria2-go-no-go.md)); Rust HTTP backend prototype
([ADR 0004](docs/decisions/0004-rust-http-backend-prototype.md)). **aria2 stays the default backend**; the HTTP backend
is selected with `dm-service --backend http`.

## Capability reports today
| Capability | aria2 (default) | dm-backend-http (`--backend http`) |
|---|---|---|
| Http | supported (max-tries 1) | supported |
| Https | unverified (WinTLS cannot load a test CA) | supported (rustls + platform verifier) |
| ResumeStrongValidator | unsupported | **supported** (second increment) |
| SegmentedHttp | unsupported | unsupported (next increment) |

## Completed
### Earlier sessions (summary)
- Foundation (91 requirements, IDM-style UI, contracts, rules, skills), ADR 0002, CI on 4 OSes.
- B0.4: HTTP correctness matrix for aria2, adapter hardening, ADR 0003. B0.4.1: TLS-backend detection, If-Match evaluation (not safe in general), service-owned Retry-After/backoff with discard-before-retry, ADR 0004.
- CI fixes: aria2 exit monitor (no zombies on Unix); deterministic fixture transitions with per-request `served` tags.
- B0.4.2 first increment (`d0d040b`): `crates/backend-http` (reqwest 0.13.5 `rustls-no-provider` + `stream`, rustls 0.23.46 ring, rustls-platform-verifier 0.7.1), manual redirects with per-hop scheme gate, origin-scoped credentials/validators, identity encoding, exact length, status + Retry-After classification, `<name>.downova-part` published without overwrite, bounded stop; `crates/test-fixtures` (`dm-test-fixtures`) shared by both backends; `dm-service --backend aria2|http` (ring installed as process provider). [Report](docs/reports/b0.4.2-http-backend.md).

### This session — B0.4.2 second increment: validated pause/resume (`dm-backend-http`)
- **Identity** from the first `200`: strong ETag (weak/missing → none), length, final URL; identity encoding enforced.
- **Checkpoint** on stop: flushed offset + SHA-256 state of the partial bytes.
- **Resume:** requires strong ETag + length (else `BACKEND_CAPABILITY_UNAVAILABLE`, no request); re-verifies the partial (length + SHA-256; deleted/truncated/extended/modified → `INVALID_RANGE`, no request); `Range: bytes=<offset>-` + `If-Range` to the final URL (no redirects during resume; caller credentials only for the original origin); a `206` is appended only with identity encoding, the same strong ETag, exact `Content-Range`, matching `Content-Length`; a `200` restarts from zero; 3xx/416/412 are errors.
- Validated bytes of a cut `206` are flushed so the partial always equals reported progress.
- Fixture server: `If-Range` evaluation (`Route.if_range`).
- **Service:** the pause acknowledgement now reports the backend's confirmed bytes (`pause_reports_the_bytes_the_backend_confirmed`; found via the CLI test: 655 360 B reported vs 720 896 B confirmed).
- `ResumeStrongValidator` promoted on `dm-backend-http` after V01–V15 passed; aria2 unchanged.
- Docs: report section "Validated resume", ADR 0004 progress, architecture resume rules, requirements (COR-01, COR-02 → implemented), traceability, setup, testing, roadmap.

## Checks actually run — second increment (Windows 11, 2026-10-25, local)
| Command | Result |
|---|---|
| `cargo test -p dm-backend-http` ×5 | PASSED each run: unit 10, `http_backend` 24, `validated_resume` 15 |
| `cargo test -p dm-service --lib` | PASSED 20/20 |
| `cargo test -p dm-service --test http_backend_process -- --nocapture` | PASSED 2/2: pause at 737 280 B → resume `bytes=737280-` + `If-Range` → SHA-256 match; weak ETag resume refused, job stays paused |
| `cargo test -p dm-cli --test real_service -- --ignored --nocapture cli_pause` | PASSED: `dm pause`/`dm resume` → continuation `bytes=720896-` with `If-Range`, 6 MiB SHA-256 match |
| `pnpm verify:strict` | **15 passed, 0 failed, 0 unavailable**: cargo test incl. backend-http unit 10 + `http_backend` 24 + `validated_resume` 15, service 20 + `http_backend_process` 2; aria2 matrix 38/38; CLI real-service 3/3 (aria2, http, pause/resume) |
| GitHub CI for the second increment (`4daa6ea`) | run #8 in progress at the time of writing |

## Honest limitations
- A server that keeps the same strong ETag for different bytes (V05) yields a spliced file; only a trusted SHA-256 detects it.
- Resume needs a strong ETag and a length; otherwise the download can only restart. `Last-Modified` is not used.
- Resume works within a running service; jobs, checkpoints, retries and idempotency are in memory (B1 makes them durable).
- Automatic retries after a failure still restart from zero (discard + fresh attempt).
- `dm-backend-http`: no segmentation, no proxies, HTTP/1.1 only; public HTTPS sites not exercised by tests.
- aria2: HTTPS unverified, no resume/segmentation, http→https redirect gap (D04), dated bundled libraries (REL-05).
- Unknown-length truncation is undetectable without a trusted size or hash (both backends).
- The second increment has only Windows evidence.

## Blockers and open decisions
- REL-05: rebuilt aria2 sidecar needed before release; licence/advisory review of the reqwest/hyper/rustls/ring tree (REL-03).
- Default-backend decision (aria2 vs `dm-backend-http`) needs a later evidence-backed ADR.
- Open: project licence (REL-03); `downova.com` ownership if needed for signing.
- `gh` is not installed: CI job conclusions are read through the public API; job logs are not.

## Exact next task
1. Read CI run #8 (`4daa6ea`) and confirm all four runners green; fix any Unix-specific difference with the same proof-first approach.
2. **B0.4.3 — validated segmentation on `dm-backend-http`** (skill `dm-engine`): split only with a strong ETag and length; every segment request carries `Range` + `If-Range` and its `206` passes the same checks; a per-segment checkpoint; any `200`, ETag change or range error stops all segments without publishing; fixtures C05-style (change during a segmented transfer), ignored If-Range per segment, segment cut, cancel during segments. Promote `SegmentedHttp` only after those pass.
3. Then B0.5 packaging smoke, B0.6 browser spike, B1 (SQLite, durable jobs/checkpoints/retries/idempotency, recovery).
