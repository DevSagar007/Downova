# ADR 0039 — Windows program folder apart from the data folder

- **Status:** Accepted
- **Date:** 2026-11-16
- **Decided by:** the owner (PROGRESS owner decisions, "NSIS install and data folder", option 2)
- **Amends:** ADR 0031 §4 ("install folder and data folder stay as they are, for now")
- **Requirements:** REL-01, ARCH-01
- **Related:** ADR 0014 (browser host registration), ADR 0031 (installer lifecycle), ADR 0038 (v1.0 owner decisions)

## Context

The per-user NSIS installer (Tauri CLI 2.12.3 template) installs to `$LOCALAPPDATA\Downova`. That is also the data
folder: `downova.sqlite3`, `run\`, `update\` and `native-messaging\`.
- Program files and user data shared one folder.
- The MSI's AppSearch followed the NSIS product key into that same folder (lifecycle rows M1–M4).

Tauri's NSIS options have no install-folder setting. The default is chosen in the template's `.onInit`, and that
function has no hook.

## Decision

1. **Program folder.**
   - Per-user NSIS installs the program in `%LOCALAPPDATA%\Programs\Downova`, the per-user equivalent of Program
     Files.
   - The data stays in `%LOCALAPPDATA%\Downova`, and nothing in the service or the clients changes.
2. **Template.**
   - `apps/desktop/src-tauri/windows/installer.nsi` is the template embedded in the pinned Tauri CLI plus exactly two
     patches. `scripts/nsis-template.mjs` builds it.
   - Patch `program-folder`: the per-user default folder.
   - Patch `leave-the-data-folder`: a previous install location equal to the data folder is not restored.
   - `pnpm verify` (structure) fails when the file differs from the pinned CLI's template plus the patches. A CLI
     upgrade therefore surfaces any upstream change for review.
3. **Moving an older installation** (NSIS_HOOK_PREINSTALL, in every mode, including `/UPDATE` from the in-app
   updater). When the product key still names the data folder:
   - the old service is stopped with `dm shutdown --only-from <old folder>`;
   - the old browser registration is removed (`unregister --host-path`, only entries naming that host);
   - a running old window is closed through the template's Restart Manager check;
   - only the program files are deleted: the main binary, `dm.exe`, `dm-service.exe`, `dm-browser-host.exe`,
     `THIRD-PARTY-NOTICES.txt` and `uninstall.exe`.

   NSIS_HOOK_POSTINSTALL then points existing Start menu and Desktop shortcuts that named the old folder at the new
   one, because an update does not recreate shortcuts. The new install registers the browser host from the new
   folder as before. Nothing in the data folder is touched.
4. **MSI.** No change. Its AppSearch now follows the NSIS product key to the program folder, never to the data
   folder. A clean machine still installs per machine to `C:\Program Files\Downova` (IL6).

## Consequences

- **Uninstall:** it removes the program folder. The data folder is never part of an installation.
- **Updates:** existing internal installs move on their next install or update. Installer-lifecycle row N7 tests an
  old-layout install (made with `/D=`) followed by an upgrade, and N1 checks the folder of a fresh install.
- **Upgrade burden:** a Tauri CLI upgrade needs `node scripts/nsis-template.mjs --write` and a review of the
  upstream diff. If a patch no longer matches exactly once, the script stops.
- **Not covered:** a user who chose a custom folder keeps it, since only the data folder is moved away from. A
  real interactive run of the directory page is a human row (IL1).
