# ADR 0042: yt-dlp replaces the optional aria2 adapter

- Date: 2026-11-17
- Status: implemented increment, acceptance incomplete; the whole-process `--backend yt-dlp` switch is
  replaced by per-download engine selection ([ADR 0043](0043-per-download-engine-selection.md))
- Owner request: remove the old optional aria2 integration and use yt-dlp.
- Supersedes: the optional-aria2 selection and tooling clauses of ADR 0005. The default native
  HTTP engine and its correctness guarantees remain in force.

## Decision and scope

`dm-backend-ytdlp` implements the existing `DownloadBackend` boundary. The service accepts
`--backend http|yt-dlp`; `http` remains the default for ordinary files. `--yt-dlp PATH` is valid
only with `--backend yt-dlp`. The desktop connects to that service through the existing IPC API.
There is no automatic URL routing or desktop engine picker in this increment. To use media mode,
stop the existing per-user service first, explicitly start the media service, then open the desktop.
The service never silently switches engines. An absent/broken selected executable fails startup.
The removed `--backend aria2` and `--aria2` flags fail with usage rather than selecting something else.

The yt-dlp path accepts public HTTP(S) media pages and direct media URLs, one media item per job.
It selects the best already combined format (`best`) and uses yt-dlp's native downloader. Playlists
are limited to the first selected item. Live/upcoming items are filtered. Separate audio/video
tracks, FFmpeg merge/postprocessing, a quality picker, subtitle export, media-specific browser
capture and scoped authenticated media are not implemented. E01 therefore remains **in-progress**.
An ordinary ZIP/EXE/ISO link belongs to the native engine. Upstream site support changes over time;
an offline generic HTML/WAV fixture does not establish YouTube or every extractor as working.

## Boundary, lifecycle and files

- One child per attempt, service-owned attempt handle; no engine PID becomes a public job ID.
- One private staging directory per service staging name, SHA-256-derived and on the destination
  volume. Fixed output `media`; no site title chooses a directory or an executable.
- Ambient config, plug-in directories, cache, external downloaders, internal continuation and
  retries are disabled. Node is enabled as a JS runtime for extractors that require one; the optional
  engine's prerequisites must be installed independently of the normal desktop package.
- URLs are piped through stdin as a single batch entry, never placed in process arguments or
  transient configuration files. Credentials/custom headers are refused before process start.
  Raw child output is bounded and classified, never returned to the UI or ordinary logs.
- Success requires a successful process exit and a regular, nonempty, synced output file. A
  progress event marked finished is insufficient. The service then checks length/trusted SHA-256
  through its completion journal and places the final file without overwriting user files.
- Pause/cancel/shutdown kill and reap the owned child before acknowledgement. Partial resume and
  durable checkpoints are explicitly unsupported; Restart discards only this job's staging data.
  A service SIGKILL/power-loss child-lifetime guarantee has **not** been established. This remains
  an acceptance gap; normal child cleanup must not be described as proof of parent-death cleanup.
- The sidecar has a conservative three-job cap; the native engine retains 1–16 and its shared
  same-host connection budget. Proxy and speed-limit settings remain capability-gated.

## Dependencies and packaging

`pnpm fetch:ytdlp` verifies the upstream **2026.10.26** standalone assets against GitHub API sizes
and SHA-256 digests for Windows x64, macOS arm64 and Linux x64. Downloads are staged before
publication; corrupt cached files are re-downloaded. `DM_YTDLP_PATH` explicitly overrides discovery;
an invalid override fails rather than selecting another executable.

The executable is **not bundled** in desktop installers. `third_party/notices/manifest.json` lists
it as optional external software. A future bundled release must inventory that exact standalone
build and its dependencies. Historical aria2 reports, old checkpoint identities and `.aria2`
filename reservations are retained as historical evidence/data compatibility, not executable support.

## Verification and remaining acceptance

- `crates/backend-ytdlp/src/tests.rs`: input refusal, bounded diagnostics, capability completeness,
  advisory naming and cleanup ownership.
- `crates/backend-ytdlp/tests/fixtures.rs` with the real pinned binary: public HTML/direct WAV
  SHA-256 equality, trusted/untrusted HTTPS, pause and no late progress, truncated output and
  zero-byte restart without Range.
- `crates/service/tests/ytdlp_backend.rs`: Add URL contract, idempotency, completion journal,
  trusted checksum and collision-safe finalization with the actual sidecar.
- `scripts/lib/ytdlp.test.mjs`: platform pins and cache corruption.
- Remaining: real media sites and their JS runtime, packaged desktop interaction, IPC startup/
  shutdown acceptance on a normal OS, Windows/macOS native runs, hard-kill child lifetime,
  extractor time/output limits beyond socket timeouts and advisory disk checks.

This increment does not establish complete E01 acceptance, IDM parity or a release gate.
