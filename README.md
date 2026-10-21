# Download Manager (working title)

A free, account-free, cross-platform download manager for Windows, macOS and Linux, with Chrome/Edge (then
Firefox) integration. It is built for large files and unreliable internet: safe resume, clear failure reasons, and no
ads, tracking or login.

> **Status: foundation phase.** The repository contains the architecture, requirements, design system,
> contracts, a desktop UI shell and domain types. **It cannot download anything yet.** The
> background service and the aria2 backend are the next milestone (B0). See [PROGRESS.md](PROGRESS.md).

## What exists today

| Part | State |
|---|---|
| Desktop UI (`apps/desktop`) | React shell with Downloads and Settings views, English/Bangla, light/dark. It shows honestly that the service is not connected. Tested with vitest |
| Tauri shell (`apps/desktop/src-tauri`) | Minimal window configuration, no commands. **Not yet compiled** in this environment |
| Contracts (`packages/contracts`) | Canonical JSON Schema, generated TypeScript, validated examples |
| `crates/domain` | IDs, byte counts, state policy (pause vs policy), error kinds, capabilities, redaction, wire types. **Not yet compiled** |
| `crates/service` | `DownloadBackend` trait, capability gating, hello result. **Not yet compiled** |
| Extension, native host, CLI, storage, aria2 backend | Planned (see [docs/roadmap.md](docs/roadmap.md)) |

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
