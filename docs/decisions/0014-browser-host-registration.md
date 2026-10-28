# ADR 0014 — Per-user browser host registration with ownership checks

- **Status:** Accepted
- **Date:** 2026-10-28
- **Builds on:** B0.6 (`dm-browser-host` and its `register|repair|unregister|status` commands), ADR 0012
- **Requirements:** BRW-07 (registration, repair, uninstall), BRW-01, REL-01 (no orphan registrations)
- **Evidence:** [B2.5 report](../reports/b2.5-browser-registration.md)

## Context

Until B2.4 nothing registered the native-messaging host on install, so a fresh install could not receive links from
Chrome or Edge. B0.6 shipped the commands, but they had three gaps.

1. **Ownership.** `unregister` deleted the registry key whatever it pointed to. `register` overwrote any manifest
   with our file name. One host name (`com.downova.host`) is written by every Downova copy: installed builds,
   development builds and a second installation. The name alone does not say whose an entry is.
2. **No per-user setup step.** Browsers read user-level registrations from the current user's profile (`HKCU` on
   Windows, the browser's user-data directory on macOS and Linux). Root package scripts (`.deb` postinst), a
   per-machine MSI and a drag-installed macOS app cannot write them for the person who will use the browser.
3. **Unstable locations.** An AppImage mount, the installer disk image under `/Volumes/` and a translocated macOS app
   all disappear or move later. A manifest pointing into them breaks silently.

The official locations ([Chrome](https://developer.chrome.com/docs/extensions/develop/concepts/native-messaging),
[Edge](https://learn.microsoft.com/en-us/microsoft-edge/extensions/developer-guide/native-messaging)) are used
unchanged:
- **Windows:** `HKCU\Software\Google\Chrome\…`, `HKCU\Software\Microsoft\Edge\…` (Edge falls back to Chromium's and
  Chrome's keys).
- **macOS:** `~/Library/Application Support/<browser>/NativeMessagingHosts/`, including `Google/ChromeForTesting`
  from Chrome 146.
- **Linux:** `~/.config/<browser>/NativeMessagingHosts/`, with `~/.config` meaning `$XDG_CONFIG_HOME` when set,
  including `google-chrome-for-testing`.
- **`allowed_origins`:** exact origins only; wildcards are not allowed.

## Decision

1. **Ownership is name *and* path.** A manifest belongs to this installation only when it names `com.downova.host`
   and its `path` is this installation's host executable. On Windows the path comparison ignores case, and both
   sides are canonicalized when they exist. A Windows registry value belongs to whoever owns the manifest it points
   to. The host classifies every entry as:
   - `absent`;
   - `ours`;
   - `stale`: a Downova manifest whose executable is gone, or a registry value pointing at a missing file;
   - `other_installation`: a live Downova copy elsewhere;
   - `unreadable`;
   - `foreign`: another host name under our file name.
2. **`register` / `repair`** (the same operation, repeatable):
   - inspects the manifest and, on Windows, the registry value before writing anything;
   - writes over `absent`, `ours` and `stale`;
   - refuses `other_installation` and `unreadable` unless `--replace` is passed, and never replaces `foreign`;
   - writes the manifest atomically (temporary file, then rename), mode `0644` on Unix;
   - on Windows, if setting the registry value fails, removes a manifest that did not exist before, or reports that
     the manifest was written and asks for `repair`.

   Each browser is handled independently. One failure does not stop the others; it gives exit code 3 and a per-browser
   line (or a `--json` row).
3. **`unregister`** removes only `ours` entries: the registry key whose manifest names this host, then that manifest
   (on Windows, also our now-empty per-browser folder). Everything else is kept and reported as `kept: <why>`, which
   is not an error. `--host-path` names the installation; the default is the running executable.
4. **`status --json`** reports the manifest path, its owner, and on Windows the registry key, value and owner.
5. **Refused locations:**
   - relative paths;
   - AppImage mounts (`/.mount_`, `$APPDIR`);
   - `/Volumes/…`, which covers the disk image and also any app run from another volume;
   - `…/AppTranslocation/…`.

   The message tells the user to move the app to Applications (or install the `.deb`) and open it once.
6. **Windows:** Chrome's channels and Chrome for Testing share Google's registry key, so they share one manifest file
   (`%LOCALAPPDATA%\Downova\native-messaging\chrome\`). Chromium and Edge have their own folders.
7. **Setup flows, all per user and in the user's own context:**
   - **NSIS** (`installMode: currentUser`, no elevation): the installer hook runs
     `dm-browser-host register --browser chrome --browser edge` after install, and `unregister … --host-path
     "$INSTDIR\dm-browser-host.exe"` before uninstall. A failing hook does not fail the install.
   - **All packages: the desktop's first launch** runs the sibling host's `register` for Chrome and Edge in the
     background. The outcome goes to `<data>/browser-setup.json`. It runs again only for a new install location or
     version, or after an incomplete attempt, so a user's own `unregister` is respected.
     `DOWNOVA_SKIP_BROWSER_SETUP=1` turns it off, for managed deployments and isolated test runs.

     This is the setup step for the MSI (per-machine files, per-user registration), macOS (the app moved to
     Applications) and Linux (`.deb` / extracted AppImage, as the desktop user). Package scripts that run as root
     write nothing per user.
   - **No uninstall hook:** MSI, macOS drag to Trash, `dpkg -r` and AppImage have none. The documented step is
     `dm-browser-host unregister --browser chrome --browser edge`, run before removing the app. If it is skipped, the
     entries left behind are `stale` once the executable is gone: browsers report the host missing, and the next
     Downova install takes them over without `--replace`. Downloads, the database and preferences are never touched by
     any of these commands.
8. **The host still never starts the service.** The desktop app has to be opened once after install, and after a
   reboot until autostart exists (B3), before a browser hand-off can be accepted. The extension says so when the
   service is not running.

## Consequences

- Registration is idempotent and safe to repeat from the installer, every first launch and by hand.
- Two side-by-side installations do not remove each other's entries. The newer one needs `--replace` to take over a
  live older one, and the older one's later uninstall leaves the newer one's entries in place.
- A user who deletes the app without running `unregister` keeps harmless stale entries, which cannot be cleaned
  automatically because no hook runs.
- Store extension IDs are not known yet. Only the development ID is on the allowlist (`allowed_origins` exactly
  `chrome-extension://bkmpnfclpdmcbdgmeakmnoilmkdmdppf/`); store IDs will be added when they exist (BRW-07 note).
- **Not decided here:**
  - system-wide (`HKLM`, `/etc/opt/chrome`) registration;
  - Firefox (`allowed_extensions`);
  - Snap/Flatpak browsers (BRW-09);
  - Edge Beta/Dev/Canary on macOS and Linux;
  - production signing.
