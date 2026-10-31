# ADR 0015 — Duplicate links, destination rules and ask-on-collision

- **Status:** Accepted
- **Date:** 2026-10-31
- **Builds on:** ADR 0006 (storage), ADR 0008 (completion journal), ADR 0011 (file naming), ADR 0013 (remove from list)
- **Amends:** ADR 0011 §8 (a placement race now picks the next free name instead of failing)
- **Requirements:** CORE-07, COR-09, SEC-03
- **Evidence:** [B2.8–B2.11 batch report](../reports/b2.8-b2.11-batch.md)

## Context

CORE-07 asks for three things:
- duplicate detection that shows the existing job before another is created;
- destination folders per file category;
- an explicit choice when a file name is taken, without ever overwriting.

Two existing guarantees must hold:
- **Idempotency (COR-09).** A retry with the same key returns the original job, so duplicate
  detection must not replace it.
- **ADR 0011 naming.** Staging names are fixed at acceptance and bound to checkpoints and journal
  entries, so rules and collisions must not move partial files.

## Decision

1. **Link identity.** `dm_domain::policy::link_identity` is the exact URL with only the scheme and
   host lower-cased.
   - The path, query (signed tokens included), fragment and userinfo are kept byte for byte, so
     different signatures or path case never match.
   - Redacted URLs are never compared.
   - The identity is used only in memory and is never logged, shown or returned.
2. **Check, then a race-safe add.**
   - `downloads.v1.check` returns the unremoved jobs with the same identity, oldest first, as
     `DuplicateSummary` items (job ID, display name, phase, desired state; never the URL).
   - `downloads.v1.add` gains an optional `on_duplicate` (`allow` | `refuse`). Omitting it means
     `allow`, the behaviour before B2.8, so the CLI, the browser host and older clients are
     unchanged.
   - `refuse` is decided under the coordinator lock, after the idempotency replay, so two
     concurrent submissions of one link cannot both pass. The refusal is `DUPLICATE_DOWNLOAD`
     (not retryable) and leaves no job and no idempotency record.
   - The desktop always checks first and offers three choices:
     - **Show existing download** selects the existing job.
     - **Download again** sends a new request: a new key with `on_duplicate: allow`.
     - **Cancel** closes the dialog.

     Its first add uses `refuse`; a `DUPLICATE_DOWNLOAD` answer (another client was faster)
     re-checks and shows the prompt.
3. **Fingerprints stay compatible.** The new optional add fields (`category`, `on_duplicate`,
   `queue_id`) join the idempotency fingerprint only when present. Every retained key from before
   B2.8 keeps its exact fingerprint.
4. **Destination rules.**
   - `settings.v1.get`/`set` hold a category → absolute folder mapping (`compressed`, `documents`,
     `music`, `programs`, `video`, `other`) and the collision policy.
   - The service validates each folder: absolute, existing, a directory, no `.`/`..` components.
     The Tauri shell checks only the shape, and browser messages cannot set folders at all.
   - The folder is resolved once, at acceptance, from the job's category (given, or derived from
     the provisional name) and stored with the job.
   - **Editing a rule never moves an existing job**, its partial file, its checkpoint or its journal
     entry.
5. **Collisions.**
   - `on_collision: rename` (the default) keeps ADR 0011: the next free name is used.
   - `ask` holds a job whose chosen final name had to change before placement. It waits with the
     verified staging file and the `name_conflict` blocker until `downloads.v1.confirm_name`
     (or cancel).
   - Under both policies nothing is overwritten.
   - **Placement race (amends ADR 0011 §8).** If another file takes the final name after it was
     chosen, the next free name is journaled and placement retries, at most 8 times, then fails
     with `DESTINATION_UNWRITABLE`. Under `ask` the new name is asked about again.
6. **Storage.** Migration 6 (forward-only, shared by the batch) adds:
   - `destination_rules` and `settings`;
   - `jobs.category` (NULL for older jobs) and `jobs.name_confirmed`.

   Existing jobs keep their stored destination.
7. **SEC-03.** (Narrowed by [ADR 0021](0021-user-triggered-folder-reveal.md): one user-triggered folder-reveal adapter.)
   - Completion and recovery never open, execute or associate a downloaded file.
   - `crates/service/tests/sec03_no_execution.rs` enforces this structurally:
     - only an allowlist of source files may start a process, each for a fixed program that is
       never a download (the engine, `reg.exe`, Downova's own executables), and none of them is a
       completion or recovery path;
     - no crate depends on an opener or shell crate or Tauri's shell/opener plugins, and the
       desktop capability grants none;
     - no source names the OS "open this file" entry points.
   - It does not check file permissions: placement copies or links bytes and never sets an
     execute bit, but no test asserts that.

## Consequences

- The UI adds one round trip (check) before an add; a refused duplicate costs nothing.
- An older client never sees `DUPLICATE_DOWNLOAD`: it never sends `refuse`.
- A job with the `ask` policy can wait indefinitely for a decision; it holds no transfer slot.
- Not covered:
  - duplicates by content (hash);
  - mirrors of one file under different URLs;
  - per-site rules;
  - a native folder picker (folders are typed as paths).
