# Testing and Verification Strategy

Principle: a check counts only if it can fail for the behaviour it claims to cover. Report each check as
run or not run, with the OS it ran on. Never claim support from a static screenshot, a framework choice or
upstream documentation (research §18.5).

## 0. Entry points

| Command | What it runs |
|---|---|
| `pnpm verify` | Structural checks, contract check, Prettier, ESLint, tsc, vitest, Vite build. Also Rust fmt, clippy and tests **if** cargo exists; otherwise they are marked UNAVAILABLE |
| `pnpm verify:strict` | Same, but UNAVAILABLE counts as failure (for CI) |
| `pnpm verify -- --only=<groups>` | Subset: `structure,contracts,format,lint,typecheck,test,build,rust,aria2` |
| `aria2` group | (1) `cargo test -p dm-backend-aria2 --test fixtures -- --ignored`: real aria2c against the controlled fixture server; (2) `cargo test -p dm-service --test process -- --ignored`: the real `dm-service` process (single instance, hello, capability gate, shutdown); (2b) `cargo test -p dm-backend-aria2 --test http_correctness -- --ignored`: the B0.4 matrix (27 tests); (3) `cargo test -p dm-cli --test real_service -- --ignored`: the `dm` binary against the real service (unknown job, https refused with no job or file, **http add completed with a SHA-256 match**, service outlives the CLI). The service process tests also cover a real download and kills of aria2 and the service mid-transfer. UNAVAILABLE without cargo or an aria2c binary |
| `cargo test --workspace` | Rust unit tests and contract round-trip |

### Structural checks
`scripts/check-structure.mjs` checks the following:
- required files exist;
- JSON, YAML and TOML parse;
- local Markdown links resolve;
- `.claude/rules` frontmatter (`paths` list) and `.claude/skills` frontmatter (`name`, `description`) are valid;
- Rust crate dependency directions are respected;
- no cross-package relative TypeScript imports, and no Tauri imports outside `services/`;
- every requirement has a traceability row, and every `verified` requirement has evidence.

### CLI (B0.3)
`cargo test -p dm-cli` covers argument parsing, help and version, default and custom data directories, missing and stale endpoints, timeouts, the API-version handshake, command→method mapping, human and JSON output, exit codes, retries with the same idempotency key, and URL redaction. It runs against an **isolated IPC test peer** (`crates/cli/tests/support`): a scripted endpoint on the real transport. That gives controlled success responses without any production capability bypass.

## 1. HTTP correctness (COR-01..05, CORE-02, E03, E04)
- **Fixtures:** a local, deterministic HTTP/HTTPS server (B0.4) in `crates/backend-aria2/tests/support/` (scriptable routes plus a request log; TLS via a test CA). It covers:
  - range honoured (206), range ignored (200), wrong Content-Range, short or truncated body and 416;
  - unknown length, zero-byte files and Content-Encoding changes;
  - strong, weak and missing ETag;
  - same-length changed content, 429/503 with Retry-After, and 401/403/410;
  - redirects with and without cross-origin hops, and Content-Disposition `filename*` with Bangla names and reserved names.
- **Oracle:** the SHA-256 of the completed file equals the expected value. Any mismatch in a file reported `completed` blocks release.
- **Report:** a per-backend capability and correctness matrix, which feeds the ADR 0003 go/no-go decision. aria2 1.37.0: [B0.4 report](reports/b0.4-http-correctness.md). Each case is classified safe success, safe failure, unsafe or not tested; engine-level unsafe outcomes are asserted as unsafe so a behaviour change fails loudly.

## 2. Resume and changed resources (COR-02, COR-06, ADV06)
- Pause and resume with a stable resource gives the correct hash.
- The resource changes while paused: the result is a safe restart or `REMOTE_CHANGED`, never mixed bytes.
- Link replacement with the same resource keeps progress. With a different resource of the same size, the user gets the restart-or-keep decision.
- A late event from a cancelled attempt does not change the job.

## 3. Crash recovery and disk failures (CORE-04, COR-08, ADV05)
- Kill the service, the engine, or both at random points (including during checkpoint and finalization), N ≥ 20 iterations. Each must end with the correct hash or an explicit failure.
- Disk full (a small loopback or tmpfs volume): the job stops with `DISK_FULL`, the partial file is kept, and no other file changes.
- Permission denied, removed volume and returning volume, a name collision, and a partial file deleted externally.
- Power-loss ordering is reasoned about and documented. Durability is never claimed beyond what the fault tests show.

## 4. Browser handoff and fallback (BRW-*)
- Matrix: browser (Chrome, Edge, then Firefox) × OS × install state (installed, not installed, host missing, service down, timeout).
- Expected results:
  - public GET is taken over only after acceptance;
  - when the host is missing or times out, the browser download continues;
  - POST, blob and data downloads stay in the browser;
  - a lost acknowledgement plus a retry gives one job (COR-09);
  - closing the browser does not stop the native job.
- Authenticated cases check that credentials do not leak across origins.

## 5. Service lifecycle and cancellation (ARCH-01, ARCH-09)
| Case | Expected |
|---|---|
| UI window closed (tray setting on) | Transfers continue; the UI reconnects to the same instance |
| UI crash | Transfers continue |
| Browser closed / host disconnect | Transfers continue |
| Service restart | Reconciliation; supported jobs resume; event clients resnapshot |
| OS logout | Graceful checkpoint; resume on next login if autostart is enabled |
| Sleep / resume | No transfers while asleep; network blockers re-evaluated on wake |
| Reboot | Same as logout, plus the missed-schedule policy |
| Cancel during transfer | Ack only after the backend `remove` returns; no later `completed` |

## 6. UI accessibility (UI-01..03, ADV12)
- **Automated, now:** vitest and Testing Library check roles, names, `aria-current`, `aria-disabled` with a description, keyboard tab order, the language switch and the theme switch (`apps/desktop/src/app/App.test.tsx`).
- **Automated, later:** an axe-core pass per view, and a 10k-row virtualized list performance test.
- **Manual, per beta:** keyboard-only essential workflow; NVDA (Windows), VoiceOver (macOS) and Orca (Linux) smoke tests; 200% zoom; forced colours; Bangla rendering; long and RTL filenames.

## 7. Packaging, updates and platform compatibility (REL-*, ADV09, BRW-07)
- CI matrix: windows-latest, macos-latest (arm64 and x64) and ubuntu-22.04/24.04. A combination counts as supported only with recorded passes (REL-02).
- For every OS: install, launch, uninstall and native-host registration plus repair.
- Updates: a tampered artifact is rejected, a failed migration still gives a recoverable startup, and downgrades are blocked.
- Licensing: a notices bundle exists, aria2 source or an offer for it is included, and the SBOM is generated (B3/B4).

## 8. Privacy (SEC-01, E05)
A synthetic secret (`SYNTHETIC-SECRET-…`) is placed in a URL query, a cookie and an Authorization header. Then grep logs, events, the diagnostics export, the SQLite file and the aria2 session file. Any hit fails the test.

## 9. Benchmarks (REL-04)
Follow research §11: the same machine and network profile, no concurrent apps, at least 5 rotated runs with medians (at least 20 runs for p95), and full process-tree memory and CPU. Publish the scripts and raw results.

## Reporting
- For every completed increment, PROGRESS.md lists each command that was run, its result and the OS.
- Traceability evidence cells cite the test name and the run.
- If something could not run, say why and which OS or tool is needed.
