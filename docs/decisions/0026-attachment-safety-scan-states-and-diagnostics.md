# ADR 0026 — Windows attachment safety, scan states and redacted diagnostics

- **Status:** Accepted for the B3.2 feature branch (2026-11-05). It was developed under the narrow sequencing
  exception the maintainer extended to F07, E06, E05 and the SEC-01 work they need. **B2 is not exited**, and nothing
  here approves a release.
- **Date:** 2026-11-05
- **Requirements:** F07 (Windows scope), E06, E05, SEC-01, SEC-03
- **Builds on:** [ADR 0008](0008-completion-journal.md) (finalization journal), [ADR 0015](0015-duplicates-destinations-collisions.md)
  (no-overwrite placement), [ADR 0022](0022-completion-notifications-and-list-performance.md) (notifications)
- **Evidence:** [B3.2 report](../reports/b3.2-download-safety-diagnostics.md)

## Context

A finished download is verified (length, SHA-256), placed without overwriting, and only then committed as
`completed` (ADR 0008).

- **F07:** on Windows, a downloaded file must go through the OS's attachment handling (`IAttachmentExecute::Save`)
  so it carries origin marking (Mark of the Web, MOTW) and registered trust services can see it.
- **E06:** users must see that a file is still being checked, and why a check did not happen.
- **E05:** users must be able to export a support file that cannot leak secrets.

Microsoft's documentation for `Save` says that it "may run virus scanners or other trust services" which "can
delete or alter the file", and that it "may attach evidence… in its NTFS alternate data stream". It returns `S_OK`
or an error `HRESULT`. Nothing in that contract is a malware verdict.

## Decision

### 1. Order inside the existing journal (no parallel pipeline)

`intent → verified → placed → [attachment check] → complete`.

- **Placement is unchanged:** a hard link, or an exclusive copy, never replaces anything.
- **At `placed`, the placed file's identity is recorded with the stage:**
  - Windows: volume serial plus file index;
  - Unix: device plus inode.
- **The check then starts.** The journal row gets `scan_started_ms`, committed before any OS call. The job's phase
  is `scanning`, and its snapshot carries `scan.status = "scanning"`.
- **When the check returns, the service re-verifies the placed file** (exists, same identity, same SHA-256) before
  it trusts any result.
- **The outcome is committed in the same transaction as `complete`**, or as the failure. There is no separate
  "scanned" stage, so a crash leaves either `placed` (the check runs again) or the final outcome.
- **The journal's `stage` CHECK constraint is untouched.** The scan is a sub-state of `placed`: migration 12 is
  additive.

### 2. Outcomes (`DownloadSnapshot.scan`, stored in `jobs.scan_json`)

| `status` | When | Job result |
|---|---|---|
| `scanning` | The check is queued or running | phase `scanning` |
| `attachment_checked` | `Save` returned `S_OK`, and afterwards the file exists with its identity and SHA-256 unchanged | `completed` |
| `blocked` | `Save` returned `INET_E_SECURITY_PROBLEM` (`policy_blocked`); or afterwards the file was missing (`file_removed`), had different bytes (`file_changed`) or was a different file (`file_replaced`) | `failed`, `SCAN_BLOCKED` |
| `unavailable` | Attachment Services could not be created or returned another error (`service_error`), or no result came within 10 minutes (`timed_out`) | `completed`, with a warning |
| `skipped` | Not attempted: this OS has no integration in this version (`platform_not_supported`), or the user cancelled during the check (`cancelled`) | `completed` (platform), or `cancelled` |

- **`attachment_checked` is described as what it is:** "Windows Attachment Services processed the file; it was not
  removed or changed." It is **never** called "safe" or "malware-free".
- **`origin_marking`** says what is on the file afterwards:
  - `present`: a `Zone.Identifier` stream exists;
  - `absent`: Windows policy did not write one, and that policy is kept;
  - `written_by_downova`: the fallback below;
  - `unsupported_volume`: the volume has no named streams (FAT32, exFAT, many network shares);
  - `not_supported_on_platform`.

  MOTW presence is never presented as proof of anything beyond origin.
- **Checksum verification (`verification`) stays separate:** it proves the recorded bytes, not who published them
  or whether they are harmful.
- **Completion policy for `unavailable` and `skipped`:**
  - the job completes, because the bytes are verified and the user can act on the file;
  - the status stays visible in the list, the details and the CLI as a warning;
  - it is never shown as verified-safe.
- **`blocked`:** the job fails with `SCAN_BLOCKED`.
  - The service never re-creates, re-places or "repairs" a file the check removed or changed.
  - It removes its own staging name, so a second name for the same bytes cannot survive the block.
  - Whatever the trust service left at the destination is left as it is.
- **Records completed before this ADR have no `scan` field.** They are presented as "not recorded", never as
  checked.

### 3. Windows adapter (`dm-platform::attachment`)

- **Creation and threading:** `CoCreateInstance(CLSID_AttachmentServices)` on a dedicated thread initialised as a
  single-threaded apartment (`COINIT_APARTMENTTHREADED`), and uninitialised on the same thread. The COM object never
  leaves that thread.
- **Calls:**
  - `SetClientGuid` with Downova's fixed client GUID;
  - `SetLocalPath` (the placed final path);
  - `SetSource` with the **sanitized origin** only;
  - then `Save()`.
- **The sanitized origin is `scheme://host[:port]/`:**
  - no userinfo, path, query or fragment, so signed-URL tokens, path tokens, cookies and authorization never reach
    the ADS, logs or diagnostics;
  - cookies and headers are never given to the API;
  - the zone is still derived from the host (Internet for public hosts).
- **Fallback (Chromium's precedent):** if `Save` fails with an error other than a policy block, and the file is
  intact, Downova writes `Zone.Identifier` itself:
  - `ZoneId=3` and the sanitized `HostUrl`, only on volumes with named streams;
  - it is reported as `written_by_downova`, with the status still `unavailable`;
  - when `Save` succeeds, Downova writes nothing: OS policy, including "do not preserve zone information", is kept.
- **Bounds:**
  - **Volume capability:** `GetVolumeInformationW` (`FILE_NAMED_STREAMS`).
  - **Concurrency:** at most 2 checks run at once (a semaphore). Each runs on its own OS thread, outside the
    coordinator lock; the poll loop only collects finished results.
  - **Time:** 10 minutes per job, counted from the request. After that the job completes as
    `unavailable/timed_out`, and the thread's late result is discarded.
- **SEC-03 is unchanged:** `Save` is used, never `Execute`, `Prompt` or any open or launch call. No file is ever
  run.

### 4. Cancellation, recovery, notifications

- **Cancel during `scanning` is allowed** (before this ADR, cancel was refused from `placed` on).
  - The job becomes `cancelled`, with `scan = skipped/cancelled`.
  - The journal entry is dropped in the same transaction.
  - The check's ticket is revoked. A result that arrives later finds no ticket, so it cannot complete, fail or
    revive the job.
  - The OS call itself cannot be interrupted safely, so it finishes in the background. The placed file stays where
    it is, and the details say it "was not checked". The staging name is removed.
- **Recovery** (a crash before, during or after the check):
  - an entry at `placed` re-verifies the final file's identity and SHA-256, then runs the check again (`Save` is
    idempotent for an intact file);
  - **if `scan_started_ms` is set and the final file is missing or changed, the job is `blocked`.** It is never
    re-placed from the staging bytes, as ADR 0008 does when no check had started;
  - an outcome committed with `complete` is final.
- **Notifications** are decided only after the journal entry is gone (ADR 0022), so a check never causes an early
  or duplicate notice. `blocked` notifies as a failure, and a cancellation does not notify.

### 5. Platforms

- **Windows:** as above. The capability is reported as `attachment_services` in diagnostics and `dm hello`'s
  platform line.
- **macOS and Linux:** no check runs in this batch (native quarantine and scanning stay at their roadmap milestone).
  Every completion records `skipped/platform_not_supported` with `origin_marking = not_supported_on_platform`. The
  UI says "No malware check on this system", never that a Windows check ran.

### 6. Diagnostics (E05)

- **`diagnostics.v1.preview`:**
  - the service builds a report from an **allowlist of typed fields** and returns it with an opaque `snapshot_id`;
  - it is a read-only, bounded collection: at most 200 jobs, the 50 most recent errors and 256 KiB serialized, with
    collection time capped by those bounds;
  - the coordinator lock is held only to copy the bounded summary, never during serialization or I/O.
- **Allowed:**
  - schema version, app and service version, the build commit when built in CI, OS and architecture;
  - backend id, version and capabilities, plus the platform scan capability;
  - non-secret settings as values or counts: concurrency, the overall limit, the collision policy, retention,
    whether notifications are on, queue count, scheduled-queue count, and the destination-rule count (not paths);
  - job counts by phase and by blocker, scan outcomes by status, and journal entries in progress;
  - the 50 most recent failures as `{job_id, phase, error kind, next_action, scan status, age in minutes}`, with no
    names, URLs, hosts, paths or free-text detail;
  - limitations, as fixed text.
- **Crash section (opt-in, `include_crash`):**
  - the last startup's recovery summary: whether the previous run ended uncleanly, how many jobs were recovered,
    interrupted or reconciled from the journal;
  - Downova writes no crash dumps, and the section says so.
- **Final check (defence in depth):** before returning a preview and again before writing, the serialized text is
  scanned for secret and identity patterns:
  - URLs with userinfo, query or fragment;
  - `authorization`, `cookie`, `bearer`, `token=`, `password`, `proxy-authorization`;
  - home-directory paths (`C:\Users\`, `/home/`, `/Users/`).

  A match refuses the operation (`INTERNAL`, "diagnostics failed the redaction check") instead of exporting.
- **`diagnostics.v1.export {snapshot_id, path, replace}`:**
  - writes exactly the previewed snapshot (byte-identical; the preview keeps the last one, so an unknown or expired
    id is refused);
  - the write is a temporary file in the destination folder, synced, then a no-overwrite hard link or rename;
    `replace` (set when the user confirmed it in the save dialog, or `--replace`) allows an atomic replace;
  - nothing secret-bearing is ever written before sanitization, because the content is the sanitized preview;
  - failures are typed: `DESTINATION_UNWRITABLE` (permission, a read-only volume, an existing file without
    `replace`), `DISK_FULL`, and an unknown snapshot.
- **The desktop:**
  - Help → "Diagnostics…" previews the report, with the crash section switch off by default;
  - "Export…" opens the native save dialog in the Tauri shell (Rust), so the path never passes through the
    webview's state;
  - cancelling the dialog changes nothing.
- **The CLI:** `dm diagnostics [--include-crash]` prints the preview, and `dm diagnostics-export <PATH>
  [--include-crash] [--replace]` previews and exports that same snapshot.
- **Offline:** nothing is transmitted, there is no analytics requirement, and no file is opened afterwards.

## Consequences

- One additive migration (12): `jobs.scan_json`, `finalization_journal.scan_started_ms` and `final_file_id`.
- Contract additions, all optional or additive:
  - `DownloadSnapshot.scan`;
  - `ScanRecord`;
  - `diagnostics.v1.preview` and `diagnostics.v1.export`, with their types.
- **A finished download on Windows completes later than before, by the duration of the check.** Download slots
  are not held during it.
- **What the tests prove, and what they do not:**
  - deterministic fakes prove the state machine and its failure paths;
  - the real Windows adapter is exercised by an ignored-by-default integration test and by the packaged run;
  - **no real antivirus was made to block a file.** A real block is a human or VM check.
