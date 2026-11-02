# ADR 0023 — Configurable active downloads and connection sharing

- **Status:** Accepted; packaged acceptance pending
- **Date:** 2026-11-02
- **Requirements:** CORE-02, CORE-05, E03, CORE-03, COR-01, COR-02
- **Amends:** [ADR 0010](0010-connection-budget-and-admission.md) and [ADR 0017](0017-queues-and-priority.md)

## Problem

The service had an unconditional three-file admission cap. Raising a queue's limit could not
raise it. Long segment requests could also occupy all connections to a host until their
segments finished, delaying a later file's probe and body.

## Decision

1. Add `concurrency.v1.get` / `concurrency.v1.set` with the closed object
   `{ "max_active_downloads": N }`. N is an integer from 1 through 16; default 3.
   Persist it under its own key in the existing settings table. Destination, history and
   notification writes cannot reset it. No database migration is needed.
2. Validate and save before acknowledging or changing the effective limit. Admission remains
   under the coordinator lock. Raising admits eligible waiting work immediately, subject to
   queue capacity, priority, order and blockers. Lowering never interrupts an existing attempt;
   new starts wait for capacity. Restore the setting before recovery. Clients never retry a
   save automatically after a lost acknowledgement; they can explicitly read the setting back.
3. Queue limits remain independent. Existing queues retain their settings (including main=3).
   To run six jobs in one queue, set both limits to at least six. A global edit never increases
   another setting silently, starts a stopped queue or resumes a manually paused job.
4. Keep the native HTTP host budget at eight and the maximum at four segments per file. Count
   probes, transfers, ranges and redirect hops with the same RAII permits as before.
5. Track host-capacity waiters with a cancellation-safe guard. A **validated segment** seeing
   waiters yields after at most 1 MiB of further body data plus one received chunk. Flush and
   sync its bytes, drop the response/body and permit, then rejoin the semaphore's FIFO queue
   for the remaining range. Every request again validates status, range, strong ETag, length
   and encoding. No request retains a permit while asking for another. With no contention,
   the ordinary full-segment request is unchanged, avoiding extra handshakes.
6. Pause/removal/failure cancel pending acquisitions. A failed flush ends the transfer rather
   than causing an endless rotation loop. Checkpoint layout, logical job/attempt identity and
   finalization are unchanged. A 200 fallback still restarts the whole file safely.
7. **Amended during Windows integration (2026-11-02).** A request that yields publishes the segment's synced
   state to the exported checkpoint, as the periodic sync does. Yielding turns end before the periodic
   thresholds (4 MiB or 1 s within one request), and each new request restarts those counters. Without
   this, a file under steady contention exported no checkpoint at all, and an abrupt stop lost all of its
   progress. Process test A05 found it on Windows; backend test C07 covers it.

## Limits

- This is basic bounded connection sharing, not equal bandwidth or adaptive tuning (ADV02).
  Slow/stalled reads use the existing read timeout. The turn bound concerns data, not time.
- Single-connection resources without the required range/identity proof are not interrupted.
  More than eight such native streams on one host still wait for host capacity.
- The optional aria2 adapter has no native shared-host limiter and retains its conservative
  three-file admission cap. A higher saved setting is available when using the normal native
  HTTP backend. The UI explicitly explains this adapter limit.
- No speed, RAM or IDM-comparison claim is made. Additional files share network/disk resources,
  and contention can add range requests and handshakes.

## Evidence

See [the report](../reports/configurable-concurrency.md) for commands, results and platform gaps.
C05 asserts that a late file receives bytes before the earlier file finishes and both hashes
match. A cancellation test checks that a dropped waiter neither leaks pressure nor frees
another request's slot. Service tests cover live changes, policies, invalid RPCs and persistence.
C07 asserts that every file exports a durable checkpoint while all six still run under contention.
Process test A05 (a real service, an abrupt stop and a restart) passed on Windows only after amendment 7.
