# ADR 0019 — Opt-in automatic browser capture for sites the user chooses

- **Status:** Accepted
- **Date:** 2026-10-31
- **Builds on:** ADR 0014 (browser host registration), the B0.6 native hand-off (BRW-01, COR-09)
- **Requirements:** BRW-03, BRW-04, BRW-06, E02 (and the BRW-10 boundary)
- **Evidence:** [B2.12–B2.14 batch report](../reports/b2.12-b2.14-batch.md)

## Context

Downova can already take an explicit link from the context menu. Automatic capture means taking over
downloads the browser starts by itself. The official Chrome/Edge APIs, checked on 2026-10-31, give these facts:
- **`chrome.downloads`:**
  - `DownloadItem` has the start and final URL, MIME, sizes, file name, `danger`, `incognito` and
    `byExtensionId`;
  - it has **no request method, headers or cookies**;
  - `cancel()` stops a download; `erase()` and `removeFile()` would touch history and files.
- **`chrome.webRequest`:**
  - non-blocking listeners stay available in MV3 (only `webRequestBlocking` is reserved for policy-installed
    extensions);
  - they report `method`, `statusCode`, redirects and request header names (`Cookie` needs `extraHeaders`);
  - they need host permission for the request.
- **Optional permissions:** `downloads` and `webRequest` can be optional, as can host permissions
  (`optional_host_permissions`). `permissions.request` must run inside a user gesture.

A download alone therefore cannot prove that it is a public, replayable GET. Replaying the wrong kind is worse
than not capturing: a POST export, a session download or a single-use link would download something else, or
fail.

## Decision

1. **Opt-in and narrow.**
   - Capture is off by default. Turning it on requests `downloads` and `webRequest`; turning it off gives them
     back.
   - It applies only to **sites the user adds**. Adding a site requests that site's origins (http and https,
     subdomains included).
   - Exclusions win over sites. Host matching respects domain boundaries.
   - There is no broad host permission at install time, no `webRequestBlocking` and no cookie API.
2. **Evidence before capture.**
   - Non-blocking `webRequest` listeners (`main_frame`, `sub_frame`, `other`) record each request's method,
     whether **Cookie** or **Authorization/Proxy-Authorization** headers were sent (names only; values are never
     read or stored), the hosts of every redirect hop and the final status.
   - Records live in `storage.session` (memory-only, survives a worker restart), bounded to 200 records and 2
     minutes.
   - A download is captured only when exactly one completed record matches its final URL.
3. **Eligibility.** The first failing check wins, in this order:
   1. http(s) start and final URL, without credentials;
   2. not incognito;
   3. both hosts on the site list and not excluded;
   4. host permission held;
   5. the browser's `danger` verdict is `safe`, evaluated after the browser chose the file name (when the
      verdict is final; bounded wait 5 s);
   6. not an executable or installer type (always browser-owned, so the browser's own safety checks keep
      applying);
   7. evidence of one **GET**, **no cookies**, **no credentials**, status **200**, every redirect hop allowed;
   8. the MIME, extension and minimum-size filters, where an unknown MIME or size never satisfies a filter
      that needs it.

   Anything else stays in the browser with a localized reason:
   - POST and form exports;
   - `blob:` and `data:` links;
   - signed-in or cookie-carrying downloads;
   - ambiguous or missing evidence;
   - browser warnings;
   - private windows.
4. **Durable state machine** (`storage.local`, at most 50 records for 7 days, at most 3 in flight).
   - **Record and key:**
     - `downloads.onCreated` writes one record per browser download (`cap_<id>_<start>`) with a fresh
       idempotency key, before any native message;
     - duplicate events find the existing record and do nothing;
     - the attempt counter is persisted before each send.
   - **States:** `evaluating`, `browser_owned`, `sending`, `accepted`, `uncertain`, `failed`. What happened to
     the browser's copy is a separate fact: `running`, `cancelled_by_downova`, `finished_in_browser`,
     `cancelled_by_user`, `kept`.
   - **Acceptance** is not "transfer established" or "completed". Those stay the service's job state.
   - **Unconfirmed send:** one more automatic try with the **same key**, only while the browser download still
     runs. Then it stays `uncertain`, and the browser keeps downloading.
   - **Missing host, service down, incompatible or rejected:** `failed`, and the browser keeps downloading.
   - **Worker start:** every interrupted record resumes with its stored key. An acceptance whose takeover did
     not happen is rechecked.
   - **Listeners** are registered synchronously at worker start once the APIs exist, and again when the
     permission is granted.
5. **Takeover only after acceptance.** `downloads.cancel` runs only after a valid acceptance for that record, and
   only if all of these still hold:
   - capture is on and the permissions are held;
   - the browser download still exists and is in progress;
   - the URL is unchanged;
   - the danger verdict is still `safe`.

   Otherwise the browser's copy is left alone and the record says why. Browser history is never erased and
   files are never removed.

   A `cancelRequested` flag is stored just before the cancel. The interruption the browser then reports
   (`USER_CANCELED`) is attributed to Downova, never to the user, even when its event arrives first or the
   worker restarts.
6. **Honesty about copies.** The service creates at most one job per browser download (one key). Two copies
   remain possible:
   - after an uncertain outcome;
   - when the browser finishes first.

   Both are explained in the activity list.
7. **UI.** The options page has:
   - the switch with permission and service status;
   - a supported/browser-only explanation;
   - sites and exclusions;
   - MIME, extension and minimum-size filters;
   - a bounded activity list with reasons (English/Bangla; signed queries shown as `?…`).

   The popup shows a summary and a link to it. The context-menu hand-off is unchanged and works without any
   capture permission.
8. **BRW-10.** `incognito` stays `not_allowed`. Downova's service keeps durable history and idempotency records,
   and Chrome's split mode does not isolate `storage.local`. A private-window opt-in would need an end-to-end
   ephemeral policy (no durable service history for such jobs). That does not exist, so private capture stays
   unsupported.
9. **Real-browser test build.** `--mode e2e` declares the capture permissions as granted for `127.0.0.1` and
   `localhost` only, because automation cannot answer the browser's permission prompt. Production builds keep
   them optional. The prompt flow itself is a human check.

## Consequences

- Capture covers ordinary public downloads from chosen sites. It does not take over the web at large. Many real
  sites send analytics cookies with downloads, and those stay in the browser by design.
- Downova does not run Safe Browsing. Dangerous types and flagged downloads therefore stay in the browser.
- A small download can finish in the browser before acceptance. The record says so, and both copies exist.
- Not covered:
  - Firefox;
  - authenticated downloads (would need scoped cookie transfer, BRW-02);
  - per-site credentials;
  - private windows.
