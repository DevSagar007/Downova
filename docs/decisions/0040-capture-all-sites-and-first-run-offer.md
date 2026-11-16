# ADR 0040 — Optional capture from all sites, offered once at first install

- **Status:** Accepted
- **Date:** 2026-11-16
- **Decided by:** the owner ("IDM-like: the extension stays in the browser and takes downloads easily"; v1.0 scope
  extended for this item on 2026-11-16)
- **Amends:** ADR 0019 (automatic capture: "only sites the user adds")
- **Requirements:** BRW-04, BRW-06, BRW-10, SEC-04

## Context

**ADR 0019:**
- automatic capture is off by default;
- it works only for sites the user adds one by one, each with its own host permission;
- every check in the policy still applies to them: plain public GET only, no cookies, exclusions, filters and the
  browser's own danger verdict.

**The gap:** users coming from other download managers expect one switch that sends every eligible download to the
app.

## Decision

1. **All sites, as a setting.**
   - Turning on "Capture from all sites" in the extension's options asks the browser for the host permission on all
     sites (`http://*/*`, `https://*/*`, already in `optional_host_permissions`) inside that click.
   - Every policy check stays: exclusions still win, and the filters, the danger verdict, the request evidence
     (one GET, no cookies or credentials, status 200, redirects) and the always-browser types are unchanged.
   - Only the site allowlist step accepts any host.
   - Turning it off removes that permission. Sites added one by one are kept.
2. **One offer at first install** (Chrome and Edge builds only, since the Firefox build has no capture).
   - When the extension is installed (not on update), the options page opens once with a short question: send
     downloads to Downova from all sites, or choose sites later.
   - "Yes" asks for the capture permissions and all-sites access in that click, and turns capture on only if the
     browser grants both.
   - "Later" changes nothing.
   - Nothing is enabled without the click. The offer never reappears by itself.
3. **Defaults stay off.** A fresh install captures nothing until the user answers "Yes". This keeps BRW-04's "off by
   default" acceptance.

## Consequences

- **Store review:** listings must justify the optional all-sites access. It is requested only from the user's click.
  `docs/store/README.md` gets that justification.
- **Unchanged:** the `incognito: "not_allowed"` manifest key and the incognito check in the policy (BRW-10).
- **Not built:** pre-registering the extension from the desktop installer (the browser's "external extensions" key).
  It needs the store item IDs, which exist only after the first upload.
