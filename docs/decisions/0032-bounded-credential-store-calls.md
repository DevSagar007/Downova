# ADR 0032 — The service never waits indefinitely for the OS credential store

- **Status:** Accepted (2026-11-09). **B2 is not exited.**
- **Requirements:** SEC-02 (OS credential store), ARCH-01 (the service outlives the UI and serves), REL-01 (upgrade)
- **Amends:** [ADR 0027](0027-secret-lifecycle-vault-and-authenticated-get.md) (the data key is read at startup)
- **Evidence:** [beta readiness report §9](../reports/beta-candidate-readiness.md), installer lifecycle runs
  [42566142231](https://github.com/DevSagar007/Downova/actions/runs/42566142231) and
  [42568238477](https://github.com/DevSagar007/Downova/actions/runs/42568238477)

## Context

The first remote installer-lifecycle runs on macOS failed row A2:
- **What happened:** version A was replaced by version B with A's service stopped, and B's `dm-service` stayed
  alive. Every `dm` call answered "service is not running" for four minutes, and the service wrote nothing.
- **Why:** at startup `credentials::startup` reads (or creates) the data key from the OS store *before* the IPC
  endpoint is bound, and that read is synchronous. Both builds are ad-hoc signed and have different signatures,
  so the Keychain asks whether B may read the item A created. No one answers that prompt on a CI runner, so the
  read blocks and the service never serves.
- **Real users:** the same happens when a user leaves such a prompt unanswered, or when the store blocks for any
  other reason (a locked Secret Service collection waiting for its unlock dialog).
- **Windows and Linux:** Windows Credential Manager and the CI Secret Service have no per-binary access control,
  so N1–N6, M1–M4 and D1–D4 passed.

## Decision

- **Time limit:** every OS store call the service makes during startup, and the on-demand data-key call, runs on
  its own thread and is waited for at most `credentials::STORE_WAIT` (10 s). The call is left to finish on its
  thread, and its late result is discarded.
- **No answer is a locked store:** in that case `store_locked` is set and the problem is recorded ("did not
  answer within 10 s"). This is the existing locked-store path:
  - sealed links and checkpoints stay in SQLite for a later start, and nothing is re-sealed or deleted;
  - remembered sign-ins read `Locked`;
  - protected parts of new links stay in memory only;
  - public downloads that have no sealed progress continue.
- **Sealed progress under a locked store:** a download whose checkpoint is sealed and cannot be opened (no key
  loaded) is held as `CREDENTIAL_STORE_UNAVAILABLE` / Retry in memory only, like a sealed link. It is never
  failed as "interrupted" or restarted from zero, and the stored row and checkpoint are what the next start with
  the key continues from. Rerun 42589351965 showed this was needed: the service then started, but the running
  download was failed for good.
- **Later calls:** after one call goes unanswered, startup skips the remaining store calls. A pending credential
  row is then kept rather than deleted, so a later start removes both the row and its secret: no unmanaged secret
  is left.
- **Retrying:** `ensure_data_key` tries again on later demand, also bounded, so the key comes back once the user
  allows access (with "Always Allow", without a restart).

## Consequences

- The service always binds its endpoint within about the wait plus normal startup time.
- **Ad-hoc signed macOS builds:** an upgrade reads the new build as having a locked store until the user allows
  access. A Developer ID–signed release keeps the same designated requirement across versions and is not
  expected to prompt. That expectation is not yet verified: there is no production signing.
- **Verification:**
  - `core::tests::credentials::a_store_that_never_answers_does_not_stop_the_service_from_starting` covers this
    with a store that blocks 30 s against a 300 ms wait;
  - `sealed_progress_survives_a_start_without_the_key` covers the held progress (it fails without the change);
  - on CI the Keychain prompt is never answered, so macOS A2 cannot reach "B finishes the download" while A and B
    are ad-hoc signed. That needs both fixture builds signed by one disposable identity (a separate change);
  - the macOS installer-lifecycle rows A2/A3 are the remote check.
