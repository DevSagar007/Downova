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
  - **Stabilization (2026-11-01):** packaged CLI calls whose service work needs the in-process fixture (Refresh link
    verification) go through `dmAsync`, never `spawnSync`, which freezes the fixture's event loop. Smoke #33 M3
    `kept={}` was that deadlock, reproduced: the sync call was killed at 60 s with no output; the async call answered
    `progress_kept` in 127 ms. M3 also refuses a same-length changed file and leaves the job unchanged.
  - **Packaged desktop UI (journey N):** the smoke bridge drives the real dialogs: Add URL with a SHA-256 (N1),
    File → Retry… pressed twice (N2: one `retried` trace event), and File → Refresh link… with Keep current, a
    compatible new signature, and an explicit Restart (N3a/N3b). Outcomes are judged through the service, its trace and
    SHA-256.
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
  - **Timeouts and lost acknowledgements (B2.33, rows X1–X8):** `scripts/e2e/chrome-handoff.mjs` points the
    browser's Downova manifest at a fault-injecting helper (`scripts/e2e/fault-host.mjs`, test only) that sits in
    front of the real host.
    - **Isolation:** the manifest is the run's own. The full run uses its temporary registration; the packaged smoke
      (`--fault-steps`) uses the installed host's manifest in its isolated LOCALAPPDATA or HOME. Only `path`
      changes, `allowed_origins` stays, and the file is restored byte for byte (X7).
    - **Rows:**
      - X1: the helper never answers. The add is uncertain after a 4 s acknowledgement timeout and no job exists;
        once the helper answers, the retry with the same key gives one job.
      - X2: the service accepts, then the helper exits before replying. The add is uncertain, never accepted; the
        retry with the same key returns that job.
      - X3: an acceptance under a different request id is uncertain.
      - X4: the real host against an empty data folder (no service; the host never starts one) gives a definite
        unavailable and no job.
      - X5: an unsupported link is rejected without starting a helper.
      - X8: the helper is not installed (the browser's manifest is missing while its registration remains): a
        definite unavailable with `host_missing`, no helper started, no job.
      - X6: automatic capture with a non-matching acknowledgement: the browser keeps and completes its download (never
        cancelled by Downova); every attempt uses one key, so the service holds one job.
    - **Evidence:** every helper launch (mode, request id, key, URL path, the real host's outcome and job, what was
      sent back) is kept in `fault-host.jsonl` beside the results.
    - **Difference from a normal installed build:** a `.cmd`/`sh` wrapper and Node stand in front of the
      installed host, and the extension is the e2e build. The host, service and browser are the real ones.
    - **Unit counterparts:** `handoff.test.ts` (lost or mismatched acknowledgements stay uncertain, retries keep
      the key, one job; mutation-checked by giving retries a new key) and `capture.test.ts`.
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
- **Event ordering and bounded failure (B2.24):** `src/services/event-ordering.test.tsx` reads the provider's
  committed state. It covers:
  - progress queued just before a pause, resume, retry or source-replacement acknowledgement;
  - a stale event delivered after an acknowledgement, corrected by the mutation's own event;
  - a removal with events queued, late events, and a later snapshot that still lists the job;
  - a superseded subscription's late events and endings;
  - a queued batch against a fresh snapshot (new instance or sequence gap);
  - hidden-window batching, then becoming visible, and the hidden interval;
  - unmounting with a batch queued (no timer, listener or render left);
  - five removal-and-reconnect cycles;
  - resubscription backoff after repeated stream endings;
  - an unreadable (oversized) list stopping in a reported state with no polling or reconnection.

  `src/app/virtualized-list.test.tsx` adds: removing a selected row scrolled out of view (its true neighbour, not
  the next rendered row, is selected and focused); a focused row removed by someone else (focus stays in the list);
  selection through sorting and filtering with matching row counts; hidden spacers and continuous row indexes.
  Each of these tests failed with its fix removed.
- **IPC framing (B2.24):** `crates/platform/src/ipc.rs` tests use a 64-byte limit:
  - a line exactly at the limit is read;
  - one byte more is the typed, non-retryable `CallError::TooLarge`;
  - a peer stopping inside a line is a lost connection;
  - a peer that never sends a newline is refused at the limit, never waited on.

  The 6 MiB response and the oversized request are covered by the B2.22 test.
- **Automated, later:** an axe-core pass per view.
- **Manual, per beta:** keyboard-only essential workflow; NVDA (Windows), VoiceOver (macOS) and Orca (Linux) smoke tests; 200% zoom; forced colours; Bangla rendering; long and RTL filenames.

- **B2.16–B2.20:** `apps/desktop/src/app/retry-refresh-folders.test.tsx` (Retry, Refresh link, checksum input and presentation, Browse… cancel, Open containing folder by keyboard, Bangla); `apps/extension/src/popup.test.ts` (Bangla states, actions and accessible names, older records, static text). The native folder dialog, the file manager and screen readers remain human checks.

- **Popup acceptance step 3b (real Chrome/Edge):**
  - It reads the hand-off row through stable hooks (`data-record`, `data-code`, `data-role`).
  - It asserts acceptance of this job's ID, with no claim that the transfer started or finished, and a redacted link
    (`?…`; the link carries a synthetic secret).
  - It then re-renders the page in Bangla with the e2e-only `downovaPopupE2E.language("bn")` hook and asserts the
    Bangla state, detail and job text.
- **Load-sensitive waits:** desktop vitest bounds Testing Library waits at 5 s and a test at 20 s, because one window test
  types through several dialogs. The defaults (1 s and 5 s) failed tests that pass alone while a release build ran.

## 7. Packaging, updates and platform compatibility (REL-*, ADV09, BRW-07)
- CI matrix: verify on windows-latest, macos-latest (aarch64), ubuntu-22.04 and ubuntu-24.04; package smoke for windows-x86_64, macos-aarch64 and linux-x86_64 (see setup, "Selected platform scope"). Windows x86 and macOS x86_64 are deferred. A combination counts as supported only with recorded passes (REL-02).
- For every OS: install, launch, uninstall and native-host registration plus repair.
- Packaged smoke (`scripts/smoke/packaged-smoke.mjs`):
  - C: graceful desktop close during a transfer;
  - E: forced desktop kill during a transfer;
  - F (B2.1): a smoke-only driver fills the real Add URL dialog in the packaged app, and the script checks that the job
    reached the service through `downloads_add`, partial progress through `downloads_list`, completion with the
    SHA-256, and the same job through `dm get`.
  - R (B2.5): isolated per-user location (Windows: LOCALAPPDATA for manifests, HKCU values saved/removed/restored; macOS/Linux: a temporary HOME); NSIS hook (Windows) and the desktop's first launch register Chrome and Edge; manifest name/type/path/origins/permissions and ownership; desktop and service ready; real Chrome and Edge (where installed) hand-off with only the product registration; the desktop's `downloads_list` shows progress, one job, SHA-256; repair of a missing, damaged or unreadable manifest and a missing registry value;
  - B2.6, B2.28: on GitHub Actions the smoke and the real-browser test publish a summary annotation (non-passed rows
    first and in full, passed rows as short IDs, under GitHub's ~4 KB cut) and one annotation per non-passed row with
    its detail (`scripts/lib/annotations.mjs`; before B2.28 the single annotation was cut after about 52 rows and hid
    late failures)
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
  - O1 (B2.23): notification decisions through the CLI and the service trace (no window): off by default, once per durable outcome, none for a cancel or while off, none after a restart.
  - P1/P2 (B2.22, B2.24): 2,000 seeded records in the packaged window (`list_probe`): bounded DOM, End/Home to offscreen rows with focus and selection, hidden spacers; then page zoom 100/150/200 % in light and dark with no page overflow, no blank band in the list (top and middle), **at least one fully visible row with a download selected** (`rows_visible ≥ 1`, B2.32) and focus kept, and a Bangla search over the whole dataset. Since B2.32 the probe (`probe_version: "b2.32"`) also records the geometry when no row is visible (`no_rows_visible`, the window's and main column's block heights), instead of returning a box height that read as 0 px blank on Windows and 6 px on Linux.
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

- **Release-build resources** (B2.21, `scripts/perf/measure.mjs`, Windows only so far; `scripts/perf/compare.mjs`
  aggregates alternating runs of two packages, B2.24):
  `node scripts/perf/measure.mjs --package-dir dist-packages/<label> --runs 5 --out perf-results/<label>`.
  - **What runs:** the normal release package (a smoke-test package is refused), extracted with `msiexec /a`.
    Nothing is installed.
  - **Isolation:** each scenario gets its own `DOWNOVA_HOME`, `DOWNOVA_DOWNLOAD_DIR` and
    `WEBVIEW2_USER_DATA_FOLDER`. A running Downova desktop stops the run, and only processes the tool
    started are ever ended.
  - **Scenarios** (order rotated per run):
    - idle with the window visible;
    - idle hidden to the tray (`WM_CLOSE`);
    - UI closed with the service running;
    - one, three and six active transfers (fixture in its own process, excluded). `active_6` (B2.32) sets the
      global limit and the Main queue limit to 6 with `dm concurrency-set` and `dm queue-limit main 6` and reads
      both back. Every active scenario records proof of activity (every download gained bytes during the sample
      window and none finished before it ended), then waits for completion and records the completion time, bytes
      transferred and every file's SHA-256 against the fixture's (`scripts/perf/fixture-process.mjs --count N
      --size-mib M`, which prints the hashes on its first line);
    - 1,000 and 10,000 finished records, seeded by `scripts/lib/seed-history.mjs` into the run's own
      database;
    - `service_start` (B2.24): the bare service started twice on one fresh data folder. The first start creates
      and migrates the database; the second opens it;
    - `cycles` (B2.24): the window hidden and reopened 5 times, then the service shut down and restarted 3
      times while the window reattaches on its own. JS heap after a forced GC, listeners, DOM nodes and the
      reattach time are recorded before and after.
  - **Measures:**
    - startup, all from the desktop spawn on one clock (B2.24):
      - `service_ready_ms`: until `dm hello` answers (includes the service's own start and, on a new
        profile, its migrations);
      - `ui_dom_ready_ms`: the page's DOMContentLoaded, always available;
      - `ui_first_paint_ms`: the WebView's first contentful paint, or null when the WebView reports none; WebView2 usually
        reports none when the window first paints while hidden;
      - `list_ready_ms`: until the list shows rows or its loaded-empty hint. It is marked `upper` when this
        happened before DevTools attached, and the attach time is then only an upper bound;
    - CPU over 10 s: Δ(kernel + user) of owned processes ÷ (wall time × cores);
    - private bytes per process, per role (desktop, WebView2, service) and summed;
    - working set, reported only as an over-counting sum because it includes shared pages;
    - process count;
    - installer, MSI and installed (extracted) sizes;
    - in the real WebView over its DevTools port: rows in the DOM, DOM nodes, time to the first list
      rows, 20 search-to-paint and 20 scroll-to-paint latencies, all with a bounded budget.
  - **Output:** raw `run<N>-<scenario>.json`, `samples.csv` and `summary.json`, which records the medians,
    versions, the package's `build-info.json` (commit and uncommitted diff) and the method. Since B2.33 latency p95
    is run-level: over per-run medians, only with ≥ 20 independent runs (observations within a run are never
    pooled).
  - **Validity (B2.33, `scripts/perf/validity.mjs`):** a run is `ok`, `error` (the run broke) or `invalid`. An
    active run is invalid when:
    - job ids are missing or not distinct;
    - a limit did not read back at least the file count;
    - a download was not transferring at the window start, gained no bytes between the window's start and end
      observations, or completed before the window ended;
    - a download failed, completion timed out (240 s), or a SHA-256 did not match.

    Only `ok` runs enter medians. The others keep their raw JSON and are listed in `summary.json` `excluded`
    (and by `compare.mjs`) with reasons. The command exits 1 when any run is not `ok`, 2 when unavailable (not
    Windows). The window starts only after every job transfers with bytes (bounded at 30 s). The fixture's first
    line is read with a bounded, chunk-safe reader. Activity evidence is two observations per job, not continuous
    proof.
  - **Diagnostics** (`--diagnose`, B2.24): separate runs, never mixed into resource medians, because the
    instrumentation (a MutationObserver and an animation-frame counter) costs memory and CPU. They record CDP
    Performance metrics at the start and end of the sample window (JS heap, DOM nodes, listeners, layouts, style
    recalculations, script time), the JS heap after a forced GC, and DOM mutations and frames during the window.
    `--slow-ms` sets the fixture's per-64 KiB delay so transfers outlast the window.
  - **Experiments and comparisons** (B2.24):
    - `--webview-args "<flags>"` adds WebView2 browser arguments; the summary records them, and such results are
      experiments, not the product;
    - `--rotate <n>` offsets the scenario rotation, so two packages can alternate one run at a time;
    - each WebView2 process carries its Chromium `type` (the GPU process is the one that varies per launch);
    - `node scripts/perf/compare.mjs <dirA> <dirB>` prints medians, ranges and sample counts per scenario.
  - **Limits:**
    - Shared CI runners are diagnostic only.
    - An IDM comparison was not run. The protocol for one (workloads W1–W6, settings classes A and B, ABBA order,
      gates) is [benchmarks/idm-comparison-protocol.md](benchmarks/idm-comparison-protocol.md). The fixture's
      `?norange=1` (B2.32) serves a file without range support (always 200, no `Accept-Ranges`) for W4.
    - Before/after comparisons need the same device and workload.

## Reporting
- For every completed increment, PROGRESS.md lists each command that was run, its result and the OS.
- Traceability evidence cells cite the test name and the run.
- If something could not run, say why and which OS or tool is needed.

## Configurable concurrency and host contention (ADR 0023)

- `cargo test -p dm-service --lib core::tests::concurrency`: more than three active files,
  non-preemptive decreases, queue limits, stopped queues, cancellation, invalid RPCs, persistence
  independently of destination settings, and the conservative optional aria2 cap. B2.31 adds:
  - the smaller of the two limits (global 1 with Main 6 runs 1; global 6 with Main 3 runs 3; 8 with 3 still 3);
  - two queues together within the global limit;
  - completion, pause, cancel, resume and queue stop/start releasing and reusing slots;
  - a restart keeping the global and Main limits;
  - a durable save refused by the database (a trigger, via a `rusqlite` dev-dependency) changing neither the
    saved nor the effective limit. Mutation-checked: applying the limit before the save fails the test.
- `cargo test -p dm-storage concurrency`: independent settings, reopen, invalid values and refused writes.
- `cargo test -p dm-domain --test contract_examples`: schema/Rust examples and the closed RPC method set.
- `cargo test -p dm-backend-http --test connection_budget`: C05 late-arrival progress and C06 six files
  progressing before earlier completion; C07 (Windows integration) every file exporting a durable checkpoint while
  all six run under contention; C08 (B2.30) a stream without range support taking the same plain requests (a probe
  and one uninterrupted stream) alone and under contention; exact hashes. The host budget is asserted twice
  (B2.30):
  - client-side, exactly: `HttpBackend::peak_requests_per_host()` (≤ the cap, and = 8 under contention);
  - server-side: `Server::max_overlap(TEARDOWN)`. Since B2.30 the fixture notices a client close while pausing a
    slow body, not at its next write, so the allowance needed fell from about 50 ms to 5 ms.
- `cargo test -p dm-service --test http_backend_process a05`: six real service downloads, restart,
  persisted global limit and byte-exact completion. Requires local Unix sockets on Unix. Its overlap assertion
  reports the observed values (overall and after the restart).
- `apps/desktop/src/app/concurrency.test.tsx`: validation, pending/double submit, uncertain acknowledgement
  and readback, load retry and Bangla. Service-client and Rust bridge tests cover fixed commands and bounds.
  B2.30 adds a rejected re-save and a stale save reply after a reconnect. B2.31 adds "What this allows":
  - the examples 6/6 → 6, 8/6 → 6 and 6/3 → 3, with the queue-limit reason and the Change queue limit button
    only when the queue decides;
  - the button opening Manage queues by keyboard and returning to Options with the new limit;
  - a refresh after a global save (the queue is untouched);
  - the aria2 cap;
  - active counts that leave out waiting, paused and disk-held downloads, a stopped Main, and more active than the
    limit;
  - re-reading after a reconnect;
  - Bangla.

  Mutation-checked (the min rule; the blocker filter).
- Run normal fmt, clippy, contracts, structure, lint, typecheck, all JS tests and builds. Record native
  bridge/process and installed-OS checks separately; restricted environments are not acceptance passes.

- `scripts/check-structure.mjs` §8 (part of `pnpm verify`, group structure): every command registered in the
  desktop shell is listed in `build.rs` and granted in `capabilities/default.json`, and every grant has a command.
- **Packaged acceptance (Windows):** `node scripts/acceptance/concurrency-acceptance.mjs --package-dir
  dist-packages/windows-x86_64`. It drives the normal package: the real window over DevTools (Options and Manage
  queues, keyboard Enter twice, an invalid value, Bangla), the packaged service and `dm`, and a fixture in its own
  process (`concurrency-fixture.mjs`, with a per-request log and the same 150 ms close allowance as the Rust
  fixture). Rows D0, U1–U4, S1 (unrelated settings keep the limit), V1 (invalid CLI values refused), T1–T3, R1, W1 (the reattached window shows the persisted limit and the same rows) and F1 (hashes, host overlap ≤ 8, per-file range overlap ≤ 4) are written to `acceptance-results/<label>/results.json`. Other OSes:
  not run.

- **Acceptance oracles (B2.31):**
  - **Host budget:** asserted client-side, from the packaged service's ESTABLISHED TCP connections to the fixture
    (`netstat -ano`, every 300 ms; at most 8). The fixture's own server-side overlap is recorded beside it.
  - **Fixture writes:** 16 KiB steps (`startFixtureServer({ chunk })`), so a close is noticed within one 30 ms step.
  - **U4:** goes through "What this allows" and its Change queue limit button, with screenshots in en and bn.
- **Engine permit trace (B2.32):** `DOWNOVA_PERMIT_TRACE=<file>` makes `dm-backend-http` append one JSON line per
  per-host permit taken or released (`t_ms`, `event`, a hashed host tag, `held` after the event). The packaged
  acceptance sets it and asserts the exact engine maximum (`engine_permits.max_held ≤ 8`) in T1 and F1, beside the
  300 ms client TCP samples (which can miss a short peak) and the fixture's server-side count (which counts a socket
  until the server notices its close). The trace is kept as `permit-trace.jsonl` in the results folder.
- **CLI queue limit (B2.32):** `dm queue-limit <QUEUE-ID> <N>` (1–16) calls `queues.set_limit`; argument tests in
  `crates/cli/src/args.rs`.
- **Installed-app acceptance kit (B2.34):** `node scripts/acceptance/build-kit.mjs --zip`.
  - **Refuses:** smoke-feature packages; installers or extension files that do not match the package's checksums; an
    extension without its manifest key; a package built with uncommitted changes; any production change since the
    package's commit.
  - **Writes:** the installers, the production extension, the fixtures with their relative import, the checklist
    (`docs/acceptance/b2-checklist.md`, with repository links turned into plain text), the Bangla quick-start, a
    `RESULTS.csv` row per checklist id, `KIT-INFO.json`, `verify-kit.mjs` and `SHA256SUMS.txt`.
  - **Test:** `scripts/acceptance/build-kit.test.mjs`.
  - **Not a substitute** for running the kit: installed and human rows are recorded only from a tester's
    `RESULTS.csv`.
  - **Since B2.35:**
    - `--zip` uses `scripts/lib/zip.mjs` (the same on every OS; executable bits kept; `zip.test.mjs`);
    - the extension ID derived from the manifest key must equal the helper's `DEV_EXTENSION_ID`;
    - the primary installer is chosen per OS;
    - the quick-start header is generated from the package.
- **Delivered-ZIP validation (B2.35):** `node scripts/acceptance/validate-kit-zip.mjs --zip <kit.zip>`.
  - **Checks:** archive integrity and exact contents; extraction with the OS's standard tool (bsdtar, `ditto`,
    `unzip`) and Unix executable bits; `verify-kit.mjs`; both fixtures started with the checklist's commands, one
    file each compared with the announced SHA-256; only its own processes stopped.
  - **Output:** `kit-validation.json`; exit 1 on failure.
  - **Scope:** build and delivery verification only.
- **Normal-package and kit workflow (B2.35):** `.github/workflows/acceptance-kit.yml`, `workflow_dispatch` only.
  - **Runs** natively on Windows x86_64, macOS aarch64 and Ubuntu 22.04 x86_64: `package.mjs` (normal), then
    `build-kit.mjs --zip` (source check on), then `validate-kit-zip.mjs`.
  - **Uploads** `downova-acceptance-kit-<label>-<sha>` (only after validation passes) and `…-logs` (always), with
    a job summary separating the build, kit-validation and acceptance (`NOT RUN`) statuses.
- **Tooling tests (B2.33, `pnpm verify` group structure, `node --test`):**
  - `scripts/lib/first-line.test.mjs`: a fixture's first line split across chunks or across a multi-byte
    character, a process that exits early or prints and exits at once, a deadline, a malformed line;
  - `scripts/perf/validity.test.mjs`: every invalid-workload rule, run status, exclusions, exit code, run-level p95,
    and `compare.mjs` over temporary runs (an invalid run never moves a median);
  - `scripts/lib/native-messaging.test.mjs`: framing byte by byte and the size cap;
  - `scripts/lib/fixture.test.mjs`: per-file statistics, the open-response peak, `norange`, and
    `fixture-process.mjs` binding, `--public-host` and `/__stats` reset.
- **Smoke JSON-lines readers (B2.31):** `readJsonl` parses only newline-terminated lines of a file a process is still
  appending to.

See [the report](reports/configurable-concurrency.md) for this run and remaining platform acceptance.

## Speed limits and queue schedules (B3.1, ADR 0024 and 0025)

- `cargo test -p dm-backend-http --lib rate`: bucket capacity, bounds, burst, live changes, cancellation and the
  unlimited path on a paused tokio clock.
- `cargo test -p dm-backend-http --test speed_limits`: S01 one download, S02 segmented, S03 global limit shared by
  1/3/6 downloads (each within ±10 %), S04 a live change without a new request, S05 1 KiB/s with a 1 s read timeout
  completes (the regression for the reqwest read-timeout defect), S06 an unlimited download unaffected by another's
  limit. Loopback fixture; throughput is measured by the test and printed.
- `cargo test -p dm-service --lib core::tests::speed_limits` (8) and `core::tests::schedules` (8): service behaviour
  with the fake backend; schedules use an injected clock (`TestClock`) so window edges, restarts and catch-up are
  deterministic. Refused saves use a `RAISE(ABORT)` trigger created while the store is closed.
- `cargo test -p dm-domain schedule`: overnight windows, merging, DST gap and fold (America/New_York), catch-up.
- `cargo test -p dm-storage`: migrations 10 and 11, reopen, and the v8/v9 upgrade simulations.
- `cargo test -p dm-cli`: `speed-limit*`, `--speed-limit`, `queue-schedule` parsing.
- `apps/desktop/src/app/speed-limits.test.tsx`, `queue-schedules.test.tsx`,
  `features/settings/speed.test.ts`: UI behaviour, validation before sending, Bangla strings.
- Not covered: real sleep/wake and OS clock changes (simulated only), macOS and Linux runs, packaged or installed
  exercise of either feature, aria2 limits (refused, `unverified`).
- **Packaged exercise (Windows):** `node scripts/acceptance/limits-schedules-acceptance.mjs --package-dir
  dist-packages/windows-x86_64` extracts the normal MSI (`msiexec /a`, nothing installed) and drives the packaged
  `dm-service` and `dm`: L1 overall limit, L2 per-download limit (each ±10 % on 4 MB), L3 the limit after a restart,
  S1 a schedule hold with no bytes received, S2 `dm queues` state, S3 the window opening admits and completes
  byte-exact, S4 the schedule after a restart and `off`. GUI rows: not run. Other OSes: not run.
- **Stabilization (B3.1, second batch):**
  - `cargo test -p dm-backend-http --test speed_limits s07`: a stalled server (no headers, or a stall mid-body)
    still ends in `NetworkOffline` after the read timeout under a 2 KB/s limit.
  - `core::tests::schedules` adds the exemption's lifetime, a retry or resume never starting outside the window, a
    moved running transfer being held, `queue_limit` only for a full limit, and clock changes around a catch-up.
  - `cargo test -p dm-storage upgrading_from_version_9`: a v9 database keeps jobs, finish stamps, queues and
    settings.
  - vitest `queue-schedules.test.tsx` (waiting reason, next window, exemption, refresh after a passed instant,
    Bangla, one save per click burst) and `speed-limits.test.tsx` (a late answer never shows on another download).
- **Packaged GUI (Windows):** `node scripts/acceptance/limits-schedules-gui-acceptance.mjs --package-dir
  dist-packages/windows-x86_64`.
  - **How it runs:**
    - the normal MSI is extracted with `msiexec /a` and the real window is driven over WebView2 DevTools (helpers
      in `scripts/lib/devtools.mjs`);
    - the fixture runs in its own process (`limits-schedules-fixture.mjs`, with back-pressure-aware per-file
      counters).
  - **Rows:** G0–G13 (§11 of the B3.1 report).
  - **Throughput criterion:** the client steady state (20 s, after 5 s) within ±10 %, with the fixture's count
    agreeing within a 4 MiB socket-buffer allowance.
  - **Schedules:** real windows starting about 2 minutes ahead in the host's zone.
  - **Not run:** an installed app, other OSes, human checks.
- **Unthrottled throughput A/B (Windows):** `node scripts/perf/throughput-compare.mjs --a <package> --b <package>
  --runs 5`.
  - Bare packaged services, no window, loopback fixture in its own process.
  - Workloads: one 512 MiB file and six 128 MiB files.
  - A and B alternate within each run; wall time from add to completion; every SHA-256 is checked.
- **`measure.mjs` process tree (B3.1 fix):** the owned tree follows `ParentProcessId` only to processes created after
  their parent (CIM `CreationDate`). Windows reuses PIDs while old processes keep stale parent IDs, so one run had
  adopted 165 system processes; check a summary's process count when reading older raw results.

## Attachment check, scan states and diagnostics (B3.2, ADR 0026)

- `cargo test -p dm-win-attachment` (identity, policy code) and `-- --ignored` (the real `IAttachmentExecute::Save`
  on a temporary file: Windows only; its outcome depends on the machine's policy and antivirus).
- `cargo test -p dm-service --lib core::tests::attachment`: deterministic fakes (`FakeScanner`, `NoScanner`) for
  checked, removed, altered, replaced, policy-blocked, unavailable (with fallback marking), no-stream volumes,
  skipped, cancel during the check with a late result, a hung check timing out while other jobs start, a restart
  during the check, and exactly one notification. The shared test constructor injects `NoScanner`, so older
  finalization tests keep their poll counts. Fakes are test-only code; production uses `OsScanner`.
- `cargo test -p dm-service --lib core::tests::diagnostics` and `diagnostics::tests`: synthetic secrets in URLs,
  engine error text, a proxy URL, a home path and a queue name never reach the preview, the export or events; bounds;
  stale snapshots, existing files, missing folders; atomic export with no temporary file left.
- `cargo test -p dm-domain redact`: `marking_origin`, `export_leaks` (including prose that must pass).
- `http_backend_process f06`: a cancel during finalization is accepted while the check runs on Windows (file intact,
  "not checked") and refused elsewhere.
- `apps/desktop/src/app/safety-diagnostics.test.tsx`: diagnostics preview, crash opt-in, one save per click burst,
  cancelled and refused saves, stale previews; the scan states in plain words, never "safe"; legacy records; Bangla.
- **Packaged (Windows):** `node scripts/acceptance/safety-diagnostics-acceptance.mjs --package-dir
  dist-packages/windows-x86_64` — P1 the real check before completion and origin-only Zone.Identifier, P2 the details
  in English and Bangla, P3 Help → Diagnostics… preview with no secrets, P4 the CLI export. The native save dialog and
  installed-app rows (W1–W4, D1–D2) are not automated.
- `scripts/check-structure.mjs` §5b: unsafe code only in `crates/win-attachment`; every other crate inherits the
  workspace lints.

## Secrets, credential store and signed-in downloads (B3.3, ADR 0027)

Every test uses synthetic secrets (`SYNTH-…-b33`) and scans the real outputs for them.

- `cargo test -p dm-domain cookies`: RFC 6265 domain (host-only and domain cookies), path, Secure-only-on-https and
  expiry rules; origin equality includes scheme and port.
- `cargo test -p dm-backend-http --test authenticated` (A01–A07, real TLS fixture trusted only through
  `extra_roots`): cookies follow domain and path per request, a same-origin redirect re-evaluates them, a
  cross-origin or https→http redirect stops with `AUTH_SCOPE_UNSUPPORTED` and the other origin is never requested,
  segments and resume carry the cookie, replace-source refuses sessions, public downloads send none.
- `cargo test -p dm-storage`: protected URLs and checkpoint payloads never reach the database, WAL or journal in the
  clear; without the key they are not persisted; an update while the key is unreadable keeps the sealed link;
  legacy plaintext rows are re-sealed one by one.
- `cargo test -p dm-platform vault` and `-- --ignored real_os_vault_round_trip` (the real OS store, a throwaway
  namespace, deleted afterwards).
- `cargo test -p dm-service --lib core::tests::credentials`: the session is used and never shown (snapshot, events,
  list, diagnostics, every file in the data folder); scope and capability refusals; remembering needs the store and a
  SQLite failure removes the vault entry; lost-acknowledgement retries give one job; restart restores remembered
  sign-ins with the sealed link and holds temporary ones; a locked store at start keeps the link for the next start;
  forget pauses users; pending rows are reconciled; without a store public downloads work and protected links stay
  in memory.
- `cargo test -p dm-browser-host --test host -- --ignored signed_in`: the real host and a real debug service
  (`DOWNOVA_TEST_CA_DER`, debug builds only) download from an HTTPS fixture with the cookie; another origin and http
  are refused; the cookie is absent from host stdout/stderr, service stderr, snapshots, `credentials.v1.list` and
  every file under the data folder.
- `apps/extension/src/signin.test.ts`: scope at the click, one cookie store per tab, partitioned/expired/other-store
  cookies dropped, bounds, cookies re-read on retry and never stored, the popup line in English and Bangla.
- `apps/desktop/src/app/signed-in.test.tsx`: the sign-in note (site, browser, mode, state), one forget per click
  burst, failed forget, expired and locked states, Bangla.
- **Real browser:** `node scripts/e2e/chrome-handoff.mjs --kind chrome|edge` steps S1–S3 (openssl makes a throwaway
  CA; the cookie is placed through DevTools; the extension's real signed-in path; one job, SHA-256, every request had
  the cookie; a cross-origin link is refused in the browser; no cookie in extension storage, CLI answers or files).
  The permission prompt itself cannot be answered by automation: the e2e build declares the permission granted.

### Reconnect (B3.3 completion, ADR 0028)

- `cargo test -p dm-service --lib core::tests::reconnect`: restart → held with its checkpoint → reconnect on the same
  job (restored once, the fresh cookie only, the sealed link back); 401 held once, expired, no retry storm, then
  Reconnect and resume; manual pause and stopped queue kept; bound to job, origin, profile, browser and generation
  (public, other site, other profile, other browser, non-applicable cookie, stale, cancelled refused); a repeat
  changes nothing twice, a reused id is a conflict (also after a restart); locked, too large and SQLite failures
  leave the old state and no new store entry; a cancel racing a remembered reconnect wins (gated vault); Forget
  releases the attempt so Resume cannot use it; a failed download gets its sign-in back and Retry restarts it; a
  repeated signed-in add never changes the accepted sign-in.
- `cargo test -p dm-service --test reconnect_process` (real service, real engine, HTTPS fixture): R1 kill → held →
  reconnect → resumed with a range request carrying only the fresh cookie, SHA-256; R2 401 after pause → one refused
  request → Reconnect and resume → SHA-256; R3 the resource changed while held → never spliced (completed with the
  whole new representation). Each scans the data folder for both cookies.
- `cargo test -p dm-backend-aria2 --test fixtures -- --ignored a_signed_public_link`: a signed public link paused and
  stopped leaves no file holding the signature.
- `cargo test -p dm-browser-host --lib`: reconnect refusals (http origin, path in origin, bad operation id, bad
  credential id, http session) before any service is asked; a refusal never echoes a cookie.
- `apps/extension/src/reconnect.test.ts`: page and partition rules, host cookie selection, explicit choice, one
  operation id per (request, download) reused by retries, refusals, the page in English and Bangla.
- `apps/desktop/src/app/signed-in.test.tsx`: the held state and reconnect guidance in both languages; a late forget
  answer never shows on another download.
- **Real browser:** `chrome-handoff.mjs` S4 (restart → held with its saved part → reconnect through the extension →
  the same job resumes with a range request and only the fresh cookie, SHA-256, no cookie in files or extension
  storage) and S5 (a third-party frame is refused).
- **Packaged (Windows):** `node scripts/acceptance/secure-auth-acceptance.mjs --package-dir
  dist-packages/windows-x86_64` Q1–Q5 (release service, real Credential Manager, packaged host and CLI; its own
  Credential Manager entries are deleted afterwards).

## Firefox and browser packages (B3.4, ADR 0029)

- `apps/extension/src/manifest.test.ts`: both manifests (Chromium identity unchanged; Firefox gecko ID, event page,
  `data_collection_permissions`, no capture/cookie/host permissions), e2e versus normal grants.
- `apps/extension/src/handoff.test.ts`: each browser's wording for a missing host maps to `host_missing`
  (unavailable), never uncertain.
- `cargo test -p dm-browser-host`: `caller()` (Firefox manifest path plus allowlisted add-on ID; missing, unknown,
  differently cased or extra arguments refused; Chrome unchanged); Firefox registration location, key and manifest
  shape; ownership, repair, other installation, unreadable, removal independent of Chrome (redirected root, private
  registry subtree); `detect` (Linux native/Snap shim/Snap/Flatpak/unknown, Windows, macOS) and the matrix; the real
  executable started with Firefox arguments (`firefox_launch_arguments_are_checked_like_chrome_origins`).
- `cargo test -p dm-desktop browser_setup`: status lines become path-free connections; unknown browsers refused.
- `apps/desktop/src/app/browser-connections.test.tsx`: package, connection and verdict text; hidden browsers;
  one repair per click burst; refusal explained; a stale answer after Check again never shown; helper unavailable;
  Bangla.
- **Real Firefox:** `node scripts/e2e/firefox-handoff.mjs` F1–F9 (isolated profile, temporary add-on through WebDriver
  BiDi, the product's registration with any previous value restored). Test-only: the temporary install,
  `MOZ_REMOTE_ALLOW_SYSTEM_ACCESS=1` for the test browser, the menu listener invoked directly, and the e2e build
  opening its own page.
- **Packaged (Windows):** `node scripts/acceptance/browser-lifecycle-acceptance.mjs --package-dir
  dist-packages/windows-x86_64` B1–B7 (two extracted copies of the MSI as two installations; redirected file root and
  a private registry subtree).
- `pnpm verify` builds both extension targets (`vite build extension` and `vite build extension (Firefox)`).

## Updates, backup and recovery (B3.5, ADR 0030)

- `cargo test -p dm-service --lib update`: hold keeps desired states (pause, stopped queue), refusals while held,
  backup and record agree, idempotent prepare, cancel resumes, lease expiry, checkpoint failure releases, invalid
  operation and version refused, startup acceptance (installed / not_installed), recovery record and restore
  (tampered backup refused, replaced files set aside).
- `cargo test -p dm-service --test update_process` (real `dm-service`, `DOWNOVA_VAULT=file`): acceptance after a
  restart with held work resuming only then (SHA-256); a failed migration (`DOWNOVA_TEST_FAIL_MIGRATION=1`) exits 6
  and restores through `--restore-update` (unknown operation exits 5); a corrupt backup
  (`DOWNOVA_TEST_CORRUPT_BACKUP=1`) cancels the prepare and work continues.
- `cargo test -p dm-desktop`: `update::check_config` (only a secure, signed-version configuration is trusted),
  package targets, operation IDs.
- `node scripts/update/verifier-fixture.mjs` (in `pnpm verify`, rust group): the real `tauri-plugin-updater` with two
  disposable key pairs from the pinned Tauri CLI against `tests/update_verifier.rs`: valid, tampered, wrong key,
  missing signature, unbound version, other version, older/equal, wrong target, cut download. Plain HTTP without
  the fixture flag is asserted refused only in a release build (the plugin only warns in debug builds).
- `apps/desktop/src/app/updates.test.tsx`: not configured is never "up to date", the full flow with two
  confirmations, stale operations dropped, failures and retry, recovery restore, auto-check opt-in, Bangla.
- `node --test scripts/lib/updater-config.test.mjs`: what `package.mjs` accepts as normal, production and fixture
  updater setups.
- Human and installed rows: UP1–UP4 in the [acceptance checklist](acceptance/b2-checklist.md).

## Installer lifecycle, notices and the update journey (B3.6, ADR 0031)

- `cargo test -p dm-platform --lib endpoint`: only a service whose recorded executable is inside the installation
  folder is "ours" (same file name elsewhere, no recorded executable, missing folder: not ours); old endpoint files
  still parse.
- `cargo test -p dm-cli --test cli shutdown`: `dm shutdown --only-from` sends nothing to another installation's
  service or to a pre-B3.6 service, stops its own one with state kept, and is refused for other commands.
- `cargo test -p dm-desktop --lib service::`: the desktop attaches only to a service of its own build version.
- `node --test scripts/lib/notices.test.mjs`: dependency classification (normal, proc-macro, build, dev), notice
  files, unresolved metadata recorded, the bundled text verbatim.
- `node scripts/notices/generate-notices.mjs --check` (in `pnpm verify`, structure group, needs cargo).
- `apps/desktop/src/app/notices.test.tsx`: Help menu → notices from the bundled inventory; every component has
  targets and existing texts; texts verbatim in a read-only textarea, `lang="en"` `dir="ltr"`; unknowns stated; filter;
  unreadable inventory; Bangla; About shows the build version in both languages.
- `node --test scripts/acceptance/build-kit.test.mjs`: multi-letter rows (FX, UP, IL) reach `RESULTS.csv`; an
  update-fixture package is refused as a candidate.
- `scripts/package.mjs`: refuses a package whose components report different versions, an MSI without the notices
  or the sidecars, or stale notices.
- `node scripts/update/verifier-fixture.mjs --release`: the plugin's own plain-HTTP refusal in a release build.
- `node scripts/acceptance/update-journey.mjs` (Windows, extracted packages; see setup).

## Native installer lifecycle (beta readiness, REL-01)

- `node scripts/acceptance/installer-lifecycle.mjs` runs REAL installers (NSIS and MSI on Windows, `.deb` with dpkg on
  Linux, the `.app` from the `.dmg` on macOS) for the update-fixture pair A → B: install, work (running, manually paused,
  stopped queue), upgrade while the service runs, the work continues (SHA-256), uninstall (no owned process; data and
  downloads kept; only Downova's registration removed), reinstall. It refuses to run outside a GitHub-hosted runner or a
  machine declared disposable (`DOWNOVA_DISPOSABLE_MACHINE=1`), and on a machine that already has Downova.
- Workflow: Actions → **Installer lifecycle** (manual). Failed rows are published as annotations.
- Evidence categories stay separate: unit/integration, smoke-feature packages, normal packages and kits, installed
  lifecycle (this), human acceptance.
