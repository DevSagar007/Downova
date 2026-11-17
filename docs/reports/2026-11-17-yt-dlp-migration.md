# Optional aria2 to yt-dlp migration: implementation and verification

Date: 2026-11-17. Base: `c68f9706fd678270c1779743588a854581022e63`.
Environment: Linux x86_64, kernel 6.18.57, glibc 2.41, pinned Rust 1.99.1 and yt-dlp 2026.10.26.
This report describes local source checks, not a released installer or Windows/macOS acceptance.

## Result

The optional aria2 module, executable lookup, fetch command and CI installations are removed.
`dm-backend-ytdlp` occupies the same engine boundary, selected explicitly with `--backend yt-dlp`.
Native Rust remains the default for ordinary files. The UI's English/Bangla sidecar concurrency
explanation now names yt-dlp and agrees with the service's three-job sidecar cap. No schema,
SQLite migration, browser capture flow or installer target is added.

The media backend accepts one public media item, selects one already combined format using native
HTTP/HLS/DASH downloading, reports progress, and gives staged bytes to the normal completion
journal. The URL is piped over stdin, credentials/config/plugins are refused or disabled, and
diagnostics contain stable messages rather than raw engine output. Pause/cancel stop the owned
child; partial resume requires Restart. `pnpm fetch:ytdlp` verifies immutable upstream executable
sizes and SHA-256 for the existing three package platforms. The universal macOS tool also covers
an Intel verification runner without introducing an Intel desktop package target.

## Checks actually run

| Check | Result on this Linux environment |
| --- | --- |
| `node scripts/verify.mjs --strict --only=structure,contracts,format,lint,typecheck,test,build,ytdlp` | **18/18 passed**: structure and tooling, notice inventory, contracts, formatting, ESLint, all three TypeScript checks, desktop and extension tests/builds, real sidecar fixtures and coordinator integration |
| Desktop Vitest | **349 passed**, 38 files |
| Extension Vitest | **111 passed**, 8 files |
| `cargo test -p dm-backend-http --locked` | **156 passed**, no failures: unit and HTTP(S), authenticated GET, host budget, disk budget, FTP/SFTP, proxy, segmentation, speed and validated/durable resume fixtures |
| `cargo test -p dm-backend-ytdlp --lib --locked` | **5 passed**, including batch injection, credentials/userinfo refusal, safe cleanup and diagnostics |
| `node scripts/verify.mjs --strict --only=ytdlp` after final protocol allowlist | **2/2 groups passed**: 4 actual-binary fixtures plus 1 coordinator test |
| Actual-binary media fixtures | **4 passed**: local media page/direct URL with SHA-256; trusted HTTPS and certificate rejection; pause/no late progress; truncated response plus clean restart without Range |
| Actual-binary coordinator integration | **1 passed**: Add URL contract, idempotency, journal/finalization, trusted SHA-256, existing-name collision protection |
| `node --test scripts/lib/ytdlp.test.mjs` | **2 passed**: supported asset pins and corrupt cache refusal |
| `cargo clippy -p dm-backend-ytdlp -p dm-service -p dm-cli --all-targets --locked -- -D warnings` | **Passed** |
| `cargo fmt --all` / formatting verification | **Passed** |
| `cargo test -p dm-service --test sec03_no_execution` | **3 passed**: process allowlist and no downloaded-file execution |
| `http_backend_process` targeted backend flag and missing-sidecar checks | Passed: invalid/repeated flags, retired aria2 options, absent yt-dlp fails startup without fallback |

The first UI run found stale generated notices; regenerating the notice inventory fixed the real
failure. The final inventory has 496 distributed components and the existing one unresolved WiX
metadata item. yt-dlp is listed only as external, not as a distributed component.

## Blocked or not established

- The attempted service unit run (`cargo test -p dm-service --lib --test http_backend_process
  --test sec03_no_execution`) had **206 passed and 6 failed** in server/stream IPC setup. All six
  failed with `PermissionDenied / Operation not permitted` when creating Unix sockets. A separate
  minimal Python `socket(AF_UNIX)` probe failed before bind with the same error. This is a measured
  environment limitation, not a passing service IPC run. The non-IPC checks above ran independently.
- Full service/CLI IPC acceptance, native Tauri builds, installed desktop interaction, the normal
  Windows/macOS runs, signing, and packaged smoke were not run here.
- No YouTube or other public extractor was exercised. The offline generic fixtures establish this
  adapter's integration; they do not establish every site's availability or JS challenge support.
- No hard-killed-parent orphan guarantee, quality picker, separate-track FFmpeg merge, subtitle
  pipeline, authenticated media session, media capture button or automatic per-URL engine routing.
- Engine-specific old aria2 characterization suites are retired with their removed implementation;
  historical reports and commit references remain. The current IPC and native-engine suites are
  retained, and media-specific checks replace the old external-engine verification group.

## Next acceptance

Run the retained four-OS CI matrix manually on the review branch. On a normal machine, stop the
service, fetch/select yt-dlp, open the desktop and test Add URL with a public media link; confirm
filename, checksum, pause/restart, cancellation, shutdown and switching back to native mode.
Then resolve hard-kill child lifetime and decide whether to extend E01 with selection/merge stages.
Do not mark E01 complete or the application release-ready from this increment.

## Delivery

The connected GitHub integration rejected branch creation with HTTP 403 (`Resource not accessible
by integration`). No remote branch/commit/PR was created and main was not updated. A patch of the
local changes is checked against a clean worktree of the recorded base for the owner to apply.
