# Downova

A free, account-free, cross-platform download manager for Windows, macOS and Linux, with Chrome/Edge (then
Firefox) integration. It is built for large files and unreliable internet: safe resume, clear failure reasons, and no
ads, tracking or login.

> **Status: B0 in progress.** Foundation verified on Windows; the aria2 backend adapter (B0.1)
> passes its fixture tests. **The app still cannot download from the UI**: the background service
> process and IPC are next (B0.2). See [PROGRESS.md](PROGRESS.md).

## What exists today

| Part | State |
|---|---|
| Desktop UI (`apps/desktop`) | IDM-style window (menu bar, toolbar, categories, list, status bar), Options/About dialogs, English/Bangla, light/dark. Shows honestly that the service is not connected. 16 vitest tests |
| Tauri shell (`apps/desktop/src-tauri`) | Builds and opens the Downova window on Windows (debug smoke test). No custom commands yet |
| Contracts (`packages/contracts`) | Canonical JSON Schema, generated TypeScript, examples checked in TS and Rust |
| `crates/domain` | IDs, byte counts, pause-vs-policy state, error kinds, capabilities, redaction, wire types. Tested |
| `crates/backend-api` | `DownloadBackend` trait, transfer types, capability gating (ADR 0002) |
| `crates/backend-aria2` | aria2 1.37.0 sidecar adapter: loopback RPC with random secret, attempt↔GID mapping, capability report. 8 fixture tests ([report](docs/reports/b0.1-aria2-fixtures.md)) |
| `crates/service` | Coordinator library (hello result). Process + IPC arrive in B0.2 |
| Extension, native host, CLI, storage | Planned (see [docs/roadmap.md](docs/roadmap.md)) |

## Prerequisites

- Node.js ≥ 22.12 and pnpm 12.14.0.
- Rust 1.99.1 (via rustup) with the platform build tools for Tauri. This is needed for the Rust crates and the desktop window.

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
| `pnpm dev` | Run the UI in a browser at http://localhost:1420 |
| `pnpm dev:desktop` | Run the Tauri desktop window (needs Rust) |
| `pnpm verify` / `pnpm verify:strict` | All available checks / fail on unavailable ones |
| `pnpm lint`, `pnpm typecheck`, `pnpm test`, `pnpm format` | Individual checks |
| `pnpm contracts:gen` | Regenerate contract types |
| `cargo test --workspace` | Rust tests |
| `pnpm fetch:aria2` | Windows: fetch the pinned aria2 1.37.0 (SHA-256 checked) for the aria2 fixture tests |

## Continuing development

1. Read [CLAUDE.md](CLAUDE.md) and [PROGRESS.md](PROGRESS.md). PROGRESS.md names the exact next task.
2. Find the requirement in [requirements.md](requirements.md) and the owning module in [docs/project-structure.md](docs/project-structure.md).
3. Follow the workflow in CLAUDE.md, verify with `pnpm verify`, and update traceability and progress.

## Documentation

[Architecture](docs/architecture.md) · [Design system](design.md) · [Requirements](requirements.md) ·
[Roadmap](docs/roadmap.md) · [Testing](docs/testing.md) · [Traceability](docs/traceability.md) ·
[ADR 0001](docs/decisions/0001-project-foundation.md) · [Research (Bangla)](docs/research/download-manager-rnd-bn.md)

## License

Not yet decided. aria2 (GPL-2.0-or-later) obligations and the dependency audit come first (REL-03).
