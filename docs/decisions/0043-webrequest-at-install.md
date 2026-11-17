# ADR 0043 — `webRequest` is granted at install (Chrome and Edge builds)

- **Status:** Accepted
- **Date:** 2026-11-17
- **Decided by:** the maintainer, from a reproduced browser behaviour. The owner asked for IDM-like capture that
  actually works (ADR 0040).
- **Amends:** ADR 0019 (capture permissions all optional), ADR 0040 (the all-sites prompt)
- **Requirements:** BRW-04, SEC-04

## Context

Automatic capture accepts a download only when there is request evidence: one GET, no cookies or credentials,
status 200, and allowed redirects. The extension collects that evidence with non-blocking `webRequest` listeners.
Until now `downloads`, `webRequest` and the site access were all optional permissions, granted at runtime from the
user's click.

Real Edge 157 runs on Windows 11 (2026-11-17, isolated profiles, `--load-extension`, the fixture download paced at
about 6 s):
- Downova's extension: the grant succeeded, the extension restarted (PR #25), and permissions and settings were
  kept. But every download stayed in the browser with `no_request_evidence`. A fresh `onBeforeRequest` listener on
  `<all_urls>` saw nothing either, while the server received the request.
- Minimal probe extensions with no Downova code, one isolated profile each. "Live worker" means right after the
  grant; "After reload" means after `chrome.runtime.reload()`.

  | `webRequest` | Site access | Live worker | After reload |
  |---|---|---|---|
  | at install | at install | sees requests | sees requests |
  | at install | runtime grant | nothing | **sees requests** |
  | runtime grant | at install | nothing | nothing |
  | runtime grant | runtime grant (`http(s)://*/*` or `<all_urls>`) | nothing | nothing |

  In every row the profile records the permission as granted, active and not withheld. A runtime-granted
  `webRequest` never delivers events, even after a restart.

## Decision

1. **`webRequest` is in `permissions`** for the Chromium dev and store builds.
   - `downloads`, `cookies` and the site access stay optional, granted from the user's click as before.
   - The restart after a capture grant (`restartForCapture`) stays. A runtime grant of site access takes effect
     only after it.
2. **The extension never requests or removes `webRequest`.**
   - The browser rejects a whole request or removal that names an install-time permission.
   - `REQUESTED_CAPTURE_APIS` (`["downloads"]`) is what capture asks for and gives back.
   - `CAPTURE_APIS` (`downloads`, `webRequest`) is what it checks for.
3. **Checks follow the change:**
   - `manifest.test.ts` and `options.test.ts` (the requested APIs must be optional in the manifest; `webRequest`
     must be in `permissions`);
   - `scripts/package.mjs` (bundled and normal builds);
   - `scripts/store/pack-extensions.mjs` (store build).

   Each check requires `webRequest` up front, and still fails if `downloads`, `cookies` or any site access is
   granted up front.
4. **Firefox is unchanged.** Its build has no capture and no `webRequest`.

## Consequences

- **Privacy is unchanged.** `webRequest` without a host permission observes no request. What the extension sees is
  still decided only by the site access the user grants: one site, or all sites. Listeners stay non-blocking (no
  `webRequestBlocking`). Header names are read; values are never stored.
- **No new install warning.** Chromium shows no warning for `webRequest` alone; the warnings come with site access
  and `downloads`, which are still asked for in context. This was not re-checked against a store review.
- **Chrome:** branded Chrome 157 cannot load an unpacked extension from automation, so the runtime-grant behaviour
  was reproduced on Edge only. Chrome's earlier failure (2026-11-16, `no_request_evidence` after a runtime grant)
  matches it. The change is the same for both.
