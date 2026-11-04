# Configurable concurrency and host contention

- **Date:** 2026-11-02
- **Baseline:** `4fc3d66a8e9dd00679ae2e6275d1e684a20e1cba`, clean isolated checkout before editing.
- **Host:** Ubuntu 24.04.3 LTS, Linux 6.18.57 x86_64; Rust 1.99.1, Node 24.22.1, pnpm 12.14.0.
- **Requirements:** CORE-02, CORE-05, E03; [ADR 0023](../decisions/0023-configurable-concurrency-and-request-rotation.md).
- No commit, push, system-wide installation or live app changes. Source changes remain uncommitted.

## B2.32: Linux P2 root cause, exact host budget, six-download baseline (2026-11-04, Windows 11 Pro 10.0.26300 x64)

**Base:** `12dc3e7` (B2.31, committed by the user). The B2.32 sources were committed by the user as `baee51f`
during this step; the only later change is the acceptance `record` fix below (uncommitted).

### Evidence matrix (public API; `gh` is not installed)

| Commit | Workflow | Windows x64 | macOS arm64 | Linux x64 (22.04 / 24.04) | Gaps |
|---|---|---|---|---|---|
| `e0609bc` | CI #49 | passed | passed | passed / passed | — |
| `e0609bc` | Package smoke #39 | passed | **failed** R3 Chrome C1/C2 (browser kept the download: `finished_in_browser`) | **failed** P2 (`blank_px_top: 6`, no geometry) | — |
| `12dc3e7` | CI #50 (41776546580) | passed | passed | passed / passed | — |
| `12dc3e7` | Package smoke #40 (41776546581) | passed | passed (Chrome and Edge 13/13) | **failed** P2 only (94 passed, 1 note) | the old probe |
| `baee51f` | CI #51 (41780221546) | passed | passed | passed / passed | — |
| `baee51f` | Package smoke #41 (41780221573) | passed (94 + 1 note) | passed (94 + 1 note) | **passed** (95 + 1 note; .deb, AppImage) | the note: aria2 not bundled |

- **macOS Chrome C1/C2:** failed once in five runs (#39); #37, #38, #40 and #41 passed. The browser finished the file
  before capture settled. Intermittent and unexplained; it belongs to the browser workstream and stays open.
- Linux P2 on `baee51f` is the first Linux pass with the strengthened assertion (`rows_visible ≥ 1`).

### Linux P2: root cause and fix
- **Measurement path:**
  - the probe (`LIST_PROBE`, `smoke.rs`) measures in the real window and returns its result through the URL
    fragment;
  - `packaged-smoke.mjs` parses it, P2 asserts it, and `annotations.mjs` publishes it.
- **The gap was in the probe.** When no row was visible, the probe returned before recording geometry and reported
  the scroll box's height as the "blank band":
  - 0 px on Windows: a false pass;
  - 6 px on Linux: the failure, with no geometry.

  An injected gap was reported correctly. Emulating 640×480 (the "small xvfb screen" hypothesis) was refuted: the
  list kept 124 px.
- **Root cause (all platforms):** at 200 % zoom the details panel of the selected download wrapped and took the whole
  main column, leaving the list 0 px tall. Before the fix, the real Windows package showed **0 visible rows** at
  zoom 2.
- **Fix:**
  - `App.css`: the details panel shrinks first (at most 40 % of the column, scrolling inside);
  - `DownloadTable.css`: the list keeps its header and one row (`calc(var(--table-row-height) * 2)`);
  - documented in design.md §1.
- **Regression check:** P2 now also requires `rows_visible ≥ 1` with a download selected. The probe
  (`probe_version: "b2.32"`) records the geometry and block heights even when no row is visible. Before the fix it
  fails (0 rows at zoom 2); after it, it passes.
- **After (local packaged smoke, Windows):** 14/4/2 visible rows at 100/150/200 % in light and dark; blank 0 px;
  overflow 0; focus kept; End/Home; hidden spacers; Bangla 334 matches → `aria-rowcount` 335.
- **Not changed:** the 2 px tolerance, the 200 % case, and Linux in the matrix.

### The server-side overlap of 9 (B2.31 `final3`): explained, no production defect
- **The B2.31 record stays:** `final3` T1 failed with a fixture overlap of 9 (raw 10), while F1 of the same run
  showed 8.
- **Three views of the budget:**
  - **Engine permits** (exact): new in B2.32. `DOWNOVA_PERMIT_TRACE` makes `dm-backend-http` log every per-host
    permit taken and released, with the count held. Hosts are hashed, never named.
  - **Client TCP** (`netstat` every 300 ms): can miss a short peak.
  - **Server sockets** (the fixture): a socket counts until the server notices its close, at its next 30 ms write
    step.
- **Packaged acceptance `concurrency-b232`:**
  - 496 trace events, 252 acquires, one host;
  - **the engine never held more than 8** (T1 and F1: `max_held` 8);
  - client TCP max 8;
  - fixture overlap 8, raw peak 12–13.
- **Cause:** 201 of 209 release→acquire handovers took ≤ 5 ms (median 0 ms). The engine releases a permit and opens
  the next request at once, so the server briefly sees the new request beside the old socket it has not yet seen
  close. That is a measurement artefact; **production scheduling is unchanged**.
- **T1 and F1 now also assert `engine_permits.max_held ≤ 8`** (with at least one acquire traced).
- **Acceptance oracle defect found and fixed (uncommitted):** `record` spread the detail after the step's
  status, and U1's detail had a `status` key (the window's status text). A failed U1 would therefore have been
  recorded as that text, and the verdict, which only looked for `"failed"`, would still pass. Now:
  - the step's own fields come last;
  - the detail key is `ui_status_text`;
  - any status other than passed or note fails the verdict.

  In `concurrency-b232` the U1 condition held (initial 3, one `concurrency_set`, service 6). The run was repeated
  with the fixed script (`concurrency-b232b`, below).
- **Repeat with the fixed script (`concurrency-b232b`, the normal package `baee51f`):** 14/14 passed, U1 now a real
  `passed`.
  - **T1:** engine `max_held` 8 (32 acquires), client TCP 8, fixture overlap 8, raw peak 11.
  - **F1:** engine `max_held` 8 (218 acquires), client TCP 8, **fixture overlap 9**, raw peak 13.
  - 178 of 184 handovers took ≤ 5 ms (median 0 ms).
  - The 9 reappeared server-side in the same run in which the engine's exact count never passed 8, which confirms
    the explanation above.

### Other changes
- **CLI:** `dm queue-limit <QUEUE-ID> <N>` (1–16, `queues.set_limit`), used by the six-download measurement.
- **Fixture:** `?norange=1` (always 200, no `Accept-Ranges`) for the non-range workload.
- **Perf:**
  - `measure.mjs`: `active_6` (global and Main limits set to 6 and read back);
  - every active scenario: proof that each download gained bytes in the window and none finished early, then
    completion time, bytes transferred and SHA-256;
  - `fixture-process.mjs` serves N files and prints their hashes;
  - `compare.mjs` aggregates the activity.
- **Docs:**
  - the IDM comparison protocol ([benchmarks/idm-comparison-protocol.md](../benchmarks/idm-comparison-protocol.md),
    unrun);
  - the installed-app runbook ([b2.32-installed-acceptance-runbook.md](b2.32-installed-acceptance-runbook.md),
    NOT RUN).

## B2.31: understandable download limits, native acceptance (2026-11-04, Windows 11 Pro 10.0.26300 x64)

**Base:** `e0609bc` (pushed by the user). This step was uncommitted on top of it when written; the user later committed it as `12dc3e7` (CI #50 and package smoke #40 ran on that commit; see B2.32).

**Remote results for `e0609bc`:**
- **CI #49: green on all runners.** The B2.30 fixture fix holds on macOS.
- **Package smoke #39:**
  - Windows passed.
  - Linux failed P2 again (`blank_px_top: 6` at 200 %), **without the geometry** that the same probe records locally
    for an injected 6 px band (checked in a real packaged window). The probe now also reports
    `probe_version: "b2.31"`, so the next Linux run shows which build ran. The layout is unchanged because the gap
    still cannot be reproduced on Windows.
  - macOS failed the real-Chrome capture rows C1/C2 once (the browser kept the download; it passed in #38). This is
    browser capture, not concurrency, and is recorded for that workstream.

### What changed for the user

Options → Simultaneous downloads now has **What this allows**, built from the service's state (never the unsaved
field):
- the saved limit for all queues;
- the Main queue's limit;
- how many Main can run at once: the smaller of the two, and at most 3 with the optional aria2 engine;
- what is **active now**, in all queues and in Main, counted as the service counts slot use. Waiting, paused and
  disk-held downloads are not counted.

Plain sentences then:
- explain that the queue limit decides when it is lower, with a **Change queue limit** button. It opens the existing
  Manage queues dialog; closing that returns to Options, which reads the limits again.
- explain the aria2 cap when it is what stops Main, a stopped Main queue, that other queues share the limit for all
  queues, and why more downloads can be active than a newly lowered limit.

**Examples:**

| Global | Main | Main can run at once |
|---|---|---|
| 6 | 6 | 6 |
| 8 | 6 | 6 (the queue limit decides) |
| 6 | 3 | 3 (the queue limit decides) |

**Other changes:**
- The global and queue limits stay independent; no action changes one through the other.
- The note no longer mentions connection details.
- In Bangla the queue is named as the app names it ("প্রধান কিউ"), never "Main".
- Options also re-reads the queues when it opens, after a reconnect and after a reload (a new `reloadQueues`
  action). A queue list answer that arrives after a newer one is ignored.

### Scheduler verification (no defect found)

Five focused tests were added to `core::tests::concurrency`, which now has 10:
- global 1 with Main 6 runs 1; global 6 with Main 3 runs 3; global 8 with Main 3 still runs 3; global 6 with Main 6
  runs 6;
- two queues together stay within the global limit;
- completion, pause, cancel, resume and queue stop/start release and reuse slots;
- a restart keeps the global and Main limits;
- a durable save refused by the database (a trigger) changes neither the saved nor the effective limit.

The last test was mutation-checked: applying the limit before the save makes it fail.

### Defects found and fixed during verification

1. **Options:** the new `h4` heading kept the browser's default margin, because base.css resets only h1–h3. Seen in
   a screenshot of the packaged window; fixed in the component CSS.
2. **Packaged smoke:** a run aborted with "Unexpected end of JSON input". The script parsed a JSON-lines file while
   the desktop was still writing a line (`writeln!` can write the text and the newline separately). Five readers
   now use `readJsonl`, which parses only newline-terminated lines.
3. **Acceptance oracle, close detection:** the Node fixture noticed a closed socket only at its next 64 KiB write, so
   a 120 ms step let eight closes register 165 ms late (overlap 9). The fixture gained a `chunk` option, and the
   acceptance writes 16 KiB steps every 30 ms (the same rate).
4. **Acceptance oracle, server-side count:** one run (`concurrency-b231-final3`) still counted 9 server-side in T1,
   while F1 of the same run counted 8.
   - The host budget is now asserted **client-side**: the packaged service's ESTABLISHED TCP connections to the
     fixture, sampled from the OS every 300 ms.
   - The server-side figure is kept as a detail.
   - The 9 itself is not explained. The exact Rust permit measurement (C06/C07: permits = 8) and the TCP sampling
     (max 8) both stay at 8.

### Native and packaged evidence (Windows 11 Pro 10.0.26300 x64)

| Check | Result |
|---|---|
| `pnpm verify` (full, before the h4/harness fixes) | **PASSED 20/20** |
| `pnpm verify -- --only=structure,format,lint,typecheck,test` (final sources) | **PASSED 9/9**; desktop **206** tests, extension 81 |
| `cargo test -p dm-service --lib core::tests::concurrency` | PASSED 10/10 |
| `cargo test -p dm-service --test http_backend_process a05` | PASSED (overlap 8, 8 after the restart) |
| `cargo test -p dm-desktop --lib concurrency` (the desktop adapter) | PASSED 2/2 |
| `cargo test -p dm-backend-http --test connection_budget` | PASSED 8/8: C06/C07 permits 8 (= cap), C08 plain requests 2 alone and 2 contended |
| Packaged smoke, final smoke package (`smoke-results/windows-x86_64-b231b`) | **verdict passed, 95/95**; P2 0 px at every zoom; `probe_version` b2.31 |
| Packaged smoke, earlier package (`windows-x86_64-b231`) | FAILED: the run aborted at 26 rows (the JSON-lines race, fixed above) |
| Concurrency acceptance, final normal package: `concurrency-b231-final4` and `final5` | **verdict passed, 15/15 each**: client TCP max 8 (239–244 samples), server overlap 8, per-file range requests ≤ 4 |
| Concurrency acceptance, final package: `concurrency-b231-final3` | FAILED T1 only (server-side overlap 9, above); F1 passed with 8 |
| Concurrency acceptance, earlier packages of this step (`b231d`, `final1`, `final2`) | passed 15/15 (`b231`, `b231b`, `b231c` failed on script defects: an `Illegal invocation` in a language switch, a "Close"/"Done" button mix-up, and the 64 KiB close lag) |
| `pnpm contracts`, `pnpm check` | UNAVAILABLE (no such scripts) |
| macOS, Linux | NOT RUN locally; they need a pushed run |

**Real window, U4** (screenshots `acceptance-results/concurrency-b231-final1/options-limits-*.png`):
- global 6 and Main 3 → "Main can run at once 3", the reason, and **Change queue limit**;
- the button opened Manage queues; Main was set to 6 there; "Done" returned to Options, which then showed 6;
- the same in Bangla.

**Actual transfer** (T1, F1; six 32 MiB files from one host, range-capable, strong ETag):
- first bytes for all six within 16.5 s (`final4`) and 9.0 s (`final5`), before any completed;
- six window rows showing a percentage;
- all eight completed files match their SHA-256, after an abrupt service stop and recovery from checkpoints (R1).

No equal-bandwidth or IDM claim is made.

### Test package

`dist-packages/windows-x86_64/`: Windows **x86_64** (`x86_64-pc-windows-msvc`), **unsigned**, production
configuration (no smoke hooks; production extension). Built 2026-11-04T04:23:18Z from `e0609bc` plus 18 uncommitted
paths (diff SHA-256 `4c64b181007f050bae5f875d2214c594518f4c7d40b6663989fc8a6fbd232e56`). After the build, only
scripts and docs changed.

| File | Size | SHA-256 |
|---|---|---|
| `Downova_0.1.0_x64-setup.exe` (NSIS, per-user installer) | 5,953,061 B | `371617d06eb8e06537f0fef7d81eb59c8efd42e9b1f065c570afea2d99a18c61` |
| `Downova_0.1.0_x64_en-US.msi` | 9,138,176 B | `b8fcb0fe0f458cfca575b1dce92ef3aa394d69d2f43e5bbe4951b209fbcb7e09` |

**Launched:** yes. The MSI contents (desktop, service, `dm`) were extracted with `msiexec /a` and exercised by the
acceptance runs above.

**Not run: its `setup.exe`.**
- Running it outside the smoke harness would register shortcuts and the browser helper in this Windows profile,
  affecting the real Chrome and Edge.
- No Downova installation was registered for this user, and none was created.
- The smoke harness ran the smoke package's `setup.exe` (the same installer code) in an isolated location that it
  restores.
 (2026-11-02, Windows 11 Pro 10.0.26300 x64)

**Base:** `2d999b9` (the user's commit of the integration). This step is uncommitted on top of it.

### Remote results for `2d999b9` (public API; readable thanks to the B2.28 annotations)

| Run | Result |
|---|---|
| CI #48 (41432895173) | Windows and both Ubuntu jobs passed. **macOS failed:** `c06_six_files…` at `srv.max_overlap(TEARDOWN) <= 8` |
| Package smoke #38 (41432895229) | Windows and macOS passed. **Linux failed P2:** `blank_px_top: 6` at 200 % zoom in light and dark; all other P2 values passed. The same in #37 for `4fc3d66` |

### C06 on macOS: a measurement lag, now measured both ways

**Diagnosis on Windows, before any change** (C06 instrumented temporarily, 4 runs):
- the server-side peak of open sockets was 14–16;
- with a 20 ms close allowance it was 8–10, and from 50 ms upward it was exactly 8 every time.

**Cause:** the fixture recorded a connection's end only at its next write after a pause. Rotation makes the client
close (and a waiter open) connections often. The macOS runner's scheduling stretched that lag past the 100 ms
allowance. The two unexplained A05 overlap failures fit the same explanation.

**Changes (no product behaviour changed):**
- **Fixture:** `dm-test-fixtures`' slow-body writer now watches the socket while it pauses between chunks. A client
  EOF or reset ends the connection at once (a stray byte is kept for the next request head).
  - Measured again: the allowance needed fell to **5 ms**.
  - The raw peak (12–16) remains: it is the genuine close-then-reconnect handover within a few milliseconds.
- **Exact client-side measure:** `HttpBackend::peak_requests_per_host()` records the most request permits any host
  held at once. C05 asserts ≤ 1; C06 and C07 assert ≤ 8 and, under contention, exactly 8. The server-side
  assertion is kept and now reports its value and the permit count.
- **New C08:** a stream without range support takes the same plain requests (the probe plus one uninterrupted
  stream) alone and with four segmented files contending; it completes byte-exact, and permits stay ≤ 8.
  - Measured: plain requests 2 alone and 2 contended, against 68 rotating segment requests.
  - C08 is a regression guard; it was not mutation-tested.

### P2 on Linux: not reproducible here, diagnostics added

**Not reproduced:**
- The gap was 6 px at the top of the list, at 200 % only, on Linux WebKitGTK.
- Windows WebView2 measured 0 px in every position, theme and zoom: smoke runs `windows-x86_64-b224b` and
  `windows-x86_64-b230`.
- No WebKit engine or WSL is available on this machine, and downloading one needs the user's approval.

**What changed instead:**
- Following the instruction not to change the layout before reproducing the gap, the layout is unchanged.
- The probe now records, only when a band appears, its side (above or below), the scroller and header boxes,
  `offsetHeight`/`clientHeight`/`scrollTop`, the shown span, the first row's height, the rendered
  `aria-rowindex` range and the spacer heights.
- The next Linux run will show the cause.

**Bangla row count:** `aria-rowcount` 335 = 334 matches + 1 header row. This is correct and was not changed.

### UI: stale and failed saves (tests added; the code was correct)

Two tests were added, and each was checked by removing the guard it protects (the test then failed):
- **A rejected re-save of an unchanged value** drops the earlier "saved" status (the reset at save start). Typing
  had been clearing it on its own, so no earlier test reached that line.
- **A save reply from a service instance the window has since reconnected away from** is ignored (the epoch guard).
  Closing and reopening the dialog cannot test this, because unmounting already discards the reply.

Desktop tests: 197 (was 193).

### Packaged acceptance (new packages)

`node scripts/acceptance/concurrency-acceptance.mjs --package-dir dist-packages/windows-x86_64` →
`acceptance-results/concurrency-b230b/results.json`: **verdict passed, 14/14.**

The new rows since the integration run:

| Row | Observed |
|---|---|
| S1 | Collision policy saved in Options ("ask"), `dm history-set 30` (exit 0), `dm notifications-set on` (exit 0): the global limit stayed 6 |
| V1 | `dm concurrency-set 0` and `17` exited 2; the effective limit stayed 6 |
| W1 | After the abrupt stop and restart, the window reattached by itself: Options showed 16; the list had 8 rows, as the service did |
| C1 | A ninth download cancelled while transferring: `cancelled`; it received no further bytes (131,072 before and after) and was served none (262,144); no final file; the others continued |
| F1 | All 8 files matched their SHA-256; host overlap 8; **per-file range requests at once: exactly 4 for every segmented file** (the cap); 13–37 requests per file (rotation) |

**The other rows passed as before:**
- T1: first bytes for all six within 10.9 s, before any completed; six window rows with a percentage; host overlap 8;
- T2/T3: live lower and raise, admission after 4.2 s, a pause with no further bytes, a stopped queue held;
- R1: both limits persisted; recovery from checkpoints;
- U1–U4: one IPC save for a double Enter, invalid input not sent, Bangla, Main's limit set to 6.

### Packaged smoke (local equivalent of the workflow)

`smoke-results/windows-x86_64-b230`: **verdict passed, 95/95** on the smoke-test package built from the same source.

### Checks (Windows 11 Pro 10.0.26300 x64, `CARGO_BUILD_JOBS=2`)

| Command | Result |
|---|---|
| `pnpm verify` (full) | 19/20: every test group PASSED (Rust including `connection_budget` 8/8 and `http_backend_process` 50/50, native host e2e, four aria2 groups); FAILED only `prettier --check` on a test file being edited during the run |
| `pnpm verify -- --only=structure,contracts,format,lint,typecheck,test` afterwards | PASSED 10/10; desktop 197, extension 81 |
| `cargo test -p dm-service --test http_backend_process a05` | PASSED (overlap 8 overall, 8 after the restart) |
| `cargo test -p dm-desktop --lib concurrency` | PASSED (2 tests) |
| `cargo test -p dm-backend-http --test connection_budget` | PASSED (8 tests; C06 and C07 printed permits 8, server overlap 8) |
| `pnpm contracts`, `pnpm check` | UNAVAILABLE: no such scripts (`contracts:check` and the `pnpm verify` groups are the equivalents) |
| macOS and Linux native | NOT RUN locally; they need a pushed run (the C06 fix and the P2 diagnostics) |

### Test package

**Normal package:** `dist-packages/windows-x86_64/`, Windows x86_64, **unsigned**, production configuration (no
smoke hooks; production extension). Built 2026-11-02T12:09:56Z from `2d999b9` plus 9 uncommitted paths (diff
SHA-256 `233336611b18768e2ed94b67034ab407a8ebe9ad4d9e2e4eb787b5f07e5f2697`).

| File | SHA-256 |
|---|---|
| `Downova_0.1.0_x64-setup.exe` (NSIS, per-user installer) | `0ab90dea6733b87d275870d8fb50a6293d3459e2ca265d76988fa33f63e392a4` |
| `Downova_0.1.0_x64_en-US.msi` | `a36b0f6aaafa82fbc424cf6ef07edaece0ecd700d89530a0e8f8e66612b3f534` |

**What was exercised:**
- **This package's MSI contents** (desktop, service, `dm`) were launched and exercised by the acceptance above,
  extracted with `msiexec /a` into an isolated folder.
- **Its NSIS installer `setup.exe` was not run.** The packaged smoke installs the smoke-test package's NSIS
  installer, built from the same source in its own folder, and only that one was installed and exercised.
- The user's live installation was not touched.

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
