# Local Setup

## Prerequisites

| Tool | Version | Needed for |
|---|---|---|
| Node.js | ≥ 22.12 (tested 26.7.0) | all JS tooling |
| pnpm | 12.14.0 | workspace install (`npm i -g pnpm@12.14.0`, or `npx pnpm@12.14.0 <cmd>`) |
| Rust (rustup) | 1.99.1, auto-selected by `rust-toolchain.toml` (MSRV 1.99) | crates, Tauri shell |
| Platform build tools | see below | compiling Rust/Tauri |

Platform prerequisites for Tauri (from the official Tauri v2 prerequisites guide):
- **Windows:** Microsoft C++ Build Tools ("Desktop development with C++") and WebView2 Runtime (preinstalled on Windows 11).
- **macOS:** Xcode Command Line Tools (`xcode-select --install`).
- **Ubuntu 22.04/24.04:** `libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev`.

aria2 (optional since [ADR 0005](decisions/0005-default-backend.md); needed only for `--backend aria2` and the `aria2` verify group):
- **Windows:** `pnpm fetch:aria2` downloads the pinned aria2 1.37.0 build into `tools/aria2/` and checks its SHA-256.
- **macOS:** `brew install aria2`. **Linux:** `sudo apt-get install aria2` (Ubuntu 22.04 ships 1.36.0; the adapter reports the actual version and whether it equals the pin).
- Or point `DM_ARIA2_PATH` at any aria2c binary.

## First run

```bash
pnpm install
```

```bash
pnpm verify
```

When Rust is installed for the first time, generate and commit the lockfile once (verify uses `--locked`):

```bash
cargo generate-lockfile
```

## Running the download service (B0.2)

From the repository root (no aria2 needed: the default backend is the built-in HTTP backend, ADR 0005):

```bash
cargo run -p dm-service -- --download-dir ./tmp-downloads
```

- Data defaults to the per-user app-data folder (`%LOCALAPPDATA%\Downova`, `~/Library/Application Support/Downova`, or `~/.local/share/downova`). Override it with `--data-dir` or `DOWNOVA_HOME`.
- Downloads default to `~/Downloads`. Override with `--download-dir` or `DOWNOVA_DOWNLOAD_DIR`.
- Stop the service with Ctrl+C or `service.v1.shutdown`.
- Exit codes: 0 clean stop, 2 bad or conflicting arguments (unknown `--backend`, an option given twice, `--aria2` without `--backend aria2`), 3 already running, 4 startup failure (for example `--backend aria2` without an aria2c; there is never a fallback to the other backend).
- `http://` and `https://` adds download with the default backend: one connection for small or unsuitable files, up to 4 validated segments for large files with a strong ETag and a length, validated pause/resume.

### Where the service keeps its state (B1.1)

- Jobs and idempotency records are kept in `<data>/downova.sqlite3`. `<data>` is the per-user data directory, or
  `--data-dir` / `DOWNOVA_HOME`.
- Only the running service opens this file; use `dm list` to see jobs.
- A download that was unfinished when the service stopped is listed as failed with an "interrupted" reason; continuing it
  is B1.2.
- To start from an empty list, stop the service and delete the file.

## Using the CLI (B0.3)

With the service running (previous section), from another terminal:

```bash
cargo run -p dm-cli --quiet -- list
```

```bash
cargo run -p dm-cli --quiet -- --json add "https://example.org/file.zip"
```

- Commands: `add <URL> [--profile ID] [--idempotency-key KEY] [--refuse-duplicate] [--category CAT] [--queue QUEUE-ID]`, `list`, `get|pause|resume|cancel|remove <JOB-ID>`, `shutdown [--pause-downloads]`.
- Queues (B2.11): `queues`, `queue-create <NAME> [--max-active N]`, `queue-start|queue-stop <QUEUE-ID>`, `queue-delete <QUEUE-ID> [--move-to QUEUE-ID]` (never deletes downloads or files), `set-queue <JOB-ID> <QUEUE-ID>`, `priority <JOB-ID> <low|normal|high>`, `move <JOB-ID> <up|down|top|bottom>`.
- History retention (B2.12): `history`, `history-set <DAYS|forever>`, `history-preview <DAYS|forever>` (changes nothing), `history-clean` (list entries only, never files).
- B2.16–B2.19 (ADR 0020): `add <URL> --sha256 HEX` (the publisher's SHA-256; the download completes only when it matches), `retry <JOB-ID> [--discard-partial]` (a failed download starts again from zero as the same job; `--discard-partial` confirms discarding partial data), `refresh-link <JOB-ID> <NEW-URL> [--restart]` (a paused or failed download; progress is kept only when proven, otherwise nothing changes until `--restart`), `locate <JOB-ID>` (folder and, once completed, file). Each Retry/Refresh-link invocation has one operation ID, and a lost acknowledgement is repeated with it once.
- Notifications (B2.23): `notifications`, `notifications-set <on|off> [--locale en|bn]` (off by default; turning on shows
  one confirmation; finished downloads from before are never notified).
- `--refuse-duplicate` (B2.8) refuses an add whose link is already listed (`DUPLICATE_DOWNLOAD`, exit 4); without it an add behaves exactly as before.
- Global options (allowed anywhere): `--json`, `--data-dir DIR`, `--help`, `--version`.
- Exit codes: 0 ok, 1 unexpected, 2 invalid arguments, 3 service unavailable, 4 rejected by the service.
- `http://` and `https://` adds download; poll with `dm get <JOB-ID>` until `completed`. `dm pause` / `dm resume` continue a paused job only when the server sent a strong ETag and a length (otherwise resume is refused with exit 4).
- After a failure the service retries transient errors itself from zero (at most 3 times, honouring `Retry-After`). Anything else waits for `dm retry` or `dm refresh-link`.
- With `--backend aria2`, `https://` adds are refused (`Https` unverified) and partial resume is refused (ADR 0003).

## Choosing the backend (ADR 0005)

`dm-service` uses `dm-backend-http` by default (no external program). aria2 is opt-in, with the restrictions of ADR 0003:

```bash
cargo run -p dm-service -- --backend aria2 --download-dir ./tmp-downloads
```

`--aria2 PATH` selects a specific aria2c and is only valid together with `--backend aria2`. The backend is fixed for the life of the process.

The HTTP backend's acceptance tests run without aria2:

```bash
cargo test -p dm-backend-http -- --nocapture
```

```bash
cargo test -p dm-service --test http_backend_process -- --nocapture
```

## Reproducing the B0.4 HTTP correctness evidence

All three need aria2c (`pnpm fetch:aria2` on Windows). Each prints one `CASE …` line per scenario.

```bash
cargo test -p dm-backend-aria2 --locked --test http_correctness -- --ignored --nocapture --test-threads=2
```

```bash
cargo test -p dm-service --locked --test process -- --ignored --nocapture
```

```bash
cargo test -p dm-cli --locked --test real_service -- --ignored --nocapture
```

Manual end-to-end download: start the service (above), serve any file over plain HTTP on loopback (for example `python -m http.server 8000` in a folder you control), then run `dm --json add http://127.0.0.1:8000/<file>` and `dm get <JOB-ID>`, and compare the SHA-256 of the result with the original (`certutil -hashfile <file> SHA256` on Windows, `sha256sum` elsewhere).
- Idempotency keys are remembered only while the service runs (in memory, B0). Durable keys are B1.

## Everyday commands

| Command | Purpose |
|---|---|
| `pnpm dev` | Desktop UI in the browser at http://localhost:1420 (no Tauri, no service) |
| `pnpm dev:desktop` | Full Tauri window (needs Rust) |
| `pnpm verify` | All available checks; prints PASSED/FAILED/UNAVAILABLE |
| `pnpm verify:strict` | Fail if any check is unavailable (CI) |
| `pnpm lint` / `pnpm typecheck` / `pnpm test` / `pnpm format` | Individual JS checks |
| `pnpm contracts:gen` / `pnpm contracts:check` | Regenerate or check contract types |
| `pnpm check:structure` | Structural checks only |
| `cargo test --workspace` | Rust tests |

Regenerate the app icons from `apps/desktop/src-tauri/app-icon.svg`:

```bash
pnpm --filter @dm/desktop tauri icon src-tauri/app-icon.svg
```

Then delete the generated `android/` and `ios/` folders until B7.

## Building native packages (B0.5)

Release sidecars plus Tauri bundles for the current OS (Windows NSIS + MSI, macOS `.app` + `.dmg` ad-hoc signed,
Linux `.deb` + AppImage). Tauri downloads its NSIS/WiX/linuxdeploy helpers on first use.

```bash
node scripts/package.mjs
```

Outputs land in `dist-packages/<os>-<arch>/` with `SHA256SUMS.txt` and `build-info.json`. aria2 is never bundled.
`--smoke` builds an internal test package with the desktop's test-only `smoke` feature
(`dist-packages/<os>-<arch>-smoke-test/`), which the packaged-app smoke test needs:

```bash
node scripts/package.mjs --smoke
```

```bash
node scripts/smoke/packaged-smoke.mjs --package-dir dist-packages/windows-x86_64-smoke-test
```

The smoke test installs (or unpacks) the package into a temporary directory, uses only the packaged binaries with
isolated data and download directories, and writes `smoke-results/<label>/results.json`, `summary.md`, logs and a
window-only screenshot. On Linux run it under `xvfb-run -a`. The same steps run in
`.github/workflows/package-smoke.yml`. Details: [B0.5 report](reports/b0.5-packaging-smoke.md).

The packaged desktop finds the running per-user service through its private endpoint and `hello`, or starts the
`dm-service` installed next to it; closing the window leaves the service and its downloads running.
`dm hello` shows which backend the running service uses.

### Selected platform scope (B3.7)

Seven checks run for every integrated source:

| Workflow | Runner | Target |
|---|---|---|
| CI `verify` (`pnpm verify:strict`) | windows-latest | Windows x86_64 |
| CI `verify` | macos-latest | macOS aarch64 |
| CI `verify` | ubuntu-22.04 | Linux x86_64 |
| CI `verify` | ubuntu-24.04 | Linux x86_64 (newer Ubuntu compatibility) |
| Package smoke | windows-latest | `windows-x86_64` (NSIS, MSI) |
| Package smoke | macos-latest | `macos-aarch64` (`.app`, `.dmg`, ad-hoc signed) |
| Package smoke | ubuntu-22.04 | `linux-x86_64` (`.deb`, AppImage) |

- **Deferred, not built and not tested:** Windows x86 (32-bit) and macOS x86_64 (Intel).
- **Linux package smoke:**
  - runs in a disposable D-Bus session with a disposable Secret Service (`scripts/smoke/linux-secret-service.sh`,
    proven by a round trip first);
  - a Linux desktop without a Secret Service keeps protected link parts in memory only (ADR 0027). After a
    restart such a download asks for the link to be refreshed.
- A target is "supported" only with recorded passes (REL-02).

## Trying the browser hand-off (B0.6, development)

The extension in `apps/extension` adds **Download with Downova** to the context menu of http(s) links. Its
development ID comes from the public key in `apps/extension/manifest.ts` and is `bkmpnfclpdmcbdgmeakmnoilmkdmdppf`.
`dm-browser-host` allows exactly that origin, `chrome-extension://bkmpnfclpdmcbdgmeakmnoilmkdmdppf/`. The host never
starts the service, so Downova (or `dm-service`) must already be running.

1. **Build** the service, the CLI, the host and the extension:

   ```bash
   cargo build -p dm-service -p dm-cli -p dm-browser-host
   ```

   ```bash
   pnpm --filter @dm/extension build
   ```

2. **Copy the host to a stable location.** The manifest records an absolute path, so do not point it at a folder you
   will clean. Registration refuses relative paths and AppImage mounts.
   - Windows: `%LOCALAPPDATA%\Downova\bin\dm-browser-host.exe`
   - macOS/Linux: `~/.local/share/downova/bin/dm-browser-host`

3. **Register it for your user.** `--browser` may be repeated: `chrome`, `chrome-beta`, `chrome-dev`, `chrome-canary`
   (not on Linux), `chrome-for-testing`, `chromium`, `edge`, `firefox` (or `all`). Run the command from the copied executable (or pass
   `--host-path`). If an installed Downova already registered itself, the command refuses to replace it; add
   `--replace` to switch to your development copy (and run `repair` from the installed one to switch back):

   ```bash
   dm-browser-host register --browser chrome
   ```

   Where the registration goes:

   | OS | Manifest | Browser lookup |
   |---|---|---|
   | Windows | `%LOCALAPPDATA%\Downova\native-messaging\<chrome\|edge\|chromium>\com.downova.host.json` (Chrome's channels and Chrome for Testing share `chrome`) | `HKCU\Software\Google\Chrome\NativeMessagingHosts\com.downova.host` (Edge: `Microsoft\Edge`, Chromium: `Chromium`) points to the manifest |
   | macOS | `~/Library/Application Support/Google/Chrome/NativeMessagingHosts/com.downova.host.json` (Edge: `Microsoft Edge`, Chromium: `Chromium`, Chrome for Testing: `Google/ChromeForTesting`) | the folder itself |
   | Linux | `~/.config/google-chrome/NativeMessagingHosts/com.downova.host.json` (`$XDG_CONFIG_HOME` instead of `~/.config` when set; Edge: `microsoft-edge`, Chromium: `chromium`, Chrome for Testing: `google-chrome-for-testing`) | the folder itself |

   Related commands (ADR 0014):
   - `dm-browser-host status --browser chrome [--json]` shows the registration and whose it is (this installation,
     another Downova installation, a removed one, unreadable, or a different host).
   - `repair` rewrites this installation's entries; it refuses another installation's or an unreadable entry unless
     `--replace`, and never replaces a different host's.
   - `unregister` removes only entries whose manifest names this executable; everything else is kept and listed.

4. **Start Downova:** open the app, or run:

   ```bash
   cargo run -p dm-service
   ```

5. **Load the extension:** open `chrome://extensions`, turn on **Developer mode**, choose **Load unpacked** and select
   `apps/extension/dist`. The card must show ID `bkmpnfclpdmcbdgmeakmnoilmkdmdppf`. The extension is not allowed in
   incognito windows.

6. **Check the service:** click the Downova toolbar icon. The popup should say **Service ready**. If it says **Service
   unavailable**, its text says what to do: open Downova, or install or repair the browser helper.

7. **Download:** right-click a public http(s) link and choose **Download with Downova**. The popup lists the hand-off:
   - **Accepted by Downova:** the service has the job. This is not completion; follow it with
     `cargo run -p dm-cli --quiet -- list`.
   - **Not accepted:** the reason is shown, for example a link with an embedded password.
   - **Downova unavailable:** nothing was sent.
   - **Not confirmed:** use **Retry**. It reuses the same idempotency key, so the service will not create a second job.
     This holds only while the service keeps running; restart-safe idempotency is B1.

   Links are shown without their query string. **Clear list** removes finished entries, and entries older than 7 days are
   dropped automatically.

8. **Undo:** remove the extension in `chrome://extensions`, then run:

   ```bash
   dm-browser-host unregister --browser chrome
   ```

**Automated real-browser test.** This drives an isolated, temporary Chrome profile over the DevTools pipe:

```bash
node scripts/e2e/chrome-handoff.mjs
```

- It writes `smoke-results/e2e-chrome-<os>-<arch>/results.json`.
- `--kind edge` targets Edge, `--browser PATH` picks a specific binary and `--headed` shows the window.
- On Windows it adds, and afterwards removes, the per-user `HKCU` key (restoring any previous value). On macOS and Linux
  the manifest stays inside the temporary profile.

- `--installed-dir DIR --data-dir HOME --download-dir DL --product-registration --handoff-only` runs steps 1–3
  against an installed package and the registration the product created (the packaged smoke uses this).

## Automatic capture in the browser (B2.13, ADR 0019)

1. Open the extension's popup and choose **Automatic capture settings**.
2. Turn on **Capture downloads automatically**. The browser asks for the downloads and request permissions.
3. Add each site to capture from (for example `downloads.example.org`); the browser asks for that site.
4. Optionally set exclusions, file types, MIME types and a minimum size.

Only plain public downloads (one GET, no cookies or sign-in, normal response, redirects inside your sites) are
taken over, after Downova accepted them. Everything else stays in the browser. Private windows are never
captured. Turning capture off or removing a site gives the permissions back.

## Measuring resources (B2.21, Windows)

```bash
node scripts/package.mjs
```

```bash
node scripts/perf/measure.mjs --package-dir dist-packages/windows-x86_64 --runs 5 --out perf-results/after
```

The tool measures the normal package extracted from its MSI, with isolated data, download and WebView2 folders. It
stops if a Downova desktop is already running and never ends a process it did not start. Close other heavy apps
first. Results are in `perf-results/<label>/` (`summary.json`, `samples.csv`, raw JSON per run); the method is in
`docs/testing.md` §9.

Only some scenarios, more runs, or the separate diagnostic runs (B2.24):

```bash
node scripts/perf/measure.mjs --package-dir dist-packages/windows-x86_64 --runs 10 --scenarios active_1,active_3 --out perf-results/active10
```

```bash
node scripts/perf/measure.mjs --package-dir dist-packages/windows-x86_64 --diagnose --slow-ms 60 --scenarios idle_visible,active_1,active_3 --out perf-results/diag
```

Diagnostic runs instrument the page; never mix them into resource medians.

## Hands-on fixture page (B2.6, extended for B2.15–B2.20)

```bash
node scripts/acceptance/fixture-server.mjs --port 8765
```

Open `http://127.0.0.1:8765/` in Chrome or Edge. Every scenario is deterministic, and the page prints each expected
SHA-256 (also printed in the terminal). **Reset scenarios** re-arms them.

| Scenario | What to do | Expected |
|---|---|---|
| P public download | Right-click `public.bin` → Download with Downova, or enable automatic capture for `127.0.0.1` and left-click it | Completes with the printed hash; with capture, the browser copy is cancelled only after acceptance |
| P fallback | Export (POST form), the blob: button, a cookie link | Stay in the browser; the extension's activity says why |
| R Retry | Right-click `retry.bin` | Fails once ("the file changed"); Retry… completes it as the same download |
| E expiring link | Start `expiring.bin`, pause it, select **expire the link**, reload the page, then Refresh link… with the new token link | Progress kept (no question); Resume completes with the printed hash |
| C changed file | Start `changing.bin`, pause it, select **Change the file**, then Refresh link… with `changing.bin?v=2` | "The progress cannot be kept"; Keep current changes nothing; Restart from zero completes with the *after* hash |
| S checksum | Add URL with the printed SHA-256, then with the wrong one | "Publisher checksum matched"; then a mismatch failure with no file placed |

## Browser integration after installing (B2.5, ADR 0014)

Installed Downova registers its browser helper (`dm-browser-host`) for **Chrome, Edge and Firefox, for the current
user**.
Nothing is registered system-wide, and the extension's development ID is the only allowed origin until store IDs exist.

| Package | Registers | Removes on uninstall |
|---|---|---|
| Windows NSIS (`-setup.exe`, per user) | the installer, then the first launch | the uninstaller (only this installation's entries) |
| Windows MSI | the first launch of the app, for each user who opens it | no hook: run the unregister command below first |
| macOS `.dmg` | the first launch **after moving Downova to Applications** (a launch from the disk image or a translocated copy is refused) | no hook (drag to Trash): run the unregister command below first |
| Linux `.deb` | the first launch, as the desktop user (the package scripts run as root and write nothing per user) | no hook: run the unregister command below before `apt remove` |
| Linux AppImage | refused while running from its mount; extract it (`--appimage-extract`) or use the `.deb` | — |

- **Open Downova once after installing.** The browser helper never starts the download service. Until the app has
  run (and after a reboot, until autostart arrives), the extension reports that Downova is not running.
- The first launch writes its outcome to `browser-setup.json` in the Downova data folder. It runs again after an
  update or a move, not on every launch, so an `unregister` you ran yourself stays in effect.
  `DOWNOVA_SKIP_BROWSER_SETUP=1` turns it off.
- **Repair** (for example after a browser profile reset). Run it from the installed helper, next to the app's
  executable; on macOS that is `/Applications/Downova.app/Contents/MacOS/`:

  ```bash
  dm-browser-host repair --browser chrome --browser edge --browser firefox
  ```

- **Before removing a package without an uninstall hook:**

  ```bash
  dm-browser-host unregister --browser chrome --browser edge --browser firefox
  ```

  If you skip it, the leftover entries point to a missing program. Browsers then report the helper as missing, and
  a later Downova install replaces them. Uninstalling never deletes downloads, the download list or settings.

- **In the app:** Help → Browser connections… shows each browser found on this computer, how it is installed
  (normally, Snap, Flatpak or unknown), whether its connection points at this Downova, and whether that combination is
  supported, with **Repair**. Repair never takes over another Downova installation's or another program's entry.

## Firefox (B3.4, ADR 0029)

The Firefox build offers **Download with Downova** for public http(s) links, with the popup's service check and
hand-off list. Downloads with your sign-in, reconnecting them and automatic capture are Chrome/Edge only for now:
Firefox does not show them, and every download Firefox starts itself stays in Firefox.

- **Build:** `pnpm --filter @dm/extension exec vite build --mode firefox` writes `apps/extension/dist-firefox/`
  (the Chromium build stays in `dist/`). Normal packages contain `extension-firefox-unpacked/` and
  `downova-firefox-unsigned.xpi`.
- **Load it (development):** Firefox → `about:debugging#/runtime/this-firefox` → **Load Temporary Add-on…** → choose
  `manifest.json` in the unpacked folder (or the `.xpi`). A temporary add-on is removed when Firefox closes.
- **Signed distribution:** not available yet. Release Firefox installs only add-ons signed by Mozilla; that needs an
  add-on ID the maintainer owns (the development ID is `downova-dev@extension.downova.invalid`).
- **Register the helper:** `dm-browser-host register --browser firefox` writes
  `HKCU\Software\Mozilla\NativeMessagingHosts\com.downova.host` and its own manifest (Windows),
  `~/Library/Application Support/Mozilla/NativeMessagingHosts/` (macOS) or `~/.mozilla/native-messaging-hosts/`
  (Linux). The manifest lists the add-on ID under `allowed_extensions`; Chrome's and Firefox's entries never
  overwrite each other.
- **Real-Firefox test:** `node scripts/e2e/firefox-handoff.mjs` (an isolated profile, a temporary add-on, WebDriver
  BiDi; any previous Firefox registration of `com.downova.host` is restored afterwards).

### Browser and package support

| OS | How the browser is installed | Chrome, Edge | Firefox | Chromium |
|---|---|---|---|---|
| Windows | normally | supported (tested in real browsers) | supported (tested in real Firefox, B3.4) | not tested |
| macOS | normally | supported (package smoke, B2.33; an earlier build) | not tested | not tested |
| Linux | normally (.deb/.rpm, tarball) | supported (package smoke, B2.33; an earlier build) | not tested | not tested |
| Linux | Snap | — (no official snaps) | not tested: may work through the native-messaging portal | not supported (sandbox) |
| Linux | Flatpak | not supported (sandbox) | not supported (sandbox) | not supported (sandbox) |

Package targets: Windows x86_64, macOS aarch64 and Linux x86_64 only (Windows x86 and macOS x86_64 are deferred; see "Selected platform scope").
"Linux works" never means a Snap or Flatpak browser works. Where a browser cannot reach Downova, its downloads stay
in the browser. `dm-browser-host status --browser all --json` and Help → Browser connections… show what was found on
this computer.

## Updates (B3.5, ADR 0030)

A normal package has no updater configuration: Help → Check for updates… says "Updates are not configured".
`scripts/package.mjs` refuses to build a normal package while `TAURI_SIGNING_PRIVATE_KEY`,
`TAURI_SIGNING_PRIVATE_KEY_PATH` or `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` is set.

- **Production (maintainer only; no production key exists yet):** an `updater.json` with
  `bundle.createUpdaterArtifacts: true` and `plugins.updater` (`pubkey`, https `endpoints`,
  `requireSignedVersion: true`), and the private key supplied from outside the repository:

  ```bash
  node scripts/package.mjs --updater-config /secure/updater.json
  ```

  Each package gets a `.sig` beside it. Publish `latest.json` with one `platforms` entry per package target
  (`windows-x86_64-nsis`, …) and re-sign with `tauri signer sign --app-version <version>` if an artifact is signed
  outside the build.
- **Fixture build (loopback HTTP, disposable key):** `--update-fixture` with a config whose endpoints are
  `http://127.0.0.1:<port>/…` and `dangerousInsecureTransportProtocol: true`. The package lands in
  `dist-packages/<os>-<arch>-update-fixture/` and `build-info.json` says `"updater": "fixture"`. Never ship it.
- **The real verifier with disposable keys** (part of `pnpm verify`'s rust group):

  ```bash
  node scripts/update/verifier-fixture.mjs
  ```

- **Recovery:** if a new version cannot open its data, `dm-service` exits with code 6 and writes
  `<data>/update/recovery.json`. Restore the checked pre-update backup from the app (Recovery required → Restore),
  or with the service stopped:

  ```bash
  cargo run -p dm-service -- --restore-update upd_0123456789abcdef
  ```

  Exit code 0 means restored, 5 means refused (unknown operation, changed or damaged backup, newer schema).

### Two fixture versions and the update journey (B3.6)

```bash
node scripts/update/build-fixture-pair.mjs
```

Builds fixture A (the workspace version) and fixture B (`--to`, default 0.1.1) with one disposable updater key
(deleted afterwards) and a loopback endpoint (`--port`, default 18765). Every component of B reports B:
`package.mjs --app-version` sets `DOWNOVA_BUILD_VERSION` for the Rust binaries and the Tauri config version for the
desktop, installer and update metadata, then checks `--version` of each binary (and, on Windows, of each binary in
the MSI). The pair is listed in `dist-packages/update-fixture-pair.json`.

```bash
node scripts/acceptance/update-journey.mjs
```

Windows only. Runs A → B through the real update dialog on MSI administrative extracts with an isolated data
folder, without running an installer (steps U1–U12, results in `acceptance-results/update-journey/`). The native
installer journey needs a clean VM or test account: checklist rows IL1–IL6 and UP2–UP3.

### Installer lifecycle (B3.6, ADR 0031)

- A per-user NSIS installation goes to `%LOCALAPPDATA%\Downova`, which is also Downova's data folder. The
  uninstaller removes only its own files; the download list, the update backups and downloaded files stay.
- Before an upgrade or uninstall the NSIS hooks run `dm shutdown --only-from "<install folder>"`: the download
  service is stopped (state kept) only if it runs from that folder. Another installation's service is left alone.
- The desktop attaches only to a service of its own build version; otherwise it says a different version is
  running.

## Third-party notices (B3.6, REL-03)

```bash
node scripts/notices/generate-notices.mjs
```

Regenerates `apps/desktop/src/generated/third-party.notices.json` after a dependency change (`pnpm verify` fails
while it is stale). Packages carry `THIRD-PARTY-NOTICES.txt` (installed beside the app and next to the
packages); the app shows the same list offline in Help → Third-party notices…. Non-package components (the Rust
standard library, NSIS, WiX) are listed in `third_party/notices/manifest.json` with verbatim texts beside it.
Missing license metadata is listed as UNRESOLVED, never filled in.

## Download concurrency

In Options → Simultaneous downloads, set Maximum active downloads (1–16; default 3) and Save.
Also open Downloads → Manage queues and set the relevant queue capacity: existing main queues remain
at three. For six downloads in main, set both values to six and ensure the queue is running. Paused
jobs stay paused. Lowering a limit lets running attempts finish and delays new starts.

CLI: `dm concurrency` reads the saved global setting; `dm concurrency-set 6` saves it. The per-queue
setting can be changed in the desktop Manage queues dialog. Same-host connections remain capped at
eight across all native HTTP jobs. Validated ranges share turns under contention; unsupported ranges
are not interrupted. The optional aria2 backend retains its conservative three-file cap.

Packaged acceptance of this behaviour (Windows; extracts the MSI, isolated folders, nothing installed):

```bash
node scripts/acceptance/concurrency-acceptance.mjs --package-dir dist-packages/windows-x86_64
```
