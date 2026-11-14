# Downova

A free, account-free, cross-platform download manager for Windows, macOS and Linux, with Chrome, Edge and Firefox
integration. It is built for large files and unreliable internet: safe resume, clear failure reasons, and no ads,
tracking or login.

> **Status: internal beta candidate. Not released.** Milestone B2 (desktop and extension) is not formally exited.
> Most B3 beta features are implemented and exercised by automated checks on all three target systems. Packages
> are unsigned internal test builds. Release gates are still open: human acceptance, licensing, signing and
> distribution (see below). The current state, the evidence and the exact next task are in
> [PROGRESS.md](PROGRESS.md).

## What exists today

| Part | State |
|---|---|
| Download engine (`crates/backend-http`) | Default native Rust HTTP/HTTPS engine ([ADR 0005](docs/decisions/0005-default-backend.md)): rustls with the platform verifier, manual redirects with a per-hop gate, segmented transfers, durable resume from checkpoints validated with `If-Range`, per-region SHA-256, speed limits. aria2 is an explicit, optional sidecar (`--backend aria2`, [ADR 0003](docs/decisions/0003-aria2-go-no-go.md)) |
| Background service (`crates/service`) | `dm-service`, one per user. It outlives the window and the browser, and is the only SQLite writer (`crates/storage`, migrations, consistent backups). It recovers interrupted downloads from checkpoints after a crash or restart. Global and per-queue concurrency limits, queues, schedules, manual pause kept apart from policy blockers, completion notifications, redacted diagnostics |
| Secrets | Protected link parts and checkpoints are sealed with a data key kept in the OS credential store: Windows Credential Manager, macOS Keychain, Linux Secret Service ([ADR 0027](docs/decisions/0027-secret-lifecycle-vault-and-authenticated-get.md)). Store calls are bounded; a store that does not answer is treated as locked, and nothing is lost ([ADR 0032](docs/decisions/0032-bounded-credential-store-calls.md)) |
| Desktop app (`apps/desktop`) | Tauri 2 + React window connected to the service: download list, queues and limits, options, signed-in downloads, browser connections, third-party notices, English/Bangla, light/dark. Verified in-app updates exist but are tested only with a disposable fixture key; there is no production update channel |
| Browser integration (`apps/extension`, `crates/browser-host`) | Manifest V3 extension for Chrome and Edge (development ID) and Firefox (unsigned, temporary loading only): context-menu hand-off, optional download capture, signed-in hand-off with scoped cookies. The native messaging host validates the sender and forwards to the service; it never transfers file bytes. Registration repair and removal touch only Downova's own entries |
| CLI (`crates/cli`) | `dm`: add, list, get, pause, resume, cancel, remove; concurrency, queues and schedules; credentials; diagnostics; shutdown. `--json` and documented exit codes |
| Contracts (`packages/contracts`) | One JSON Schema for the service API; TypeScript is generated from it and Rust mirrors it |
| Windows safety (`crates/win-attachment`) | Windows Attachment Services for finished files: origin marking and the policy and antivirus scan states ([ADR 0026](docs/decisions/0026-attachment-safety-scan-states-and-diagnostics.md)) |

## Evidence and remaining gates

- **Automatic on every push to `main` (seven jobs):** verification on windows-latest, macos-latest, ubuntu-22.04
  and ubuntu-24.04, and package smoke tests for windows-x86_64, macos-aarch64 and linux-x86_64 (Ubuntu 22.04
  baseline).
- **Manual workflows:** acceptance kits (normal packages for testers) and the installer lifecycle (real installs,
  upgrades and removals on disposable runners). Their latest runs are recorded in [PROGRESS.md](PROGRESS.md).
- **Not yet done:**
  - human acceptance on installed builds (real right-click, tray, notifications, Bangla review, screen readers,
    display scaling, sleep, logout and reboot);
  - macOS continuation across versions with a Developer ID signature;
  - the project license and the remaining dependency notices;
  - production signing, notarization and the update channel;
  - extension store distribution;
  - benchmarks against other download managers.
- Support claims are limited to what has evidence ([docs/traceability.md](docs/traceability.md)).

## Prerequisites

- Node.js ≥ 22.12 and pnpm 12.14.0 (`npx pnpm@12.14.0` works too).
- Rust (the version pinned in `rust-toolchain.toml`, via rustup) with the platform build tools for Tauri.

Full details are in [docs/setup.md](docs/setup.md).

## Commands

```bash
pnpm install
```

```bash
pnpm verify
```

| Command | Purpose |
|---|---|
| `pnpm dev:desktop` | Run the desktop window; it starts the service |
| `pnpm dev` | Run the UI alone in a browser at http://localhost:1420 (no service) |
| `pnpm verify` / `pnpm verify:strict` | All available checks / fail on unavailable ones |
| `pnpm lint`, `pnpm typecheck`, `pnpm test`, `pnpm format` | Individual checks |
| `pnpm contracts:gen` | Regenerate contract types after a schema change |
| `cargo test --workspace` | Rust tests |
| `cargo run -p dm-service -- --download-dir DIR` | Start the per-user service by hand |
| `cargo run -p dm-cli --quiet -- list` | The `dm` CLI (see `-- --help`) |
| `pnpm fetch:aria2` | Windows: fetch the pinned aria2 1.37.0 (SHA-256 checked) for the optional aria2 backend |

## Continuing development

1. Read [CLAUDE.md](CLAUDE.md) and [PROGRESS.md](PROGRESS.md). PROGRESS.md names the exact next task.
2. Find the requirement in [requirements.md](requirements.md) and the owning module in
   [docs/project-structure.md](docs/project-structure.md).
3. Follow the workflow in CLAUDE.md, verify with `pnpm verify`, and update traceability and progress.

## Documentation

[Architecture](docs/architecture.md) · [Design system](design.md) · [Requirements](requirements.md) ·
[Roadmap](docs/roadmap.md) · [Testing](docs/testing.md) · [Traceability](docs/traceability.md) ·
[Acceptance checklist](docs/acceptance/b2-checklist.md) · [Decisions](docs/decisions/) ·
[Research (Bangla)](docs/research/download-manager-rnd-bn.md)

## License

Not yet decided. The options are in [docs/decisions/proposals/project-license.md](docs/decisions/proposals/project-license.md);
dependency notices are generated and checked (REL-03), and a few still need the owner's decision.
