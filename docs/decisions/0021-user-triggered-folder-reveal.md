# ADR 0021 — Native folder picker and user-triggered "Open containing folder"

- **Status:** Accepted
- **Date:** 2026-10-31
- **Narrows:** the SEC-03 guard of ADR 0015 (§7)
- **Requirements:** CORE-07, ADV12, SEC-03, SEC-05
- **Evidence:** [B2.15–B2.20 batch report](../reports/b2.15-b2.20-batch.md)

## Context

Users must type rule folders by hand, and there is no way to find a finished file from the app.
Both need the operating system:
- **Browse…** needs a native folder dialog.
- **Open containing folder** needs the file manager.

SEC-03 forbids any path that opens or executes a downloaded file. Its structural guard
(`crates/service/tests/sec03_no_execution.rs`) banned every opener dependency outright. Removing
the guard to build the feature would lose the protection, so the guard is narrowed instead.

Official Tauri 2 plugins, checked on 2026-10-31:
- **`tauri-plugin-dialog` 2.8.4:** `FileDialogBuilder::blocking_pick_folder()` returns `None` when the
  user cancels. It is desktop only.
- **`tauri-plugin-opener` 2.7.2:** `Opener::reveal_item_in_dir(path)` shows a path in the file manager
  (selected, not run), and `open_path(path, None)` opens a path with its default handler.
- Plugin **permissions** gate the webview's IPC calls. Rust code calls the plugin API directly.

## Decision

1. **One adapter.** `apps/desktop/src-tauri/src/folders.rs` is the only code that uses either plugin.
   The plugins are registered in `lib.rs`. **No `dialog:` or `opener:` permission is granted** in
   `capabilities/default.json`, so the page cannot call them. The UI reaches the adapter only through
   two narrow app commands, each granted by name: `folder_pick` and `downloads_reveal`.
2. **Browse….**
   - `folder_pick(start, title)` opens the native dialog after a click.
   - It returns the chosen folder or `null` (cancelled). Cancel changes neither the field nor the saved
     settings.
   - The folder is saved only by **Save**, through `settings.v1.set`, which validates it as before.
     Existing jobs keep their bound destination (ADR 0015).
3. **Open containing folder.**
   - `downloads_reveal(job_id)` takes only a job ID. The folder comes from the service
     (`downloads.v1.locate`): the job's bound destination, plus its final name once completed. A path
     from the renderer is never trusted.
   - **Target:**
     - The folder must be an absolute, real directory, checked with `symlink_metadata`, so a link is
       never followed.
     - A completed file that exists as a real file is **selected** with `reveal_item_in_dir`.
     - Otherwise (no final file yet, file missing, or an odd name), the **folder itself** is opened
       with `open_path(dir, None)`. The adapter re-checks right before that call that the path is a
       directory.
   - **Errors:**
     - a missing folder: `DESTINATION_UNWRITABLE`;
     - a missing drive (its root is gone): `VOLUME_MISSING` with `reconnect_volume`;
     - a missing file: the folder opens and the UI says the file is gone.
   - It runs only on the user's menu or button action. Completion, recovery, events, lifecycle and the
     smoke bridge never call it.
4. **SEC-03 guard, narrowed:**
   - Crates still may not depend on any opener or shell crate. The desktop shell may have **exactly
     one** opener line, `tauri-plugin-opener = …`, and no shell plugin.
   - The UI package may not depend on the shell, opener or dialog JS plugins, and the capability grants
     no `shell:`, `opener:` or `dialog:` permission.
   - Only `folders.rs` may use `OpenerExt`, `.opener()`, `open_path`, `reveal_item_in_dir` or
     `open_url`. `lib.rs` may only register the plugin. Only the `downloads_reveal` command in
     `lib.rs` may call `folders::reveal`.
   - In the adapter:
     - no `open_url`;
     - no process start;
     - exactly one `open_path`, preceded by its directory check and with no "open with" program;
     - links never followed.

## Consequences

- Users choose folders natively and find finished files from the app. Downova still never opens or
  runs a downloaded file.
- Opening a folder depends on the OS file manager. It cannot be exercised in headless CI, so the
  native dialog and the file manager are human checks. The pure target decision and the service's
  `locate` are tested automatically.
- Mobile platforms (no folder picker or reveal) are out of scope.
