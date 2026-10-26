# ADR 0008 — Staged output and a durable completion journal

- **Status:** Accepted
- **Date:** 2026-10-26
- **Builds on:** ADR 0002 (backend seam), ADR 0006 (SQLite), ADR 0007 (durable checkpoints)
- **Requirements:** ADV06, COR-05, CORE-04, COR-08
- **Evidence:** [B1.3 completion-journal report](../reports/b1.3-completion-journal.md)

## Context

Until B1.3, `dm-backend-http` published a finished transfer itself:
- it hard-linked `<name>.downova-part` to `<name>`, or renamed it after an existence check where links were unavailable;
- only then did `dm-service` check the length and the expected SHA-256.

That had three flaws:
- a file that failed verification was already at its destination;
- the rename fallback could replace a file created between the check and the rename (and `std::fs::rename` replaces on
  every platform);
- nothing durable recorded that finalization was under way, so a crash between publishing and the `completed` commit
  left a placed file with a job that recovery treated as an interrupted transfer.

## Decision

1. **The backend stops at a staged output.**
   - `TransferState::Complete` now means "complete and synced, still staged".
   - `DownloadBackend::staged_output(handle)` returns one of:
     - `StagedOutput::Staged(path)`: the attempt's own `<name>.downova-part`, in the destination directory (same
       volume);
     - `StagedOutput::PlacedByEngine(path)`: aria2, see 6.
   - `dm-backend-http` no longer publishes. Transport, checkpoints and segments stay inside it.
   - A checkpoint that already covers the whole representation finishes without a request, instead of asking for
     `Range: bytes=<length>-`.
2. **The service finalizes through a journal** (`finalization_journal`, migration 3). There is one row per job, bound to
   the attempt, the staging path and the final path. Each stage is committed **together with the job row** before the
   next one starts:
   1. `intent`: job `verifying`; the attempt must be current (`StaleAttempt` otherwise).
   2. `verified`: length and SHA-256 of the staging file. If a trusted `expected_sha256` was given, it must match: a
      mismatch (or a wrong length) fails the job with `CHECKSUM_MISMATCH` (or `INVALID_RANGE`), deletes the attempt's
      own bad staging file, and places nothing.
   3. `placed`: the file is at the final path (point 3 below).
   4. `complete`: the job becomes `completed` with its verification evidence, the checkpoint is dropped, and the stage
      is recorded, all in one transaction. Then the staging name is removed, the backend attempt forgotten and the row
      deleted.
3. **Placement never replaces anything.**
   - First a hard link: atomic, it fails if the final name exists, including when another process creates it at the same
     moment.
   - Where hard links are unsupported, the fallback is `create_new` (also atomic against an existing name) plus a synced
     copy, re-verified by SHA-256. A half-finished copy removes only the file it just created.
   - The existence-check-plus-rename fallback is gone.
   - On Unix the directory is synced after placement. Windows has no portable safe-Rust directory sync, so there it is
     not done.
4. **Evidence:** `DownloadSnapshot.verification = { sha256, source }`, where `source` is
   - `local_hash`: computed, nothing to compare with; or
   - `publisher_checksum`: matched the trusted value.

   It is stored in `jobs.verified_sha256` and `jobs.verification_source`. The contract schema, the generated
   TypeScript and the Rust mirror changed together, with a new `download-snapshot.completed` example.
5. **Restart reconciliation** runs before checkpoint recovery. It continues each entry from its stage and re-checks every
   file by SHA-256 before relying on it:

   | Stage at the crash | Reconciliation |
   |---|---|
   | `intent` | Re-hash the staging file. Missing → explicit failure. |
   | `verified` | The staging file must still hash to the recorded value (else the job fails, files untouched). If the final name exists and holds exactly those bytes, it was our own unrecorded link: adopt it. Any other existing file is a collision: fail, never overwrite. |
   | `placed` | The final file must hash to the recorded value. If it was removed but the staging file is intact, place it again. If it changed, fail and leave it. |
   | `complete` | Cleanup only: the staging name is removed only if its bytes are the verified ones. |

   Reconciliation is idempotent: repeated crashes during it converge (tested).
6. **aria2 keeps engine placement.**
   - aria2 writes `<name>` itself (`--allow-overwrite=false`, `--auto-file-renaming=false`), so its journal entry is
     `placement = engine`. Verification happens **in place, after the file is already visible**.
   - A failed check fails the job but cannot un-place the file. aria2 is optional and never bundled (ADR 0005).
7. **Cancellation and stale attempts:**
   - Journal writes are attempt-checked.
   - Finalization runs under the coordinator lock, so a cancel waits for it.
   - A cancel is refused once the file is placed ("already at its destination and is being completed"); before that it
     abandons the entry in the same transaction as the job.
   - Late events of old attempts stay attempt-guarded.
8. **Fault hooks** (`DOWNOVA_FAULT`: `after_intent`, `after_verify`, `after_link`, `after_place`, `after_commit`,
   `hold_before_place`, `fail_journal_once`) exist only in **debug builds** (`cfg(debug_assertions)`). Release packages
   contain no fault paths.

## Consequences

- No file at a destination is ever produced by a transfer whose bytes failed verification (`http` backend).
- No existing file is ever replaced, whatever the timing.
- After a crash at any finalization boundary, the job ends `completed` with the exact file, or `failed` with an
  explicit reason and every file left as found.
- Finalization hashes the file once, plus once more for each stage reconciled after a crash. The coordinator lock is held
  while hashing, so other API calls wait for large files; a background finalizer is a later optimisation.
- **Process kills only.** No power-loss claim: directory sync is missing on Windows, and durability depends on the OS
  and storage honouring `fsync`.
- Origin marking (MOTW) and malware scanning (ADV06 stages between `verified` and `placed`) are not implemented and are
  not reported as done.
