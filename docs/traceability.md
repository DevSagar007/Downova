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
| ARCH-03 | B0 | service, backend-aria2 | crates/backend-api/src/lib.rs; crates/backend-aria2/src/lib.rs (attempt→GID map); crates/domain/src/ids.rs | cargo test PASSED (`ids::tests`, contract invalid example); aria2 fixture `handle_is_the_attempt_id_not_the_gid` PASSED (Windows 11, 2026-10-21); service-side ID generation pending B0.2 | in-progress |
| ARCH-04 | B0 | domain, service, backend-aria2 | crates/domain/src/capability.rs; crates/backend-api/src/lib.rs (`require_capability`); crates/backend-aria2/src/capabilities.rs | cargo test PASSED (`capability::tests`, `only_supported_capabilities_pass`, `report_is_complete_and_never_claims_unproven_support`) (Windows 11, 2026-10-21); hello over IPC pending B0.2 | in-progress |
| ARCH-05 | B1 | storage, service | — | — | documented |
| ARCH-06 | B0 | service, platform | — | — | documented |
| ARCH-07 | B0 | contracts, domain | packages/contracts/schema/; crates/domain/src/api.rs; scripts/gen-contracts.mjs | `pnpm contracts:check` PASSED + `cargo test -p dm-domain --test contract_examples` PASSED (5 valid / 2 invalid) (Windows 11, 2026-10-21) | verified |
| ARCH-08 | B2 | service, desktop | — | — | documented |
| ARCH-09 | B2 | service, desktop, platform | — | — | documented |
| ARCH-10 | B0 | docs | docs/decisions/ (0001, 0002); .claude/rules/architecture.md | ADR 0002 recorded for the backend-interface move | implemented |
| CORE-01 | B0, B2 | service, cli, desktop | — | — | documented |
| CORE-02 | B1 | backend-aria2 | crates/backend-aria2 (split / max-connection-per-server) | aria2 fixture `segmented_download_uses_ranges_and_matches_hash` PASSED, 3 ranged requests, hash match (Windows 11, 2026-10-21); [B0.1 report](reports/b0.1-aria2-fixtures.md) | in-progress |
| CORE-03 | B0 | service, domain | crates/backend-aria2/src/lib.rs (pause/resume/remove) | aria2 fixture `pause_resume_and_remove` PASSED at backend level (Windows 11, 2026-10-21); service commands pending B0.2 | in-progress |
| CORE-04 | B1 | service, storage, backend-aria2 | — | — | documented |
| CORE-05 | B2 | service, desktop | — | — | documented |
| CORE-06 | B3 | service, backend-aria2 | — | — | documented |
| CORE-07 | B2 | service, domain | — | — | documented |
| CORE-08 | B3 | service | — | — | documented |
| CORE-09 | B3 | desktop, platform | — | — | documented |
| CORE-10 | B2 | storage, desktop | — | — | documented |
| CORE-11 | B1 | domain, desktop, cli | crates/domain/src/error_kind.rs; crates/backend-aria2/src/errors.rs | cargo test PASSED (`error_kind::tests`, `errors::tests`) (Windows 11, 2026-10-21); truncation→RANGE_UNSUPPORTED is only partly accurate ([B0.1 report](reports/b0.1-aria2-fixtures.md)) | in-progress |
| COR-01 | B1 | backend-aria2 | crates/backend-aria2 (engine behaviour under test) | aria2 fixtures range-ignored (Complete, hash match) and truncated (Failed, no false success) PASSED (Windows 11, 2026-10-21); full matrix B0.4 | in-progress |
| COR-02 | B1 | service, backend-aria2 | — | — | documented |
| COR-03 | B1 | backend-aria2 | — | — | documented |
| COR-04 | B1 | service, domain | — | — | documented |
| COR-05 | B1 | service | — | — | documented |
| COR-06 | B1 | service | — | — | documented |
| COR-07 | B0 | domain | crates/domain/src/state.rs (`decide_run`) | cargo test PASSED (`state::tests` incl. `manual_pause_survives_network_reconnect`) (Windows 11, 2026-10-21) | verified |
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
| SEC-01 | B0 | domain, service | crates/domain/src/redact.rs; crates/backend-aria2 (no --save-session, conf file deleted, codes-only errors) | cargo test PASSED (`redact::tests`); aria2 fixture `secret_header_is_sent_but_never_persisted` PASSED (Windows 11, 2026-10-21); logs/events/export/SQLite not yet built | in-progress |
| SEC-02 | B3 | platform | — | — | documented |
| SEC-03 | B2 | service, desktop | — | — | documented |
| SEC-04 | B0 | all | — | — | documented |
| SEC-05 | B2 | desktop (src-tauri) | apps/desktop/src-tauri/capabilities/default.json; tauri.conf.json (CSP) | Tauri debug build + window launch PASSED with CSP and core:default only (Windows 11, 2026-10-21); no custom commands yet | in-progress |
| REL-01 | B0, B3 | scripts, desktop | apps/desktop/src-tauri (tauri build --debug --no-bundle) | Windows: debug binary built and window launched/closed (Windows 11, 2026-10-21); no installer yet; macOS/Linux not run | in-progress |
| REL-02 | B3 | docs | — | — | documented |
| REL-03 | B3 | scripts, docs | — | — | documented |
| REL-04 | B3 | scripts, tests | — | — | documented |
| REL-05 | B0 | backend-aria2, scripts | scripts/fetch-aria2.mjs (pinned 1.37.0 + SHA-256); crates/backend-aria2/src/binary.rs (version check) | fetch + version check PASSED (Windows 11, 2026-10-21); bundled OpenSSL version not yet audited ([B0.1 report](reports/b0.1-aria2-fixtures.md)) | in-progress |
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
