# PROGRESS — handoff

_Last updated: 2026-10-28 (Windows 11 Pro 10.0.26300 x64, Node 26.7.0, pnpm 12.14.0 via npx, rustc 1.99.1, aria2 1.37.0 in tools/aria2, Chrome 156.0.8078.48, Edge 156.0.4314.50)_

## Current milestone
**B2.6 (cross-platform acceptance verification and B2 gap audit): in progress on `1d87c0b`; a Linux harness repair
is uncommitted.** Report: [docs/reports/b2.6-acceptance.md](docs/reports/b2.6-acceptance.md).

Baseline: `1d87c0b` (B2.6 verification), pushed.
- [CI #33](https://github.com/DevSagar007/Downova/actions/runs/40646555464) is **green on all 4 runners**.
- [Package smoke #23](https://github.com/DevSagar007/Downova/actions/runs/40646555452) on the same commit:
  - windows-x86_64: **56 passed, 1 note** — R3 real Chrome 156.0.8078.48 and Edge 155.0.4283.62, R4 and D2 passed;
  - macos-aarch64: **56 passed, 1 note** — R3 real Chrome 155.0.8059.40 and Edge 155.0.4283.45, R4 and D2 passed;
  - **linux-x86_64 failed** ([job](https://github.com/DevSagar007/Downova/actions/runs/40646555452/job/122676605931)):
    47 passed, 1 note, 4 failed. R1/R2/R5/D0/D passed; failed rows are R3 Chrome (`e2e run`), R3 Edge
    (`e2e run`), R4 and D2.
  - Row annotations were read through the public API, so the previous "rows unreadable" limitation no longer
    applies to #23.
- **Linux cause (confirmed in Chromium source):** Chrome 136+ refuses the DevTools pipe when `--user-data-dir` is
  the default directory (`IsUsingDefaultDataDirectory`). On Linux the product manifest lives in exactly that
  directory (`$XDG_CONFIG_HOME/<browser>`), so both browsers timed out before loading the extension
  (`Extensions.loadUnpacked: no answer from the browser in 60 s`). macOS passes because its default is the real
  OS home, not the isolated `HOME`; Windows uses a temporary profile. The product's native-messaging path is
  unaffected. Repair: [report §1](docs/reports/b2.6-acceptance.md#1-baseline-what-ci-shows).
- **Browser hand-off gate: not passed.** Automated (simulated menu, product registration) on Windows 11 and in
  package smoke #23 on Windows and macOS for Chrome and Edge; Linux waits for the repair to be pushed and rerun;
  no human right-click on any OS.
- **B2 overall: not complete.** Of 20 B2 requirements, 6 are `implemented`, 6 `in-progress`, 8 `documented`;
  ARCH-08, CORE-05 and most of CORE-07 are not built
  ([audit](docs/reports/b2.6-acceptance.md#4-b2-requirement-audit)).

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
| B2.2 close/exit lifecycle + R01 barrier | `88e550e` | [CI #29](https://github.com/DevSagar007/Downova/actions/runs/40457197491): macos-latest, ubuntu-22.04, ubuntu-24.04 green; **windows-latest failed: DNS error fetching a crate (no test ran)**; [package-smoke #19](https://github.com/DevSagar007/Downova/actions/runs/40457197501) green on all 3 (incl. G, H) |
| B2.3 status, cancel, remove from list | `12966a7` | [CI #30](https://github.com/DevSagar007/Downova/actions/runs/40462052415): **green on all 4 runners** (first Windows run of R01 with the B2.2 barrier: passed); [package-smoke #20](https://github.com/DevSagar007/Downova/actions/runs/40462052409) green on all 3 (incl. G, H, I) |
| B2.4 CLI remove/shutdown | `10ae139` | [CI #31](https://github.com/DevSagar007/Downova/actions/runs/40471760829): **green on all 4 runners** (incl. the CLI real-service step); [package-smoke #21](https://github.com/DevSagar007/Downova/actions/runs/40471760744) green on all 3 |
| B2.5 browser registration | `645ca86` | [CI #32](https://github.com/DevSagar007/Downova/actions/runs/40640583940): **green on all 4 runners**; [package-smoke #22](https://github.com/DevSagar007/Downova/actions/runs/40640584294): windows and macOS green (rows unreadable), **linux-x86_64 failed** (browser runs hung; B2.6) |
| B2.6 acceptance verification | `1d87c0b` | [CI #33](https://github.com/DevSagar007/Downova/actions/runs/40646555464): **green on all 4 runners**; [package smoke #23](https://github.com/DevSagar007/Downova/actions/runs/40646555452): windows 56 + 1 note, macOS 56 + 1 note, **linux 47 + 1 note + 4 failed** (R3 Chrome/Edge, R4, D2) |
| B2.6 Linux harness repair | uncommitted | not run |

CI evidence is the public job conclusion (and the failure annotation `scripts/verify.mjs` emits) of each run; full job logs need GitHub authentication.

## Backends (ADR 0005)
`dm-service` defaults to the built-in `dm-backend-http` (Http, Https, ResumeStrongValidator, SegmentedHttp supported); aria2 only with `--backend aria2` (Http only) and never bundled while REL-05 is open. No fallback between backends.

## Completed (summary)
- Foundation; ADRs 0001–0005; CI on 4 OSes; aria2 adapter (hardened); service (user-only IPC, capability gates, service-owned retry); `dm` CLI.
- `dm-backend-http`: HTTP/HTTPS with platform trust, manual redirects, origin-scoped credentials, identity encoding, validated pause/resume, validated segmentation.
- Reports: [aria2 B0.4](docs/reports/b0.4-http-correctness.md), [dm-backend-http](docs/reports/b0.4.2-http-backend.md), [B0.5 packaging](docs/reports/b0.5-packaging-smoke.md).

### This session — B2.6 ([report](docs/reports/b2.6-acceptance.md))
- **Baseline `1d87c0b`:** CI #33 green on all four runners. Package smoke #23 passed on windows (56 + 1 note) and
  macOS (56 + 1 note), including R3 Chrome + Edge, R4 and D2. Linux failed with 4 rows: R3 Chrome, R3 Edge, R4 and
  D2. The public row annotations were read, so those rows are evidence now, not "unreadable".
- **Linux failure and cause:** both browsers timed out before the extension loaded (`Extensions.loadUnpacked: no
  answer from the browser in 60 s`); R4 therefore had no job and D2 had no hand-off file. Cause confirmed in
  Chromium source: Chrome 136+ refuses the DevTools pipe when `--user-data-dir` is the default
  (`IsUsingDefaultDataDirectory`), and the Linux product manifest is written into exactly that directory
  (`$XDG_CONFIG_HOME/<browser>`). macOS's default is the real OS home (not the isolated `HOME`) and Windows uses a
  temporary profile, which is why both passed. Native messaging in the product is unaffected.
- **Repair (scripts only, uncommitted):**
  - in Linux product-registration mode the browser process gets an isolated `XDG_CONFIG_HOME`/`CHROME_CONFIG_HOME`,
    while `--user-data-dir` stays the product directory, so the registration is still the one the browser reads;
  - `Browser.getVersion` readiness (20 s) before `Extensions.loadUnpacked`, per-call bounds, and distinct
    transport/extension errors;
  - bounded, sanitized stderr with exit code and signal (`browser-stderr.log`, `results.browser_diagnostics`);
  - every launched browser is closed or killed in `finally`, including startup failures;
  - R4 is `unavailable` (not failed) when the Chrome hand-off did not run; D2 is split into registration/data and
    D2b file survival;
  - `scripts/lib/browser-profile.mjs` + 3 unit tests; wired into `pnpm verify` (structure group).
- **Local verification after the repair (Windows 11, 2026-10-28):**
  - `node --test scripts/lib/browser-profile.test.mjs`: 3/3 PASSED;
  - `node scripts/e2e/chrome-handoff.mjs --kind chrome` on Chrome 156.0.8078.48: steps 0–5 and unregister PASSED
    (`smoke-results/b26-repair-chrome`);
  - `node scripts/smoke/packaged-smoke.mjs --package-dir dist-packages/windows-x86_64-smoke-test`: 60/60 PASSED,
    including R3 Chrome/Edge, R4, D2 and D2b (`smoke-results/b26-repair-windows`);
  - `node scripts/verify.mjs --only=structure,format,lint`: 4/4 PASSED.
  - **Not verified:** Linux and macOS after the repair (needs a pushed run), Edge's use of the isolated config home
    on Linux, H1–H11 by hand, and manual tray clicks.
- **Manual acceptance:** `scripts/acceptance/fixture-server.mjs` (link page, expected SHA-256, request log), the H1–H11
  procedure, a Windows production package built from `645ca86`. **Human cases not run.**
- **Audit:** all 20 B2 requirements mapped to behaviour, evidence, gaps and the next increment.
  - Statuses corrected: SEC-05 (11 commands), SEC-03 and CORE-07 notes, BRW-01/BRW-07 evidence.
  - A corrupted first line of `docs/traceability.md` (rows fused since before B0.6) was replaced with its heading.
  - Checked that the NSIS uninstaller cannot delete the database even though the default install folder is the data
    folder: it removes only its own files, plus a non-recursive `RMDir`.

### Earlier — B2.5 ([report](docs/reports/b2.5-browser-registration.md), ADR 0014, `645ca86`)
- **Handoff:** CI #31 and package smoke #21 recorded; stale B2.4 statements corrected.
- **Ownership-checked registration** (`dm-browser-host`, strengthened in place):
  - an entry is ours only when its manifest names `com.downova.host` and this installation's host path;
  - `register`/`repair` refuse other installations and unreadable files unless `--replace`, and never replace a
    different host;
  - `unregister` removes only owned entries and lists what it kept;
  - `status --json`;
  - per-browser exit 3;
  - disk-image and translocated paths refused;
  - Chrome for Testing locations; `$XDG_CONFIG_HOME` on Linux.
- **Setup flows:**
  - NSIS `installer-hooks.nsh` (per user): register after install, unregister before uninstall;
  - the desktop's first launch runs the sibling host (`browser_setup.rs`, `browser-setup.json`,
    `DOWNOVA_SKIP_BROWSER_SETUP`);
  - MSI, macOS and `.deb`: first launch;
  - no-hook uninstalls: the documented `dm-browser-host unregister`.
- **Harness:**
  - the e2e has an installed / product-registration mode;
  - Edge `--edge-skip-compat-layer-relaunch` gave the first real Edge run;
  - packaged smoke R0–R5, D0/D2;
  - the smoke awaits the e2e asynchronously, so its fixture server keeps serving.
- **Docs:** ADR 0014; setup (installed browser integration, repair, unregister); architecture; testing; BRW-07 →
  `implemented`; BRW-01 and REL-01 evidence; roadmap (B2.5 done, B2.6 defined).

### Earlier — B2.4 ([report](docs/reports/b2.4-cli-parity.md), `10ae139`)
- **`dm remove <JOB-ID>`** → `downloads.v1.remove`. Human output says the file was kept; unfinished jobs give
  exit 4.
- **`dm shutdown [--pause-downloads]`** → `service.v1.shutdown` with its mode. Success (exit 0,
  `stopped: true`) only after the endpoint stopped answering (`RunOptions::stop_timeout`, 20 s); otherwise
  exit 1 "acknowledged … but still running". `--pause-downloads` is refused elsewhere.
- **Extension:** reachability already shown since B0.6; an "open the app" link is not possible without new
  permissions or installer registration (recorded as a gap).
- **Tests:** CLI peer tests (4); real-service `cli_remove_and_paused_shutdown_with_http_backend`.
- **Docs:** the B2.4 report; F01 note; traceability F01; testing (CLI); roadmap (B2.4 done, B2.5 defined).

### Earlier — B2.3 ([report](docs/reports/b2.3-status-cancel-remove.md), ADR 0013, `12966a7`)
- **Status presentation:**
  - Status cell with a Lucide icon and a semantic tone per state (manual pause before blockers);
  - a BigInt progress bar (0–1000, labelled, `aria-valuetext`) only while unfinished with a known non-zero total;
  - new token `--progress-height`;
  - polite announcements for complete, failed, cancelled and removed only.
- **Cancel download:**
  - File → "Cancel download…" with a confirmation (says nothing is deleted; the partial file stays);
  - one request, acknowledgement-only display; not offered while finishing, failed or finished;
  - **service fix:** `cancel` keeps the attempt when the engine fails to stop (it used to drop ownership first).
- **Remove from list (ADR 0013):**
  - `downloads.v1.remove`, `DownloadSnapshot.removed` (schema, Rust, TypeScript, examples);
  - migration 5 `jobs.removed_at_ms` (tombstone; rows and keys kept); `Store::remove_job`, `removed_jobs`,
    `purge_removed`;
  - `Core::remove` (finished only, persisted before the acknowledgement, files untouched); lists omit removed jobs;
    `get` and replays answer; tombstones are purged after key retention;
  - bridge `downloads_remove`;
  - UI File → "Remove from list…" with a confirmation, then selection and focus move;
  - Delete stays unavailable and points to Remove from list.
- **Other:**
  - `MenuBar` items carry `data-command` (stable hook);
  - smoke-only row-command driver;
  - packaged smoke step I.
- **Tests:**
  - storage 4;
  - core 3; process RM01/RM02;
  - bridge 1;
  - UI `status-cancel-remove.test.tsx` (11); updated Delete reason assertion.
- **Docs:**
  - ADR 0013 and the B2.3 report;
  - requirements UI-03 `implemented`, CORE-10 `in-progress`;
  - traceability UI-03, CORE-10, CORE-03, COR-09;
  - design §3/§4/§5, testing, roadmap.

### Earlier — B2.2 ([report](docs/reports/b2.2-close-and-exit.md), ADR 0012, `88e550e`)
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

## Checks actually run (local, Windows 11 Pro 10.0.26300 x64, 2026-10-28)
| Command | Result |
|---|---|
| Public GitHub API: CI #33 and package smoke #23 job annotations | read: CI green on 4; #23 windows/macOS 56 passed + 1 note, Linux 47 + 1 note + 4 failed |
| `node scripts/smoke/packaged-smoke.mjs --package-dir dist-packages/windows-x86_64-smoke-test --out smoke-results/b26-repair-windows` (package `645ca86`, repaired scripts) | PASSED 60/60 incl. R3 Chrome/Edge, R4, D2, D2b |
| `node scripts/e2e/chrome-handoff.mjs --kind chrome --out smoke-results/b26-repair-chrome` (Chrome 156.0.8078.48) | PASSED steps 0–5 and unregister; stderr diagnostics written |
| `node --test scripts/lib/browser-profile.test.mjs` | PASSED 3/3 |
| `node scripts/verify.mjs --only=structure,format,lint` (after the repair) | PASSED 4/4 |
| `GITHUB_ACTIONS=true node scripts/smoke/packaged-smoke.mjs …` (`645ca86` package, `1d87c0b` scripts) | PASSED 57/57; smoke, Chrome and Edge annotations emitted |
| `node scripts/acceptance/fixture-server.mjs --port 8799` + curl | page, 206 for Range, SHA-256 match, request log |
| `node scripts/package.mjs` (production, `645ca86`) | PASSED (NSIS `403f5d6c…`, MSI `cd128796…`) |
| `pnpm verify:strict` (`1d87c0b`) | PASSED: 19 passed, 0 failed, 0 unavailable |

No macOS or Linux run of the repair exists; it needs a pushed package-smoke run. No human acceptance case was run.

## Honest limitations
- B2.6:
  - The Linux failure cause is confirmed from Chromium source, not from the job log (logs need sign-in). The repair
    is unverified on Linux; whether Edge on Linux honours `CHROME_CONFIG_HOME`/`XDG_CONFIG_HOME` is unconfirmed.
  - Windows and macOS rows are read from #23; CI runs real Edge only on Windows and macOS, not Ubuntu.
  - The human H1–H11 flow has not been run on any OS.
  - The two headless Edge probe processes from B2.5 may still be running locally.
- B2.5:
  - macOS registration and D0 passed in #23; Linux registration passed, but its browser hand-off is blocked by the
    harness issue above;
  - a real per-machine MSI install was not run (administrative extract only);
  - the menu click is simulated, and no human right-click is recorded;
  - store IDs are unknown (development ID only);
  - Edge on macOS/Linux only where CI images have it;
  - two headless Edge probe processes (temporary `edge-probe-*` profiles) were left running locally; stopping them was
    not permitted from the session.
- B2.4:
  - the same link from desktop and CLI still creates two jobs (different keys);
  - no "open the app" link in the extension;
  - reboot behaviour is not documented (F01).
- B2.3:
  - Cancel leaves the `.downova-part` file (nothing is ever deleted);
  - CORE-10 search, bulk clear, undo and retention settings are not built;
  - no `dm remove` yet;
  - announcements follow polling (about 1 s);
  - no contrast audit;
  - R01 on windows-latest still unproven.
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
1. **Finish B2.6 evidence:**
   - commit and push the Linux harness repair (scripts and docs only; only when the user asks);
   - read #24's row annotations for each OS
     (`https://api.github.com/repos/DevSagar007/Downova/check-runs/<job-id>/annotations`);
   - Linux R3 must pass, or the failure must name the DevTools phase, browser stderr and exit; this also settles
     whether Edge on Linux honours the isolated config home;
   - record every row in the [B2.6 matrix](docs/reports/b2.6-acceptance.md#3-evidence-matrix).
2. **Human acceptance H1–H11** on each reachable OS with
   [the procedure](docs/reports/b2.6-acceptance.md#2-hands-on-acceptance-procedure-human); record the results and the
   tray checks.
3. **Then B2.7: CORE-10 search and Clear finished**, with the acceptance criteria in
   [the report](docs/reports/b2.6-acceptance.md#6-next-increment).
