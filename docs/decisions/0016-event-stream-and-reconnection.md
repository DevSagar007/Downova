# ADR 0016 — Service event stream and attach-only reconnection

- **Status:** Accepted
- **Date:** 2026-10-31
- **Builds on:** ADR 0002 (backend seam), ADR 0006 (storage), ADR 0012 (Exit and stop downloads), ADR 0013 (remove from list)
- **Requirements:** ARCH-08, ARCH-09, UI-04
- **Evidence:** [B2.8–B2.11 batch report](../reports/b2.8-b2.11-batch.md)

## Context

Until B2.8 the desktop polled `downloads.v1.list` every second. ARCH-08 asks for streamed updates:
- an initial snapshot and a sequence watermark;
- 250–500 ms progress coalescing;
- bounded subscribers that never block transfers.

ARCH-09 asks for recovery when the service restarts while the UI is open, without replaying
uncertain mutations and without "Exit and stop downloads" starting the service again.

## Decision

1. **Transport.** `events.v1.subscribe` runs on the existing private IPC connection (one
   subscription per connection). The answer is a `SubscribeResult` with:
   - `service_instance_id`;
   - `sequence`;
   - every listed job.

   `events.v1.event` notifications follow on the same connection, with no `id`. The server writes
   every line whole, so answers and notifications never interleave inside a line.
2. **Race-free start, one sequence.**
   - The hub lives in the coordinator state: subscribing and publishing are serialized with every
     job change.
   - Each event carries the next sequence after the subscriber's watermark (decimal strings,
     exact beyond 2⁵³), plus the instance ID and the job and attempt IDs where they apply.
3. **Diff-based coalescing.** Each poll (400 ms) compares every listed job with the last snapshot
   published:
   - new → `job_added`;
   - any change except the byte count → `job_changed`;
   - byte count only → `progress`;
   - left the list → `job_removed`.

   Mutations publish when they finish. Unchanged jobs produce nothing, so normal coalescing never
   skips a sequence number.
4. **Bounded subscribers.** Each subscriber has a buffer of 1024 events and delivery is
   `try_send`. On overflow:
   - the subscriber is dropped;
   - its connection sends `snapshot_required` and closes.

   A slow or vanished client never delays a transfer or a coordinator mutation.
5. **Client rule.** The desktop applies an event only when both hold:
   - `service_instance_id` equals the subscription's;
   - `sequence` is exactly the previous one + 1.

   On a gap, a new instance, `snapshot_required` or the end of the stream, it subscribes again
   and replaces its list with the fresh snapshot.

   The shell tags forwarded events with a subscription number; the client buffers events that
   arrive before it knows its number and ignores other numbers. A job seen removed is never
   re-added by a late update.
6. **Explicit fallback.** Polling the list remains only when the service refuses
   `events.v1.subscribe` (an older service: `rejected`). It is a separate, tested mode, not a
   silent default.
7. **Attach-only reconnection.**
   - After a lost connection the desktop probes for a running service
     (`service_probe`: `hello` only, never starting it) after 0.5, 1, 2, 4, 8 and 8 s. Then it
     stops and offers **Reconnect**, the only path that may start the service, and only on the
     user's request.
   - A successful probe re-attaches with a fresh subscription. Mutations whose answer was lost
     stay "uncertain" and are never replayed automatically. A retried add reuses its idempotency
     key, so a lost acknowledgement still yields one job.
8. **Exit.**
   - "Exit and stop downloads" first ends the stream and disables reconnection in the window.
     The shell's `service_connect` also only probes once exiting began.
   - If the service was not stopped (refused, uncertain or still running), the window
     re-attaches only, never starting the service.
9. **Shutdown and wake (service).**
   - A graceful stop checkpoints and stops transfers within `SHUTDOWN_CHECKPOINT_BOUND` (10 s),
     below the desktop's 20 s wait.
   - A wall-clock jump of more than 30 s past the poll interval is treated as a wake:
     - disk and drive holds are re-checked;
     - network retry waits become due;
     - server cool-downs, user pauses and stopped queues are unchanged.

## Consequences

- Live updates arrive within about 400 ms, without full-list polling.
- A busy UI that cannot keep up gets a fresh snapshot, never a partial view.
- Logout, sleep and reboot are documented in [architecture §lifecycle](../architecture.md):
  - The service is per-user and ends with the session (logout, reboot).
  - It does not start at login (no autostart in this batch). Opening the application starts it,
    and unfinished downloads continue from their checkpoints; manually paused ones stay paused.
  - Sleep is handled as a wake re-evaluation.
- Simulated wake (clock jump) and process kills are tested. Real OS sleep, logout and reboot are
  human tests and are recorded as not run.
