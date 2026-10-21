# Architecture and dependency boundaries

Canonical sources: `docs/architecture.md` and `docs/project-structure.md`. Do not restate them; follow them.

Non-negotiable invariants:
- File bytes flow origin server → backend engine → local disk. No IPC, native-messaging message, Tauri command or React state carries file content.
- The React UI presents service state and sends commands. It never opens SQLite, talks to aria2 or holds secrets.
- Closing the UI or the browser is not service shutdown. Only an explicit stop ends transfers.
- `dm-service` is the only SQLite writer. Engine-owned resume and segment state is not duplicated in SQLite.
- Job IDs (`job_…`) and attempt IDs (`att_…`) belong to the service. aria2 GIDs never leave `dm-backend-aria2`.
- The engine seam is `dm-backend-api` (ADR 0002): `dm-service` and engine adapters depend on it, adapters never depend on `dm-service`, and clients never depend on it.
- Backends report every capability as supported, unsupported or unverified. Features refuse anything that is not `supported`, with `BACKEND_CAPABILITY_UNAVAILABLE`. There are no silent fallbacks.
- Manual user pause (`desired_state`) stays distinct from automatic policy blockers.
- Cookies, tokens, Authorization headers and proxy passwords never reach ordinary logs, events, history or exports.
- Clients (UI, CLI, browser host) use the one contract in `packages/contracts`. A schema change means regenerating TypeScript and updating `crates/domain/src/api.rs` together.
- Crate and package dependencies follow the table in `docs/project-structure.md`. `pnpm check:structure` enforces it.
- Replacing a major decision (engine, IPC, storage, UI stack, contract source) requires an ADR.
