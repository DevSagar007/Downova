# Desktop Design System

This is the canonical design specification for the desktop UI and the browser-extension surfaces. Tokens live in
[apps/desktop/src/ui/tokens.css](apps/desktop/src/ui/tokens.css), and primitives live in `apps/desktop/src/ui/`.
Before adding a new visual value or component, check whether an existing token or component fits. If one
doesn't, add the token or component here and in code within the same change.

**Character:** a classic desktop download manager. It uses the familiar menu bar, big-icon toolbar, categories and columned list layout that IDM-style users know **(user decision, 2026-10-21)**, kept calm and professional. Dense but readable. Status comes before decoration.
No marketing visuals, gradients or celebratory animation. The UI reports what the service says and never
implies work that is not happening.

Reversible defaults chosen without explicit user preference are marked **(default)**.

## 1. Layout and navigation

The window follows the classic download-manager layout users already know from tools such as IDM. Only the
**layout and workflow pattern** is reused: icons come from Lucide, colours from our tokens, and no
third-party logo, icon artwork or product name is copied.

```text
┌───────────────────────────────────────────────────────────────────────┐
│ Tasks  File  Downloads  View  Help                         (menu bar) │
├───────────────────────────────────────────────────────────────────────┤
│  +       ▶       ⏸      ⊗    │   🗑       ☑      │  ⚙      ⏰     ▷     □   │
│ Add    Resume  Stop  Stop all│ Delete  Delete    │ Options Sched. Start Stop│
│ URL                          │         completed │                queue queue│
├──────────────┬────────────────────────────────────────────────────────┤
│ Categories   │ File name │ Size │ Status │ Time left │ Rate │ Last try │ Descr. │
│ ▾ All downl. │────────────────────────────────────────────────────────│
│    Compressed│ rows… (or an honest empty-state row)                   │
│    Documents │                                                        │
│    Music     │                                                        │
│    Programs  │                                                        │
│    Video     │                                                        │
│   Unfinished │                                                        │
│   Finished   │                                                        │
│ ▾ Queues     │                                                        │
│    Main queue│                                                        │
├──────────────┴────────────────────────────────────────────────────────┤
│ ● Download service not connected                          0 downloads │
└───────────────────────────────────────────────────────────────────────┘
```

- **Menu bar** (`ui/MenuBar`): Tasks, File, Downloads, View and Help. Items that need the service stay visible and say why they are unavailable. View holds the category-panel toggle, theme and language. Help → About.
- **Toolbar** (`features/downloads/DownloadsToolbar`): large icon-over-label commands grouped by separators. These are transfer (Add URL, Resume, Stop, Stop all), cleanup (Delete, Delete completed) and organisation (Options, Scheduler, Start/Stop queue). Commands that are not available are faded, never hidden.
- **Categories panel** (`features/downloads/CategoryTree`): the file-type categories under All downloads, then Unfinished, Finished and Queues. The selection is `aria-current`, and the table caption names the category.
- **Download list** (`features/downloads/DownloadTable`): see §5.
- **Status bar:** the service connection state (a dot plus text) at the inline start and the item count at the inline end.
- **Options** is a modal dialog, not a page (`features/settings/OptionsDialog`).
- **Information hierarchy:** commands (menu, toolbar) → where you are (category) → what is happening (list) → overall state (status bar).

## 2. Typography

- **Font stack** (`--font-sans`): the system UI font first (Segoe UI Variable, San Francisco, system-ui), then Bangla-capable fallbacks: Nirmala UI (Windows), Noto Sans Bengali, and Kohinoor or Bangla Sangam (macOS). No web fonts are bundled **(default)**.
- **Scale:** xs 12 / sm 13 / md 14 (base) / lg 16 / xl 20 px. Weights are 400, 500 and 600 only.
- **Line height:** 1.5 by default, and `--leading-bangla` (1.7) under `:lang(bn)`, because conjuncts and vowel signs need extra vertical room.
- **Numbers** (speed, size, ETA) use `font-variant-numeric: tabular-nums` so values don't jitter.
- **Monospace** (`--font-mono`) is used only for URLs, hashes and paths in detail panels.

## 3. Tokens

All values are CSS custom properties in `tokens.css`. Components must not use raw colours, spacing,
radii or font sizes.

| Group | Tokens | Rule |
|---|---|---|
| Spacing | `--space-0…10` (4px grid) | Padding, gaps and margins only from this scale |
| Radius | `--radius-sm` 4, `--radius-md` 6 (controls), `--radius-lg` 8 (panels), `--radius-pill` | No other radii |
| Border | `--border-width`, `--color-border`, `--color-border-strong` | 1px hairlines; strong for inputs |
| Surfaces | `--color-bg`, `--color-surface`, `--color-surface-sunken`, `--color-surface-hover`, `--color-surface-selected` | Background → panel → hover → selected |
| Text | `--color-text`, `--color-text-muted`, `--color-text-disabled` | Muted for metadata; disabled only for unavailable controls |
| Accent | `--color-accent`, `--color-accent-hover`, `--color-accent-text`, `--color-focus` | One accent (blue) **(default)**; primary actions and focus |
| Status | `--color-status-{info,success,warning,danger,neutral}` + `-bg` | Paired with an icon and text, never alone |
| Progress | `--color-progress-track`, `--color-progress-fill`, `--color-progress-fill-paused`, `--progress-height` | The bar in the Status cell (B2.3); paused and waiting rows use the paused fill |
| Sizing | `--control-height-sm/md`, `--icon-sm/md/lg/xl`, `--row-height`, `--sidebar-width(-collapsed)` | |
| Motion | `--duration-fast/normal`, `--easing-standard` | Zeroed under `prefers-reduced-motion` |
| Elevation | `--shadow-raised`, `--shadow-overlay`, `--color-backdrop` | Overlays (menus, dialogs) only |
| Classic chrome | `--color-chrome`, `--color-chrome-border`, `--color-table-header`, `--color-grid`, `--color-row-selected(-text)` | Menu bar, toolbar, panel headers, list grid lines, selection |
| Toolbar tones | `--color-icon-{add,resume,stop,delete,options,schedule,queue}`, `--icon-toolbar`, `--toolbar-button-width`, `--opacity-unavailable` | Toolbar icon colour per command group |
| Window layout | `--menubar-height`, `--category-panel-width(-compact)`, `--table-row-height`, `--table-min-width`, `--search-max-width`, `--statusbar-height`, `--menu-min-width`, `--dialog-width` | Main window regions and the compact history-search field |

**Icons:** Lucide (`lucide-react`, ISC license), outline style, 16px (`--icon-md`) in controls and 32px in empty states. Icons are decorative (`aria-hidden`) unless they are the only content of a control, in which case the control has an `aria-label`.

## 4. Components

| Component | File | Status | Rules |
|---|---|---|---|
| MenuBar | `ui/MenuBar.tsx` | exists | `role="menubar"`. ←/→ move between menus, ↓ opens, ↑/↓ move within a menu, Esc closes and returns focus. Items are `menuitem`, `menuitemradio` (theme, language) or `menuitemcheckbox` (toggles). Unavailable items use `aria-disabled` plus a tooltip reason |
| ToolbarButton | `ui/ToolbarButton.tsx` | exists | Icon (`--icon-toolbar`) over label. A tone sets the icon colour (`--color-icon-*`). An unavailable button stays focusable with `aria-disabled` and a described reason, and its icon fades to `--opacity-unavailable`. The toolbar supports ←/→ |
| Dialog | `ui/Dialog.tsx` | exists | Modal, `aria-modal`, labelled by its title. It focuses the element marked `data-autofocus` (a dialog's main field), otherwise the first control; it traps Tab, closes on Esc, and returns focus to the opener. Chrome-coloured header and footer |
| Button | `ui/Button.tsx` | exists | Variants: `primary` (one per dialog or view), `secondary`, `ghost`. Unavailable actions use `unavailableReason` |
| Banner | `ui/Banner.tsx` | exists | Inline tone message. Main window (B2.1): a warning above the list when the service is lost or incompatible (with Reconnect), and the selected download's error or the last failed Stop/Resume below it |
| EmptyState | `ui/EmptyState.tsx` | exists (unused in the main window) | Large empty-state panels outside the table |
| CategoryTree | `features/downloads/CategoryTree.tsx` | exists | Nested list. Parents have an expand/collapse button (`aria-expanded`) |
| DownloadSearch | `features/downloads/DownloadSearch.tsx` | exists (B2.7) | Compact labelled field above the list (`role="search"`, `type="search"`), icon, own clear button, and a muted match count that is `aria-hidden` (a polite live region announces one throttled sentence). Ctrl/Cmd+F focuses and selects the field; Esc clears it while focused. Width is capped by `--search-max-width`. Search matches the display name or source hostname only; it is local presentation state and never a service query |
| DownloadTable | `features/downloads/DownloadTable.tsx` | exists | Native `<table>` with a caption, sticky header and fixed column widths. See §5 |
| AddUrlDialog | `features/downloads/AddUrlDialog.tsx` | exists (B2.1) | One labelled URL field (focused on open), inline validation with `aria-invalid` and an `alert`, one primary Add button that shows "Adding…" and is unavailable while a request is pending |
| ExitDialog | `features/lifecycle/ExitDialog.tsx` | exists (B2.2) | The one confirmation for "Exit and stop downloads" (Tasks menu and tray). Cancel changes nothing; the primary button sends one request and shows "Stopping…" until the answer; the result is shown as a Banner: verified stopped (the app then closes), acknowledged but still running, or not stopped (unreachable / unconfirmed / refused) |
| DownloadDetails | `features/downloads/DownloadDetails.tsx` | exists (B2.1) | Banner below the list: the selected download's error summary, action and follow-up (CORE-11 copy), or why the last Stop/Resume did not complete |
| Status cell | `DownloadTable` `StatusCell` (B2.3) | exists | Lucide icon (`aria-hidden`) + text, tone from `--color-status-*`: Complete `CircleCheck` success, Error `CircleX` danger, Cancelled `Ban`, Paused `CirclePause`, Waiting `Hourglass` warning, Queued `Clock`, Connecting `PlugZap`, Downloading `ArrowDownToLine`, Verifying `ShieldCheck`, Scanning `ScanSearch`, Finishing `FileCheck` (info). A native `<progress>` (0–1000, labelled "Progress of {name}", `aria-valuetext` = percent) only while unfinished with a known non-zero total; no bar for unknown totals. Its width transition uses `--duration-normal` (0 with reduced motion) |
| ConfirmCommandDialog | `features/downloads/ConfirmCommandDialog.tsx` (B2.3) | exists | Confirmation for Cancel download and Remove from list: names the file, says what happens to files, "Keep" sends nothing, the primary sends one request and shows a pending label, failures use the error catalogue |
| ClearFinishedDialog | `features/downloads/ClearFinishedDialog.tsx` (B2.7) | exists | One confirmation for File → Clear finished…. It names the frozen count, says files are kept and unfinished/paused jobs are untouched, and that removed rows stay out after a restart. The primary runs once (pending label, duplicate clicks ignored) with at most three `downloads.v1.remove` requests in flight. All-success closes and announces; a partial failure stays open with a warning Banner showing removed/kept counts and one `<bdi>` name plus the error catalogue reason per kept row. A second click cannot repeat the batch |
| Row selection | `DownloadTable` (B2.1) | exists | Single selection by job ID. The file-name cell is a button with `aria-pressed`; click a row or the button; ↑/↓ moves the selection. Selected rows use `--color-row-selected` / `--color-row-selected-text`. Selection drives Resume and Stop in the toolbar and the File menu. A selected job hidden by the search or category filter keeps its ID but enables no row command; clearing the filter restores it |
| ProgressBar, range selection, context menu | planned B2 | — | The context menu mirrors the File menu |

## 5. Download list, progress and queues

- **Columns:** File name · Size · Status · Time left · Transfer rate · Last try · Description. The table uses a fixed layout with ellipsis truncation, and the full name is in the `title` attribute. Filenames are wrapped in `<bdi>` for mixed-direction text.
- **Values come only from service snapshots** (`features/downloads/present.ts`). The v1 contract provides no rate, ETA or last-try time, so those cells show "—". The UI never estimates them.
  - Size: binary units, decimal-string safe beyond 2^53, truncated rather than rounded up. "Unknown" when the total is null.
  - Status:
    - Complete, Error, Cancelled;
    - Paused (user);
    - "Waiting: <blocker>" (policy, always distinct from Paused);
    - "Downloading NN.N%" (the percentage only when the total is known);
    - Connecting, Verifying, Scanning, Finishing.
- **Empty state:** one full-width row explaining the situation. Without the service it says the list comes from the service, which is not connected. An empty table drops its minimum width so the message stays visible. A search with no matches says so and offers to clear the search only when the unfiltered category still has rows; an empty category/history keeps the ordinary empty copy, and a pending first list keeps "Loading downloads…".
- **Categories:** extension-based defaults (see `categories.ts`); a job's own `category` (B2.8) wins when the service reports it. Unfinished means not completed or cancelled.
- **Queues (B2.11, ADR 0017):** Queues in the category panel lists the service's queues (the main queue translated, others by their name as data). A stopped queue says "(stopped)" in words. A queue view lists its unfinished downloads in **execution order** (`queue_position`); clicking a column header only sorts the view. Start/Stop queue (toolbar and Downloads menu) acts on the selected queue, else the selected download's queue, else the main queue, and says why when it is already in that state. Downloads → Manage queues… creates, renames, limits (1–16), starts/stops and deletes queues (a delete names the queue its downloads move to and says that nothing is deleted). File → Queue and priority… changes the selected download's queue and priority and its place (top/up/down/bottom); File menu items and Alt+↑/↓ do the same from the keyboard. Every change is a service command; the list updates from the event stream.
- **Live data (B2.9, ADR 0016):** the list starts from the `events.v1.subscribe` snapshot and follows its events (progress about every 400 ms, changes at once). A gap, another service instance or `snapshot_required` takes a fresh snapshot; a removed job never returns. Only a service without events is polled about once a second (the B2.1 behaviour, kept as an explicit fallback). A transfer without a known total shows the received size ("Downloading 1.5 MB"). Completed rows show how they were verified in Description.
- **Row commands (B2.3):** File → Stop, Resume, Cancel download…, Remove from list… (both with a confirmation). Delete (toolbar) stays unavailable: deleting files does not exist, and its reason points to Remove from list, which keeps the file. After a removal, the next row (or the previous one) is selected and focused.
- **History search (B2.7, CORE-10):** the field filters the loaded snapshots by `display_name` or the source hostname/domain, case-insensitively after NFKC normalisation, trimmed, Bangla/RTL safe. Paths, queries, fragments, credentials and other hidden URL values never match. It combines with the category filter and the table's local sorting. The status bar count and the visible count follow the filtered rows. No match, empty history, first-list loading and service-unavailable states stay distinct.
- **Clear finished (B2.7, CORE-10):** File → Clear finished… covers every currently known completed, failed or cancelled job across category and search filters. Row IDs are frozen when the dialog opens; jobs that finish later are not added. It uses `downloads.v1.remove` with bounded concurrency, removes a row only after its acknowledgement, never deletes files and never touches unfinished, paused, blocked or finalizing jobs. On partial failure the kept rows, counts and actionable reasons stay visible; an uncertain result is reported honestly and the list is refreshed from the service.
- **Announcements (B2.3):** a polite live region in the status bar announces a download completing, failing or being cancelled, and a removal; progress is never announced per poll.
- **Planned (B2):** Shift/Ctrl range selection, a per-row context menu, a double-click properties dialog, column sorting and resizing, and virtualization for 10k+ rows (ADV12).

## 6. States

Every state pairs an icon, a text label and a tone. Colour is never the only cue.

| State | Source (contract) | Tone | Label (en) | Notes |
|---|---|---|---|---|
| Empty | no jobs | neutral | "No downloads to show" | Says why, and the primary next step |
| Loading / checking | client connecting | neutral | "Checking the download service…" | No skeleton longer than 1s; then show a real state |
| Loading list | connected, first list pending | neutral | "Loading downloads…" | In the table's empty row, even while a search is active |
| No search matches | local search, category has rows, none match | neutral | "No downloads match your search." | Hint to clear the search; announced once after typing settles, never per keystroke |
| Clear finished partial failure | bulk remove, some rejected/unavailable/uncertain | warning | "Some downloads were not cleared" | Banner in the confirmation: removed/kept counts and each kept `<bdi>` name with its error kind/action; uncertain outcomes say so |
| Service lost | `ConnectionState.unavailable` (`not_running`) after or before a list | warning | "Download service not connected" | Banner above the list; while the bounded automatic reconnection (attach-only, B2.10) runs it says "Trying to reconnect…" (a status), then offers Reconnect; the last known rows stay, explicitly marked as possibly out of date; commands say why they are unavailable |
| Duplicate link | `downloads.v1.check` found same-link jobs | warning | "This link is already in the list" | In Add URL (`role=alert`): the existing `<bdi>` name, "and N more", and Show existing download (primary) · Download again · Cancel. Nothing is added until a choice |
| Name taken (ask) | blocker `name_conflict` | warning | "A file with this name already exists" | In the details panel: says nothing was replaced and names the free `<bdi>` name; "Save as <name>" (primary) · Cancel download |
| Queue stopped | blocker `queue_stopped` | warning | "Waiting: queue stopped" | Distinct from Paused; Start queue never resumes a user pause |
| Service unavailable | `ConnectionState.unavailable` | warning | "Download service not connected" | Explains the reason honestly; disables actions with a reason |
| Queued | `phase=queued` | neutral | "Queued" | |
| Active | `phase=transferring` | info | percentage / speed | |
| Paused (user) | `desired_state=paused` | neutral | "Paused" | Paused fill; Resume is the primary row action |
| Held by policy | `blockers[]` non-empty | warning | e.g. "Waiting for unmetered network" | Shows the specific blocker; distinct from Paused |
| Verifying / scanning | `phase=verifying/scanning` | info | "Verifying…" / "Scanning…" | Never displayed as complete |
| Failed | `phase=failed` + `error` | danger | error kind message | Shows the `next_action` as a button (e.g. "Refresh link") |
| Cancelled | `phase=cancelled` | neutral | "Cancelled" | |
| Completed | `phase=completed` | success | "Completed" | Only after verification and finalization |
| Capability unavailable | `BACKEND_CAPABILITY_UNAVAILABLE` | warning | "Not supported by the current engine" | Never offers a hidden fallback |

## 7. Settings and dialogs

- **Close behaviour (B2.2):** Options → "When the window is closed": hide to the tray or menu bar, or close the window. Both say that downloads continue. The value is saved by the desktop shell, not by the window. Without a tray, the note says that hiding minimizes instead. In the browser preview the setting is shown as unavailable.
- **Tray (B2.2):** a menu with "Open Downova" and "Exit and stop downloads", in the window's language. Opening never depends on clicking the icon only.

- Settings are grouped into sections (General, Downloads, Network, Browser, Advanced), each a labelled form region. Changes apply immediately unless they are risky. Risky changes (deleting files, clearing history, the proxy) need confirmation.
- "Clear list" and "Delete file" are separate, explicitly worded actions (CORE-10).
- **Options → Downloads (B2.8, ADR 0015):** a labelled group "Download folders" with one text field per category (a full path to an existing folder, `dir="ltr"`; empty = the default download folder) and "When a file with the same name exists": save with a new name (recommended) or ask first; the note says Downova never replaces a file. One "Save download settings" button; the service validates every folder and the result is reported (`role=status` / `role=alert`). A change applies to new downloads only, and the note says so. Unavailable without the service.
- **Options → History (B2.12, ADR 0018):** "Keep finished downloads in the list": Forever (recommended), 7 / 30 / 90 days, 1 year. The description says that completed, failed and cancelled downloads leave the list after that period, that downloaded and `.downova-part` files always stay on disk, and that unfinished work is never removed.
  - **Apply** first asks the service for the exact count. With a non-zero count, a warning banner (`role=alert`) says "N of M finished downloads … will leave the list now" and that files stay, with **Apply and remove** (primary) and **Keep them**. With zero it saves directly.
  - **Clean now** does the same for the saved period. It is unavailable, with a reason, while history is kept forever.
  - Results are a `role=status` line. Focus returns to the button that opened the question.
- **Add URL (B2.8):** besides the link, "Save as" (automatic or a category) and "Queue"; the duplicate prompt replaces the footer until the user chooses.
  - **B2.18:** an optional "Publisher's SHA-256" field (`dir="ltr"`, its hint says where it comes from and that the download only completes when the file matches). Anything but exactly 64 hexadecimal digits is explained inline (`aria-invalid`, `role=alert`) before sending.
- **Retry (B2.16, ADR 0020):** File → "Retry…" and a primary "Retry…" button in the failed download's details. A confirmation dialog names the file, says it starts again from the beginning with the same name, folder, queue and priority, states how much partial data is discarded, and mentions the publisher checksum when one was given. Buttons: **Keep as it is** and **Retry from the beginning** (primary). Pending, uncertain (same request repeated) and refused states use the error catalogue. Unavailable with reasons for anything not failed, and for expired links (Refresh link instead) or sign-in failures.
- **Refresh link (B2.17):** File → "Refresh link…" and a details button, for a paused or failed download. A labelled "New link" field (validated like Add URL) and **Use new link**. When progress cannot be kept, a warning banner gives the localized reason and how much would be discarded, and the footer becomes **Keep current download** (sends nothing) and **Restart from zero with the new link** (primary). The new link is never shown anywhere else.
- **Checksum presentation (B2.18):** a completed download's details say either "Publisher checksum matched" (success tone) or "SHA-256 computed locally (no publisher checksum)" (info tone, explaining that a local hash does not prove the file is the publisher's), with the hash in `--font-mono` wrapping anywhere. A pending checksum and a mismatch (with Retry and Refresh link as the recovery) are explained in the same panel.
- **Folders (B2.19, ADR 0021):** each folder field in Options → Downloads has a "Browse…" button (accessible name "Choose the folder for <category>") that opens the native dialog and only fills the field; cancel changes nothing, Save still saves. File → "Open containing folder" and a details button reveal the selected download's folder (the file selected when present); a missing file, folder or drive is explained in the details panel.
- A setting that only exists locally in the UI says so (see the current language and theme note).

## 8. Window resizing

| Width | Behaviour |
|---|---|
| > 760px | Full layout; category panel `--category-panel-width` |
| 561–760px | Category panel narrows to `--category-panel-width-compact` |
| ≤ 560px | Category panel hidden; Options fields stack |
| any | The toolbar scrolls horizontally rather than wrapping. A populated table keeps `--table-min-width` and scrolls inside its pane. The status bar always stays within the window |

The minimum window size is 640×480 (`tauri.conf.json`). The page itself never scrolls horizontally. Only the toolbar and the table pane do.

## 9. Content and language

- The UI is available in English and Bangla (`i18n/messages.ts`). The `Messages` type forces every locale to define every key. `<html lang>` follows the locale.
- Copy is plain, short and status-first, and it explains causes ("The link expired — get a new link from the website"). No exclamation marks or blame.
- **Bangla:** Bangla numerals are not forced; Latin digits are used for sizes and speeds for consistency with filenames and units **(default)**. Units are written as "MB/s" in both languages.
- **Long filenames:** truncate in the middle, keeping the extension. The full name is available via the tooltip and the detail view, and is announced in full by screen readers.

## 10. Accessibility

- **Keyboard:** everything works without a mouse. A skip link goes to the main content. Tab order follows the visual order.
- **Focus:** one visible focus ring (`--color-focus`, 2px, offset 2px). It is never removed.
- **Implemented:** menu bar arrows and Esc, toolbar ←/→, dialog Tab trap and Esc with focus return, and Ctrl/Cmd+F to focus/select the history search (not while a dialog is open; Esc clears the field while it has focus).
- **Planned shortcuts:**
  - Ctrl/Cmd+N add
  - Space pause/resume the selected job
  - Delete cancel (with confirmation)
  - F6 cycle regions
- **Screen readers:**
  - landmarks (`nav`, `main`);
  - headings per view;
  - live regions only for meaningful changes (job completed, failed or cancelled, removals, service state, a search's match count once typing settles), with progress announcements throttled to at most one every 10s per job;
  - the progress bar exposes values.
- **Contrast:** text and focus colours target WCAG 2.2 AA in both themes. Formal conformance is claimed only after an audit.
- Forced colours and reduced motion are respected (`tokens.css`).

## 11. Themes and RTL

- **Theme:** follows the system preference by default. `data-theme="light|dark"` on `<html>` overrides it. Future themes (high contrast, F16) are additional token sets only.
- **RTL (F15, future):** layout already uses logical properties (`inset-inline-*`, `border-inline-*`, `text-align: start`). URLs, paths and filenames are wrapped in `dir="auto"` or `<bdi>` so mixed-direction text stays identifiable. Icons with a direction (arrows) are mirrored, while media and progress glyphs are not.

## 12. Browser extension consistency

- **Automatic capture options page (B2.13, ADR 0019):** opened from the popup ("Automatic capture settings"); same colours, type scale and controls as the popup (`popup.css` + `options.css`), single column, at most 44 rem wide.
  - **Order:** the switch (with its permission explanation); permission and Downova status (text, `role=status`); "What is captured" (supported and browser-only lists); sites and exclusions (one text field + Add, removable entries with a labelled × button); filters (file types, MIME types, minimum size in MB, Save); recent activity.
  - **Activity:** each entry has a text state (never colour alone), the link as `scheme://host/path?…`, the reason when kept in the browser, what happened to the browser's copy, and the job ID. `aria-live=polite` announces new entries only.
  - **Language:** English/Bangla from the browser UI language. Site and filter inputs are `dir=ltr`.
  - **Accessibility:** forced-colors and reduced-motion rules; colours pass WCAG 2.2 AA (automated check).

- The popup and options pages reuse the same tokens. The token file will be shared through a package when the extension is created in B2. They follow the same state vocabulary (§6) and copy.
- **Popup** (~360px wide): connection status to the desktop app (connected, not installed, needs repair), the last few handoffs with their status, and a "Capture downloads automatically" toggle (off by default).
- **Context menu:** a single "Download with Downova" item on links. If the app is unavailable, the item still appears, and choosing it explains the problem and leaves the browser download alone (BRW-03).
- The extension never shows file progress it does not get from the service.

- **Language (B2.20):** the popup (hand-offs, service readiness, capture summary), the options page and the context-menu title follow the browser's UI language (English or Bangla) from the one extension catalogue. Stored records keep stable outcome codes, never translated text; records from before B2.20 are shown from their state.

## 13. Classic-layout parity checklist

This checklist compares the window with the classic IDM-style download-manager surfaces, using the layout and workflow only (§1). Keep it current. "Needs service" means the surface needs the service API or real download data, so building it earlier would show controls that cannot work.

| Surface | Status | Notes / milestone |
|---|---|---|
| Menu bar: Tasks, File, Downloads, View, Help | done | No "Registration" menu (free app) |
| Big-icon toolbar: Add URL … Stop queue | done | Faded with a reason when unavailable. "Grabber" (F09, B6) and "Tell a friend" are intentionally omitted |
| Categories panel with tree, close button, View toggle | done | |
| Columns: File name, Size, Status, Time left, Transfer rate, Last try, Description | done | Rate, ETA and last try show "—" until the contract provides them |
| Column sorting (name, size, status) | done | `aria-sort`; resizing and reordering columns planned for B2 |
| File-type icons per row | done | |
| "Q" (queue position) column | needs contract | Add when the snapshot carries queue membership (CORE-05, B2) |
| Row selection, right-click context menu, double-click → properties | needs service | B2 (needs real rows; drives File menu and toolbar availability) |
| Add URL dialog → download file info (category, save as, description, Start / Download later) | needs service | B2 (CORE-01, CORE-07) |
| Per-download progress window (segments, connections, speed limiter tab, on-completion options) | needs service | B2–B3 (CORE-02, CORE-06) |
| Download complete dialog (Open, Open with, Open folder) | needs service | B2–B3 (SEC-03: never auto-open) |
| Scheduler dialog (queues, schedule, files in queue) | needs service | B3 (CORE-05, CORE-08) |
| Speed limiter (Downloads menu) | menu entry only | B3 (CORE-06) |
| Options tabs: General, File types, Save to, Downloads, Connection, Proxy, Site logins, Sounds | partial | Only language and theme today; other tabs arrive with the settings service (B2+); proxy F06; site logins need SEC-02 |
| Check for updates | menu entry only | B3 (ADV09 signed updates) |
| System tray icon and menu | needs Tauri build | B2 (UI-04; needs the Rust toolchain) |
| Browser "download this video" panel | later | B5 media phase (E01), subject to store policy |
| Drop target / basket | later | B4 (F03) |
