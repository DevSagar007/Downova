# Configurable concurrency and host contention

- **Date:** 2026-11-02
- **Baseline:** `4fc3d66a8e9dd00679ae2e6275d1e684a20e1cba`, clean isolated checkout before editing.
- **Host:** Ubuntu 24.04.3 LTS, Linux 6.18.57 x86_64; Rust 1.99.1, Node 24.22.1, pnpm 12.14.0.
- **Requirements:** CORE-02, CORE-05, E03; [ADR 0023](../decisions/0023-configurable-concurrency-and-request-rotation.md).
- No commit, push, system-wide installation or live app changes. Source changes remain uncommitted.

## Windows integration and native acceptance (2026-11-02)

- **Host:** Windows 11 Pro 10.0.26300 x64, 7.5 GB RAM; rustc 1.99.1; Node 26.7.0; pnpm 12.14.0; WebView2
  156.0.4314.58. Cargo builds use `CARGO_BUILD_JOBS=2` (full parallelism runs out of memory on this machine).
- **Integration:**
  - **Baseline:** HEAD was `4fc3d66` with a clean tree, the patch's exact baseline.
  - **Hash:** the patch SHA-256 `4a672ad8…71dc6` matches `manifest.json`.
  - **Apply:** `git apply --check` passed, then one `git apply`. No newer commits overlapped and nothing was forced.
  - Everything is uncommitted.

### Defects found during integration (all fixed, each with a check that fails without its fix)

1. **The packaged app would have refused both new commands.**
   - **Defect:** `concurrency_get` and `concurrency_set` were registered in `lib.rs` but missing from
     `build.rs` COMMANDS and `capabilities/default.json`. Tauri denies an ungranted command at run time,
     which no web, unit or headless check sees.
   - **Fix:** both are now listed and granted.
   - **Check:** `scripts/check-structure.mjs` §8 now fails on any registered command that is not listed or
     granted, and on any grant without a command. Verified by removing one grant.
2. **No durable checkpoint while a file was under contention (data-loss risk on a crash).**
   - **Defect:**
     - The periodic sync publishes a segment's checkpoint after 4 MiB or 1 s within one request.
     - Rotating requests end after about 1 MiB and restart both counters.
     - So a file under steady contention exported no checkpoint at all, and an abrupt stop lost all of its
       progress.
   - **Found by:** the attachment's own process test A05, the first time it ran (it could not run on Linux): "no
     checkpoint for job …", twice.
   - **Fix:** a yielding request publishes its synced segment state, as the periodic sync does
     (`segmented.rs`; ADR 0023 amendment 7).
   - **New backend test C07:** every file exports a durable checkpoint while all six still run under contention.
     It fails without the fix (verified) and passes with it.
3. **The both-limits rule was not stated clearly.**
   - **Defect:** the Options note only said that queue limits "still apply". The Bangla note also named the menu in
     English ("Downloads → Manage queues"), while the Bangla window shows "ডাউনলোড → কিউ পরিচালনা…".
   - **Fix:** both languages now say that a queue runs at most the smaller of the two limits, and that six at once
     in Main needs both set to 6, naming the menu as the window shows it.

Reviewed and kept unchanged:
- **Final flush:** a failed final flush now fails a yielding or otherwise ending request instead of rotating
  forever. A user pause still ends as "stopped": `run` checks the stop flag first.
- **Pause model:** "Paused" is `desired_state`. The `phase` field keeps its last value, exactly as without
  contention, so the evidence that a pause took effect is that bytes stop.

### Checks (Windows 11 Pro 10.0.26300 x64)

| Command / case | Result |
|---|---|
| `git apply --check` then `git apply` (once) | PASSED |
| `pnpm verify -- --only=structure,contracts,format,lint,typecheck,test,build` | PASSED 12/12; desktop **193** tests, extension **81** (as recorded on Linux) |
| `pnpm run contracts:check` | PASSED (42 valid, 23 invalid examples) |
| `pnpm contracts`, `pnpm check` | UNAVAILABLE: no such scripts in `package.json` (`contracts:check` and the `pnpm verify` groups are the equivalents) |
| `cargo fmt --all --check` | PASSED |
| `cargo clippy --workspace --all-targets -- -D warnings` (including `dm-desktop`, Tauri macros) | PASSED |
| `cargo test --workspace`, first run (attachment as delivered) | FAILED: A05 "no checkpoint for job …" (defect 2); the run stopped at that suite |
| `cargo test -p dm-service --test http_backend_process a05`, attachment as delivered | FAILED (the same, reproduced) |
| `cargo test --workspace --no-fail-fast`, after the fix | 472 passed, 2 failed: A05 in the workspace run and again standalone, both at the host-overlap assertion (value not recorded then) |
| A05 after making that assertion report its values | PASSED 14 of 14 standalone runs and 3 of 3 full `http_backend_process` runs (50 tests each); overlap 8 overall and 8 after the restart every time. **The two earlier overlap failures are unexplained and recorded as an intermittent** |
| `cargo test -p dm-backend-http --test connection_budget c07` | PASSED; FAILED without the fix (proved twice, at 32 MiB and at 16 MiB). At 32 MiB it once failed in a parallel suite run with the backend's 10 s read timeout on this 7.5 GB machine (1 of 3), so it now uses C06's 16 MiB: the whole suite then PASSED 3/3 |
| `cargo test -p dm-desktop --lib concurrency` (the real adapter inside the Tauri crate) | PASSED |
| `pnpm verify` (full), run 1 | 18/20: every test group PASSED (all Rust, native-host end-to-end, aria2 groups); FAILED only `prettier --check` and `cargo fmt --check` (files edited while it ran), both fixed |
| `pnpm verify -- --only=format,rust` | format and fmt PASSED; `cargo test` FAILED once on C07 (the read timeout above) |
| `pnpm verify` (full), final run | **PASSED 20/20** (structure incl. §8, contracts, format, lint, typecheck, desktop and extension tests, builds, `cargo fmt`, `cargo clippy`, `cargo test` incl. A05 and C07, native-host end-to-end, four aria2 groups) |

### Packaged acceptance (the normal package, real window, real IPC, real service)

`node scripts/acceptance/concurrency-acceptance.mjs --package-dir dist-packages/windows-x86_64` extracts the normal
MSI (nothing installed). It runs with its own data, download and WebView2 folders, and its fixture in its own
process (range-capable, strong ETag, paced). It drives:
- the packaged window through its DevTools port: real Tauri commands, so the grants are exercised;
- the packaged service through the packaged `dm`.

Final run `acceptance-results/concurrency-win-7/results.json`: **verdict passed, 10/10.**

| Row | Observed |
|---|---|
| D0 | The packaged desktop started its packaged service; backend `http` |
| U1 | Options: initial 3; "6" typed and **Enter pressed twice**: exactly one `POST ipc.localhost/concurrency_set` (plus its CORS `OPTIONS` preflight); "Saved global limit: 6"; `dm concurrency` = 6 |
| U2 | "17": `aria-invalid`, the alert "Enter a whole number from 1 to 16.", Save unavailable, **no** request sent |
| U3 | Bangla: "একসাথে ডাউনলোড", label "সর্বোচ্চ সক্রিয় ডাউনলোড", the both-limits sentence, the menu named in Bangla |
| U4 | Downloads → Manage queues…: Main set to 6 in the window; the service reports 6 |
| T1 | Six 32 MiB files from one host: first bytes at 0.2, 0.2, 2.6, 2.6, 7.3, 7.3 s, all before any completed; the fixture served bytes for all six; **six window rows showed a percentage**; host overlap **8** |
| T2 | Global lowered to 2 while six ran: all six kept `transferring` and gained bytes over 15 s; a new download waited (`queued`, `queue_limit`, 0 bytes) |
| T3 | Raised to 16: the waiting Main download started within 3.5 s. The manually paused one gained no bytes (17,367,040 before and after) and was served none. The one in a stopped queue stayed held (`queue_stopped`) |
| R1 | Abrupt service stop after every running job had a committed checkpoint (trace), then a restart: global 16 and Main 6 persisted; all six interrupted downloads resumed; pause and stop still held |
| F1 | After resume and queue start, **all eight completed and matched their SHA-256**; host overlap 8 including across the restart; 21–37 requests per file (rotation) |

**How the oracle was corrected along the way (runs 1–6, kept under `acceptance-results/`):**
- **Script defects:**
  - a DOM element returned by value;
  - an `invoke` wrapper that Tauri does not allow, replaced by counting IPC requests at the network level;
  - a 4 s window, too short under rotation.
- **Measurement artefacts:**
  - The first fixture ran inside the script, whose blocking CLI calls delayed close detection: overlap 10 to 12,
    raw peak 15.
  - Large payloads held twice slowed the fixture to about 0.2 MiB/s per request on this 7.5 GB machine, which made
    rotation look like waves of about 4.5 MiB.
  - With the fixture in its own process and 32 MiB payloads, overlap is 8 with the same 150 ms close allowance
    the Rust fixture uses. The raw count, which includes not-yet-noticed closes, is reported beside it.
- **Assertion corrected:** "the paused job is not `transferring`" was wrong for this product's model.

### Evidence boundaries

- **Native on Windows:** the Tauri crate, its macros and grants, real IPC from the packaged window, the packaged
  service and CLI, the HTTP backend over loopback, persistence and recovery after an abrupt stop.
- **Not run:**
  - macOS and Linux native builds or packages; no remote CI exists for this change;
  - an installed (NSIS) build; the MSI was extracted;
  - screen readers;
  - OS-level zoom and the look of the dialog by a person;
  - an uncertain save (lost acknowledgement) in the packaged app. Readback without resending is covered only by
    `apps/desktop/src/app/concurrency.test.tsx`.
- **No speed, RAM or IDM claim.**

## Linux preparation (from the attachment; historical)

## Behaviour

- Global maximum is persistent and user-configurable from 1 to 16 (default 3), through Options,
  the typed Tauri/service API and `dm concurrency` / `dm concurrency-set N`.
- Saving destination/history/notification settings cannot reset it. Restore it before recovery.
  Invalid values and unknown fields are refused. A failed durable save cannot change the effective limit.
- An increase admits eligible jobs immediately; a decrease never interrupts a running attempt.
  Per-queue limits, order, priority, stopped queues, user pauses and cancellation are preserved.
  Existing main queues stay at three: to run six in main, set both limits to six.
- Native HTTP still caps each host at eight concurrent requests, across probes/ranges/redirects.
  Contended validated segments flush/sync, release their request after bounded data and rejoin
  capacity waiters. Each new range must prove the same identity. With no waiters, requests are unchanged.
- Optional aria2 keeps its conservative three-file cap, explicitly explained in Options.
- English/Bangla, labelled controls, validation, one pending save, explicit uncertain outcomes and
  readback are included. Late responses from an old view/service instance are ignored.

## Verification

Commands ran in the pinned environment above with `CARGO_BUILD_JOBS=2`. Raw logs accompany
this patch; no missing or unrun check is counted as passed.

| Command / case | Result (Linux) |
|---|---|
| `pnpm verify -- --only=contracts,lint,typecheck,test,build` | PASSED 9/9; desktop 193 tests, extension 81; both production web builds |
| `pnpm verify -- --only=structure,format` | PASSED 3/3 |
| `cargo test -p dm-backend-http -- --test-threads=2` | PASSED 102 tests (including C05 and C06) |
| C05 on the original backend in an isolated baseline worktree | FAILED as expected: the earlier file had completed before the late file received data |
| Final connection-budget suite | PASSED 6/6; C05 additionally passed two isolated repetitions (3/3 final runs); C06 passed standalone and in the suite |
| `cargo test -p dm-service --lib core::` | PASSED 105/105, including the five new concurrency tests |
| `cargo test -p dm-storage` | PASSED 38/38; existing opt-in latency benchmark ignored, not counted |
| `cargo test -p dm-domain --test contract_examples` | PASSED 4/4, including closed RPC method parity |
| CLI / domain library groups of the combined library run | PASSED 17/17 and 29/29 respectively |
| `cargo fmt --all --check`, `git diff --check` | PASSED |
| `cargo clippy -p dm-service -p dm-storage -p dm-domain -p dm-cli -p dm-backend-http --all-targets -- -D warnings` | PASSED, no warnings |
| All service library tests | FAILED (environment): 123 passed; six IPC tests cannot bind Unix sockets (EPERM) |
| `cargo clippy --workspace --all-targets -- -D warnings` | FAILED (environment): `glib-sys` cannot find `pkg-config`; native GUI prerequisites unavailable |
| `cargo test --workspace` | FAILED at the same native GUI prerequisite; no full-workspace test pass claimed |
| Exact Rust downloads adapter in a headless harness | PASSED: canonical source/tests byte-identical; adapter typechecked and one bounds test passed. No native GUI or IPC claim |
| Real-service A05 and native installed bridge/desktop journeys | NOT RUN: AF_UNIX bind is denied and native GUI build prerequisites are missing |
| Installed Windows/macOS/Linux UI acceptance, screen reader and zoom checks | NOT RUN |

The environment denies AF_UNIX bind even in an independent minimal Python probe. No production IPC
guard or test was weakened. A successful web build, adapter check or pure core test is not native
Tauri acceptance. The headless adapter check is kept separate from native display/IPC evidence.
No remote workflow has run on this uncommitted change.

The original pre-fix failure is reproducible using just C05 against baseline `4fc3d66`.
Current C05/C06 verify actual bytes and SHA-256, not simply an accepted/active snapshot. No inference
that every real-world server supports this is made. The final suite also covers changed validators,
wrong ranges, 200 fallbacks, durable resume, replacement identity and disk guards.

## Manual acceptance after building

1. Use a separate test profile/download folder with the controlled fixture server.
2. Set global and main queue limits to six; add six large range-capable fixture files from one host.
   Verify all six receive bytes before earlier files finish and all completed hashes match.
3. While they run, decrease global to two: running downloads continue; waiting work starts only
   when the count is below two. Raise it again and confirm immediate eligible admission.
4. Pause one job and stop another queue before raising limits: neither user intent is overridden.
5. Restart the service; confirm both limits persist and resumed files are byte-exact.
6. Repeat in Bangla and with keyboard-only operation. Simulate a lost acknowledgement and use
   Reload: no extra save is sent. Check actual native desktop placement/zoom on each target OS.

## Limits and next work

Connection rotation does not promise equal bandwidth, a fixed time-to-first-byte, or an IDM speed
advantage. Non-range/validator-less streams cannot safely be interrupted for sharing; requests beyond
host capacity still wait. Keep ADV02 adaptive tuning separate. Run the new A05 process case and native
bridge tests on unrestricted OS runners, then packaged acceptance and resource measurements.

## Headless adapter verification

The temporary `bridge-check` crate copies the canonical `downloads.rs` and `downloads/tests.rs`
byte-for-byte and uses the repository Cargo.lock and pinned dependencies. It compiles those exact
modules without linking the unavailable Tauri GUI libraries. Command:
`cargo test --offline --manifest-path ../bridge-check/Cargo.toml --lib concurrency_rejects_bad_limits_without_contacting_a_service`.
It passes one bounds test; the transport tests are compiled but not run. The copied module reports
one expected dead-code warning for helpers used by the omitted shell. No lint is suppressed in the
project. This is not proof that Tauri macros, the installed application or OS IPC work here.

## Applying and validating the change

The downloadable change bundle includes a patch, source/diff hashes, this report and raw logs.
Apply with `git apply --check` followed by `git apply` in a clean checkout based on the baseline.
If the check fails against newer work, integrate the diff without discarding that work.
On a normal desktop build machine run the full repository verification and the new A05 process
journey; the prepared native bridge tests remain an explicit acceptance gate. No release-ready or
B2-exit claim is made.
