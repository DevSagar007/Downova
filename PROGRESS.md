# PROGRESS — handoff

_Last updated: 2026-10-21 (Windows 11 Pro 10.0.26300 x64, Node 26.7.0, pnpm 12.14.0 via npx, rustc 1.99.1, MSVC Build Tools 17.14)_

## Current milestone
- **B0.1 (aria2 adapter + fixtures): done.**
- **B0.2 (service process + IPC): done on Windows.**
- Next: **B0.3 — CLI harness** ([docs/roadmap.md](docs/roadmap.md)).

## Completed
### Earlier sessions
- Foundation:
  - documentation (91 requirements);
  - IDM-style desktop UI with 16 vitest tests;
  - contracts and Claude rules/skills.
- Prerequisites: Rust 1.99.1 and VS Build Tools 17.14 were installed with the user's approval.
- ADR 0002:
  - the `dm-backend-api` crate;
  - Downova naming;
  - the stable identifier `com.downova.desktop`.
- CI workflow for four OSes.
- B0.1: the aria2 1.37.0 adapter and 8 fixture tests ([report](docs/reports/b0.1-aria2-fixtures.md)).

### This session — B0.2
- **Contracts:**
  - Added `RpcRequest`, `RpcResponse`, `RpcError`, `JobRef`, `ListDownloadsResult` and `EmptyResult`.
  - The method set is closed: hello, shutdown, add, list, get, pause, resume, cancel. There is no engine passthrough.
  - Examples: 9 valid and 4 invalid. The Rust mirror is in `crates/domain/src/api.rs` (`methods`, `codes`). `ErrorKind` gained `default_next_action` and `to_error_data`.
- **`crates/platform` (`dm-platform`, new):**
  - per-user data, run and download paths;
  - private file writes;
  - the endpoint discovery file;
  - local IPC. On Windows this is a named pipe `downova-<random>` with the owner-only DACL `D:P(A;;GA;;;OW)(A;;GA;;;SY)` that rejects remote clients. On Unix it is a socket in a 0700 run directory (0600 on Linux).
  - Line-framed JSON-RPC with a 1 MiB cap, a `Client`, and `CallError` (`SERVICE_UNAVAILABLE` for transport failures).
- **`crates/service` (`dm-service`):**
  - **`core.rs`** holds in-memory jobs:
    - service-owned `job_`/`att_` IDs;
    - **in-memory idempotency** (durable idempotency is B1);
    - capability gates: `add` needs `Http`/`Https` = supported; 4 connections only if `SegmentedHttp` is supported; resuming partial data needs `ResumeStrongValidator`;
    - attempt guards: late events are ignored and cancel is acknowledged only after backend `remove`;
    - completion checks: length and an optional trusted SHA-256;
    - URL scheme and host validation without echoing the URL;
    - sanitized, unique file names;
    - redacted snapshots.
  - **`rpc.rs`** dispatches requests. **`server.rs`** runs the accept loop and a 400 ms progress poller.
  - **`main.rs`** is the `dm-service` binary:
    - single instance through a `service.lock` file lock;
    - launches aria2;
    - writes the endpoint file;
    - stops on Ctrl+C or `service.v1.shutdown`;
    - exit codes 0/2/3/4.
- **`aria2c` lookup** now also checks the service executable's directory (packaged sidecar), plus a new `binary::locate_at`.
- **`verify.mjs`:** the `aria2` group also runs the real-process service tests.
- Docs updated: architecture §2/§5/§6 (IPC as implemented, gates, in-memory idempotency), project-structure, roadmap, testing and setup ("Running the download service").
- Requirements:
  - now **implemented**: ARCH-03, ARCH-04, ARCH-06;
  - still **verified**: ARCH-07;
  - **in-progress, with new evidence**: ARCH-01, CORE-01, CORE-03, COR-09, SEC-01.

## Checks actually run (Windows 11, 2026-10-21)
| Command | Result |
|---|---|
| `cargo test -p dm-platform` | PASSED 3 (real named-pipe round trip, unavailable/stale endpoint, private file) |
| `cargo test -p dm-service` (lib) | PASSED 13 (10 core with a fake backend + 2 over real IPC + file names). Covers idempotency same/different key, **lost acknowledgement then retry gives one job**, gate refusal leaves no job, segmentation gate, partial-resume refusal, cancel guard against a late event, hash verification and redaction |
| `cargo test -p dm-service --test process -- --include-ignored` | PASSED 3. The real `dm-service` + aria2 1.37.0: hello reports `aria2` with Http=Unverified and ResumeStrongValidator=Unsupported; a second instance exits 3; `add` is **refused: BACKEND_CAPABILITY_UNAVAILABLE "Http is Unverified for this backend"** (synthetic secret not echoed); unknown job → INVALID_REQUEST; shutdown → exit 0 and endpoint file removed. Missing aria2 → exit 4; bad args → exit 2 |
| `pnpm verify:strict` | **13 passed, 0 failed, 0 unavailable**. Groups: structure, contracts, format, lint, typecheck ×2, vitest, vite build, cargo fmt/clippy/test, aria2 fixtures, service process |
| GitHub CI run #1 (commit `203de61`, B0.1 state) | **success on windows-latest, macos-latest, ubuntu-22.04, ubuntu-24.04** (`pnpm verify:strict`, including aria2 fixtures; Ubuntu 22.04 uses aria2 from apt) |
| GitHub CI for the B0.2 changes | **Not run.** These changes are uncommitted |

## Honest limitations of B0.2
- **No real download through the service yet.** aria2 reports `Http`/`Https` as *Unverified*, so the gate refuses `add`. This is deliberate and not bypassed. A real download with SHA-256 verification becomes possible only after B0.4 and an ADR 0003 promotion. The full pause/resume/cancel/completion logic is covered by the fake-backend tests; engine-level control is covered by B0.1.
- Idempotency is **in-memory only**: retries work within one service lifetime, but not across restarts (B1).
- IPC security is OS-level only: owner-only pipe or socket plus the private discovery file. Denial of a different user was **not** tested. Running the service elevated on Windows is unsupported (the owner can become the Administrators group).
- The Unix socket path has only been compiled and type-checked locally. Runtime evidence comes when CI runs this commit.
- The service is not yet autostarted or tied to tray/UI lifecycle (B2). The UI still uses the foundation client and shows "not connected".

## Blockers and open decisions
- Bundled OpenSSL in the pinned aria2 Windows build: not audited (REL-05).
- Truncated-body classification `RANGE_UNSUPPORTED` is only partly accurate (COR-03/CORE-11).
- Open decisions: project license (REL-03), and confirming `downova.com` ownership if `com.downova` is needed for signing.
- pnpm is not installed globally. Use `npx pnpm@12.14.0`.

## Exact next task
**B0.3 — CLI harness (`crates/cli`, package `dm-cli`, executable `dm`; skill `dm-engine`).** Requirements: F01 (internal), CORE-01, CORE-03, COR-09.
1. Commands: `dm add <url> [--profile ID] [--idempotency-key KEY]`, `dm list`, `dm get <job-id>`, `dm pause|resume|cancel <job-id>`, plus `--json`, `--data-dir`, help and documented exit codes.
2. Use only `dm-platform::ipc::Client` and the `dm-domain::api` types. The CLI does not depend on `dm-service`, `dm-backend-*` or storage.
3. Handle service-unavailable, invalid argument, unknown job and capability-refusal errors with sanitized messages.
4. Tests:
   - argument parsing;
   - unavailable service;
   - JSON output shape;
   - a round trip against the real `dm-service` and a fixture.
   - The expected real-backend `add` outcome is the capability refusal. Record it as such, not as a download.
5. Before or alongside: commit and push so CI runs B0.2 on macOS and Linux.

**Then B0.4:** HTTP correctness fixtures (changed resource with the same length, ETag variants, 416, encoding, redirects, kill/crash), leading to **ADR 0003, the aria2 go/no-go**. This decides whether `Http`/`SegmentedHttp` can be promoted to `supported`.
