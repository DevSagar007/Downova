# ADR 0017 — Persistent queues, priority and manual order

- **Status:** Accepted
- **Date:** 2026-10-31
- **Builds on:** ADR 0010 (connection budget and admission)
- **Amends:** ADR 0010 §2 (admission order)
- **Requirements:** CORE-05, CORE-03, COR-07
- **Evidence:** [B2.8–B2.11 batch report](../reports/b2.8-b2.11-batch.md)

## Context

CORE-05 asks for persistent queues: create, rename, delete, start and stop, membership, priority,
manual order and per-queue limits. All of it must sit under the existing global limit of 3 active
files and the backend's 8-per-host budget (ADR 0010). A queue must never override a manual pause,
cancellation, disk hold or retry cool-down.

## Decision

1. **Model.** Queues are persistent rows (migration 6) with ID, name, list position, `max_active`
   (1–16) and `running`.
   - `main` always exists and cannot be deleted; clients show its name localized.
   - Every job has a `queue_id`, a `priority` (`low` | `normal` | `high`, stored as -1/0/1) and a
     manual `queue_position`.
   - Existing jobs migrate into `main` at normal priority, in acceptance order.
2. **Admission order** (`dm_domain::policy::admission_order`, pure):
   1. higher priority;
   2. the queue's position in the list;
   3. the manual position inside the queue;
   4. acceptance time;
   5. list order.

   **Fairness policy:** priority is global, so a high-priority job of a later queue starts
   before a normal one of the first. Ties never depend on random IDs (the ADR 0010 job-ID
   tie-break is replaced). A job starts only while both the global slots (3) and its queue's
   `max_active` allow. The backend's per-host budget is unchanged and still applies inside every
   attempt.
3. **Limits.**
   - Lowering a limit, or moving an active job to a full queue, never preempts a running
     transfer; it only delays new starts.
   - Capacity is always counted from live jobs (never stored), so moves and limit changes cannot
     corrupt accounting.
4. **Stop and Start queue** are policy state, not user intent.
   - Stop sets `running = false`: each running transfer of the queue is paused and checkpointed,
     and its unfinished jobs carry the `queue_stopped` blocker. `desired_state` is never touched.
   - Start clears the blocker. A job the user paused, a cancelled job, a disk hold or a retry
     cool-down stays as it is.
   - Both are persisted before they are acknowledged and are restored after a restart.
5. **Deletion never deletes jobs or files.**
   - `queues.v1.delete` refuses `main`.
   - A queue that still has jobs is refused unless `move_to` names another queue; its jobs then
     move there in their order.
6. **Order commands.**
   - `downloads.v1.move` (`up` | `down` | `top` | `bottom`) changes the manual position inside
     the job's queue.
   - `downloads.v1.set_queue` moves a job to the end of another queue.
   - `downloads.v1.set_priority` sets its priority.

   **Execution order is distinct from presentation:** in a queue view the desktop lists rows by
   `queue_position`, and clicking a column header only sorts the view.
7. **Clients.**
   - **Desktop:**
     - queues appear under Queues in the category panel, with a "(stopped)" marker in words;
     - Start/Stop queue (toolbar and Downloads menu) acts on the selected queue, else the selected
       download's queue, else `main`;
     - Downloads → Manage queues…;
     - File → Queue and priority…;
     - Move to top / up / down / bottom, also with Alt+↑/↓.
   - **CLI:**
     - `dm queues`, `queue-create`, `queue-start`, `queue-stop`, `queue-delete`;
     - `dm set-queue`, `priority`, `move`;
     - `dm add --queue`.

## Consequences

- A high-priority job can wait while lower ones run (no preemption); that is intended.
- Queue names are free text (1–64 printable characters) and are rendered as data.
- Not covered:
  - schedules;
  - per-queue speed limits;
  - drag-and-drop reordering;
  - queue-level history.
