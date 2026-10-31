# PROGRESS — handoff

_Last updated: 2026-10-31 (Windows 11 Pro 10.0.26300 x64, Node 26.7.0, pnpm 12.14.0 via npx, rustc 1.99.1, Chrome 156.0.8078.49, Edge 156.0.4314.50)_

## Current milestone
**Batch B2.12–B2.14 in progress (started 2026-10-31, uncommitted):**
- **B2.12** history retention (CORE-10);
- **B2.13** opt-in automatic browser capture with filters and fallback (BRW-03/04/06, E02; BRW-10 boundary);
- **B2.14** focused accessibility checks.

Report: [docs/reports/b2.12-b2.14-batch.md](docs/reports/b2.12-b2.14-batch.md). New ADRs: 0018 (retention), 0019
(automatic capture).

**Baseline `c8652c8`** (B2.8–B2.11 docs), in sync with DevSagar007/main:
- [CI #38](https://github.com/DevSagar007/Downova/actions/runs/41173386488) green on all 4 runners.
- [Package smoke #28](https://github.com/DevSagar007/Downova/actions/runs/41173386476) green on all 3.
- Linux job 124455853810 passed K1–K7 and the real Edge hand-off. Edge answered \`Browser.getVersion\` after
  20,071 ms, which produced the late-start note within the 60 s hard bound. The cause of the original intermittent
  #25 failure is **not** proven.
- Human right-click, tray, logout, sleep and reboot: not run.

**Plan** (shared changes first: migration 7, one contract revision, one extension storage model):
1. **B2.12 retention (service-owned)**
   - Model:
     - \`jobs.finished_at_ms\` is stamped once, by storage, at the first terminal write. It never changes after.
     - Legacy terminal rows count from the upgrade time (conservative).
     - The policy is a separate setting (\`history.v1.get/set\`, default keep forever; 7/30/90/365 days), so older
       clients saving \`settings.v1.set\` cannot reset it.
   - \`history.v1.preview\` gives the exact count before applying.
   - \`history.v1.clean\` cleans now, in bounded batches.
   - Sweeps run at startup and every 60 s in batches of 100, through the durable removal and tombstones.
   - Only terminal jobs without a finalization entry expire. No file is touched and no backend discard happens.
   - Removal events reach open clients.
   - Desktop: Options → History. CLI: \`dm history\`.
2. **B2.13 automatic capture (extension)**
   - Permissions:
     - optional \`downloads\` + \`webRequest\` (non-blocking, never \`webRequestBlocking\`);
     - per-site optional host permissions requested from user actions.
   - Capture happens only for sites the user allowed. Every one of these must hold:
     - non-blocking \`webRequest\` evidence correlated to the download shows a single **GET**;
     - the browser sent **no Cookie or Authorization**;
     - status 200;
     - every redirect hop is allowed;
     - the item is http(s), not incognito, browser-safe, and passes the filters.

     Anything else stays in the browser with a reason.
   - Capture record (storage.local) and idempotency key are persisted before the add. Bounded concurrency and
     records; one native job per browser download across duplicate events, worker restarts and lost
     acknowledgements.
   - The browser download is cancelled only after a matching acceptance and a recheck. Browser history is never
     erased and browser files are never removed.
   - Options page (enable, permission/service status, sites/exclusions, MIME/extension/min-size filters, recent
     activity, en/bn).
   - The context-menu hand-off is unchanged.
   - **BRW-10:** incognito stays \`not_allowed\` unless an end-to-end ephemeral policy exists (it does not; the gap is
     documented).
3. **B2.14 accessibility:** keyboard/labels/focus, themes, forced colors, reduced motion, contrast, narrow/200%
   zoom, long Bangla/RTL names, throttled announcements for the new and affected desktop and extension UI.

**Acceptance checks:**
- service/storage tests for retention boundaries, legacy rows, clock changes, storage failure, restart, events,
  idempotency replay and byte-exact files;
- extension unit tests for filters, permissions, duplicate events, worker restarts, host failure modes, races,
  browser-owned categories and redaction;
- real Chrome/Edge download-event tests (supported GET → host → service → SHA-256 and browser cancel only after
  acceptance; POST, blob/data, cookie and uncertain cases stay in the browser; restart or same-key retry → one job);
- packaged smoke additions for retention and capture;
- \`pnpm verify\`.

**Checkpoint:** see "Batch B2.12–B2.14 progress" below; every phase updates it.

### Batch B2.12–B2.14 progress
- **B2.12 retention — done (service, storage, contract, shell, UI, CLI; Windows 11):**
  - migration 7 (`finished_at_ms`, stamped once by storage; legacy finished rows count from the upgrade);
  - `history.v1.get/set/preview/clean` (separate from `settings.v1`), with schema, TypeScript and 4 valid + 2
    invalid examples; `retention_days` must be present (`null` = forever);
  - service sweeps: startup, then every 60 s, 100 per batch, oldest first, through the shared durable `tombstone`;
    publishes `job_removed`;
  - debug-only `DOWNOVA_TEST_NOW_OFFSET_MS` (release builds read 0);
  - Options → History (exact count before removal; files stay); `dm history`, `history-set`, `history-preview`,
    `history-clean`.
- **Checks:**

  | Check | Result |
  |---|---|
  | dm-storage | 31 passed |
  | dm-domain | 29 + contract 3 passed |
  | `dm-service --lib` | 99 passed (9 retention) |
  | `batch_process history_retention_…` | passed (store refusal kept the job, restart removed it, file byte-exact, same-key replay) |
  | CLI real service | passed |
  | desktop `history-retention.test.tsx` | 6 passed |
  | clippy | clean |

  A03 failed once while clippy compiled in parallel, then passed 3/3 alone; it is load-sensitive.
- **Next:** B2.13 capture (extension), then packaged smoke L (retention) and capture rows, B2.14 accessibility,
  docs.

### Previous batch (B2.8–B2.11)
**Batch B2.8–B2.11: implemented and verified locally on Windows 11 (2026-10-31); committed as `75a3a59` + `4d6f1bf` (this note in the next commit), pushed to DevSagar007/Downova `main`; CI #36 and package smoke #26 green on every runner.** Report:
[docs/reports/b2.8-b2.11-batch.md](docs/reports/b2.8-b2.11-batch.md).

Baseline `cf79d8a` (B2.7, pushed):
- [CI #35](https://github.com/DevSagar007/Downova/actions/runs/40673194566) is **green on all 4 runners**.
- [Package smoke #25](https://github.com/DevSagar007/Downova/actions/runs/40673194550): windows and macOS
  **passed**; **linux-x86_64 failed** (job 122764904289, from its public row annotation). 58 rows passed, 1 note
  and 2 failed:
  - Chrome 155 R3, R4 and D2 passed;
  - **Edge 155.0.4283.45** had no `Browser.getVersion` answer in 20 s, with empty stderr.

  Edge passed on Linux in #24 with the same harness, so the failure is **intermittent**. The cause is not
  confirmed: a slow cold start and a hang are both possible.

**Batch plan.** One schema migration (6) and one contract revision are shared by all phases:
1. **A. Baseline:**
   - Linux Edge diagnostics: a soft (20 s) and hard (60 s) bound, launcher, process tree, `/proc` fds, profile and
     `--enable-logging=stderr`; a late answer is a separate note row;
   - stale docs.
2. **B2.8 (CORE-07, SEC-03):**
   - `downloads.v1.check` and the add `on_duplicate` policy (omitted = allow);
   - destination rules per file-type category (`settings.v1.get/set`);
   - an ask-on-collision policy (`name_conflict` blocker, `downloads.v1.confirm_name`);
   - an SEC-03 guard.
3. **B2.9 (ARCH-08):** `events.v1.subscribe` with a snapshot watermark and `events.v1.event` notifications;
   bounded per-subscriber buffers (overflow → `snapshot_required`, then the stream ends); the shell forwards events
   to the UI; polling only as an explicit fallback for an older service.
4. **B2.10 (ARCH-09):** bounded attach-only reconnection after a service restart (never starts the service; Exit
   and stop ends it); native-host disconnect before and after the acknowledgement; wake detection; the lifecycle
   evidence matrix.
5. **B2.11 (CORE-05):**
   - persistent queues (`main` default), priority and manual order;
   - per-queue limits under the global limit;
   - Start/Stop queue through the `queue_stopped` blocker;
   - `queues.v1.*`, `downloads.v1.set_queue/set_priority/move`;
   - UI in the existing Queues panel and menus.

**Checkpoint (service side done, Windows 11):**
- schema 6 and the contract revision (31 valid and 15 invalid examples; Rust round-trip passed);
- policy module in `dm-domain`;
- core features: duplicates, rules, ask-on-collision, name race, queues, priority, limits, stop/start, events
  hub, subscribe streaming, wake re-evaluation, bounded shutdown checkpointing;
- the SEC-03 structural guard;
- tests passed: `cargo test -p dm-service --lib` (90), `--test batch_process` (3, real service),
  `--test sec03_no_execution` (2), `-p dm-storage` (28), `-p dm-domain`, and the host disconnect test (ignored,
  run by verify).

**Status (2026-10-31): every phase done.**
- **What exists:**
  - service, storage (migration 6), domain policy, contract (with examples);
  - Tauri shell (stream forwarding, attach-only probe);
  - TypeScript client and provider (event stream, explicit polling fallback, bounded reconnection, no
    reconnection after Exit);
  - UI: duplicate prompt, category and queue on Add URL, Options → Downloads, name-conflict prompt, service queues
    in the panel, Start/Stop queue, Manage queues, Queue and priority, Move items and Alt+↑/↓;
  - CLI flags and queue commands;
  - packaged smoke K1–K7;
  - Linux Edge diagnostics (soft and hard bound, evidence file, note row);
  - ADRs 0015–0017 and docs.
- **Checks (Windows 11):**

  | Check | Result |
  |---|---|
  | `cargo test --workspace --no-fail-fast` | 389 passed |
  | host disconnect test | passed |
  | CLI real-service test | passed |
  | UI vitest | 107 passed |
  | packaged smoke run 2 | 84 passed, 0 failed (run 1: K3 failed on a harness race, fixed) |
  | `pnpm verify:strict` | PASSED 20/20, 0 unavailable (2026-10-31; after fixing the capability JSON format and three redundant clippy patterns left by the 2026-10-28 session — the first run had 18 passed, 2 failed) |

- **F04** was updated to the ADR 0015 contract: a placement race completes under the next free name and the other
  file stays untouched. Before the update it still asserted the pre-B2.8 failure.
- **Git:** `origin` pointed at the fork teamtdevsfront/Downova; on 2026-10-31 it was reset to
  DevSagar007/Downova at the user's request.
- **Linux Edge:** #25's failure (Edge 155, no `Browser.getVersion` answer) did not recur in #26: Edge 156.0.4314.42 answered within the 20 s soft bound, so no diagnostics were captured. The cause stays **unconfirmed**; the evidence collector stays in place for a recurrence.
- **Not run:** real OS logout/sleep/reboot, H1–H11, tray clicks, a real right-click.

**Still open regardless of this batch:**
- human acceptance H1–H11 and tray clicks (not run);
- CORE-10 retention;
- automatic capture (BRW-04/06/10, E02);
- release requirements.

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
| B2.6 Linux harness repair + docs | `61acd6d` | [CI #34](https://github.com/DevSagar007/Downova/actions/runs/40660812389): **green on all 4 runners**; [package smoke #24](https://github.com/DevSagar007/Downova/actions/runs/40660812501): **59 + 1 note on Windows and macOS, 60 + 1 on Linux**, R3 Chrome/Edge, R4 and D2 all passed |
| B2.7 history search and Clear finished | `cf79d8a` | [CI #35](https://github.com/DevSagar007/Downova/actions/runs/40673194566): **green on all 4 runners**; [package smoke #25](https://github.com/DevSagar007/Downova/actions/runs/40673194550): windows and macOS green, **linux-x86_64 failed** (Edge 155 `Browser.getVersion` timeout; Chrome passed) |
| B2.8–B2.11 batch | `75a3a59`, `4d6f1bf`, `7990a7e` (docs) | [CI #36](https://github.com/DevSagar007/Downova/actions/runs/41170500501) (`4d6f1bf`): **green on all 4 runners**; [package smoke #26](https://github.com/DevSagar007/Downova/actions/runs/41170500457): **green on all 3** — windows 69 + 1 note, macOS 69 + 1 note, linux 70 + 1 note (the expected aria2c note), K1–K7 passed everywhere, real Chrome and Edge hand-off passed on every OS (Linux: Edge 156.0.4314.42, Chrome 156.0.8078.47; no slow-start note). #37/#27 for the docs commit `7990a7e` were still running when this was written |

CI evidence is the public job conclusion (and the failure annotation `scripts/verify.mjs` emits) of each run; full job logs need GitHub authentication.

## Backends (ADR 0005)
`dm-service` defaults to the built-in `dm-backend-http` (Http, Https, ResumeStrongValidator, SegmentedHttp supported); aria2 only with `--backend aria2` (Http only) and never bundled while REL-05 is open. No fallback between backends.

## Completed (summary)
- Foundation; ADRs 0001–0005; CI on 4 OSes; aria2 adapter (hardened); service (user-only IPC, capability gates, service-owned retry); `dm` CLI.
- `dm-backend-http`: HTTP/HTTPS with platform trust, manual redirects, origin-scoped credentials, identity encoding, validated pause/resume, validated segmentation.
- Reports: [aria2 B0.4](docs/reports/b0.4-http-correctness.md), [dm-backend-http](docs/reports/b0.4.2-http-backend.md), [B0.5 packaging](docs/reports/b0.5-packaging-smoke.md).

### This session — B2.7 ([report](docs/reports/b2.7-history-search-clear.md))
- **Search** (`features/downloads/search.ts`, `DownloadSearch.tsx`/`.css`):
  - NFKC, case-insensitive, trimmed matching on `display_name` and source hostname only; malformed sources match no
    hostname;
  - the full URL, query, fragment and credentials are never matched, shown or announced;
  - combines with the category filter and stable sort; a polite live region announces the throttled match count;
  - Ctrl/Cmd+F focuses unless a dialog is open; Escape clears while focused;
  - a selected row hidden by the query keeps its ID but exposes no commands; clearing restores it;
  - the table distinguishes loading, no matches and genuine empty/disconnected states;
  - new token `--search-max-width: 24rem`.
- **Clear finished** (`features/downloads/clear.ts`, `ClearFinishedDialog.tsx`, File menu):
  - targets only completed, failed and cancelled jobs, frozen when the dialog opens;
  - bounded concurrency 3, duplicate confirmation ignored, one row removed per acknowledgement;
  - partial failures keep those rows and list the name, error kind and offered action;
  - no file is deleted and unfinished/paused jobs are never sent;
  - disabled with `clear.none`, `offline` or `busy` reasons.
- **Service fix:** `Core::remove` refused a terminal failed job that still had `attempt: Some`. It now rejects only a
  non-terminal job or `finalizing.is_some()`, releases a terminal failed attempt with `backend.remove(&handle)`
  (keeping files), clears the attempt and persists the tombstone. The core removal test covers this.
- **Tests:** `search.test.ts` (5), `clear.test.ts` (5), `history-clear.test.tsx` (9); desktop 10 files / 84 tests;
  extension 26. `cargo test --workspace` 65 core + 49 process dm-service tests (aria2-only tests ignored by design);
  clippy clean.
- **Packaged smoke:** new J section (mixed finished and unfinished jobs, service restart). Rebuilt
  `windows-x86_64-smoke-test` from the working tree; **63/63 passed** including J1–J3, R3 Chrome 156.0.8078.49 /
  Edge 156.0.4314.50, R4, D2/D2b. MSI `fe0ec94d…`, setup `cab95ca2…`.
- **Docs:** `design.md` (tokens, components, §5/§6/§10), this report, requirements CORE-10 note, traceability,
  testing, roadmap. No commit or push.
- **Not run:** Linux/macOS for B2.7; no human acceptance.

### Earlier — B2.6 ([report](docs/reports/b2.6-acceptance.md))
- **Baseline `1d87c0b`, then `61acd6d`:**
  - CI #33 and package smoke #23: Windows and macOS passed (56 + 1 note each, including R3 Chrome/Edge, R4, D2);
    Linux failed at R3 Chrome/Edge, R4 and D2 (4 rows). The public row annotations were read.
  - CI #34 on `61acd6d`: **green on all four runners**.
  - Package smoke #24 on `61acd6d`: **green on Windows, macOS and Linux** (59 rows + 1 note on Windows/macOS, 60 + 1 on Linux); R3
    Chrome/Edge, R4 and D2 passed on all three. Linux now passes with the isolated config home.
- **Linux failure and cause:** both browsers timed out before the extension loaded (`Extensions.loadUnpacked: no
  answer from the browser in 60 s`); R4 therefore had no job and D2 had no hand-off file. Cause confirmed in
  Chromium source: Chrome 136+ refuses the DevTools pipe when `--user-data-dir` is the default
  (`IsUsingDefaultDataDirectory`), and the Linux product manifest is written into exactly that directory
  (`$XDG_CONFIG_HOME/<browser>`). macOS's default is the real OS home (not the isolated `HOME`) and Windows uses a
  temporary profile, which is why both passed. Native messaging in the product is unaffected.
- **Repair (scripts only, committed in `61acd6d`):**
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
- **Cross-platform evidence:** package smoke #24 passed R3 Chrome/Edge, R4 and D2 on Windows, macOS and Linux, so
  Edge's use of the isolated config home on Linux is confirmed by the run. CI #34 is green on four runners.
- **Still open:** H1–H11 by hand and manual tray clicks.
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
| Public GitHub API: CI #34 and package smoke #24 job conclusions | CI green on 4; #24 passed 59 + 1 note on Windows/macOS and 60 + 1 on Linux, including R3, R4 and D2 |
| `node scripts/verify.mjs` (full) | **PASSED 20/20**, 0 failed, 0 unavailable; desktop 10 files / 84 tests, extension 3 files / 26 tests, fmt, clippy, workspace tests, native-host e2e and all aria2 groups |
| `node scripts/verify.mjs --only=structure,format,lint,typecheck,test` | PASSED 9/9 (before the doc-only changes) |
| `node scripts/verify.mjs --only=rust` | PASSED 4/4: fmt, clippy `-D warnings`, workspace tests, native-host e2e |
| `node scripts/verify.mjs --only=format,structure` (after the final doc edits) | PASSED 3/3 |
| `cargo test -p dm-service --locked` | PASSED; 65 core tests including the failed-job removal regression, 49 process tests, 5 aria2-only tests ignored |
| `node scripts/package.mjs --smoke` | PASSED; NSIS `cab95ca2…`, MSI `fe0ec94d…`, built from `61acd6d` + working tree |
| `node scripts/smoke/packaged-smoke.mjs --package-dir dist-packages/windows-x86_64-smoke-test --out smoke-results/b27-windows` | **PASSED 63/63** incl. J1–J3, R3 Chrome 156.0.8078.49 / Edge 156.0.4314.50, R4, D2, D2b |
| `node scripts/e2e/chrome-handoff.mjs --kind chrome --out smoke-results/b26-repair-chrome` (earlier) | PASSED steps 0–5 and unregister; stderr diagnostics written |

No Linux or macOS B2.7 run exists; that needs a pushed CI/package-smoke run. No human acceptance case was run.

## Honest limitations
- B2.7:
  - no Linux or macOS run; CI/package smoke needs a pushed commit, which was not requested;
  - search is local to the loaded snapshot, with no server-side history query or pagination;
  - the match-count announcement follows polling and is throttled;
  - no retention policy, undo or "delete file" action (CORE-10 stays `in-progress`);
  - the service removal fix is proven on Windows by core and packaged smoke only;
  - human H1–H11 and tray checks are not run.
- B2.6:
  - The Linux failure cause was confirmed from Chromium source; #24 now records the repaired run passing on Linux,
    so the isolated config home is verified there. The human H1–H11 flow is still not run on any OS.
  - CI runs real Edge only on Windows and macOS, not Ubuntu; the Linux rotation is Chrome/Edge in packaged smoke #24.
  - The two headless Edge probe processes from B2.5 may still be running locally.
- B2.5:
  - macOS and Linux registration, browser hand-off and D0 passed in #24 (Linux after the B2.6 harness repair);
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
- B2.3 (state at that increment; later resolved where noted):
  - Cancel leaves the `.downova-part` file (nothing is ever deleted);
  - CORE-10 search and bulk clear were built in B2.7; undo and retention settings are still missing;
  - `dm remove` was added in B2.4;
  - announcements follow polling (about 1 s);
  - no contrast audit;
  - R01 on windows-latest was unproven then and passed in CI #30.
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
1. **CI for the batch is green** (#36, #26). Confirm #37/#27 for the docs commit `7990a7e`. If Linux Edge fails again, read its `browser-diagnostics.json` (launcher, process tree, pipe fds, profile, exit) before changing the harness.
2. **Human acceptance** on each reachable OS:
   - H1–H11 and tray clicks;
   - a real browser right-click;
   - the ARCH-09 OS cases (logout, sleep, reboot) per docs/architecture.md §2.
3. **Next implementation batch:** CORE-10 retention, then automatic capture with the fallback matrix (BRW-04/06/10,
   E02). Scheduler, speed limits, engine replacement and adaptive segmentation stay out.
