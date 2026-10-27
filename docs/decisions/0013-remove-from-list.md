# ADR 0013 — "Remove from list" keeps a tombstone

- **Status:** Accepted
- **Date:** 2026-10-27
- **Builds on:** ADR 0006 (SQLite, idempotency records), ADR 0007 (checkpoints), ADR 0008 (completion journal)
- **Requirements:** CORE-10 (the "clear list" part), COR-09, CORE-03
- **Evidence:** [B2.3 report](../reports/b2.3-status-cancel-remove.md)

## Context

CORE-10 asks for "Clear list" and "delete file" to be separate, explicitly labelled actions. The UI has had "Remove
from list" and "Delete" visible but unavailable since B2.1.

Deleting a job row is not safe. `idempotency_records.job_id` references `jobs` with `ON DELETE CASCADE`, so deleting
the row would also delete the request key. Suppose a client's acknowledgement for an add was lost, and the user
removed that job before the client retried. The retry would find no key and create a second download of the same
request, which breaks COR-09.

## Decision

1. **New method `downloads.v1.remove(JobRef)`** (contract enum, Rust `methods::REMOVE`, generated TypeScript). It
   answers with the job's snapshot, which now carries `removed: true`. The optional `removed` field is omitted when
   false.
2. **Only finished jobs.** A job can be removed only when it is completed, failed or cancelled, has no attempt and has
   no finalization entry. Anything else is refused with `INVALID_REQUEST`, including paused and policy-blocked jobs;
   cancel first. The store checks the same condition again inside its transaction.
3. **A tombstone, not a deletion.**
   - Migration 5 adds `jobs.removed_at_ms`. `Store::remove_job` sets it in one transaction before the service
     acknowledges.
   - Ordinary job writes never touch that column, so nothing later can undo a removal.
   - If the write fails, the service answers `INTERNAL` with `next_action: retry`, and the job stays in the list.
4. **What clients see:**

   | Request | Answer |
   |---|---|
   | `list` | omits removed jobs, also after a restart |
   | `get` | still answers, with `removed: true` |
   | `remove` of a removed job | answers it again; the first removal time is kept |
   | `add` with a retained key and the same request | the original (removed) job; nothing starts |
   | `add` with the same key and a different request | still `IDEMPOTENCY_CONFLICT` |
   | `pause` / `resume` / `cancel` | refused, as for any finished job |

5. **Never files.** Removal changes no file: not the final file, not a partial or control file, and nothing else. It
   never cancels a transfer.
   - Deleting files remains unavailable; the Delete command says so.
   - "Cancel download" is a separate action with its own confirmation.
6. **Retention.**
   - At startup the service prunes idempotency records older than 30 days (`IDEMPOTENCY_RETENTION_MS`).
   - It then deletes tombstones that were removed before that cutoff **and** are no longer referenced by any key.
   - A tombstone therefore lives at least as long as any key that could replay it, and the database does not grow
     without bound.

## Consequences

- Wire change is additive: one method, one optional field.
- The UI drops a removed row as soon as the acknowledgement arrives, and ignores list answers that started earlier
  (B2.1's stale-answer rule). Lists after that never contain the row.
- **Not covered:**
  - "Clear list" for many jobs at once;
  - history search;
  - user-configurable retention;
  - undo;
  - deleting files.

  These remain CORE-10 work.
