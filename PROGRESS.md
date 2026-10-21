# PROGRESS — handoff

_Last updated: 2026-10-21 (Windows 11 Pro 10.0.26300 x64, Node 26.7.0, pnpm 12.14.0 via npx, rustc 1.99.1, MSVC Build Tools 17.14)_

## Current milestone
- **B0.1** (aria2 adapter): done.
- **B0.2** (service + IPC): done, with CI on all 4 runners.
- **B0.3** (`dm` CLI): done on Windows.

Next: **B0.4 — HTTP correctness fixtures and ADR 0003 (aria2 go/no-go)** ([docs/roadmap.md](docs/roadmap.md)).

## Completed
### Earlier sessions
- Foundation:
  - documentation (91 requirements);
  - the IDM-style desktop UI (16 vitest tests);
  - contracts, rules and skills.
- Rust 1.99.1 and VS Build Tools 17.14 installed (user-approved).
- ADR 0002: `dm-backend-api`, Downova naming, `com.downova.desktop`.
- CI on 4 OSes.
- **B0.1:** the aria2 1.37.0 adapter and 8 fixtures ([report](docs/reports/b0.1-aria2-fixtures.md)).
- **B0.2 (commit `73fe021`):**
  - `dm-platform` with user-only IPC;
  - the `dm-service` process: single instance, JSON-RPC, in-memory jobs and idempotency, capability gates, attempt guards;
  - contract RPC types.

### This session — B0.3 (`crates/cli`, package `dm-cli`, executable `dm`)
- **`args.rs`** parses the command line:
  - `add <URL> [--profile ID] [--idempotency-key KEY]` (profile defaults to `downloads`), `list`, and `get|pause|resume|cancel <JOB-ID>`;
  - global `--json` and `--data-dir DIR` are accepted anywhere, plus `-h/--help`, `-V/--version`, `--opt=value`, and `--` to end options;
  - job IDs are validated locally;
  - errors never echo URL values, and `Debug` for `add` redacts the URL.
- **`run.rs`** talks to the service:
  - resolves the data directory with `--data-dir` or `dm_platform::paths::data_dir()` (same discovery as other clients, `DOWNOVA_HOME` override);
  - uses `dm_platform::ipc::Client`;
  - **hello handshake on every connection**, and an API-version mismatch stops before any application call;
  - connect is bounded at 5 s, each response at 30 s (configurable in `RunOptions`);
  - one idempotency key per `add` (`cli-<128-bit hex>` unless `--idempotency-key`), **re-sent once on a new connection after a lost acknowledgement**;
  - malformed replies are detected.
- **`output.rs`** formats results:
  - human output: job, name, redacted source, state with phase, progress (binary units, % only when the total is known), blockers, attempt, error and next-step hint;
  - a list table;
  - a stable JSON shape, `{ok, command, data, idempotency_key?}` or `{ok:false, command, exit_code, error:{kind, rpc_code, retryable, next_action, detail}}`;
  - `sanitize` redacts any `http(s)://` text before printing (defence in depth);
  - in human mode stdout carries results only and errors go to stderr; in JSON mode both go to stdout as JSON.
- **Exit codes:** 0 ok, 1 unexpected, 2 invalid arguments, 3 service unavailable / timeout / incompatible, 4 rejected by the service.
- **Dependencies** are `dm-domain` and `dm-platform` only, as `check-structure` enforces. The CLI never downloads and leaves the service running.
- **Tests:**
  - 7 unit tests;
  - 12 tests (`tests/cli.rs`) against an **isolated IPC test peer** (`tests/support`, a scripted endpoint on the real transport) and the binary;
  - 1 real-process test (`tests/real_service.rs`, `#[ignore]`, `aria2` verify group).
- Docs: README, setup ("Using the CLI"), testing, architecture, project-structure, roadmap and CLAUDE.md commands. Stale "CI pending / uncommitted" statements were corrected (ARCH-06 row, PROGRESS).
- Requirements:
  - **F01 → in-progress** (internal harness only; public docs and headless packaging are B4);
  - new evidence for CORE-01, CORE-03, COR-09 and SEC-01 (all still in-progress);
  - ARCH-06 evidence updated with CI #2.

## Checks actually run
| Command | Result |
|---|---|
| `cargo fmt --all --check` | PASSED |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | PASSED |
| `cargo test --workspace --locked` | PASSED: 65 passed, 0 failed, 10 ignored (aria2-dependent; run by the `aria2` group) |
| `cargo test -p dm-cli` | PASSED 19. Covers usage errors (human and JSON), help and version, the default data dir via `DOWNOVA_HOME`, missing and stale endpoints, the incompatible API, all 6 command→method mappings, human and JSON success, **redaction with a hostile peer echoing secret URLs**, lost acknowledgement → one retry with the same key, an explicit key reused across runs, a bounded timeout, and exit codes for capability refusal, conflict, unknown job, malformed and internal errors |
| `cargo test -p dm-cli --test real_service -- --ignored` | PASSED. `dm` against the real `dm-service` + aria2 1.37.0: empty list; `get job_unknown01` → exit 4 INVALID_REQUEST; `add http://…?token=SECRET` → **exit 4 BACKEND_CAPABILITY_UNAVAILABLE, no job, no file, no secret in output**; the service outlives the CLI; after shutdown `dm list` → exit 3 |
| `pnpm verify:strict` | **14 passed, 0 failed, 0 unavailable**, including the aria2 fixtures, service process and CLI real-service tests |
| Manual demo (Windows) | `dm list` → "No downloads." (0); `dm add "https://…?token=…"` → `BACKEND_CAPABILITY_UNAVAILABLE: Https is Unverified` (4); `dm --json get job_missing01` → JSON error (4); service force-killed → `dm list` reports a stale endpoint (3); **no orphan aria2c** (`--stop-with-process`) |
| Exploratory battery vs real service (2026-10-21, Windows) | **26/26 PASS**. Covers usage/version, pure-JSON stdout, https/userinfo/query/fragment secrets refused with no leak (human and JSON), ftp → INVALID_REQUEST, unknown profile, garbage URL, short or control-char key, a 20 KB URL, unknown and malformed job IDs, no job or file after refusals, 20 concurrent clients, second instance → 3, service outliving the CLI, no secret in the service log, force-kill → stale endpoint (3), no orphan aria2c |
| **Bug found and fixed while testing** | `add` ran the capability gate *before* input validation, so a wrong `--profile` was reported as `BACKEND_CAPABILITY_UNAVAILABLE`. Now: scheme → profile → SHA format → capability gate. Regression test `input_errors_are_reported_before_the_capability_gate`; `pnpm verify:strict` re-run, 14/14 PASSED |
| GitHub CI run #2 (commit `73fe021`, B0.2) | success on windows-latest, macos-latest, ubuntu-22.04 and ubuntu-24.04 |
| GitHub CI for B0.3 | **Not run yet.** The B0.3 changes are local and uncommitted |

## Honest limitations
- **No real download yet.** aria2 `Http`/`Https` remain *Unverified*, so `dm add` against the real service is refused (exit 4). The CLI's success path is proven with controlled peer responses only, not with a real transfer or a SHA-256-verified file.
- Idempotency is **in-memory** (the service lifetime). The CLI's retry reuses the key correctly, but durable idempotency is B1.
- Strong-validator resume stays **unsupported**: `dm resume` on a job with partial bytes is refused by the service.
- The CLI has no `shutdown` command on purpose. The service is stopped with Ctrl+C or `service.v1.shutdown`.
- Cross-user IPC denial is untested. Running the service elevated on Windows is unsupported.
- The desktop UI is not yet connected to the service (B2).

## Blockers and open decisions
- Bundled OpenSSL in the pinned aria2 Windows build: not audited (REL-05).
- Truncated-body classification `RANGE_UNSUPPORTED` is only partly accurate (COR-03/CORE-11).
- Open decisions: project license (REL-03) and `downova.com` ownership if needed for signing.
- pnpm is not installed globally. Use `npx pnpm@12.14.0`.

## Exact next task
**B0.4 — HTTP correctness fixtures (skill `dm-engine`).** Requirements: COR-01, COR-02, COR-03, COR-04, CORE-02, E04, REL-05.
1. Extend the controlled fixture server (`crates/backend-aria2/tests/support`) with:
   - same-length changed content between pause and resume;
   - strong, weak and missing ETag;
   - 416;
   - wrong Content-Range;
   - Content-Encoding changes;
   - unknown length and zero-byte files;
   - 429/503 with Retry-After;
   - 401/403/410;
   - same-origin and cross-origin redirects;
   - Content-Disposition `filename*`;
   - kill or crash of aria2 and the service mid-transfer.
2. Record the outcome per case (safe failure, safe success with a SHA-256 match, or **unsafe**) in `docs/reports/b0.4-http-correctness.md`.
3. Write **ADR 0003, the aria2 go/no-go**: which capabilities to promote to `supported` (Http/Https/SegmentedHttp), which wrapper validation is needed, and whether a patch or replacement is required. Also cover the bundled OpenSSL audit.
4. Only after ADR 0003 promotes `Http`: the first real `dm add` with a SHA-256-verified file.

Commit and push B0.3 so CI runs the CLI tests on all four runners.
