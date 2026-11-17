# ADR 0041 — IDM-like browser integration without the Chrome Web Store

- **Status:** Accepted
- **Date:** 2026-11-17
- **Decided by:** the owner ("no Chrome Web Store upload; installing Downova should offer to add the extension, like
  IDM")
- **Requirements:** BRW-07, BRW-04, SEC-03, SEC-05
- **Related:** ADR 0014 (host registration), ADR 0029 (Firefox), ADR 0040 (all sites, first-run offer), docs/store

## Context

IDM adds its extension because that extension is in the Chrome Web Store. Its installer only points Chrome at the
store item. Downova will not use the Chrome Web Store. What each browser allows outside a store (2026):

| Browser | Automatic, outside its store | What remains |
|---|---|---|
| Chrome (Windows, macOS) | Not possible. Off-store extensions are blocked, and enterprise policy works only on managed machines | "Load unpacked" in developer mode, by the user |
| Edge | Possible via the "external extensions" registry key, but only for an Edge Add-ons item (free) | Until then, "Load unpacked" as for Chrome |
| Firefox | A Mozilla-signed add-on (free, unlisted on AMO), which the installer can register for a prompt at the next start | Until then, nothing permanent |

Checked on this PC (Chrome 157, Edge 157): both browsers ignore their own `chrome://extensions` and
`edge://extensions` pages when given on the command line. They open a new tab instead.

## Decision

1. **The extension ships inside Downova.**
   - Packages carry the normal build in `<resources>/extension` (`scripts/package.mjs`, `tauri.package.conf.json`),
     checked for no e2e hook and optional capture permissions.
2. **Guided "Add to Chrome / Edge"** under Help → Browser connections, in English and Bangla:
   - the steps;
   - **Open the browser:** started at a fixed absolute location with no argument (SEC-03 allowlist,
     `browser_add.rs`);
   - **Copy the extensions page address:** Rust writes only `chrome://extensions` or `edge://extensions` to the
     clipboard;
   - **Show the extension folder:** through the single opener adapter (`folders.rs`), a checked directory, never
     run.
3. **One offer at first start.** An installed Downova that ships the extension, on a computer with Chrome or Edge,
   shows once "Use Downova from your browser" with "Show me how". After installing the extension, its own first-run
   offer (ADR 0040) asks about capture from all sites.
4. **Automatic for Edge and Firefox, later:**
   - once the owner has an Edge Add-ons item ID and a Mozilla-signed add-on, the installer registers them per user
     (Edge external-extension key with the Edge store update URL; Firefox's per-user extensions key pointing at the
     signed `.xpi`);
   - the browser then asks the user to turn Downova on;
   - uninstall removes only those entries.

   This is a follow-up ADR, because it needs those IDs.

## Consequences

- **Chrome:** the user does "Load unpacked" once, with the guide. Chrome may show its developer-mode reminder. That
  is Chrome's behaviour and cannot be avoided off-store.
- **Updates:** the unpacked extension lives in the installation folder. An update replaces it in place, and Chrome
  reloads it at its next start. Uninstalling removes it, and Chrome then reports it missing.
- **Security:** two new user-triggered desktop actions (start a browser with no argument; open one fixed folder),
  each guarded by the SEC-03 test (allowlist entry; `open_path` only after a directory check, only from its command).
