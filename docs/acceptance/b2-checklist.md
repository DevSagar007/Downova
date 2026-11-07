# Downova B2 acceptance checklist (installed build, human and agent checks)

Self-contained: a tester needs only the acceptance kit, not the repository. The source of these rows is
[the installed-app runbook](../reports/b2.32-installed-acceptance-runbook.md) (C, F, U) and the
[B2.24–B2.27 report §7](../reports/b2.24-b2.27-batch.md#7-acceptance-checklist-human-checks-build-identity-in-5)
(H, A, N).

**Rules:**
- Use a **clean VM or a separate test OS account**, never your everyday account.
- Use only the **normal** package from the kit. `KIT-INFO.json` must say `"smoke_feature": false`.
- **Record every row** in `RESULTS.csv` with:
  - the platform, OS version, browser and its version;
  - the package SHA-256;
  - the observer: `human` (a person looked), `agent` (an AI agent drove the app and read the screen) or `automation`;
  - the result: `PASSED`, `FAILED` or `NOT RUN`;
  - the evidence: a screenshot or file name, kept in `evidence/`.
- **Page zoom and OS display scaling are different checks.** Page zoom is Ctrl/⌘ + inside the app; OS display
  scaling is set in Windows Settings → Display, macOS Displays, or GNOME Displays. Write which one you used.
- Never mark a screen-reader or real-click row PASSED because an automated test passed.

## 0. Setup

| ID | Steps | Expected |
|---|---|---|
| S1 | Verify the kit: `node verify-kit.mjs` (in the kit folder) | Every file matches `SHA256SUMS.txt`; `KIT-INFO.json` shows the package SHA-256 values and `"smoke_feature": false` |
| S2 | Start the scenario fixture: `node fixtures/acceptance/fixture-server.mjs --port 8765`. Open `http://127.0.0.1:8765/` in Chrome and Edge | The page lists scenarios and the SHA-256 of each file. It binds `127.0.0.1` only: run it on the test machine |
| S3 | Start the large-file fixture: `node fixtures/perf/fixture-process.mjs --count 7 --size-mib 256 --port 8766` | Its first line is JSON with `base` (`http://127.0.0.1:8766`) and the SHA-256 of `f0.bin`–`f6.bin`. Save that line to `evidence/fixture-hashes.json` |
| S4 | Install (as the test user). **Windows:** run `Downova_0.1.0_x64-setup.exe` (per-user) and note the folder on its folder page (default `%LOCALAPPDATA%\Downova`). **macOS:** the `.dmg`, move Downova to Applications (it must stay at `/Applications/Downova.app`), open once; the build is ad-hoc signed and not notarised, so the first open may need right-click → Open. **Linux:** `sudo apt install ./package/<the .deb file>` (its name is in `KIT-INFO.json`), then open Downova once as the desktop user; the AppImage does not register the browser helper while it runs, so use the .deb for every browser row | Downova opens. The window says the download service is connected |
| S5 | Chrome and Edge → Extensions → Developer mode → Load unpacked → `extension/` from the kit. Open its popup | The popup says "Service ready" |

`dm` (the command-line client) is in the install folder:
- Windows: the folder from S4;
- macOS: `/Applications/Downova.app/Contents/MacOS/`;
- Linux: `/usr/bin/`.

## 1. Downloads and limits

URLs are `<base>/f0.bin?slow=100` … `<base>/f6.bin?slow=100` (about 0.6 MB/s per request, so they run for minutes).

| ID | Steps | Expected |
|---|---|---|
| C1 | Options → Simultaneous downloads | "What this allows": saved limit 3, Main 3, "Main can run at once 3", "Active now 0" |
| C2 | Set the limit to 6, Save. Press **Change queue limit**, set Main to 6 → Set limit, close Manage queues | Options shows Main 6 and "Main can run at once 6". `dm concurrency` → 6; `dm queues` → main `max_active` 6 |
| C3 | Add `f0`–`f5` (Downloads → Add URL, six times) | All six rows show a rising percentage before any finishes |
| C4 | **Lowering never stops running work.** While the six run, set the limit to 2 and Save. Add `f6` | All six keep gaining bytes for 30 s. "Active now 6" with the note "More downloads are active than the limit…". `f6` waits (`dm get <JOB-ID>`: phase `queued`, blocker `queue_limit`) |
| C5 | **Raising admits only when there is capacity.** Set the limit **and** Main to 7 | `f6` starts within a few seconds (7 active). Raising back to 6 alone would not start it: six already run |
| C6 | Select one running download → Stop (pause) | It gains no bytes for 30 s; the others continue |
| C7 | Downloads → Stop queue (Main); wait 30 s; Start queue | While stopped, no Main download gains bytes. After Start they continue, **except the one paused in C6**, which stays paused |
| C8 | Cancel one running download | It ends cancelled; there is no final file with its name; the others continue |
| C9 | Tray icon → Exit and stop downloads → confirm | The window and the service end (no `dm-service` process) |
| C10 | Open Downova again | The limits are still 7 and 7. **Nothing starts by itself**: the unfinished downloads are paused at their progress. Select each → Resume: it continues from its progress, not from zero (the fixture's `<base>/__stats` shows a ranged request starting past 0) |
| C11 | Wait for completion; hash each file: Windows `Get-FileHash <file> -Algorithm SHA256`, macOS `shasum -a 256 <file>`, Linux `sha256sum <file>` | Every completed file matches `evidence/fixture-hashes.json`; the cancelled one has no file |

## 2. Browser (real clicks)

| ID | Steps | Expected |
|---|---|---|
| H1 | Extension options → enable automatic capture → add the site `127.0.0.1`; later remove it | The browser asks for the optional permissions at each step; removing gives them back |
| H2 | On the fixture page, **right-click** `public.bin` → Download with Downova | One Downova job; the file's SHA-256 equals the page's; the popup says "Accepted by Downova" without claiming the transfer finished |
| H3 | Switch the browser UI language to Bangla; open the popup and the context menu | Popup states, buttons, readiness and menu text in Bangla; nothing is sent twice |
| F1 | Tray → Exit and stop downloads. Then right-click a fixture link → Download with Downova, and (capture on) click `public.bin` | The popup says Downova is unavailable and to open the app; with capture on, the browser keeps and finishes the download; no job appears after reopening Downova |
| F2 | In the install folder: `dm-browser-host status --browser chrome --browser edge`, then `dm-browser-host unregister --browser chrome --browser edge`, then right-click a fixture link. Finally `dm-browser-host repair --browser chrome --browser edge` | `status` shows both as owned. After `unregister` the popup says the helper is not installed and the browser keeps the download. After `repair` the popup says "Service ready" again |

## 3. Desktop

| ID | Steps | Expected |
|---|---|---|
| H4 | Close the window (it hides to the tray); tray → Open Downova; tray → Exit and stop downloads | Each acts once; the exit asks for confirmation |
| H5 | Options → Downloads → Browse…: choose a folder; open again and Cancel; Save | Cancel changes nothing; only a saved folder is used, and only by new downloads |
| H6 | File → Open containing folder for a completed file, a running download, a deleted file, and a removed USB drive | The file is selected (never run); then the folder; "no longer in it"; the drive message |
| H7 | Fixture page scenarios R (Retry), E (expired link → Refresh link), C (changed file → Restart), S (checksum) | As described on the fixture page, with its hashes |
| H8 | With a download running: log out and back in; sleep and wake; reboot | It resumes or reports clearly; never a corrupt "completed" file |

## 4. Accessibility and display

| ID | Steps | Expected |
|---|---|---|
| H9 | With a screen reader (Windows: NVDA or Narrator; macOS: VoiceOver; Linux: Orca): the Retry and Refresh link dialogs, the details panel, the extension popup | Labels, focus order, pending and error announcements, each spoken once |
| A1 | Same screen reader, a list of 10,000 records: ↑/↓, Page Up/Down, Home/End, Alt+↑/↓ on a queued row. (Seeding needs the repository: `node scripts/lib/seed-history.mjs`; else use the longest list you have and note its size) | "Row n of N" is announced; focus never lands on blank space; reordering is announced once |
| A2 | Remove the focused row (File → Remove from list…); then remove a row from another client (`dm remove <JOB-ID>`) | Focus moves to the next row and is announced; never to the page |
| A3 | At **200 % OS display scaling**, then separately at **200 % page zoom**: a narrow window, light and dark, English and Bangla, a 200-character Bangla file name, **with a download selected** | No clipped controls, no horizontal page scroll; the name stays identifiable; at least one list row stays visible and the details panel scrolls instead of covering the list |

## 5. Notifications (installed build)

| ID | Steps | Expected |
|---|---|---|
| N1 | Notifications on; complete a download with the window visible, then hidden to the tray, then closed (service running) | One notification per completion in all three states |
| N2 | Block Downova's notifications in the OS; complete a download | Options reports the delivery failure once; the download is unaffected |
| N3 | Language Bangla; complete one download and fail one for good | Bangla title and text; no link, path or credential in them |
| N4 | Select a notification | It may bring Downova forward; it never opens or runs the file |

## 6. B3 feature builds (only when this kit was built from a B3.1, B3.2 or B3.3 feature package)

Skip this section, marked NOT RUN, for a B2 package. These rows are not B2 gates.

| ID | Steps | Expected |
|---|---|---|
| L1 | Options → Speed limit: limit the overall speed to 500 KB/s and save; start a large fixture download; select it and set its own limit to 200 KB/s | The transfer settles near each limit (the details show the saved values); changing a limit does not restart the download |
| L2 | Downloads → Manage queues → a new queue → Schedule: today, starting 2 minutes from now for 2 minutes; add a download to it; pause another one in it | Before the window: "Waiting: queue schedule" and the next window in your time zone. In the window: it runs; the paused one stays paused. After the window: it waits again |
| L3 | Restart the computer (or sign out and in) during L2's wait | The limit and the schedule are kept; nothing starts outside the window |
| W1 | **Windows:** download the fixture's `public.bin`; when it completes open its details; in a terminal `Get-Content -Path <file> -Stream Zone.Identifier` | Details: "Checked by Windows Attachment Services" with "not a malware verdict"; the stream shows `ZoneId=3` and a `HostUrl` that is only `http://host:port/` (no path, no query) |
| W2 | **Windows (test VM only):** with Microsoft Defender on, download the EICAR anti-malware test file (from eicar.org; the fixture does not serve it) | Defender removes it; Downova shows "Blocked by Windows security", the download is failed, and no copy (no `.downova-part`) remains |
| W3 | **Windows:** download to a FAT32 or exFAT USB stick (a destination rule) | It completes; details say the drive cannot store Mark of the Web; nothing claims it is safe |
| W4 | **macOS / Linux:** complete any download; open its details | "No malware check on this system"; nothing says Windows checked it |
| D1 | Help → Diagnostics…: read the preview; turn on the crash-related section; Save… to the Desktop; open the saved file in a text editor | The preview and the file are identical; no file names, links, sites, folders, cookies or tokens appear; nothing was sent (works offline) |
| D2 | Save diagnostics again onto the same name and confirm the replace; then try a read-only or full drive; cancel the save dialog once | Replace only after confirming; a clear "not saved" reason; cancel writes nothing |
| K1 | **B3.3, Chrome or Edge (normal window):** sign in to a test site you control over https; right-click a file link on its page → “Download with Downova using my sign-in (this time only)”; allow the prompt (cookies for that one site) | One prompt naming only that site; the popup says it uses your sign-in for that site, temporarily; the file completes; Downova's details show the site, the browser and “Kept only in memory”; no cookie or token appears anywhere in Downova |
| K2 | Repeat with “… and remember it”; restart the computer during a large download; then Forget sign-in in the details | After the restart it continues with the remembered sign-in (Windows: an entry under “Downova/…” in Credential Manager); Forget removes that entry and pauses the download, which then needs the browser again |
| K3 | Sign out on the site (or let the session expire), then resume a signed-in download; and right-click a link to another site, and an http link, on the same page | The download stops with “The website no longer accepts this sign-in” and says to start it again from the browser; “… using my sign-in” on the other-site link is refused in the browser and nothing is sent; an http link does not offer it at all |
| K4 | **macOS / Linux:** K1 and K2 with the Keychain or Secret Service (on Linux also with the keyring locked) | Same results; a locked store gives a clear message to unlock and restart, never a plaintext fallback |
| K5 | **Reconnect after a restart:** start a large signed-in download (“this time only”); when it is about half done, Exit Downova (tray → Exit) and open it again | The download shows “Waiting: sign-in needed” with the part already saved, and how to reconnect; nothing more is downloaded |
| K6 | On the site in the same browser profile, right-click the page → “Reconnect a Downova download with my sign-in”; on the page that opens, choose that download and press “Reconnect and resume” | One prompt at most; nothing is preselected; the page says it reconnected; the same download continues from its saved part (not from 0) and completes; no cookie appears anywhere in Downova |
| K7 | Pause a signed-in download, sign out on the site and back in, then reconnect it with “Reconnect” (not “and resume”); also try the reconnect menu inside an embedded frame of another site, and from another browser profile | It stays paused until you resume it; the frame is refused (“belongs to another site”); the other profile is offered nothing |

## 7. Uninstall

| ID | Steps | Expected |
|---|---|---|
| U1 | First place an unrelated manifest beside Downova's (copy any `com.downova.host.json` to `com.example.test.json`, change its `name` inside to `com.example.test`; on Windows also add `HKCU\Software\Google\Chrome\NativeMessagingHosts\com.example.test` with its path). Then uninstall: **Windows** Settings → Apps → Downova; **macOS** `dm-browser-host unregister --browser chrome --browser edge`, then move the app to the Trash; **Linux** the same `unregister` as the desktop user, then `sudo apt remove downova` | Downloaded files and the download folder stay. Only Downova's own entries are removed; the `com.example.test` manifest and value remain |

## Cleanup (after the results are saved)

- **Keep:** `RESULTS.csv` and `evidence/`.
- **Delete:** downloaded test files only after hashing them (C11).
- **Remove the test-only entry:** `com.example.test` manifest and registry value.
- **Test VM:** discard the snapshot.
- **Test account:** remove it with the OS's own tools after copying the results.
