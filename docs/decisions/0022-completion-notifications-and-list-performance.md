# ADR 0022 — Service-owned completion notifications, and list performance

- **Status:** Accepted
- **Date:** 2026-11-01
- **Requirements:** CORE-09, ADV12, ARCH-08, REL-04
- **Evidence:** [B2.21–B2.23 batch report](../reports/b2.21-b2.23-batch.md)

## Context

Users want to know when a long download finishes or fails, including when the window is hidden
to the tray or closed. The desktop window is optional: the per-user service keeps transferring
without it, so a notification sent from React would miss exactly the cases that matter. Keeping
a WebView alive only to deliver notifications would cost hundreds of MiB (see the measurements).

Measurements of the release build also showed that the download list rendered every record:
1,000 records produced 21,101 DOM nodes and search answers of about 0.3 s, and 10,000 records
left the window unresponsive for more than 150 s.

## Decision

### Notifications (CORE-09)

1. **The service decides; a platform adapter shows.**
   - The decision belongs to `dm-service` (`core::decide_notifications`, called in the poll loop).
   - `dm-platform::notify::SystemNotifier` shows it through `notify-rust` 4.18.2:
     - Windows: WinRT toasts with the installed app's AppUserModelID, `com.downova.desktop`;
     - macOS: user notifications posted as the app bundle;
     - Linux: D-Bus.
   - The adapter is behind `dm-platform`'s `notify` feature, so only the service links it.
   - `dm-domain` stays pure.
2. **What notifies.**
   - **Notifies:** only a durable terminal outcome:
     - `completed`, which is committed with its verification after placement (ADR 0008);
     - `failed` for good.
   - **Never notifies:**
     - accepted, transferring, 100 % transferred, verifying or finalizing (a job with a journal entry);
     - an automatic retry, because it is not terminal;
     - cancellation, which is the user's own action;
     - jobs removed from the list.
3. **Once per outcome, at most once.**
   - Migration 9 adds `jobs.notify_state` (`NULL`, `sent`, `skipped`).
   - `Store::decide_notification` records a decision only for a terminal job without one.
   - **Order:**
     1. The decision is committed under the coordinator lock.
     2. The lock is released.
     3. The notice is handed to the system on a blocking thread.
   - A crash between steps 1 and 3 loses that notification. It is never repeated. Exactly-once delivery
     to the operating system cannot be promised.
   - Replays, resubscriptions, reconnections and restarts never notify again.
   - An explicit Retry or Refresh-link restart clears the decision together with the finish time
     (`reopen_job`), so the new outcome is notified.
4. **Nothing historical.**
   - Migration 9 marks every job that finished before it as `skipped`.
   - At startup, any terminal job without a decision (for example, one failed as interrupted by the
     restore) is marked `skipped` before the first poll.
   - Outcomes that happened while notifications were off are `skipped`, not caught up.
5. **Settings and permission.**
   - `notifications.v1.get` and `set` take `{ enabled, locale }`. The setting is off by default and
     persisted in the store.
   - Turning it on is the explicit user action, and it shows one confirmation notification. On
     systems that ask for permission, the system asks at that moment.
   - A system that refuses a notification sets `last_delivery_failed`. Downova never retries and never
     asks again, and downloads are unaffected.
   - Without an adapter, `availability` is `unavailable` and turning notifications on is refused.
6. **Language and content.**
   - The service writes English or Bangla text in the language the window last set. The window
     re-sends the language when it changes while notifications are on.
   - The text contains the file's display name, cleaned of control and bidi characters. It never
     contains a URL, header or path.
   - Notifications have no actions. A click can at most activate the app; it never opens a file
     (SEC-03).
7. **Window states.**
   - **Visible:** the service notifies the same way in every window state; the window does not
     duplicate it, and its own polite announcement of finished downloads is unchanged.
   - **Hidden to the tray:** the same.
   - **Closed:** the same.

### List performance (ADV12, ARCH-08)

1. **A virtualized table.**
   - Only the rows in view are rendered, plus 8 rows of overscan on each side and the selected and
     focused rows (pinned).
   - Spacer rows (`aria-hidden`) stand for the rest. The table states `aria-rowcount`, and each row
     its `aria-rowindex`.
   - The row height is measured at runtime, which covers zoom and fonts.
   - Keyboard navigation covers arrows, Page Up/Down, Home and End. It scrolls the target into view
     and moves focus once it is rendered.
   - The implementation is dependency-free (`DownloadTable.tsx`).
2. **Memoized work.**
   - Rows are memoized by snapshot identity.
   - The presentation sort is recomputed only when a sorted field changes. Status sorts without its
     percentage, so progress alone never re-sorts.
   - Category, search and queue order are memoized in `App`.
3. **Event batching in the window.**
   - Events are kept per job (the latest snapshot, or the removal) and applied in one indexed pass.
     The batch is therefore bounded by the number of jobs, never by hidden time.
   - Batches are flushed within 50 ms while visible, inside the service's 400 ms progress
     coalescing. While hidden they are flushed at most once per second, and at once on becoming
     visible.
   - Mutation acknowledgements flush queued events first, so an older event never overwrites one.
   - Gaps, new instances and `snapshot_required` still take a fresh authoritative snapshot. Removed
     jobs still never come back.

## Consequences

- Notifications work with the window hidden or closed, at the cost of one small dependency in the
  service process; its resource cost is part of the measurements.
- Whether the operating system actually displayed a notification is not observable by Downova.
  Display is a human check per OS. On Windows it needs an *installed* app (a Start-menu shortcut
  with the AppUserModelID); extracted or development builds show nothing.
- A crash can lose one notification; it is never duplicated.
- The list's DOM no longer grows with history; search and sorting still cover the whole list.
