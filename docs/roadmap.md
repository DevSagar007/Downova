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
  - **Next, in order:** human H1–H11 and tray checks on each reachable OS → B2.8 CORE-07 duplicates and destination rules (+ SEC-03 guard) → B2.9 ARCH-08 event stream → B2.10 ARCH-09 remaining lifecycle cases → B2.11 CORE-05 queues and priority ([audit](reports/b2.6-acceptance.md#4-b2-requirement-audit)).
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
