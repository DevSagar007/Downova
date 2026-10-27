# ADR 0011 — Service-owned file naming and file-system limits

- **Status:** Accepted
- **Date:** 2026-10-26
- **Builds on:** ADR 0002 (backend seam), ADR 0006 (storage), ADR 0007 (checkpoints), ADR 0008 (completion journal)
- **Requirements:** E04, ADV05, CORE-11
- **Evidence:** [B1.6 file-naming report](../reports/b1.6-file-naming.md)

## Context

E04 requires that `Content-Disposition` (RFC 6266) and `filename*` (RFC 8187) are parsed and treated as advisory, that
names are made safe on every OS, and that long Unicode and Bangla names, file-system limits and invalid paths are
tested.

Until B1.6 the service chose one name at acceptance, from the URL path, before any HTTP request. It used that name
for three things:
- the partial file (`<name>.downova-part`);
- the checkpoint binding (ADR 0007);
- the final file.

The server's suggested name was never seen. A header cannot be used to rename the partial file without breaking the
checkpoint binding and journal recovery, and an adapter must never choose where a file goes.

## Decision

1. **Two names per job, both owned by the service.**
   - **Staging name.** Chosen at acceptance from the requested URL. It is sanitized, fitted and made unique, and it
     never changes. The backend gets it as `TransferRequest.file_name`. The backend's partial and control files use
     it, and checkpoints and journal entries bind to it.
   - **Final name** (`display_name`). It starts *provisional*, equal to the staging name, and is **frozen** once.
2. **Metadata path through `dm-backend-api`.**
   - `BackendProgress.naming: Option<ResponseNaming>` carries the unparsed `Content-Disposition` value and the
     still-encoded last path segment of the response URL. The query and fragment are never included.
   - The value comes from the final response after redirects: the `200` that starts a full body. A validated `206`
     fills it only when the attempt has seen no `200` of its own (a resumed or restored attempt).
   - The data comes from responses the backend already receives. No extra request is made.
   - Values longer than 4 KiB are dropped. Field bytes that are not UTF-8 are read as ISO-8859-1.
   - aria2 cannot see headers and reports `None`.
3. **Freeze point.** The final name is chosen and persisted **the first time the service sees response metadata for
   the job**. That happens in the same poll in which it arrives, before any finalization in that poll. If the job
   reaches finalization without metadata (aria2, or a crash before the first poll), the provisional name is frozen
   with the journal's `intent` row.
   - After the freeze, nothing renames the job: later responses, pauses, resumes, retries, restarts after a changed
     resource, and journal recovery all keep the name.
   - The journal's `final_path` is always the frozen name.
   - For an engine that placed the file itself (aria2), the final path is the file it wrote under the staging name.
4. **Precedence.** The first source that is non-empty after sanitizing wins:
   1. a valid `filename*`;
   2. `filename`;
   3. the final response URL's last path segment;
   4. the requested URL's last path segment;
   5. `download`.
5. **Parsing (RFC 6266 §4, RFC 8187).**
   - The disposition type is ignored. Parameter names are case-insensitive.
   - Quoted strings honour `\` escapes, and a `;` inside quotes is data.
   - A parameter that occurs more than once is ignored entirely.
   - An unterminated quote, or text after a closing quote, ends parsing.
   - `filename*` accepts the charsets UTF-8 and ISO-8859-1 only. Malformed percent-encoding, invalid UTF-8 or another
     charset discard it.
   - `filename` is taken literally: no percent or RFC 2047 decoding.
6. **Sanitizing.**
   - Names are normalized to NFC; scripts and emoji are kept.
   - `/`, `\`, `<>:"|?*` and control characters become `_`.
   - Bidirectional overrides, isolates and zero-width spaces are removed. ZWJ and ZWNJ are kept.
   - Leading dots and spaces are removed (no hidden files, no `..`), as are trailing dots and spaces.
   - Windows device names (`CON`, `PRN`, `AUX`, `NUL`, `CONIN$`, `CONOUT$`, `COM0–9`, `LPT0–9`, `COM¹²³`, `LPT¹²³`;
     with any extension and in any case) get a `_` prefix.
   - Names ending in `.downova-part` or `.aria2` get a `_` appended.
7. **Length limits** are documented and conservative per platform, not universal:

   | Platform | Name component | Whole path |
   |---|---|---|
   | Windows | 255 UTF-16 units (NTFS, ReFS, exFAT, FAT long names) | 259 units, so programs without long-path support can open the file |
   | Elsewhere | 255 UTF-8 bytes (ext4, Btrfs, XFS, APFS) | 4095 bytes |

   - Every name keeps 24 units free for a collision suffix (up to ` (xxxxxxxx)`) plus `.downova-part`.
   - The name is shortened further to fit the path limit, but never below 32 units. Downova itself uses long paths
     through the standard library.
   - Truncation cuts the stem at grapheme-cluster boundaries, keeps an extension of up to 16 units, and trims trailing
     dots and spaces again.
   - Volumes with smaller limits (eCryptfs, some network shares) refuse the name when the file is created.
8. **Collisions.**
   - A candidate is free only when all of the following hold:
     - no directory entry of that name exists (the file system decides case and normalization; dangling links count);
     - no other job in the folder uses it as a final or staging name (compared NFC, lower case);
     - no `<candidate>.downova-part` or `<candidate>.aria2` exists, except the job's own partial file.
   - Otherwise ` (1)`, ` (2)`, … is inserted before the extension.
   - Placement stays non-overwriting (ADR 0008): a file that appears after the freeze stops placement with an
     explicit error and is never replaced.
9. **New error kind: `DESTINATION_UNWRITABLE`** (contract enum addition). It covers:
   - `ENAMETOOLONG`, and Windows invalid or too-long names (`io::ErrorKind::InvalidFilename`);
   - permission denied;
   - read-only volumes;
   - a path that is not a folder.

   It applies both when the partial file is created and at placement. The detail names the reason, never the path.
   It is not retryable.

   *Amended by B1.7:* a file that appears at the frozen final name before placement (§8) is also reported as
   `DESTINATION_UNWRITABLE`. It was `INVALID_REQUEST` before, which described the client rather than the folder. The
   file is still never overwritten.
10. **Storage** (migration 4, forward-only).
    - New columns: `jobs.staging_name` and `jobs.name_source`. A null `name_source` means provisional. Otherwise it
      is one of `filename_ext`, `filename`, `response_url`, `request_url`, `default` or `legacy`.
    - Existing jobs get `staging_name = display_name` and `name_source = 'legacy'`. Their partial files, checkpoints
      and journal entries keep working unchanged, and their names never change.
11. **Allocation policy (documented, unchanged).**
    - Partial files grow as bytes are written. Segmented transfers write at offsets, which may leave sparse regions.
    - No space is pre-allocated or reserved, and sparse sizing reserves nothing.
    - The disk budget (ADR 0009) is an advisory snapshot. `DISK_FULL` remains possible and is recoverable.

## Consequences

- During a transfer the snapshot's `display_name` can change once, from provisional to final, and never again.
  Clients must not use it as an identity; job IDs remain the identity.
- The staging file can carry a different name from the final file (`get.downova-part` → `report.pdf`). That is
  intended: renaming the partial file would break the checkpoint binding.
- A name selected on Windows can be longer in UTF-8 than Linux allows. Copying such a file to ext4 later can fail.
  The limits follow the platform where the file is written.
- Not covered:
  - RFC 2231 continuations (`filename*0*`) and RFC 2047 encoded words;
  - per-volume limit probing (for example eCryptfs's 143 bytes);
  - Windows short (8.3) name aliases;
  - per-directory case-sensitivity flags on NTFS.
