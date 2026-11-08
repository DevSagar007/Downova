# ADR 0031 — Installer lifecycle ownership, one build version, and bundled third-party notices

- **Status:** Accepted for the B3.6 feature branch (2026-11-08), under the narrow exception the maintainer granted
  for REL-01, REL-02, REL-03, UI-01 and the remaining B3.5 validation. **B2 is not exited.** No release, no
  production key, no publishing.
- **Requirements:** REL-01 (install, update, repair, uninstall), REL-03 (notices), ADV09 (version binding), ARCH-01
- **Builds on:** [ADR 0014](0014-browser-host-registration.md) (registration ownership),
  [ADR 0030](0030-verified-updates-quiescence-backup-recovery.md) (updates)
- **Evidence:** [B3.6 report](../reports/b3.6-beta-candidate-preparation.md)

## Context

Reading the NSIS template inside the pinned Tauri CLI 2.12.3 showed three things:

- **Install folder:** a per-user NSIS installation goes to `$LOCALAPPDATA\Downova`. That is the folder where
  Downova keeps its data (`downova.sqlite3`, `run/`, `update/`, `native-messaging/`).
- **Uninstall:**
  - the uninstaller deletes only the files it installed, then runs a non-recursive `RMDir "$INSTDIR"`, so the data
    survives;
  - the optional "delete app data" box removes `%APPDATA%` and `%LOCALAPPDATA%\com.downova.desktop` (the WebView
    profile), not Downova's data.
- **Running programs:** before replacing or deleting files, the installer and uninstaller check only the main
  binary (`dm-desktop.exe`). The download service deliberately outlives the window (ARCH-01), so a running
  `dm-service.exe` keeps its own file locked. An uninstall or upgrade would then leave that file and the process
  behind.

Separately:

- **Versions:** the Rust binaries took their version from each crate's `CARGO_PKG_VERSION`, while the desktop,
  installer and updater took theirs from `tauri.conf.json`. A build for another version could therefore ship a
  service that still reported the old version.
- **About box:** it showed a hard-coded "0.1.0".
- **Notices:** packages carried no third-party notices.

## Decision

1. **One build version.**
   - Every Rust component reports `dm_domain::BUILD_VERSION`. It is the workspace version unless
     `DOWNOVA_BUILD_VERSION` is set at compile time; `scripts/package.mjs --app-version` sets it for **fixture
     builds only**.
   - The desktop's About box shows the build version, injected by Vite.
   - After every build, `package.mjs` runs each staged binary with `--version`, and on Windows each binary inside
     the MSI too. It checks the installer file names, and refuses the package if any of them disagree.
2. **No silent attachment to another build.**
   - The desktop connects only to a service whose `hello.service_version` equals its own build version.
   - Any other service, even one with the same API version (for example one left running across an update), is
     reported as "a different version of the download service is running" and is never used or replaced.
3. **The installer stops only its own service.**
   - The service records its executable in its private `run/endpoint.json` (optional field; older readers ignore
     it).
   - `dm shutdown --only-from DIR` stops the service, keeping its state, only when that executable is inside
     `DIR`.
   - The NSIS pre-install (upgrade) and pre-uninstall hooks call it with `$INSTDIR`.
   - A service of another installation is left running, and so is a pre-B3.6 service that records no executable.
     So is any other process named `dm-service.exe`. Nothing is terminated by file name.
4. **Install folder and data folder stay as they are, for now.**
   - Program files and data share `%LOCALAPPDATA%\Downova` for per-user NSIS installs. The uninstaller's
     non-recursive `RMDir` and the hooks above keep the data.
   - Moving either one would need a migration for existing internal testers. A custom NSIS template (for example
     `%LOCALAPPDATA%\Programs\Downova`) is a decision for the maintainer before a public beta. It is recorded as
     open in the B3.6 report.
5. **A manual pause survives a restart even without saved progress (amends [ADR 0012](0012-exit-and-stop.md) §5).**
   - Found by the update journey: a download paused by the user before any checkpoint was confirmed came back
     after a clean service stop as failed, "interrupted … add the link again".
   - Now it is restored paused, with 0 bytes and its unconfirmed attempt dropped; resuming it starts afresh.
   - No data is invented. A download that was *transferring* without a checkpoint still fails as interrupted.
6. **Notices.**
   - `scripts/notices/generate-notices.mjs` builds a reproducible inventory:
     - Rust crates reachable through normal dependency edges from the shipped binaries, per supported target
       (procedural macros and build dependencies are build-only);
     - the desktop's production npm closure;
     - `third_party/notices/manifest.json` for the Rust standard library and the installer runtimes.
   - License texts come only from the published packages. Missing metadata stays `UNRESOLVED`.
   - The inventory is checked by `pnpm verify`, shown offline in Help → Third-party notices…, and bundled as
     `THIRD-PARTY-NOTICES.txt`: as a Tauri resource installed beside the app, and next to the packages.
   - aria2 stays unbundled (ADR 0005).
   - The project license remains undecided (ADR 0001); no license field or `LICENSE` file is added.

## Consequences

- An uninstall or upgrade no longer leaves this installation's service running with its files locked. Another
  installation keeps working.
- A version mismatch between components fails the package build instead of reaching a tester.
- REL-03 is not complete:
  - 24 distributed components have unresolved metadata;
  - the Rust standard library's full notice file is not bundled;
  - the project license is open.
- The native NSIS/MSI install, upgrade and uninstall journeys still need an isolated machine or account. On this
  developer machine they are NOT RUN (see the report).
