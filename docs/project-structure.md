# Project Structure

## Canonical layout

Directories marked *(planned)* do not exist yet. Create them only when their milestone starts. Do not add
empty placeholder implementations.

```text
.
├── CLAUDE.md, PROGRESS.md, README.md   # entry points (session, handoff, humans)
├── requirements.md, design.md          # canonical requirements and design system
├── apps/
│   ├── desktop/                        # @dm/desktop — React UI + Tauri shell
│   │   ├── src/
│   │   │   ├── app/                    # shell, routing/views, providers (preferences)
│   │   │   ├── features/<feature>/     # one folder per feature (downloads, settings, queues…)
│   │   │   ├── ui/                     # shared primitives + tokens.css/base.css
│   │   │   ├── services/               # service-client boundary (only place touching Tauri)
│   │   │   ├── i18n/                   # message catalogues (en, bn)
│   │   │   └── test/                   # test setup
│   │   └── src-tauri/                  # Rust shell crate `dm-desktop`
│   └── extension/                      # (planned B2) @dm/extension — MV3 WebExtension
├── crates/
│   ├── domain/                         # dm-domain — types, policies, redaction, API wire types
│   ├── backend-api/                    # dm-backend-api — DownloadBackend trait + transfer types (ADR 0002)
│   ├── service/                        # dm-service — coordinator lib + `dm-service` binary (B0.2)
│   ├── backend-aria2/                  # (B0.1) dm-backend-aria2 — aria2 sidecar adapter
│   ├── backend-http/                   # (B0.4.2) dm-backend-http — Rust HTTP/HTTPS backend prototype (ADR 0004)
│   ├── test-fixtures/                  # dm-test-fixtures — controlled HTTP/HTTPS fixture server + test PKI (dev-dependency only)
│   ├── storage/                        # (planned B1) dm-storage — SQLite + migrations
│   ├── platform/                       # dm-platform — paths, endpoint discovery, local IPC (B0.2); more OS adapters later
│   ├── browser-host/                   # (planned B2) dm-browser-host — native messaging exe
│   └── cli/                            # dm-cli — `dm` executable (B0.3)
├── packages/
│   └── contracts/                      # @dm/contracts — canonical schema, generated TS, examples
├── tests/fixtures/                     # controlled protocol and failure fixtures (B0+)
├── scripts/                            # verify, structure checks, contract generation
├── docs/                               # architecture, roadmap, testing, traceability, ADRs, research
└── .claude/                            # rules/ and skills/ for Claude Code sessions
```

Future modules (B5/B6), created only with their milestone and an ADR if they add a process:
`crates/media` (E01), `crates/integrations` (ADV08), `apps/companion` (ADV10), and mobile adapters (B7).

## Dependency directions

Rust (`dm-*` crates). Enforced by `scripts/check-structure.mjs`.

| Crate | May depend on | Must not depend on |
|---|---|---|
| `dm-domain` | (no `dm-*`) | everything else; no I/O, Tauri, SQLite or aria2 |
| `dm-backend-api` | `dm-domain` | everything else; trait and types only, no engine code |
| `dm-storage` | `dm-domain` | backend, platform, service |
| `dm-backend-aria2` | `dm-domain`, `dm-backend-api` | storage, service, platform |
| `dm-backend-http` | `dm-domain`, `dm-backend-api` | storage, service, platform |
| `dm-test-fixtures` | (no `dm-*`) | everything; dev-dependency of backend and service tests only |
| `dm-platform` | `dm-domain` | storage, backend, service |
| `dm-service` | `dm-domain`, `dm-backend-api`, `dm-storage`, `dm-backend-aria2` and `dm-backend-http` (construction only), `dm-platform` | desktop, cli, browser-host |
| `dm-cli` | `dm-domain`, `dm-platform` (IPC client) | service internals, storage, backend |
| `dm-browser-host` | `dm-domain`, `dm-platform` (IPC client) | service internals, storage, backend |
| `dm-desktop` | `dm-domain`, `dm-platform` (IPC client) | service internals, storage, backend |

Only `dm-backend-aria2` may reference aria2. Engine adapters implement `dm-backend-api` and never depend on `dm-service` (ADR 0002). Clients (CLI, browser host, desktop) never depend on `dm-backend-api`; they reach the service **only** through IPC.

TypeScript:

| Package / folder | May import | Must not import |
|---|---|---|
| `packages/contracts` | nothing internal | any app |
| `apps/desktop/src/services/` | `@dm/contracts`, `@tauri-apps/*` | features |
| `apps/desktop/src/features/*` | `app/`, `ui/`, `services/` (interfaces), `i18n/`, `@dm/contracts` | `@tauri-apps/*`, other features' internals |
| `apps/desktop/src/ui/` | React, `lucide-react` | features, services |
| `apps/extension` | `@dm/contracts` | `apps/desktop` |

The checks enforce this as follows. ESLint `no-restricted-imports` blocks `@tauri-apps/*` outside `services/`. The structure check blocks relative imports that cross packages.

## Naming conventions

- Rust crates use `dm-<area>` (kebab-case) and their modules use snake_case. Each type gets one module when it is substantial.
- TypeScript packages are named `@dm/<name>`.
  - React component files are PascalCase (`DownloadsView.tsx`), and each has a co-located `Component.css`.
  - Hooks and utilities are camelCase.
  - Tests are named `*.test.ts(x)` and sit next to the code they test.
- CSS classes use BEM-like naming (`block__element--modifier`). Values come only from `tokens.css` variables.
- Requirement IDs, ADR numbers (`NNNN-kebab-title.md`) and contract method names (`area.vN.verb`) are stable.

## Adding a module or dependency

1. Identify the requirement IDs and the owning module in this table. Do not create a new top-level directory without updating this file.
2. A new crate goes into the root `Cargo.toml` members, the dependency table above, and `ALLOWED_DM_DEPS` in `scripts/check-structure.mjs`.
3. Add new third-party dependencies only for a real requirement:
   - prefer the standard library or an existing dependency;
   - pin the exact version and verify it against the official documentation or registry;
   - check its license is compatible (record GPL/LGPL/AGPL in ADR or REL-03 notes);
   - commit the lockfile.
4. A new process, IPC surface or persistent store requires an ADR.
