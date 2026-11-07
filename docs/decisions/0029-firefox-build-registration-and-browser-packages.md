# ADR 0029 — Firefox build, registration and per-package browser support

- **Status:** Accepted for the B3.4 feature branch (2026-11-07), under the narrow development exception the
  maintainer granted for BRW-08, BRW-09 and the BRW-07 and UI work they need. **B2 is not exited**: installed-app
  acceptance, the kit results and BRW-10 remain release gates.
- **Requirements:** BRW-08, BRW-09, BRW-07 (with BRW-01, BRW-03 and BRW-06 unchanged)
- **Builds on:** [ADR 0014](0014-browser-host-registration.md) (registration ownership),
  [ADR 0019](0019-automatic-capture.md) (capture evidence), [ADR 0027](0027-secret-lifecycle-vault-and-authenticated-get.md)
  and [ADR 0028](0028-reconnect-signed-in-downloads.md) (Chrome/Edge sign-in and reconnect)
- **Evidence:** [B3.4 report](../reports/b3.4-firefox-browser-compatibility.md)

## Context

Downova's extension and native host were Chrome-family only:

- the host accepted only a `chrome-extension://` origin as its first argument;
- the host wrote only `allowed_origins` manifests, in Chrome-family locations;
- the extension had one static manifest with an MV3 service worker and a fixed Chromium key.

Mozilla's documentation (checked 2026-11-07) states:

- **Background:** Firefox does not support `background.service_worker` for MV3. It runs `background.scripts` as an
  event page.
- **Add-on ID:** MV3 add-ons need `browser_specific_settings.gecko.id` to be signed. New AMO submissions must declare
  `data_collection_permissions`, which Firefox understands from 140.
- **Host manifest:** a native host's manifest lists `allowed_extensions` (add-on IDs).
- **Per-user host locations:**
  - Windows: `HKCU\Software\Mozilla\NativeMessagingHosts\<name>`;
  - macOS: `~/Library/Application Support/Mozilla/NativeMessagingHosts/`;
  - Linux: `~/.mozilla/native-messaging-hosts/`.
- **Host launch:** Firefox starts the host with the **manifest path, then the add-on ID**.

Nothing in those pages covers Snap or Flatpak. On Linux, the way a browser is packaged decides whether the host can
be reached at all.

## Decision

1. **One manifest builder, two targets.** `apps/extension/manifest.ts` builds both manifests from shared fields.
   The target is chosen explicitly (`vite build --mode firefox|firefox-e2e`; the default and `e2e` are Chromium) and
   compiled in as `__DOWNOVA_TARGET__`, never guessed from which namespace exists. Outputs are separate: `dist/`
   and `dist-firefox/`, so building one never erases the other.
   - **Chromium:** unchanged identity (the development key, so the same extension ID), service worker, and optional
     capture and sign-in permissions.
   - **Firefox:**
     - `background.scripts` (event page, `type: module`);
     - gecko ID `downova-dev@extension.downova.invalid` (development identity; a signed distribution needs an ID the
       maintainer owns, added to both the manifest builder and the host allowlist);
     - `strict_min_version` 140;
     - `data_collection_permissions.required: ["browsingActivity"]`: the link the user hands off leaves the browser
       for the local Downova app, and Mozilla counts data handled outside the add-on as transmitted;
     - only `contextMenus`, `nativeMessaging` and `storage`; no optional or host permissions; no reconnect page.
2. **Firefox offers explicit public hand-offs only (`FEATURES`).**
   - Automatic capture needs request evidence (Chromium's `extraHeaders` view of cookies and credentials) that
     Firefox does not give the same way. It is off, with an explanation, and its listeners are never registered.
   - Signed-in downloads and reconnect are Chrome/Edge flows (BRW-05). Their menus are not created, their messages
     are refused, and the popup says so.
   - Browser-only downloads (anything Firefox starts itself, `blob:`) always stay in Firefox.
   - The menu is recreated at browser start as well as at install, because an event page may unload.
3. **Host launch and sender validation.** `caller(args)` classifies the command line:
   - a Chrome-family origin is checked exactly as before;
   - a Firefox launch needs an absolute path to `com.downova.host.json` **and** exactly one add-on ID on
     `allowed_extensions()`. The path is a shape check only, never an identity;
   - anything else is refused with one framed reply and exit 1, before stdin is read.

   The operation allowlist and framing limits are unchanged.
4. **Registration.**
   - `Browser::Firefox` has its own manifest (`allowed_extensions`, never `allowed_origins`) and Mozilla's per-user
     location. Linux uses `~/.mozilla/native-messaging-hosts`, not moved by `$XDG_CONFIG_HOME`. Windows uses its own
     manifest folder (`native-messaging\firefox`) and the `Mozilla` registry key.
   - Chromium and Firefox entries never share a file or a key.
   - Ownership rules (ADR 0014) apply unchanged: repair our own entries, refuse another live installation (unless
     `--replace` from the command line), keep foreign and unreadable entries, refuse unstable host paths.
   - The NSIS hooks and the desktop's first launch register and unregister Firefox together with Chrome and Edge.
5. **Per-package support (BRW-09).** `detect` checks a fixed list of executable locations per browser family:
   - no directory listing, no profiles; categories only, never paths;
   - on Linux, an ELF check tells a real binary from a launcher script (Ubuntu's `/usr/bin/firefox` Snap shim).

   It reports `native`, `snap`, `flatpak` or `unknown` (or nothing found). `support()` is a static,
   evidence-backed matrix with a reason code:

   | OS | Package | Browser | Verdict | Reason |
   |---|---|---|---|---|
   | Windows | native | Chrome, Edge, Firefox | supported | `tested_this_os` (real-browser runs, B3.4) |
   | macOS, Linux | native | Chrome, Edge | supported | `tested_package_smoke` (B2.33 package smoke on those OSes; an earlier build) |
   | macOS, Linux | native | Firefox, Chromium | untested | `not_tested_on_this_os` |
   | Linux | snap | Firefox | untested | `snap_portal_untested`: may reach the host through the system's native-messaging portal; not tested |
   | Linux | snap | others | unsupported | `snap_sandbox`: the sandbox does not see Downova's registration |
   | Linux | flatpak | all | unsupported | `flatpak_sandbox`: the sandbox cannot start the host Downova registers outside it |
   | any | unknown | any | untested | `package_unknown` |
   | any | none found | any | untested | `not_detected` |

   `dm-browser-host status --json` reports packages and verdicts. Downova never weakens a sandbox or copies its host
   into a browser's sandbox directories.
6. **Desktop.** Help → Browser connections… shows, per browser found:
   - how it is installed;
   - whether its connection points at this installation;
   - the verdict and its reason;
   - Repair.

   The shell runs the packaged host and strips every path before the webview sees anything. Repair never passes
   `--replace`. There is one repair per browser at a time, and an answer from before "Check again" is dropped.

## Consequences

- Firefox users can hand public links to Downova with the same acknowledgement, idempotency, fallback and timeout
  rules as Chrome and Edge. Real-Firefox evidence exists for Windows only.
- The packaged Firefox build is unsigned. It loads only as a temporary add-on (`about:debugging`), until a signing
  identity exists. Firefox Developer Edition, Nightly and ESR can disable signature checks for local testing; release
  Firefox cannot.
- A Snap or Flatpak browser is never reported as supported until its own end-to-end path is tested.
- Firefox capture, sign-in, containers and private browsing remain out of scope. Adding them needs Firefox evidence
  and a new decision.
