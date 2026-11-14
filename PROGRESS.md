# PROGRESS — handoff

## Current: B4 F02 clipboard monitor and F03 drag and drop on `feat/b4-clipboard-dragdrop` (2026-11-14)

Branch from `main` `b01ace3` (PR #7 merged). **B2 is not exited. Not release-ready.**

**F02 clipboard monitor (`implemented`, Windows evidence only):**
- **Settings:** off by default (Options → Clipboard, kept in the window's preferences).
- **Reading:** while on and the service is reachable, the desktop reads the clipboard every 2 s through
  `clipboard_text`. The text present when monitoring starts is never offered.
- **What is offered:** new http(s) links that name a file (the categories' extensions). The same text, or a link
  already offered this session, is not offered again, and sites can be ignored.
- **Prompt only:** "Download…" opens Add URL prefilled (several links: the batch dialog); nothing starts from a copy.
- **Privacy:** the copied text is neither kept nor sent.

**F03 drag and drop (`in-progress`):**
- **Config:** `dragDropEnabled: false`, so the webview receives HTML drops (Tauri's own handler only gives file
  paths).
- **Results of a drop:**
  - one link: Add URL prefilled;
  - several links, or one .txt/.lst/.csv list file: the batch dialog;
  - anything else: a message with Paste links… and Import list….
- **Not done:** the floating drop target is not built, and drag-out is not offered on any OS.

**Evidence (Windows 11):**
- **Tests:** `intake.test.ts` (5) and `app/intake.test.tsx` (3).
- **Native window (source build, isolated data):**
  - drops through CDP `Input.dispatchDragEvent`: one link, several links, plain text;
  - real clipboard: the existing text is not offered, a page link is not offered, a file link is prompted, and
    nothing is added by itself.
- **Checks:** `pnpm verify --only=structure,contracts,format,lint,typecheck,test` 12/12; clippy clean.

**NOT RUN:**
- a real OS drag from a browser into the window (human row);
- macOS/Linux;
- the prompt while the window is hidden. By design it is shown when the window is next opened, with no focus
  stealing.

**Exact next task:** merge after review. Then F08 (shutdown/sleep after downloads) and the basic HTTP proxy (F06).

## Previous: P1 IDM-parity controls and P2 fixes on `feat/batch-import-properties` (2026-11-14)

Report: [docs/reports/idm-parity-2026-11-14.md](docs/reports/idm-parity-2026-11-14.md). Branch from `main` `cb05a43`
(PR #6 merged). **B2 is not exited. Not release-ready.**

**Done (each with tests that fail without it):**
- **Add batch download from clipboard and Import download list:**
  - the clipboard is read by the desktop (`clipboard_text`, `tauri-plugin-clipboard-manager` 2.5.0), because
    `navigator.clipboard` is refused in the Tauri window;
  - links are parsed and checked like Add URL, each sent once with its own key, with per-link results.
  - Notices: `clipboard-win` has no license file, so its BSL-1.0 text is pinned from the exact upstream revision;
    still 14 unresolved.
- **File → Properties:** read-only facts from the service's snapshot.
- **Delete with the file (ADR 0033, amends ADR 0013):** `RemoveParams.delete_file`, completed downloads only; the
  service deletes only the download's own regular file, first, and keeps the job if that fails. Desktop: an
  unticked box in the Remove confirmation. CLI: `dm remove --delete-file`.
- **Redirect loop:** now `LINK_REFRESH_REQUIRED` with a reason (NB05; D05's expectation updated deliberately).
- **Corrected findings (not defects):**
  - 429/500 are retried automatically with `Retry-After` (1 + 3 requests);
  - a truncated body is shown as "the connection failed or was interrupted".

**Native checks (Windows 11, source build, isolated data, driven over WebView2 CDP):**
- clipboard batch: 2 added;
- list import: 2 added;
- all 4 files SHA-256 OK;
- Delete with the box ticked: file deleted, neighbour kept, job gone, announcement correct.

**Checks (Windows 11):**
- `cargo test -p dm-service -p dm-cli -p dm-domain -p dm-desktop`: 384/0;
- `cargo test -p dm-backend-http`: 130/0;
- `cargo clippy --workspace --all-targets -D warnings`: clean;
- `pnpm verify --only=structure,contracts,format,lint,typecheck,test`: 12/12.

**Open:**
- **IDM gaps, already requirements:**
  - F02 clipboard monitor;
  - F03 drag and drop;
  - F08 shutdown after downloads;
  - F06 proxy;
  - F05 FTP/SFTP;
  - F16 high contrast;
  - quotas (no requirement yet).
- **Deferred:** F09 grabber, F10 sync, E01 video.
- **NOT RUN:**
  - every IDM-observed comparison;
  - macOS/Linux windows;
  - installed human rows;
  - performance.
- **Not yet reproduced:** brief duplicate list keys during rapid adds.

**Exact next task:** merge this branch after review. Then start B4 with F02 (clipboard monitor) and F03 (drag and
drop), which reuse the batch dialog.

## Previous: Add URL investigation, web-page refusal, wired toolbar commands (2026-11-14)

Branch `fix/add-url-and-toolbar-parity` (from `main` `5832f7e`). **B2 is not exited. Not release-ready.**

**The reported failure ("Add new download → Address (URL) does not start a download") was not reproduced with an
ordinary file link.**
- **What was running here:** a leftover NSIS install of the `2ede2b2` kit in
  `%TEMP%\downova-smoke-65jkhB\install`, run with the user's real data folder. Its data held no job from the
  attempt, so the add never reached the service. That install was removed while the agent restarted its window,
  so the exact case could not be replayed.
- **Browser preview** (`pnpm dev`, `localhost:1420`): the toolbar Add URL, Tasks → "Add new download…" and
  Ctrl+N are all disabled there and say why. The dialog cannot open, so the user was in a native window.
- **Same build, native window:** the `2ede2b2` kit's MSI extract, isolated data, driven over WebView2 CDP. The
  Add button and form submission both added and completed a 3 MiB fixture with the matching SHA-256
  (`3b35763c…abf2e`).
- **Missing to diagnose:** the exact URL and the text the dialog showed.

**Add URL matrix through the real dialog** (native window, local fixtures, Windows 11):
- **Completed, with each file's SHA-256 equal to the fixture:** plain, redirect, query string, no extension,
  Unicode/percent-encoded name, Content-Disposition (`report final.pdf`), HEAD refused, unknown length, Range
  ignored, surrounding whitespace.
- **Refused in the dialog with a clear message:** `ftp://`, text with spaces, empty input.
- **Duplicate:** the existing download was offered.
- **Server errors:** 401 → `AUTH_REQUIRED`/sign in; 403 and 404 → `LINK_REFRESH_REQUIRED`; 429 and 500 →
  `SERVER_BUSY`/wait.

**Fixed:**
- **P0 (HTML page saved as the file):** an HTML page answered for a link naming a file (`login.zip`, or
  `setup.exe` redirected to a sign-in page) was completed and saved as that file. The engine now refuses it as
  `LINK_REFRESH_REQUIRED`, with "the server sent a web page instead of the file". A real `.html` page, an
  extensionless address, and an attachment the server names still download.
  - Owner: `crates/backend-http/src/transfer.rs` `web_page_instead_of_file`.
  - Test: `naming.rs` NB04, which fails without the change.
  - Rechecked in the native window from source: `login.zip` failed with `LINK_REFRESH_REQUIRED`, shown in the
    list with the refresh-link explanation; every file case above completed again with the matching SHA-256.
- **P1 (controls never connected):** toolbar Stop all, Delete completed and Scheduler, and Downloads → Stop all,
  Delete all completed, Scheduler… and Speed limiter…, were permanently disabled with "connected in a later
  milestone".
  - **Now:**
    - Stop all pauses every pausable download;
    - Delete completed opens the existing Clear finished confirmation (it also lists failed and cancelled
      downloads, unlike IDM's completed-only command);
    - Scheduler opens Manage queues, where schedules are set;
    - Speed limiter opens Options, with its Speed limit section.
  - **Tests:** `App.test.tsx`.
  - **Native check:** two slow downloads paused by one Stop all click.

**Checks (Windows 11):**
- `cargo test -p dm-backend-http -p dm-test-fixtures`: 129 passed.
- `cargo test -p dm-service`: 265 passed.
- `cargo clippy --workspace --all-targets -D warnings`: clean.
- `pnpm verify --only=structure,format,lint,typecheck,test`: 11/11 PASSED.

**IDM 6.43 build 16 (Trial), installed on this machine:**
- **Inventory:** taken from the official feature page and the help index, and set against every Downova menu
  entry, toolbar button and Options section read from the window.
- **Side-by-side comparison: NOT RUN.** IDM's command line (`/d … /p … /n`) wrote no file within about a minute
  and its window could not be observed. The 18 test entries may remain in IDM's list.

**Open (ranked):**
- **P1:**
  - Tasks → "Add batch download from clipboard" and "Import download list…": not connected (IDM has both).
  - File → Properties: not connected.
  - Delete with files: not offered.
- **P2:**
  - redirect loop reported as `INVALID_REQUEST` without an action;
  - a truncated response reported as `NETWORK_OFFLINE`;
  - 429/500 end as failed instead of retrying automatically after `Retry-After`;
  - brief duplicate job IDs in the window's list during rapid adds (a React key warning; the final list is
    correct).
- **MISSING compared with IDM:**
  - FTP/MMS;
  - site grabber;
  - video capture from web pages;
  - clipboard monitoring;
  - drag and drop;
  - proxy and NTLM/Kerberos authentication;
  - download quotas;
  - completion actions (shutdown or disconnect);
  - periodic synchronisation;
  - skins.
- **NOT RUN:**
  - every IDM-observed comparison;
  - macOS and Linux native Add URL;
  - installed-build human rows;
  - performance comparison.

**Exact next task:** get the exact failing URL and the dialog text from the user and replay it in the native
window. Then connect batch-from-clipboard and Import list (P1).

## Previous: normal candidate `2ede2b2`, kits validated, packaged acceptance on Windows; installed rows not run (2026-11-14)

Report: [docs/reports/beta-candidate-readiness.md](docs/reports/beta-candidate-readiness.md) §11. **B2 is not exited. Not release-ready.** Branch `milestone/installed-acceptance-handoff` (README, checklist IL6, harness label, B1 kit layout).

**Candidate:** `main` = `2ede2b26be2d18095022606f62d62fe6ef32ec10` (2026-11-14).
- Since the earlier kits' source `1fa1f3b`, the only product change is `f95aa46` (ADR 0032 sealed progress). It is
  in this candidate and in the installer-lifecycle source `77d7f5f`. Nothing in the product changed after
  `77d7f5f`, so that lifecycle evidence still applies.
- **Automatic checks on `2ede2b2`:** [CI 42794974445](https://github.com/DevSagar007/Downova/actions/runs/42794974445)
  4/4 and [Package smoke 42794974671](https://github.com/DevSagar007/Downova/actions/runs/42794974671) 3/3, both
  success.

**Normal kits:** [Acceptance kit 43552869197](https://github.com/DevSagar007/Downova/actions/runs/43552869197), source
`2ede2b2`, dispatched from DevSagar007; all three jobs succeeded. The artifacts were downloaded and checked locally
(Windows 11):

| Target | Artifact (id, GitHub outer-archive digest) | Nested kit ZIP SHA-256 | Installer SHA-256 | Local validation |
|---|---|---|---|---|
| windows-x86_64 | `14096899676`, `cf5cb4782c808037d3636314255c6d68ae40395066191744e7bced7dedce6add` | `f130437212e9716f2c959e325278b6d9bbee67c0d136ae5b38cbf7fdf5ae4dae` | setup `7817762cbe05d0876ad69984b295bcc469a1a97a16f8038c4d0c170001cb3995`, MSI `be1ea77a99e4664210a4958dfe73ce51ce32fea0647239b67a5db43f43958f2a` | passed |
| macos-aarch64 | `14097008628`, `45cbdf671b4ffeb1d965b0f6b0c8357999251c8a6ad883fc71dfd8c67f68a6d8` | `ac9aa37d220221564207c5ee6ec165b4ea0435ef8667e3c9bcfaa8ff062347b7` | dmg `cd951f113c38affdf444ac8d243f7d32ca3c9906399e34d164fbc9ae33a5e7bd` (`Downova.app.zip` `ff69f725dd8e3ff549bae6fbe3a2495ab72dc51b33e803765923fac50631beed`) | passed |
| linux-x86_64 | `14097350555`, `480a1d43f7cc7524a7d1f1ca1d601458687df92d8366469cba4cfeebb4dc0360` | `8e52d633df1ab7720fe49a340e72e12b9353cf83c62310d4be321246a97c50ba` | deb `e43455cc5a27762dc73a5f42c3a1f4a20fa4ff5f35c3c8ad1e66f8ec253338d3`, AppImage `c9856d7fbca1f98161b819484ec618242794d9576cde12b2919fe8a80586d716` | passed |

- **The checks on each kit:**
  - the downloaded artifact matches GitHub's digest and size;
  - the nested kit ZIP matches its `.sha256`;
  - `validate-kit-zip.mjs` passed: archive, `verify-kit.mjs` 28/28 files, and both fixtures;
  - `build-info.json`: version 0.1.0, `build_kind` normal, `smoke_feature` false, `updater` none, and every
    component at 0.1.0;
  - `KIT-INFO.json` `source`: the build commit equals the head commit, with no production or test-only changes
    since the build.
- **Signing:** Windows and Linux packages are unsigned; macOS is ad-hoc and not notarised.
- **Shared by all three kits:** Firefox `.xpi` `ae68456fb81f3297dafe81b87b63a5533d705d24d3054a8b479c464601273be8`
  and `THIRD-PARTY-NOTICES.txt` `94c267ffdc63c0e5c1276601abde548b7b807f241de0581083337756c077c9b1`, both unchanged
  since the `a80019a` kit.
- The older kits from run 42589426037 (source `1fa1f3b`) are kept as historical evidence for that source.

**Automatically exercised on the Windows normal package from that kit.** The packaged binaries ran from MSI
extracts with isolated data folders. This is not an installed build (Windows 11 development machine, 2026-11-14):

| Script | Rows | Result |
|---|---|---|
| `concurrency-acceptance.mjs` | global and Main-queue limit 6, six downloads progressing together, lower/raise, stop/start, manual pause, abrupt restart, cancel, SHA-256 (real packaged window and service) | 14/14 passed |
| `limits-schedules-acceptance.mjs` | speed limits and schedules through the service, kept across a restart | passed; its GUI part is covered by the next script |
| `limits-schedules-gui-acceptance.mjs` | limits and schedules in the real packaged window | 15 passed |
| `browser-lifecycle-acceptance.mjs` | B1–B7 (Firefox package, registration, repair, other installations, update in place, removal, status) | first run: B1 failed (the script required the unpacked Firefox folder, which kits do not ship by design); script fixed; rerun 7/7 passed |
| `secure-auth-acceptance.mjs` | Q1–Q5 and two leak checks (Credential Manager entries removed afterwards) | passed |
| `safety-diagnostics-acceptance.mjs` | P1–P4 (Attachment Services check, explanation in English and Bangla, redacted diagnostics) | passed |
| Checklist UP4 (agent with the repository) | `cargo test -p dm-service --test update_process` on `2ede2b2` | 3/3 passed |

**Installer lifecycle** (reused, still valid): [42784162067](https://github.com/DevSagar007/Downova/actions/runs/42784162067),
source `77d7f5f`. Windows N1–N6 and M1–M4, Linux D1–D4, and macOS A1, A2a and A3 passed; A2b NOT RUN.

**Correction (MSI scope).** Read from the MSI's own tables:
- the MSI is **per-machine** (`ALLUSERS=1`, `INSTALLDIR` under `ProgramFiles64Folder`, default
  `C:\Program Files\Downova`, so an elevation prompt);
- its `AppSearch` reuses the folder recorded in `HKCU\Software\downova\Downova`, which the NSIS installer writes.

On CI the NSIS rows run first, so M1–M4 installed into `%LOCALAPPDATA%\Downova`. The earlier statement in §10 that
"the MSI installs per user" was wrong. Checklist IL6 and the harness label now say this, and the installer scope
is unchanged.

**NOT RUN (prerequisite):**
- **Installed-build human rows** S1–S5, C1–C11, H1–H9, F1–F2, A1–A3, N1–N4, L1–L3, W1–W4, D1–D2, K1–K7, FX1–FX4,
  UP1–UP3, IL1–IL6 and TN1 need a disposable VM or a separate test account plus a human observer, or an agent
  that can drive native windows. This machine has neither: no Windows Sandbox or Hyper-V, the account is not an
  administrator, and the agent has no native-desktop control.
- Screen reader, 200 % OS scaling, real right-click, tray and notifications, and sleep, logout and reboot need a
  human observer.
- **macOS and Linux installed rows:** no macOS or Linux desktop is available to the agent. Their CI evidence is
  installer lifecycle and package smoke, not installed human acceptance.
- **macOS A2b:** needs a Developer ID (Team ID) signature.
- The kits' `RESULTS.csv` stay unfilled: no installed row was run by a human or an agent.

### Owner decisions (nothing below is chosen; each blocks only what it names)

| Decision | Observed now | Smallest options | Recommendation | Owner input needed | Blocked until decided |
|---|---|---|---|---|---|
| **Project license** | No license; [proposal](docs/decisions/proposals/project-license.md) lists A `MIT OR Apache-2.0`, B `Apache-2.0`, C `GPL-3.0-or-later`, D `MPL-2.0` | Pick one of A–D (+ DCO or CLA) | A, the Rust ecosystem norm, if closed reuse is acceptable; C if forks must stay open | The license and the contribution rule | `LICENSE`, the About and notice header text, any public distribution |
| **Remaining notices** | 14 unresolved of 388 distributed components: 13 objc2-family crates (at their exact upstream revisions there is a licensing statement but no license text or copyright notice) and WiX (the `-ext` set is inferred from the Tauri CLI binary) | (1) ship with the upstream licence texts the crates declare (MIT/Apache-2.0/Zlib) plus the statement; (2) ask upstream for texts; (3) decode the WiX arguments from the Tauri source | (1) for objc2 with the exact statement quoted; (3) for WiX | Acceptance of option 1 as sufficient for objc2 | REL-03 `verified`, macOS public distribution |
| **BRW-10 private windows** | Capture is off in private windows (`incognito: not_allowed`, policy reason `incognito`); enabling is not offered ([proposal](docs/decisions/proposals/brw-10-private-capture-scope.md)) | A: keep it unsupported in B2 and amend the acceptance; B: build an ephemeral private mode | A (no durable-trace policy exists for private sessions) | A or B | BRW-10 status, B2 exit |
| **NSIS install and data folder** | NSIS installs per user to `%LOCALAPPDATA%\Downova`, which is also the data folder. Uninstall keeps `downova.sqlite3`, `run/` and `native-messaging/` (N5), and a reinstall brings the list back (N6). The MSI is per-machine (`C:\Program Files\Downova`) but reuses that folder when `HKCU\Software\downova\Downova` exists | (1) keep it, and document that uninstall keeps data; (2) install to `%LOCALAPPDATA%\Programs\Downova` and keep data separate; (3) also offer "remove my data" on uninstall | (2): it separates program and data and stops the MSI from following NSIS into the data folder; it needs an installer migration and a new IL/N run | 1, 2 or 3 | REL-01 `verified` for Windows |
| **macOS signing and notarization; A2b** | Builds are ad-hoc (`TeamIdentifier=not set`). A2a passes; A2b (B continues from A's sealed progress) is NOT RUN, because Keychain items follow the build's own `cdhash` partition without a Team ID | (1) an Apple Developer ID certificate and notarization credentials, given to CI through GitHub encrypted secrets; (2) stay unsigned and accept a Keychain prompt on every update | (1) | An Apple Developer account and the secrets (never in chat) | A2b, macOS public distribution, Gatekeeper-friendly installs |
| **Updates, Windows signing, extension distribution** | Normal kits have `updater: none`; in-app update is verified only with a disposable fixture key. Windows packages are unsigned. Chrome/Edge use a development extension ID; the Firefox add-on is unsigned (temporary loading only) | Production updater key and HTTPS endpoint; a code-signing certificate (OV/EV, or Azure Trusted Signing); Chrome Web Store, Edge Add-ons and AMO listings with fixed IDs | Decide the update host and key custody first, then signing, then store listings | Hosting, key custody, certificate vendor, store accounts | ADV09 `verified`, SmartScreen-friendly installs, store installs |
| **IDM benchmark** | Protocol and tooling exist; no comparison has been run | Run it after the installed candidate is stable | Wait | A Windows machine with IDM installed | Any faster/lighter claim (REL-04) |

**Exact next task:** a tester on a disposable Windows 11 VM or a separate test account installs the `2ede2b2` Windows kit (setup SHA-256 `7817762c…`) and runs checklist sections 1–7 into `RESULTS.csv` and `evidence/`, starting with S1–S5, C1–C11 and IL1–IL6. Meanwhile the owner answers the decisions above.

## Previous: installer lifecycle green on all three runners; macOS A2b needs a Team ID (2026-11-10)

Report: [docs/reports/beta-candidate-readiness.md](docs/reports/beta-candidate-readiness.md) §10. **B2 is not exited.**

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

**Run on `d7db136`:** [Installer lifecycle 42595591408](https://github.com/DevSagar007/Downova/actions/runs/42595591408): Windows (N1–N6, M1–M4) and Linux (D1–D4) passed; macOS A1 and A3 passed. A2 records `run1.error` = `CREDENTIAL_STORE_UNAVAILABLE` / `retry`: the progress is held, not failed for good, so the ADR 0032 fix is confirmed on macOS. A2 still fails its "B finishes the download" criterion, because no one answers the Keychain prompt for ad-hoc signed builds on CI.

**Run on `f38c057`:** [Installer lifecycle 42602432329](https://github.com/DevSagar007/Downova/actions/runs/42602432329) was cancelled after 120 minutes. Windows and Linux passed. On macOS the disposable identity was created and both fixture builds succeeded, but the cleanup step hung on `sudo security remove-trusted-cert` (an authorization no one gives on a runner), so the rows never ran. **Fix:** cleanup no longer removes the trust setting, since the runner is discarded; every `security` call is bounded (60 s, `sudo -n`); and the signing, cleanup and harness steps have their own time limits, with cleanup not failing the job.

**Run on `39bae3a`:** [Installer lifecycle 42781522915](https://github.com/DevSagar007/Downova/actions/runs/42781522915): the macOS job now finishes, in 15 minutes rather than hanging. Signing took 1 s and cleanup under 1 s. Windows and Linux passed; A1 and A3 passed. A2 was unchanged: `run1` held as `CREDENTIAL_STORE_UNAVAILABLE` / retry, and B's signature showed `TeamIdentifier=not set`. **Cause:** a Keychain item belongs to its creator's partition: `teamid:` for an Apple-issued signature, otherwise that build's own `cdhash:`. A self-signed identity therefore cannot let B read A's item, and the disposable signing was removed. **A2 is split:** A2a (B serves, nothing is lost) is a real pass/fail check; A2b (B finishes from A's sealed progress) needs a Developer ID with a Team ID, and without one it is reported NOT RUN (a warning annotation), never passed.

**Run on `77d7f5f`:** [Installer lifecycle 42784162067](https://github.com/DevSagar007/Downova/actions/runs/42784162067) succeeded on all three runners. Windows N1–N6 and M1–M4, Linux D1–D4, and macOS A1, A2a and A3 PASSED. A2b is NOT RUN: the builds are ad-hoc (`TeamIdentifier=not set`), and `run1` is held as `CREDENTIAL_STORE_UNAVAILABLE` / retry. The macOS job took 18 minutes. CI [42784021778](https://github.com/DevSagar007/Downova/actions/runs/42784021778) and Package smoke [42784021684](https://github.com/DevSagar007/Downova/actions/runs/42784021684) on the same commit passed. One earlier Package smoke (`2c57023`, 42600754510) failed macOS Chrome check C5 once (`browser_owned`, reason null); the next three runs passed it. It is an intermittent flake, not yet investigated.

**C5 flake (fixed, test design):** in package smoke 42600754510 on macOS, `cookie.bin` (1 MiB, 80 ms per 64 KiB, about 1.3 s) finished before the extension decided. The extension may wait up to 5 s for the filename and request evidence. A download that is no longer in progress is correctly left to the browser with no reason, so the check saw `reason: null`. C3 (`export.csv`) had the same race. The fixture now holds C3's and C5's response after the first chunk until the test has read the decision, and the rows record `browser_at_decision`. Local `chrome-handoff.mjs` (Windows 11, Chrome 156): 29 passed, 0 failed; C3 and C5 decided while `running`. The remote check is the next package smoke on `main`.

**Package smoke 42787380922 on `2d3ba56`:** C3 and C5 passed on all three runners. Windows Edge failed C1 with the same race: `capture.bin` (4 MiB, about 3.8 s) finished in the browser before Downova could cancel it (`finished_in_browser`, `cancel_requested: false`). The job itself completed with the right SHA-256. C1 now holds the response, including the service's own fetch of the same URL, until the takeover has settled. Local `chrome-handoff.mjs` (Windows 11, Chrome 156): 29 passed, 0 failed; C1 shows `cancelled_by_downova`.

**`55ffe53`:** [CI 42790787416](https://github.com/DevSagar007/Downova/actions/runs/42790787416) and [Package smoke 42790787398](https://github.com/DevSagar007/Downova/actions/runs/42790787398) passed: Chrome and Edge capture C1–C5 on Windows, macOS and Linux. One green run does not prove a flake is gone; repeated runs on `main` will show it.

**Exact next task:**
1. Move C2 (`restart.bin`) and F3 (`f3_capture.bin`) to the same hold gate if they ever race; watch C1–C5 on the
   next `main` runs.
2. Verifying A2b needs a Developer ID (Team ID) signing identity: an owner decision, together with production
   signing.
3. Read the kit hashes from the artifacts; human rows per `CHECKLIST.md`; owner decisions (license, objc2
   notices, BRW-10, NSIS folder, release configuration).

## Previous: candidate `dc2ec71` on main, seven checks green; manual workflows not run (2026-11-09)

Report: [docs/reports/beta-candidate-readiness.md](docs/reports/beta-candidate-readiness.md) §8–§9. **B2 is not exited.**

**Repository state:**
- **`main` = `dc2ec71`.** It includes the r02b/S04 test fixes (`73b8d91`, pushed and merged on 2026-11-08; the branch
  `fix/flaky-r02b-s04` is no longer needed).
- **Seven checks on `dc2ec71`:** [CI 42479556011](https://github.com/DevSagar007/Downova/actions/runs/42479556011)
  4/4 and [package smoke 42479556022](https://github.com/DevSagar007/Downova/actions/runs/42479556022) 3/3, both success.

**Manual workflows (checked 2026-11-09 via the public API):** Acceptance kit and Installer lifecycle still have **0 runs**.
- No dispatch was possible from the development session: git has no stored GitHub credential, and the built-in browser is not signed in.
- macOS/Linux kits, installer rows (N1–N6, M1–M4, D1–D4, A1–A3) and human rows are **NOT RUN**.

**Exact next task:**
1. The owner runs **Acceptance kit** and **Installer lifecycle** (Actions → Run workflow → `main` → green Run workflow) and confirms each appears in the run list.
2. Read the kit hashes and installer rows (results.json and annotations), fix defects on `main`, and rerun the affected scope.
3. Human rows from the Windows kit (`1d8d74d2…`) per `CHECKLIST.md`; owner decisions (license, objc2 notices, BRW-10, NSIS folder, release configuration).

## Previous: flaky r02b and S04 fixed (2026-11-08), branch `fix/flaky-r02b-s04`, merged into `main` as `73b8d91`

Report: [docs/reports/beta-candidate-readiness.md](docs/reports/beta-candidate-readiness.md) §8. **B2 is not exited.**

**Repository state:**
- **`main` = `c874c32` (docs only):** package smoke 3/3. CI failed r02b (Windows) and S04 (macOS), both timing-dependent tests; the previous `a80019a` passed all seven checks.
- **This branch (`73b8d91`):**
  - r02b fixture with fixed, uneven, on-disk region progress;
  - S04 measured between progress changes.
  - Both suites passed 5/5 locally.
  - **Not pushed:** git had no GitHub credential.

**Manual workflows:** Acceptance kit and Installer lifecycle still have 0 runs. macOS/Linux kits, installer rows and human rows are NOT RUN.

(Historical: the "not pushed" statement above was later resolved; see the current section.)

## Previous: candidate a80019a, seven checks green, Windows packaged acceptance (2026-11-08)

Report: [docs/reports/beta-candidate-readiness.md](docs/reports/beta-candidate-readiness.md) §7. **B2 is not exited.**

**Repository state:**
- **`main` = `a80019a`.** All earlier branches are merged. B3.1–B3.7, the S4 contract, the notices, download reliability and the failure annotations are all on `main`.
- **Seven checks:** [CI 42472217944](https://github.com/DevSagar007/Downova/actions/runs/42472217944) 4/4 and [package smoke 42472217984](https://github.com/DevSagar007/Downova/actions/runs/42472217984) 3/3.

**Evidence on `a80019a` (Windows 11, local):**
- normal package and kit built and validated (kit `1d8d74d2…`);
- concurrency acceptance 14/14;
- browser lifecycle B1–B7;
- secure-auth Q1–Q5.

**NOT RUN:**
- **Acceptance kit and Installer lifecycle:** 0 runs ever; the owner's dispatch did not register, and the GitHub API answered 504 at the time.
- **Installed rows** (IL, UP, N/M/D/A).
- **macOS and Linux** kits.
- **All human rows.**

**Exact next task:**
1. Run Actions → **Acceptance kit** and **Installer lifecycle** on `main`, and check that each appears in the run list.
2. Read the results, fix what the rows reveal, and record them in §7.
3. Then the human rows and the owner decisions.

## Previous: reliable build-tool downloads (2026-11-08), branch `fix/reliable-tool-downloads`

Report: [docs/reports/beta-candidate-readiness.md](docs/reports/beta-candidate-readiness.md) §6. **B2 is not
exited.** Owner decisions remain open: the project license, the 13 objc2-family notices, BRW-10, the NSIS install
folder and production release configuration.

**Repository state:**
- **`main` = `e7309fe`:** B3.1–B3.7, the S4 resume contract, the notices (388 components, 14 unresolved) and the
  CI policy. `e7309fe` itself is the owner's docs-only merge of PR #3.
- **This branch, from `e7309fe`:**
  - `1dfc056`: bounded retries for pinned downloads; positively identified re-runs of `tauri build`; validated
    aria2 and Tauri Windows tool caches; tests;
  - a docs commit.
- **Not merged; nothing dispatched from here** (no GitHub API access).

**Evidence:**
- **CI on `70e16a6`:** all seven checks passed.
- **CI on `38fd4a7` and `e7309fe`:** 4/7.
  - Failed: Windows verify (aria2), the Windows package build (NSIS) and the Linux package build (linuxdeploy),
    each on an HTTP 504 from GitHub release assets.
  - The outage was confirmed from here, and a live `fetch-aria2` still got 504 five times and stopped within its
    bounds.
- **Locally (Windows 11), on `1dfc056`:**
  - `download.test.mjs` 9 passed;
  - `pnpm verify --only=structure,format,lint` 6 passed;
  - the workflow parse checks passed;
  - a real normal package build through the new wrapper passed: MSI `a7772089…`, setup `777e06ac…`, notices
    identical in the MSI.

**Exact next task:**
1. Merge this branch to `main`: one canonical run, seven checks.
   - The caches are empty until a Windows build succeeds, so if GitHub's release assets still answer 504, the
     first run can still fail after its bounded retries.
   - Then record the seven results for the merged SHA.
2. Once the seven pass, run **Acceptance kit** and **Installer lifecycle** once each (Actions → Run workflow,
   `main`).
   - Record the kit hashes and the N1–N6, M1–M4, D1–D4 and A1–A3 rows.
   - Fix what the rows reveal.
3. The owner decisions above, then the B2 human pass.

## Previous: resume contract and notice gaps (2026-11-08), merged into `main` as `38fd4a7`

Report: [docs/reports/beta-candidate-readiness.md](docs/reports/beta-candidate-readiness.md) §5. **B2 is not
exited.** BRW-10, the NSIS install-folder decision, production signing and publishing remain open gates.

**Repository state (reconciled with the actual refs):**
- **`main` = `70e16a6` (pushed).** It contains B3.1–B3.6, all of B3.7 (detect fix, a07, the Linux Secret Service
  for package smoke and the browser step, the macOS `.app` fix, the count-based S4), the installer-lifecycle
  harness and workflow, and the CI trigger policy. PRs #1 and #2 are contained in `main`.
- **CI policy:** automatic only on pushes to `main` (four verify + three package smoke). Branches and PRs are
  verified by hand. Acceptance kit and installer lifecycle are manual-only.
- **This branch, from `70e16a6`, adds:**
  - `b714eba`: test `r02b` and the mode-aware S4;
  - `1ff5539`: exact-revision notice texts, generator overrides with checks, and the license proposal;
  - a docs commit.

**Evidence:**
- **CI on `70e16a6`** ([42462944235](https://github.com/DevSagar007/Downova/actions/runs/42462944235)): when last
  read, 3 of 4 jobs had finished with no error. macOS and ubuntu-24.04 passed the real-browser hand-off 28/28 with
  S4. Windows was still running.
- **Package smoke on `70e16a6`:** [42462944252](https://github.com/DevSagar007/Downova/actions/runs/42462944252),
  result to be recorded.
- **Locally (Windows 11):**
  - `r02`/`r02b` 3/3;
  - real Chrome with the mode-aware S4 passed;
  - notices `--check` passed; notice tests 5 and viewer tests 8 passed;
  - both failure paths of the override checks were exercised.
- **Normal Windows package from `1ff5539`:** MSI `9a2afd83…`, setup `63f510ab…`, `THIRD-PARTY-NOTICES.txt`
  `94c267ff…` (2.5 MB). It is identical inside the MSI and holds 388 components.
- **This branch has not run remotely.**

**CI:** `70e16a6` passed all seven checks. On `38fd4a7`, four passed. verify (windows-latest) failed in the aria2 download, and package windows-x86_64 and linux-x86_64 failed in their builds; these are exactly the jobs that download GitHub release assets. The same SHA built locally without error. The cause is likely transient and not confirmed (see the readiness report).

**Exact next task:**
1. *Re-run failed jobs* for runs 42464689519 and 42464689606, then record the outcome.
2. If a failure repeats, read that step's log and fix the cause.
3. **Owner:**
   - choose the project license from the proposal;
   - decide how to present the 13 objc2-family crates that have no upstream copyright line;
   - decide BRW-10 and the NSIS install folder.
4. Run **Acceptance kit** and **Installer lifecycle** by hand when the kits are needed.

## Previous: beta-candidate readiness (2026-11-08), merged into `main` (`be09937`, then `70e16a6`)

Report: [docs/reports/beta-candidate-readiness.md](docs/reports/beta-candidate-readiness.md). It builds on B3.7
([report](docs/reports/b3.7-platform-ci-stabilization.md)). **B2 is not exited**, and BRW-10 is pending.

**Repository state:**
- **Branch** `fix/beta-candidate-readiness`, from `fe8b8bd`, which is `origin/main` `69a9068` plus the B3.7 S4
  fix.
- **Adds** the real installer-lifecycle harness and its manual workflow.
- **Not merged; `main` not pushed.**

**Evidence:**
- **CI on `69a9068`:** 6 of 7 passed. ubuntu-24.04 failed S4 for lack of an OS credential store, now fixed on this
  branch.
- **Locally (Windows 11):**
  - full verify 24/24;
  - real Chrome with S4 passed with the real store; the store-off control fails explicitly.
- **NOT RUN on this branch:**
  - the seven CI checks;
  - normal kits for the three targets (Acceptance kit workflow);
  - the installed lifecycle (Installer lifecycle workflow);
  - all human rows.
  - The workflows need a manual dispatch: this session has no GitHub API access.

**Exact next task:**
1. Push this branch and dispatch **CI**, **Package smoke**, **Acceptance kit** and **Installer lifecycle** on it
   (or open a draft PR, which starts CI).
2. Record all results with the source SHA in the readiness report, and fix what the lifecycle rows find.
3. Owner decisions: license, NSIS install folder, BRW-10, production update configuration. Then the human pass.

## Previous: B3.7 macOS/Linux CI stabilization (2026-11-08)

Report: [docs/reports/b3.7-platform-ci-stabilization.md](docs/reports/b3.7-platform-ci-stabilization.md).
**B2 is not exited.** No feature work. Installed and human acceptance, licensing, production signing and BRW-10
remain release gates.

**Repository state (recover from here after a reset):**
- **`main`** = `69a9068`, pushed by the user. It contains the detect, a07, Linux package-smoke Secret Service,
  `--no-fail-fast` and macOS `.app` fixes.
- **`fix/b3.7-linux-reconnect-ci`** = `3ae2732` (from `69a9068`), **local only**. It holds:
  - the Secret Service for the ubuntu-24.04 real-browser step;
  - the deterministic S4: a gate, a sealed checkpoint proven before the restart, no blind request, and the first
    range at the kept bytes.
- **The push failed:** the GitHub sign-in dialog was cancelled, and this session has no stored credential.

**CI on `69a9068`:** 6 of 7 passed.
- verify passed on windows-latest, macos-latest and ubuntu-22.04;
- **verify (ubuntu-24.04) failed:** strict verify passed, then real-browser S4 failed for lack of an OS credential
  store (diagnosed and reproduced locally);
- package smoke passed for windows-x86_64, macos-aarch64 and linux-x86_64.

**Local checks (Windows 11):**
- full verify 24/24 (`cargo test` 613, 0 failed);
- real Chrome: verdict passed, S4 included, with the real store. With the store off, S4 fails explicitly (the
  control).

**Exact next task:**
1. `git push -u origin fix/b3.7-linux-reconnect-ci`, then run **CI** and **Package smoke** on that branch
   (Actions → Run workflow).
2. Record all seven results for `3ae2732` in the B3.7 report. Only seven completed successes close B3.7.
3. Then the clean-machine installer batch (IL1–IL6, UP2–UP3), the REL-03 notice gaps and the license decision,
   and the B2 human pass.

## Previous: B3.6 internal beta-candidate preparation (2026-11-08), committed as `6312d2e`; `main` now includes B3.1–B3.6 and is pushed

Report: [docs/reports/b3.6-beta-candidate-preparation.md](docs/reports/b3.6-beta-candidate-preparation.md).
Design: [ADR 0031](docs/decisions/0031-installer-lifecycle-version-agreement-and-notices.md).
**B2 is not exited.** The exception covers REL-01, REL-02, REL-03, UI-01 and the remaining B3.5 validation
(recorded in the roadmap). No release, no production key, no publishing. Installed human acceptance, the
acceptance-kit results and BRW-10 remain release gates.

**Repository state (recover from here after a reset):**
- **Refs:** branch created locally from `7679438`. That is the B3.5 head, committed and pushed by the maintainer
  (local = `origin/feature/b3.5-signed-updates-recovery`).
- **Committed and merged (at the maintainer's request):**
  - B3.6 is `6312d2e`;
  - `main` was fast-forwarded from `9e44a2e` to `6312d2e` (it contains every B3.1–B3.6 branch) and pushed to
    `origin/main`;
  - the `feature/b3.6-beta-candidate-preparation` branch itself is local only.
- **Candidate rebuilt from `6312d2e`** (clean tree):
  - setup `15b04882…`, MSI `6a2b19fc…`;
  - kit ZIP `f882be31…`, without `--skip-source-check`, validated.
- **CI:** the push to `main` should start CI and Package smoke. Their results are **not checked** here: this
  machine has no `gh` CLI or token.

**What works now (Windows 11):**
- **Updates:**
  - one build version across the desktop, service, CLI, host, installer and update metadata; `package.mjs` refuses
    a disagreement;
  - the desktop refuses a service of another build;
  - the extracted-package update journey A 0.1.0 → B 0.1.1 passes U1–U12 through the real dialog: verify,
    prepare, cancel, accept, resume with hashes, pause and stopped queue kept, damaged data → exit 6 → restore
    through the dialog, isolation proven;
  - the plugin's release-build plain-HTTP refusal is verified (at initialisation).
- **Installer:**
  - the NSIS hooks stop only this installation's service before an upgrade or uninstall
    (`dm shutdown --only-from`);
  - finding: per-user NSIS installs into `%LOCALAPPDATA%\Downova`, the data folder; the decision is open in
    ADR 0031.
- **Fixed defects:**
  - a manual pause without saved progress failed after a restart (amends ADR 0012);
  - no service was started after a restore;
  - the B3.5 contract check;
  - `build-kit` dropped FX/UP rows;
  - stale bundles could be collected;
  - About showed a hard-coded version.
- **Notices:** 385 distributed components, 24 unresolved; `THIRD-PARTY-NOTICES.txt` in the packages and the MSI;
  Help → Third-party notices… offline, in English and Bangla.

**Checks (Windows 11 Pro 10.0.26300 x64):**
- **B3.5 head `7679438`, full verify:** 22 passed, 1 failed (the B3.5 contract defect, fixed here).
- **B3.6 final tree, `npx pnpm@12.14.0 verify`:** **PASSED, 24/24** (cargo test 610 passed; vitest 268 + 101). An earlier run of the batch was 21/24; its three failures are fixed (see the report §6).
- **Update journey:** 12/12, final run. **Verifier `--release`:** 1 test, 10 cases. **Browser lifecycle
  acceptance** B1–B7 on the normal candidate: passed. **Kit ZIP validation:** passed.
- **NOT RUN:**
  - any installer execution (IL1–IL6, UP2–UP3), because this machine is not an isolated environment;
  - macOS, Linux, CI;
  - human Bangla review, screen reader, the B2 installed-app pass.

**Requirement status:** REL-01, REL-02, REL-03 and UI-01 are `in-progress`; ADV09 is `in-progress` (B4 scope
open).

**Exact next task:**
1. Read the CI and Package smoke results for `main` (started by the push), then fix what macOS and Linux show.
2. Hand the kit `f882be31…` to testers.
3. On a disposable Windows VM or test account: IL1–IL6 and UP2–UP3, with the fixture pair from
   `scripts/update/build-fixture-pair.mjs`.
4. The license decision (ADR 0001) and the 24 unresolved notice items; then the B2 human pass and B2 exit.

## Previous: B3.5 verified updates, coordinated shutdown, consistent backup, recoverable migration (2026-11-07), committed and pushed by the maintainer as `7679438` on `feature/b3.5-signed-updates-recovery`

> **Status corrected 2026-11-08:** written while uncommitted; the maintainer committed it as `7679438`. Its full verify on that head was 22 passed, 1 failed (a contract-check defect, fixed in B3.6). The native install journey and the release plain-HTTP case are covered in B3.6.

Report: [docs/reports/b3.5-signed-updates-recovery.md](docs/reports/b3.5-signed-updates-recovery.md).
Design: [ADR 0030](docs/decisions/0030-verified-updates-quiescence-backup-recovery.md).
**B2 is not exited.** The exception covers the ADV09 B3 update scope and the REL-01 work it needs (recorded in the
roadmap). No release, no production key, no publishing; installed-app acceptance, the acceptance-kit results and
BRW-10 remain release gates.

**Repository state (recover from here after a reset):**
- **Refs:** branch from `453e23e` (B3.4 head). Everything in this batch is uncommitted, including the B3.4 report's
  results and the B3.4 PROGRESS section (written after `453e23e`). The agent committed and pushed nothing.
- **CI:** not run for this branch.

**What works (Windows 11):**
- **Desktop:** Help → Check for updates… with every state (not configured, checking, available, downloading,
  verified, preparing, ready to install, installing, failed, recovery required) in English and Bangla; two
  confirmations before installing; automatic checking off by default (opt-in, check only). Without a trusted
  compiled config, and for `.deb`/`.rpm` or unpackaged builds, it says "Updates are not configured".
- **Shell:** `update.rs` state machine, 7 `update_*` commands (operation ID only); the plugin is registered only
  with a trusted compiled config (key, `requireSignedVersion: true`, https; loopback http only with the
  `update-fixture` feature); install only after the service has exited and its lock is free.
- **Service:** `prepare_update`/`cancel_update` (lease, held transfers keep their desired state, checked
  `VACUUM INTO` backup, pending marker); refusals while held; startup acceptance; recovery record and exit 6;
  `--restore-update` (exit 0/5).
- **Packaging:** `--updater-config` and `--update-fixture`; normal packages refuse fixture configs and signing
  variables; a key given by path is passed to `tauri build` as `TAURI_SIGNING_PRIVATE_KEY` (the CLI reads only
  that variable; found by the fixture package build).
- **Defects fixed:** the Windows backup sync needed a write handle; a held job's byte count is refreshed from the
  engine.

**Checks (Windows 11 Pro 10.0.26300 x64):**
- `cargo test -p dm-service --lib update`: 6 passed. `--test update_process`: 3 passed (real service), run twice.
- `node scripts/update/verifier-fixture.mjs`: 1 test, 10 cases passed (real plugin 2.14.1, disposable keys,
  deleted afterwards).
- `cargo test -p dm-desktop` (shell unit tests) passed; `updates.test.tsx` 8 passed; `updater-config.test.mjs` 3
  passed; `cargo clippy --workspace --all-targets -- -D warnings` clean.
- Fixture package: PASSED: NSIS `Downova_0.1.0_x64-setup.exe` (`08bb3afe…`) and MSI (`c4b28a46…`) each with a `.sig` whose trusted comment is `file:<name> | version:0.1.0` (signed-version binding); `build-info.json` `"updater": "fixture"`; the temporary key folder was deleted. First attempt FAILED (the Tauri CLI reads only `TAURI_SIGNING_PRIVATE_KEY`, not `_PATH`); fixed in `package.mjs`. Not installed or updated (NOT RUN)
- `npx pnpm@12.14.0 verify` on `7679438` (2026-11-08): 22 passed, 1 failed (`dm-domain` contract example; fixed in B3.6).
- **NOT RUN:** the native fixture install journey, the plugin's release-build plain-HTTP refusal, MSI, macOS,
  Linux, CI, human rows UP1–UP4, a screen-reader pass of the dialog.

**Requirement status:** ADV09 `in-progress` (B3 part implemented; B4 channels/SBOM/provenance open); REL-01
`in-progress` (update path evidence added).

**Exact next task:**
1. The maintainer reviews and commits this batch (with the B3.4 report results), then dispatches Actions → **CI**
   and **Package smoke** for the branch.
2. The Windows NSIS fixture install journey: two fixture builds (0.1.0 and a bumped version) with one disposable
   key, `latest.json` on loopback, update through the dialog (UP2, UP3).
3. Fix what macOS and Linux show for B3.1–B3.5; the B2 installed-app and human pass (with UP1–UP4) → B2 exit.
4. Production updater key, HTTPS endpoint and config (maintainer; [report §7](docs/reports/b3.5-signed-updates-recovery.md#7-production-configuration-still-needed-maintainer-not-created-here)).

## Previous: B3.4 Firefox public hand-off and per-package browser support (2026-11-07), `453e23e` on `feature/b3.4-firefox-browser-compatibility`

Report: [docs/reports/b3.4-firefox-browser-compatibility.md](docs/reports/b3.4-firefox-browser-compatibility.md).
Design: [ADR 0029](docs/decisions/0029-firefox-build-registration-and-browser-packages.md).
**B2 is not exited.** The exception covers BRW-08, BRW-09 and the BRW-07/UI work they need. No release,
installed-app acceptance not waived, BRW-10 open, private-window policy unchanged.

**Repository state (recover from here after a reset):**
- **Refs:** branch head `453e23e` (committed and pushed by the maintainer), stacked on `e1a2bc4` (B3.3); `main` is
  `9e44a2e`.
- **Uncommitted after `453e23e`:** this PROGRESS section and the B3.4 report's results (the committed report still
  has `RESULTS_PLACEHOLDER`).
- **CI:** 0 runs for `453e23e`, `e1a2bc4`, `e42c77f`, `38a3f32`, `b701e3c` and `d4c6acb`. CI and package smoke
  run only on `main` pushes, PRs or manual dispatch; there is no `gh` or token here.

**What works (Windows 11):**
- **Firefox:** “Download with Downova” for public http(s) links; popup service check; acknowledgement; same-key
  retry; missing-host and service-down answers; browser fallback. Capture, sign-in, reconnect, containers and
  private windows are gated off with an explanation. The build is unsigned (temporary add-on only).
- **Host:** Firefox launch arguments with an exact add-on allowlist; Firefox registration (`allowed_extensions`,
  Mozilla locations) under the existing ownership rules; installer, uninstaller and first launch include Firefox.
- **Browser packages:** native, Snap, Flatpak or unknown, with an evidence-backed verdict per browser package, in
  `dm-browser-host status` and Help → Browser connections… (Repair, never `--replace`).

**Checks (Windows 11 Pro 10.0.26300 x64):**
- **Real Firefox** 159.0: `firefox-handoff.mjs` F1–F9 passed, also rerun on the clean `453e23e` tree.
- **Chrome** 156.0.8078.80 and **Edge** 157.0.4341.42: 29/29 each.
- **`npx pnpm@12.14.0 verify`:** run 1 20/22 and run 2 21/22. The `cargo test` group failed in `dm-backend-http`
  (crash and OOM under memory pressure; that crate is unchanged). Reruns passed, and a complete
  `cargo test --workspace` passed: 593 passed, 0 failed. **No single 22/22 run.**
- **Package:** MSI `6fd637f6…`, `.xpi` `ae68456f…`; tree equals `453e23e`. `browser-lifecycle-acceptance.mjs`
  B1–B7 passed, and the secure-auth, safety-diagnostics, limits-schedules GUI and concurrency exercises all passed.
- **NOT RUN:** macOS, Linux, any Snap or Flatpak browser, CI, installed-app and human rows (FX1–FX4, K, earlier).

**Requirement status:** BRW-08 `implemented`; BRW-09 `in-progress` (no sandboxed package tested); BRW-07 implemented
with Firefox evidence.

**Exact next task:**
1. The maintainer commits the uncommitted report and PROGRESS, and dispatches Actions → **CI** and **Package smoke**
   for the branch.
2. Fix what macOS and Linux show for B3.1–B3.4 (the Keychain, Secret Service and Linux Firefox paths never compiled
   here); obtain one clean 22/22 verify run.
3. The B2 installed-app pass with the K and FX rows.

## Previous: B3.3 secret protection, OS credential store, signed-in GET and reconnect (2026-11-07), part 1 `38a3f32`, part 2 `e42c77f` on `feature/b3.3-secure-authenticated-downloads`

Report: [docs/reports/b3.3-secure-authenticated-downloads.md](docs/reports/b3.3-secure-authenticated-downloads.md).
Design: [ADR 0027](docs/decisions/0027-secret-lifecycle-vault-and-authenticated-get.md), amended by
[ADR 0028](docs/decisions/0028-reconnect-signed-in-downloads.md).
**B2 is not exited.** The exception covers SEC-01, SEC-02 and BRW-05, including reconnect. No release, installed-app
acceptance not waived, BRW-10 open, private capture unchanged.

**Repository state (recover from here after a reset):**
- **Refs:** remote branch head = local `HEAD` = `38a3f32` (part 1: vault, sealing, signed-in GET; pushed by the
  maintainer); parents `c389476` → `b701e3c` (B3.2); `main` is `9e44a2e`.
- **Part 2 was committed and pushed by the maintainer as `e42c77f`** (12:42 local; the branch matches the remote).
  It contains:
  - reconnect, the `sign_in_required` holds and migration 14;
  - the extension reconnect page;
  - the gap fixes (cookie-less resume after a restart, Forget leaving a usable session, a repeated add replacing
    the sign-in, the token-bearing source display);
  - the aria2 signed-link audit test;
  - the docs;
  - the removal of the accidental empty root file `{`.
- **Uncommitted after `e42c77f`:** `PROGRESS.md`, the report (`e42c77f` still has its placeholders, filled here) and
  the packaged script's scanner fix (unreadable locked files reported separately; Q4 expects a `forgotten` record).
- **CI:** 0 runs for `38a3f32`, `b701e3c` and `d4c6acb`. `e42c77f` could not be checked (public API rate limit).
  CI and package smoke run only on `main` pushes, PRs or manual dispatch, so a branch push runs nothing. No `gh` or
  token here.

**What works (Windows 11):**
- Download with Downova using my sign-in (Chrome and Edge, https, same origin, no third-party frame).
- A lost, expired, forgotten or locked sign-in **holds** the download with its saved part.
- Reconnect a Downova download with my sign-in: explicit choice; Reconnect or Reconnect and resume; idempotent; bound
  to job, origin, profile and generation. The same job resumes; a changed resource is never spliced.

**Checks (Windows 11 Pro 10.0.26300 x64):**
- `npx pnpm@12.14.0 verify` run 4: **21/21 PASSED** (Rust 644 passed, 0 failed; vitest 246 + 98). Run 3 was 19/21
  (eslint and clippy, fixed).
- **Real browsers** (`chrome-handoff.mjs`, full): Chrome 156.0.8078.80 **29/29**, Edge 157.0.4341.42 **29/29**,
  including S4 (reconnect after restart) and S5 (third-party frame).
- **Process tests** `reconnect_process` R1–R3 passed; `real_os_vault_round_trip` passed (Credential Manager).
- **Package:** recorded as `38a3f32` plus diff `4c76035a…` (built at 06:35 UTC, before the commit; its code equals
  `e42c77f`, with no diff under `crates/`, `apps/` or `packages/`, but it is not a clean build of that commit), MSI `30e7e74b…`. `secure-auth-acceptance.mjs` 7/7,
  `safety-diagnostics-acceptance.mjs` P1–P4, `limits-schedules-gui-acceptance.mjs` G0–G13 and
  `concurrency-acceptance.mjs` (all rows) passed.
- **NOT RUN:** macOS, Linux, CI, installed-app and human rows (K1–K7 and the earlier ones).

**Requirement status:** SEC-02 and BRW-05 are `implemented` (not verified); SEC-01 is `in-progress`.

**Exact next task:**
1. The maintainer commits the three files above and dispatches Actions → **CI** and **Package smoke** for the
   branch (also owed for `38a3f32`, `b701e3c` and `d4c6acb`).
2. Fix what macOS and Linux show: the Keychain and Secret Service adapters and the `cfg` paths were never compiled
   here.
3. Then the B2 installed-app pass with the kit rows (including K1–K7).

## Previous: B3.2 Windows attachment safety, scan states, redacted diagnostics (2026-11-05), committed as `b701e3c` (stacked on `d4c6acb`)

Report: [docs/reports/b3.2-download-safety-diagnostics.md](docs/reports/b3.2-download-safety-diagnostics.md).
Design: [ADR 0026](docs/decisions/0026-attachment-safety-scan-states-and-diagnostics.md).
**B2 is not exited.**
- **The exception was extended by the maintainer** to F07 (Windows), E06, E05 and the SEC-01 work they need. It is
  recorded in the roadmap.
- **Unchanged:** no release, installed-app acceptance not waived, BRW-10 open, private capture unchanged, IDM unrun.

**Remote:**
- `main` `9e44a2e`;
- B3.1 branch `d4c6acb` (pushed by the maintainer);
- **0 CI or package-smoke runs and no PR** for `c04ed52` or `d4c6acb`;
- no authorized GitHub access here.

**Done (Windows 11 x64):**
- **F07:**
  - `IAttachmentExecute::Save` between `placed` and `complete`, off the coordinator lock (dedicated STA thread,
    2 at once, 10-minute timeout);
  - only the sanitized origin is given;
  - identity and SHA-256 are re-verified before any outcome;
  - blocked files are never re-created; MOTW fallback only when the service failed; never "safe";
  - the new `dm-win-attachment` crate is the only unsafe code, guarded by the structure check.
- **E06:** `DownloadSnapshot.scan` (scanning / attachment_checked / blocked / unavailable / skipped).
  - Migration 12; the journal's `scan_started_ms` and `final_file_id`.
  - Cancel during a check is allowed; a late result is dropped; restart-safe; one notification.
  - The UI and CLI report the same values; legacy completed jobs show no check.
- **E05:**
  - an allowlisted `diagnostics.v1.preview` with an opt-in crash section and the final `export_leaks` check;
  - `diagnostics.v1.export` of exactly that snapshot (atomic, no unasked replace);
  - Help → Diagnostics… with the save dialog in the shell;
  - `dm diagnostics` and `dm diagnostics-export`.
- **Docs updated:** requirements (F07, E05, E06 → `implemented`; a SEC-01 note), traceability, architecture,
  testing, project structure, roadmap and the report.
- **Acceptance kit:** rows L1–L3, W1–W4 and D1–D2 added to `docs/acceptance/b2-checklist.md` §6.

**Checks (Windows 11 Pro 10.0.26300 x64):**
- `npx pnpm@12.14.0 verify` (run 2): **20/21**. Rust 607 passed, 0 failed; vitest 237 and 82.
  - The single failure was the structure link to the then-missing report; `check-structure` is OK since.
- Real `IAttachmentExecute::Save` (`dm-win-attachment --ignored`): `Saved`, ZoneId=3, origin-only `HostUrl`.
- `node scripts/package.mjs`: a normal package.
  - MSI `8a5a7306…` (9,764,864 bytes), NSIS `a3884a22…`.
  - Source `d4c6acb` plus diff `b34c7db4…` (not a clean commit build); unsigned.
- Packaged exercise `safety-diagnostics-acceptance.mjs`: **4/4 passed** (P1–P4).
- B3.1 packaged GUI regression on this package: **15/15 passed**, with every completion going through the real check.
- **NOT RUN:**
  - CI and package smoke for both branches;
  - macOS and Linux;
  - installed-app and human rows (L, W, D);
  - a real antivirus block;
  - the native save dialog.

**Remaining external actions:**
1. **The maintainer** reviews, commits and pushes `feature/b3.2-download-safety-diagnostics`, and dispatches
   Actions → **CI** and **Package smoke** with that branch selected. A push alone does not run them. The same is
   owed for B3.1.
2. **B2:**
   - the acceptance-kit workflow on `main`;
   - the testers' `RESULTS.csv`, now with the B3 rows for feature packages;
   - the BRW-10 choice.

**Exact next task:** item 1, then fix what CI shows on macOS and Linux (the `not_supported` scanner path and Unix
identity compile only there).

## Previous: B3.1 stabilization (CORE-06, CORE-08) (2026-11-05), uncommitted on `feature/b3.1-speed-limits-schedules` at `c04ed52`

Report: [docs/reports/b3.1-speed-limits-schedules.md](docs/reports/b3.1-speed-limits-schedules.md), Part 2 (§8–§15).
**B2 is not exited.** The sequencing exception covers only CORE-06 and CORE-08 on this branch.
- **Remote state:**
  - `main` is `9e44a2e`; the branch is `c04ed52` (the first B3.1 part, pushed by the maintainer);
  - **no CI or package-smoke run exists for `c04ed52`, and no PR exists.**

**Done (Windows 11 x64):**
- **Schedule presentation:**
  - `schedule_exemption` (contract, service, UI explanation);
  - the next window per download, in the queue's zone (with a refresh once it passes);
  - `queue_limit` only when a full limit is the reason, and the primary reason ranked.
- **Defects fixed, each with a failing-first test:**
  - `resume` bypassed a closed window;
  - a retry dropped the queue's holds;
  - a running transfer moved into a closed window kept running;
  - an exempt transfer was re-checked only every 60 s;
  - Manage queues saved twice on a double click;
  - the per-download speed control could double-submit and show a late answer on another job;
  - Bangla times showed "PM".
- **Engine review:**
  - S07: a stalled server still times out under a limit;
  - no lock is held during a token wait (code read).
- **Migrations:** a v9 upgrade keeps jobs, finish stamps, queues and settings (new test).
- **Packaged GUI acceptance:** 15/15 on the final normal package (`scripts/acceptance/limits-schedules-gui-acceptance.mjs`).
- **Docs updated:**
  - ADR 0024 and 0025 notes;
  - requirements;
  - traceability;
  - testing;
  - the report. Its push statements are corrected: feature-branch pushes do not trigger CI or package smoke.

**Checks (Windows 11 Pro 10.0.26300 x64):**
- `npx pnpm@12.14.0 verify`: **PASSED 21/21**.
  - Rust: 586 passed, 0 failed.
  - Vitest: desktop 227, extension 82.
- `node scripts/package.mjs`: a normal package.
  - MSI `160c7932…`, NSIS `d40a68d1…`.
  - Source `c04ed52` plus diff `694f9af1…`; unsigned.
- GUI acceptance: **passed 15/15**; installed app NOT RUN.
- Performance A/B against `baee51f` (production source identical to `main`): no regression visible at n = 5.
  - Memory, CPU and paced completion: report §14.
  - Unthrottled medians: 116.2 → 111.6 MiB/s for one 512 MiB file and 78.7 → 84.0 MiB/s for six 128 MiB files.
  - Order-dependent spread in both packages.
- **Measurement-tool fix:** `measure.mjs` adopted unrelated processes through reused PIDs; a child must now start
  after its parent.
- **NOT RUN:**
  - CI and package smoke on this branch (no authorized access);
  - macOS and Linux;
  - installed-app and human checks;
  - real sleep/wake.

**Remaining external actions:**
1. **The maintainer** commits and pushes this diff. Then dispatches Actions → **CI** and **Package smoke** with
   branch `feature/b3.1-speed-limits-schedules` selected (report §12).
2. **B2:**
   - the acceptance-kit workflow on `main`;
   - the testers' `RESULTS.csv`;
   - the BRW-10 choice.

**Exact next task:** item 1. Then fix any platform failure CI shows, and add the limits and schedules rows to the
installed-app acceptance kit.

## Previous: B3.1 speed limits (CORE-06) and queue schedules (CORE-08) on a feature branch (2026-11-05), uncommitted on `feature/b3.1-speed-limits-schedules` (base `9e44a2e`)

Report: [docs/reports/b3.1-speed-limits-schedules.md](docs/reports/b3.1-speed-limits-schedules.md).
**B2 is not exited.**
- **The sequencing exception:**
  - granted by the maintainer and recorded in [docs/roadmap.md](docs/roadmap.md);
  - it allows only CORE-06 and CORE-08, and only on this branch;
  - the B2 blockers and BRW-10 stay open, private capture is unchanged, and IDM comparisons are unrun.

**Done (Windows 11 x64):**
- **CORE-06 → `implemented`:**
  - native HTTP token buckets, one global and one per download shared by its segments; live changes;
    cancellation-safe waits;
  - an engine-owned read timeout. This fixes a real defect: reqwest's read timeout counted token waits.
  - contract `speed_limits.v1.get/set` and `downloads.v1.set_speed_limit`;
  - migration 10; limits restored before recovery;
  - capability gating, with aria2 refusing (`unverified`);
  - CLI, Options and details UI in English and Bangla.
- **CORE-08 → `implemented`:**
  - pure `dm-domain::schedule` (`jiff`): IANA zone, overnight windows, merging, DST gap and fold;
  - the `schedule_window` policy blocker, evaluated before admission and on wake;
  - checkpointed transfers held at a window's end; ones without a checkpoint finish;
  - durable bounded catch-up;
  - migration 11; schedules restored before recovery;
  - `queues.v1.set_schedule`; CLI `queue-schedule`; Manage queues → Schedule (English and Bangla);
  - a moved job, or a deleted queue's jobs, take the destination's policy.
- **ADR 0024 and 0025:** accepted for this branch, with implementation notes (corrections).
- **Docs updated:**
  - `requirements.md`;
  - `docs/traceability.md`;
  - architecture and testing;
  - the roadmap.

**Checks (Windows 11 Pro 10.0.26300 x64):**
- `npx pnpm@12.14.0 verify` (full): **PASSED 21/21**, 0 unavailable.
  - Rust: 579 passed, 0 failed.
  - Vitest: desktop 221, extension 82.
- `node scripts/package.mjs`: a **normal** Windows package built.
  - MSI `419013e4…`, NSIS `529e13cb…`.
  - The source is `9e44a2e` plus the uncommitted diff `2d4409b2…`.
- `node scripts/acceptance/limits-schedules-acceptance.mjs --package-dir dist-packages/windows-x86_64`:
  **7/7 passed** (L1 −6.6 %, L2 −4.8 %, S1–S4, L3); GUI not run.
- **NOT RUN:**
  - macOS and Linux;
  - the installed app and the packaged GUI;
  - real sleep/wake and OS clock changes;
  - the acceptance kit (`build-kit` refuses uncommitted packages);
  - the measurement-tool regression run.

**Not built:**
- the "cannot resume, finishing outside the window" note;
- the per-job next-window text (it is shown in Manage queues).

**Display nit (existing B2 behaviour):** a schedule-held job also shows "queue limit".

**Remaining external actions (B2, unchanged):**
1. **The maintainer** runs the acceptance-kit workflow. No `gh`, token or signed-in browser is available here.
   The step: Actions → "Acceptance kit (normal packages, internal)" → Run workflow → `main`.
2. **Testers** return `RESULTS.csv` from clean VMs or test accounts.
3. **The maintainer** chooses BRW-10 A or B.
4. **The maintainer** reviews this branch, and asks for a commit or push if wanted. (Corrected later: a push of this branch does not by itself run CI or package smoke
   on macOS and Linux.

**Exact next task:** item 1 (B2). On this branch, only the maintainer's review, then a commit and push on request
to get the macOS and Linux CI evidence.

## Previous: B2.36 delivery status, B2 exit assessment, B3.1 designs (2026-11-04) — documentation only, uncommitted on `682cef5`

Report: [docs/reports/b2.36-delivery-status-and-b3-designs.md](docs/reports/b2.36-delivery-status-and-b3-designs.md).
**B2 is not exited. No B3 code was written** (the roadmap requires the B2 exit first).

**Last verified source:** `682cef5b49af211472e71bc4dc9044e8b2ba72ea` (B2.35, committed and pushed by the user).
- CI #58 (41817259789): passed.
- Package smoke #48 (41817259770): passed.
- The B2.35 report markers left in that commit are filled with the actual results (staged).

**Done:**
- **Acceptance-kit workflow (id 406799339): 0 runs; not dispatched.** There is no `gh`, no API token, and the
  browser pane is not signed in to GitHub. Exact action: Actions → "Acceptance kit (normal packages, internal)" →
  Run workflow → `main`.
- **B2 exit assessed:** automation gates met (#45–#48, CI #58). Three gates not met:
  - normal packages for every OS: the workflow is unrun;
  - installed and human acceptance: no tester results;
  - the BRW-10 choice: [proposal](docs/decisions/proposals/brw-10-private-capture-scope.md).
- **ADR 0024 reviewed** (still Proposed):
  - burst floor fixed (16 s → at most 1 s at the minimum rate);
  - throttled rotation after 1 MiB or 2 s;
  - both buckets taken in order.
- **ADR 0025 queue schedules** written (Proposed): zones, DST, policy-only blocker, bounded durable catch-up, `jiff`
  proposed.

**Checks:** structure OK; prettier passed. No code changed. Last full `pnpm verify` (B2.35): 21/21.

**Remaining external actions:**
1. **The maintainer** runs the acceptance-kit workflow.
2. **Testers** run the kits on clean VMs or test accounts and return `RESULTS.csv`.
3. **The maintainer** chooses BRW-10 A or B.
4. **The maintainer** accepts ADR 0024 and 0025 (and the `jiff` dependency).

**Exact next task:** item 1. Once B2 exits, B3.1 implements ADR 0024 (CORE-06) and then ADR 0025 (CORE-08).

## Previous: B2.35 native normal-package and acceptance-kit workflow (2026-11-04), committed as `682cef5` (CI #58 and package smoke #48 passed)

Report: [docs/reports/b2.35-native-kit-workflow.md](docs/reports/b2.35-native-kit-workflow.md). **B2 is not exited.**

**Last verified source:** `015e815f1c6f1fda5ed2a2279d3122b45a73a311` (main, pushed).
- CI #57 (41811368649): passed.
- Package smoke #47 (41811368586): passed.

B2.35 is an uncommitted diff of scripts, a new workflow, tests and docs; no production source changed.

**Done:**
- **`.github/workflows/acceptance-kit.yml`:**
  - manual `workflow_dispatch`; `contents: read`; no secrets or releases;
  - native `windows-latest`, `macos-latest` and `ubuntu-22.04`;
  - steps: a **normal** `package.mjs` build, then `build-kit.mjs --zip` (source check on), then
    `validate-kit-zip.mjs`;
  - artifacts `downova-acceptance-kit-<label>-<sha>` (after validation passes) and `…-logs` (always), kept 30
    days, with a job summary.
- **`scripts/lib/zip.mjs`:** a portable ZIP writer and reader. GNU tar cannot write ZIP, so the builder no longer
  depends on it.
- **`build-kit.mjs`:**
  - checks the extension ID against the helper's `DEV_EXTENSION_ID`;
  - picks the primary installer per OS;
  - generates the quick-start header from the package;
  - enforces a path allowlist;
  - writes `<zip>.sha256` and `<zip>.json`.
- **`validate-kit-zip.mjs`:** checks the delivered ZIP with the OS's standard extractor, runs `verify-kit.mjs`, and
  runs both fixtures with a matching file from each.
- **Tester documents:** `QUICKSTART-bn.md` covers getting the kit from the workflow and page zoom vs OS scaling;
  the checklist's S4 no longer hard-codes file names.

**Local kit (Windows, normal `baee51f` package, production unchanged up to `015e815`):**
- `dist-packages/acceptance-kit-b2-windows-x86_64.zip`: 14,904,036 B, SHA-256
  `f6dffa51e328d05c5276bd763a929a65898937873666c4a46d6242e3dd274689`;
- package: unsigned; setup `84690544…e4fc`; MSI `f0443a87…c035`.

**Checks (Windows 11 Pro 10.0.26300 x64):** full `pnpm verify` **21/21 passed**; `zip.test.mjs` 6/6; `build-kit.test.mjs` 6/6; `validate-kit-zip.mjs` 7/7 on the final Windows kit ZIP; PowerShell `Expand-Archive` cross-check passed; the workflow YAML parsed and its step references checked.

**Not run:**
- the native workflow (it needs a push and a manual dispatch), so no macOS or Linux kit exists yet;
- every installed and human row;
- the Python `zipfile` cross-check (Python is not installed);
- any IDM measurement.

**Open decisions:**
- BRW-10, [proposal](docs/decisions/proposals/brw-10-private-capture-scope.md): stays `in-progress`.
- ADR 0024: stays Proposed.

**Exact next task:**
1. **The maintainer:** push, then Actions → "Acceptance kit (normal packages, internal)" → Run workflow on `main`;
   confirm three green jobs and three kit artifacts.
2. **Testers** run their platform's kit (`QUICKSTART-bn.md`, then `CHECKLIST.md`) on a clean VM or test account and
   return `RESULTS.csv` and `evidence/`.
3. **The maintainer** chooses BRW-10 A or B.
4. **Reassess the B2 exit;** then accept ADR 0024 and start B3.1.

## Previous: B2.34 acceptance kit, BRW-10 decision note, ADR 0024 proposed (2026-11-04, Windows), committed as `015e815` (CI #57 and package smoke #47 passed)

Report: [docs/reports/b2.34-acceptance-kit.md](docs/reports/b2.34-acceptance-kit.md). **B2 is not exited.**

**Source and workflows:**
- `46477896089b23c28ab7b667ab814685dc7c1043` (main, pushed).
  - **CI #56 (41801632788):** passed on all four runners.
  - **Package smoke #46 (41801632786):** passed on Windows, macOS and Linux, with X1–X8 21/21 for Chrome and Edge on
    each.
- B2.34 is an uncommitted diff of scripts, tests and documentation only.
- Production code is unchanged since `baee51f` (only `apps/extension/src/handoff.test.ts` differs in the production
  paths), so the `baee51f` normal package is still the candidate.

**Done:**
- **Acceptance kit** (`scripts/acceptance/build-kit.mjs`, tested by `build-kit.test.mjs` 4/4):
  - **Folder:** `dist-packages/acceptance-kit-b2-windows-x86_64/`.
  - **Zip:** `dist-packages/acceptance-kit-b2-windows-x86_64.zip`, 14,890,538 B, SHA-256
    `9674de91b86d5f8da9a8d73affc867f64de173f3f875bea83b3a73ec0f0d244f`.
  - **Contents:** the normal installers and production extension of `baee51f`; the fixtures; the full checklist
    (`docs/acceptance/b2-checklist.md`, 35 test ids); the Bangla quick-start (`docs/acceptance/b2-quickstart-bn.md`);
    `RESULTS.csv`; `KIT-INFO.json`; `verify-kit.mjs`; `SHA256SUMS.txt`.
  - **Missing:** macOS and Linux normal packages. None exists locally, and smoke builds are never substitutes.
- **Kit validated:**
  - unzipped and verified (22/22 files match);
  - both fixtures run from the unzipped kit, with a served file matching its announced SHA-256;
  - every checklist command matches the packaged `dm` and `dm-browser-host` (MSI administratively extracted to
    temp, nothing installed).
- **BRW-10 decision note** (proposed): `docs/decisions/proposals/brw-10-private-capture-scope.md`.
  - Option A: private capture unsupported in B2, with the acceptance reworded.
  - Option B: an end-to-end ephemeral mode.
  - It lists the effects on storage, history, idempotency and recovery. BRW-10 stays `in-progress`.
- **ADR 0024 bandwidth limits** (Proposed): `docs/decisions/0024-bandwidth-limits.md`. Not started: B3 needs the B2
  exit.

**Checks (Windows 11 Pro 10.0.26300 x64):** full `pnpm verify` 20/21: ESLint caught a real defect in `build-kit.mjs` (the SystemRoot fallback string had an unescaped backslash, so its path lost the separator), fixed; the rerun of `--only=lint,structure,format` passed 5/5. Every other group passed (cargo test, native host e2e, aria2 groups). Kit validation and `build-kit.test.mjs` 4/4 passed. The baseline and earlier acceptance were reused: no production change.

**Not run:**
- every installed and human row: no clean VM or test account here;
- the macOS and Linux kits: no normal packages;
- any IDM measurement.

**Exact next task:**
1. **A tester** with a clean Windows VM or test account unzips the kit, runs `node verify-kit.mjs`, follows
   `QUICKSTART-bn.md` and then `CHECKLIST.md`, and returns `RESULTS.csv` with `evidence/`.
2. **A maintainer** builds normal packages on macOS and Linux (`node scripts/package.mjs`) and rebuilds the kit with
   `--package-dir`.
3. **The maintainer** chooses option A or B for BRW-10.
4. **Reassess the B2 exit;** then accept ADR 0024 and start B3.1.

## Previous: B2.33 fault acceptance, trustworthy measurements, B2 closeout assessment (2026-11-04), committed as `ee93de4`, `6866013`, `d202a60`; all automated gates green

Report: [docs/reports/b2.33-closeout.md](docs/reports/b2.33-closeout.md). **B2 is not exited** (matrix in the
report §6).

**State of the source:**
- `baee51f` (B2.32) is committed and pushed by the user.
  - CI #51 (41780221546): green on Windows, macOS, Ubuntu 22.04 and 24.04.
  - Package smoke #41 (41780221573): green on Windows x86_64, macOS aarch64 and Linux x86_64; Chrome and Edge 13/13
    each.
- `ee93de4`: a snapshot of B2.33 the user committed and pushed during the work.
  - **Package smoke #42 (41784478573) passed on all three OSes**, with X1–X7 20/20 per browser on Chrome and Edge.
  - **CI #52 (41784478652) FAILED on all four runners**, ESLint only: an unused parameter in
    `scripts/perf/validity.test.mjs`. It stays a failed historical run.
- `6866013` and `d202a60` hold the rest of B2.33: X8, the 8 s silent helper, the lint fix and the docs. CI #53/#54 and
  smoke #43/#44 were **cancelled**, superseded by the next push, not failed.
- **Current: `d202a601a52941c34786a3a743e92ef2175c78ff`.**
  - **CI #55 (41798143677): passed** on Windows, macOS, Ubuntu 22.04 and 24.04.
  - **Package smoke #45 (41798143800): passed** on Windows (110 + 1 note), macOS (110 + 1 note) and Linux (111 +
    1 note), with **X1–X8 21/21** for Chrome and Edge on each:
    - Windows: Chrome 156.0.8078.80, Edge 156.0.4314.56;
    - macOS: Chrome 156.0.8078.48, Edge 156.0.4314.42;
    - Linux: Chrome 156.0.8078.47, Edge 156.0.4314.42.
  - Local `pnpm verify` at `d202a60` **passed 21/21** (Windows 11).
- Everything since `baee51f` is scripts, tests and documentation only (`git diff baee51f d202a60` over `crates`,
  `apps/desktop/src*` and `packages` is empty), so the `baee51f` packages are still the candidate.
- The `segmented` timeouts under full load and the intermittent Edge teardown did not recur in #55, #45 or the local
  full verify. No code was changed for them; their causes stay unproven, and the failed runs are kept.

**Done:**
- **Runbook corrected** ([b2.32-installed-acceptance-runbook.md](docs/reports/b2.32-installed-acceptance-runbook.md)):
  - admission only with capacity (C5 raises both limits to 7);
  - Exit and stop pauses durably, and Resume is needed after reopening (ADR 0012);
  - stopping a queue never resumes a manual pause;
  - a stopped service is a deterministic "unavailable", because the helper never starts one;
  - the Windows manifest folder, `--browser` flags, MSI vs NSIS hooks, the AppImage limitation and the fixture
    binding are corrected.
- **Fault acceptance X1–X8** in the real-browser harness and the packaged smoke (`--fault-steps`):
  `scripts/e2e/fault-host.mjs` is a test-only helper in front of the installed host. It covers:
  - a helper that never answers;
  - an acceptance whose acknowledgement is lost or mismatched;
  - no service, and no helper installed;
  - an unsupported link;
  - capture keeping the browser's download.

  Same-key retries give one job. The manifest is restored.
- **Measurements:**
  - `scripts/perf/validity.mjs`: invalid workloads fail and are excluded with reasons; exit 1 when any run is not
    ok, 2 when unavailable;
  - the sample window starts after every job transfers;
  - a chunk-safe first-line reader (`scripts/lib/first-line.mjs`);
  - run-level p95 (`stats.mjs`);
  - the fixture binds to the LAN, with `/__stats` and an open-response peak.
- **IDM protocol made executable:**
  - W2a (defaults: 3 active) separate from W2b (6 active);
  - the host budget recorded, not assumed equal;
  - two host names for W3;
  - W5 evidence from `/__stats`;
  - IDM rows unrun.
- **The B2.32 acceptance oracle defect fixed:** `record` let a detail named `status` overwrite the step status.

**Checks (Windows 11 Pro 10.0.26300 x64):**
- **Packaged smoke on the `baee51f` smoke package:**
  - `b233-fault1`: X6 got a 404 (a hyphenated file name the capture route's `[w.]+` did not match); kept.
  - `b233-fault2`: X6 cookie left by C5 (`sent_cookies`, correct product behaviour); kept.
  - `b233-fault3`: 109 passed, X1–X7 on Chrome and Edge, but the Edge teardown failed (intermittent, also seen in
    `b232-final` without the fault rows).
  - **`b233-fault4`: 111/111 passed**, X1–X8 on Chrome 156.0.8078.79 and Edge 156.0.4314.58.
- **Concurrency acceptance** `concurrency-b232b`: 14/14; engine permits at most 8; fixture count 9.
- **Baseline** `perf-results/b233-baseline`: 5 rotated runs, 50/50 ok, exit 0.
- **Invalid demo** `perf-results/b233-invalid-demo`: `invalid`, exit 1.
- **Extension unit test** (`handoff.test.ts`): 21/21, the new test mutation-checked.
- **Tool tests:** first-line 9, validity 7, native-messaging 3, fixture 6.
- **Full `pnpm verify`:** first full run **FAILED** (19/21: an unused parameter in `validity.test.mjs`, fixed; `cargo test` with 6 of 16 `dm-backend-http --test segmented` cases failing with `NetworkOffline: the server did not respond in time`, no Rust changed in this step). Then: `segmented` alone 16/16; `--only=lint,rust` 5/5 PASSED (cargo test --workspace included); `--only=structure,contracts,format,typecheck,test,build` 12/12 PASSED on the final sources; the aria2 groups passed in the first run. The `segmented` failure under full load is recorded as a possible timing sensitivity, not explained.

**Test packages (unchanged, `baee51f`, no uncommitted diff, unsigned, internal):**

| Package | File | Size (B) | SHA-256 |
|---|---|---|---|
| Normal | `dist-packages/windows-x86_64/Downova_0.1.0_x64-setup.exe` | 5,957,248 | `84690544d4a9e0634549d85f807a8fefe07e33a837a8103f156da20ed799e4fc` |
| Normal | `dist-packages/windows-x86_64/Downova_0.1.0_x64_en-US.msi` | 9,146,368 | `f0443a875b19cea91a0f16f3313b45e86c63943e410d4d4e6f95fb66d958c035` |
| Smoke-test | `dist-packages/windows-x86_64-smoke-test/Downova_0.1.0_x64-setup.exe` | 6,002,683 | `c426a074bb6c23cfe3ed2eda17d0e89527097ec5226001f9c831cba8955c4f97` |
| Smoke-test | `dist-packages/windows-x86_64-smoke-test/Downova_0.1.0_x64_en-US.msi` | 9,216,000 | `c2b635c4c965de64476d4eadd4ce653ac1dabfd6d829f96a0b37ead40051cc8e` |

- The normal MSI was extracted, launched and exercised (baseline, acceptance). The smoke package was installed into
  isolated per-user locations (its HKCU values saved and restored), launched and exercised by the smoke.
- Neither was installed for the real user.

**Not run:**
- the installed-app and human pass on any OS: no clean VM or test account exists here, and creating one is a
  system change;
- p95 (needs ≥ 20 runs);
- any IDM measurement.

**Open decision:** BRW-10's second criterion ("Enabling it shows the ephemeral session and history policy") has no
implementation, because private capture is not offered (ADR 0019 §8). Is "not offered" the agreed B2 scope (amend the
criterion with an ADR note), or must the ephemeral policy be built before B2 exits? See report §6a. E02's Opera QA is
B4 by the requirement's own priority line.

**Exact next task:**
1. **A tester with a clean VM or test account per OS** runs the corrected runbook with the normal `baee51f` package.
   The checklist is in report §7; record the observer of each row.
2. **The maintainer decides the BRW-10 scope** (report §6a).
3. **Reassess the B2 exit** against report §6.
4. **Then B3.1, CORE-06 speed limits:** write ADR 0024 first, then implement as planned in report §7. Not started:
   the B2 exit is its prerequisite.

## Previous: B2.32 Linux P2 root cause, permit trace, six-download measurement (2026-11-04, Windows), committed as `baee51f`

Report: [docs/reports/configurable-concurrency.md](docs/reports/configurable-concurrency.md) § B2.32.
- **Linux P2:** at 200 % with a download selected, the details panel squeezed the list to 0 px on every platform.
  - The probe reported that as a blank band of 0 px on Windows (a false pass) and 6 px on Linux.
  - Fixed in `App.css` and `DownloadTable.css`; P2 now requires `rows_visible ≥ 1`.
  - Package smoke #41 passed on all three OSes.
- **Server-side overlap of 9:** explained by the engine permit trace (`DOWNOVA_PERMIT_TRACE`). The engine never
  held more than 8; handovers took ≤ 5 ms. No production change.
- **Other changes:** `dm queue-limit`; `active_6` with activity, completion and SHA-256; `?norange=1`; the
  installed-app runbook and the IDM protocol (both corrected in B2.33).
- **Checks:** local smoke `windows-x86_64-b232-final2` 95/95; acceptance `concurrency-b232b` 14/14; the four
  concurrency test commands passed. CI #51 and package smoke #41 are green.

## Previous: B2.31 understandable download limits (2026-11-04, Windows), committed as `12dc3e7`

_Historical handoff. "Uncommitted" and "Exact next task" below describe the state before the user committed it as
`12dc3e7` (CI #50 green; package smoke #40 failed only Linux P2, fixed in B2.32)._

Report: [docs/reports/configurable-concurrency.md](docs/reports/configurable-concurrency.md) § B2.31. **B2 is not exited.**

**Remote results for `e0609bc`:**
- CI #49: green on all runners.
- Package smoke #39:
  - Windows passed;
  - Linux P2 `blank_px_top: 6` at 200 % with no geometry (the probe now reports `probe_version`);
  - macOS real-Chrome C1/C2 failed once (capture; browser workstream).

**Done:**
- **Options "What this allows":**
  - the saved global limit, Main's limit, how many Main can run (the min; aria2 ≤ 3), and active now;
  - the queue-limit reason with **Change queue limit** (opens Manage queues, returns to Options);
  - aria2, stopped, shared and over-limit notes, in en and bn ("প্রধান কিউ");
  - queues re-read from the service.
- **Scheduler:** 5 new tests (10 in total); no defect found.
- **Fixed:**
  - the h4 margin;
  - the smoke JSON-lines race (`readJsonl`);
  - the acceptance close lag (16 KiB steps);
  - the host-budget oracle, now client-side TCP.

**Checks (Windows 11 Pro 10.0.26300 x64):**
- full `pnpm verify` 20/20; quick groups 9/9 on the final sources (desktop 206);
- A05, `dm-desktop concurrency`, `connection_budget` 8/8 (permits = 8);
- smoke `windows-x86_64-b231b` 95/95;
- acceptance on the final package: `final4` and `final5` 15/15; `final3` FAILED T1 (server-side overlap 9,
  unexplained; client-side 8).

**Test package:** `dist-packages/windows-x86_64/`, x86_64, unsigned.
- setup `371617d0…8c61`, MSI `b8fcb0fe…7e09`;
- `e0609bc` plus diff `4c64b181…2e56`;
- MSI contents launched and exercised; `setup.exe` not run (it would register in the real profile).

**Not run:**
- macOS and Linux (they need a push);
- the normal NSIS install;
- screen readers;
- OS zoom by a person;
- an uncertain save in the packaged app;
- REL-04 benchmarks and any IDM comparison.

**Exact next task:**
1. The user reviews, commits and pushes. Read CI and package smoke. P2 on Linux: if `probe_version` is "b2.31"
   with no geometry, investigate the probe/runner path; with geometry, fix the band from it.
2. Then the human acceptance pass on an installed build, in a separate profile.
3. Then the REL-04 benchmark protocol before any IDM comparison.

## Previous: B2.30 native verification of configurable concurrency (2026-11-02, Windows), committed as `e0609bc`

Report: [docs/reports/configurable-concurrency.md](docs/reports/configurable-concurrency.md) § B2.30. **B2 is not exited.**

**Remote results for `2d999b9`:**
- CI #48: macOS failed C06's server-side overlap assertion.
- Package smoke #38 (and #37): Linux P2 `blank_px_top: 6` at 200 %.
- Everything else passed.

**Done:**
- **C06 on macOS, a measurement lag:** the fixture saw closed sockets late. Now:
  - the fixture notices client closes at once (5 ms allowance, down from about 50);
  - `HttpBackend::peak_requests_per_host()` gives the exact permit peak (C05–C07 assert it);
  - C08 checks that a non-range stream is never rotated.
- **P2 on Linux:** not reproducible on Windows WebView2 (0 px); the probe now records geometry when a band appears. The
  layout is unchanged.
- **UI tests:** a rejected re-save and a stale save reply after a reconnect (mutation-checked). Desktop tests: 197.
- **Acceptance script:** S1, V1, W1, C1, and the per-file range overlap.

**Checks (Windows 11 Pro 10.0.26300 x64):**
- full `pnpm verify`: 19/20 (only prettier, on a file being edited), then 10/10 for the JS and doc groups;
- A05 PASSED; `dm-desktop --lib concurrency` PASSED; `connection_budget` 8/8;
- packaged smoke `smoke-results/windows-x86_64-b230`: 95/95;
- concurrency acceptance `acceptance-results/concurrency-b230b`: 14/14.

**Test package:** `dist-packages/windows-x86_64/`, Windows x86_64, unsigned.
- setup `0ab90dea…92a4`, MSI `a36b0f6a…f534`;
- source `2d999b9` plus diff `23333661…2697`;
- its MSI contents were launched and exercised; its NSIS installer was not run.

**Not run:**
- macOS and Linux (they need a push);
- the installed NSIS normal build;
- screen readers;
- OS zoom by a person;
- an uncertain save in the packaged app;
- the IDM benchmark.

**Exact next task:**
1. The user reviews, commits and pushes. Read CI (C06 and C08 on macOS) and package smoke from the annotations:
   P2 on Linux will now carry geometry. Fix the 200 % top band only from that evidence.
2. Then the human acceptance pass.
3. Then the REL-04 benchmark protocol before any IDM comparison.

## Previous: configurable concurrency integrated and accepted on Windows (2026-11-02), committed as `2d999b9`

**Base and patch:**
- `4fc3d66` (clean) plus the attached patch (SHA-256 `4a672ad8…71dc6`, matching `manifest.json`), applied once.
- Uncommitted, with no remote run. **B2 is not exited.**
- Report: [docs/reports/configurable-concurrency.md](docs/reports/configurable-concurrency.md); ADR 0023 (amended).

**Corrected during integration (each with a check that fails without its fix):**
1. **Tauri grants:** `concurrency_get` and `concurrency_set` were not in `build.rs`/the capability, so the packaged
   app would refuse them. `check-structure` §8 now guards every command.
2. **Checkpoints under contention:** rotating segment requests never published a durable checkpoint, so a crash
   lost all progress. Fixed in `segmented.rs`; new backend test C07; process test A05 (which found it) now passes.
3. **Options text:** the note now states the both-limits rule in en and bn, and the Bangla note names the menu in
   Bangla.

**Windows checks (Windows 11 Pro 10.0.26300 x64; `CARGO_BUILD_JOBS=2`):**
- `pnpm verify` **PASSED 20/20** (final run; an earlier run failed only formatting, and one C07 read-timeout at 32 MiB led to its 16 MiB size).
- JS groups 12/12 (desktop 193, extension 81).
- `cargo fmt` PASSED; workspace `cargo clippy -D warnings` PASSED.
- `dm-desktop --lib concurrency` PASSED; C07 PASSED.
- A05:
  - FAILED with the patch as delivered;
  - after the fix: 2 unexplained overlap failures, then 14/14 standalone and 3/3 in the suite (overlap 8).
- `pnpm contracts` and `pnpm check`: UNAVAILABLE (no such scripts; `contracts:check` PASSED).

**Packaged acceptance:** `acceptance-results/concurrency-win-7`, verdict passed, 10/10:
- the real window: Options, Enter twice (one save), invalid value, Bangla, Manage queues;
- six files at once;
- live lower and raise;
- pause and stopped queue held;
- abrupt stop and restart: settings persisted, recovery;
- SHA-256 of all 8 files.

Earlier runs 1–6 (kept) document script and oracle corrections.

**Internal test build:** `dist-packages/windows-x86_64/` (normal production configuration, unsigned; no smoke hooks;
production extension).
- MSI `bea73634b934274975e21f535756f3e9b0649afb1fc8cdcbf97d8e894c13b1cf`;
- setup `17aa28132a2c9aa1046c8a6afd92b7367e2ceb3ec16969f832fb7ce11e383dcd`;
- source `4fc3d66` plus diff `f0eeef60…` (47 paths at build time);
- after the build, only tests, scripts and docs changed.

**Not run:**
- macOS and Linux native builds and remote CI;
- an installed (NSIS) build;
- screen readers;
- OS zoom by a person;
- an uncertain save in the packaged app (unit-tested only).

**Exact next task:**
1. The user reviews, commits and pushes. Read CI and package smoke on all runners from the annotations, including
   A05 and C07 on macOS and Linux for the first time.
2. Watch A05's reported overlap values for the intermittent.
3. Then the human acceptance pass (B2.24–B2.27 report §7), plus this feature's Options dialog by a person with a
   screen reader and at OS zoom.

## Attached Linux preparation (historical, from the patch)

Isolated checkout of `4fc3d66a8e9dd00679ae2e6275d1e684a20e1cba`; user work and newer B2.24–B2.28
stabilization changes preserved. This feature is uncommitted and has no new remote workflow result.
See [the report](docs/reports/configurable-concurrency.md) and ADR 0023.

- Global active-file setting 1–16 (default 3), persisted independently and loaded before recovery;
  service/RPC/CLI/Tauri/UI en/bn. Queue limits remain independent. Increasing admits eligible jobs;
  decreasing does not preempt. Optional aria2 keeps its conservative three-file cap.
- Host contention: cancellation-safe waiters and rotation of validated ranges, flushing and syncing
  before releasing permits. Same eight-per-host budget; no adaptive tuning or speed claim.
- Local evidence: JS verification 9/9 (193 desktop, 81 extension tests); structure/format 3/3; HTTP
  backend 102 tests; core 105 (new concurrency 5), storage 38; Rust contracts/method parity 4/4.
  Targeted Rust clippy with -D warnings, rustfmt and diff checks passed. C05 reproduced a failure
  in the baseline, then passed 3/3 final runs; C06 proved six files progressing with exact hashes.
- The full service-lib run reports 123 passed and 6 failed because this environment denies AF_UNIX
  binding (independently reproduced). This is not counted as a passed suite; IPC acceptance is pending.
- The exact Rust downloads adapter and its tests also typecheck in a headless harness; its bounds
  test passes. This is distinct from native Tauri/IPC acceptance.
- Full workspace Rust build/checks are blocked by missing native GUI prerequisites (`pkg-config` /
  `glib-sys`), separately from the Unix-socket restriction. No native acceptance pass is claimed.
- Exact next task: apply/review the prepared patch, then run A05 plus the native bridge and packaged
  acceptance checks on an unrestricted OS runner with the desktop build prerequisites. Existing B2 gates
  below remain open. No commit, push or live installation was changed.

### Baseline milestone and historical evidence (preserved)

B2.28 is included in baseline `4fc3d66`; its older pre-push notes below are historical.

_Last updated: 2026-11-02 (Windows 11 Pro 10.0.26300 x64, Node 26.7.0, pnpm 12.14.0 via npx, rustc 1.99.1)_

## Baseline milestone (historical handoff)
**B2.24–B2.27 committed and pushed by the user as `2f5ecb2`. B2.28 (readable failure annotations): done locally,
uncommitted.** **B2 is not exited.**

**Remote results for `2f5ecb2` (public API, 2026-11-02):**
- **CI #46 (41401830164): green on all 4 runners.** The macOS Clippy fix is confirmed.
- **Package smoke #36 (41401830271):**
  - Windows passed.
  - macOS failed **G2** (second launch → existing window; passed in #34 and #35; cause unknown, no detail readable).
  - Linux failed **1 row that cannot be identified**: the annotation is cut at about 4 KB (52 of 96 rows), as in
    every run #30–#36.

**B2.28 (this step):**
- `scripts/lib/annotations.mjs` publishes non-passed rows first plus one detail annotation per failed row (bounded).
  Both the smoke and the real-browser test use it.
- `scripts/lib/annotations.test.mjs` is in `pnpm verify` (structure).
- No product code changed.

**Exact next task:**
1. The user reviews, commits and pushes B2.28.
2. Read package smoke on all runners from the new annotations: identify the Linux row and the macOS G2 detail, and
   fix only from that evidence.
3. Then continue with B2.24–B2.27 report §8: the human acceptance pass, the WebView2 GPU-process ADR, and the
   startup outliers.

### B2.24–B2.27 (committed as `2f5ecb2`; the lines below are its local handoff)
**Remote reference (read through the public API, 2026-11-02):**
- CI #45 (`0e78942`): macOS failed Clippy; Windows and both Ubuntu jobs passed.
- Package smoke #35: passed on all 3 platforms.
- No newer run exists. The macOS fix is verified locally only (Windows clippy plus a Linux cross-check; a macOS
  cross-check was UNAVAILABLE).

**Done:**
- macOS Clippy (`SystemNotifier.app_id` only on Windows).
- Ordered, bounded notification delivery (`notify::Delivery`, 32).
- Typed `CallError::TooLarge`: the window shows "list could not be loaded" instead of a reconnect loop.
- Resubscription backoff.
- Three list fixes:
  - the removal neighbour of an offscreen selection;
  - focus kept when the focused row is removed elsewhere;
  - a blank band at CSS zoom 200 %.
- New tests: event-ordering (15), virtualized list (+4), IPC framing, delivery, bridge.
- Smoke P2.
- Measurement tooling: `service_start`, `cycles`, startup split, `--diagnose`, `--rotate`, `--webview-args`,
  process types, `compare.mjs`.

**Measurements** (`perf-results/cmp-*`):
- No resource regression and no improvement is claimed.
- The WebView2 GPU process is bimodal per launch (about 52 vs about 100 MiB) in both builds. Within each mode the
  builds agree within about 3 MiB. This explains the B2.21–B2.23 "178 → 197".
- `--disable-gpu` experiment: idle 115 MiB, one transfer 129, 10k 192, with CPU and latency unchanged. **Not
  adopted** (needs an ADR and wider hardware).
- Service start: about 0.1 s with a new profile and about 0.065 s with an existing one; the 3–7 s desktop-spawn
  outliers are unexplained.

**Checks (Windows 11 Pro 10.0.26300 x64):**
- `pnpm verify` PASSED 20/20.
- `cargo fmt`, `cargo clippy` and `cargo test` PASSED (`CARGO_BUILD_JOBS=2`).
- Packaged smoke `smoke-results/windows-x86_64-b224b`: verdict passed 95/95.
- `pnpm check`: UNAVAILABLE (no such script).
- **Not run:** macOS, Linux, remote, human checks (H1–H9, A1–A3, N1–N4).

**Internal test build:** `dist-packages/windows-x86_64/`, unsigned, from `0e78942` plus diff `c0a82085…`
(setup `3a296638…`, MSI `7e23e8e0…`). The full hashes and instructions are in report §5.
`dist-packages/b223-windows-x86_64` and `b224-windows-x86_64` are the measured packages.

**Exact next task:**
1. The user reviews, commits and pushes. Read CI (the macOS Clippy fix) and package smoke on all runners (P2 runs on
   macOS and Linux for the first time); read failures from their artifacts before changing code.
2. The human acceptance pass from report §7 (N1–N4, H1–H9, A1–A3), recording results with the build hash.
3. The WebView2 GPU-process ADR with measurements on more hardware; then the startup outliers and a 50-cycle heap
   series.

### Previous: B2.21–B2.23 (committed as `0e78942`; the statements below about "uncommitted" or "no remote run" are historical)
**Batch B2.21–B2.23: done locally on Windows 11 (2026-11-01), uncommitted; no remote run yet.** Report:
[docs/reports/b2.21-b2.23-batch.md](docs/reports/b2.21-b2.23-batch.md). ADR 0022.

**Baseline `30560da`** (the stabilization batch, committed and pushed by the user; notes below that call it
"uncommitted" are historical):
- CI #44 green on all 4 runners.
- Package smoke #34: Windows and macOS passed; Linux failed only N3b.

**Done:**
- **N3b:** evidence from the service's restart transition (harness defect: a timeout fallback read the pre-restart
  state); unit regression.
- **Measurement tool:** `scripts/perf/measure.mjs`, Windows, normal release package, isolated profiles.
- **Accessible virtualized list (ADV12):** memoized rows, sorting and filters; bounded event batching.
- **Product fix:** the IPC line bound (1 MiB both ways) prevented any history of about 3,000+ jobs from loading.
  Clients now accept 64 MiB answers; requests are still capped at 1 MiB.
- **CORE-09 notifications:** service-owned, at most once, off by default, en/bn, CLI + Options.

**Measurements** (same machine, 5 rotated runs, medians; `perf-results/baseline`, `perf-results/after`):
- 1,000 records: 299 → 209 MiB private; DOM 21,101 → 828 nodes; search 251 → 33 ms (p95 379 → 88).
- 10,000 records: unusable → works (237 MiB, search 34 ms).
- Idle: unchanged within noise. Hidden to tray ≈ visible (WebView2 memory is not released); UI closed ≈ 2.6 MiB.
- **Unexplained regression:** one transfer 178 → 197 MiB (+11 %, wide spread); three transfers +8 MiB.
  Service-ready latency medians rose, with outliers. Neither is explained yet.

**Checks (Windows 11 Pro 10.0.26300 x64, 2026-11-01):**
- `pnpm verify`: 19/20 in one run. `cargo test` hit out-of-memory while compiling (7.5 GB machine); the Rust group
  rerun with `CARGO_BUILD_JOBS=2` PASSED 4/4.
- desktop vitest 168; extension vitest 81.
- packaged smoke `smoke-results/windows-x86_64-b221b`: verdict passed 94/94 (N3b, O1, P1 new). Run 1 was 57/60:
  the P1 probe channel was wrong, and R failed, probably because of the P desktop lingering (not proven).
- **Not run:** macOS, Linux, remote CI/smoke; OS notification display; screen readers; human checks.

**Internal test build:** `dist-packages/windows-x86_64/` (unsigned; `30560da` + diff `3c4d83ae…`)
- Full hashes and test instructions are in report §5.

**Exact next task:**
1. The user reviews, commits and pushes. Read CI and package smoke on all runners (N3b, O1, P1 on Linux and macOS
   for the first time); read failures from their artifacts before changing code.
2. Investigate the active-transfer WebView2 memory rise (heap snapshots before and after batching).
3. The human pass: N1–N4 (notifications, report §4), H1–H9, screen readers on the virtualized list.

### Previous: stabilization batch
**Stabilization batch for B2.15–B2.20: done locally on Windows 11 (2026-11-01), uncommitted; no remote run yet.** B2.15–B2.20 itself is
**committed** as `36edd54` (by the user); notes below that call it "uncommitted" are historical.

**Remote baseline `36edd54`:**
- [CI #43](https://github.com/DevSagar007/Downova/actions/runs/41197630303) **failed on all 4 runners**: `clippy::items_after_test_module` in
  `crates/cli/src/output.rs` (production functions added after `mod tests`). My earlier local `pnpm verify` run was
  interrupted before its Rust group finished, so this was never caught locally.
- [Package smoke #33](https://github.com/DevSagar007/Downova/actions/runs/41197630412) **failed on all 3 OSes**, the same rows everywhere:
  - `3b popup shows acceptance, not completion` (Chrome and Edge): the harness still expected the old English phrase "not
    necessarily started"; the hand-off itself (step 3) passed.
  - `M3 Refresh link …`: `kept={}` — the keep-progress call returned nothing.
  - The earlier Windows Edge **C1** failure (#32) **passed** in #33: acceptance, Downova-attributed cancel, one job, SHA-256.
- No newer local or remote change at the start of this batch (tree clean at `36edd54`).

**Failure / evidence checklist (this batch):**
| # | Item | Root cause | Fix type | Status |
|---|---|---|---|---|
| 1 | CI clippy `items_after_test_module` | functions after `mod tests` in `crates/cli/src/output.rs` | product code order | fixed; `cargo clippy --workspace --all-targets -D warnings` clean (Windows) |
| 2 | smoke 3b popup (Chrome/Edge, all OSes) | obsolete phrase in `scripts/e2e/chrome-handoff.mjs` | harness | rewritten on stable hooks (`data-record/-code/-role`), en + bn, redaction; real Chrome/Edge 3+3 runs and packaged smoke PASSED (Windows) |
| 3 | smoke M3 `kept={}` | **harness deadlock, reproduced**: `dm()` = `spawnSync` blocks the in-process fixture while the service verifies the new link against it (sync: killed at 60 s, no output; async: `progress_kept` in 127 ms) | harness | `dmAsync` runner for M2/M3; M3 also checks a same-length mismatch; packaged smoke M3 PASSED (Windows) |
| 4 | Edge "timed out waiting for browser to exit" (one local run) | not reproduced in 7 later Edge runs | harness | bounded, escalating shutdown with recorded evidence (no escalation was needed) |
| 5 | Desktop dialogs through the packaged UI | coverage gap | tests | journey N (N1–N3b) through the smoke bridge PASSED (Windows) |
| 6 | Verified job held for name confirmation could be reopened | product gap found by new tests | **product** | `verifying`/`scanning`/`finalizing` never reopened; test added |
| 7 | Desktop vitest failing under load | 1 s / 5 s defaults | test config | waits 5 s, tests 20 s; 5/5 runs green under load |
| 8 | ESLint read packaged outputs | `dist-packages/` not ignored | config | ignored; lint PASSED |
| 9 | Segmented tests read-timeout once inside `pnpm verify` | not proven (passed 3/3 alone and in later runs) | — | **open, recorded** |
| 10 | Internal test build, fixture page, manual checklist | — | — | done: `dist-packages/windows-x86_64/`, `scripts/acceptance/fixture-server.mjs`, report §3 |

**Checks (Windows 11 Pro 10.0.26300 x64, 2026-11-01):**
- `cargo clippy --workspace --all-targets -- -D warnings`: PASSED.
- `cargo test --workspace`: PASSED 3/3 standalone (service lib 116, operations process 4).
- `pnpm verify` (final): 19/20 PASSED; lint FAILED only on packaged outputs; after ignoring them,
  `verify --only=lint,format,structure` PASSED 4/4.
- desktop vitest 157 (5 runs under load); extension vitest 81.
- real Chrome 156.0.8078.79 ×3 and Edge 156.0.4314.58 ×3: 16/16 each.
- packaged smoke `smoke-results/windows-x86_64-stab2`: **verdict passed, 92/92** (run 1: 91/92, harness field-name bug).
- **Not run:** macOS, Linux, any remote workflow, all human checks.

**Internal test build:** `dist-packages/windows-x86_64/` (unsigned, internal; `36edd54` + uncommitted diff
`c11f2be6…ec1a`).
- `Downova_0.1.0_x64-setup.exe` `475112df…396d`
- `Downova_0.1.0_x64_en-US.msi` `0e190eea…141f`
- `extension-unpacked/` list hash `6d41074b…0b09`

Instructions (separate Windows account or VM, `DOWNOVA_HOME`/`DOWNOVA_DOWNLOAD_DIR`) and the H1–H9 checklist are
in [the batch report](docs/reports/b2.15-b2.20-batch.md) §3.

**Exact next task:**
1. The user reviews and commits the stabilization changes, then pushes. Read CI and package smoke on all runners.
   Expected: clippy green; R3 3b and M3/N rows green on macOS and Linux too. Read any failure from its artifacts
   (results.json) before changing code.
2. The human pass H1–H9 with the internal build, recorded here with OS, browser and build hash.
3. The next batch: ADV12 list virtualization (10k items) and CORE-09 completion notifications (report §5).

### B2.15–B2.20 batch (committed as `36edd54`)
**Baseline before it:** `3c559e3`.  (B2.12–B2.14 with every fix, committed and pushed by the user; the earlier "uncommitted" note is
stale):
- [CI #42](https://github.com/DevSagar007/Downova/actions/runs/41190394377) green on all 4 runners. This includes
  real Chrome capture C0–C5 on macOS and Ubuntu 24.04, so macOS/Linux capture evidence exists.
- [Package smoke #32](https://github.com/DevSagar007/Downova/actions/runs/41190394338): macOS ARM64 and Linux x64
  passed; Windows x64 failed one row, Edge C1 (job 124505831964): `state=accepted, browser_side=running,
  browser_state=interrupted, browser_error=USER_CANCELED`, one native job, completed with the expected SHA-256.
  C2–C5 passed in that Edge run.
- No newer remote runs at the start of this batch.

**Increments and ADRs:** B2.15 capture sync · B2.16 Retry (CORE-03) · B2.17 Refresh link (COR-06) · B2.18 checksum
input and presentation (COR-05) · B2.19 folder controls (CORE-07, ADV12, SEC-03) · B2.20 extension en/bn popup.
ADR 0020 (explicit retry and source replacement; amends ADR 0018's finish-time rule) and ADR 0021 (user-triggered
directory reveal; narrows the SEC-03 guard of ADR 0015).

**Acceptance criteria:**
1. **B2.15:** C1 waits, within its deadline, for the persisted takeover outcome and keeps independent assertions
   (acceptance, cancel + attribution, one job, SHA-256). Deterministic tests delay storage writes, native replies,
   cancel callbacks and events (accepted before takeover, event before/after cancel resolves, worker stopped after
   intent, genuine user cancel, browser completion, revoke/disable). Repeated real-browser runs are reported.
2. **B2.16 Retry:** `downloads.v1.retry` (contract, service, storage, CLI, desktop).
   - Only a failed, listed, non-finalizing job; never completed/cancelled/removed.
   - A fresh attempt; a failed job never has a reusable checkpoint, so partial bytes are discarded only with
     `discard_partial: true` (refused otherwise with `choose_restart_or_keep`).
   - Queue, priority, destination, names and trusted checksum preserved; admission limits and stopped queues apply.
   - Durable operation identity; replays and double clicks give one attempt; `expected_attempt_id` guard; stale
     callbacks of the old attempt dropped.
   - The finish time is cleared only by this explicit reopen (ADR 0020).
3. **B2.17 Refresh link:** `downloads.v1.replace_source`.
   - Progress is kept only with a backend-verified proof: the replacement is resolved through the gated redirect
     path; a ranged read of the last confirmed bytes returns `206` with the same strong ETag and length, and the
     bytes equal those on disk; and the resource is the same (same origin and path) or the job has a trusted
     SHA-256.
   - Otherwise `choice_required` with a reason; Restart is explicit; Keep current sends nothing.
   - No secret URLs in logs, events or errors; durable operation identity; crash-safe ordering.
   - Fixtures: compatible replacement, same-length changed content, changed/missing validators, shared ETag on
     another resource, range ignored, origin-changing redirect, crash boundaries, lost acknowledgement.
4. **B2.18 checksum:** Add URL takes an optional SHA-256 (exactly 64 hex digits); legacy fingerprints unchanged
   when absent; details show "Publisher checksum matched" separately from "SHA-256 computed locally"; mismatch
   copy and recovery in en/bn.
5. **B2.19 folders:** native Browse… for rule folders (cancel changes nothing); Open containing folder for an
   eligible selected job, resolved by the service, directory only, never the file; missing file/folder/volume
   errors; SEC-03 guard narrowed, not removed.
6. **B2.20 extension:** popup, hand-off states, readiness, actions, empty/loading/unavailable/uncertain states and
   context-menu text in en/bn from the one catalogue; stable codes stored; older records render.
7. **Checks:** owning-layer tests, contract examples, migrations, `pnpm verify`, real Chrome/Edge, packaged smoke
   additions where automatable.

### Batch B2.15–B2.20 progress
- **B2.15 capture sync — done (Windows 11):**
  - C1 now re-reads the persisted record until the takeover outcome is stored (acceptance is stored before the
    takeover finishes; the old C1 asserted the early `accepted + running` snapshot, which is exactly the Windows
    Edge failure in smoke #32). It also records `first_settled` and `cancel_requested`.
  - Product refinements: one takeover per record per worker (resume racing a late acceptance cancels once); a
    non-cancel interruption during takeover is `kept`, not claimed as Downova's cancel.
  - 9 new deterministic interleaving tests (delayed storage, deferred cancel callback, events before/after the
    cancel promise, worker stop after intent, genuine user cancel, completion during takeover, revoke/disable,
    concurrent takeover). Extension capture tests: 35 passed.
  - Repeated real-browser runs (isolated fixtures each): Edge 156.0.4314.50 6 runs, C0–C5 passed 6/6 (run 4 later
    failed step 4 with "timed out waiting for browser to exit", unrelated to capture); Chrome 156.0.8078.79 2 runs
    16/16; 2 further Chrome runs did not start (cargo build failed while I was editing Rust) and are not counted.
- **B2.16/B2.17 service — done:** migration 8 (`operations`), `reopen_job`/`replace_source_kept`;
  `downloads.v1.retry`, `downloads.v1.replace_source`, `downloads.v1.locate` (contract + TS + 4 valid/2 invalid
  examples); backend seam `discard_output` + `verify_replacement` (HTTP implementation in
  `crates/backend-http/src/replace.rs`); CLI `retry`, `refresh-link`, `locate`, `add --sha256`.
  - Checks: dm-storage 34; dm-service lib 113 (14 new operations tests); `backend-http --test replace_source` 5
    (S01–S05); `dm-service --test operations_process` 3 (kill after release/restore, lost ack, byte-exact);
    dm-domain + contract examples; dm-cli 15 + 17.
- **B2.16–B2.19 desktop — done:** shell commands `downloads_retry`, `downloads_replace_source`, `downloads_reveal`,
  `folder_pick` (`folders.rs`, dialog 2.8.4 + opener 2.7.2 from Rust only, no plugin permission); Retry and Refresh
  link dialogs, menus and details actions; SHA-256 input; checksum presentation (local-hash wording corrected);
  Browse…; Open containing folder; en/bn copy. Desktop vitest 157 passed; dm-desktop 21 passed; SEC-03 guard narrowed
  (3 passed).
- **B2.20 extension — done:** popup/menu en/bn from the one catalogue, stored codes; extension vitest 81 passed.
- **Docs — done:** ADRs 0020/0021 (+ notes in 0002, 0015, 0018), requirements, traceability, architecture, design,
  testing, setup, roadmap, [batch report](docs/reports/b2.15-b2.20-batch.md) (results pending).
- **Packaged smoke:** journey M (M1 checksum, M2 Retry, M3 Refresh link, M4 locate) added.
- **Next (superseded 2026-11-01):** that verify run was interrupted, and the push then failed CI #43 and package smoke #33. See the stabilization batch at the top.

### Previous batch (B2.12–B2.14)
**Batch B2.12–B2.14: committed and pushed by the user (`32676dd`, `d278f10`, `074c378`, fixes in `3c559e3`). CI #42
green; package smoke #32 green on macOS and Linux, Windows Edge C1 failed (fixed in B2.15):**
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

#### Batch B2.12–B2.14 progress
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
- **B2.13 automatic capture — implemented (extension; Windows 11, real Chrome 156.0.8078.79 and Edge 156.0.4314.50):**
  - `apps/extension/src/capture/`: `policy.ts` (sites/exclusions, filters, eligibility precedence), `evidence.ts`
    (non-blocking webRequest evidence in storage.session), `captures.ts` (durable state machine), `i18n.ts` (en/bn);
    options page (`options.html`, `src/options.ts`, `src/options.css`); popup summary;
  - manifest: optional `downloads` + `webRequest`, optional host permissions, `options_ui`; `incognito` stays
    `not_allowed`; the e2e build pre-grants the capture permissions for 127.0.0.1/localhost only;
  - ADRs 0018 (retention) and 0019 (capture) written.
- **Checks (B2.13):**
  - extension vitest: 54 passed (24 capture, 4 options/catalogue);
  - `node scripts/e2e/chrome-handoff.mjs --kind chrome`: 16 passed, 0 failed (C0–C5 + 0–5);
  - `--kind edge`: C0–C5 passed (run 5).
  - **Findings while making it work:**
    - `bin` was wrongly in the always-browser types (removed);
    - `chrome.runtime.reload()` was not a usable worker restart; it is now `ServiceWorker.stopAllWorkers` plus a
      wake;
    - observed in Edge 156 (headless and headed): a download in a tab opened by automation with no user gesture
      ends with HTTP 204 in webRequest and creates no download item. The harness now starts downloads by
      clicking a link with a user gesture, in both browsers.
  - C2's boundary depends on timing: the Chrome run 3 stop hit `sending` (re-sent with the same key, 1 job); an
    earlier Chrome run hit `accepted`.
- **B2.14 accessibility (done):**
  - WCAG 2.2 AA contrast tests: desktop `src/ui/contrast.test.ts` (31) and extension `src/contrast.test.ts` (13),
    light and dark;
  - focus returns after the History confirmation (test);
  - the extension options page has forced-colors and reduced-motion rules; its activity region announces additions
    only;
  - Options at 320 px in Bangla/dark: no overflow (browser preview).
- **Fixes found by the first packaged smoke** (all fixed and re-verified):
  - the browser cancel could be attributed to the user (stored `cancelRequested` flag now; unit test);
  - the harness counted another run's job (now matches this run's fixture origin);
  - a small POST export could finish before the decision (export slowed).
- **Final checks (Windows 11 Pro 10.0.26300 x64, 2026-10-31):**

  | Check | Result |
  |---|---|
  | `pnpm verify:strict` | **20/20 PASSED**, 0 unavailable |
  | extension vitest | 69 passed |
  | desktop vitest | passed |
  | `chrome-handoff.mjs --kind chrome` | 16/16 (C0–C5) |
  | `--kind edge` | C0–C5 passed (standalone run 5; and in the smoke) |
  | packaged smoke `smoke-results/windows-x86_64-b212b` | **verdict passed**, 84 rows, 0 failed (L1/L2, R3 Chrome + Edge C0–C5 against the installed host, all A–K/R/D) |

- **Not run:** macOS and Linux (CI after a push); the browser's optional-permission prompts; real right-click;
  tray; OS logout/sleep/reboot; screen readers.
- **Docs:**
  - ADRs 0018 and 0019;
  - [batch report](docs/reports/b2.12-b2.14-batch.md);
  - requirements: CORE-10, BRW-03, BRW-04, BRW-06 `implemented`; BRW-10, E02, UI-02 `in-progress`;
  - traceability, architecture (§5 methods, §7 state, §8 capture and support matrix), design (History, extension
    options page), testing, setup (CLI history commands, capture how-to), roadmap.
- **Remote (074c378):**
  - CI #41: windows-latest and ubuntu-22.04 green; macOS and ubuntu-24.04 Chrome passed C0/C1/C2/C4 and failed
    C3/C5;
  - package smoke #31: only C rows failed on all three; L1/L2 passed.
  - Fixes are uncommitted: cancelRequested attribution, origin-matched counts, slowed POST and cookie downloads.
    Local results: Chrome 16/16, Edge 16/16. The C5 cause is unconfirmed (row details need authentication).
- **Exact next task:**
  1. The user commits the remaining 6 files and pushes. Then confirm C0–C5 on all runners in CI and package
     smoke. If C5 still fails, read the job artifacts (results.json) to see its reason before changing anything.
  2. The human acceptance pass: optional-permission prompts, right-click, tray, OS lifecycle, screen-reader spot
     check.
  3. The next batch: the remaining B2 items in the batch report §5.

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
See **Exact next task** in the current section at the top (B2.33, 2026-11-04). The list that stood here predated B2.12 and
is superseded.
