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
| `rust` group, HTTP backend | `cargo test --workspace` includes `crates/backend-http/tests/http_backend.rs` (24 acceptance cases against the shared `dm-test-fixtures` server, SHA-256 oracle), `crates/backend-http/tests/validated_resume.rs` (15 pause/resume cases: changed resources, ignored/honoured If-Range, weak/missing ETag, wrong ranges, 200/416, encoding, modified partials, cancel during resume) and `crates/service/tests/http_backend_process.rs` (real `dm-service --backend http`): no external binary needed |
| Packaged-app smoke (B0.5) | `node scripts/package.mjs --smoke` then `node scripts/smoke/packaged-smoke.mjs --package-dir dist-packages/<label>`: installs or unpacks the real package and uses only its binaries (service hello/list/download with SHA-256, desktop window + service handshake, UI closed during a transfer, uninstall). CI: `.github/workflows/package-smoke.yml` (Windows x86_64, macOS arm64, Linux x86_64 on Ubuntu 22.04). Not part of `pnpm verify` (minutes-long release builds) |
| `aria2` group | (1) `cargo test -p dm-backend-aria2 --test fixtures -- --ignored`: real aria2c against the controlled fixture server; (2) `cargo test -p dm-service --test process -- --ignored`: the real `dm-service` process (single instance, hello, capability gate, shutdown); (2b) `cargo test -p dm-backend-aria2 --test http_correctness -- --ignored`: the B0.4 matrix plus B0.4.1 TLS-backend and If-Match cases (38 tests); (3) `cargo test -p dm-cli --test real_service -- --ignored`: the `dm` binary against the real service (unknown job, https refused with no job or file, **http add completed with a SHA-256 match**, service outlives the CLI). The service process tests also cover a real download and kills of aria2 and the service mid-transfer. UNAVAILABLE without cargo or an aria2c binary |
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
`cargo test -p dm-cli` covers argument parsing, help and version, default and custom data directories, missing and stale endpoints, timeouts, the API-version handshake, command→method mapping, human and JSON output, exit codes, retries with the same idempotency key, and URL redaction. It runs against an **isolated IPC test peer** (`crates/cli/tests/support`): a scripted endpoint on the real transport. That gives controlled success responses without any production capability bypass. B2.4 adds `remove` and `shutdown` (mode params, a verified stop versus an acknowledged-but-running service, usage errors), and the ignored real-service test `cli_remove_and_paused_shutdown_with_http_backend` (removal keeps the file and survives a restart; a paused shutdown restarts paused and resumes byte-exact).

## 1. HTTP correctness (COR-01..05, CORE-02, E03, E04)
- **Fixtures:** a local, deterministic HTTP/HTTPS server (B0.4) in `crates/backend-aria2/tests/support/` (scriptable routes plus a request log; TLS via a test CA). It covers:
  - range honoured (206), range ignored (200), wrong Content-Range, short or truncated body and 416;
  - unknown length, zero-byte files and Content-Encoding changes;
  - strong, weak and missing ETag;
  - same-length changed content, 429/503 with Retry-After, and 401/403/410;
  - redirects with and without cross-origin hops, and Content-Disposition `filename*` with Bangla names and reserved names.
- **Oracle:** the SHA-256 of the completed file equals the expected value. Any mismatch in a file reported `completed` blocks release.
- **Report:** a per-backend capability and correctness matrix, which feeds the ADR 0003 go/no-go decision. aria2 1.37.0: [B0.4 report](reports/b0.4-http-correctness.md). Each case is classified safe success, safe failure, unsafe or not tested; engine-level unsafe outcomes are asserted as unsafe so a behaviour change fails loudly.

- **File naming (B1.6, E04):**
  - `dm-service` `naming::tests`: the parser, sanitizer, limits on both measures and collisions.
  - Coordinator naming tests with the fake backend.
  - `dm-backend-http` `tests/naming.rs` NB01–NB03: metadata after redirects, oversized headers, a refused name.
  - Process NM01–NM04: real service, Bangla/emoji/traversal/reserved/long names, collisions, the freeze across pause, kill and restart, journal recovery.
  - The oracle is the selected name plus the SHA-256 of its contents, and that pre-existing files are unchanged.
  - Collisions with existing entries follow the file system, so a case-variant test measures the temporary folder first
    (B1.7). The case-sensitive branch runs on Linux CI, or locally in an NTFS folder marked with
    `fsutil file setCaseSensitiveInfo <dir> enable` and `TMP`/`TEMP` pointed at it.

- **Error classification and copy (B1.7, CORE-11):**
  - `ErrorKind::ALL` / `NextAction::ALL` equal the schema enums (`crates/domain/tests/contract_examples.rs`).
  - The CLI and the UI catalogues (`crates/cli/src/output.rs`, `apps/desktop/src/features/downloads/errors.test.ts`)
    must cover every kind and action in English (and Bangla for the UI). They must offer only implemented actions and
    never derive text from `detail`; a synthetic signed URL in `detail` must not appear in any output.
  - Process fixtures E01 (401/407/403/404/410), E03 (no strong validator), E04 (changed resource with If-Range
    ignored) and E06 (a folder this user cannot write to, restricted by the OS) assert `kind`, `retryable` and
    `next_action`. DK01–DK11 assert the disk kinds' actions.
  - A classification the server does not state (403, a missing destination) is documented as a heuristic in the
    [B1.7 report](reports/b1.7-error-catalogue.md).

## 2. Resume and changed resources (COR-02, COR-06, ADV06)
- Pause and resume with a stable resource gives the correct hash.
- The resource changes while paused: the result is a safe restart or `REMOTE_CHANGED`, never mixed bytes.
- Link replacement with the same resource keeps progress. With a different resource of the same size, the user gets the restart-or-keep decision.
- **B2.16/B2.17 (ADR 0020), as implemented:**
  - `crates/backend-http/tests/replace_source.rs` S01–S05: a refreshed signature keeps progress and completes byte-exact (the continuation starts at the checkpoint); same-length changed content with a new or the same ETag; missing and weak validators, a changed length, a server ignoring Range; another path or an origin-changing redirect sharing the ETag is never "the same resource"; leftover partial data is released only when no attempt holds it.
  - `crates/service/src/core/tests/operations.rs` (14): Retry eligibility, one fresh attempt per operation, conflicts, attempt guard, stale callbacks, admission and stopped queues, retention after a retry, persistence across a restart, leftover release; Refresh link proofs, choices, explicit restart, failed restores.
  - `crates/service/tests/operations_process.rs` (3, real service + HTTP backend): kills after the retry release and after the replacement restore, lost acknowledgements, byte-exact results, an expired link refreshed.
  - Storage: `reopen_job` clears the finish time once and records the operation; `replace_source_kept` is atomic and refuses a stale attempt.
  - Packaged smoke M1–M4: publisher checksum match/mismatch, Retry, Refresh link (restart and kept progress), locate.
- A late event from a cancelled attempt does not change the job.

## 3. Crash recovery and disk failures (CORE-04, COR-08, ADV05)
- Kill the service, the engine, or both at random points (including during checkpoint and finalization), N ≥ 20 iterations. Each must end with the correct hash or an explicit failure.
- Disk full (a small loopback or tmpfs volume): the job stops with `DISK_FULL`, the partial file is kept, and no other file changes.
- Permission denied, removed volume and returning volume, a name collision, and a partial file deleted externally.
- Power-loss ordering is reasoned about and documented. Durability is never claimed beyond what the fault tests show.
- **Repetition (B1.7):** R01 (repeated hard kills during a single-connection and a segmented transfer) and R02 (a crash
  at each of the 5 finalization boundaries, repeated) read `DOWNOVA_KILL_REPS`, which defaults to 3 so plain
  `cargo test` stays short. Evidence runs use N = 20:
  `DOWNOVA_KILL_REPS=20 cargo test -p dm-service --test http_backend_process r0 -- --nocapture --test-threads=1`.
- **Exit and stop (B2.2):** service unit tests for `begin_shutdown` (durable pause, races, repeated requests, retry-waiting jobs) and shutdown params; process test X01 (the real service: acknowledgement, verified exit, paused restart with no requests, Resume byte-exact); packaged smoke H (tray path + real dialog → service gone → restart paused → `dm resume` → SHA-256).
- **Kill barrier (B2.2):** R01 kills only once every unfinished job has a `checkpoint`/`recovered` trace event for its current attempt; failures print the job's trace.
- **UI kill:** packaged smoke step E (`scripts/smoke/packaged-smoke.mjs`) forcibly terminates a desktop that started
  the service (Windows `taskkill /F` without `/T`, Unix `SIGKILL`) during a transfer. The service must survive and
  the file must match its SHA-256. Step C covers a graceful close.

## 4. Browser handoff and fallback (BRW-*)
- Matrix: browser (Chrome, Edge, then Firefox) × OS × install state (installed, not installed, host missing, service down, timeout).
- Expected results:
  - public GET is taken over only after acceptance;
  - when the host is missing or times out, the browser download continues;
  - POST, blob and data downloads stay in the browser;
  - a lost acknowledgement plus a retry gives one job (COR-09);
  - closing the browser does not stop the native job.
- Authenticated cases check that credentials do not leak across origins.
- B0.6 keeps three kinds of evidence apart (never report one as another):
  - **mocked extension tests:** `apps/extension` vitest;
  - **direct host-process tests:** `cargo test -p dm-browser-host`, plus `--ignored` host → real `dm-service` (part of `pnpm verify`, bounded at 15 min), plus packaged smoke A4/A5;
  - **real browser:** `node scripts/e2e/chrome-handoff.mjs [--kind chrome|edge]`. It uses an isolated profile, loads the e2e build of the extension over the DevTools pipe and uses a reversible registration. It covers identity, readiness, hand-off with SHA-256, lost acknowledgement plus browser restart → one job, and browser close during a transfer. The native menu click is simulated by calling the registered listener. CI runs it on windows-latest, macos-latest and ubuntu-24.04.
    - It first proves the transport with `Browser.getVersion`, so "DevTools did not start" is distinct from
      "the extension failed to load"; every DevTools call has a named, bounded wait.
    - That first answer has a 20 s soft and 60 s hard bound (B2.8, after Edge 155 on Linux gave no answer in 20 s
      with empty stderr in smoke #25).
      - At the soft bound and on giving up, `scripts/lib/browser-diagnostics.mjs` records the launcher, the
        process tree, `/proc` file descriptors (pipe ends), the profile state, the exit status and the stderr tail
        into `browser-diagnostics.json`. On Linux the browser logs to stderr.
      - A late answer is a separate **note** row with that evidence, never folded into the pass row. No answer
        is still a failure. Edge is never skipped and nothing is retried blindly.
      - `browser-diagnostics.test.mjs` covers the collector (part of `pnpm verify`).
    - Chrome 136+ refuses DevTools when `--user-data-dir` is the platform default. On Linux the product manifest lives
      in that directory, so in product-registration mode the browser **process** gets an isolated
      `XDG_CONFIG_HOME`/`CHROME_CONFIG_HOME` while `--user-data-dir` remains the product directory;
      `scripts/lib/browser-profile.mjs` decides this and `scripts/lib/browser-profile.test.mjs` pins it (part of
      `pnpm verify`). The harness never copies, links or creates registrations.
    - Browser stderr (bounded and path-redacted), exit code and signal are written to `browser-stderr.log` and
      `results.browser_diagnostics`; every launched browser is closed, or killed, in `finally`.
  - **product registration (B2.5):** registration ownership unit tests (`cargo test -p dm-browser-host`: repeated setup, missing/damaged/unreadable manifests, other installations, foreign hosts, upgrade and uninstall ordering; on Windows against a private `HKCUSoftwareDownova-Test-*` subtree that is deleted afterwards) and packaged smoke R/D2, which runs the real-browser test with `--product-registration` against the installed host. Registration checks, real-browser steps and the simulated menu click are recorded as separate rows.

- **B2.8–B2.11 batch (CORE-07, ARCH-08, ARCH-09, CORE-05, SEC-03):**
  - **Service:**
    - `core::tests::batch`: duplicates incl. concurrent refusing adds and same-key replays, fingerprint
      compatibility, rules fixed at acceptance, folder validation, ask-on-collision, the placement race,
      queues, priority, order, limits, stop/start with user intent, wake;
    - `server::stream_tests`: consistent start, whole-line framing, a slow subscriber cut off without
      blocking others.
  - **Real processes:**
    - `tests/batch_process.rs`: rules, duplicates and ask-on-collision with real files and SHA-256; the stream
      across a service restart (new instance); a stopped queue and its order across a restart;
    - F04 (a placement race takes the next free name; the other file stays byte-exact);
    - `sec03_no_execution.rs`;
    - the host disconnect test (`--ignored`, in verify);
    - the CLI real-service test (`--refuse-duplicate`, queue commands).
  - **UI:**
    - `batch-live.test.tsx`: stream ordering, gaps, new instances, `snapshot_required`, sequences beyond 2^53,
      removed jobs, explicit polling fallback, attach-only reconnection, no reconnection after Exit, the
      duplicate prompt and its race, uncertain retries, name confirmation, download settings, queues;
    - `service-client.test.ts`: subscription buffering and filtering, optional add fields.
  - **Packaged smoke K1–K7** (real UI through the smoke bridge, real service): the window subscribed (service
    trace `subscribed`), duplicate prompt → nothing created, Download again → a second byte-exact file, Stop/Start
    queue from the menu with a manual pause preserved, Move to top, restart restores queue, order and pause.

- **B2.12–B2.14 batch (CORE-10, BRW-03/04/06/10, E02, UI-02):**
  - **Retention:**
    - service `core::tests::retention` (sweeps at controlled times);
    - storage (stamp once, legacy rows, setting apart);
    - process `batch_process history_retention_…` with the debug-only `DOWNOVA_TEST_NOW_OFFSET_MS` clock offset;
    - CLI real service;
    - desktop `history-retention.test.tsx`.
    - Packaged smoke L1/L2 runs on release binaries, which have no test clock: policy persistence, preview/clean
      safety, files.
  - **Capture unit tests (`apps/extension/src/capture/capture.test.ts`):** policy precedence and domain
    boundaries, evidence folding and correlation, and the state machine against a scripted browser and host
    (duplicate events, unconfirmed and lost acknowledgements, a stopped worker at each boundary, browser
    finishing or user cancelling first, capture disabled or permission revoked mid-hand-off, bounded concurrency,
    redaction).
  - **Capture with a real browser** (`scripts/e2e/chrome-handoff.mjs`, steps C0–C5):
    - downloads start the way a user starts them: a fixture page whose link is clicked with a user gesture;
    - the extension's capture runs on the browser's real `downloads` and `webRequest` events;
    - C1: a public GET is accepted by Downova, then the browser copy is cancelled; SHA-256;
    - C2: a duplicate event is ignored, and the worker is stopped (`ServiceWorker.stopAllWorkers`) and woken → one
      job;
    - C3: POST; C4: blob:; C5: cookies — each stays in the browser and completes there.
    - Observed: Edge 156 ends a download opened by automation in a new tab without a user gesture as HTTP 204 with
      no download item, hence the clicked link.
    - The e2e build pre-grants the optional permissions for 127.0.0.1/localhost, because automation cannot answer
      the permission prompt.
    - The packaged smoke runs the same steps with `--capture` against the installed host.
  - **Contrast:** `apps/desktop/src/ui/contrast.test.ts` computes WCAG ratios from the token files (desktop and
    extension, light and dark). Automated only: human screen-reader and visual audits are separate evidence.

- **Process-test hygiene (B1.1 repair):**
  - Every test that spawns `dm-service` holds it in a `Service` guard. On drop (success, panic or timeout) the guard kills and reaps that child if it is still running.
  - Tests that build `dm-service` with cargo build it once per test binary, before spawning it.
  - A leaked service locks `target/debug/dm-service.exe` on Windows, and `cargo test` and `cargo build` produce different binaries there, so a leak makes the next build fail with "Access is denied".

- **Crash recovery (B1.2):**
  - `crates/backend-http/tests/durable_resume.rs` (R01–R09) restores a checkpoint exported by one backend instance into another, after the first wrote past it.
  - `crates/service/tests/http_backend_process.rs` K01–K07 hard-kill the real `dm-service` and restart it. They synchronise on `DOWNOVA_CHECKPOINT_TRACE` (committed checkpoints and recovery outcomes) and on the partial file's length, never on sleeps alone.
  - Each case checks the fixture's request log for the continuation `Range` and judges the result by SHA-256.
  - These tests run in plain `cargo test`, so CI runs them on all four runners.
  - Process kills only: no power-loss claim.

- **Finalization crash windows (B1.3):**
  - `crates/service/tests/http_backend_process.rs` F01–F08 start `dm-service` with `DOWNOVA_FAULT=<point>`, which exists only in debug builds and makes the service `abort()` at an exact finalization boundary. Then they restart it and assert the exact SHA-256, the persisted state, preserved foreign files and a single download.
  - `hold_before_place` plus a `.release` file is the barrier for concurrent-destination and cancel tests.
  - `fail_journal_once` injects a journal write failure.

- **Disk budget (B1.4):** tests never fill a real disk.
  - Free space comes from `DOWNOVA_TEST_FREE_SPACE_FILE` (debug builds; re-read on every query).
  - `ENOSPC` is injected once per process with `DOWNOVA_FAULT=enospc_write|enospc_sync|enospc_copy`, and the copy fallback is forced with `no_hard_links`.
  - Backend: `crates/backend-http/tests/disk_budget.rs` (injected `SpaceGuard`). Service: `http_backend_process.rs` DK01–DK11.

- **Connection budget and admission (B1.5):**
  - The fixture server records each TCP connection's open and close time. `Server::max_overlap(100 ms)` is the oracle: a dropped connection is noticed by the server only at its next I/O, so connections closing within 100 ms after another opens are not counted. The raw maximum is reported as well.
  - Backend: `tests/connection_budget.rs` C01–C04. Service: `http_backend_process.rs` A01–A04. Coordinator: admission unit tests.

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
- **Live service state (B2.1):** `apps/desktop/src/app/downloads-live.test.tsx` drives the window with a scriptable
  `FakeServiceClient` (`src/test/fake-service.ts`). It covers:
  - loading and populated lists, and renamed files keeping their selection;
  - Add URL validation, a single submission under repeated clicks, and an uncertain add retried only on request with the
    same idempotency key;
  - selection by mouse and arrow keys; Stop and Resume from the toolbar and the File menu;
  - waiting versus paused versus failed, and command failures explained with the CORE-11 catalogue;
  - a lost service (last rows kept, polling stopped, Reconnect) and an incompatible service;
  - a list answer that started before an acknowledged Stop being dropped; no polling after unmount; Bangla.

  `src/services/service-client.test.ts` checks the Tauri client's command names and arguments (including cancel) and
  its error mapping with a mocked `invoke`. These tests prove UI behaviour against the client interface, not the shell.
- **Status, cancel and remove (B2.3):** `src/app/status-cancel-remove.test.tsx`: status kinds and exact BigInt progress (incl. counts above 2^53, unknown and zero totals); progress bar labelling; Cancel kept/confirmed once/refused/not offered while finishing; late list answers cannot revive a cancelled row or restore a removed one; Remove only for finished rows, focus and selection move, announcement, failure keeps the row; keyboard and Bangla. Service: storage removal tests, core removal and cancel-ownership tests, process RM01 (durable removal, file kept, replay) and RM02 (injected store failure).
- **History search and Clear finished (B2.7):** `features/downloads/search.test.ts` covers NFKC/case folding, Bangla and RTL names, source hostname (trailing dot, malformed URL), blank queries and the guarantee that only the hostname, never the URL or query, is matched. `features/downloads/clear.test.ts` covers the finished scope (completed/failed/cancelled only), concurrency three, partial failure, an empty target list and an uncertain acknowledgement. `src/app/history-clear.test.tsx` covers filtering by name/source/category, no-match versus empty history, sorting within filtered rows, Ctrl/Cmd+F and Escape, a hidden selection keeping its ID but exposing no commands, the frozen target list (a job finishing after the dialog opened is not cleared), duplicate confirmation, partial-failure counts and per-row errors, service unavailable, a stale list answer unable to restore a removed row, and Clear being unavailable when nothing is finished. Service: `only_finished_jobs_are_removed_and_files_are_never_touched` also removes a terminal failed job that still owned an attempt.
- **Shell bridge (B2.1):** `cargo test -p dm-desktop` runs the five commands against a fake service on the real private
  IPC transport: canonical add params, argument refusal before sending, method mapping, structured service errors, and
  uncertain versus unavailable.
- **Automated, later:** an axe-core pass per view, and a 10k-row virtualized list performance test.
- **Manual, per beta:** keyboard-only essential workflow; NVDA (Windows), VoiceOver (macOS) and Orca (Linux) smoke tests; 200% zoom; forced colours; Bangla rendering; long and RTL filenames.

- **B2.16–B2.20:** `apps/desktop/src/app/retry-refresh-folders.test.tsx` (Retry, Refresh link, checksum input and presentation, Browse… cancel, Open containing folder by keyboard, Bangla); `apps/extension/src/popup.test.ts` (Bangla states, actions and accessible names, older records, static text). The native folder dialog, the file manager and screen readers remain human checks.

## 7. Packaging, updates and platform compatibility (REL-*, ADV09, BRW-07)
- CI matrix: windows-latest, macos-latest (arm64 and x64) and ubuntu-22.04/24.04. A combination counts as supported only with recorded passes (REL-02).
- For every OS: install, launch, uninstall and native-host registration plus repair.
- Packaged smoke (`scripts/smoke/packaged-smoke.mjs`):
  - C: graceful desktop close during a transfer;
  - E: forced desktop kill during a transfer;
  - F (B2.1): a smoke-only driver fills the real Add URL dialog in the packaged app, and the script checks that the job
    reached the service through `downloads_add`, partial progress through `downloads_list`, completion with the
    SHA-256, and the same job through `dm get`.
  - R (B2.5): isolated per-user location (Windows: LOCALAPPDATA for manifests, HKCU values saved/removed/restored; macOS/Linux: a temporary HOME); NSIS hook (Windows) and the desktop's first launch register Chrome and Edge; manifest name/type/path/origins/permissions and ownership; desktop and service ready; real Chrome and Edge (where installed) hand-off with only the product registration; the desktop's `downloads_list` shows progress, one job, SHA-256; repair of a missing, damaged or unreadable manifest and a missing registry value;
  - B2.6: on GitHub Actions the smoke and the real-browser test each publish **one annotation listing every row**
    (annotations are readable without sign-in, logs and artifacts are not); browser DevTools calls are bounded at
    60 s; a browser run killed at 300 s is recorded as such. Manual acceptance (human right-click and tray flow) uses
    `scripts/acceptance/fixture-server.mjs` and the procedure in `docs/reports/b2.6-acceptance.md`; all three scripts
    share `scripts/lib/fixture.mjs`.
    - When R3 Chrome produces no job, R4 is `unavailable` ("not run"), not a second failure; a failed R3 records the
      `e2e.log` and `browser-stderr.log` tails, exit code and signal.
  - D0/D2 (B2.5): macOS/Linux run the documented `dm-browser-host unregister` (no per-user uninstall hook); after uninstall only this installation's entries are gone, an unrelated manifest and registry key survive, and downloads and data remain.
    - D2 checks registration/data only. D2b checks the hand-off file's survival independently and is `unavailable`
      when no file was produced, so a missing hand-off is never reported as uninstall damage.

  - G (B2.2): the window is closed through Tauri (the same close request as its button) with the default hide-to-tray: the desktop and the transfer continue; a second launch exits and shows the first window;
  - H (B2.2): confirmed Exit and stop downloads, then restart paused, then Resume;
  - I (B2.3): Cancel download and Remove from list through the packaged UI (menus and dialogs); the completed file stays byte-exact and both jobs stay out of the list after a service restart.
  - J (B2.7): completed, cancelled and failed jobs coexist with a running and a paused job; Clear finished through the packaged UI (File menu and dialog) removes exactly the finished jobs across the active filter and search, leaves the running and paused jobs, keeps the completed file byte-exact, and the cleared rows stay out after a restart.

  Manual per OS before a beta: the title-bar close button, the tray icon and its menu items. On Linux the smoke runs under `dbus-run-session` (single instance needs a session bus).

  F proves the packaged production command path. It does not exercise selection, Stop or Resume, which the UI tests
  cover.
- Updates: a tampered artifact is rejected, a failed migration still gives a recoverable startup, and downgrades are blocked.
- Licensing: a notices bundle exists, aria2 source or an offer for it is included, and the SBOM is generated (B3/B4).

## 8. Privacy (SEC-01, E05)
A synthetic secret (`SYNTHETIC-SECRET-…`) is placed in a URL query, a cookie and an Authorization header. Then grep logs, events, the diagnostics export, the SQLite file and the aria2 session file. Any hit fails the test.

## 9. Benchmarks (REL-04)
Follow research §11: the same machine and network profile, no concurrent apps, at least 5 rotated runs with medians (at least 20 runs for p95), and full process-tree memory and CPU. Publish the scripts and raw results.

- **COR-08 checkpoint commit latency** (B1.7):
  `DOWNOVA_BENCH_SAMPLES=1000 cargo test -p dm-storage --release --test checkpoint_latency -- --ignored --nocapture`.
  - It times `Store::save_checkpoint` with the production settings (WAL, `synchronous=FULL`) on isolated temporary
    databases. `DOWNOVA_BENCH_DIR` selects the device.
  - It also prints a `synchronous=NORMAL` comparison for the same statements. Production never uses that mode.
  - Report the OS, the device, the sample count and the percentiles.

## Reporting
- For every completed increment, PROGRESS.md lists each command that was run, its result and the OS.
- Traceability evidence cells cite the test name and the run.
- If something could not run, say why and which OS or tool is needed.
