# ADR 0009 — Advisory disk budget and recoverable disk-full handling

- **Status:** Accepted
- **Date:** 2026-10-26
- **Builds on:** ADR 0002 (backend seam), ADR 0007 (durable checkpoints), ADR 0008 (completion journal)
- **Requirements:** ADV05 (basic), COR-07, CORE-04, COR-08
- **Evidence:** [B1.4 disk-budget report](../reports/b1.4-disk-budget.md)

## Context

Before B1.4 a full disk surfaced as an ordinary write error:
- The job failed (terminal), its durable checkpoint was dropped with it, and the download had to start again.
- Nothing checked free space before writing.
- The completion journal's copy fallback could need a whole second copy of the file without asking.

ADV05 (basic) requires `DISK_FULL` to stop the job while keeping its partial file, a space check for the known
remaining bytes plus a margin, and never overwriting another file.

## Decision

1. **Probe:** `dm-platform::space`.
   - `OsSpaceProbe` uses `fs4::available_space` (safe Rust: `statvfs` `f_bavail` on Unix, `GetDiskFreeSpaceExW` on
     Windows). It returns `Available(bytes)`, `VolumeMissing` (the directory or its drive is absent), or
     `Unavailable(reason)`. It never returns an invented number.
   - The probe is a trait. Debug builds accept `DOWNOVA_TEST_FREE_SPACE_FILE` (a byte count, `error` or `missing`,
     re-read on every query) so tests never fill a real disk.
2. **Policy:** `dm-service::space::PlatformSpaceGuard`.
   - `free ≥ needed + 64 MiB` → proceed; otherwise `DISK_FULL`, with the amounts and "free some space and the download
     continues by itself".
   - A missing volume gives `VOLUME_MISSING`.
   - If the OS cannot tell, the check passes (advisory), is reported once on stderr, and the real `ENOSPC` path still
     protects the job.
   - The policy implements `dm-backend-api::SpaceGuard`, a trait in the backend seam. Backends receive it through their
     config (`HttpConfig.space`) and never depend on `dm-platform` or `dm-service`.
3. **Where it is checked:**
   - **Single connection, known length:** before the first body byte, and again at every durable sync (at least every
     second or 4 MiB): the remaining length.
   - **Unknown length:** the same points, for the next 4 MiB sync window. The final size is never claimed.
   - **Segmented:** before the file is sized, for the whole unconfirmed length (pre-sizing reserves no blocks, since
     sparse files are possible), and at every per-segment sync: that segment's remainder.
   - **Finalization:** a hard link needs no new data blocks and is not budgeted. The copy fallback is budgeted for the
     whole file first.
   - **Waiting jobs:** every 2 s (`DISK_RECHECK_INTERVAL`), for `total − durable` bytes (0 when the size is unknown).
4. **Advisory, not a reservation.** The checks compare a snapshot; nothing is reserved. Concurrent jobs and other
   programs on the same volume can still exhaust it, so a real `ENOSPC` (or `EDQUOT`/quota) remains expected, and both
   are classified `DISK_FULL`. Capacity is never promised.
5. **Disk full (or the volume missing) is recoverable, not terminal**, when the backend supports `DurableResume`:
   1. The last durable checkpoint is committed; data is synced before any checkpoint is published (ADR 0007), so
      nothing unsynced is claimed.
   2. The attempt is released and its partial file kept.
   3. The job goes back to `queued` with the `disk_unavailable` (or `volume_missing`) blocker and a retryable
      `DISK_FULL`/`VOLUME_MISSING` error with `free_disk_space`/`reconnect_volume`.
   4. Its desired state is untouched. No discard-before-retry happens, and the terminal failure path that drops
      checkpoints is not taken.

   With **nothing durable yet** there is no progress to keep. The unconfirmed partial (bytes that could never be trusted
   for a resume) is removed, and the job starts afresh once space returns. Across a restart such a job keeps waiting
   (its persisted error says why) instead of failing as interrupted.
6. **Continuing:**
   - When the budget fits, the job continues through the validated restore path (new attempt, checkpoint re-verified by
     SHA-256, `Range` + `If-Range`, strict `206`), or starts afresh.
   - A user pause outranks restored space (COR-07 precedence): a paused job is not transferred, and resuming it only
     returns it to the waiting state.
   - Cancel is final.
7. **Finalization:**
   - If the copy fallback does not fit, or hits `ENOSPC` half-way, only the partial copy this operation created is
     removed.
   - The journal entry stays at `verified` (durable, as last committed) and the job waits with `disk_unavailable` while
     still `verifying`. The staging file is kept.
   - Retries and restarts reconcile it like any `verified` entry.
8. **aria2** (optional, no `DurableResume`) keeps the previous behaviour: a disk-full transfer fails explicitly. Resuming
   its control file without validation is not allowed (ADR 0003).
9. **Fault hooks** exist only in debug builds. `DOWNOVA_FAULT` is now a comma-separated list, adding `enospc_write` and
   `enospc_sync` (backend, once per process) and `no_hard_links` and `enospc_copy` (finalizer).

## Consequences

- A full disk costs at most the bytes written since the last durable sync, which are fetched again. It never costs the
  whole download.
- The margin (64 MiB) can block a download that would just fit. That is deliberate headroom for the database, the
  journal and other programs.
- The check frequency is bounded: at most one probe per sync and per waiting job every 2 s.
- If SQLite itself cannot commit while the disk is full, the last committed recovery state stays authoritative.
  A failed write is reported on stderr and never claimed durable. The next state change writes the row again, and
  journal stages are retried from their last committed stage (ADR 0008).
- Open:
  - removable-volume identity (the same drive returning; ADV05 advanced, B4);
  - per-volume reservations across concurrent jobs;
  - surfacing the blocker in the UI (B2).
