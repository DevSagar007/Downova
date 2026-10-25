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

## Using the CLI (B0.3)

With the service running (previous section), from another terminal:

```bash
cargo run -p dm-cli --quiet -- list
```

```bash
cargo run -p dm-cli --quiet -- --json add "https://example.org/file.zip"
```

- Commands: `add <URL> [--profile ID] [--idempotency-key KEY]`, `list`, `get|pause|resume|cancel <JOB-ID>`.
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
