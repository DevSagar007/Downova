# IDM parity inventory and Add URL matrix (2026-11-14)

- **Scope:** an inventory of user-visible features against Internet Download Manager, the Add URL fixture matrix
  run through the real window, and the fixes it led to.
- **Not a claim of parity:** "PASSED" below means the Downova behaviour was exercised. **No IDM behaviour was
  observed by the agent** (see Method).

## Method

- **IDM side:**
  - installed version: **IDM 6.43 build 16 (Trial)** (`HKCU\Software\DownloadManager` `idmvers`);
  - feature list from the official [feature page](https://www.internetdownloadmanager.com/features2.html) and
    [help index](https://www.internetdownloadmanager.com/support/using_idm/using_idm.html);
  - running the same fixtures through IDM's command line (`IDMan.exe /d URL /p DIR /n`) wrote no file within
    about a minute, and IDM's window cannot be observed by the agent, so **every IDM-observed column is NOT
    RUN**.
- **Downova side:**
  - every menu entry, toolbar button and Options section was read from the running window over WebView2 CDP;
  - each disabled control's stated reason was recorded;
  - downloads were driven through the real dialog (native Tauri window, service, disk) against local fixtures,
    and completed files were checked by SHA-256;
  - builds: the `2ede2b2` normal kit (MSI extract, isolated data) and this branch's source build (debug, isolated
    data), both on Windows 11.

## Add URL matrix (real dialog, native window, Windows 11)

| Case | Result | Status |
|---|---|---|
| plain file, redirect, query string (kept exactly), no extension, Unicode/percent-encoded name, `Content-Disposition` name, HEAD refused, unknown length, Range ignored, surrounding whitespace | added and completed; every SHA-256 equals the fixture | PASSED |
| `ftp://`, text with spaces, empty | refused in the dialog with a specific message | PASSED |
| the same link twice | the existing download is offered (show / download again / cancel) | PASSED |
| 401 | `AUTH_REQUIRED`, sign in | PASSED |
| 403, 404 | `LINK_REFRESH_REQUIRED`, refresh link | PASSED |
| 429, 500 | `SERVER_BUSY`: retried automatically with `Retry-After` and a cooldown blocker (1 + 3 requests), then failed, because the fixture always refuses | PASSED |
| truncated body | the "connection failed or was interrupted" error | PASSED |
| HTML sign-in page served for `login.zip` | **before:** saved and completed as `login.zip`; **after:** `LINK_REFRESH_REQUIRED`, "the server sent a web page instead of the file" | FAILED → fixed (`crates/backend-http/src/transfer.rs`, test NB04) |
| real `page.html` | completed (a page is a legitimate download) | PASSED |
| redirect loop | **before:** `INVALID_REQUEST` with no action; **after:** `LINK_REFRESH_REQUIRED`, "the site kept redirecting" | FAILED → fixed (NB05, D05) |

**The originally reported "Address (URL) does not start a download" was not reproduced.**
- **Ordinary links:** they add and complete through both the Add button and form submission.
- **Where the report came from:** a leftover NSIS test install in `%TEMP%\downova-smoke-*` running on the user's
  real data. It was removed while the agent restarted its window.
- **Browser preview:** `pnpm dev` disables Add entirely, and says why.
- **Needed to reproduce:** the exact URL and the dialog text.

## Control inventory (Downova window) against IDM

| ID | Downova control | IDM counterpart | Before | Now | Evidence |
|---|---|---|---|---|---|
| P-01 | Tasks → Add new download… / toolbar Add URL | Add URL | works | works | matrix above |
| P-02 | Tasks → Add batch download from clipboard | Add batch download from clipboard | disabled, "later milestone" | **works**: clipboard read by the desktop (the webview may not read it), links parsed, each added once, per-link results | `batch*.test.ts(x)`; native: 2 links added, SHA-256 OK |
| P-03 | Tasks → Import download list… | Import list | disabled, "later milestone" | **works** (.txt/.lst/.csv, ≤ 1 MiB) | test; native via `DOM.setFileInputFiles`: 2 links, SHA-256 OK |
| P-04 | toolbar Stop all / Downloads → Stop all | Stop All | disabled, "later milestone" | **works** (pauses every pausable download) | App test; native: 2 slow downloads paused |
| P-05 | toolbar Delete completed / Downloads → Delete all completed | Delete Completed | disabled, "later milestone" | **works** (Clear finished confirmation; also lists failed and cancelled downloads, IDM's removes completed only) | App test |
| P-06 | toolbar Scheduler / Downloads → Scheduler… | Scheduler | disabled, "later milestone" | **works** (opens Manage queues with schedules) | App test |
| P-07 | Downloads → Speed limiter… | Speed limiter | disabled, "later milestone" | **works** (opens Options → Speed limit) | App test |
| P-08 | File → Properties | Download Properties | disabled, "later milestone" | **works** (read-only facts from the service) | App test |
| P-09 | toolbar Delete | Delete (with or without the file) | disabled | **works**: Remove confirmation with an unticked "also delete the file" box for completed downloads (ADR 0033) | service, bridge, CLI and UI tests; native: file deleted, neighbour kept |
| P-10 | Resume / Stop / Start queue / Stop queue / Manage queues / Retry / Refresh link / Open folder / Queue and priority / Move ↑↓ / Clear finished / Options / View (categories, theme, language) / Help (updates, diagnostics, browser connections, notices, about) | Resume, Stop, queues, categories, Options, About | work | work | existing suites; not repeated in this run |

## Gaps (MISSING), in order, with existing requirements

| Order | Feature (IDM) | Requirement | Milestone | Acceptance (summary) |
|---|---|---|---|---|
| 1 | Clipboard monitoring (offer a copied link) | F02 | B4 | Off by default; offers only http(s) file links; never adds without a click |
| 2 | Drag and drop links into the window | F03 | B4 | A dropped link opens Add URL prefilled; dropped text uses the batch dialog |
| 3 | Shutdown/sleep after downloads | F08 | B4 | Explicit per-session choice, cancellable countdown, never with unfinished manual pauses lost |
| 4 | HTTP proxy (basic) | F06 | B4 | Per-profile proxy, credentials in the OS store; NTLM/Kerberos/SOCKS/PAC in B6 |
| 5 | FTP/SFTP | F05 | B4 | Resume with size and modification-time validation; SFTP host-key pinning |
| 6 | High-contrast theme | F16 | B4 | System high contrast respected; full custom skins deferred (B6) |
| 7 | Download quotas (MB per hour) | none | none | Owner decision: add a requirement or record "not planned" |
| 8 | Site grabber / spider | F09 | B6 (deferred) | Separate project |
| 9 | Periodic synchronisation | F10 | B6 (deferred) | Separate project |
| 10 | Video capture from web players / streams | E01 | B5 (deferred) | Separate project; legal review first |

## NOT RUN

- Every IDM-observed comparison: IDM's results could not be observed. **Side effect:** up to 18 test entries
  (`127.0.0.1:18433/…`) may remain in IDM's list.
- The new controls in macOS and Linux windows.
- Installed-build human rows.
- Performance comparison (REL-04): no comparable measurements, so no faster or lighter claim.
- The brief duplicate job keys React reported during 23 rapid adds: not reproduced in code. Every list merge is
  deduplicated and the final list was correct. A deterministic reproduction is needed.
  - **Update (2026-11-14, `fix/duplicate-row-keys`):** reproduced and fixed. The list was never duplicated. In
    the details panel, the speed limit control and the sign-in note were siblings with the same key (the job ID),
    so every selected unfinished download, including one just added, logged the warning.
