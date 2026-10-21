# PROGRESS — handoff

_Last updated: 2026-10-21 (Windows 11 Pro 10.0.26300 x64, Node 26.7.0, pnpm 12.14.0 via npx, rustc 1.99.1, MSVC Build Tools 17.14)_

## Current milestone
**Foundation: verified on Windows.** **B0.1 (aria2 adapter + controlled fixtures): done on Windows.**
Next: **B0.2 — service process and IPC** ([docs/roadmap.md](docs/roadmap.md)).

## Completed
### Foundation (earlier sessions)
- Research preserved verbatim at `docs/research/` (SHA-256 `ae56b81e…da641d`).
- Documentation:
  - `requirements.md` (91 requirements) and `design.md`;
  - architecture, structure, roadmap, testing and traceability docs, and ADR 0001;
  - Claude rules (6) and skills (4).
- JS tooling, contracts (canonical schema + generated TS), and the IDM-style desktop UI with 16 vitest tests (design.md §13 parity list).

### This session
- **Prerequisites installed with the user's approval** via winget: rustup 1.29.1 (toolchain 1.99.1 picked up from `rust-toolchain.toml`) and VS 2022 Build Tools 17.14.37806.7 (VC tools workload). WebView2 155.0.4283.62 was already present.
- **Architecture fix (ADR 0002).**
  - `DownloadBackend` moved from `dm-service` to the new crate `dm-backend-api` (`crates/backend-api`).
  - The graph is acyclic: `dm-service → dm-backend-api ← dm-backend-aria2`.
  - `check-structure.mjs` allowlist, `docs/project-structure.md`, `docs/architecture.md` §4, `.claude/rules/architecture.md` and the dm-engine skill were updated.
  - The aria2 go/no-go ADR is now numbered **0003**.
- **Cargo.lock generated.** MSRV raised 1.85 → **1.99**: the Tauri dependency `tray-icon` needs ≥1.90, so 1.85 was never true. ADR 0001 notes the change.
- Fixes needed to make the Rust code pass:
  - rustfmt applied to all crates;
  - `#![allow(clippy::unwrap_used)]` added in the contract test file (its helper functions are not `#[test]`s).
  - No logic changes were needed.
- **Downova naming (ADR 0002):**
  - `productName`, window title, `<title>`, UI copy (en/bn), About dialog, README and the design.md extension label all say Downova.
  - **Stable bundle identifier `com.downova.desktop`.**
  - Internal `@dm/*` / `dm-*` names are unchanged.
- Tauri `beforeDevCommand`/`beforeBuildCommand` now use `npm run …`, so they work without a global pnpm.
- **CI** (`.github/workflows/ci.yml`):
  - runs on windows-latest, macos-latest, ubuntu-22.04 and ubuntu-24.04;
  - uses the pinned Rust (`rustup toolchain install` reads `rust-toolchain.toml`), Node 24 and pnpm from `packageManager`;
  - installs the Tauri Linux dependencies and aria2 (apt, brew, or the pinned fetch on Windows);
  - runs `pnpm verify:strict`.
- **B0.1:**
  - `scripts/fetch-aria2.mjs` (`pnpm fetch:aria2`): pinned aria2 1.37.0 Windows build, size + SHA-256 checked, installed into `tools/aria2/` (gitignored).
  - `crates/backend-aria2`:
    - binary lookup and version check;
    - loopback RPC with a random 256-bit secret passed by a private conf file that is deleted after startup;
    - no `--save-session`;
    - `--no-netrc`, `--allow-overwrite=false`, `--stop-with-process`;
    - attempt-ID handles, so GIDs stay private;
    - error-code classification without URLs;
    - a complete capability report in which nothing is `Supported`.
  - A controlled HTTP fixture server and 8 fixture tests. See [docs/reports/b0.1-aria2-fixtures.md](docs/reports/b0.1-aria2-fixtures.md).
  - `verify.mjs` has a new `aria2` group, which reports UNAVAILABLE when cargo or aria2c is missing.
- Requirement statuses updated with evidence:
  - **verified:** ARCH-07 (contracts) and COR-07 (pause vs policy);
  - **in-progress, with new evidence:** ARCH-03/04, CORE-02/03/11, COR-01, SEC-01/05, REL-01/05.

## Checks actually run (Windows 11, 2026-10-21)
| Command | Result |
|---|---|
| `cargo fmt --all --check` | PASSED (after `cargo fmt --all`) |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | PASSED (domain, backend-api, service, backend-aria2, desktop shell) |
| `cargo test --workspace --locked` | PASSED: 28 tests (domain 17 + contract 2, backend-api 1, backend-aria2 8 unit). The 8 fixture tests are ignored here by design |
| `cargo test -p dm-backend-aria2 --test fixtures -- --ignored` | PASSED 8/8, three consecutive runs |
| `pnpm verify:strict` (all groups: structure, contracts, format, lint, typecheck ×2, test, build, rust ×3, aria2) | **12 passed, 0 failed, 0 unavailable** |
| **Tauri desktop smoke** (separate from the browser preview): `tauri build --debug --no-bundle`, then launch `target/debug/dm-desktop.exe` | Built in 1m24s. The process stayed up and the window titled "Downova" (1116×759) rendered the full UI in WebView2 (screenshot checked). `CloseMainWindow` → process exited, none left running. The exit code was not captured |
| Browser preview (`pnpm dev`) | Earlier session; unchanged |
| CI workflow | **Not run.** Changes are uncommitted; nothing pushed to `origin` (github.com/DevSagar007/Downova) |

## Blockers and open decisions
- **CI has never executed.** macOS and Linux results (including Tauri deps and aria2 1.36.0 on Ubuntu 22.04) are unknown until the workflow runs on GitHub. Running it requires commit + push, which needs the user's go-ahead.
- **Bundled OpenSSL in the pinned aria2 Windows build** (a 2023 release) is not yet audited. It must be resolved before shipping (REL-05; see the fixture report).
- Truncated-body classification (`RANGE_UNSUPPORTED`) is only partly accurate (COR-03/CORE-11).
- Open decisions: the project license (REL-03), and confirming ownership of the `downova.com` domain if `com.downova` is needed for signing or store listings.
- pnpm is still not installed globally. Use `npx pnpm@12.14.0` or `npm i -g pnpm@12.14.0`. New terminals get `cargo` on PATH from rustup.

## Platform limitations
- Every result above is **Windows 11 x64 only**. macOS and Linux are unverified.
- The Tauri smoke test used a debug binary. No installer (MSI/NSIS/DMG/DEB) has been built.

## Exact next task
**B0.2 — `dm-service` binary with IPC (skill `dm-engine`).** Requirements: ARCH-01, ARCH-06, COR-09, CORE-01, CORE-03.
1. Single-instance per-user service process.
2. IPC endpoint: a Windows named pipe with a user-only ACL (Unix socket with mode 0600 on Unix).
3. JSON-RPC 2.0 framing with `service.v1.hello`, using `dm_service::hello_result` with the aria2 backend.
4. `downloads.v1.add/list/get/pause/resume/cancel`, kept in memory for now, driving `Aria2Backend` through `DownloadBackend`.
5. Idempotent add (same key + same payload → same job; different payload → `IDEMPOTENCY_CONFLICT`).
6. Tests: an IPC round-trip, an idempotency test, and a lost-acknowledgement retry.

Before or in parallel: push the branch so the CI matrix runs, and record the macOS/Linux results.
