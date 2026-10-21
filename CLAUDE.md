# CLAUDE.md

Free, account-free, cross-platform download manager (Windows, macOS, Linux and browser extensions).
Its focus is large files and unreliable networks: correct resume, clear failure reasons, and honest support claims.

**Phase:** Foundation is complete. The next milestone is **B0 (architecture gate)**. The authoritative current state is
in [PROGRESS.md](PROGRESS.md). Read it first.

## Authoritative documents
| Topic | File |
|---|---|
| Current state, blockers, next task | [PROGRESS.md](PROGRESS.md) |
| Requirements (IDs, acceptance, status) | [requirements.md](requirements.md) |
| Design system (tokens, components, states) | [design.md](design.md) |
| Architecture and contracts | [docs/architecture.md](docs/architecture.md) |
| Directory layout, dependency directions | [docs/project-structure.md](docs/project-structure.md) |
| Milestones | [docs/roadmap.md](docs/roadmap.md) |
| Requirement → code → evidence | [docs/traceability.md](docs/traceability.md) |
| Test strategy | [docs/testing.md](docs/testing.md) |
| Decisions | [docs/decisions/](docs/decisions/) |
| Setup | [docs/setup.md](docs/setup.md) |
| Source research (read-only, Bangla) | [docs/research/download-manager-rnd-bn.md](docs/research/download-manager-rnd-bn.md) |

Do not load the whole research document by default. Open the cited section (`§N`) when a requirement needs it.
Later sections (§14–§19) correct earlier ones.

## Architecture boundaries (summary; details in docs/architecture.md)
- Processes: React UI (Tauri webview) → Tauri shell → `dm-service` (per-user Rust process) → `DownloadBackend` → aria2 sidecar. The CLI and the browser native host use the same JSON-RPC contract.
- File bytes flow origin → engine → disk only. The UI presents state and sends commands.
- UI or browser closure is not service shutdown. The service is the single SQLite writer, and the engine owns segment state.
- Job and attempt IDs belong to the service. aria2 GIDs never leave the adapter. Capabilities are reported explicitly, with no silent fallback.
- Manual pause (`desired_state`) is separate from policy blockers. Secrets never appear in logs or exports.
- Contracts: the canonical schema is `packages/contracts/schema/service-api.v1.schema.json`. TypeScript is generated from it, and Rust mirrors it in `crates/domain/src/api.rs`.
- Major decision changes require a new ADR.

## Commands (all exist; see docs/setup.md)
```text
pnpm install            # install JS deps (pnpm 12.14.0; `npx pnpm@12.14.0` works too)
pnpm verify             # every available check; reports PASSED / FAILED / UNAVAILABLE
pnpm verify:strict      # UNAVAILABLE counts as failure (CI)
pnpm verify -- --only=lint,typecheck,test   # groups: structure,contracts,format,lint,typecheck,test,build,rust
pnpm dev                # UI in a browser at :1420 (no service)
pnpm dev:desktop        # Tauri window (needs Rust)
pnpm contracts:gen      # regenerate TS types after a schema change
pnpm check:structure    # links, frontmatter, boundaries, traceability
pnpm format             # Prettier
cargo test --workspace  # Rust tests (needs Rust toolchain)
```

## Skills and rules
Pick the skill matching the work and follow it:
- `dm-engine`: service, aria2 backend, storage, IPC, CLI, recovery, contracts.
- `dm-ui`: desktop React UI, Tauri shell, tokens, i18n, accessibility.
- `dm-browser`: extension, native host, capture, registration, fallback.
- `dm-verify`: before reporting completion; status and evidence updates.

`.claude/rules/` is loaded automatically. Path-scoped rules apply to Rust, the UI and browser code.

## Required workflow
**Before changing code**
1. Read this file, `PROGRESS.md`, the applicable rules and the relevant skill.
2. Identify the requirement IDs and their acceptance criteria in `requirements.md`.
3. Inspect the existing implementation. Confirm the owning module (`docs/project-structure.md`) and the design pattern.

**During**
1. Complete one coherent increment.
2. Reuse existing components, tokens and contracts.
3. Preserve unrelated behaviour.
4. Stay within the module boundaries.
5. Add verification that would fail without the change.

**Before reporting completion**
1. Run the appropriate checks (`pnpm verify` and targeted tests).
2. Fix failures your change caused.
3. Update the affected docs, `requirements.md` statuses and `docs/traceability.md` evidence.
4. Update `PROGRESS.md`: the work done, the commands run with results and OS, blockers, and the exact next task.
5. Report what changed, what was verified, and what remains unverified or blocked.

Statuses are `documented` → `in-progress` → `implemented` → `verified`. `verified` needs recorded evidence.
UNAVAILABLE or unrun checks are never reported as passing. OS support is claimed only where a check ran.

## Preserving user work
- Run `git status` before editing. Never discard, overwrite or reformat uncommitted changes you did not make.
- Never edit `docs/research/` (a verbatim copy of the source research).
- Do not commit, push or install system-wide tools unless the user asks.
- Ask before destructive operations (deleting files or branches, resetting).

## After a context reset
Recover the state from `PROGRESS.md`, then `docs/traceability.md`, then `git status` and `git log`. Do not rely on memory of earlier sessions.
