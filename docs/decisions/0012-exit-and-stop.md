# ADR 0012 — Explicit "Exit and stop downloads" and desktop close behaviour

- **Status:** Accepted
- **Date:** 2026-10-27
- **Builds on:** ADR 0001 (processes), ADR 0007 (checkpoints), ADR 0008 (completion journal), ADR 0010 (admission)
- **Requirements:** UI-04, ARCH-09 (the UI-close case), CORE-04
- **Evidence:** [B2.2 report](../reports/b2.2-close-and-exit.md)

## Context

UI-04 asks for a setting that decides what closing the window does, and for a tray offering "Open" and "Exit and stop
downloads". The UI must never stop the service implicitly.

The only shutdown before B2.2 was `service.v1.shutdown` with `{}` (and Ctrl+C). It answered with `{}` and then ran
`Core::prepare_shutdown`, which stops and checkpoints transfers but keeps `desired_state = running`. After the next
start those downloads continue by themselves. That is right for a restart or an OS logout, but not for a user who
asked to *stop* downloads.

Two more defects were found:
- the server stopped whenever the method name was `service.v1.shutdown`, even if the request was then refused (for
  example, with malformed params);
- serde accepted a JSON array as struct params, so `params: []` passed as `{}`.

## Decision

1. **Shutdown modes (contract).**
   - `service.v1.shutdown` takes `ShutdownParams { mode?: "keep_state" | "pause_downloads" }`. Unknown fields and
     modes are rejected.
   - `{}` (and Ctrl+C) means `keep_state`, the previous behaviour, so existing callers are unchanged.
   - The answer is `ShutdownResult { mode, paused_jobs }` instead of `{}`.
2. **Only an accepted request stops the service.** The server exits only when dispatch succeeded. Params must be a JSON
   object for every method.
3. **Stopping state.** The coordinator sets a stopping flag under its lock, before anything else. From then on:
   - admission starts nothing;
   - `add` of a new download and `resume` are refused with `SERVICE_UNAVAILABLE` ("the download service is
     stopping");
   - a replay of an accepted add (same key) still returns its job, and reads still work.
4. **`pause_downloads`, before the acknowledgement.** For every unfinished job whose desired state is `running` and
   that is not in finalization:
   - set `desired_state = paused` (a manual pause, distinct from policy blockers);
   - stop its attempt (the backend flushes and syncs) and commit its checkpoint;
   - persist the row.

   It applies equally to queued, admission-waiting, retry-waiting and disk-blocked jobs.
   - Jobs already paused, completed, failed or cancelled are untouched.
   - Jobs in finalization finish or are reconciled by the journal as before.
   - Repeating the request pauses nothing more.
5. **Recovery.**
   - Paused jobs with a checkpoint are restored paused (as since ADR 0007).
   - A paused job without an attempt (never started, or waiting for a retry) waits again, paused, instead of being
     reported as interrupted. It holds no partial data.
   - A job that was transferring without a usable checkpoint (no strong validator, nothing confirmed yet) still fails
     explicitly as interrupted. No data is invented.
6. **The acknowledgement is not termination.** The desktop sends `pause_downloads` only after the user confirmed in
   the one dialog that both the tray item and the Tasks menu open. Its result has three cases:
   - it waits (≤ 20 s) until the service endpoint stops answering, then reports `stopped` and quits;
   - if the service acknowledged but did not exit in time, the desktop says so and stays open;
   - a refused, unreachable or unacknowledged request stops nothing, and the dialog says which.

   The desktop never force-kills the service.
7. **Close behaviour is a desktop setting.**
   - `<data>/desktop-settings.json` holds `close_behaviour`, which is `hide_to_tray` (default) or `close_window`. It
     is persisted by the shell, which decides on the close request before the UI is asked.
   - Neither value stops the service.
   - Without a tray (creation failed), hiding becomes minimizing, so the window stays reachable.
   - A second launch shows and focuses the existing window (Tauri single-instance plugin; on Linux it needs a D-Bus
     session bus).

## Consequences

- **Wire change:** additive. Old clients sending `{}` get `keep_state` and a richer acknowledgement they can ignore.
  `params: []` is now refused for every method, matching the schema.
- **After Ctrl+C:** the service also stops admitting new transfers before it checkpoints and exits.
- **Persistence:** "Exit and stop downloads" survives a restart as paused jobs, which the user resumes explicitly. It
  is a process-level guarantee (bytes synced, checkpoint committed, WAL `synchronous=FULL`), not a power-loss claim.
- **Not covered:**
  - OS logout, sleep and reboot (the rest of ARCH-09);
  - closing the browser and the native-host lifecycle;
  - tray icons on Linux desktops without a StatusNotifier host (the menu still works where the icon is shown; without
    a visible icon, a second launch reopens the window).
