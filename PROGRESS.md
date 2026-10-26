# PROGRESS — handoff

_Last updated: 2026-10-26 (Windows 11 Pro 10.0.26300 x64, Node 26.7.0, pnpm 12.14.0 via npx, rustc 1.99.1, aria2 1.37.0 in tools/aria2)_

## Current milestone
**B1.1 (durable jobs and idempotency, ADR 0006) is committed in `90453b4`. CI #17 failed on all 4 runners, and this repair (uncommitted) fixes the cause.** Locally on Windows 11 the repaired tree passes `pnpm verify:strict`: **PASSED 19/19, 0 unavailable**. Remote CI for the repair has not run yet.

B0.6 (browser hand-off spike) is done in `564ad14`:
- Real Chrome passed locally on Windows 11.
- In CI #16 the real-Chrome step passed on windows-latest, macos-latest and ubuntu-24.04 (job conclusions; artifacts not read).
- ubuntu-22.04 has failed in `verify:strict` since #15. The cause is unknown (log needs sign-in) and is not the race that `564ad14` fixed, because #16 still failed there.

| Increment | Commit | GitHub CI (4 runners: windows-latest, macos-latest, ubuntu-22.04, ubuntu-24.04) |
|---|---|---|
| B0.1–B0.3 (aria2 adapter, service + IPC, CLI) | … `371c23a` | #2, #3 green |
| B0.4 aria2 HTTP correctness, ADR 0003 | `aee52e3` | #4 failed on macOS + both Ubuntu |
| B0.4.1 engine remediation, ADR 0004 | `c5dc042` | #5 failed on macOS + both Ubuntu |
| Cross-platform CI fixes | `3e37105` | #6 green |
| B0.4.2 `dm-backend-http` single connection | `d0d040b` | #7 green |
| B0.4.2 validated pause/resume | `4daa6ea` | #8 green |
| B0.4.3 validated segmentation | `9047a44` | #9 green |
| Default backend `http` (ADR 0005) | `d1f1b93` | #10 green |
| B0.5 packaging + smoke | `d241a18`, `8955629` | CI #12 green; package-smoke #2 green on all 3 |
| B0.6 host crate + contract | `56ee713`, `f5bc7b8` | CI #14 green on all 4; package-smoke #4 green |
| B0.6 extension, packaging, verify wiring | `330d306` | CI #15: windows, macOS, ubuntu-24.04 green; **ubuntu-22.04 failed**; package-smoke #5 green |
| B0.6 real-browser test, report | `564ad14` | [CI #16](https://github.com/DevSagar007/Downova/actions/runs/40178687044): windows, macOS, ubuntu-24.04 green, **including the real-Chrome step**; **ubuntu-22.04 failed** in `verify:strict` (real-Chrome step skipped); package-smoke #6 green |
| B1.1 SQLite jobs + idempotency (ADR 0006) | `90453b4` | [CI #17](https://github.com/DevSagar007/Downova/actions/runs/40180116969): **failed on all 4** in `verify:strict`; the real-Chrome step was skipped on all 4. [Package-smoke #7](https://github.com/DevSagar007/Downova/actions/runs/40180116963) green on all 3 |
| B1.1 repair (this session) | uncommitted | not run |

CI evidence is the public job conclusion of each run; job logs need GitHub authentication and were not read here.

## Backends (ADR 0005)
`dm-service` defaults to the built-in `dm-backend-http` (Http, Https, ResumeStrongValidator, SegmentedHttp supported); aria2 only with `--backend aria2` (Http only) and never bundled while REL-05 is open. No fallback between backends.

## Completed (summary)
- Foundation; ADRs 0001–0005; CI on 4 OSes; aria2 adapter (hardened); service (user-only IPC, capability gates, service-owned retry); `dm` CLI.
- `dm-backend-http`: HTTP/HTTPS with platform trust, manual redirects, origin-scoped credentials, identity encoding, validated pause/resume, validated segmentation.
- Reports: [aria2 B0.4](docs/reports/b0.4-http-correctness.md), [dm-backend-http](docs/reports/b0.4.2-http-backend.md), [B0.5 packaging](docs/reports/b0.5-packaging-smoke.md).

### This session — B1.1 repair (CI #17)
- **Confirmed cause 1:** a test expectation was obsolete.
  - `crates/service/tests/process.rs` `service_killed_during_a_transfer_leaves_no_orphan_and_no_false_state` still asserted `list.items.is_empty()` after the restart (B0 in-memory semantics).
  - With B1.1 the job survives, so the assertion fails. This was reproduced locally (`verify:strict` run before the fix: `process.rs:527 assertion failed: list.items.is_empty()`).
  - The aria2 group runs on all 4 runners, which matches CI #17 failing everywhere while package-smoke #7 passed.
- **Confirmed cause 2:** a leaked service locked the executable.
  - The assertion fired before the restarted service was shut down, and nothing reaped it.
  - `cargo test -p dm-service` and `cargo build -p dm-service` produce different `target/debug/dm-service.exe` binaries (dev-dependency feature unification; measured 11 455 488 vs 11 340 288 bytes), so every switch between them replaces the file.
  - With the leaked service still running it, the next step's `cargo build` (the CLI real-service tests) failed with "failed to remove file … dm-service.exe: Access is denied (os error 5)". All three CLI tests then failed.
- **Fixes:**
  - The crash test now checks the B1.1 contract:
    - exactly the original job survives, with the same ID;
    - phase `failed` with the `INTERRUPTED` reason, never `completed`;
    - the same add (same key and payload) after the restart returns that job;
    - a changed payload under that key → `IDEMPOTENCY_CONFLICT`;
    - still one job;
    - graceful shutdown → exit 0.
    The orphan-engine and partial-file assertions are unchanged.
  - A `Service` guard (the pattern from `http_backend_process.rs`) kills and reaps a still-running test-owned service on drop (success, panic or timeout). It is used for every service spawned by `crates/service/tests/process.rs`, `crates/cli/tests/real_service.rs` and the browser-host end-to-end test. Only those children are touched.
  - `crates/cli/tests/real_service.rs` builds `dm-service` once per test binary (`std::sync::Once`) before any test spawns it, so no build can try to replace the executable while another test's service runs from it.
- **Docs:** this file, testing §6 note, traceability (COR-09, CORE-04, BRW-01), the B0.6 report (CI #16 real-browser results).

### Earlier — B1.1 (ADR 0006, `90453b4`)
- `dm-storage` (rusqlite 0.41 bundled; exclusive locking, WAL + `synchronous=FULL`; forward-only migrations; `jobs`, `idempotency_records` with the fingerprint as SHA-256).
- `dm-service`:
  - a job and its key are written before the acknowledgement;
  - state changes are durable;
  - on restart, jobs and keys are restored; an unfinished job → `failed` with `INTERRUPTED`;
  - idempotency records are kept for 30 days.

### Earlier — B0.6 ([report](docs/reports/b0.6-browser-handoff.md))
- **Contract:** `NativeHostRequest` (`hello`, `add`), `NativeHostOutcome` and `NativeHostResponse`. TypeScript is generated and the Rust mirror is in `dm-domain::api`.
- **`dm-browser-host`:**
  - framing caps;
  - an exact origin allowlist;
  - only `hello` and `add` → `downloads.v1.add`;
  - it **never starts the service** ("open the Downova app, then try again");
  - per-user `register` / `repair` / `unregister` / `status`.
- **`apps/extension` (MV3):**
  - context menu "Download with Downova" (http/https links only);
  - permissions `contextMenus`, `nativeMessaging`, `storage`; incognito `not_allowed`;
  - one key per action, persisted before sending; same key and payload on retry, including after a worker or browser restart;
  - response checks (version, `request_id`, outcome);
  - popup: a service readiness line ("Service ready / unavailable / status unknown"), the hand-off list with Retry, Dismiss and Clear list, links shown without the query string, at most 20 records, 7-day retention.
- **Identity:** the manifest public key hashes to `bkmpnfclpdmcbdgmeakmnoilmkdmdppf`. This is checked by vitest and by the ID real Chrome reports; it is the host's only allowed origin.
- **Packaging:** `dm-browser-host` is built and staged as a Tauri `externalBin`. Smoke steps: A4 (packaged host → service, same key → same job) and A5 (foreign origin refused). Installer registration is deferred (REL-01).
- **Real browser:** `scripts/e2e/chrome-handoff.mjs`:
  - isolated profile; DevTools pipe with `Extensions.loadUnpacked`;
  - reversible registration (Windows HKCU key, restoring any previous value; macOS/Linux use the profile's `NativeMessagingHosts`);
  - Chrome or Edge;
  - added to CI on windows-latest, macos-latest and ubuntu-24.04, with results uploaded as an artifact.
- **Verify:** the native-host end-to-end step is bounded at 15 min (`timeoutMs`). Its test now waits (≤ 30 s) for the service before running the host, the probable cause of the ubuntu-22.04 failure.
- **Docs:**
  - report `docs/reports/b0.6-browser-handoff.md` (renamed from `b0.6-chrome-handoff.md`);
  - setup (per-OS development steps);
  - architecture §8, testing §4, roadmap;
  - requirements: BRW-01 `in-progress`, BRW-02 `implemented`, BRW-03 `in-progress`;
  - traceability.

### Earlier — B0.5
- Native packages, packaged service discovery, packaged-app smoke and workflow ([report](docs/reports/b0.5-packaging-smoke.md)).

## Checks actually run (local, Windows 11 Pro 10.0.26300 x64, 2026-10-26)
| Command | Result |
|---|---|
| `cargo test -p dm-service --test process service_killed_during_a_transfer_leaves_no_orphan_and_no_false_state -- --ignored --nocapture` (DM_ARIA2_PATH = tools/aria2) | **PASSED**: engine exited with the service; partial 1.5 MiB with control file; after restart 1 job [Failed/INTERRUPTED], same job for the same key, conflict for a changed payload |
| `cargo test -p dm-service --test http_backend_process jobs_and_idempotency_survive_a_service_kill -- --nocapture` | **PASSED** (same job after a hard kill and restart; interrupted reason; completed file SHA-256 kept) |
| `cargo test -p dm-cli --test real_service -- --ignored --nocapture` | **PASSED 3/3** (aria2, http, pause/resume with SHA-256) |
| `tasklist` after each of the above | no `dm-service.exe` left running |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | PASSED |
| `pnpm verify:strict` (repaired tree) | **PASSED 19/19, 0 unavailable** |
| `pnpm verify:strict` (`90453b4` tree, before the repair) | FAILED 17/19: `[aria2] service process tests` (process.rs:527 assertion) and `[aria2] CLI against real service` ("Access is denied" replacing dm-service.exe) |

Remote evidence is listed in the table above. No remote run exists for the repair yet. The real-Chrome CI step (configured for windows-latest, macos-latest and ubuntu-24.04) was **skipped** on all of them in #17 because `verify:strict` failed first, so it is unverified for that commit.
## Honest limitations
- **ubuntu-22.04 fails `verify:strict` in CI #15, #16 and #17.** The cause is unknown (log not readable). It is not the native-host readiness race (fixed in `564ad14`, still failing in #16). It needs the job log.
- B1.1:
  - an unfinished job cannot continue after a restart (B1.2);
  - there are no `attempts`, `resume_checkpoints` or `finalization_journal` tables yet;
  - SQLite runs on macOS/Linux have only CI #17, which failed for the reason fixed here.
- B0.6:
  - real Chrome passed locally on Windows and in CI #16 on windows-latest, macos-latest and ubuntu-24.04 (job conclusions only);
  - Edge was not run;
  - the menu click is simulated;
  - the installer does not register the host (REL-01).
- Packages are unsigned (macOS ad-hoc) and not notarised. There is no connection budget across jobs (CORE-02).
## Blockers and open decisions
- Code signing and notarisation identities; project licence (REL-03); dependency licence/advisory review (REL-03); aria2 not releasable (REL-05).
- `gh` not installed: CI conclusions via the public API only.

## Exact next task
1. The user decides on committing this repair. After a push, check CI on all 4 runners. If ubuntu-22.04 still fails, read its job log (GitHub sign-in) to find the remaining cause.
2. **B1.2: engine-owned durable checkpoints and validated recovery after a service restart (CORE-04, ARCH-05, COR-08):**
   - `dm-backend-api`: an opaque, versioned checkpoint the backend can export and re-import: `dm-backend-http`'s strong ETag + length, confirmed byte ranges, partial path. The service never interprets segment maps.
   - `dm-storage` migration 2: a `resume_checkpoints` table (job, attempt, backend id + version, checkpoint blob, updated time). It is written at pause, at clean shutdown and on a bounded interval, not per packet.
   - On restart, reconcile job ↔ checkpoint ↔ partial file (length, existence):
     - if they match → a new attempt that resumes with `If-Range` and validates the 206;
     - a changed or missing validator, or a missing or shorter file → an explicit restart-from-zero or failure, never a splice;
     - jobs without a checkpoint keep the `INTERRUPTED` failure.
   - Tests: hard-kill `dm-service` mid-transfer N times (fixtures: unchanged, changed ETag, truncated partial) → the exact SHA-256 or an explicit failure, never a corrupt "completed" file; aria2 stays unsupported for resume.
