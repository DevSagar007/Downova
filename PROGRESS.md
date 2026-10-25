# PROGRESS — handoff

_Last updated: 2026-10-25 (Windows 11 Pro 10.0.26300 x64, Node 26.7.0, pnpm 12.14.0 via npx, rustc 1.99.1, MSVC Build Tools 17.14, Tauri CLI 2.12.3)_

## Current milestone
**B0.5 (native packaging + packaged-app smoke): done. Windows x86_64 passed locally (15/15 steps, 3 runs); package-smoke #2 is green on Windows x86_64, macOS arm64 and Linux x86_64 (job conclusions; artifact step details not read). Next: B0.6, the minimal Chrome public-GET handoff spike.**

| Increment | Commit | GitHub CI (4 runners: windows-latest, macos-latest, ubuntu-22.04, ubuntu-24.04) |
|---|---|---|
| B0.1–B0.3 (aria2 adapter, service + IPC, CLI) | … `371c23a` | #2, #3 green |
| B0.4 aria2 HTTP correctness, ADR 0003 | `aee52e3` | #4 failed on macOS + both Ubuntu |
| B0.4.1 engine remediation, ADR 0004 | `c5dc042` | #5 failed on macOS + both Ubuntu |
| Cross-platform CI fixes | `3e37105` | #6 green |
| B0.4.2 `dm-backend-http` single connection | `d0d040b` | #7 green |
| B0.4.2 validated pause/resume | `4daa6ea` | #8 green |
| B0.4.3 validated segmentation | `9047a44` | #9 green |
| Default backend `http` (ADR 0005) | `d1f1b93` | #10 green |
| B0.5 packaging + smoke | `d241a18`, `8955629` | CI #12 green; **package-smoke #2 green on windows-x86_64, macos-aarch64, linux-x86_64** (#11 / package-smoke #1 for `d241a18` were cancelled by the next push) |

CI evidence is the public job conclusion of each run; job logs need GitHub authentication and were not read here.

## Backends (ADR 0005)
`dm-service` defaults to the built-in `dm-backend-http` (Http, Https, ResumeStrongValidator, SegmentedHttp supported); aria2 only with `--backend aria2` (Http only) and never bundled while REL-05 is open. No fallback between backends.

## Completed (summary)
- Foundation; ADRs 0001–0005; CI on 4 OSes; aria2 adapter (hardened); service (user-only IPC, capability gates, service-owned retry); `dm` CLI.
- `dm-backend-http`: HTTP/HTTPS with platform trust, manual redirects, origin-scoped credentials, identity encoding, validated pause/resume, validated segmentation.
- Reports: [aria2 B0.4](docs/reports/b0.4-http-correctness.md), [dm-backend-http](docs/reports/b0.4.2-http-backend.md), [B0.5 packaging](docs/reports/b0.5-packaging-smoke.md).

### This session — B0.5
- **Packaging:** `scripts/package.mjs` (release `dm-service` + `dm` staged as Tauri `externalBin` with the host triple; `tauri build` with `src-tauri/tauri.package.conf.json`; Windows NSIS+MSI, macOS app+dmg ad-hoc signed, Linux deb+AppImage; `dist-packages/<os>-<arch>[-smoke-test]/` with SHA-256 sums and build info; no aria2).
- **Service discovery:** desktop command `service_connect` (`apps/desktop/src-tauri/src/service.rs`): reuse a compatible service via endpoint + `hello`; refuse to replace an incompatible one; otherwise start the packaged `dm-service` detached and wait ≤ 20 s; races settled by the single-instance lock.
- **UI:** `TauriServiceClient`; the status bar shows the real connection; download commands stay unavailable with a new honest reason (`unavailable.notWired`, en + bn); new vitest test.
- **CLI:** `dm hello`.
- **Test hook:** desktop feature `smoke` (test builds only) writes the window + handshake report to `$DOWNOVA_SMOKE_FILE`.
- **Smoke:** `scripts/smoke/packaged-smoke.mjs` installs/unpacks the real package and uses only its binaries; screenshots are window-only on developer machines (a first full-screen capture showed unrelated private windows and was deleted).
- **Workflow:** `.github/workflows/package-smoke.yml` (Windows x86_64, macOS arm64, Linux x86_64 on Ubuntu 22.04 with `xvfb-run`), internal artifacts `downova-<label>-smoke-test-<sha>`, no releases.

## Checks actually run (Windows 11, 2026-10-25, local)
| Command | Result |
|---|---|
| `pnpm verify:strict` (before `d1f1b93`, ADR 0005) | **15 passed, 0 failed, 0 unavailable** |
| `node scripts/package.mjs --smoke` | PASSED: `Downova_0.1.0_x64-setup.exe` (3.59 MiB), `Downova_0.1.0_x64_en-US.msi` (5.59 MiB), SHA-256 sums written |
| `node scripts/smoke/packaged-smoke.mjs --package-dir dist-packages/windows-x86_64-smoke-test` ×3 | PASSED 15/15 steps each run after fixing the script's main-binary detection (first run failed on that): NSIS per-user install; MSI extract contains `dm-service.exe`; no aria2; packaged service `hello` → `http`; `dm list`; 3 MiB download SHA-256 match; desktop window "Downova" visible; desktop started the packaged service; window closed (WM_CLOSE) at ~2.6 MiB of 6 MiB → service alive → download completed with SHA-256 match; uninstall removed the binaries |
| UI checks (`pnpm verify -- --only=format,lint,typecheck,test,build`) | PASSED |
| `cargo test -p dm-cli` | PASSED (unit 7, `cli` 13 incl. `hello_shows_the_running_service_and_its_backend`) |
| `cargo clippy -p dm-desktop --all-targets [--features smoke] -- -D warnings` | PASSED both |
| `pnpm verify:strict` (B0.5 working tree) | **15 passed, 0 failed, 0 unavailable**: incl. vitest (new connected-state test), `dm-cli` 13, backend-http 12 + 24 + 16 + 15, service 20 + `http_backend_process` 4, aria2 matrix 38/38, CLI real-service 3/3 |

## Honest limitations
- macOS arm64 and Linux x86_64: package-smoke #2 green, but the per-step `results.json` in the artifacts (graceful close vs. forced, screenshots, deb install vs. unpack) was not read (needs GitHub auth). Intel macOS: unverified (no runner).
- Packages are unsigned (macOS ad-hoc), not notarised; the smoke packages contain the test-only hook.
- No clean-machine install, upgrade, or orphan/registration audit (REL-01); no public HTTPS download through the packaged app.
- No service autostart at login and no "Exit and stop downloads" yet (ARCH-01). The UI issues no download commands (B2).
- Jobs and checkpoints are in memory (B1). No connection budget across jobs (CORE-02).

## Blockers and open decisions
- Code signing and notarisation identities; project licence (REL-03); dependency licence/advisory review (REL-03); aria2 not releasable (REL-05).
- `gh` not installed: CI conclusions via the public API only.

## Exact next task
1. Download the package-smoke #2 artifacts (GitHub UI) and read each `results.json`/`summary.md`; record any `note`/`unavailable` steps (macOS graceful close, Linux deb install, screenshots) in the B0.5 report.
2. **B0.6 — minimal Chrome public-GET handoff spike** (skill `dm-browser`): an MV3 extension context-menu "Download with Downova" for a public `http(s)` URL → native messaging host → `dm-service` `downloads.v1.add`; no cookies/credentials (BRW-05 stays blocked); host registration per user; a fixture-based end-to-end test.
