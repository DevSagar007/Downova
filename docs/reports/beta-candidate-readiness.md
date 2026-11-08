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

| Check |  ([CI](https://github.com/DevSagar007/Downova/actions/runs/42462944235), [smoke](https://github.com/DevSagar007/Downova/actions/runs/42462944252)) |  ([CI](https://github.com/DevSagar007/Downova/actions/runs/42464689519), [smoke](https://github.com/DevSagar007/Downova/actions/runs/42464689606)) |
|---|---|---|
| verify (windows-latest) | passed | **failed** in  (the pinned GitHub-release download), before any test |
| verify (macos-latest) | passed (hand-off 28/28, S4) | passed |
| verify (ubuntu-22.04) | passed | passed |
| verify (ubuntu-24.04) | passed (hand-off 28/28, S4) | passed |
| package (windows-x86_64) | passed | **failed** in the package build |
| package (macos-aarch64) | passed | passed |
| package (linux-x86_64) | passed | **failed** in the package build |

- ** is the first fully green seven.** Every check passed, including S4 on all three operating systems.
- **On , the three failures share one pattern.** They are exactly the jobs that download release assets
  from github.com during the job: aria2; NSIS, WiX and nsis_tauri_utils; linuxdeploy and the AppImage tools.
  Every job without such a download passed.
- The same  built the smoke package locally on Windows without error (exit 0, notices identical in the
  MSI).
- **Diagnosis: likely a transient download failure, NOT confirmed.** The step logs need a GitHub sign-in.
- **Next:** *Re-run failed jobs* on both runs (same SHA), then record the outcome. If a failure repeats, read that
  step's log.
