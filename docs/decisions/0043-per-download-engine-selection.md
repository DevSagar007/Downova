# ADR 0043 — Per-download engine selection: native files and yt-dlp media in one service

- **Status:** Accepted
- **Date:** 2026-11-17
- **Decided by:** the owner ("one Add URL; ordinary files and media run at the same time; changing the engine must not
  need a service restart; choosing the engine is the service's job")
- **Requirements:** E01, ARCH-04, CORE-01, BRW-04
- **Amends:** ADR 0042 (the whole-process `--backend yt-dlp` switch), ADR 0005 (one backend per process)

## Context

ADR 0042 added yt-dlp as a backend chosen for the whole life of the process. To download media, the user stopped the
service and started it again in media mode; ordinary files then could not run. The owner wants one download manager:
ordinary files and media side by side, one Add URL, and the service choosing the engine.

## Decision

1. **Two engines, one service.** `dm-service` always runs the native Rust engine. When a yt-dlp executable is found
   at startup, it also runs the media engine. `EngineRouter` (in `dm-service`) implements `DownloadBackend` over both
   and sends each call to the engine of its job. Engine handles carry a private prefix and never leave the router.
2. **The engine is part of the job.** `Engine` is `file` (native) or `media` (yt-dlp). It is chosen when the job is
   added, stored with the job (`jobs.engine`, migration 0015, `file` for every older job) and never changes, so a
   restart, retry or resume always uses the same engine.
3. **Contract.** `downloads.v1.add` takes an optional `engine` (`file` or `media`); omitted means `file`, so older
   clients and their idempotency fingerprints are unchanged. Every snapshot reports its `engine`.
4. **Capabilities per engine.** The service checks a job's features against its own engine's capability report. A
   `media` add when no media engine is installed is refused with `BACKEND_CAPABILITY_UNAVAILABLE` and creates no
   job. There is no fallback from one engine to the other in either direction.
5. **Admission.** The configured limit (1–16) counts all active jobs. Media jobs also have their own cap of three,
   because yt-dlp does not share the native per-host budget. A waiting media job never blocks an ordinary file.
6. **Startup.** `--yt-dlp PATH` names the media executable; an invalid path fails startup. Without it, discovery uses
   `DM_YTDLP_PATH`, then `tools/ytdlp` and `PATH`; if nothing is found, the service runs with files only and reports
   that media is unavailable. `--backend http` stays accepted; `--backend yt-dlp` is refused with a usage message,
   because media is now chosen per download.
7. **Clients.** The desktop Add dialog gets a "Video or audio from a web page" choice; the CLI gets `dm add --media`.

## Not in this decision

Automatic detection (`auto`: probe the link and offer media formats), the format and quality picker, FFmpeg merge and
subtitles. They build on the stored engine and come in the next E01 increments.

## Consequences

- A media download no longer stops ordinary downloads, and no restart is needed to start one.
- Durable checkpoints stay native-only; a media job's partial output still needs Restart (ADR 0042).
- `hello` reports the native engine as the service's backend and adds whether media is available.
