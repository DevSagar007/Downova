# PROGRESS — handoff

_Last updated: 2026-10-26 (Windows 11 Pro 10.0.26300 x64, Node 26.7.0, pnpm 12.14.0 via npx, rustc 1.99.1, aria2 1.37.0 in tools/aria2)_

## Current milestone
**B1.3 (staged output and durable completion journal, ADR 0008): implemented; everything passes locally on Windows 11 (see the checks below); uncommitted, so there is no remote CI yet.**

Committed so far:
- B1.2 (durable checkpoints, `f73a41a`) and the native-host BrokenPipe test fix (`95322e3`).
- CI #19's Ubuntu failures were `BrokenPipe` in `a_caller_not_on_the_allowlist_is_refused`, confirmed from the job logs provided by the maintainer.
- CI #21: windows, macOS and ubuntu-22.04 green. ubuntu-24.04 failed on the new deterministic test, confirmed via the public annotation `scripts/verify.mjs` now emits; the test is fixed in this session.
- Earlier ubuntu-22.04 failures (#15/#16) remain unattributed (logs not read).

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
| B1.1 repair (Service guards, crash-test contract) | `da2f4d2`, `8eefee6` | #18 cancelled; [CI #19](https://github.com/DevSagar007/Downova/actions/runs/40276832394): **windows-latest and macos-latest green (including the real-Chrome step); ubuntu-22.04 and ubuntu-24.04 failed** in `verify:strict`; [package-smoke #9](https://github.com/DevSagar007/Downova/actions/runs/40276832372) green |
| B1.2 durable checkpoints (ADR 0007) | `f73a41a` | #20 / package-smoke #10 cancelled by the next push |
| Native-host BrokenPipe test fix | `95322e3` | [CI #21](https://github.com/DevSagar007/Downova/actions/runs/40281797983): windows-latest, macos-latest (including real Chrome) and **ubuntu-22.04 green**; **ubuntu-24.04 failed**: the new deterministic test expected a late write to fail (annotation; fixed in this session); [package-smoke #11](https://github.com/DevSagar007/Downova/actions/runs/40281797992) green on all 3 |
| B1.3 completion journal + host-test fix (this session) | uncommitted | not run |

CI evidence is the public job conclusion (and, since `95322e3`, the failure annotation) of each run; full job logs need GitHub authentication.

## Backends (ADR 0005)
`dm-service` defaults to the built-in `dm-backend-http` (Http, Https, ResumeStrongValidator, SegmentedHttp supported); aria2 only with `--backend aria2` (Http only) and never bundled while REL-05 is open. No fallback between backends.

## Completed (summary)
- Foundation; ADRs 0001–0005; CI on 4 OSes; aria2 adapter (hardened); service (user-only IPC, capability gates, service-owned retry); `dm` CLI.
- `dm-backend-http`: HTTP/HTTPS with platform trust, manual redirects, origin-scoped credentials, identity encoding, validated pause/resume, validated segmentation.
- Reports: [aria2 B0.4](docs/reports/b0.4-http-correctness.md), [dm-backend-http](docs/reports/b0.4.2-http-backend.md), [B0.5 packaging](docs/reports/b0.5-packaging-smoke.md).

### This session — B1.3 ([report](docs/reports/b1.3-completion-journal.md), ADR 0008)
- **API:**
  - `dm-backend-api`: `StagedOutput` (`Staged(path)` | `PlacedByEngine(path)`) and `staged_output`. `Complete` now means staged and synced.
  - `dm-backend-http` no longer publishes (the unsafe rename fallback is removed). A fully confirmed checkpoint finishes without a range request.
  - aria2 reports `PlacedByEngine`.
- **Contract:**
  - `DownloadSnapshot.verification` = `{ sha256, source: local_hash | publisher_checksum }`;
  - schema, TypeScript, Rust mirror, `download-snapshot.completed` example.
- **`dm-storage` migration 3:**
  - `finalization_journal`, plus `jobs.verified_sha256` and `jobs.verification_source`;
  - `begin_finalization`, `advance_finalization`, `abandon_finalization`, `finish_finalization`, `load_journal` (attempt-checked; `completed` with evidence and checkpoint removal in one transaction);
  - cancelled or failed jobs drop their entry.
- **`dm-service` `core/finalize.rs`:**
  - stages: intent → verified (length, SHA-256, trusted checksum; a mismatch deletes the bad staging file) → placed (`place_no_overwrite`: hard link, else `create_new` + synced copy re-verified) → complete → cleanup;
  - reconciliation at startup, before checkpoint recovery, re-checking files by SHA-256;
  - cancel refused once placed;
  - debug-only fault hooks `DOWNOVA_FAULT`.
- **Backend tests:** adapted to staged output (the destination is never written by the backend).
- **Host test:** `a_refused_caller_is_answered_before_its_input_is_read` accepts success or `BrokenPipe` for the late write (Linux fd inheritance race, CI #21).
- **Docs:**
  - ADR 0008, the B1.3 report, architecture §7, testing, roadmap, this file;
  - the B1.2 doc updates that were missing from `f73a41a` (requirements, traceability, architecture, testing, roadmap);
  - requirements ADV06, COR-05, CORE-04 `in-progress`.

### Earlier — B1.2 ([report](docs/reports/b1.2-crash-recovery.md), ADR 0007, `f73a41a`)
- **API:**
  - `dm-backend-api`: `DurableCheckpoint` (opaque, versioned, ≤ 64 KiB) with `checkpoint` / `restore`;
  - new capability `durable_resume` (schema and TypeScript regenerated): `http` Supported, aria2 Unsupported (aria2 refuses restore).
- **`dm-backend-http`:**
  - writers flush + `sync_data` + publish confirmed state at least every 1 s / 4 MiB and on stop;
  - `durable.rs` format v1 (mode, destination binding, final URL, strong ETag, length, regions with SHA-256 digests; no hasher state);
  - import re-verifies the regions and rebuilds the hashers, truncates single-connection bytes past the checkpoint, and refuses foreign, corrupt, newer, misbound, damaged or destination-existing cases.
- **`dm-storage` migration 2:**
  - `jobs.attempt_id` and `resume_checkpoints`;
  - `save_checkpoint` (job + checkpoint in one transaction; `StaleAttempt` refusal), `rebind_checkpoint`, `load_checkpoint`;
  - `update_job` drops checkpoints of finished or re-attempted jobs.
- **`dm-service`:**
  - checkpoints on pause (with the paused state), every 2 s while transferring if they grew, and at graceful shutdown (`prepare_shutdown`);
  - `recover()` at start: a new attempt, a backend restore, an atomic rebind; paused jobs stay paused; refused restores fail with `RECOVERY_FAILED: …`;
  - `INTERRUPTED` remains for jobs without a usable checkpoint;
  - test hook `DOWNOVA_CHECKPOINT_TRACE`.
- **CI diagnostics:** `scripts/verify.mjs` emits a GitHub error annotation with the tail of each failed step (CI only).
- **Docs:**
  - ADR 0007, the B1.2 report, architecture §7, testing, roadmap;
  - requirements: ARCH-05 `implemented`; CORE-04, COR-08, ADV06 `in-progress`;
  - traceability.

### Earlier — B1.1 repair (CI #17)## Checks actually run (local, Windows 11 Pro 10.0.26300 x64, 2026-10-26)
| Command | Result |
|---|---|
| Repair tree (= `95322e3`): `cargo test -p dm-browser-host --test host -- --include-ignored`; `-p dm-storage`; `-p dm-backend-http --test durable_resume`; `-p dm-service --test http_backend_process`; `pnpm verify:strict` | PASSED: 8, 15, 9, 13; verify 19/19; no leftover services |
| `cargo test -q -p dm-browser-host --test host a_caller_not_on_the_allowlist_is_refused` ×20; `… refused` ×10 | PASSED every run (Windows only; no Linux here) |
| `cargo test -p dm-storage` | PASSED 19 |
| `cargo test -p dm-backend-http` | PASSED (lib 15, durable_resume 9, http_backend 24, segmented 16, validated_resume 15) |
| `cargo test -p dm-service --lib` | PASSED 35 |
| `cargo test -p dm-service --test http_backend_process -- f0` ×3 | PASSED 8/8 each |
| `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all --check`, `pnpm contracts:gen` (13 valid / 7 invalid examples), `check-structure` | PASSED |
| `pnpm verify:strict` (B1.3 tree) | **PASSED 19/19, 0 unavailable**, no leftover services (an earlier run on this tree FAILED 1 step: the contract round-trip serialized an absent `verification` as `null`; fixed by omitting it) |

No macOS or Linux run of B1.3 exists yet; CI will provide it after a push.
## Honest limitations
- B1.3:
  - process kills only (no power-loss claim; no directory sync on Windows);
  - the copy fallback for file systems without hard links is untested;
  - a crash mid-copy leaves a partial final file reported as a collision;
  - aria2 places before verifying;
  - hashing holds the coordinator lock;
  - no origin marking or scanning;
  - the UI does not show the verification source (B2).
- B1.2:
  - no power-loss claim, no `synchronous=FULL` benchmark;
  - recovery reads confirmed data twice;
  - aria2 jobs end as interrupted.
- CI: ubuntu-24.04 fix not yet run remotely; ubuntu-22.04 failures before #19 unattributed.
- B0.6: Edge not run; the menu click is simulated; the installer does not register the host (REL-01).
- Packages are unsigned (macOS ad-hoc) and not notarised. There is no connection budget across jobs (CORE-02).
## Blockers and open decisions
- Code signing and notarisation identities; project licence (REL-03); dependency licence/advisory review (REL-03); aria2 not releasable (REL-05).
- `gh` not installed: CI conclusions via the public API only.

## Exact next task
1. The user decides on committing B1.3. After the push:
   - confirm CI on all 4 runners (ubuntu-24.04 host test; F01–F08 and K01–K07 on macOS/Linux);
   - if anything fails, read its annotation: `https://api.github.com/repos/DevSagar007/Downova/check-runs/<job id>/annotations`.
2. **B1.4: basic disk budget (ADV05):**
   - `dm-platform` free-space query, in safe Rust;
   - a preflight before start and before finalization: known remaining bytes plus a margin, else `DISK_FULL` with nothing started;
   - `ENOSPC` while writing → the job stops with `DISK_FULL` (paused with a `disk_full`-style blocker or failed, per COR-07), keeping its partial file and durable checkpoint, and never touching other files;
   - tests: a fixture larger than an artificially small budget; injected `ENOSPC` (debug-only hook) → partial and checkpoint intact, resume after space frees → exact SHA-256.
