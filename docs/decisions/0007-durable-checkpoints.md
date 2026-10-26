# ADR 0007 — Engine-owned durable checkpoints and validated recovery after a restart

- **Status:** Accepted
- **Date:** 2026-10-26
- **Builds on:** ADR 0002 (backend seam), ADR 0004/0005 (`dm-backend-http`, default backend), ADR 0006 (SQLite)
- **Requirements:** CORE-04, ARCH-05, COR-08, COR-09
- **Evidence:** [B1.2 crash-recovery report](../reports/b1.2-crash-recovery.md)

## Context

B1.1 kept jobs and idempotency records across restarts, but a job unfinished at a crash could only be reported as
interrupted. `dm-backend-http` already resumed paused transfers safely in-process, but only from in-memory state: a
confirmed offset (or one per segment) and the live `Sha256` hasher of those bytes. That state did not survive the
process. ARCH-05 forbids a service-side segment map, and COR-08 requires that metadata never claims more than is on
disk.

## Decision

1. **The checkpoint belongs to the engine and is opaque to the service.**
   - `dm-backend-api` gains `DurableCheckpoint { backend, format_version, confirmed, payload }` (payload ≤ 64 KiB) and two
     trait methods:
     - `checkpoint(handle)` exports the latest durable state, or `None`;
     - `restore(request, checkpoint)` re-imports it into a **paused** attempt.
   - A new capability, `DurableResume`, gates its use:
     - `dm-backend-http`: `Supported`.
     - aria2: `Unsupported`. Its `checkpoint` always returns `None` and `restore` refuses, so aria2 jobs interrupted by a
       restart keep the explicit "interrupted" failure.
2. **`dm-backend-http` format v1 (JSON):**
   - the mode (`single` or `segmented`), the destination directory and file name, the final URL, the strong ETag and the
     length;
   - one region per connection (`start`, `end`, `done`), with the **SHA-256 digest** of the confirmed bytes of that
     region.

   The in-memory hasher state is never stored. Transfers without a strong ETag and a known length export nothing. Any
   change to the format gets a new `format_version`. An importer accepts only versions it knows and refuses others
   explicitly; there is no silent migration of partial data.
3. **Durability order (COR-08):**
   1. A running writer `flush`es, `sync_data`s, and only then publishes its confirmed state, at least every second or
      4 MiB. Single-connection and segmented transfers each do this per worker. Stopping (pause, shutdown) also syncs
      before publishing.
   2. The service reads the published state and commits it with the job row in one SQLite transaction (WAL,
      `synchronous=FULL`). This happens:
      - on pause, together with the paused state;
      - every 2 s while transferring, if it grew;
      - for each running job at graceful shutdown.

   Live progress counters are never committed as checkpoints.
4. **Storage, migration 2:**
   - `jobs.attempt_id` (the current attempt) and `resume_checkpoints` (job, attempt, backend id and version, format,
     confirmed bytes, opaque payload).
   - `save_checkpoint` is refused (`StaleAttempt`) unless the stored job is unfinished and its current attempt wrote the
     checkpoint.
   - `update_job` deletes, in the same transaction, a checkpoint whose job became terminal or moved to another attempt
     (for example a retry from zero).
   - `rebind_checkpoint` moves a checkpoint to the recovered attempt atomically.
5. **Recovery:**
   - At start, each unfinished job with a checkpoint usable by this backend (same backend id, `DurableResume` supported)
     gets a **new service-owned attempt**.
   - The backend import refuses:
     - another backend, an unknown version, an oversized or corrupt payload, or another destination;
     - regions that do not tile the file;
     - an existing destination file;
     - a partial file that is missing, shorter than the confirmed data, or whose confirmed regions no longer match their
       digests.
   - Otherwise it rebuilds the hashers from the bytes on disk.
   - Bytes past a single connection's confirmed offset were written after the last checkpoint (a crash between a write
     and the commit), so they are **truncated**, never trusted. In segmented mode the file is pre-sized, and unconfirmed
     bytes inside a segment are overwritten when that segment continues.
   - The job keeps its ID, and old attempts cannot update it: progress is attempt-guarded and checkpoints are
     attempt-checked.
   - Paused jobs stay paused. Running jobs resume through the existing validated `Range` + `If-Range` path with strict
     `206` checks. A `200` restarts from zero, and a changed validator, a redirect or an invalid range keeps its
     existing safe outcome.
   - Cancelled and completed jobs are never touched.
   - A refused import fails the job with an explicit reason ("the interrupted download could not be continued
     safely: …"), drops the checkpoint, and leaves the partial file as it is.
6. **Test hook:** `DOWNOVA_CHECKPOINT_TRACE=<file>` appends one JSON line per committed checkpoint and recovery outcome
   (job, attempt, bytes; never URLs). Process tests use it as a deterministic barrier because the database is locked by
   the running service.

## Consequences

- After a hard kill, an `http` download continues from at most about 3 s (or 4 MiB plus 2 s) before the crash. The
  re-fetched bytes are validated like any continuation.
- The guarantee comes from process-kill tests. **Power-loss safety is not claimed.** It depends on the OS and storage
  honouring `fsync`/`sync_data` and SQLite `synchronous=FULL`, which no test here exercises.
- Recovery reads every confirmed byte once to verify it, and the backend's `resume` verifies again before continuing.
  This costs time on very large files; a later optimisation may skip the second pass for a just-restored attempt.
- The payload contains the final URL (it may carry signed-query tokens). It stays in the private per-user database and
  is never logged or exported, as with `jobs.url` (ADR 0006).
- Open: the cost of `synchronous=FULL` is not benchmarked (COR-08). The desktop UI does not show recovery outcomes yet
  (B2). Engine kill separate from the service applies only to aria2, which does not support durable resume.
