---
paths:
  - "apps/extension/**"
  - "crates/browser-host/**"
  - "crates/platform/**"
---

# Browser and native messaging integration

Use the `dm-browser` skill. Requirements: BRW-01..11, E02, COR-09. Architecture: `docs/architecture.md` §8.

- MV3 only. Use explicit actions (context menu) first and optional capture through the downloads API. No `webRequestBlocking`. Never rely on the extension service worker staying alive.
- Handoff order: send to host → service accepts (idempotency key) → only then cancel the browser download. If the host is missing, times out or the request is unsupported, leave the browser download running.
- POST, blob:, data:, single-use and browser-generated downloads stay in the browser. Never replay them natively in the MVP.
- The native host validates the sender (`allowed_origins` for Chrome/Edge, `allowed_extensions` for Firefox), framing (with a size cap) and payload against `@dm/contracts`. It forwards to the service and never transfers bytes.
- Cookies only with an explicit permission and a user action, scoped to origin, store and partition. Re-check scope on every redirect, and never forward credentials across origins. Incognito capture is off by default.
- Registration paths differ per browser, OS and channel. Support repair and uninstall that remove only our own entries.
- Request only the extension permissions a shipped feature needs. Keep the privacy text in sync with actual behaviour.
