| ADV12 | B2, B4 | desktop | apps/desktop/src/ui/MenuBar.tsx, ToolbarButton.tsx, Dialog.tsx; features/downloads/CategoryTree.tsx | `App.test.tsx` menu-bar keys, toolbar arrows, dialog Esc/focus return PASSED; no screen-reader run; no virtualization yet | in-progress || UI-03 | B2 | desktop | apps/desktop/src/features/downloads/present.ts, DownloadTable.tsx; status bar in src/app/App.tsx | `downloads.test.tsx` pause-vs-policy status text + table rendering PASSED; `App.test.tsx` disconnected state PASSED | in-progress || UI-02 | B2 | desktop | apps/desktop/src/ui/tokens.css (incl. classic chrome tokens); src/app/preferences.tsx | `App.test.tsx` View-menu dark theme PASSED; light/dark checked in browser preview (Windows); no contrast audit | in-progress || UI-01 | B3 | desktop | apps/desktop/src/i18n/messages.ts; src/app/preferences.tsx; View menu + Options dialog | `App.test.tsx` Options→Bangla PASSED (vitest, Windows, 2026-10-21); Bangla rendering checked in browser preview; no localisation QA | in-progress |# Traceability

Maps each requirement in [requirements.md](../requirements.md) to its milestone, owning module,
implementation location, and verification evidence. Update the row in the same change that
touches the requirement. `scripts/check-structure.mjs` fails if a requirement has no row, a row has
no requirement, or a `verified` requirement has no evidence.

Rules:
- **Evidence** must name a check that was actually run, the OS it ran on, and the date. A test that exists but was not run is written as "written — not yet run".
- Platform evidence is per OS. A pass on Windows proves nothing about macOS or Linux.
- Owning modules use the short names from [project-structure.md](project-structure.md). "(future)" means the location is planned and not yet created.

| ID | Milestone | Owning module | Implementation | Evidence | Status |
|---|---|---|---|---|---|
| ARCH-01 | B0 | service | — | — | documented |
| ARCH-02 | B0 | service, backend-aria2 | — | — | documented |
| ARCH-03 | B0 | service, backend-aria2 | crates/service/src/backend.rs; crates/domain/src/ids.rs | Rust tests written (`ids::tests`, contract invalid example `aria2-gid-as-job-id`) — Rust not yet run | in-progress |
| ARCH-04 | B0 | domain, service, backend-aria2 | crates/domain/src/capability.rs; crates/service/src/backend.rs (`require_capability`) | Rust tests written (`capability::tests`, `backend::tests`) — not yet run | in-progress |
| ARCH-05 | B1 | storage, service | — | — | documented |
| ARCH-06 | B0 | service, platform | — | — | documented |
| ARCH-07 | B0 | contracts, domain | packages/contracts/schema/; crates/domain/src/api.rs; scripts/gen-contracts.mjs | `pnpm contracts:check` PASSED (2026-10-21, Windows); Rust round-trip test not yet run | in-progress |
| ARCH-08 | B2 | service, desktop | — | — | documented |
| ARCH-09 | B2 | service, desktop, platform | — | — | documented |
| ARCH-10 | B0 | docs | docs/decisions/; .claude/rules/architecture.md | ADR 0001 exists | implemented |
| CORE-01 | B0, B2 | service, cli, desktop | — | — | documented |
| CORE-02 | B1 | backend-aria2 | — | — | documented |
| CORE-03 | B0 | service, domain | — | — | documented |
| CORE-04 | B1 | service, storage, backend-aria2 | — | — | documented |
| CORE-05 | B2 | service, desktop | — | — | documented |
| CORE-06 | B3 | service, backend-aria2 | — | — | documented |
| CORE-07 | B2 | service, domain | — | — | documented |
| CORE-08 | B3 | service | — | — | documented |
| CORE-09 | B3 | desktop, platform | — | — | documented |
| CORE-10 | B2 | storage, desktop | — | — | documented |
| CORE-11 | B1 | domain, desktop, cli | crates/domain/src/error_kind.rs | Rust tests written — not yet run | in-progress |
| COR-01 | B1 | backend-aria2 | — | — | documented |
| COR-02 | B1 | service, backend-aria2 | — | — | documented |
| COR-03 | B1 | backend-aria2 | — | — | documented |
| COR-04 | B1 | service, domain | — | — | documented |
| COR-05 | B1 | service | — | — | documented |
| COR-06 | B1 | service | — | — | documented |
| COR-07 | B0 | domain | crates/domain/src/state.rs (`decide_run`) | Rust tests written (`state::tests`) — not yet run | in-progress |
| COR-08 | B1 | storage, service | — | — | documented |
| COR-09 | B0 | service, storage | — | — | documented |
| BRW-01 | B2 | extension | — | — | documented |
| BRW-02 | B2 | browser-host | — | — | documented |
| BRW-03 | B2 | extension, browser-host | — | — | documented |
| BRW-04 | B2 | extension | — | — | documented |
| BRW-05 | B3 | extension, service, platform | — | — | documented |
| BRW-06 | B2 | extension | — | — | documented |
| BRW-07 | B2 | platform, browser-host | — | — | documented |
| BRW-08 | B3 | extension | — | — | documented |
| BRW-09 | B3 | platform, extension | — | — | documented |
| BRW-10 | B2 | extension | — | — | documented |
| BRW-11 | B6 | extension (Safari) | — | — | deferred |
| UI-01 | B3 | desktop | apps/desktop/src/i18n/messages.ts; src/app/preferences.tsx | `App.test.tsx` Bangla switch PASSED (vitest, Windows); no localisation QA | in-progress |
| UI-02 | B2 | desktop | apps/desktop/src/ui/tokens.css; src/app/preferences.tsx | `App.test.tsx` theme switch PASSED; no contrast audit | in-progress |
| UI-03 | B2 | desktop | apps/desktop/src/features/downloads/DownloadsView.tsx (service-unavailable state only) | `App.test.tsx` unavailable-state PASSED | in-progress |
| UI-04 | B2 | desktop | — | — | documented |
| SEC-01 | B0 | domain, service | crates/domain/src/redact.rs | Rust synthetic-secret tests written (`redact::tests`) — not yet run | in-progress |
| SEC-02 | B3 | platform | — | — | documented |
| SEC-03 | B2 | service, desktop | — | — | documented |
| SEC-04 | B0 | all | — | — | documented |
| SEC-05 | B2 | desktop (src-tauri) | apps/desktop/src-tauri/capabilities/default.json; tauri.conf.json (CSP) | Config only; not built | in-progress |
| REL-01 | B0, B3 | scripts, desktop | — | — | documented |
| REL-02 | B3 | docs | — | — | documented |
| REL-03 | B3 | scripts, docs | — | — | documented |
| REL-04 | B3 | scripts, tests | — | — | documented |
| REL-05 | B0 | backend-aria2, scripts | — | — | documented |
| MOB-01 | B7 | mobile (future) | — | — | deferred |
| MOB-02 | B7 | mobile (future) | — | — | deferred |
| F01 | B0, B4 | cli, service | — | — | documented |
| F02 | B4 | desktop, platform | — | — | documented |
| F03 | B4 | desktop | — | — | documented |
| F04 | B4 | desktop, service | — | — | documented |
| F05 | B4 | backend-aria2 | — | — | documented |
| F06 | B4, B6 | service, backend-aria2, platform | — | — | documented |
| F07 | B3, B4 | platform | — | — | documented |
| F08 | B4 | platform, service | — | — | documented |
| F09 | B6 | integrations (future) | — | — | deferred |
| F10 | B6 | service | — | — | deferred |
| F11 | B6 | backend-aria2 | — | — | deferred |
| F12 | B6 | backend-aria2, service | — | — | deferred |
| F13 | B6 | companion (future) | — | — | deferred |
| F14 | B6 | service | — | — | deferred |
| F15 | B4 | desktop | — | — | documented |
| F16 | B4, B6 | desktop | — | — | documented |
| E01 | B5 | media (future) | — | — | deferred |
| E02 | B2, B4 | extension | — | — | documented |
| E03 | B1, B4 | service, backend-aria2 | — | — | documented |
| E04 | B1 | service, domain | — | — | documented |
| E05 | B3 | service, desktop | — | — | documented |
| E06 | B3 | service, desktop, cli | — | — | documented |
| ADV01 | B4 | platform, service, domain | — | — | documented |
| ADV02 | B4 | service, backend-aria2 | — | — | documented |
| ADV03 | B4 | service, desktop | — | — | documented |
| ADV04 | B4 | service, contracts | — | — | documented |
| ADV05 | B1, B4 | platform, service | — | — | documented |
| ADV06 | B1 | service, storage | — | — | documented |
| ADV07 | B4 | service, desktop | — | — | documented |
| ADV08 | B5, B6 | integrations (future) | — | — | proposed |
| ADV09 | B3, B4 | scripts, desktop | — | — | documented |
| ADV10 | B6 | companion (future) | — | — | deferred |
| ADV11 | B4 | platform, browser-host | — | — | documented |
| ADV12 | B2, B4 | desktop | apps/desktop/src/ui/Button.tsx; src/app/App.tsx (skip link, focus, aria-current) | `App.test.tsx` keyboard focus tests PASSED; no screen-reader run | in-progress |
