# ADR 0002 — Backend interface in its own crate; product name Downova

- **Status:** Accepted
- **Date:** 2026-10-21
- **Supersedes:** part of ADR 0001 decision 3 (the location of `DownloadBackend`). Also resolves the product-name and bundle-identifier assumptions in ADR 0001.

## Context

1. The foundation placed the `DownloadBackend` trait in `dm-service` (`crates/service/src/backend.rs`). The dependency rules allow `dm-service → dm-backend-aria2`, but forbid `dm-backend-aria2 → dm-service`. An adapter could therefore never implement the trait without creating a cycle (`service → aria2 → service`).
2. The product name was a placeholder ("Download Manager"), and the bundle identifier `dev.downloadmanager.desktop` was marked as temporary.

## Decision

1. **The backend interface moves into a new crate, `dm-backend-api` (`crates/backend-api`).** It contains `DownloadBackend`, `TransferRequest`, `TransferState`, `BackendProgress`, `BackendHandle`, `BackendError` and `require_capability`.
   - It depends only on `dm-domain`.
   - `dm-service` and every engine adapter depend on it, and never on each other's internals.
   - The resulting graph is acyclic: `dm-service → dm-backend-api ← dm-backend-aria2`, with `dm-service → dm-backend-aria2` used only to construct the adapter.
   - `dm-service` re-exports the API types for convenience.
2. **Why not `dm-domain`?** `dm-domain` is reserved for pure types and policies shared by every client (UI, CLI, host). The backend interface is an engine-side seam that clients must never use, so keeping it out of `dm-domain` prevents clients from reaching engine concepts.
3. **The product name is Downova.** It is used in all product-facing metadata: window title, Tauri `productName`, UI copy, the About dialog and the README. Internal package and crate prefixes (`@dm/*`, `dm-*`) stay unchanged because they are not user-facing.
4. **The bundle identifier is `com.downova.desktop`.** It is **stable from now on**: it determines OS app-data paths, the Windows AppUserModelID and native-host registration. Changing it after the first public release requires a migration ADR.

## Consequences

- `docs/project-structure.md`, `scripts/check-structure.mjs` (`ALLOWED_DM_DEPS`), `.claude/rules/architecture.md` and `docs/architecture.md` §4 have been updated.
- The aria2 go/no-go decision that the roadmap previously called "ADR 0002" becomes **ADR 0003**.
- If the `com.downova` reverse-DNS name is ever needed for signing or store listings, ownership of the matching domain must be confirmed. This is recorded as an open item in PROGRESS.md.
