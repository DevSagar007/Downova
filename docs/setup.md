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

aria2 (needed for the `aria2` verify group, B0.1+):
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

On Windows, fetch aria2 first if you have not (`pnpm fetch:aria2`), then from the repository root run:

```bash
cargo run -p dm-service -- --download-dir ./tmp-downloads
```

- Data defaults to the per-user app-data folder (`%LOCALAPPDATA%\Downova`, `~/Library/Application Support/Downova`, or `~/.local/share/downova`). Override it with `--data-dir` or `DOWNOVA_HOME`.
- Downloads default to `~/Downloads`. Override with `--download-dir` or `DOWNOVA_DOWNLOAD_DIR`.
- Stop the service with Ctrl+C or `service.v1.shutdown`.
- Exit codes: 0 clean stop, 2 bad arguments, 3 already running, 4 startup failure (for example, aria2c not found).
- Until ADR 0003 promotes `Http`/`Https` for aria2, `downloads.v1.add` is refused with `BACKEND_CAPABILITY_UNAVAILABLE`. This is by design.

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
