---
name: dm-engine
description: Use when implementing or changing the background download service, the aria2 backend adapter, SQLite storage, job lifecycle, retries, resume/identity validation, capability checks, IPC/service API, CLI, or crash recovery in crates/ (dm-domain, dm-service, dm-backend-aria2, dm-storage, dm-platform, dm-cli).
---

# dm-engine: service, backend, persistence and recovery

## 1. Trigger and scope
Use this skill for any work in `crates/domain`, `crates/service`, `crates/backend-aria2`, `crates/storage`, `crates/cli` and the IPC parts of `crates/platform`, and for changes to `packages/contracts` that the service implements. For UI, use `dm-ui`. For the extension and native host, use `dm-browser`.

## 2. Consult first
- `PROGRESS.md` for the current milestone and the next task.
- `docs/architecture.md` §2–§7 (processes, backend trait, API, job/attempt model, SQLite ownership).
- `requirements.md`: ARCH-*, CORE-*, COR-*, ADV05/06, SEC-01, F01.
- `docs/testing.md` §1–§3 and §5 (fixtures, recovery, lifecycle).
- `.claude/rules/rust.md` and `.claude/rules/architecture.md`.

## 3. Inputs and boundaries
- **Inputs:** the requirement IDs and acceptance criteria, the affected contract methods, and the backend capabilities involved.
- **Boundaries:**
  - bytes never leave the engine → disk path;
  - the service is the single SQLite writer;
  - GIDs stay in the adapter;
  - secrets are `Secret`;
  - no aria2 option passthrough;
  - `dm-domain` stays pure.
- If a required behaviour cannot be guaranteed by aria2 (for example If-Range resume), do not paper over it. Report the capability as `unsupported` and record the finding for the B0 go/no-go ADR.

## 4. Workflow
1. Write or extend the failing test first: a domain unit test, a contract example, or an HTTP fixture under `tests/fixtures/`.
2. Model state with `DesiredState`, `Phase` and `Blocker`, and go through `decide_run`. Never fold pause and policy into one flag.
3. For new API surface: change the schema, run `pnpm contracts:gen`, update `crates/domain/src/api.rs`, and add valid and invalid examples.
4. Implement in the owning crate (per the `docs/project-structure.md` table). Classify errors into `ErrorKind` at the boundary.
5. Persistence: schema changes go through forward-only migrations. Write durable checkpoints at state changes, never per packet.
6. Recovery paths: define what happens after a crash at each step (journal stage) and test at least the step you added.

## 5. Verification
- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.
- `pnpm contracts:check` when contracts change.
- For protocol or recovery work: run the relevant fixtures and compare the SHA-256 of completed files. Kill tests for crash paths.
- If cargo or aria2 is unavailable: state it, and keep the requirement `in-progress`, not `implemented`.

## 6. Documentation and progress
Update `requirements.md` statuses and `docs/traceability.md` rows (implementation path, evidence). Update `docs/architecture.md` if a contract, state or lifecycle changed. Write an ADR for engine or IPC or storage decisions. Update `PROGRESS.md`.

## 7. Completion evidence
List the tests added (names), the commands run with results and the OS, the fixtures exercised, capability report changes, and any behaviour proven unsafe or unsupported.
