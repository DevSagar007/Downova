# ADR 0006 — SQLite through `rusqlite` (bundled), owned exclusively by `dm-service`

- **Status:** Accepted
- **Date:** 2026-10-25
- **Implements:** ADR 0001 decision 4 (SQLite holds metadata, single writer); docs/architecture.md §7
- **Requirements:** ARCH-05, COR-09, CORE-04 (first step)

## Context

Until B1.1 every job and idempotency record lived in the service's memory. A lost acknowledgement followed by a
service restart could therefore create a second job for the same browser action (COR-09), and a restart lost the job
list entirely. ADR 0001 already chose SQLite with the service as the only writer, but the access library, the locking
model, the migration scheme and the restart semantics were open.

## Decision

1. **A new crate, `dm-storage`, uses `rusqlite` 0.41 with the `bundled` feature.**
   - The SQLite library is compiled into the binary, so behaviour does not depend on the system's SQLite version on
     Windows, macOS or Linux.
   - `dm-storage` depends only on `dm-domain`. `dm-service` is its only user (`docs/project-structure.md`).
2. **The database is `<data>/downova.sqlite3`.** It is opened only after the service holds its single-instance lock, with:
   - `locking_mode=EXCLUSIVE`, with a lock taken at open: no other process can read or write the file while the service
     runs, so the CLI, UI and browser host read through the service API;
   - `journal_mode=WAL` and `synchronous=FULL`;
   - `foreign_keys=ON` and a zero busy timeout: a second opener fails at once with "in use by another process".
3. **Schema version 1:**
   - `jobs`: job state, last confirmed byte count, error;
   - `idempotency_records`: key, the SHA-256 of the request fingerprint, job, created time;
   - there is no segment or byte map; resume state belongs to the engine (ARCH-05).
4. **Migrations:**
   - They are forward-only SQL files tracked in `PRAGMA user_version`, all applied in one `IMMEDIATE` transaction, and
     a failure rolls back completely.
   - A database whose version is newer than the build understands is refused and left untouched.
5. **Write points.** A job and its idempotency record are inserted in one transaction **before** `downloads.v1.add` is
   acknowledged; if that write fails, nothing is started and the client gets `INTERNAL` with `retry`. Pause, resume,
   cancel, failure and completion are written when they happen, and byte progress is never written per packet.
6. **Restart semantics (B1.1):**
   - Jobs and idempotency records are restored, so the same key returns the same job and a different payload conflicts.
   - A job that was unfinished is restored as `failed` with an explicit reason ("interrupted when the Downova service
     stopped…"), because the engine's resume checkpoint is not persisted yet. Continuing it is B1.2 (CORE-04).
   - Idempotency records older than 30 days are pruned at startup; jobs are kept.
7. **Secrets:**
   - `jobs.url` keeps the raw URL, because later recovery needs it. The file is in the per-user private data directory,
     `JobRow`'s `Debug` redacts the URL, and storage errors never include row contents.
   - The fingerprint, which contains the URL, is stored only as a SHA-256.

## Consequences

- COR-09 holds across service restarts and hard kills, tested with the real `dm-service` process.
- The writes are small synchronous calls made under the coordinator lock. That is acceptable at state-change frequency;
  a dedicated writer task is a later optimisation if measurements call for it.
- Exclusive locking means diagnostic tools cannot read the database while the service runs. Diagnostics go through the
  service (SEC-03) or need the service stopped.
- Licences: SQLite is public domain; `rusqlite` and `libsqlite3-sys` are MIT. Both are added to the pending dependency
  review (REL-03).
- Open: `attempts`, `resume_checkpoints`, `finalization_journal` and the other §7 tables; reconciliation of partial
  files against engine checkpoints (B1.2+); a benchmark of `synchronous=FULL` (COR-08).
