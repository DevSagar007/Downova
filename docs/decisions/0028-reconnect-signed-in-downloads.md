# ADR 0028 — Reconnect a signed-in download and keep its partial data

- **Status:** Accepted for the B3.3 feature branch (2026-11-07), under the same narrow exception as
  [ADR 0027](0027-secret-lifecycle-vault-and-authenticated-get.md) (SEC-01, SEC-02, BRW-05). **B2 is not exited.**
- **Requirements:** BRW-05, SEC-01, SEC-02 (BRW-03 idempotency, COR-02 resume validation, CORE-08 schedules
  unchanged)
- **Amends:** ADR 0027 (what happens when a signed-in download loses its sign-in; display of signed-in sources)
- **Evidence:** [B3.3 report](../reports/b3.3-secure-authenticated-downloads.md)

## Context

The first B3.3 increment ended a signed-in download that lost its sign-in. That happened after a service restart
(temporary sign-ins live in memory), when the site rejected the session (401), or when the user forgot the sign-in.
In each case the download **failed**. A failed job keeps no resume point (ADR 0020), so the partial data was lost.
The user was told to start again from the browser. The audit for this batch also found:

- **Resume after a restart bypassed the session check.** `restore_from_checkpoint` rebuilt the engine transfer with
  the job's session, which is `None` for a lost temporary sign-in. Admission then resumed it without the start
  guard: a cookie-less request. That request either fails (401, losing the checkpoint) or, on a site that also
  serves anonymously, silently becomes a public download.
- **Forget only paused the engine transfer.** That transfer keeps its own copy of the session, so a later manual
  Resume would have used the forgotten sign-in.
- **A repeated `add_authenticated` replaced the accepted job's sign-in.** A lost-acknowledgement retry, with cookies
  re-read by the browser, overwrote the context and the remembered store entry before the idempotency check.
- **The signed-in display still showed the last path segment,** which can be the token itself (`/dl/<token>`).
- **A locked credential store at startup failed jobs in memory only** (fixed during B3.3), but nothing could
  continue them without a restart.

## Decision

1. **Hold, never fail, for a missing sign-in.** A new blocker, `sign_in_required`, holds an unfinished signed-in
   job whenever its sign-in is missing, expired, forgotten or locked away. The hold works like the disk hold
   (B1.4):
   - the attempt's last durable checkpoint is committed, then the attempt is released while its partial file is
     kept;
   - without a durable checkpoint, the unconfirmed bytes are removed, which loses no resumable progress;
   - the job shows the kept (durable) byte count;
   - the desired state is kept;
   - nothing retries on its own: a 401 is one request, not a retry storm.

   The hold applies at four points: admission (fresh start or resume), startup recovery (the checkpoint is kept for
   the reconnect, not restored without a session), an engine `AUTH_REQUIRED` for a signed-in job (the context
   becomes `expired`, in memory and in its row; the secret is dropped from memory), and Forget (the attempt is
   released, so a later Resume cannot use the forgotten session). A manual Resume of a held job records the wish
   to run and stays held.
2. **Reconnect** (`downloads.v1.reconnect`) attaches a fresh browser sign-in to one existing job:
   - **Binding.** It names the job, the origin, the browser and profile, and `expected_credential_id` (the job's
     current credential). Every reconnect creates a new credential, so this id is the operation generation: a
     stale or replayed request is refused. The job's link must be the session's origin. The context's browser and
     profile must match: no mixing of Chrome and Edge, or of profiles. At least one cookie must apply to the job's
     own link. Public downloads never gain a session. Completed, cancelled, removed and finalizing downloads are
     never reopened.
   - **Idempotency.** `operation_id` is recorded with the change in one transaction (`operations`, migration 14
     widens its kinds). A repeat answers with the job as it is now and changes nothing. The fingerprint excludes
     cookie values, so a retry that re-read the cookies is still the same operation. The same id for another job,
     origin, profile, remember or resume choice is `IDEMPOTENCY_CONFLICT`. A repeated `add_authenticated` likewise
     answers with its job and never touches that job's sign-in.
   - **Consistency.** A remembered sign-in goes to the OS store first, outside every lock. Then the job is checked
     again under the lock (a cancel, removal, forget or concurrent reconnect wins), and one transaction records the
     new credential row, the job's new reference, the operation and the removal of the retired row. Any failure
     deletes the new store entry and leaves the old state unchanged. A refused remembered sign-in (locked,
     unavailable, too large) is an error, never a silent switch to temporary.
   - **Shared contexts.** The old context (memory, row and store entry) is retired only when no other download
     references it. Today every context belongs to one download: the credential id derives from the add's
     idempotency key, or from the reconnect's operation id. The rule is kept so that sharing can never orphan
     another download's sign-in.
   - **Continuing.** Reconnect alone keeps the desired state: a manual pause stays paused and a stopped queue stays
     stopped. "Reconnect and resume" also asks to run, under the normal queue, schedule, limit and disk rules. A
     held job continues from its checkpoint through the engine's restore, which re-verifies the partial data and
     the strong validator. A resource that changed is never spliced: the existing rule (COR-02, process test K04)
     restarts it with the whole new representation. A failed download (one that failed before holds existed) only
     gets its sign-in back; starting it again is Retry's decision. Retry now accepts an auth failure once a sign-in
     is available.
   - **Lost link.** If the link itself could not be kept (no data key), a new sign-in cannot continue it. The answer
     is `LINK_REFRESH_REQUIRED` with the reason. Refresh link still refuses signed-in jobs (ADR 0027), and the
     partial data stays until the user removes the download.
3. **Browser flow.** "Reconnect a Downova download with my sign-in" is offered on https pages and frames:
   - the page rule is checked first: a normal window, an https top-level page, and a frame of the same origin;
   - a frame of another site has partitioned cookie state, so it is refused (`rejected_signin_partitioned`) and
     never guessed;
   - the optional `cookies` permission for that one origin is requested inside the click, and the tab's single
     cookie store is resolved;
   - `credentials.v1.reconnectable` (metadata only) lists that origin's downloads for this browser profile;
   - a reconnect page asks for an explicit choice: nothing is preselected and nothing is matched by name;
   - cookies are read only when the user confirms: the root URL's cookies plus the host's own cookies on any path,
     filtered to cookies that apply to the host, without partitioned or expired ones;
   - records hold no secret (`storage.session`: origin, store, browser, metadata, the choice and its operation id);
   - one operation id per (request, download): a retry reuses it, and another download gets a new one.
4. **Display.** A signed-in download's source is shown as its origin only (`https://host/…`). Its file name is
   shown as the download's name. A site that puts a token in the last path segment and sends no file name still
   gives the file that name, on disk and in the list (a stated limit).

## Consequences

- A signed-in download no longer loses its partial data to a restart, an expired session or Forget. Nothing is
  ever requested without its session.
- The desktop and CLI show the hold (`sign_in_required`, "sign-in needed") and explain how to reconnect. Neither
  can reconnect by itself: cookies are never typed into or shown by Downova.
- The fake-engine tests cannot prove engine re-verification. That is covered by the process tests R1–R3 (real
  engine, HTTPS) and the real-browser rows S4 and S5.
- A parent-domain cookie restricted to a deeper path is not offered by the reconnect read. The service accepts the
  reconnect only if a cookie applies to the job's link, so the result is a clear refusal, not a broken download.
