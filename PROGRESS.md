# PROGRESS — handoff

_Last updated: 2026-10-25 (Windows 11 Pro 10.0.26300 x64, Node 26.7.0, pnpm 12.14.0 via npx, rustc 1.99.1, MSVC Build Tools 17.14)_

## Current milestone
**B0 engine work is complete; next is B0.5 (packaging smoke).**

| Increment | Commit | GitHub CI (4 runners: windows-latest, macos-latest, ubuntu-22.04, ubuntu-24.04) |
|---|---|---|
| B0.1–B0.3 (aria2 adapter, service + IPC, CLI) | … `371c23a` | #2, #3 green |
| B0.4 aria2 HTTP correctness, ADR 0003 | `aee52e3` | #4 failed on macOS + both Ubuntu |
| B0.4.1 engine remediation, ADR 0004 | `c5dc042` | #5 failed on macOS + both Ubuntu |
| Cross-platform CI fixes | `3e37105` | #6 green |
| B0.4.2 `dm-backend-http` single connection | `d0d040b` | #7 green |
| B0.4.2 validated pause/resume | `4daa6ea` | #8 green |
| B0.4.3 validated segmentation | `9047a44` | #9 green |
| Default-backend decision (ADR 0005) | `d1f1b93` | #10 (see B0.5 section) |

CI evidence is the public job conclusion of each run; job logs need GitHub authentication and were not read here.

## Default backend (ADR 0005, this session)
- `dm-service` without `--backend` runs **`dm-backend-http`**; it needs no external program.
- aria2 only with an explicit `--backend aria2` (optionally `--aria2 PATH`), with its ADR 0003 restrictions.
- The backend is fixed per process; no fallback. `--backend aria2` without an aria2c exits 4. Unknown backend, a repeated option, or `--aria2` without `--backend aria2` exit 2 with the usage text.
- The pinned aria2 sidecar is not bundled while REL-05 is open.
- Public IPC contract and UI unchanged (`hello` already reports the backend id and capabilities).

| Capability | `http` (default) | `aria2` (explicit) |
|---|---|---|
| Http | supported | supported (max-tries 1) |
| Https | supported (rustls + platform verifier) | unverified |
| ResumeStrongValidator | supported | unsupported |
| SegmentedHttp | supported (≤ 4 validated segments ≥ 1 MiB) | unsupported |

## Completed (summary)
- Foundation; ADRs 0001–0005; CI on 4 OSes.
- aria2 adapter (hardened, exit monitor), service (user-only IPC, capability gates, service-owned retry with Retry-After/backoff and discard-before-retry), `dm` CLI.
- `dm-backend-http`: HTTP/HTTPS with platform trust, manual redirects with per-hop gate, origin-scoped credentials/validators, identity encoding, exact lengths, backend-owned partials published without overwrite, bounded stop; validated pause/resume (strong ETag + length, partial SHA-256 re-check, exact 206); validated segmentation (≤ 4 segments, per-segment If-Range and 206 checks, 200 → single-connection restart, all workers awaited, per-segment checkpoints).
- Shared fixture crate `dm-test-fixtures` with deterministic state transitions and per-request evidence.
- Reports: [aria2 B0.4](docs/reports/b0.4-http-correctness.md), [dm-backend-http](docs/reports/b0.4.2-http-backend.md).

### This session
- **B0.4.3 validated segmentation** (`9047a44`): G01–G16, service and CLI segmented downloads and pause/resume with SHA-256 matches; `SegmentedHttp` supported on `dm-backend-http`. CI #9 green.
- **ADR 0005:** default backend `http`; `crates/service/src/main.rs` (`BackendChoice` default `Http`, `set_once`, conflict checks, usage text).
- Tests: aria2 process and CLI tests now pass `--backend aria2`; new `default_backend_is_http_and_needs_no_aria2`, `explicit_aria2_without_an_executable_fails_and_never_falls_back`, `backend_flags_are_validated_with_clear_errors`.
- Docs: ADR 0005, ADR 0003/0004 updates, report sections (CI #8, #9, segmentation, default backend), architecture, setup, README, roadmap, traceability (CORE-02 stays in-progress).

## Checks actually run (Windows 11, 2026-10-25, local)
| Command | Result |
|---|---|
| `cargo test -p dm-backend-http` ×5 (B0.4.3) | PASSED: unit 12, `http_backend` 24, `segmented` 16, `validated_resume` 15 (one G15 timing assumption failed once and was fixed; then `segmented` ×6 green) |
| `cargo test -p dm-service --test http_backend_process -- --nocapture` | PASSED 4/4: end-to-end (3 MiB → 3 segments; 503 + Retry-After; cut on 1.5 MiB single path; untrusted TLS; cancel; pause/resume across 4 segments; weak ETag refused), default backend without aria2 (5 MiB → 4 segments, SHA-256 match), explicit aria2 missing → exit 4, six bad argument sets → exit 2 |
| `cargo test -p dm-cli --test real_service -- --ignored --nocapture` | PASSED 3/3 (aria2 explicit, http, segmented pause/resume via `dm`) |
| `pnpm verify:strict` (before `d1f1b93`) | **15 passed, 0 failed, 0 unavailable**: backend-http unit 12 + `http_backend` 24 + `segmented` 16 + `validated_resume` 15; service unit 20 + `http_backend_process` 4; aria2 matrix 38/38 and process 5 (explicit `--backend aria2`); CLI real-service 3/3 |

## Honest limitations
- Jobs, checkpoints, retries and idempotency are **in memory**: a service restart loses them (B1: COR-08, COR-09).
- **No connection budget across jobs** (CORE-02: ≤ 8 per host, 3 active files); each file uses ≤ 4 connections.
- A server that keeps the same strong ETag for different bytes yields a spliced file (V05, G06); only a trusted SHA-256 detects it.
- Resume/segmentation need a strong ETag and a length; otherwise one connection and restart-only.
- Unknown-length truncation is undetectable without a trusted size or hash.
- No proxies; HTTP/1.1 only; public HTTPS sites not covered by automated tests; no speed claims (REL-04).
- The desktop UI is not connected to the service (B2).
- ADR 0005's code change has only Windows evidence until CI runs it.

## Blockers and open decisions
- REL-05: aria2 sidecar not releasable (dated libssh2/expat/c-ares); not bundled.
- REL-03: licence/advisory review of the reqwest/hyper/rustls/ring/rustls-platform-verifier tree, and of aria2 if ever bundled.
- Project licence; `downova.com` ownership for signing identifiers.
- `gh` not installed: CI conclusions via the public API only.

## Exact next task — B0.5 packaging smoke (plan)
1. **Native binaries:** build `dm-service` and `dm-desktop` (and `dm`) in release mode per runner; record sizes and `--version`/`hello` output.
2. **OS/architecture-matched packaging:** Tauri bundles per OS — Windows MSI/NSIS (x86_64), macOS `.app`/`.dmg` (arm64 on macos-latest; x86_64 only if a runner is added), Linux AppImage + `.deb` (x86_64, Ubuntu 22.04 baseline for glibc). Ship `dm-service` (and `dm`) as Tauri `externalBin` sidecars with target-triple names; no aria2.
3. **Packaged service discovery and launch:** the desktop shell locates the sidecar next to its executable, checks `<data>/run/endpoint.json` + `hello`, starts `dm-service` (default backend `http`) if none answers, and never stops it on window close (ARCH-01/02). This needs the first Tauri bridge commands (`service_hello`) — keep UI unchanged otherwise.
4. **Installable workflow artifacts:** a `package` workflow (matrix of the four runners, or the three OS families) uploading the bundles plus SHA-256 sums as artifacts; unsigned at this stage, clearly labelled.
5. **Desktop launch smoke tests:** on each runner, install or unpack the bundle, start the packaged `dm-service` headless and run `dm hello`/`list`/one fixture download with SHA-256; start the desktop app with a timeout and assert the process stays up and the window/service handshake succeeds (Linux under `xvfb-run`).
6. **Manual checks still required:** real install/uninstall on clean machines, SmartScreen/Gatekeeper behaviour of unsigned builds, a public HTTPS download through the packaged app, window close vs. service lifetime, upgrade over an older install, code signing and notarisation (blocked on identity and licence decisions).
