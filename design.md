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
| Progress | `--color-progress-track`, `--color-progress-fill`, `--color-progress-fill-paused` | |
| Sizing | `--control-height-sm/md`, `--icon-sm/md/lg/xl`, `--row-height`, `--sidebar-width(-collapsed)` | |
| Motion | `--duration-fast/normal`, `--easing-standard` | Zeroed under `prefers-reduced-motion` |
| Elevation | `--shadow-raised`, `--shadow-overlay`, `--color-backdrop` | Overlays (menus, dialogs) only |
| Classic chrome | `--color-chrome`, `--color-chrome-border`, `--color-table-header`, `--color-grid`, `--color-row-selected(-text)` | Menu bar, toolbar, panel headers, list grid lines, selection |
| Toolbar tones | `--color-icon-{add,resume,stop,delete,options,schedule,queue}`, `--icon-toolbar`, `--toolbar-button-width`, `--opacity-unavailable` | Toolbar icon colour per command group |
| Window layout | `--menubar-height`, `--category-panel-width(-compact)`, `--table-row-height`, `--table-min-width`, `--statusbar-height`, `--menu-min-width`, `--dialog-width` | Main window regions |

**Icons:** Lucide (`lucide-react`, ISC license), outline style, 16px (`--icon-md`) in controls and 32px in empty states. Icons are decorative (`aria-hidden`) unless they are the only content of a control, in which case the control has an `aria-label`.

## 4. Components

| Component | File | Status | Rules |
|---|---|---|---|
| MenuBar | `ui/MenuBar.tsx` | exists | `role="menubar"`. ←/→ move between menus, ↓ opens, ↑/↓ move within a menu, Esc closes and returns focus. Items are `menuitem`, `menuitemradio` (theme, language) or `menuitemcheckbox` (toggles). Unavailable items use `aria-disabled` plus a tooltip reason |
| ToolbarButton | `ui/ToolbarButton.tsx` | exists | Icon (`--icon-toolbar`) over label. A tone sets the icon colour (`--color-icon-*`). An unavailable button stays focusable with `aria-disabled` and a described reason, and its icon fades to `--opacity-unavailable`. The toolbar supports ←/→ |
| Dialog | `ui/Dialog.tsx` | exists | Modal, `aria-modal`, labelled by its title. It focuses the first control, traps Tab, closes on Esc, and returns focus to the opener. Chrome-coloured header and footer |
| Button | `ui/Button.tsx` | exists | Variants: `primary` (one per dialog or view), `secondary`, `ghost`. Unavailable actions use `unavailableReason` |
| Banner | `ui/Banner.tsx` | exists (unused in the main window) | Inline tone message for dialogs and detail panels |
| EmptyState | `ui/EmptyState.tsx` | exists (unused in the main window) | Large empty-state panels outside the table |
| CategoryTree | `features/downloads/CategoryTree.tsx` | exists | Nested list. Parents have an expand/collapse button (`aria-expanded`) |
| DownloadTable | `features/downloads/DownloadTable.tsx` | exists | Native `<table>` with a caption, sticky header and fixed column widths. See §5 |
| ProgressBar, row selection, context menu | planned B2 | — | Selection drives the File menu and toolbar. The context menu mirrors the File menu |

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
- **Empty state:** one full-width row explaining the situation. Without the service it says the list comes from the service, which is not connected. An empty table drops its minimum width so the message stays visible.
- **Categories:** extension-based defaults (see `categories.ts`), editable later through rules (ADV03). Unfinished means not completed or cancelled. Queues show queued jobs until queue membership is in the contract.
- **Planned (B2):** row selection (single and Shift/Ctrl range), a progress bar in the Status cell, a per-row context menu, a double-click properties dialog, column sorting and resizing, and virtualization for 10k+ rows (ADV12).

## 6. States

Every state pairs an icon, a text label and a tone. Colour is never the only cue.

| State | Source (contract) | Tone | Label (en) | Notes |
|---|---|---|---|---|
| Empty | no jobs | neutral | "No downloads to show" | Says why, and the primary next step |
| Loading / checking | client connecting | neutral | "Checking the download service…" | No skeleton longer than 1s; then show a real state |
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

- Settings are grouped into sections (General, Downloads, Network, Browser, Advanced), each a labelled form region. Changes apply immediately unless they are risky. Risky changes (deleting files, clearing history, the proxy) need confirmation.
- "Clear list" and "Delete file" are separate, explicitly worded actions (CORE-10).
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
- **Implemented:** menu bar arrows and Esc, toolbar ←/→, dialog Tab trap and Esc with focus return.
- **Planned shortcuts:**
  - Ctrl/Cmd+N add
  - Space pause/resume the selected job
  - Delete cancel (with confirmation)
  - Ctrl/Cmd+F search
  - F6 cycle regions
- **Screen readers:**
  - landmarks (`nav`, `main`);
  - headings per view;
  - live regions only for meaningful changes (job completed or failed, service state), with progress announcements throttled to at most one every 10s per job;
  - the progress bar exposes values.
- **Contrast:** text and focus colours target WCAG 2.2 AA in both themes. Formal conformance is claimed only after an audit.
- Forced colours and reduced motion are respected (`tokens.css`).

## 11. Themes and RTL

- **Theme:** follows the system preference by default. `data-theme="light|dark"` on `<html>` overrides it. Future themes (high contrast, F16) are additional token sets only.
- **RTL (F15, future):** layout already uses logical properties (`inset-inline-*`, `border-inline-*`, `text-align: start`). URLs, paths and filenames are wrapped in `dir="auto"` or `<bdi>` so mixed-direction text stays identifiable. Icons with a direction (arrows) are mirrored, while media and progress glyphs are not.

## 12. Browser extension consistency

- The popup and options pages reuse the same tokens. The token file will be shared through a package when the extension is created in B2. They follow the same state vocabulary (§6) and copy.
- **Popup** (~360px wide): connection status to the desktop app (connected, not installed, needs repair), the last few handoffs with their status, and a "Capture downloads automatically" toggle (off by default).
- **Context menu:** a single "Download with Downova" item on links. If the app is unavailable, the item still appears, and choosing it explains the problem and leaves the browser download alone (BRW-03).
- The extension never shows file progress it does not get from the service.

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
