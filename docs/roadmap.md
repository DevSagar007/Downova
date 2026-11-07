# Roadmap

This roadmap turns research §12, §15.8 and §18.4 into increments. The time ranges are research estimates for one
full-time developer reusing an existing engine (§12). They are planning assumptions, not commitments. Requirement IDs
refer to [requirements.md](../requirements.md).

Rule from §12: do not start media (B5) or mobile (B7) work until packages for all three OSes and an end-to-end
handoff in one browser exist (the B2 exit).

## F — Foundation (current)
- **Prerequisites:** none.
- **Smallest deliverable:** a buildable workspace with docs, contracts, a desktop shell that honestly shows "not connected", the domain types, a verify entry point, and Claude rules and skills.
- **Acceptance:** `pnpm verify` passes every available check. Unavailable checks (for example Rust when it is not installed) are listed in PROGRESS.md as blockers.

## B0 — Architecture gate (weeks 1–2, research §15.8)
- **Prerequisites:** Rust toolchain plus platform build tools; aria2 binaries for each OS; access to Windows, macOS and Linux machines (or CI runners).
- **Increments:**
  1. **B0.1 — done (Windows, 2026-10-21)** `dm-backend-aria2`: spawn aria2 on loopback with a random secret, GID ↔ attempt mapping, and a capability report (ARCH-03/04, REL-05).
  2. **B0.2 — done (Windows, 2026-10-21)** `dm-service` binary: IPC endpoint (pipe/UDS, user-only), `service.v1.hello`, in-memory `downloads.v1.add/list/pause/resume/cancel` with idempotency (ARCH-06, COR-09, CORE-01/03).
  3. **B0.3 — done (Windows, 2026-10-21)** `dm-cli` harness (`dm` executable, `--json`, documented exit codes): `dm add|list|pause|resume|cancel` against the service (F01 internal).
  4. **B0.4 — done (Windows, 2026-10-21)** HTTP/HTTPS fixture matrix (27 tests) against aria2 and a correctness report (COR-01..04); [ADR 0003](decisions/0003-aria2-go-no-go.md): conditional go, `Http` supported, `Https` unverified, `SegmentedHttp` and strong-validator resume unsupported.
  4a. **B0.4.1 — done (Windows, 2026-10-25)** engine remediation: TLS backend detection and CA refusal on WinTLS, If-Match evaluation (M01–M10, not safe in general), service-owned Retry-After/backoff with discard-before-retry; [ADR 0004](decisions/0004-rust-http-backend-prototype.md).
  4b. **B0.4.2 — first increment done (Windows, 2026-10-25)** `dm-backend-http`: single-connection HTTP/HTTPS, `Http`/`Https` supported on that backend, `dm-service --backend http`; [report](reports/b0.4.2-http-backend.md).
  4c. **B0.4.2 second increment — done (Windows, 2026-10-25)** validated single-connection pause/resume on `dm-backend-http` (`ResumeStrongValidator` supported there).
  4d. **B0.4.3 — done (Windows local; CI #9 green on 4 runners, 2026-10-25)** validated segmentation on `dm-backend-http`.
  4e. **Default backend — done:** [ADR 0005](decisions/0005-default-backend.md), `dm-backend-http` is the default; aria2 opt-in.
  5a. **B0.5 — Windows done locally (2026-10-25); macOS/Linux pending the package-smoke workflow:** native packages, packaged service discovery, packaged-app smoke ([report](reports/b0.5-packaging-smoke.md)).
  6a. **B0.6 — real Chrome hand-off passed on Windows (2026-10-25); Edge/macOS/Linux real-browser runs pending:** minimal Chrome public-GET handoff spike ([report](reports/b0.6-browser-handoff.md)).
  5. **B0.5** Packaging smoke test on each OS: the Tauri bundle starts, and the sidecar launches.
  6. **B0.6** Spike: Chrome context menu → host → service for one public GET.
- **Smallest useful deliverable:** add, list, pause and resume a fixture file from the terminal, with a matching hash.
- **Acceptance / exit:** a documented go/no-go on aria2 (ADR 0003) based on the fixture report, with blockers listed for each OS.

## B1 — Reliable core (weeks 3–5)
- **Prerequisites:** B0 go decision.
- **Scope:** `dm-storage` (SQLite, migrations, single writer), attempt guards, identity validation, disk basics, restart reconciliation, completion journal, and error classification.
  Requirements: ARCH-05, CORE-02, CORE-04, CORE-11, COR-01..08, ADV05 (basic), ADV06, E03, E04.
- **Smallest useful deliverable:** kill the service mid-download, restart, and get the exact file hash or an explicit failure.
- **Increments:**
  - **B1.1 (done locally, 2026-10-25):** `dm-storage` (ADR 0006); durable jobs and idempotency records; a hard kill and restart give the same job for the same key; an unfinished job reports an explicit "interrupted" failure.
  - **B1.2 (done locally on Windows, 2026-10-26):** engine-owned durable checkpoints (ADR 0007); `http` jobs continue after a hard kill with the exact SHA-256, or fail explicitly ([report](reports/b1.2-crash-recovery.md)).
  - **B1.3 (done locally on Windows, 2026-10-26):** staged backend output and a durable completion journal (ADR 0008); a crash at every finalization boundary is reconciled to the exact file or an explicit failure ([report](reports/b1.3-completion-journal.md)).
  - **B1.4 (done locally on Windows, 2026-10-26):** advisory disk budget and recoverable `DISK_FULL`/`VOLUME_MISSING` (ADR 0009; [report](reports/b1.4-disk-budget.md)).
  - **B1.5 (done locally on Windows, 2026-10-26):** per-host connection budget (8) and active-file admission (3) (ADR 0010; [report](reports/b1.5-connection-budget.md)).
  - **B1.6 (done locally on Windows, 2026-10-26):** service-owned file naming (E04): advisory `Content-Disposition` / `filename*` names, staging vs final name, per-platform limits, collisions, `DESTINATION_UNWRITABLE` (ADR 0011; [report](reports/b1.6-file-naming.md)).
  - **B1.7 (done locally on Windows, 2026-10-26):** CORE-11 error catalogue audit, English/Bangla UI copy and situational CLI explanations; repeated kill and finalization-crash runs, forced desktop kill, `synchronous=FULL` benchmark; Linux CI test repair ([report](reports/b1.7-error-catalogue.md)). Committed as `b13e625`; CI #27 green on 4 runners and package smoke #17 green on 3 OSes: **B1 exited.**
- **Acceptance:** the fault-injection suite passes repeatedly with no silent corruption, and changed-resource fixtures stop or restart safely.

## B2 — Desktop and extension (weeks 6–8)
- **Prerequisites:** B1.
- **Scope:** the real service client in the UI, download list, rows and progress, queues, close-to-tray, and the MV3 extension with public GET capture, host install/repair and safe fallback.
  Requirements: ARCH-08/09, CORE-05/07/10, UI-02/03/04, ADV12 (basic), BRW-01/02/03/04/06/07/10, E02 (filters), SEC-03/05.
- **Smallest useful deliverable:** install → right-click a link in Chrome → file in the folder, with the UI showing progress.
- **Increments:**
  - **B2.1 (done locally on Windows, 2026-10-27):** the existing desktop UI shows real service downloads: five validated Tauri commands, polling, Add URL, selection, Stop/Resume, error copy; packaged smoke F adds a fixture through the UI ([report](reports/b2.1-desktop-downloads.md)).
  - **B2.2 (done locally on Windows, 2026-10-27):** close-to-tray setting, tray, single instance and a confirmed "Exit and stop downloads" that pauses downloads durably (ADR 0012); R01 kill barrier repaired ([report](reports/b2.2-close-and-exit.md)).
  - **B2.3 (done locally on Windows, 2026-10-27):** status icons and progress bar, confirmed Cancel download, durable "Remove from list" with tombstones (ADR 0013) ([report](reports/b2.3-status-cancel-remove.md)).
  - **B2.4 (done locally on Windows, 2026-10-27):** `dm remove` and `dm shutdown [--pause-downloads]` with a verified stop; the extension's reachability check already existed (B0.6) ([report](reports/b2.4-cli-parity.md)).
  - **B2.5 (done locally on Windows, 2026-10-28):** ownership-checked per-user registration (ADR 0014); NSIS hooks and first-launch setup; repair; uninstall removes only owned entries; packaged smoke runs real Chrome and Edge against the product registration ([report](reports/b2.5-browser-registration.md)).
  - **B2.6 (done, 2026-10-28, `61acd6d`):** CI #34 green on all four; package smoke #24 green on Windows, macOS and Linux, including R3 Chrome/Edge, R4 and D2. The Linux harness isolation repair is committed. Human H1–H11 and tray clicks are still not run, so the browser-hand-off gate and B2 are **not passed** ([report](reports/b2.6-acceptance.md)).
  - **B2.7 (done locally on Windows, 2026-10-28):** CORE-10 history search by display name and source hostname, and File → "Clear finished…" for completed, failed and cancelled jobs through the durable remove; terminal failed attempts can now be removed; packaged smoke J1–J3 (63/63) proves files byte-exact and cleared rows staying out after a restart ([report](reports/b2.7-history-search-clear.md)).
  - **B2.8–B2.11 batch (done locally on Windows, 2026-10-31):** CORE-07 duplicates, destination rules and ask-on-collision with the SEC-03 guard (ADR 0015); ARCH-08 event stream (ADR 0016); ARCH-09 attach-only reconnection, host disconnect, shutdown bound and wake re-evaluation; CORE-05 persistent queues, priority and manual order (ADR 0017); desktop, CLI and packaged smoke K ([report](reports/b2.8-b2.11-batch.md)).
  - **B2.12–B2.14 batch (done locally on Windows, 2026-10-31):** CORE-10 service-owned history retention (ADR 0018); opt-in automatic capture for chosen sites with request evidence, filters, durable state machine and cancel-after-acceptance (ADR 0019; BRW-03/04/06 implemented, BRW-10 and E02 in progress); focused accessibility checks ([report](reports/b2.12-b2.14-batch.md)).
  - **B2.15–B2.20 batch (done locally on Windows, 2026-10-31):** capture takeover synchronization (C1); explicit Retry (CORE-03) and Refresh link (COR-06) with durable operation identity (ADR 0020, migration 8); publisher SHA-256 input and presentation (COR-05); native Browse… and user-triggered Open containing folder (ADR 0021, SEC-03 guard narrowed); extension popup and context menu in English and Bangla ([report](reports/b2.15-b2.20-batch.md)).
  - **B2.15–B2.20 stabilization (done locally on Windows, 2026-11-01):** CI #43 / smoke #33 failures fixed (clippy item order; popup step 3b rewritten for the localized semantics; M3 was a harness deadlock); packaged desktop journeys N1–N3b; a verified job is never reopened; internal unsigned test build and an extended fixture page ([report](reports/b2.15-b2.20-batch.md)).
  - **B2.21–B2.23 batch (committed as `0e78942`; CI #45 failed on macOS Clippy, package smoke #35 passed on all 3 OSes):** N3b proven from the service's restart transition; release-build resource measurements (REL-04 tooling, Windows); virtualized, accessible download list for 10,000+ records with memoized rendering and bounded event batching (ADV12); service-owned completion/failure notifications (CORE-09, ADR 0022) ([report](reports/b2.21-b2.23-batch.md)).
  - **B2.24–B2.27 batch (local, Windows, 2026-11-02; committed later as `2f5ecb2`):** the macOS Clippy fix; ordered, bounded notification delivery; typed oversized IPC answers with a reported window state instead of a reconnect loop; resubscription backoff; virtualized-list removal and focus fixes; event-ordering, IPC-framing and real-window (zoom, theme, Bangla) checks; startup split and attach/restart cycles in the measurement tool ([report](reports/b2.24-b2.27-batch.md)). **B2 is not exited.**
  - **B2.28–B2.31 (2026-11-02 to 2026-11-04; committed as `4fc3d66`, `2d999b9`, `e0609bc`, `12dc3e7`):** readable failure annotations; configurable concurrency (global 1–16 plus per-queue limits, non-preemptive decreases, ADR 0023) with packaged acceptance; "What this allows" in Options ([report](reports/configurable-concurrency.md)).
  - **B2.32 (committed as `baee51f`; CI #51 and package smoke #41 green on all platforms):** the Linux P2 root cause (the details panel squeezed the list to 0 px at 200 %; fixed in CSS, assertion strengthened); the engine's per-host permit trace (the server-side 9 was a measurement artefact); `dm queue-limit`; `active_6`; the installed-app runbook and the IDM protocol.
  - **B2.33 (committed as `ee93de4`, `6866013`, `d202a60`; CI #55 and package smoke #45 green on all platforms, X1–X8 on Chrome and Edge):** packaged timeout and lost-acknowledgement rows X1–X6 through a fault-injecting helper; measurement validity (invalid workloads fail and are excluded), run-level p95, a chunk-safe fixture reader; LAN-capable fixture with server statistics; the corrected runbook and protocol; the Windows baseline and a B2 acceptance matrix ([report](reports/b2.33-closeout.md)).
  - **B2.34 (local, Windows, 2026-11-04):** an installed-app acceptance kit built and validated from the normal `baee51f` package (Windows; macOS and Linux normal packages missing); the BRW-10 scope decision prepared (proposed); ADR 0024 bandwidth limits proposed ([report](reports/b2.34-acceptance-kit.md)).
  - **B2.35 (local, Windows, 2026-11-04):** a manual native workflow for normal packages and acceptance kits on Windows, macOS and Linux (prepared, not yet run), a portable ZIP writer, delivered-ZIP validation ([report](reports/b2.35-native-kit-workflow.md)).
  - **B2.36 (2026-11-04):** the native kit workflow is still unrun (no authorized GitHub access here); B2 exit assessed: not met; ADR 0024 revised and ADR 0025 (schedules) proposed; no B3 code ([report](reports/b2.36-delivery-status-and-b3-designs.md)).
  - **Sequencing exception (2026-11-05, granted by the maintainer, narrow):** CORE-06 (speed limits) and CORE-08 (queue schedules) may be developed on the isolated branch `feature/b3.1-speed-limits-schedules` while the remaining B2 installed-app and human acceptance and the BRW-10 decision stay open. **This does not exit B2**, does not start any other B3 item, and nothing from the branch ships in a B2 release until B2 exits. Work done there: B3.1 ([report](reports/b3.1-speed-limits-schedules.md)).
  - **Extension (2026-11-05, granted by the maintainer, narrow):** F07 (Windows attachment safety), E06 (scan lifecycle states), E05 (redacted diagnostics) and the SEC-01 work they need may be developed on `feature/b3.2-download-safety-diagnostics`, stacked on the B3.1 branch. Same terms: **B2 is not exited**, no release is approved, installed-app acceptance is not waived, BRW-10 is not resolved. macOS/Linux scanning stays at its B4 milestone. Work done there: B3.2 ([report](reports/b3.2-download-safety-diagnostics.md)).
  - **Extension (2026-11-07, granted by the maintainer, narrow):** SEC-01 (secret protection across persistence and outputs), SEC-02 (OS credential store) and BRW-05 (a limited, explicitly authorized authenticated GET for Chrome and Edge with the native HTTP engine) may be developed on `feature/b3.3-secure-authenticated-downloads`, stacked on the B3.2 branch. Same terms: **B2 is not exited**, no release is approved, installed-app acceptance is not waived, BRW-10 is not resolved, private-window capture keeps its policy; Firefox, proxies, updates, media and mobile are out of scope. The maintainer extended it to reconnecting an existing signed-in download (ADR 0028) with its Retry/Resume/Refresh-link integration. Work done there: B3.3 ([report](reports/b3.3-secure-authenticated-downloads.md)); part 1 is committed as `38a3f32`, part 2 is uncommitted.
  - **Extension (2026-11-07, granted by the maintainer, narrow):** BRW-08 (Firefox: explicit public GET hand-off, connection status, registration/repair, acknowledgement, idempotency, fallback), BRW-09 (browser-package detection and truthful per-package support), and the BRW-07 and English/Bangla UI work they need may be developed on `feature/b3.4-firefox-browser-compatibility`, stacked on the B3.3 branch. Same terms: **B2 is not exited**, no release is approved, installed-app acceptance is not waived, BRW-10 is not resolved, private-window policy unchanged; Firefox sign-in, reconnect, containers and private browsing are out of scope. Work done there: B3.4 ([report](reports/b3.4-firefox-browser-compatibility.md)).
  - **Extension (2026-11-07, granted by the maintainer, narrow):** ADV09's B3 scope (verified updates with signed-version binding, coordinated service shutdown, a consistent state backup, recoverable migration failure) and the REL-01 work a safe installation needs may be developed on `feature/b3.5-signed-updates-recovery`, stacked on the B3.4 branch. Same terms: **B2 is not exited**, no release is approved, installed-app acceptance is not waived, the acceptance-kit results and BRW-10 remain release gates; production update hosting, production signing keys, release channels, SBOM and provenance are out of scope. Work done there: B3.5 ([report](reports/b3.5-signed-updates-recovery.md), [ADR 0030](decisions/0030-verified-updates-quiescence-backup-recovery.md)).
  - **Next:** the installed-app and human pass on a clean VM or test account per OS ([runbook](reports/b2.32-installed-acceptance-runbook.md)) → B2 exit → B3. B2 is not exited.
- **Acceptance:** the end-to-end test passes on all three OSes for Chrome and Edge, and missing-host and timeout fallback tests pass.

## B3 — Public beta (weeks 9–16)
- **Prerequisites:** B2.
- **Scope:** scheduler, speed limits, notifications, Windows origin marking and scan states, signed updates, credential store, authenticated GET (limited), Firefox, Bangla QA, redacted diagnostics, per-OS installers, licensing notices and benchmarks.
  Requirements: CORE-06/08/09, F07, E05, E06, ADV09 (signature), SEC-02, BRW-05/08/09, UI-01, REL-01..04.
- **Smallest useful deliverable:** a signed beta installer for one OS with a published supported-case matrix.
- **Acceptance:** the release gates in research §15.9 hold, and the support matrix lists only evidenced combinations (REL-02).

## B4 — Useful advanced desktop (MVP+ / V1)
- **Prerequisites:** B3 stable. Each item is an independent increment.
- **Scope:**
  - Public CLI (F01), clipboard (F02), drag-and-drop (F03), bulk import (F04), FTP/SFTP (F05), basic proxy (F06), finish actions (F08), RTL foundation (F15), high contrast (F16), Opera QA (E02).
  - ADV01–05, ADV07, ADV09 (channels/SBOM), ADV11, ADV12 (scale).
- **Acceptance:** acceptance evidence for each feature in traceability.

## B5 — Media and extensibility
- **Prerequisites:** B4 core items; a separate media requirements review and store-policy check (§14.4).
- **Scope:** persisted media stages, scoped helpers (E01), declarative site adapters (ADV08 part 1).
- **Acceptance:** controlled VOD fixtures; DRM content reports an explicit unsupported status.

## B6 — Optional ecosystem (V2)
- **Prerequisites:** separate requirements, ownership and effort estimates for each module.
- **Scope:** runtime plugins (ADV08), LAN companion (ADV10/F13), device sync (F14), torrent (F11), Metalink (F12), site grabber (F09), periodic sync (F10), advanced proxy and auth (F06), Safari (BRW-11).

## B7 — Mobile
- **Prerequisites:** a desktop beta in use; MOB feasibility spikes.
- **Scope:** an Android adapter (MOB-01; research estimate 6–10 extra weeks), then an iOS feasibility decision (MOB-02).
