# PROGRESS — handoff

_Last updated: 2026-10-27 (Windows 11 Pro 10.0.26300 x64, Node 26.7.0, pnpm 12.14.0 via npx, rustc 1.99.1, aria2 1.37.0 in tools/aria2)_

## Current milestone
**B2.2 (close-to-tray, single instance, "Exit and stop downloads"; R01 kill-barrier repair): implemented locally on Windows 11, uncommitted (not pushed; no CI run yet).** Report: [docs/reports/b2.2-close-and-exit.md](docs/reports/b2.2-close-and-exit.md); [ADR 0012](docs/decisions/0012-exit-and-stop.md).

Baseline: `efc23f8` (B2.1).
- [Package-smoke #18](https://github.com/DevSagar007/Downova/actions/runs/40451506991) is green on all 3 OSes, including step F.
- [CI #28](https://github.com/DevSagar007/Downova/actions/runs/40451507077) is green on macos-latest, ubuntu-22.04 and ubuntu-24.04 and **failed on windows-latest** in `r01_repeated_hard_kills_during_transfers_end_exact`: a recovered job was `SERVICE_UNAVAILABLE` / interrupted.
- The R01 barrier is repaired in this change. No production defect was found, and the cause on the runner is not proven (report §1).

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
| B1.3 completion journal + host-test fix (ADR 0008) | `370c424` | [CI #22](https://github.com/DevSagar007/Downova/actions/runs/40287476544): **green on all 4 runners**, including real Chrome on windows, macOS and ubuntu-24.04; [package-smoke #12](https://github.com/DevSagar007/Downova/actions/runs/40287476545) green on all 3 |
| B1.4 disk budget (ADR 0009) | `0b14fbd` | [CI #23](https://github.com/DevSagar007/Downova/actions/runs/40292006636): **green on all 4 runners** (real Chrome on windows, macOS, ubuntu-24.04); [package-smoke #13](https://github.com/DevSagar007/Downova/actions/runs/40292006708) green on all 3 |
| B1.5 connection budget + admission (ADR 0010) | `e895f2b` | [CI #24](https://github.com/DevSagar007/Downova/actions/runs/40295179730): windows-latest and ubuntu-22.04 green; **macos-latest and ubuntu-24.04 failed** in `cargo test` (`dm-service` admission tests `pause_frees_a_slot…` / `at_most_three_jobs…`): jobs accepted in the same millisecond were ordered by their random IDs (fixed in B1.6); [package-smoke #14](https://github.com/DevSagar007/Downova/actions/runs/40295179716) green on all 3 |
| B1.6 file naming + admission tie-break fix (ADR 0011) | `93faed3` | [CI #25](https://github.com/DevSagar007/Downova/actions/runs/40299824155) cancelled by the next push; [package-smoke #15](https://github.com/DevSagar007/Downova/actions/runs/40299824161) green on all 3 |
| B1.6 progress/report | `3e8a9a9` | [CI #26](https://github.com/DevSagar007/Downova/actions/runs/40300613459): windows-latest and macos-latest green; **ubuntu-22.04 and ubuntu-24.04 failed** (case-sensitive ext4 vs. a case-insensitive test expectation; fixed in B1.7); [package-smoke #16](https://github.com/DevSagar007/Downova/actions/runs/40300613571) green on all 3 |
| B1.7 error catalogue + CI test repair | `b13e625` | [CI #27](https://github.com/DevSagar007/Downova/actions/runs/40445068317): **green on all 4 runners**; [package-smoke #17](https://github.com/DevSagar007/Downova/actions/runs/40445068364) green on all 3 (incl. step E) |
| B2.1 desktop download integration | `efc23f8` | [CI #28](https://github.com/DevSagar007/Downova/actions/runs/40451507077): macos-latest, ubuntu-22.04, ubuntu-24.04 green; **windows-latest failed** (R01, repaired in B2.2); [package-smoke #18](https://github.com/DevSagar007/Downova/actions/runs/40451506991) green on all 3 (incl. step F) |
| B2.2 close/exit lifecycle + R01 barrier | uncommitted | not run |

CI evidence is the public job conclusion (and the failure annotation `scripts/verify.mjs` emits) of each run; full job logs need GitHub authentication.

## Backends (ADR 0005)
`dm-service` defaults to the built-in `dm-backend-http` (Http, Https, ResumeStrongValidator, SegmentedHttp supported); aria2 only with `--backend aria2` (Http only) and never bundled while REL-05 is open. No fallback between backends.

## Completed (summary)
- Foundation; ADRs 0001–0005; CI on 4 OSes; aria2 adapter (hardened); service (user-only IPC, capability gates, service-owned retry); `dm` CLI.
- `dm-backend-http`: HTTP/HTTPS with platform trust, manual redirects, origin-scoped credentials, identity encoding, validated pause/resume, validated segmentation.
- Reports: [aria2 B0.4](docs/reports/b0.4-http-correctness.md), [dm-backend-http](docs/reports/b0.4.2-http-backend.md), [B0.5 packaging](docs/reports/b0.5-packaging-smoke.md).

### This session — B2.2 ([report](docs/reports/b2.2-close-and-exit.md), ADR 0012)
- **CI #28 repair (test):**
  - R01 kills only at `wait_all_durable`: every unfinished job needs a `checkpoint`/`recovered` trace event for
    its current attempt, and the primary job a newer one; completed jobs are verified first; failures print the job's
    trace;
  - the service traces `retry_scheduled` (test hook);
  - a 1 ms kill-timing experiment (6 runs) and 5 full-binary runs did not reproduce the failure. No production defect
    was found.
- **Service and contract (ADR 0012):**
  - `ShutdownParams { mode? }` and `ShutdownResult { mode, paused_jobs }` (schema, Rust, TypeScript, 3 + 2
    examples);
  - `Core::begin_shutdown`: stopping flag; `pause_downloads` pauses durably before the acknowledgement;
  - admission, new adds and resume refused while stopping;
  - only an accepted shutdown stops the server; params must be an object (`[]` had passed as `{}`);
  - Ctrl+C enters the stopping state too;
  - a paused job without an attempt recovers paused, not interrupted.
- **Shell** (`lifecycle.rs`):
  - `desktop-settings.json` close behaviour (`hide_to_tray` by default, or `close_window`);
  - tray "Open Downova" / "Exit and stop downloads", relabelled by the UI;
  - hide, or minimize without a tray, or close;
  - single instance (`tauri-plugin-single-instance` 2.5.4; `tauri` feature `tray-icon`);
  - `service_exit_and_stop`: one at a time, acknowledgement, then verified gone (≤ 20 s), then quit;
  - 4 new commands, declared and granted by name;
  - smoke hooks: lifecycle event log, close and exit triggers.
- **UI:**
  - `DesktopShell` (Tauri / browser) via `ServiceProvider`;
  - the Options close setting;
  - `ExitDialog` from the Tasks menu and the tray;
  - tray labels in the window's language; English and Bangla strings.
- **Tests:**
  - core: 3 lifecycle tests; server: valid versus invalid shutdown;
  - process: X01;
  - `dm-desktop`: lifecycle (3);
  - `lifecycle.test.tsx` (8); `FakeShell`;
  - packaged smoke G1–G3 and H1–H3; step C pinned to `close_window`; the Linux smoke runs in `dbus-run-session`.
- **Docs:**
  - ADR 0012 and the B2.2 report;
  - requirements UI-04 `implemented`, ARCH-09 `in-progress`;
  - traceability UI-04, ARCH-09, CORE-04;
  - testing, architecture §2/§5, design §4/§7, roadmap (B2.2 done, B2.3 defined);
  - B2.1 report status corrected.

### Earlier — B2.1 ([report](docs/reports/b2.1-desktop-downloads.md), `efc23f8`)
- **Shell bridge** (`apps/desktop/src-tauri`):
  - `src/downloads.rs` adds `downloads_list`, `downloads_add`, `downloads_pause`, `downloads_resume` and `downloads_cancel`:
    fixed contract methods over the `dm-platform` IPC client, arguments validated in Rust, structured `ErrorData`,
    and the outcome `unavailable` / `uncertain` / `rejected`;
  - timeouts: connect 3 s, list 5 s, acknowledgement 15 s;
  - `build.rs` app manifest with exactly six commands, each granted by name in `capabilities/default.json`;
    generated `permissions/autogenerated/` is ignored;
  - smoke-only `src/smoke.rs` (the B0.5 report plus the B2.1 UI driver).
- **`dm-domain::validate`:** `check_url`, `is_token` and `is_idempotency_key`, moved from the browser host (which
  re-exports them) so the desktop uses the same checks.
- **UI:**
  - `ServiceClient` list/add/pause/resume/cancel with `ServiceCallError`;
  - `ServiceProvider` polling (1 s, never overlapping, stale answers dropped, stops when the service is lost) and
    actions; Reconnect;
  - `AddUrlDialog`: validation, one request at a time, the same key for an unconfirmed link even after reopening;
  - row selection by job ID (`aria-pressed` buttons, ↑/↓);
  - Add URL, Stop and Resume wired in the toolbar and the menus, with snapshot-based reasons (failed is never
    resumable); Delete / "Remove from list" unavailable with an explicit reason;
  - `DownloadDetails` (error, action and follow-up; failed commands) and the service notice Banner;
  - verification text; received size without a total; `Dialog` `data-autofocus`;
  - About text corrected; new English and Bangla strings.
- **Tests:**
  - `cargo test -p dm-desktop` (7 bridge tests on the real IPC transport);
  - `validate` unit tests;
  - `downloads-live.test.tsx` (21) and `service-client.test.ts` (4); `App.test.tsx` updated;
  - `src/test/fake-service.ts`;
  - packaged smoke step F (F1–F4, plus a `desktop-live-list` screenshot).
- **Docs:**
  - the B2.1 report;
  - requirements CORE-01 and SEC-05 `implemented`; ARCH-08 note; traceability CORE-01, CORE-03, UI-03, SEC-05, F01
    and ARCH-08;
  - design.md §4–§6, architecture §3 (desktop shell bridge), testing §6–§7, roadmap (B2.1 done, B2.2 defined).

### Earlier — B1.7 ([report](docs/reports/b1.7-error-catalogue.md), `b13e625`)
- **Linux CI repair (E04):**
  - `simultaneous_jobs_announcing_one_name_get_distinct_names` now uses an exact-case existing file;
  - the new `an_existing_name_differing_in_case_follows_the_file_system` measures the temporary folder's case behaviour
    and expects ADR 0011 §8 for that folder;
  - staging-name and user-file assertions kept; production naming unchanged.
- **Domain/contract:**
  - `ErrorKind::ALL` and `NextAction::ALL`; a test that both equal the schema enums;
  - `ErrorData.retryable` documented as "transient", not "will be retried" (schema description and generated TS).
- **Service fixes found by the audit:**
  - `resume_refusal`: a backend's resume refusal without a strong validator now carries `choose_restart_or_keep` (it
    was `none`; immediate resume and admission);
  - a file appearing at the frozen final name is `DESTINATION_UNWRITABLE` (was `INVALID_REQUEST`; ADR 0011 §9
    amended; F04 updated).
- **CLI** (`crates/cli/src/output.rs`):
  - `explain(kind)` adds a `meaning:` / `Meaning` line;
  - `hint(action, situation)` gives a different step for a refused command, a waiting download and a failed one;
  - no more "then resume" for waiting disk jobs, "export diagnostics" or sign-in promises;
  - JSON and exit codes unchanged.
- **Desktop UI copy:**
  - `error.kind.*` (19), `error.action.*` (9) and `error.followUp.*` (3) in English and Bangla;
  - `present.ts` `errorText`; the description cell shows the localized summary (layout unchanged).
- **Tests added:**
  - domain schema parity;
  - CLI catalogue (4);
  - UI `errors.test.ts` (5);
  - core `a_refused_resume_offers_the_restart_choice_only_for_capability_refusals`;
  - process E01, E03, E04, E06 (real OS permission restriction);
  - `assert_disk_blocked` checks next actions (DK01–DK11);
  - R01/R02 repeated kills (`DOWNOVA_KILL_REPS`);
  - storage `checkpoint_latency` benchmark (ignored by default);
  - packaged smoke step E (forced desktop kill).
- **Docs:**
  - the B1.7 report;
  - requirements CORE-04, CORE-11 and COR-08 `implemented`;
  - traceability CORE-04, CORE-11, COR-08 and E04;
  - testing §1/§3/§9, architecture §5, roadmap (B1.7 done, B2.1 defined), ADR 0011 §9.

### Earlier — B1.6 ([report](docs/reports/b1.6-file-naming.md), ADR 0011, `93faed3`, docs `3e8a9a9`)
- **`dm-backend-api`:**
  - `BackendProgress.naming: Option<ResponseNaming>` (unparsed `Content-Disposition` ≤ 4 KiB, still-encoded final URL segment; never the query);
  - `destination_refused` (shared file-system refusal mapping);
  - `TransferRequest.file_name` documented as the stable staging name.
- **`dm-backend-http`:** records naming from the final `200` after redirects (and a validated `206` when the attempt saw no `200`); non-UTF-8 field bytes read as ISO-8859-1; `InvalidFilename` / `PermissionDenied` / `ReadOnlyFilesystem` / not-a-folder → `DESTINATION_UNWRITABLE`. aria2 reports no naming.
- **Contract:** `ErrorKind` gains `DESTINATION_UNWRITABLE` (schema, TypeScript, Rust).
- **`dm-service` `naming.rs`:**
  - RFC 6266/8187 parser (precedence `filename*` > `filename` > response URL > request URL > `download`; duplicates ignored; UTF-8/ISO-8859-1 only);
  - NFC sanitizer (separators, controls, bidi overrides, leading/trailing dots and spaces, Windows device names incl. `COM¹`, control suffixes);
  - `fit`: 255 UTF-16 units (Windows) / 255 UTF-8 bytes (elsewhere), 24-unit suffix reserve, 259-unit / 4095-byte path target with a 32-unit floor, grapheme-safe truncation keeping the extension;
  - `unique`: existing entries, other jobs' final and staging names (NFC, case-insensitive), foreign `.downova-part` / `.aria2` files.
- **`dm-service` core:**
  - the staging name is fixed at acceptance and given to every attempt;
  - `select_final_name` freezes the final name the first time metadata arrives (before finalization in the same poll), persists it and traces `name_selected`;
  - finalization freezes the provisional name at the journal intent at the latest; aria2's engine-placed file keeps its path;
  - placement refusals by the file system → `DESTINATION_UNWRITABLE`;
  - admission order: creation time, then acceptance order (fixes the CI #24 failure).
- **`dm-storage` migration 4:** `jobs.staging_name` and `jobs.name_source`; existing jobs become `legacy` (frozen, staging = display name).
- **Docs:**
  - ADR 0011, the B1.6 report, architecture, testing, roadmap (B1.6 done, B1.7 next);
  - requirements E04 `implemented`;
  - traceability E04, CORE-11, ADV05;
  - the B1.5 doc updates; ADR 0010 and the B1.5 report note the corrected tie-break.

### Earlier — B1.5 ([report](docs/reports/b1.5-connection-budget.md), ADR 0010, `e895f2b`)
- **`dm-backend-http` `limits.rs`:**
  - `HostLimiter`: one semaphore per host key (lowercase/punycode host, trailing dot removed, port and scheme ignored), `HttpConfig.max_connections_per_host` = 8;
  - a permit is taken before every request (probe, transfer, resumed range, segment worker, each redirect hop) and held until its body is dropped; redirects release the previous hop first;
  - waiting stops with the attempt;
  - `pool_max_idle_per_host(0)`.
- **`dm-service` admission:**
  - `MAX_ACTIVE_FILES` = 3;
  - a `Pending` start (`Fresh` / `Resume`) shows `queued` + `queue_limit`;
  - `Core::admit` (oldest first; never waits) runs after add, resume, pause, cancel, every poll and recovery;
  - slots are held only by live, user-running, non-finalizing, non-blocked attempts;
  - recovered attempts are restored paused and then admitted;
  - a never-started queued job stays queued across a restart;
  - an immediate resume still returns its refusal; a refusal at admission leaves the job paused, never failed.
- **Fixture:** `Server` counts open TCP connections and records each connection's lifetime (`max_overlap`, `max_overlap_since`, `url_on`).
- **Docs:**
  - ADR 0010, the B1.5 report, architecture, testing, roadmap;
  - requirements CORE-02 and E03 `implemented`;
  - traceability E03, CORE-02, CORE-03;
  - the B1.4 doc updates.

### Earlier — B1.4 ([report](docs/reports/b1.4-disk-budget.md), ADR 0009, `0b14fbd`)
- **`dm-platform::space`:**
  - `SpaceProbe` trait; `OsSpaceProbe` (`fs4::available_space`) returns `Available` / `VolumeMissing` / `Unavailable`, never an invented number;
  - debug-only `FileSpaceProbe` via `DOWNOVA_TEST_FREE_SPACE_FILE`.
- **`dm-backend-api`:** `SpaceGuard` trait plus `NoSpaceGuard`. Backends get the guard through `HttpConfig.space`, with no dependency on platform or service.
- **`dm-service::space::PlatformSpaceGuard`:** a 64 MiB margin; advisory pass when unknown (reported once); `DISK_FULL` / `VOLUME_MISSING` with actionable text.
- **`dm-backend-http`:**
  - budget before the first body byte and at every durable sync (unknown length: the next 4 MiB window);
  - segmented budgets the whole unconfirmed length before sizing;
  - `QuotaExceeded` is also classified `DISK_FULL`;
  - checkpoints stay exportable after a failure;
  - debug-only `enospc_write` / `enospc_sync` faults.
- **`dm-service` core:**
  - `block_on_disk` commits the checkpoint, releases the attempt (keeping the partial file; with nothing durable the unconfirmed partial is removed) and sets the `disk_unavailable` / `volume_missing` blocker, keeping desired_state;
  - `recheck_disk` every 2 s, then a validated restore or a fresh start;
  - resume of a waiting job only restores its desired state;
  - a waiting job without a checkpoint keeps waiting across a restart;
  - aria2 keeps terminal disk-full failure.
- **Finalization:** the copy fallback is budgeted; `ENOSPC` in the copy removes only the partial copy; the job waits at `verified` (journal kept); faults `no_hard_links` and `enospc_copy`; `DOWNOVA_FAULT` is now a comma-separated list.
- **Docs:**
  - ADR 0009, the B1.4 report, architecture, testing, roadmap;
  - requirements ADV05 `in-progress`;
  - traceability ADV05, COR-07, CORE-04.

### Earlier — B1.3 ([report](docs/reports/b1.3-completion-journal.md), ADR 0008, `370c424`)
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

### Earlier — B1.1 repair (CI #17)

## Checks actually run (local, Windows 11 Pro 10.0.26300 x64, 2026-10-27)
| Command | Result |
|---|---|
| R01 1 ms kill-timing experiment (temporary test, removed) ×6 | the segmented job always had a committed checkpoint and recovered; CI failure not reproduced |
| `cargo test -p dm-service --test http_backend_process` ×5 (repaired R01, before the lifecycle changes) | PASSED 46/46 each |
| `cargo test -p dm-service --lib` | PASSED 62 (incl. 3 exit-and-stop core tests and `only_a_valid_shutdown_request_stops_the_service`) |
| `cargo test -p dm-service --test http_backend_process` | PASSED 47 (incl. X01: paused at 884,736 B, process exited 0, resumed `bytes=884736-`, SHA-256 match) |
| `cargo test -p dm-domain` (contract examples: 13 → 16 valid, 7 → 9 invalid), `-p dm-cli`, `-p dm-browser-host`, `-p dm-desktop` | PASSED |
| `cargo clippy -p dm-desktop --all-targets [--features smoke] -- -D warnings` | PASSED |
| `vitest` desktop | PASSED 54/54 |
| `node scripts/package.mjs --smoke` + `node scripts/smoke/packaged-smoke.mjs --package-dir dist-packages/windows-x86_64-smoke-test` | **passed**, every step: A–F as before; **G1** hidden, desktop and service alive; **G2** second launch exit 0, first window visible and focused; **G3** SHA-256 match; **H1** `{paused_jobs: 1, stopped: true}`, service gone, desktop quit; **H2** paused at 786,432 B, unchanged 2 s later; **H3** resume → SHA-256 match; D uninstall |
| `pnpm verify:strict` | 17 passed, 2 failed: `structure` (ADR 0012 linked to the B2.2 report before it was written) and `format` (prettier on the capability file and the schema), both from documents still being written during the run. Every Rust, aria2, native-host, test and build group PASSED. After formatting, `pnpm verify -- --only=structure,contracts,format,lint,typecheck,test,build` PASSED 11/11 |

No macOS or Linux run of B2.2 exists. Tray icon and menu clicks have not been checked by hand on any OS.

## Honest limitations
- B2.2:
  - tray clicks are manual checks, not yet done;
  - Linux single instance needs a D-Bus session; a tray may be invisible without a StatusNotifier host, and then
    only a second launch reopens the window;
  - macOS Dock-icon reopen is not wired;
  - an unacknowledged exit may still be stopping (the dialog says so);
  - ARCH-09 covers only UI close;
  - the CI #28 R01 cause is not proven.
- B2.1:
  - updates come from polling about once a second (no events, ARCH-08), with one IPC connection per call;
  - single selection only; no context menu, details dialog or progress bar;
  - cancel has no UI control; Delete waits for removal semantics (CORE-10);
  - the add dialog uses profile `downloads` only;
  - an unconfirmed add's key is forgotten when the app restarts;
  - status cells are distinguished by text, not by a tone icon (UI-03 `in-progress`);
  - the selected-row CSS specificity fix came after the packaged run (CSS only);
  - packaged step F does not exercise Stop or Resume (the UI tests do).
- B1.7:
  - process kills only; no power-loss test (the `synchronous=FULL` benchmark measures cost, not power-loss safety);
  - a job that fails before receiving a byte leaves an empty `<staging>.downova-part` (existing behaviour, asserted in E01; cleanup candidate);
  - 403 → `LINK_REFRESH_REQUIRED` and `NotFound` → `VOLUME_MISSING` are heuristics;
  - `HOST_NOT_INSTALLED` and `SCAN_BLOCKED` have copy but are never produced by the service;
  - Bangla copy has no native-speaker QA pass yet (B3); the CLI is English only;
  - E06 on Unix (mode 0555) has not run; as root it reports "not run";
  - the case-sensitive naming branch ran on NTFS with the per-folder flag, not on ext4;
  - `downloads.v1.retry` / `replaceSource` are not implemented: a failed download restarts only by adding its link again.
- B1.6:
  - limits are per platform, not probed per volume (eCryptfs, some network shares refuse at creation → `DESTINATION_UNWRITABLE`);
  - deep folders rely on long-path support beyond the 32-unit floor;
  - no RFC 2231 continuations or RFC 2047 words;
  - a file appearing at the frozen name stops placement explicitly (no automatic rename);
  - aria2 jobs keep URL names;
  - read-only volumes are classified but not process-tested (permission denial now is: E06 on Windows);
  - the UI does not show the name change yet (B2).
- B1.5:
  - the per-host guarantee counts Downova's open requests; servers see sub-100 ms teardown tails (measured, reported raw);
  - the budget is per backend instance;
  - no idle pool: a handshake per request, not benchmarked;
  - no speed claims;
  - no stalled-server timeout test;
  - aria2 is conservatively 1 connection per file;
  - limits are fixed, with no priorities or queues (B2).
- B1.4:
  - advisory snapshots, no reservations across concurrent jobs; fixed 64 MiB margin;
  - `ENOSPC` injected, not observed on a real full disk;
  - unknown-length or validator-less downloads restart after disk full;
  - SQLite failing on a full disk is reasoned about, not fault-tested;
  - no removable-volume identity (B4);
  - aria2 disk full stays terminal;
  - the UI does not show blockers (B2).
- B1.3:
  - process kills only (no power-loss claim; no directory sync on Windows);
  - a crash mid-copy leaves a partial final file reported as a collision;
  - aria2 places before verifying;
  - hashing holds the coordinator lock.
- B1.2: recovery reads confirmed data twice; aria2 jobs end as interrupted (the `synchronous=FULL` benchmark exists since B1.7).
- B0.6: Edge not run; the menu click is simulated; the installer does not register the host (REL-01).
- Packages are unsigned (macOS ad-hoc) and not notarised.

## Blockers and open decisions
- Code signing and notarisation identities; project licence (REL-03); dependency licence/advisory review (REL-03); aria2 not releasable (REL-05).
- `gh` not installed: CI conclusions via the public API only.

## Exact next task
1. **Commit and push B2.2 (only when the user asks)**, then confirm:
   - CI on all 4 runners, especially windows-latest R01 with the new barrier (if it fails, the message now carries the
     job's checkpoint trace);
   - package smoke on all 3 OSes, with G/H on macOS and Linux (`dbus-run-session`).

   Failures are readable at `https://api.github.com/repos/DevSagar007/Downova/check-runs/<job id>/annotations`.
2. **Manual tray check per OS:** the title-bar close button, the tray icon, Open Downova, and Exit and stop downloads.
   Record the results in the B2.2 report.
3. **B2.3:**
   - status tone icons and a progress bar in the Status cell;
   - "Remove from list" with service removal semantics (`downloads.v1.remove`, contract, ADR; never deletes files,
     never cancels running jobs);
   - a cancel control with confirmation.

   Tests: UI with the fake client, service removal tests, and a packaged step that removes a completed job and keeps
   its file. Queues, scheduler, speed limits, event streaming and the extension stay out of scope.
