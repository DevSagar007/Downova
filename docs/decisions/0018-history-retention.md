# ADR 0018 — Service-owned history retention

- **Status:** Accepted; amended by [ADR 0020](0020-explicit-retry-and-source-replacement.md) (an explicit Retry or Refresh-link restart clears the finish time; nothing else does)
- **Date:** 2026-10-31
- **Builds on:** ADR 0006 (storage), ADR 0013 (remove from list), ADR 0016 (event stream)
- **Requirements:** CORE-10
- **Evidence:** [B2.12–B2.14 batch report](../reports/b2.12-b2.14-batch.md)

## Context

CORE-10 asks for a history retention policy. History entries are jobs that stay listed after they
finish. Removing them must use the existing durable removal (ADR 0013). Removal must never touch
files, discard backend output or break idempotent replays. It must also work while no window is
open, so retention belongs to the service and not to the desktop.

## Decision

1. **Policy.**
   - `history.v1.get` / `history.v1.set` with `{ retention_days: 1–3650 | null }`. `null`, the default, keeps
     history indefinitely. The desktop offers forever, 7, 30, 90 and 365 days.
   - The field must be present; a missing field is an error, never "forever".
   - The policy is stored under its own settings key, apart from `settings.v1`. `ServiceSettings` refuses
     unknown fields, and saving destination rules (an older client) can therefore never reset retention.
2. **Durable finish time.**
   - Migration 7 adds `jobs.finished_at_ms`. The storage layer stamps it at the **first** write that makes the
     job completed, failed or cancelled (with that write's `updated_at`) and never changes it afterwards.
     Renames, queue moves, settings and removal leave it alone.
   - **Legacy rows** that were already finished when migrated count from the upgrade time. A retention period
     can never remove them earlier than it would remove a job finished at upgrade.
   - A job that finished in the current run counts from the first sweep that sees it. That is never earlier
     than the stored stamp, so retention can only be late, never early.
3. **Eligibility.** All of these must hold:
   - completed, failed or cancelled;
   - not already removed;
   - no open finalization journal entry;
   - `now − finished_at ≥ retention_days` (wall clock).

   Unfinished, paused, queued, blocked and finalizing work never expires. A clock set back makes nothing due.
   A clock set forward can make entries due early, but only list entries are affected.
4. **Sweeps.**
   - One bounded batch (100, oldest first) at startup before clients connect, then one per minute from the
     poll loop.
   - `history.v1.clean` applies the saved policy now, in batches.
   - `history.v1.preview` returns `{ expiring, finished }` for any policy without changing anything. The UI
     shows that exact count and asks before applying a policy that would remove entries now. On confirmation
     it saves the policy and cleans at once, so what the user confirmed is what happens.
5. **Removal.**
   - Each due job goes through the same `tombstone` path as `downloads.v1.remove`: release a terminal attempt's
     handle (keeping every file), then record `removed_at_ms`.
   - A removal the store refuses keeps the job listed. It is reported as `kept` and retried by the next sweep.
   - Sweeps publish at once, so open clients receive `job_removed` and never resurrect the row.
   - Tombstones keep answering `get` and same-key replays. Their purge still waits for the 30-day idempotency
     retention (ADR 0013).
6. **Never:** deleting downloaded or partial files, backend discard, opening or executing anything.
7. **Test hook:** debug builds read `DOWNOVA_TEST_NOW_OFFSET_MS` to move the service's clock forward in process
   tests. Release builds always read 0.

## Consequences

- With a finite period, a finished download leaves the list without a further prompt once it ages out. The
  setting's description says so, and files stay on disk.
- Retention uses the wall clock. A clock moved forward by hand can make entries due early. That is documented
  and accepted, because no file is affected.
- Not covered:
  - per-category or per-queue retention;
  - size-based limits;
  - automatic deletion of files (deliberately never).
