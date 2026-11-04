# Proposal: BRW-10 private-window capture scope for B2

- **Status:** **Proposed: awaiting the maintainer's choice.** Nothing here changes `requirements.md` or ADR 0019 until
  an option is chosen. BRW-10 stays `in-progress`.
- **Date:** 2026-11-04
- **Requirement:** BRW-10 (P0 · B2), "Incognito capture disabled by default"
- **Related:** [ADR 0019](../0019-automatic-capture.md) §8, [ADR 0018](../0018-history-retention.md) (history),
  COR-09 (idempotency)

## What the requirement says and what exists

**Current acceptance:** "Capture is off in incognito by default. Enabling it shows the ephemeral session and history
policy."

| Clause | State | Evidence |
|---|---|---|
| Off in incognito by default | **Met** | `apps/extension/public/manifest.json` `"incognito": "not_allowed"`; `policy.ts` refuses `item.incognito` (reason `incognito`); unit tests in `capture.test.ts` |
| Enabling it shows the ephemeral session and history policy | **No counterpart**: enabling is not offered | ADR 0019 §8: no end-to-end ephemeral policy exists, so private capture stays unsupported |

## Why "enabling" is not a UI change

A private-window capture would leave durable traces in four places today:

| Where | What is kept | Lifetime |
|---|---|---|
| **Extension storage:** `chrome.storage.local` | hand-off records (URL redacted for display, idempotency key, state) and capture records | hand-offs: up to 20 records, settled ones dropped after 7 days; captures: settled ones dropped after 7 days (`CAPTURE_RETENTION_MS`). The incognito mode setting does not isolate `storage.local` (ADR 0019 §8), so private records would sit beside normal ones |
| **Service history:** SQLite, the only writer is `dm-service` | the job, its source URL, its file name and its state | until removed by the user or by history retention (ADR 0018: off by default, or 1–3650 days) |
| **Idempotency records** (COR-09) | key and request fingerprint, written with the job before the acknowledgement | **30 days** (`IDEMPOTENCY_RETENTION_MS`), pruned by the service |
| **Restart behaviour** | unfinished jobs are recovered from checkpoints after a crash, logout or restart; a manual pause persists (ADR 0012) | until the job finishes or is removed |

A private session expects none of these to outlive the window. Making that true is service, storage and recovery
work, not a toggle.

## Option A: keep private capture unsupported in B2 (recommended for the B2 exit)

- **Effect:** B2 exits on the shipped behaviour. Private downloads always stay in the browser, and the user can still
  copy a link and add it in Downova deliberately, which then becomes ordinary, durable history.
- **Proposed requirement text** (`requirements.md`, BRW-10):

  ```diff
  -- **Acceptance:** Capture is off in incognito by default. Enabling it shows the ephemeral session and history policy.
  +- **Acceptance:** Capture never acts on private (incognito/InPrivate) windows: the extension declares
  +  `"incognito": "not_allowed"` and ignores private download items, and the options page says so. Offering private
  +  capture requires a separate design of an end-to-end ephemeral policy (extension storage, service history,
  +  idempotency records, recovery) and is not part of B2.
  ```

- **Proposed ADR 0019 §8 addition:** "B2 scope (chosen <date>): private capture is not offered; BRW-10 was amended
  accordingly. A future opt-in needs its own ADR covering the four stores listed in the proposal."
- **Remaining work for A:**
  - one sentence in the extension options page (English and Bangla) saying private windows are never captured;
  - a test for that text.
  - Then BRW-10 can become `implemented`, and `verified` once the real-browser smoke asserts that a private download
    stays in the browser.
- **Risk:** low. It records the shipped behaviour honestly; nothing is removed from the code.

## Option B: keep the enabling criterion (build an ephemeral private mode)

- **Effect:** B2 cannot exit until private capture exists end to end.
- **Required design and work:**
  1. **ADR:** the private-job model. A job created from a private window is flagged `ephemeral` at add time by the
     extension and checked by the host.
  2. **Extension:**
     - `"incognito": "split"` and a separate in-memory record list for private windows (never `storage.local`);
     - the permission and opt-in flow;
     - popup and options text in English and Bangla stating the policy.
  3. **Contract and storage:**
     - an `ephemeral` flag on `downloads.v1.add` (schema, TypeScript, `api.rs`);
     - a migration for the job column;
     - idempotency records for ephemeral jobs held in memory only (or deleted at completion);
     - no history row after completion or removal.
  4. **Service:**
     - ephemeral jobs are not recovered after a restart; their partial data and checkpoints are deleted at startup;
     - notifications without a file name or URL;
     - excluded from history retention and exports.
  5. **The completed file itself stays** in the download folder (it is the user's download). The policy covers what
     Downova remembers, not the file.
- **Acceptance for B:**
  - unit and process tests showing no trace in SQLite, the idempotency table or `storage.local` after completion,
    removal or restart;
  - a real-browser test in a private window (Chrome incognito, Edge InPrivate) on all three OSes;
  - a human check of the opt-in text.
- **Size:** a multi-increment feature touching every layer; comparable to automatic capture itself (B2.13).

## Decision record (to fill in when chosen)

| Field | Value |
|---|---|
| Chosen option | — |
| Chosen by / date | — |
| Follow-up | A: amend BRW-10 and ADR 0019 §8 as above. B: open the private-mode ADR |

**E02 is not part of this decision:** its own priority line already places Opera, Brave and Vivaldi QA in B4.
