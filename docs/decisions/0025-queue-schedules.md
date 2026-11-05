# ADR 0025 — Queue schedules (time windows)

- **Status:** **Accepted for the B3.1 feature branch** (2026-11-05, implemented on
  `feature/b3.1-speed-limits-schedules` under the narrow sequencing exception recorded in the roadmap; B2 is still not
  exited). Corrections found while implementing are in "Implementation notes" at the end.
- **Date:** 2026-11-04
- **Requirements:** CORE-08 (P0 · B3), ARCH-09 (wake and reconnect), CORE-05 (queues), CORE-06 (limits)
- **Builds on:** [ADR 0017](0017-queues-and-priority.md) (queues; stop/start is policy, not intent),
  [ADR 0012](0012-exit-and-stop.md) (manual pause is durable), [ADR 0023](0023-configurable-concurrency-and-request-rotation.md)
  (admission limits)

## Problem

CORE-08: "A schedule (timezone, window, eligible queue) adds a `schedule_window` blocker outside the window. Missed
runs follow the configured policy. The UI states that schedules do not run while the computer is off or asleep."

What exists:
- the blocker `Blocker::ScheduleWindow` (`crates/domain/src/state.rs`), rank 1 (with offline and stopped queue),
  never set;
- `decide_run`, where a manual pause and a cancellation always win over any policy blocker;
- `on_wake` (B2.10), which re-evaluates waiting work after a wall-clock jump;
- no time-zone dependency: `chrono`, `jiff` and `time` appear in `Cargo.lock` only transitively.

## Decision

### 1. Model: one optional schedule per queue

```json
{ "enabled": true,
  "timezone": "Asia/Dhaka",
  "windows": [ { "days": ["mon","tue","wed","thu","fri"], "start": "01:00", "end": "07:00" },
               { "days": ["sat","sun"], "start": "22:00", "end": "08:00" } ],
  "missed_window": "skip" }
```

- **`timezone`:** an IANA name, stored explicitly. The UI preselects the system zone (read once when the user
  creates the schedule). A later change of the OS zone does **not** silently move the schedule; the UI shows both
  zones when they differ.
- **`windows`:** 1–14 windows per schedule.
  - `start` and `end` are local `HH:MM` on a 1-minute grid.
  - **`end` < `start` crosses midnight** and belongs to the day it starts on: the Saturday window above ends on
    Sunday at 08:00.
  - `start` = `end` is refused (ambiguous); a whole day is `00:00`–`23:59` plus the next day's window, or no
    schedule.
  - Overlapping windows merge.
- **`enabled: false`** keeps the schedule stored but ignores it: no blocker.
- **The schedule belongs to the queue.** Its jobs follow it; moving a job to another queue applies that queue's
  schedule at once.

### 2. Evaluation (pure, deterministic)

- **Where:** `dm-domain::schedule`, no I/O:

  ```text
  evaluate(schedule, now: Timestamp, tz: &TimeZone) -> { in_window: bool, next_change: Option<Timestamp> }
  ```

  The service resolves the zone (`jiff::tz::TimeZone`). Tests pass fixed instants and zones.
- **DST gap** (local times that do not exist): a boundary inside the gap moves **forward to the first instant after
  the gap**. A 02:30 start on a spring-forward night starts at 03:00 local.
- **DST overlap** (a repeated hour):
  - a **start** resolves to the **earlier** occurrence and an **end** to the **later** one, so a window is never
    shorter than its wall-clock length;
  - a window entirely inside the repeated hour runs once, from its earlier start to its later end.
- **Times:** next-change instants are UTC timestamps in the contract; the UI renders them in the schedule's zone and
  states that zone.

### 3. Effect on jobs (policy only, never intent)

- **When the blocker is set:** for each non-terminal job whose queue has an enabled schedule, `schedule_window` is
  set when `in_window` is false and cleared when it is true. Through `decide_run`:
  - **a manual pause stays a pause:** a window opening never resumes it;
  - **a stopped queue stays stopped:** `queue_stopped` and `schedule_window` are separate blockers, and both must
    clear;
  - **cancelled or terminal jobs never revive;**
  - **inside a window the global and queue limits apply as usual** (ADR 0023): a window only makes jobs eligible.
- **A window closing on running work:**
  - a **resumable** transfer is held exactly like a queue stop: checkpoint, sync, release the permits; it resumes
    from its checkpoint in the next window (ADR 0017, 0023);
  - a **non-resumable** transfer (no range support or no strong validator) is **allowed to finish**, because stopping
    it would discard its progress. The UI says so ("finishing outside the window: this server cannot resume").
- **Order:** the schedule is evaluated **before admission**. A job outside its window is never started and then
  stopped.

### 4. Timers, clock changes, wake

- **Timer:** the service keeps one timer for the earliest `next_change` over all schedules, **bounded to at most 60
  s**. Every expiry recomputes from the wall clock, so a clock change of any size is caught within 60 s.
- **Wake and clock jumps:** `on_wake` (and a jump detected by the existing poll) re-evaluates schedules immediately,
  before any other admission.
- **Locks:** evaluation runs under the coordinator lock briefly. No storage write or wait happens while the lock is
  held, beyond the ordinary state save.

### 5. Missed windows

- **What counts as missed:** a window that **started and ended while the service was not running** (the computer was
  off or asleep, or the user exited). The service records `last_evaluated_at` durably on every evaluation and at
  shutdown.
- **`missed_window: "skip"`** (default): nothing special; work waits for the next window.
- **`missed_window: "catch_up"`:** at start or wake, if a window was missed, the service opens **one catch-up
  window**:
  - **it runs from now for the missed window's length, capped at 6 hours**;
  - **only the most recent missed window counts**, and only if it ended within the last 24 hours;
  - **it is recorded durably** (`catch_up_for` = the missed window's start instant) **before it takes effect**, so a
    restart during or after it never opens a second one for the same missed window;
  - it is a schedule window like any other: the same limits apply, and it never resumes manual pauses or stopped
    queues;
  - **"Run now" means exactly this:** jobs that were waiting only for the schedule become eligible, in queue order,
    within the limits.

### 6. Durability and failures

- **Storage:**
  - the schedule is a JSON column on queues (`schedule_json`, nullable);
  - plus `schedule_last_evaluated_at` and `schedule_catch_up_for`;
  - one migration, the next free number at implementation time (after ADR 0024's).
- **Validate and persist, then apply:** a refused write changes neither the stored schedule nor any blocker. The
  same trigger-based test as concurrency (B2.31).
- **Restore before recovery:** schedules load before recovered jobs are admitted, so a restart outside a window
  never starts transfers.

### 7. Surfaces

| Layer | Change |
|---|---|
| Contract (schema → TS, `api.rs`) | `queues.v1.set_schedule` `{ queue_id, schedule: Schedule \| null }`; `QueueInfo` gains `schedule` and `schedule_state { in_window, next_change_at, catch_up_until \| null }`; examples; the closed method set |
| `crates/domain` | `schedule` module (pure evaluation, validation, DST rules) |
| `crates/storage` | the migration; load and save with reopen tests |
| `crates/service` | the blocker, timer, wake, catch-up, persistence, restore order |
| `crates/cli` | `dm queue-schedule <QUEUE-ID> --window mon-fri@01:00-07:00 … [--timezone Z] [--missed skip\|catch_up]`, `dm queue-schedule <QUEUE-ID> off`, `dm queue-schedule <QUEUE-ID> --disable` |
| `apps/desktop` | Manage queues → Schedule: enable; time zone (preselected, with a note when the system zone differs); windows per day; missed-window policy; "next window: …" in the schedule's zone; per job "Waiting for the queue's schedule (next window …)"; the fixed sentence "Downloads cannot run while the computer is off or asleep." All strings in English and Bangla |
| Dependency | `jiff` (MIT / Unlicense) as a direct dependency of `dm-service`, with its bundled tz database on Windows (`jiff-tzdb`) and the system database on macOS and Linux. Licence and advisory entry under REL-03 |

## Verification

- **Pure, with fixed instants:**
  - boundaries (inclusive start, exclusive end); overnight windows; merged overlaps;
  - DST gap and overlap in `America/New_York` and `Europe/London`, and a zone without DST (`Asia/Dhaka`);
  - next-change computation across week ends.
- **Service, with an injected clock:**
  - the blocker set and cleared at boundaries;
  - a paused job stays paused when the window opens;
  - a stopped queue stays stopped;
  - limits are respected inside the window;
  - a resumable transfer is held at the end and resumes byte-exact; a non-resumable one finishes;
  - a clock jump forward and back is detected within 60 s; wake re-evaluation;
  - catch-up opens once, survives a restart without duplicating, and respects its cap and its 24 h limit;
  - a refused write changes nothing.
- **Integration:** a scheduled queue under a CORE-06 global limit and a queue limit of 6. Inside the window it obeys
  both; outside, its jobs show `schedule_window` with the next window; after a restart the same.
- **UI:** desktop tests in both languages; keyboard access; the off/asleep sentence.
- **Packaged acceptance:** a scheduled queue in the packaged app, with the window opening a few minutes ahead.

## Consequences

- **Gains:** schedules are pure policy, consistent with pauses, queue stops and limits. They are deterministic to
  test and robust to sleep and clock changes.
- **Costs:** a new direct dependency (time zones); a migration; a 60 s timer.
- **Rejected alternatives:**
  - OS task schedulers: per-OS, and they would need privileges;
  - waking the computer: not portable, and it is not the app's place;
  - local-time-only schedules without a zone: ambiguous under DST and travel.

## Implementation notes (B3.1, 2026-11-05)

- **"Resumable" at the window's end (correction):** the service cannot know before a transfer runs whether the server
  supports ranges with a strong validator. It decides from what it has: a running job with a **durable checkpoint**
  (`durable > 0` bytes) is held like a queue stop (paused with a checkpoint, `schedule_window` set); a job with
  no durable checkpoint is **allowed to finish** and is re-checked at every evaluation (it is held as soon as it has
  one). The UI text "finishing outside the window: this server cannot resume" is **not built**; the job simply shows
  no `schedule_window` blocker while it runs on.
- **Unknown zone at restore:** `queues.v1.set_schedule` refuses a zone the service's tz database does not know
  (`INVALID_REQUEST`, nothing saved). If a stored zone later becomes unknown (a tz database change), the service
  evaluates it as UTC and logs that, rather than dropping the schedule or blocking forever.
- **Storage:** migration 11 adds `schedule_json`, `schedule_evaluated_ms`, `catch_up_for_ms`, `catch_up_from_ms`
  and `catch_up_until_ms` to `queues`. Every edit or removal clears the catch-up and resets the evaluation time,
  so an edit never turns a past occurrence into a "missed" one. The evaluation time is written at most every 5
  minutes and on catch-up.
- **Timer:** the existing poll loop re-evaluates every poll (at most 60 s apart) and `on_wake` forces it; there is
  no separate timer task.
- **Catch-up window:** from now for the missed window's length capped at 6 h, only for the most recent window that
  ended within 24 h; stored with its absolute `from`/`until` before it applies, so a restart neither repeats nor
  extends it, and a clock set back before its start ends it.
- **Queue deletion and moves:** a moved job (`set_job_queue`, or a deleted queue's jobs) takes the destination
  queue's stop state and schedule at once.
- **Dependency:** `jiff` 0.2 (Unlicense OR MIT), workspace dependency with `default-features = false`; `dm-service`
  enables `tz-system` plus the bundled database (`tzdb-bundle-platform`, which bundles only where no system
  database exists, i.e. Windows), `tzdb-zoneinfo` and `tzdb-concatenated`. It was already in `Cargo.lock`.
- **CLI:** positional rather than the option form above: `dm queue-schedule <QUEUE-ID> <ZONE> <DAYS@HH:MM-HH:MM>...
  [skip|catch-up] [disabled]` and `dm queue-schedule <QUEUE-ID> off`; `dm queues` prints the schedule and its
  state.
- **UI:** Manage queues → Schedule per queue (enable, IANA zone preselected from the system, windows with days and
  times, overnight note, missed-window policy, the service's state with the next change shown in the schedule's zone,
  the off/asleep sentence). The per-job waiting reason is the existing `schedule_window` blocker label ("schedule");
  the next window is shown in Manage queues, not per job.
- **Evidence:** see [the B3.1 report](../reports/b3.1-speed-limits-schedules.md).
