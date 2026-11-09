# Beta-candidate readiness: Linux reconnect CI, normal kits, native installer lifecycle

- **Date:** 2026-11-08. **Local host:** Windows 11 Pro 10.0.26300 x64 (a development machine with real Downova data;
  **not** an isolated environment).
- **Branch:** `fix/beta-candidate-readiness`, from `fe8b8bd`. That is `origin/main` `69a9068` plus the two B3.7
  commits of `fix/b3.7-linux-reconnect-ci` (the S4 fix).
- **Not authorized and not done:** no merge, no push to `main`, no release, no production key.
- **Release gates unchanged:** B2 is not exited, and the BRW-10 private-window decision is pending.

## 1. Linux S4 (the remaining CI failure on `69a9068`)

**Confirmed cause: runner setup.** The ubuntu-24.04 real-browser step ran the service without an OS credential
store. Evidence and the fix are in the [B3.7 report](b3.7-platform-ci-stabilization.md):
- **Store off:** the local Windows reproduction with the store off gives the CI failure exactly: checkpoint
  `sealed: 0`, `AUTH_REQUIRED`, no candidates.
- **Real store:** the real Chrome journey passes, S4 included. The sealed checkpoint was 2,686,976 bytes, nothing
  was requested before the explicit reconnect, and the first range was `bytes=2686976-…` with only the fresh
  cookie. SHA-256 exact, no leaks.

**The fix:**
- a verified disposable Secret Service in the CI step's own D-Bus session;
- S4 with a fixture gate, and a proven sealed checkpoint before the restart.

The persistent data key and the temporary cookie keep their different lifetimes. No test vault is used, and
nothing is persisted in plaintext.

## 2. What this batch adds

- **`scripts/acceptance/installer-lifecycle.mjs`:** the real installer lifecycle, with a hard isolation guard (a
  GitHub-hosted runner, or `DOWNOVA_DISPOSABLE_MACHINE=1`; refuses where Downova exists). Rows:

  | Package | Rows |
  |---|---|
  | NSIS | N1–N6 |
  | MSI | M1–M4 |
  | `.deb` | D1–D4 |
  | `.app` | A1–A3 |

  The rows cover:
  - install, then work (running, manually paused, stopped queue);
  - upgrade while the service runs, then the work continues (SHA-256);
  - uninstall with no owned process left, data and downloads kept, only Downova's registration removed (an
    unrelated registration must survive);
  - reinstall with the retained list.

  Processes are identified by installation path, never by name.
- **`.github/workflows/installer-lifecycle.yml`:** manual, three runners. Each builds the fixture pair, runs the
  harness, uploads `results.json`, and publishes every row as an annotation.
- **Docs:** testing and setup.

## 3. Results

| Evidence | Source | Status |
|---|---|---|
| Unit and integration (local verify, Windows) | `00ce723` tree | 24/24 passed (`cargo test` 613, 0 failed) |
| Real Chrome journey with S4 (local, Windows, real store) | the `3ae2732` tree | passed; the store-off control fails explicitly |
| Seven CI checks for the S4 fix | `e117e2f` or later on this branch | **NOT RUN:** the workflows must be dispatched (no API access from this session) |
| Smoke-feature packages | `69a9068` | 3/3 passed ([run 42455584846](https://github.com/DevSagar007/Downova/actions/runs/42455584846)) |
| Normal packages and kits, three targets (Acceptance kit workflow) | this branch | **NOT RUN:** dispatch needed. Windows only, locally from `6312d2e` (B3.6): kit `f882be31…` validated |
| Installed lifecycle (Installer lifecycle workflow) | this branch | **NOT RUN:** dispatch needed. The harness's isolation guard was checked locally (it refuses here) |
| Human acceptance (context menu, screen reader, Bangla review, tray, sleep/logout/reboot) | — | NOT RUN |

## 4. Remaining work

- **Dispatch needed** (Actions → Run workflow, branch `fix/beta-candidate-readiness`): **CI**, **Package smoke**,
  **Acceptance kit**, **Installer lifecycle**. Opening a draft PR also starts CI.
- **Owner decisions:**
  - the project license and the REL-03 notice gaps;
  - the NSIS install-folder layout (ADR 0031 §4);
  - BRW-10 private windows;
  - production update configuration.
- **Human checks:** the B2 installed pass and the B3 rows.
- **Next batch:** the IDM comparison and the remaining release configuration.

## 5. Resume contract and notice gaps (2026-11-08, branch `fix/resume-contract-and-notices`)

### S4 and the segmented resume contract

- **Confirmed cause of the S4 failures on `be09937`:** the harness required the first range after the reconnect
  to start at the **total** kept bytes.
  - A segmented checkpoint keeps `SUM(region.done)`, and each region resumes at `region.start + region.done` in
    any order. For example, the CI evidence: kept 1,900,544 bytes, first range `bytes=1245184-2097151`.
  - Every other S4 predicate held: a sealed checkpoint; restored bytes equal to it; 0 requests before the
    reconnect; the same job offered and reconnected; SHA-256 exact; no leaks.
  - This is a **test defect, not an engine defect.**
- **Engine contract proven precisely:** `dm-backend-http` `r02b_uneven_segment_progress_resumes_each_region_from_its_own_offset`.
  - A real segmented checkpoint is rewritten to uneven, non-contiguous region progress (fresh digests over bytes
    already on disk).
  - Every unfinished region must resume exactly at `start + done … end − 1` with `If-Range`, and the total is
    no region's start. Length and SHA-256 must be exact.
  - Passed 3/3 (Windows 11). The existing `r02` also passed.
- **S4 (browser) is mode-aware:**
  - **single connection:** one range starting at the kept bytes;
  - **segmented:** disjoint ranges, none from 0, together exactly the missing bytes;
  - **both:** every continuation validated with the fixture's ETag as `If-Range` (the fixture now logs it).
  - The checkpoint is sealed, so per-region offsets are proven by `r02b`, not by the browser harness. That
    boundary is deliberate: no checkpoint-decryption API.
  - Locally (Windows, real store) S4 passed: segmented, three disjoint ranges, exactly the 5,701,632 missing
    bytes, `If-Range` on all.

### Notice gaps

| | Before (`be09937`) | After (`1ff5539`) |
|---|---|---|
| Distributed components | 385 | 388 (+ WebView2 loader, Vite helpers, rolldown helpers) |
| With unresolved metadata | 24 | 14 |
| Excluded | 106 | 106 |
| Distinct texts | 239 | 251 |
| Inventory size | 956 KiB | 2,604 KiB (the Rust std notice is 1.5 MB; the viewer loads it lazily) |

**Resolved, from the exact shipped revision:**
- **Seven crates:** alloc-stdlib, dlopen2, libappindicator-sys, mac-notification-sys, selectors, webview2-com and
  webview2-com-sys.
  - Each crate's `.cargo_vcs_info.json` gives the commit; the files are fetched from that commit with their URLs
    and SHA-256 in `third_party/notices/overrides.json`.
  - **selectors (MPL-2.0):** servo/stylo has no MPL file at `572ecba2`, and its source headers point to
    mozilla.org. The text is from https://www.mozilla.org/media/MPL/2.0/index.txt, and the source offer is
    https://github.com/servo/stylo/tree/572ecba2d1600e7c3d490586692a209faf703baa/selectors.
- **Rust std:** the full `COPYRIGHT-library.html` (rustc 1.99.1 `58cf8f9c`, sha256 `5647be07…`). The viewer shows
  it as text; HTML is never rendered.
- **NSIS:** the Tauri template uses `SetCompressor /SOLID` with the default lzma, so Zlib AND CPL-1.0 (bzip2 not
  used).
- **nsis_tauri_utils:** 0.5.3, SHA-1 `75197fee…` matches the Tauri CLI pin; MIT OR Apache-2.0 from tag commit
  `13d9edd2`.
- **WebView2 loader:** `WebView2LoaderStatic.lib` is statically linked through webview2-com-sys. It is
  byte-identical to Microsoft.Web.WebView2 1.0.3800.47 (nupkg sha256 `56c9f26b…`). `LICENSE.txt` (BSD-3-Clause)
  and `NOTICE.txt` are included.
- **Web bundle helpers:** Vite 8.3.4 (modulepreload polyfill, `__vitePreload`) and rolldown 1.2.14 (CommonJS
  interop), with their license files.

**Still unresolved (14), kept explicit:**
- **13 objc2-family crates** (macOS only): block2, dispatch2, objc2, objc2-encode, objc2-exception-helper,
  objc2-foundation and 7 framework crates.
  - At the exact commits, upstream has only a `LICENSE.md` licensing statement, with links and no license text
    or copyright line.
  - It is included, but an MIT notice needs a copyright line that upstream does not provide.
  - Owner options: ask upstream; ship the statement plus the standard license texts and note the missing line;
    or replace the crates.
- **WiX:** the `-ext` arguments are inferred from the Tauri CLI binary, not decoded from its source.

**Generator safeguards:** an override is applied only to that exact `name@version` when the crate ships no
license file. A wrong checksum, a stale entry or an unneeded entry fails generation; both failures were exercised.

**Project license:** not chosen. See the [proposal](../decisions/proposals/project-license.md): four options and
the obligations that hold whatever is chosen. REL-03 stays `in-progress` (14 items, the license, legal clearance).

### Seven checks (canonical runs on main)

| Check | `70e16a6` ([CI](https://github.com/DevSagar007/Downova/actions/runs/42462944235), [smoke](https://github.com/DevSagar007/Downova/actions/runs/42462944252)) | `38fd4a7` ([CI](https://github.com/DevSagar007/Downova/actions/runs/42464689519), [smoke](https://github.com/DevSagar007/Downova/actions/runs/42464689606)) |
|---|---|---|
| verify (windows-latest) | passed | **failed** in `Windows aria2` (the pinned GitHub-release download), before any test |
| verify (macos-latest) | passed (hand-off 28/28, S4) | passed |
| verify (ubuntu-22.04) | passed | passed |
| verify (ubuntu-24.04) | passed (hand-off 28/28, S4) | passed |
| package (windows-x86_64) | passed | **failed** in the package build |
| package (macos-aarch64) | passed | passed |
| package (linux-x86_64) | passed | **failed** in the package build |

- **`70e16a6` is the first fully green seven.** Every check passed, including S4 on all three operating systems.
- **On `38fd4a7`, the three failures share one pattern.** They are exactly the jobs that download release assets
  from github.com during the job: aria2; NSIS, WiX and nsis_tauri_utils; linuxdeploy and the AppImage tools.
  Every job without such a download passed.
- The same `38fd4a7` built the smoke package locally on Windows without error (exit 0, notices identical in the
  MSI).
- **Diagnosis, later confirmed:** a GitHub release-asset outage (HTTP 504). See section 6.
- **Next:** *Re-run failed jobs* on both runs (same SHA), then record the outcome. If a failure repeats, read that
  step's log.

## 6. Reliable build-tool downloads (2026-11-08, branch `fix/reliable-tool-downloads`)

### Cause of the three failures on `38fd4a7` and `e7309fe`

- **GitHub release assets answered HTTP 504 (Gateway Timeout).**
  - The job logs need admin rights, so the URLs were tested directly from the development machine. aria2 1.37.0,
    `wix314-binaries.zip` (wix3141rtm) and `nsis_tauri_utils.dll` (v0.5.3) all answered 504.
  - The owner's job evidence names the failing downloads: `scripts/fetch-aria2.mjs` (Windows verify job
    128688731895); Tauri's `nsis-3.13.zip` (Windows package job 128688731823); Tauri's
    `linuxdeploy-x86_64.AppImage` after the AppRun download (Linux package job 128688731608).
  - `70e16a6` passed all seven checks earlier with the same pins.
  - `e7309fe` is a docs-only merge that failed the same way. So the cause is the outage, not the source, but a
    code-independent retry is not a reliability fix.
- **Still failing hours later:** a live `node scripts/fetch-aria2.mjs` on the development machine got 504 on five
  bounded attempts and stopped with an actionable message, leaving the existing install untouched.

### What changed

- **`scripts/lib/download.mjs`:**
  - finite attempts (5), a per-attempt timeout (120 s), a total budget (10 min) and backoff of 5, 10, 20, 40 s,
    then 60 s;
  - `Retry-After` is honoured only within the remaining budget;
  - retries network errors, timeouts and HTTP 408, 429 and 5xx only;
  - a size or SHA-256 mismatch, or any other status, fails immediately;
  - logs name the tool, the attempt and the URL without query or credentials (signed redirects carry tokens there).
- **`scripts/fetch-aria2.mjs`:**
  - the same pin (1.37.0, exact size, SHA-256) through that downloader;
  - extraction is staged and files are renamed into place;
  - `tools/aria2/.pin.json` records the installed digests. A later run reuses the folder only if every file still
    matches, so a partial or corrupt copy is replaced, never trusted because its name exists.
  - aria2 stays an optional external backend, never bundled.
- **`scripts/package.mjs`:** the pinned Tauri CLI downloads its own tools and verifies them (it reports "hash
  mismatch of downloaded file"; its HTTP client is ureq 3.x). That cache layout cannot be pre-filled without
  guessing, so only `tauri build` is re-run:
  - at most 3 attempts, with 30 and 90 s pauses;
  - only when its output **positively identifies** a transient download of a known bundling tool (NSIS, WiX,
    nsis_tauri_utils, linuxdeploy, AppRun) together with a transient error (`http status: 408/429/5xx`, `timeout:`,
    a connection reset or refused, `host not found`);
  - compile errors, hash mismatches, bundling, notices or version failures stop at once;
  - cargo's incremental build makes a re-run cheap.
- **Validated caches (workflows):**
  - **aria2** (`tools/aria2`, key = pin SHA-256), validated by `.pin.json`;
  - **Tauri's Windows tool folder** (`%LOCALAPPDATA%\tauri`; key = CLI 2.12.3, NSIS 3.13, WiX 3.14,
    nsis_tauri_utils 0.5.3) in package smoke, acceptance kit and installer lifecycle. After a build,
    `scripts/lib/tool-cache.mjs snapshot` records every file's SHA-256 per entry. After a restore, `validate`
    removes only the changed or unrecorded entries, so Tauri re-downloads and re-verifies just those; without a
    manifest nothing restored is trusted.
  - **Linux tool cache:** its location could not be confirmed locally, so Linux has no tool cache, only the retry.
  - **Seven-check policy unchanged:** automatic on `main` only; acceptance kit and installer lifecycle manual.

### Local verification (Windows 11)

- `node --test scripts/lib/download.test.mjs`: 9 passed, with no live network dependency:
  - 504 then a valid response over real HTTP;
  - repeated timeouts reach the limit;
  - 404 fails at once;
  - a checksum mismatch is not retried;
  - a `Retry-After` beyond the budget stops;
  - the URL is sanitized;
  - failure classification (hash mismatch, compile error and a tool-less 504 are never retried);
  - a corrupt or partial aria2 install is rejected;
  - a changed or unrecorded cached tool entry is removed alone.
- The workflow parse checks, `pnpm verify --only=structure,format,lint` (6 passed) and a real normal package
  build through the new wrapper: see PROGRESS for the result.

### Not covered locally

- A live transient outage that ends within the retry window cannot be produced on demand. The current outage
  outlasted it, as designed: a bounded failure. The retry branch itself is covered by the deterministic tests.

## 7. Candidate `a80019a`: seven checks and acceptance evidence (2026-11-08)

**Candidate:** `main` = `a80019a2d1e9a6d859f1d4c37827cf121904677e`.

### Seven checks (automatic, on main)

| Check | Result | Run |
|---|---|---|
| verify windows-latest, macos-latest, ubuntu-22.04, ubuntu-24.04 | all passed | [CI 42472217944](https://github.com/DevSagar007/Downova/actions/runs/42472217944) |
| package windows-x86_64, macos-aarch64, linux-x86_64 | all passed | [Package smoke 42472217984](https://github.com/DevSagar007/Downova/actions/runs/42472217984) |

- The earlier HTTP 504 failures on `38fd4a7`, `e7309fe` and `5d80463` are historical.
- On `a80019a` the package builds succeeded, so the new failure-log annotation did not fire. This run therefore does
  not show whether the retry or cache helped, or whether the outage had simply ended.

### Manual workflows: not run

- Acceptance kit and Installer lifecycle have **no runs**: `acceptance-kit.yml` and `installer-lifecycle.yml` report
  0 runs, so the owner's dispatch did not reach GitHub.
- GitHub's REST API itself answered HTTP 504 at the time.
- The NSIS N1–N6, MSI M1–M4, DEB D1–D4 and macOS A1–A3 rows, and the macOS and Linux kits, are therefore **NOT
  RUN**.
- **Next:** Actions → Acceptance kit → Run workflow (branch `main`), then the same for Installer lifecycle, and
  confirm each run appears in the run list.

### Windows normal package and kit (local build, development machine)

- **Package** from the clean `a80019a` (0 uncommitted paths; `build_kind` normal; `updater` none; desktop, service,
  CLI and host all 0.1.0; MSI payload: notices identical, all binaries present with matching versions):

  | File | SHA-256 |
  |---|---|
  | `Downova_0.1.0_x64-setup.exe` | `e2b1ad795a195cfaf5de86328968b1863020ed229cd8d4ad5ae9138bc1e38d7a` |
  | `Downova_0.1.0_x64_en-US.msi` | `a58440631174442550e4acba7d39535714b0a8b61cad8bac3b59cd44426566da` |
  | `downova-firefox-unsigned.xpi` (temporary loading only) | `ae68456fb81f3297dafe81b87b63a5533d705d24d3054a8b479c464601273be8` |
  | `THIRD-PARTY-NOTICES.txt` | `94c267ffdc63c0e5c1276601abde548b7b807f241de0581083337756c077c9b1` |

- **Kit:** `acceptance-kit-b2-windows-x86_64.zip`, SHA-256
  `1d8d74d27f95372e79c32c65497fef6dc39ca67101ed25d215613186e8fb552a`, 29 files, 66 rows. Built with the source check
  on; `validate-kit-zip.mjs` passed (archive, file list, `tar` extraction, `verify-kit`, both fixtures).
- **Status:** an internal unsigned test build. It is a local artifact, not uploaded by a workflow.

### Automated packaged acceptance on that package (MSI extracts, isolated data; not an installation)

- **`concurrency-acceptance.mjs`: 14 of 14 passed.** It drove the real packaged window and service:
  - global limit 6 by keyboard; invalid 17 refused; Bangla Options; other settings keep the limit;
  - Main queue 6 through Manage queues: six 32 MiB downloads active before any completed, at most 8 host requests;
  - lowering to 2 preempts nothing and a new download waits; raising starts it;
  - the manual pause and the stopped queue hold;
  - an abrupt service stop and restart keeps the limits and resumes from checkpoints, and the window reattaches;
  - cancel under contention;
  - all eight files match their SHA-256.
- **`browser-lifecycle-acceptance.mjs`: B1–B7 passed.** Firefox package contents; registration; repeatable repair;
  another installation's entry respected; update in place; removal of only our entries; per-package status.
- **`secure-auth-acceptance.mjs`: Q1–Q5 passed, plus two leak checks.**
  - a protected link is sealed;
  - a remembered signed-in hand-off stores metadata only;
  - another origin and http are refused;
  - Forget pauses the download;
  - a reconnect with a fresh sign-in resumes the same job;
  - no secret is in stderr or the data folder.

  The run's Credential Manager entries were deleted.

### Still NOT RUN

- **Installed-app rows**, which need a disposable machine or test account: IL1–IL6, UP2–UP3 and the Installer
  lifecycle rows above.
- **macOS and Linux** kits and installs.
- **Human rows, all in `docs/acceptance/b2-checklist.md` with steps and expected outcomes:**
  - real right-click hand-off (C/F rows), tray and notifications (N rows);
  - Bangla copy review, screen reader (A rows), display scaling;
  - sleep, logout and reboot (L3 and K rows).
- **Owner decisions:** project license, the 13 objc2-family notices, BRW-10, the NSIS install folder, production
  release configuration.

## 8. Two timing-dependent correctness tests (2026-11-08, branch `fix/flaky-r02b-s04`)

`main` `c874c32` was docs only. Its [package smoke 42476539141](https://github.com/DevSagar007/Downova/actions/runs/42476539141)
passed 3/3. In [CI 42476539159](https://github.com/DevSagar007/Downova/actions/runs/42476539159) the two Ubuntu jobs
passed and two tests failed:

| Test | Failure | Confirmed cause | Category | Fix |
|---|---|---|---|---|
| `r02b_uneven_segment_progress_resumes_each_region_from_its_own_offset` (Windows job 128719043448) | geometry guard: confirmed 354,986 equals the first expected range start | The fixture lowered whatever progress the real checkpoint had. At the 1 MiB threshold only region 0 had progressed, so the "uneven" total equalled region 0's resume offset and the guard stopped the test before the product assertion ran | test fixture | Known body bytes are written into the real partial file at fixed, uneven, non-contiguous offsets per region (300,001 / 0 / 1,200,007 / 77,777), with matching digests. The real checkpoint now supplies only the destination, identity and region layout. Checkpoint format, validator, partial-file integrity, the guard and the per-region, `If-Range`, length and SHA-256 assertions are unchanged |
| `s04_a_live_increase_applies_without_a_new_request` (macOS job 128719043667) | 110,486 B/s for a 100,000 B/s limit (+10.5 %) | Progress advances in parts of up to the bucket capacity, a quarter second of the rate (ADR 0024: 25,000 B at 100 kB/s). Two samples 1.5 s apart can each be off by almost a part: about ±17 % at 100 kB/s, ±10 % at 400 kB/s. The limiter's accounting is covered by the virtual-time bucket tests | measurement design | `measured_rate` samples every 5 ms and measures between the first and last progress change in the window. Both ends are part completions, so end quantization cancels. The transfer must still be active after both windows. Tolerance stays ±10 %, and the no-new-request, length and SHA-256 assertions are unchanged |

**Stability (Windows 11, local, normal parallel test execution):** 5 predetermined repeats of both full suites,
`durable_resume` (10 tests) and `speed_limits` (7 tests): **5/5 passed, no failures.**
- r02b: total 1,577,785 every time; each region continued at `start + done`.
- S04: 94,952–97,495 B/s at 100 kB/s and 377,936–391,725 B/s at 400 kB/s.
- Clippy and fmt are clean.

**Remote:** the branch commit `73b8d91` could **not be pushed**, because git had no GitHub credential in this
session ("could not read Username"). It has no CI run.

**Manual workflows:** `acceptance-kit.yml` and `installer-lifecycle.yml` still have **0 runs**, checked through the
public API by workflow file. The macOS/Linux normal kits and all installer rows remain NOT RUN.

## 9. Candidate `dc2ec71` (2026-11-09)

- The `73b8d91` commit reported as unpushed in §8 was later pushed and merged; `main` = `dc2ec71` includes it.
- **Seven checks on `dc2ec71`:** [CI 42479556011](https://github.com/DevSagar007/Downova/actions/runs/42479556011)
  passed on all four verify jobs (r02b and S04 included), and
  [package smoke 42479556022](https://github.com/DevSagar007/Downova/actions/runs/42479556022) passed 3/3.
- **Manual workflows:** `acceptance-kit.yml` and `installer-lifecycle.yml` still report **0 runs** (public API, 2026-11-09).
  The development session could not dispatch them (no stored GitHub credential; the built-in browser is not signed in).
  The macOS/Linux normal kits and installer rows N1–N6, M1–M4, D1–D4 and A1–A3 remain **NOT RUN**.
- Human rows and owner decisions are unchanged from §7.

## 10. First remote installer lifecycle and acceptance kits (2026-11-09)

**Manual workflows now run.** The owner's earlier dispatches never reached GitHub, because the browser accounts
used (`yeasherarafath`, `teamtdevsfront`) have no write access. Dispatched from the DevSagar007 account:

| Run | Source | Result |
|---|---|---|
| [Installer lifecycle 42566142231](https://github.com/DevSagar007/Downova/actions/runs/42566142231) | `c9afed7` | Linux D1–D4 passed; Windows N1–N6 passed, M1 failed; macOS A1 passed, A2/A3 failed |
| [Acceptance kit 42566054802](https://github.com/DevSagar007/Downova/actions/runs/42566054802) | `c9afed7` | Windows and macOS kits succeeded; Linux build failed (HTTP 504 on Tauri's linuxdeploy/AppRun downloads, 3 attempts) |
| [Installer lifecycle 42568238477](https://github.com/DevSagar007/Downova/actions/runs/42568238477) | `a7c319f` | **Windows N1–N6 and M1–M4 passed, Linux D1–D4 passed**; macOS A1 passed, A2/A3 failed (cause found) |
| [Acceptance kit 42568292024](https://github.com/DevSagar007/Downova/actions/runs/42568292024) | `a7c319f` | Windows and macOS kits succeeded; Linux build failed again (504 on AppRun) |

**Defects and fixes:**
- **M1 (harness):** the MSI installs per user to `%LOCALAPPDATA%\Downova`, and the harness looked only in
  Program Files. `a7c319f` now finds the folder from the Uninstall entry; M1–M4 then passed.
- **A2/A3 (product, ADR 0032):**
  - B's service started but never served. Startup read the data key from the Keychain synchronously, before
    binding IPC.
  - The builds are ad-hoc signed, so the Keychain asked whether B may read A's item, and no one answers that
    prompt on CI.
  - **Fix:** store calls are bounded (10 s), and an unanswered store is treated as locked. Test
    `a_store_that_never_answers_does_not_stop_the_service_from_starting`.
- **Linux kit (infrastructure):** GitHub release-asset 504s. Both assets downloaded fine when probed later.
  - `tool-cache.mjs` now also records single files.
  - The three build workflows cache and validate `~/.cache/tauri` on Linux.
  - The cache's location is Tauri's documented cache folder, not yet confirmed by a run.

**Local checks (Windows 11, 2026-11-09):** `cargo test --workspace` 616 passed, 0 failed (617 after the sealed-progress test); `cargo clippy --workspace --all-targets -D warnings` clean; `pnpm verify --only=structure,contracts,format,lint` 7/7 PASSED; `download.test.mjs` 9/9 (tool cache with single-file entries). macOS and Linux behaviour of the fix: not run locally; the reruns are the check.

**Reruns on `1fa1f3b` (store-wait fix):**

| Run | Result |
|---|---|
| [Acceptance kit 42589426037](https://github.com/DevSagar007/Downova/actions/runs/42589426037) | **Windows, macOS and Linux kits all succeeded**, each including the in-job kit ZIP validation. Kit hashes are not yet read from the artifacts, which need a signed-in download |
| [Installer lifecycle 42589351965](https://github.com/DevSagar007/Downova/actions/runs/42589351965) | Windows N1–N6, M1–M4 and Linux D1–D4 passed again; macOS A1 and **A3 passed**; A2 failed |

**Why A2 still failed:**
- **What worked:** B's log reads "the credential store did not answer within 10 s", then "dm-service ready". The
  service served, and the pause and the stopped queue were kept.
- **What didn't:** `run1`'s progress was sealed, and without the key the start failed it as "interrupted", for
  good.
- **Fix (ADR 0032, second part):** sealed progress under a locked store is held as `CREDENTIAL_STORE_UNAVAILABLE`
  / Retry in memory only, and the next start with the key continues it. Covered by
  `sealed_progress_survives_a_start_without_the_key`, which fails without the change.
- **Limit:** on CI no one answers the Keychain prompt, so macOS A2 cannot reach "B finishes the download" while
  the two fixture builds are ad-hoc signed. That needs both signed by one disposable identity.

**Run on `f95aa46`:** [Installer lifecycle 42593354264](https://github.com/DevSagar007/Downova/actions/runs/42593354264): Windows and Linux passed again, macOS A1 and A3 passed, A2 failed. B started after the 10 s store wait and served (`hello` 0.1.1), and the pause and stopped queue were kept. `run1` shows phase `failed`, but the harness did not yet record the error kind, so this run cannot show whether it is the held store-locked state or the old interrupted failure. The harness now records `run1.error` (kind, next action).

**Exact next task:**
1. Rerun **Installer lifecycle** on the new `main`: A2 should record `run1.error` as `CREDENTIAL_STORE_UNAVAILABLE` / Retry.
2. Sign both macOS fixture builds with one disposable identity in the installer-lifecycle workflow, so A2 can
   check that B continues the work.
3. Read the kit hashes from the artifacts; human rows per `CHECKLIST.md`; owner decisions (license, objc2
   notices, BRW-10, NSIS folder, release configuration).