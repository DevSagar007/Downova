# ADR 0027 — Secret lifecycle, OS credential store, and scoped authenticated GET

- **Status:** Accepted for the B3.3 feature branch (2026-11-07). It was developed under the narrow exception the
  maintainer extended to SEC-01, SEC-02 and BRW-05. **B2 is not exited**: installed-app acceptance, the kit results
  and BRW-10 remain release gates.
- **Requirements:** SEC-01, SEC-02, BRW-05 (with BRW-03 and BRW-06 unchanged)
- **Builds on:** [ADR 0019](0019-automatic-capture.md) (capture policy), [ADR 0026](0026-attachment-safety-scan-states-and-diagnostics.md)
  (diagnostics, `export_leaks`)
- **Amended by:** [ADR 0028](0028-reconnect-signed-in-downloads.md): a lost sign-in holds the download (partial data kept) for a reconnect instead of failing it; signed-in sources display as their origin only.
- **Evidence:** [B3.3 report](../reports/b3.3-secure-authenticated-downloads.md)

## Context

The audit before this batch found:

- `jobs.url` stores the raw request URL (signed query values, userinfo and fragments included).
- The native HTTP checkpoint payload (`crates/backend-http/src/durable.rs`, `Payload.url`) stores the **final URL
  after redirects** as plain JSON. The service persists it unchanged in `checkpoints.payload`. Its encoding and
  region checksums protect integrity, not confidentiality.
- `TransferRequest.headers` (`Secret`-wrapped) exists but no path fills it. The extension keeps cookie-carrying
  downloads in the browser (BRW-06), and the native host rejects a `cookies` field.
- There is no OS credential store.

## Decision

### 1. Secret classes and where each may exist

| Material | Ingress | May exist in | Never in |
|---|---|---|---|
| Session cookies (BRW-05) | The extension, after a user action and permission → native host (stdin frame) → IPC → `downloads.v1.add_authenticated` | Service memory as `Secret`; the backend request for this job's attempts; the OS vault, only when the user chose **Remember** | SQLite, snapshots, events, logs, diagnostics, CLI or desktop output, extension storage, aria2 |
| Protected URL material (userinfo, query, fragment of any URL; the whole URL of an authenticated job) | The add request | Service memory; SQLite only **sealed** (AES-256-GCM) under the data key | Any plaintext column or blob |
| The data key (32 random bytes) | Created by the service | The OS vault entry `Downova data key` | Anywhere else |

- **Ingress DTOs:** cookie values arrive in `SessionInput.cookies[].value`, a **deserialize-only** `SecretInput`.
  It has no `Serialize`, and its `Debug` shows `REDACTED`. It becomes the core `Secret` at once. Ordinary domain
  types never serialize a secret.
- **Responses** (snapshots, `credentials.v1.list`, the native host's answer) carry only display-safe metadata: the
  origin, the mode, the state and an opaque id. **There is no endpoint that returns a secret.**

### 2. Temporary and remembered context

- **Temporary (the default):**
  - the cookies live only in service memory, bound to one credential id and one job;
  - they end when the job ends (completed, failed or cancelled), when the user selects **Forget**, or when the
    service stops;
  - after a restart the job holds with `credential_missing`, and the action is **reconnect**: start the download
    again from the browser.
- **Remembered (an explicit choice in the extension, per request):**
  - the cookies are written to the OS vault under `Downova/<namespace>`, account `cred_<id>`;
  - SQLite gets a `credentials` row: id, origin, browser, profile id, mode, generation, state, and the earliest
    cookie expiry;
  - labels and lookup attributes contain the id and the origin only, never a value.
- **The reference:** `cred_<16 hex>` is derived from the idempotency key and the origin, so a retried add maps to
  the same id. It is bound to `(origin, browser, profile_id, generation)`, and a job references exactly one id.

### 3. SQLite and vault consistency

- **Add (remembered):**
  1. the credentials row is written `state = pending`;
  2. then the vault write;
  3. then the job row and the credential become `active` in one transaction.
- **Failures:**
  - a failed vault write deletes the pending row and refuses the add (`CREDENTIAL_STORE_UNAVAILABLE`), with no job;
  - a vault success followed by a SQLite failure deletes the vault entry before the error is returned.
- **Startup reconciliation:** a `pending` row (a crash between the steps) has its vault entry deleted, if present,
  and then the row. Remaining vault entries are never enumerated beyond Downova's own ids.
- **Lost acknowledgement:** the same idempotency key returns the same job. The vault entry is replaced (the same
  id), never duplicated.
- **Forget:**
  - deletes the vault entry and marks the row `forgotten`;
  - the referencing jobs keep their record, and an unfinished one holds with `credential_missing`;
  - forgetting never deletes another application's entries or files.

### 4. Vault errors

`dm-platform::vault` maps the store's errors:

| Error | Meaning | Result |
|---|---|---|
| `Locked` | `NoStorageAccess` (locked keychain, denied) | Job held with `credential_locked`; action: unlock, then resume |
| `Missing` | `NoEntry` | `credential_missing`; reconnect |
| `Unavailable` | No platform store, D-Bus without Secret Service, other platform failures | `credential_unavailable` |
| `TooLarge` | The payload exceeds the store's limit (Windows 2,560 bytes) | Remember refused, explicitly; a temporary context still works |

- **Each failure is reported with a message, never a secret.** Vault calls run in `spawn_blocking`, never under the
  coordinator lock.
- **There is no fallback store:** no file, no environment variable, no SQLite blob, no engine configuration.
- **Public downloads never need the vault.** If the data key is unavailable, protected URL material is kept in
  memory only. Such a job works in this run; after a restart it needs **Refresh link** (B2.17), and it says so.

### 5. URL and checkpoint persistence (SEC-01)

- **What is sealed:**
  - `jobs.url` holds the **display** form (`redact_url`);
  - when the URL has userinfo, a query or a fragment, or the job is authenticated, the full URL is stored as
    `jobs.url_sealed`: AES-256-GCM via `ring`, a random 96-bit nonce, and AAD = `job_id || "url"`, so a ciphertext
    cannot be moved to another job;
  - `checkpoints.payload` is sealed the same way (AAD = `job_id || "checkpoint"`) whenever the key is available.
- **Identity:** the idempotency fingerprint already uses a SHA-256 of the request. The duplicate check compares
  full URLs in memory, so two resources that differ only in a redacted part never become "the same".
- **Path segments:**
  - a token inside a path segment cannot be recognised, so for **public** jobs the display form keeps the path;
  - that is a documented limitation, mitigated only by sealing the whole URL of authenticated jobs (whose display
    form is the origin plus the last path segment).
- **Upgrade (migration 13):**
  - the migration adds columns only;
  - existing plaintext rows are re-sealed by the service after it has the key: one row per transaction, so a failure
    leaves that row as it was, never half-written;
  - if the vault is unavailable, legacy rows stay as they were and diagnostics count them.
  - **No claim of secure deletion is made:** older SQLite pages, WAL frames and backups may still hold former
    plaintext. The service runs `PRAGMA wal_checkpoint(TRUNCATE)` and `VACUUM` after re-sealing (best effort), and
    documents that disk sectors and backups are outside its control.

### 6. Scoped authenticated GET (BRW-05)

- **The supported subset, all of which must hold:**
  - Chrome or Edge with the normal profile (incognito stays excluded, as ADR 0019 decided);
  - the user chose the context-menu item **Download with Downova using my sign-in**;
  - `cookies` and the link's host permission were granted at that moment;
  - an `https:` GET link whose origin equals the page or frame origin it was clicked in;
  - a top-level frame, or a same-site frame (no partitioned or CHIPS cookies);
  - the native HTTP backend (`authenticated_get` = `supported`; aria2 reports `unsupported` and refuses).
- **Anything else** gets an actionable `unsupported` answer, and the browser keeps its own flow.
- **Cookies** come only from `chrome.cookies.getAll({ url, storeId })`:
  - the store is the tab's own store;
  - expired, partitioned and non-applicable cookies are dropped;
  - no browser database, password store, page script or local storage is read.
- **The engine computes the `Cookie` header per request with RFC 6265 rules:**
  - domain match (host-only versus domain);
  - path match;
  - `Secure` only on `https`;
  - expiry checked at that moment.
- **This applies to the probe, every redirect hop, every segment, every resume and every retry,** so a same-origin
  redirect to another path re-evaluates which cookies apply.
- **Origin = scheme + host + effective port:**
  - a redirect that leaves the origin, `https`→`http` included, **stops** the job with `AUTH_SCOPE_UNSUPPORTED`
    ("finish it in the browser"), instead of continuing without the session;
  - `Cookie` and `Authorization` are never sent to another origin.
- **No retries on 401 or 407:** the job fails or holds with `credential_expired` and the action **reconnect**. 403
  keeps its existing meaning (refresh the link).
- **No session refresh in the background:** a fresh session needs a fresh browser action.
- **Unchanged:**
  - public context-menu handoff, automatic capture (BRW-03, BRW-06), the fallback;
  - "accepted" versus "transfer established";
  - POST, blob:, data:, browser-generated and one-time downloads stay in the browser.

## Consequences

- **Migration 13:**
  - `credentials` table;
  - `jobs.credential_id`;
  - `jobs.url_sealed`;
  - `checkpoints.sealed`.
- **New dependencies** (MIT OR Apache-2.0):
  - `keyring-core` 1.0;
  - `windows-native-keyring-store` 1.1;
  - `apple-native-keyring-store` 1.0 (keychain);
  - `zbus-secret-service-keyring-store` 1.0, which brings `secret-service` 5.2 and reuses `zbus` 5.19;
  - `ring` (already locked) in `dm-storage` for AEAD.
- **Contract:**
  - `downloads.v1.add_authenticated`, `credentials.v1.list`, `credentials.v1.forget`;
  - `DownloadSnapshot.auth`;
  - the error kinds `AUTH_SCOPE_UNSUPPORTED` and `CREDENTIAL_STORE_UNAVAILABLE`;
  - capability `authenticated_get`;
  - the native-host op `add_authenticated`.
- **Not claimed:** compatibility with every login-protected site. Single sign-on hand-offs, other-origin CDNs,
  POST-based sessions and partitioned contexts stay in the browser.
