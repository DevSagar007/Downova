//! Native folder controls (B2.19, ADR 0021): the only module that touches the OS file manager.
//!
//! - **Browse…** ([`pick_folder`]): the native folder dialog, opened only by a user's click. It
//!   returns the chosen path to the UI, which still saves it through `settings.v1.set` (the
//!   service validates every folder). Cancelling returns `None` and changes nothing.
//! - **Open containing folder** ([`reveal`]): the path comes from the service's own record
//!   (`downloads.v1.locate`), never from the renderer. A completed file that still exists is
//!   *selected* in its folder (`reveal_item_in_dir` shows it, never runs it); otherwise the folder
//!   itself is opened, and only after checking, without following links, that it is a directory.
//!   Nothing here opens or executes a file, and nothing runs without an explicit user action:
//!   completion, recovery, events and background work never call this module (SEC-03, guarded by
//!   `crates/service/tests/sec03_no_execution.rs`).

use std::path::{Path, PathBuf};

use dm_domain::api::LocateResult;
use dm_domain::{ErrorKind, NextAction};
use serde::Serialize;
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

use crate::downloads::BridgeError;

/// What "Open containing folder" did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Revealed {
    /// The folder opened with the file selected.
    File,
    /// The folder opened; the download has no final file yet.
    Folder,
    /// The folder opened, but the completed file is no longer there.
    FolderFileMissing,
}

/// What [`reveal`] will do for a located download (pure, so it is unit-tested).
#[derive(Debug, PartialEq, Eq)]
pub enum Target {
    SelectFile(PathBuf),
    OpenFolder(PathBuf, Revealed),
}

fn refused(kind: ErrorKind, action: NextAction, detail: &str) -> BridgeError {
    let mut error = kind.to_error_data(detail);
    error.next_action = action;
    BridgeError::rejected(error)
}

/// The directory exists as a real directory (a link is never followed).
fn is_real_dir(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.is_dir())
}

fn is_real_file(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.is_file())
}

/// Chooses the action from the service's answer and the file system as it is now.
pub fn target(located: &LocateResult) -> Result<Target, BridgeError> {
    let dir = PathBuf::from(&located.directory);
    if !dir.is_absolute() {
        return Err(refused(
            ErrorKind::Internal,
            NextAction::None,
            "the download folder is not an absolute path",
        ));
    }
    if !is_real_dir(&dir) {
        // A missing drive (its root is gone) or a folder that was moved or deleted.
        let volume_present = dir.ancestors().last().is_some_and(Path::exists);
        return Err(if volume_present {
            refused(
                ErrorKind::DestinationUnwritable,
                NextAction::None,
                "the download folder no longer exists",
            )
        } else {
            refused(
                ErrorKind::VolumeMissing,
                NextAction::ReconnectVolume,
                "the drive of the download folder is not available",
            )
        });
    }
    if located.file_name.is_empty() {
        return Ok(Target::OpenFolder(dir, Revealed::Folder));
    }
    // The name comes from the service, but it must still be a plain name inside the folder.
    let name = Path::new(&located.file_name);
    let plain = name.components().count() == 1
        && matches!(
            name.components().next(),
            Some(std::path::Component::Normal(_))
        );
    if !plain {
        return Ok(Target::OpenFolder(dir, Revealed::FolderFileMissing));
    }
    let file = dir.join(name);
    if is_real_file(&file) {
        Ok(Target::SelectFile(file))
    } else {
        Ok(Target::OpenFolder(dir, Revealed::FolderFileMissing))
    }
}

/// "Open containing folder" for one job, after an explicit user action.
pub async fn reveal<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    data_dir: &Path,
    job_id: &str,
) -> Result<Revealed, BridgeError> {
    let located = crate::downloads::locate(data_dir, job_id).await?;
    let failed = |_| {
        refused(
            ErrorKind::Internal,
            NextAction::None,
            "the file manager could not be opened",
        )
    };
    match target(&located)? {
        Target::SelectFile(file) => {
            app.opener().reveal_item_in_dir(&file).map_err(failed)?;
            Ok(Revealed::File)
        }
        Target::OpenFolder(dir, how) => {
            // Checked again right before opening: only a directory is ever handed to the OS.
            if !is_real_dir(&dir) {
                return Err(refused(
                    ErrorKind::DestinationUnwritable,
                    NextAction::None,
                    "the download folder no longer exists",
                ));
            }
            let dir = dir.to_str().ok_or_else(|| {
                refused(
                    ErrorKind::Internal,
                    NextAction::None,
                    "unsupported folder path",
                )
            })?;
            app.opener().open_path(dir, None::<&str>).map_err(failed)?;
            Ok(how)
        }
    }
}

/// The native folder dialog (Browse…). `None` when the user cancelled.
pub async fn pick_folder<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    start: Option<String>,
    title: String,
) -> Result<Option<String>, BridgeError> {
    let mut dialog = app.dialog().file().set_title(title);
    if let Some(start) = start.filter(|s| is_real_dir(Path::new(s))) {
        dialog = dialog.set_directory(start);
    }
    let picked = tauri::async_runtime::spawn_blocking(move || dialog.blocking_pick_folder())
        .await
        .map_err(|_| {
            refused(
                ErrorKind::Internal,
                NextAction::None,
                "the folder dialog failed",
            )
        })?;
    Ok(picked
        .and_then(|p| p.into_path().ok())
        .and_then(|p| p.to_str().map(str::to_owned)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn located(dir: &Path, name: &str) -> LocateResult {
        LocateResult {
            directory: dir.to_string_lossy().into_owned(),
            file_name: name.into(),
            directory_present: true,
            file_present: true,
        }
    }

    #[test]
    fn a_present_file_is_selected_never_opened() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("setup.exe"), b"MZ").unwrap();
        assert_eq!(
            target(&located(dir.path(), "setup.exe")).unwrap(),
            Target::SelectFile(dir.path().join("setup.exe"))
        );
    }

    #[test]
    fn missing_files_folders_and_odd_names_open_only_a_real_folder() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            target(&located(dir.path(), "")).unwrap(),
            Target::OpenFolder(dir.path().to_owned(), Revealed::Folder)
        );
        assert_eq!(
            target(&located(dir.path(), "gone.bin")).unwrap(),
            Target::OpenFolder(dir.path().to_owned(), Revealed::FolderFileMissing)
        );
        for odd in ["../escape.bin", "a/b.bin"] {
            assert_eq!(
                target(&located(dir.path(), odd)).unwrap(),
                Target::OpenFolder(dir.path().to_owned(), Revealed::FolderFileMissing),
                "{odd}"
            );
        }
        let missing = dir.path().join("moved-away");
        let e = target(&located(&missing, "x.bin")).unwrap_err();
        assert_eq!(e.error.kind, ErrorKind::DestinationUnwritable);
        let e = target(&located(Path::new("relative/dir"), "")).unwrap_err();
        assert_eq!(e.error.kind, ErrorKind::Internal);
    }

    #[cfg(windows)]
    #[test]
    fn a_missing_drive_is_reported_as_such() {
        // Q: is very unlikely to exist on a test machine; skip if it does.
        if Path::new("Q:\\").exists() {
            return;
        }
        let e = target(&located(Path::new("Q:\\Downloads"), "x.bin")).unwrap_err();
        assert_eq!(e.error.kind, ErrorKind::VolumeMissing);
        assert_eq!(e.error.next_action, NextAction::ReconnectVolume);
    }
}
