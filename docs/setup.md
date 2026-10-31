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
- `--refuse-duplicate` (B2.8) refuses an add whose link is already listed (`DUPLICATE_DOWNLOAD`, exit 4); without it an add behaves exactly as before.
- Global options (allowed anywhere): `--json`, `--data-dir DIR`, `--help`, `--version`.
- Exit codes: 0 ok, 1 unexpected, 2 invalid arguments, 3 service unavailable, 4 rejected by the service.
- `http://` and `https://` adds download; poll with `dm get <JOB-ID>` until `completed`. `dm pause` / `dm resume` continue a paused job only when the server sent a strong ETag and a length (otherwise resume is refused with exit 4).
- After a failure the service retries transient errors itself from zero (at most 3 times, honouring `Retry-After`).
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

## Trying the browser hand-off (B0.6, development)

The extension in `apps/extension` adds **Download with Downova** to the context menu of http(s) links. Its
development ID comes from the public key in `public/manifest.json` and is `bkmpnfclpdmcbdgmeakmnoilmkdmdppf`.
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
   (not on Linux), `chrome-for-testing`, `chromium`, `edge`. Run the command from the copied executable (or pass
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

## Browser integration after installing (B2.5, ADR 0014)

Installed Downova registers its browser helper (`dm-browser-host`) for **Chrome and Edge, for the current user**.
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
  dm-browser-host repair --browser chrome --browser edge
  ```

- **Before removing a package without an uninstall hook:**

  ```bash
  dm-browser-host unregister --browser chrome --browser edge
  ```

  If you skip it, the leftover entries point to a missing program. Browsers then report the helper as missing, and
  a later Downova install replaces them. Uninstalling never deletes downloads, the download list or settings.
