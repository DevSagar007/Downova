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
| ARCH-01 | B0 | service | crates/service/src/main.rs (separate `dm-service` process, `service.lock` single instance, shutdown) | `cargo test -p dm-service --test process -- --ignored` PASSED: separate process, second instance exits 3, shutdown exits 0 (Windows 11, 2026-10-21); UI-close/browser-close cases need B2 | in-progress |
| ARCH-02 | B0 | service, backend-aria2 | — | — | documented |
| ARCH-03 | B0 | service, backend-aria2 | crates/backend-api/src/lib.rs; crates/backend-aria2 (attempt→GID map); crates/service/src/core.rs (job_/att_ generation); crates/backend-http | core tests `ids_are_service_owned_and_snapshots_are_redacted`, aria2 fixture `handle_is_the_attempt_id_not_the_gid` PASSED (Windows 11, 2026-10-21); CI run #1 (commit 203de61) green on windows-latest, macos-latest, ubuntu-22.04, ubuntu-24.04 — covers B0.1 code only; engine exit monitor: watcher task owns and reaps aria2 (`wait_exit`, `exit_status`); K01 + process engine-kill test with bounded sync, process `gone` ×3 (Windows 11, 2026-10-25, local); CI runs #4/#5 failed on Unix before this fix (unreaped zombie); second backend `dm-backend-http` behind the same trait, selected with `dm-service --backend http` (http_backend_process) (Windows 11, 2026-10-25); [B0.4.2 report](reports/b0.4.2-http-backend.md) | implemented |
| ARCH-04 | B0 | domain, service, backend-aria2 | crates/backend-aria2/src/capabilities.rs (`report(max_tries)`); crates/service/src/core.rs (gates on add/segmentation/resume); hello; crates/backend-http | core gate tests; capabilities unit test (only Http supported, and only with max-tries 1); process test `real_service_single_instance_capabilities_and_https_gate` (hello shows Http supported, Https unverified, SegmentedHttp/ResumeStrongValidator unsupported; https add refused) (Windows 11, 2026-10-21); promotions per [ADR 0003](decisions/0003-aria2-go-no-go.md); B0.4.1: TLS backend in the Https note; CA bundle ignored by WinTLS refused at launch (`tls_backend_is_read_from_the_banner`, T00, T01) (Windows 11, 2026-10-25); dm-backend-http report: Http/Https supported only after H/S/D/T/X/Z acceptance passed, Segmented/Resume unsupported (`report_is_complete_and_promotes_only_proven_entries`) (Windows 11, 2026-10-25); dm-backend-http: ResumeStrongValidator promoted only after V01–V15 passed (Windows 11, 2026-10-25) | implemented |
| ARCH-05 | B1 | storage, service | — | — | documented |
| ARCH-06 | B0 | service, platform | crates/platform/src/ipc.rs (owner-only named pipe / 0700 socket, random pipe name, endpoint file); crates/service/src/rpc.rs (closed method set) | platform and service IPC tests PASSED (Windows 11, 2026-10-21); GitHub CI run #2 (commit 73fe021) green on windows-latest, macos-latest, ubuntu-22.04, ubuntu-24.04 incl. real-process service tests (Unix socket ran on macOS/Linux); cross-user denial not tested | implemented |
| ARCH-07 | B0 | contracts, domain | packages/contracts/schema/ (incl. RpcRequest/RpcResponse/JobRef/ListDownloadsResult); crates/domain/src/api.rs; scripts/gen-contracts.mjs | `pnpm contracts:check` PASSED (9 valid / 4 invalid) + `cargo test -p dm-domain --test contract_examples` PASSED (Windows 11, 2026-10-21) | verified |
| ARCH-08 | B2 | service, desktop | — | — | documented |
| ARCH-09 | B2 | service, desktop, platform | — | — | documented |
| ARCH-10 | B0 | docs | docs/decisions/ (0001, 0002); .claude/rules/architecture.md | ADR 0002 recorded for the backend-interface move | implemented |
| CORE-01 | B0, B2 | service, cli, desktop | crates/service/src/core.rs (`add`); crates/cli (`dm add`); crates/backend-http | real HTTP download, SHA-256 match: process test `real_http_download_through_the_service_is_sha256_verified` and CLI `cli_against_real_service_and_aria2` (`dm add` → completed, hash match; https refused, exit 4) (Windows 11, 2026-10-21); HTTPS stays unverified ([ADR 0003](decisions/0003-aria2-go-no-go.md)); UI is B2; T01 trusted HTTPS passed on Linux GnuTLS and macOS OpenSSL in CI run #4 (as reported); not testable on Windows WinTLS; HTTPS end to end on dm-backend-http: T01 (fixture CA as explicit extra root) success, T02–T04 rejected; `dm add` via `--backend http` → completed, SHA-256 match (Windows 11, 2026-10-25) | in-progress |
| CORE-02 | B1 | backend-aria2 | crates/backend-aria2 (split / max-connection-per-server) | B0.1 `segmented_download_uses_ranges_and_matches_hash` PASSED; B0.4 C05: a change during a segmented transfer is spliced silently → SegmentedHttp **unsupported** (Windows 11, 2026-10-21); [B0.4 report](reports/b0.4-http-correctness.md), [ADR 0003](decisions/0003-aria2-go-no-go.md); B0.4.1 M03/M10: If-Match keeps conforming segmented transfers safe, but M04 rules it out as a general guarantee (Windows 11, 2026-10-25); ADR 0004 | in-progress |
| CORE-03 | B0 | service, domain | crates/service/src/core.rs (pause/resume/cancel); crates/cli (`dm pause|resume|cancel`); crates/backend-aria2 (engine calls) | core + IPC tests PASSED; CLI command→method mapping PASSED via IPC test peer (Windows 11, 2026-10-21); real-engine path blocked by the Http gate; partial resume refused until strong validator; pause/resume through `dm-service --backend http` and `dm pause/resume`: continuation `Range` + `If-Range`, SHA-256 match; pause acknowledges confirmed bytes (`pause_reports_the_bytes_the_backend_confirmed`) (Windows 11, 2026-10-25) | in-progress |
| CORE-04 | B1 | service, storage, backend-aria2 | — | — | documented |
| CORE-05 | B2 | service, desktop | — | — | documented |
| CORE-06 | B3 | service, backend-aria2 | — | — | documented |
| CORE-07 | B2 | service, domain | — | — | documented |
| CORE-08 | B3 | service | — | — | documented |
| CORE-09 | B3 | desktop, platform | — | — | documented |
| CORE-10 | B2 | storage, desktop | — | — | documented |
| CORE-11 | B1 | domain, desktop, cli | crates/domain/src/error_kind.rs (+ SERVER_BUSY, TLS_HANDSHAKE_FAILED); crates/backend-aria2/src/errors.rs (`classify(code, message)`); crates/backend-http | `errors::tests` PASSED; http_correctness S01–S03, T02/T03, L01, C08 classify 401/403/410/429/503/TLS/interruption exactly (Windows 11, 2026-10-21); [B0.4 report](reports/b0.4-http-correctness.md); Bangla copy not built; 412 → REMOTE_CHANGED on the first request (M06); aria2 drops the status on resumed requests (M02/M07) (Windows 11, 2026-10-25); dm-backend-http keeps HTTP status classes and TLS reasons (S01, T02–T04) (Windows 11, 2026-10-25) | in-progress |
| COR-01 | B1 | backend-aria2 | crates/backend-aria2 (`--max-tries=1`, `--continue=false`); crates/backend-http/src/transfer.rs (`validate_partial_response`) | http_correctness R01–R04 safe success (range ignored, wrong Content-Range, 416 restart from zero), L01 production safe failure (Windows 11, 2026-10-21); aria2 sends no If-Range, so validated range resume needs an engine patch ([ADR 0003](decisions/0003-aria2-go-no-go.md)); dm-backend-http validated resume: V01–V15 (206 appended only with exact Content-Range/ETag/length/identity; 200 → restart; 416/wrong range rejected), unit `a_206_is_accepted_only_when_every_check_passes`, `content_range_parsing_is_strict` (Windows 11, 2026-10-25); [B0.4.2 report, Validated resume](reports/b0.4.2-http-backend.md#validated-resume-second-increment-2026-10-25) | implemented |
| COR-02 | B1 | service, backend-aria2 | crates/backend-aria2/src/capabilities.rs (ResumeStrongValidator unsupported); crates/service/src/core.rs (partial resume refused); crates/backend-http (Identity, Checkpoint, If-Range) | http_correctness C01–C06 [engine] prove aria2 mixes versions (strong, weak, lying, missing ETag); C08 [production] fails safely; service test `resume_gate_uses_fresh_engine_progress_not_the_last_poll` (Windows 11, 2026-10-21); [B0.4 report](reports/b0.4-http-correctness.md), [ADR 0003](decisions/0003-aria2-go-no-go.md); B0.4.1 M01–M10: If-Match via header is safe only with conforming servers; M04 (ignored If-Match) unsafe → stays unsupported (Windows 11, 2026-10-25); C01–C06 now prove the change happened (fixture transition + per-request `served` tag) before asserting the splice (Windows 11, 2026-10-25, local); CI run #4 C05 on aria2 1.36.0 was an unproven scenario, not safety evidence; dm-backend-http: strong ETag only (V06 weak/missing refused), same-length change detected by ETag (V04) or restarted via If-Range (V03); lying validator undetectable without a hash (V05) (Windows 11, 2026-10-25) | implemented |
| COR-03 | B1 | backend-aria2 | crates/backend-aria2 | http_correctness R04 (416), L01 (truncated), L02 (unknown length), L03 (unknown length cut short: **unsafe**, protocol-inherent), L04 (zero bytes), C07 (encoding change: safe failure) (Windows 11, 2026-10-21); [B0.4 report](reports/b0.4-http-correctness.md); C07 proves the gzip continuation was served before judging; safe failure `RANGE_UNSUPPORTED` ×3 (Windows 11, 2026-10-25, local); CI run #4 Linux "SafeSuccess" was an unexercised scenario | in-progress |
| COR-04 | B1 | service, domain, backend-api | crates/domain/src/retry.rs (policy, `parse_retry_after`); crates/service/src/core.rs (`after_failure`, cool-down, discard before retry); crates/backend-api (`BackendError.retry_after`, `discard`); crates/backend-aria2/src/errors.rs (403/410 → LINK_REFRESH_REQUIRED, 401 → AUTH_REQUIRED, 429/503 → SERVER_BUSY, 412 → REMOTE_CHANGED); crates/backend-http | domain `retry::tests` (6) and service retry tests with paused time (4) PASSED; real service + aria2 `transient_failures_are_retried_by_the_service_from_zero` (503 and mid-body cut → retry without Range, SHA-256 match) (Windows 11, 2026-10-25); Retry-After proven with the fake backend only, aria2 cannot report it; [B0.4 report §B0.4.1](reports/b0.4-http-correctness.md#b041-engine-remediation-2026-10-25); dm-backend-http reports Retry-After (S02); real service waited ≥ 1 s for `Retry-After: 1` and retried from zero (Windows 11, 2026-10-25) | implemented |
| COR-05 | B1 | service | — | — | documented |
| COR-06 | B1 | service | — | — | documented |
| COR-07 | B0 | domain | crates/domain/src/state.rs (`decide_run`) | cargo test PASSED (`state::tests` incl. `manual_pause_survives_network_reconnect`) (Windows 11, 2026-10-21) | verified |
| COR-08 | B1 | storage, service | — | — | documented |
| COR-09 | B0 | service, storage | crates/service/src/core.rs (in-memory idempotency map); crates/cli/src/run.rs (one key per add, reused on retry) | service `round_trip_over_ipc_with_lost_acknowledgement_retry`; CLI `lost_acknowledgement_is_retried_once_with_the_same_key`, `explicit_key_is_reused_across_invocations` PASSED (Windows 11, 2026-10-21); **in-memory only** — durable records are B1 | in-progress |
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
| SEC-01 | B0 | domain, service | crates/domain/src/redact.rs; crates/backend-aria2 (errorMessage never forwarded; credential headers refused); crates/service (redacted snapshots); crates/cli/src/output.rs; crates/backend-http | redact, aria2 secret-persistence, service redaction and CLI tests PASSED; http_correctness D02 proves aria2 forwards headers cross-origin, D03 proves the adapter refuses them; real `dm add` output redacts the token (Windows 11, 2026-10-21); logs/export/SQLite not built yet; dm-backend-http: credentials/validators dropped on cross-origin hops (D02, D03), secrets absent from errors and Debug (Z01) (Windows 11, 2026-10-25) | in-progress |
| SEC-02 | B3 | platform | — | — | documented |
| SEC-03 | B2 | service, desktop | — | — | documented |
| SEC-04 | B0 | all | — | — | documented |
| SEC-05 | B2 | desktop (src-tauri) | apps/desktop/src-tauri/capabilities/default.json; tauri.conf.json (CSP) | Tauri debug build + window launch PASSED with CSP and core:default only (Windows 11, 2026-10-21); no custom commands yet | in-progress |
| REL-01 | B0, B3 | scripts, desktop | apps/desktop/src-tauri (tauri build --debug --no-bundle) | Windows: debug binary built and window launched/closed (Windows 11, 2026-10-21); no installer yet; macOS/Linux not run | in-progress |
| REL-02 | B3 | docs | — | — | documented |
| REL-03 | B3 | scripts, docs | — | — | documented |
| REL-04 | B3 | scripts, tests | — | — | documented |
| REL-05 | B0 | backend-aria2, scripts | scripts/fetch-aria2.mjs (pinned 1.37.0 + SHA-256); crates/backend-aria2/src/binary.rs (version check) | fetch + version check PASSED (Windows 11, 2026-10-21); TLS backend audited: WinTLS/SChannel, no OpenSSL; libssh2 1.11.0, expat 2.5.0, c-ares 1.19.1 predate security fixes → rebuild required before release ([B0.4 report](reports/b0.4-http-correctness.md), [ADR 0003](decisions/0003-aria2-go-no-go.md)); B0.4.1: TLS backend detected at runtime (`binary::TlsBackend`) (Windows 11, 2026-10-25) | in-progress |
| MOB-01 | B7 | mobile (future) | — | — | deferred |
| MOB-02 | B7 | mobile (future) | — | — | deferred |
| F01 | B0, B4 | cli, service | crates/cli (`dm` executable: args.rs, run.rs, output.rs) | `cargo test -p dm-cli` PASSED (7 unit + 12 peer/binary) and `--test real_service -- --ignored` PASSED against dm-service + aria2 1.37.0 (Windows 11, 2026-10-21). Internal harness only; public scripting docs and headless packaging are B4 | in-progress |
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
| E04 | B1 | service, domain | crates/service/src/core.rs (service-chosen file name) | http_correctness N01: Content-Disposition `filename*` with Unicode and `../` is ignored; only the service-chosen name is written (Windows 11, 2026-10-21); normalisation and limits are B1 | in-progress |
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
