# PROGRESS — handoff

_Last updated: 2026-10-25 (Windows 11 Pro 10.0.26300 x64, Node 26.7.0, pnpm 12.14.0 via npx, rustc 1.99.1, Google Chrome 155.0.8059.85, Microsoft Edge 156.0.4314.42)_

## Current milestone
**B0.6 (minimal browser public-GET hand-off spike): implemented. Real Chrome 155 end-to-end passed on Windows 11 (3 runs). Edge, macOS and Linux have not been run in a real browser. CI #15 for `330d306` failed on ubuntu-22.04 (log unreadable; probable test race fixed here, unconfirmed). This session's later changes are in `564ad14` (committed by the user); only this PROGRESS update is uncommitted.**

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
| B0.6 host crate + contract (committed by the user mid-increment) | `56ee713`, `f5bc7b8` | CI #14 green on all 4; package-smoke #4 green |
| B0.6 extension, packaging, verify wiring | `330d306` | **CI #15: windows, macOS, ubuntu-24.04 green; ubuntu-22.04 failed** (log needs sign-in); package-smoke #5 green on all 3 |
| B0.5 packaging + smoke | `d241a18`, `8955629` | CI #12 green; **package-smoke #2 green on windows-x86_64, macos-aarch64, linux-x86_64** (#11 / package-smoke #1 for `d241a18` were cancelled by the next push) |

CI evidence is the public job conclusion of each run; job logs need GitHub authentication and were not read here.

## Backends (ADR 0005)
`dm-service` defaults to the built-in `dm-backend-http` (Http, Https, ResumeStrongValidator, SegmentedHttp supported); aria2 only with `--backend aria2` (Http only) and never bundled while REL-05 is open. No fallback between backends.

## Completed (summary)
- Foundation; ADRs 0001–0005; CI on 4 OSes; aria2 adapter (hardened); service (user-only IPC, capability gates, service-owned retry); `dm` CLI.
- `dm-backend-http`: HTTP/HTTPS with platform trust, manual redirects, origin-scoped credentials, identity encoding, validated pause/resume, validated segmentation.
- Reports: [aria2 B0.4](docs/reports/b0.4-http-correctness.md), [dm-backend-http](docs/reports/b0.4.2-http-backend.md), [B0.5 packaging](docs/reports/b0.5-packaging-smoke.md).

### This session — B0.6 ([report](docs/reports/b0.6-browser-handoff.md))
- **Contract:** `NativeHostRequest` (`hello`, `add`), `NativeHostOutcome` and `NativeHostResponse`. TypeScript is generated and the Rust mirror is in `dm-domain::api`.
- **`dm-browser-host`:**
  - framing caps;
  - an exact origin allowlist;
  - only `hello` and `add` → `downloads.v1.add`;
  - it **never starts the service** ("open the Downova app, then try again");
  - per-user `register` / `repair` / `unregister` / `status`.
- **`apps/extension` (MV3):**
  - context menu "Download with Downova" (http/https links only);
  - permissions `contextMenus`, `nativeMessaging`, `storage`; incognito `not_allowed`;
  - one key per action, persisted before sending; same key and payload on retry, including after a worker or browser restart;
  - response checks (version, `request_id`, outcome);
  - popup: a service readiness line ("Service ready / unavailable / status unknown"), the hand-off list with Retry, Dismiss and Clear list, links shown without the query string, at most 20 records, 7-day retention.
- **Identity:** the manifest public key hashes to `bkmpnfclpdmcbdgmeakmnoilmkdmdppf`. This is checked by vitest and by the ID real Chrome reports; it is the host's only allowed origin.
- **Packaging:** `dm-browser-host` is built and staged as a Tauri `externalBin`. Smoke steps: A4 (packaged host → service, same key → same job) and A5 (foreign origin refused). Installer registration is deferred (REL-01).
- **Real browser:** `scripts/e2e/chrome-handoff.mjs`:
  - isolated profile; DevTools pipe with `Extensions.loadUnpacked`;
  - reversible registration (Windows HKCU key, restoring any previous value; macOS/Linux use the profile's `NativeMessagingHosts`);
  - Chrome or Edge;
  - added to CI on windows-latest, macos-latest and ubuntu-24.04, with results uploaded as an artifact.
- **Verify:** the native-host end-to-end step is bounded at 15 min (`timeoutMs`). Its test now waits (≤ 30 s) for the service before running the host, the probable cause of the ubuntu-22.04 failure.
- **Docs:**
  - report `docs/reports/b0.6-browser-handoff.md` (renamed from `b0.6-chrome-handoff.md`);
  - setup (per-OS development steps);
  - architecture §8, testing §4, roadmap;
  - requirements: BRW-01 `in-progress`, BRW-02 `implemented`, BRW-03 `in-progress`;
  - traceability.

### Earlier — B0.5
- Native packages, packaged service discovery, packaged-app smoke and workflow ([report](docs/reports/b0.5-packaging-smoke.md)).

## Checks actually run (Windows 11, 2026-10-25, local)
| Kind | Command | Result |
|---|---|---|
| Mocked extension | `apps/extension`: `tsc`, `vitest run`, `vite build` | PASSED (3 files, 26 tests) |
| Direct host process | `cargo test -p dm-browser-host -- --include-ignored` | PASSED (unit 13; host 7 including the end-to-end test: host → real `dm-service` → SHA-256, same key → same job, conflict, service outlives the host) |
| Packaged host | `node scripts/package.mjs --smoke`, then `packaged-smoke.mjs` | PASSED 17/17 (A4, A5 new) |
| **Real Chrome** | `node scripts/e2e/chrome-handoff.mjs` (Chrome 155.0.8059.85, headless=new) | **PASSED, 3 consecutive runs:** ID matches; popup "Service ready"; menu listener → native messaging → host → service → 3 MiB SHA-256; lost ack → uncertain → browser restart → same-key retry → one job; browser closed at 1 MiB of 6 MiB → completed, SHA-256; registration removed, other hosts untouched |
| Real Edge | `node scripts/e2e/chrome-handoff.mjs --kind edge` (Edge 156.0.4314.42) | **NOT RUN:** Edge exits immediately when started for automation (pipe or port), with an update pending; not investigated |
| All | `pnpm verify:strict` (before this session's last changes, tree of `330d306`) | PASSED 19/19, 0 unavailable |
| All | `pnpm verify:strict` (final working tree) | **PASSED 19/19, 0 unavailable** (incl. extension tsc/vitest/build, native-host end-to-end 22.5 s) |
## Honest limitations
- B0.6:
  - a real browser has run only for Chrome on Windows;
  - the native context-menu click is simulated: the test calls the registered listener; no manual click has been made;
  - idempotency is in memory (B1), so a lost acknowledgement followed by a service restart can duplicate a job;
  - the host needs a running service (no login autostart, ARCH-01);
  - only the development extension ID is allowed;
  - the installer does not register the host (REL-01);
  - the extension UI is English only.
- CI #15 ubuntu-22.04 failure: log not read; the fix is unconfirmed.
- macOS arm64 and Linux x86_64 package-smoke: green job conclusions only; per-step results not read. Packages are unsigned (macOS ad-hoc) and not notarised.
- Jobs and checkpoints are in memory (B1). No connection budget across jobs (CORE-02).
## Blockers and open decisions
- Code signing and notarisation identities; project licence (REL-03); dependency licence/advisory review (REL-03); aria2 not releasable (REL-05).
- `gh` not installed: CI conclusions via the public API only.

## Exact next task
1. After the user commits and pushes:
   - check that CI ubuntu-22.04 is green; if not, read the job log (needs GitHub sign-in);
   - read the new "Real-browser hand-off" CI step on windows-latest, macos-latest and ubuntu-24.04 (artifact `browser-handoff-*`), and record macOS/Linux real-Chrome results in the B0.6 report.
2. Manual check by a person: load `apps/extension/dist` and click the menu by hand (docs/setup.md, "Trying the browser hand-off").
3. **B1.1, the smallest storage/recovery increment:**
   - create the `dm-storage` crate (SQLite, migrations, single writer in `dm-service`);
   - persist jobs and idempotency keys (key → job, payload fingerprint) when `downloads.v1.add` is accepted;
   - on restart, `downloads.v1.add` with a known key returns the stored job and a different payload still conflicts;
   - test: kill `dm-service` after acceptance, restart it, re-send the same key → same `job_id`, one row (COR-09, ARCH-05, CORE-04).
