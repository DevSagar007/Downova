# ADR 0033 — Remove may delete a completed download's own file, on explicit request

- **Status:** Accepted (2026-11-14). **B2 is not exited.**
- **Requirements:** CORE-10 (clear list), CORE-03; IDM parity (Delete)
- **Amends:** [ADR 0013](0013-remove-from-list.md) rule 5 ("Never files"), for completed downloads only
- **Evidence:** [PROGRESS.md](../../PROGRESS.md) (2026-11-14 section)

## Context

- **Before:** ADR 0013 made Remove from list keep every file, and the toolbar Delete was permanently disabled.
- **Why change:** Internet Download Manager's Delete can also delete the downloaded file. Users expect that, and
  a disabled Delete that only explains itself is a missing feature.
- **Hard constraint:** deleting the wrong path is unrecoverable.

## Decision

- **Contract:** `downloads.v1.remove` takes `RemoveParams { job_id, delete_file? }`, replacing `JobRef`.
  `delete_file` defaults to false, so every existing client keeps ADR 0013's behaviour.
- **When `delete_file` is true:**
  - only a **completed** download qualifies; anything else is `INVALID_REQUEST` and nothing is touched;
  - the service deletes the download's own file **first**: `destination / display_name`, where the name must be
    exactly one path component;
  - it checks the path with `symlink_metadata` and removes it only if it is a **regular file**; a folder or a link
    in its place is refused and never followed;
  - a file that is already gone is not an error;
  - if the delete fails, the job stays in the list (`DESTINATION_UNWRITABLE`, retry), so the user keeps a handle
    on it;
  - only after that does the normal ADR 0013 removal run; partial and staging files of other states are never
    touched.
- **Clients:**
  - the desktop's Remove confirmation, also opened by the toolbar Delete, shows an unticked "Also delete the
    downloaded file" box for completed downloads only;
  - the announcement says whether the file was deleted;
  - the CLI has `dm remove <ID> --delete-file`;
  - nothing deletes a file without that explicit choice.

## Consequences

- ADR 0013 rule 5 now reads: removal changes no file **unless** the user explicitly asked to delete a completed
  download's own file, as above.
- **Tests:**
  - service: `delete_with_file_removes_only_the_finished_downloads_own_file`;
  - bridge: `remove_uses_its_own_method_and_validates_the_job_id` (`delete_file` sent only when chosen);
  - CLI: `remove_keeps_the_file_unless_delete_file_is_given`;
  - UI: the Delete test in `App.test.tsx` and the updated `status-cancel-remove.test.tsx`;
  - contract example: `remove.delete-file.params.json`.
- **Native check (Windows 11, source build):** the box is unticked at first; ticked, the file is deleted, a
  neighbouring file stays, and the job leaves the list.
- **Not covered:** deleting files of failed or cancelled downloads, deleting a whole category, and the Recycle
  Bin/Trash. The file is deleted permanently, as the confirmation states.
