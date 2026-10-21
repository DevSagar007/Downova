---
name: dm-browser
description: Use when working on the browser extension (Chrome/Edge/Firefox), download capture, the native messaging host, host registration/installation/repair, authenticated download context, or browser-to-app handoff and fallback (apps/extension, crates/browser-host, browser parts of crates/platform).
---

# dm-browser: capture, native messaging, installation and fallback

## 1. Trigger and scope
This skill covers the MV3 extension, the context-menu and automatic capture, the native messaging host, host manifest and registry registration, the handoff protocol and fallback, and the cookie and authentication context. Service-side job handling is covered by `dm-engine`.

## 2. Consult first
- `docs/architecture.md` §8 (browser path) and §10 (credentials).
- `requirements.md`: BRW-01..11, E02, COR-09, SEC-01, ADV11.
- `docs/testing.md` §4 (handoff matrix).
- `.claude/rules/browser-integration.md`.
- Official docs: Chrome native messaging, downloads and cookies APIs; Firefox native messaging (research S10, S33, S40).

## 3. Inputs and boundaries
- **Inputs:** which download category (research §15.5 matrix) the change supports, the browser(s) and OS(es), and the contract messages used.
- **Boundaries:**
  - the host never transfers bytes;
  - the extension never assumes its service worker stays alive;
  - unsupported categories stay in the browser;
  - credentials are scoped and never logged.

## 4. Workflow
1. Classify the target category as one of: public GET, ordinary download, redirect, authenticated, POST/single-use, blob/data, or sandboxed browser. Implement only supported categories. Everything else falls back.
2. Generate an idempotency key in the extension for each browser download. Treat "accepted" as distinct from "transfer established".
3. Cancel or erase the browser download only after the service accepts. Implement a timeout that leaves the browser download running.
4. In the host, validate the sender, enforce a framing size cap, validate against `@dm/contracts`, then forward to the service through IPC.
5. Registration: per browser (allowed_origins vs allowed_extensions), per OS (registry or manifest path), stable install paths (AppImage), repair, and an uninstall that removes only our entries.
6. Permissions: the minimum set. Cookies are opt-in and scoped. Incognito is off by default.

## 5. Verification
- Unit tests for message validation and fallback decisions.
- End-to-end on each browser and OS: installed, not installed, host missing, service down and timeout. Public GET gets a matching file hash. POST and blob stay in the browser. A retry after a lost acknowledgement gives one job.
- Report each browser × OS combination separately. Never generalise from one.

## 6. Documentation and progress
Update the BRW rows in traceability with the browser and OS evidence, the extension privacy text if data handling changed, and `PROGRESS.md`.

## 7. Completion evidence
The supported-category matrix with pass, fail or not-run for each browser and OS, the permissions requested, and the fallback tests run.
