# ADR 0020 — Explicit Retry and safe source replacement

- **Status:** Accepted
- **Date:** 2026-10-31
- **Amends:** ADR 0018 (finish time), ADR 0002 (backend seam: two default methods)
- **Requirements:** CORE-03, COR-06, COR-05, CORE-10
- **Evidence:** [B2.15–B2.20 batch report](../reports/b2.15-b2.20-batch.md)

## Context

Until now a failed job never continued: the user had to add its link again, which created a second
job. An expired or replaced link had no remedy either. Both need operations on the *existing* job
that are safe to repeat, never splice unverified bytes, and never discard partial data without the
user's choice.

Two facts constrain the design:
- A failed job has **no resume point**: the store drops a job's checkpoint at its first terminal
  write (ADR 0007), and the automatic retry policy discards the failed attempt's partial data.
- An equal ETag is only meaningful for the resource it came from. Equal length proves nothing
  (COR-02).

## Decision

1. **Operation identity.**
   - `downloads.v1.retry` and `downloads.v1.replace_source` take a client-chosen `operation_id`.
   - The first request that changes the job records `(operation_id, kind, job, request fingerprint,
     outcome)` in the new `operations` table (migration 8), in the **same transaction** as the change.
   - A repeat answers with the recorded outcome and never starts another attempt, also after a lost
     acknowledgement or a restart. The same id for a different request is `IDEMPOTENCY_CONFLICT`.
   - Refusals and `choice_required` answers record nothing, because nothing changed.
   - An optional `expected_attempt_id` refuses the request when the job moved to another attempt
     meanwhile.
   - Operation rows expire with the idempotency records (30 days). A job they reference is not purged.
2. **Retry (CORE-03).**
   - **Eligible:** a listed, failed job that is not finalizing.
   - **Never eligible:** completed, cancelled and removed jobs, and jobs in an automatic cool-down
     (those are not failed).
   - `LINK_REFRESH_REQUIRED` is refused with `next_action: refresh_link`, and `AUTH_REQUIRED` with
     `sign_in`.
   - **Always from zero.** When the job holds partial data the request must carry
     `discard_partial: true`; otherwise it is refused with `choose_restart_or_keep`.
   - **Order:**
     1. Release the old attempt's ownership and partial output: `discard` of a live handle, or the new
        backend method `discard_output` for a leftover of an earlier service run.
     2. Commit the reopen with the operation.
     3. Admit through the normal queue, priority and limit rules.

     Stale callbacks of the old attempt are dropped by the attempt guard.
   - **Kept:** queue, priority, manual position, destination binding, staging and final names, and the
     trusted checksum. The automatic-retry counter restarts.
   - A kill between release and commit leaves a failed job without partial data, and Retry applies once
     afterwards.
3. **Finish time (amends ADR 0018).**
   - `finished_at_ms` is still stamped once per terminal outcome and never moved by ordinary writes.
   - The **only** path that clears it is the explicit reopen (`Store::reopen_job`: Retry, or a Refresh
     link restart of a failed job), in the same transaction. The next terminal write stamps the new
     outcome.
   - A retried job that completes is therefore never expired by its old failure's timestamp, and
     metadata edits still never extend retention.
4. **Refresh link (COR-06).**
   - Applies to a paused or failed job that is not waiting for disk space and not finalizing.
   - **Keep progress (`keep_if_verified`)** only with all of these proofs:
     1. The job has a live checkpoint. A failed job has none: `no_checkpoint`.
     2. The backend's `verify_replacement` resolves the new link through the gated, manual redirect path
        and asks for the **last confirmed bytes** (at most 64 KiB) of the first confirmed region.
     3. The answer is a `206` with the checkpoint's **strong ETag**, the same total length and exactly
        the requested `Content-Range`, and its bytes equal the bytes on disk.
     4. The final URL has the **same origin and path** (only the query differs, e.g. a new signature),
        **or** the job has a trusted SHA-256 that will verify the completed file. An equal ETag on
        another resource is never proof by itself (`different_resource`).
   - **On success:**
     1. The old attempt is released; its files are kept.
     2. A new attempt is restored from the checkpoint rebound to the new final URL; the backend
        re-verifies every confirmed region on import.
     3. The job row (new source, new attempt), the rebound checkpoint and the operation are committed in
        one transaction.

     Until that commit the stored job keeps its old source and checkpoint, so a kill recovers the old
     source. A failed restore re-attaches the old checkpoint; if that also fails, the job ends with an
     explicit recovery error.
   - **Otherwise** the answer is `choice_required` with a reason (`validator_missing`,
     `validator_changed`, `length_changed`, `range_ignored`, `content_differs`, `different_resource`,
     `no_checkpoint` or `unverifiable`) and **nothing changes**. "Keep current" in the UI sends nothing.
   - **Restart** (`restart`) is explicit:
     - it releases only this job's partial data and commits the new source;
     - a failed job runs again, while a paused job stays paused and starts from zero on Resume.
   - The resumed transfer still appends only validated `206`s (COR-01). A server that answers `200`
     restarts from zero, never splices.
   - The replacement URL is never echoed, logged or published. Snapshots show `redact_url` only.
5. **Backend seam (amends ADR 0002).** `DownloadBackend` gains two default methods:
   - `discard_output`: by default refused with `BACKEND_CAPABILITY_UNAVAILABLE`;
   - `verify_replacement`: by default `Incompatible(Unverifiable)`.

   The HTTP backend implements both. aria2 keeps the defaults: a Retry there after a restart is refused
   with a typed error, and Refresh link can only restart.
6. **Locate (B2.19).** `downloads.v1.locate` answers a job's bound folder and, once completed, its final
   file name, from the service's own record. It serves the user-triggered "Open containing folder"
   ([ADR 0021](0021-user-triggered-folder-reveal.md)).

## Consequences

- One logical job per download through failures and new links. Adding the link again stays possible but
  is no longer the only remedy.
- Partial data is discarded only after an explicit choice. Progress survives a new link only with
  byte-level evidence, and a trusted checksum is required whenever the resource differs.
- A legitimate mirror without a publisher checksum restarts from zero. This is the price of never
  splicing unproven bytes.
- Not covered: authenticated links (sign-in), keeping progress after a failure (no resume point exists),
  and aria2 continuations.
