# ADR 0030 — Verified updates, service quiescence, consistent backup and migration recovery

- **Status:** Accepted for the B3.5 feature branch (2026-11-07), under the narrow exception the maintainer granted
  for ADV09's B3 scope (signed updates and update recovery) and the REL-01 work a safe installation needs.
  **B2 is not exited.** Production update hosting, production signing keys, release channels, SBOM and provenance
  are out of scope.
- **Requirements:** ADV09 (B3 part), REL-01 (update path), ARCH-01/ARCH-05 (service ownership), SEC-01
- **Builds on:** [ADR 0012](0012-exit-and-stop.md) (exit and shutdown modes),
  [ADR 0027](0027-secret-lifecycle-vault-and-authenticated-get.md) (vault, sealing)
- **Evidence:** [B3.5 report](../reports/b3.5-signed-updates-recovery.md)

## Context

- Downova had no update path. A new version could only be installed by hand, with no backup, and nothing ensured
  the service had stopped cleanly first.
- The service already stops gracefully: `service.v1.shutdown` with "keep state" checkpoints running transfers and
  keeps every desired state.
- Schema migrations run in one `IMMEDIATE` transaction, so a failed migration changes nothing. Until now it simply
  stopped the service with "storage unavailable".
- **The updater plugin, as checked on 2026-11-07:**
  - `tauri-plugin-updater` 2.14.1 always verifies a minisign signature over the artifact;
  - it rejects non-newer versions by default and requires HTTPS endpoints unless
    `dangerousInsecureTransportProtocol` is set;
  - its source also implements **`requireSignedVersion`**: after the signature (including the trusted comment)
    verifies, the `version:` field of the trusted comment must equal the version the endpoint announced (the web
    page does not document this key yet);
  - the pinned CLI (2.12.3) writes that field with `tauri signer sign --app-version <v>`;
  - on Windows, installing exits the app;
  - the plugin's config type requires `plugins.updater.pubkey` once the plugin is registered.

## Decision

1. **Ownership.**
   - The **desktop shell** owns checking, downloading, verifying and starting the installer.
   - The **service** owns quiescence, checkpoints, the backup, the update records, startup acceptance and restore:
     it stays the single SQLite writer, and nothing else copies the database.
   - The **webview** only asks the shell to run fixed steps. It never supplies a URL, key, path or installer, and
     no `updater:*` plugin permission is granted to it.
2. **Trusted configuration.**
   - Updater settings (`plugins.updater`: `pubkey`, `endpoints`, `requireSignedVersion: true`) and
     `bundle.createUpdaterArtifacts` live only in a build-time config file merged by `tauri build --config`. They
     are compiled into the binary.
   - The base `tauri.conf.json` has none. A build without them registers no updater and shows **"Updates are not
     configured"**, never "up to date".
   - **Production:** HTTPS endpoints. Insecure transport is accepted only in a **fixture build**, which is compiled
     with the shell's `update-fixture` cargo feature (`package.mjs --update-fixture`), uses loopback endpoints and
     a disposable key, and is labelled as a fixture (`-update-fixture` package directory, `updater: "fixture"` in
     `build-info.json`).
   - The shell refuses an insecure or non-loopback-HTTP configuration in any non-fixture binary. `package.mjs`
     refuses to build a normal package with a fixture config or updater signing variables present.
3. **Verification.**
   - `check` uses the plugin with its default "must be newer" comparator, so ordinary downgrades are refused.
   - The platform target is the plugin's `{os}-{arch}-{bundle_type}` (the running build's own package format),
     falling back to `{os}-{arch}`. An update for another format is never selected.
   - `download` returns bytes only after the plugin verified the signature against the compiled key and the
     signed version against the announced one.
   - Downova keeps the verified bytes in memory together with the version they were verified for; `prepare` sends
     that same version to the service and `install` installs only those bytes, so nothing announced later can
     replace them.
   - The UI shows **"Verified"** only after that succeeds.
4. **State machine** (shell, typed, one operation at a time): `not_configured → idle → checking →
   (no_update | available) → downloading → verifying → verified → preparing → ready_to_install → installing`,
   plus `failed` and `recovery_required`. Duplicate requests are refused, and every step names the operation it
   belongs to, so stale answers are dropped.
5. **Quiescence: `service.v1.prepare_update` and `service.v1.cancel_update`.** The service holds a **bounded update
   lease** (operation ID, target version, expiry):
   - **(a)** It stops admitting transfers. Starting, resuming, retrying, reconnecting, refreshing and adding are
     refused with `SERVICE_UNAVAILABLE` "an update is being prepared". Reads and pauses still work.
   - **(b)** It stops running transfers and commits their checkpoints **without changing any desired state**. A
     manual pause stays paused and a stopped queue stays stopped; jobs that were running are remembered as "held for
     the update", not paused.
   - **(c)** It writes a backup with `VACUUM INTO` from its own connection: one consistent snapshot including WAL
     content, synced, then reopened read-only and checked (`integrity_check`, schema version, job count, SHA-256).
   - **(d)** It records the backup's metadata and an `update/pending.json` marker (operation, source app and schema
     version, target version, backup checksum, completion).

   **Any failure in (b)–(d) releases the lease**, resumes what it held, and returns the error: nothing is installed.
   The lease expires on its own (10 minutes) if no shutdown follows.
6. **Installation handoff.**
   - Only after `ready_to_install` and an explicit confirmation does the shell call `service.v1.shutdown` (keep
     state).
   - It confirms the service has exited (the IPC endpoint is gone and the instance lock is free, bounded at 60 s);
     on timeout it does not install, and the lease expiry gives the old version back.
   - Then it calls the plugin's `install` with the verified bytes. All persistence happened before this call; none
     relies on an exit callback.
7. **Startup acceptance and recovery** (service, before serving):
   - **Version reached, migration succeeded:** a pending marker for the running version is **accepted**
     (`update/last.json`: installed), and work resumes from its checkpoints only now.
   - **Version not reached:** a marker for another version (the installer was cancelled) is closed as
     `not_installed`; the old version continues unchanged.
   - **Migration or open failure:** the service records `update/recovery.json` (operation, from/to schema, what
     failed, the matching backup) and exits with code 6 instead of serving. The database is unchanged because the
     migration transaction rolled back.
   - **Restore:** `dm-service --restore-update <operation>` replaces an unusable database with that operation's
     backup, only if the backup still matches its recorded checksum, passes the integrity check, and has a schema
     version this binary can open. The replaced files are set aside, never deleted.

   The desktop shows "Recovery required" with exactly two actions: try again, or restore the pre-update backup. It
   never restores automatically, and never downgrades binaries itself.
8. **What a backup is.**
   - It contains the database: jobs, attempt and checkpoint bindings, queues, schedules, limits, desired states,
     verification and scan evidence, sealed URLs and credential *references*.
   - It never contains vault secrets. The data key and remembered sign-ins stay in the OS store under the same
     data-folder namespace, and nothing rotates or deletes them during an update, so a restored database opens its
     sealed links again.
   - Downloaded files and partial files are never touched. Partial files are re-verified by the engine at restore
     time as on every start.
   - Backups live under `update/backups/` in the data folder and are referred to only by operation ID. The two
     newest are kept.
9. **Per-format capability:**

   | Format | In-app update |
   |---|---|
   | Windows NSIS (per user) | supported by the updater plugin; fixture-tested here (see the report) |
   | Windows MSI | the plugin can install it; not tested, and per-machine installs need elevation |
   | macOS `.app` | plugin `.app.tar.gz` artifacts; not built or tested here |
   | Linux AppImage | plugin artifacts; not tested (and AppImage is refused for browser registration) |
   | Linux `.deb` / `.rpm` | not offered: the package manager updates them |

   Updater signatures (minisign, from the updater key) are **not** operating-system code signing or notarization.
   Neither exists for Downova yet.

## Consequences

- An update never starts installing with transfers running, an unchecked backup or a service still holding the
  database. A failed migration leaves a startup path that explains itself and offers a checked restore.
- Production updates need, from the maintainer: a production updater key pair (private key kept outside this
  repository), an HTTPS update endpoint, the production `plugins.updater` config, and re-signing with
  `--app-version` for every release users can update from. None of these are created here.
- Release channels, SBOM and provenance stay at B4.
