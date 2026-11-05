//! Diagnostics export (E05, ADR 0026): the native save dialog runs here, in the shell, and the
//! chosen path goes straight to the service; it never passes through the webview. The webview
//! learns only the outcome and the file name it was saved as.

use serde::Serialize;
use tauri_plugin_dialog::DialogExt;

use dm_domain::api::{DiagnosticsExportParams, DiagnosticsExportResult, methods};
use dm_domain::{ByteCount, ErrorKind, NextAction};

use crate::downloads::{BridgeError, call_value};

/// What happened to an export the user started.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ExportOutcome {
    /// The user closed the save dialog: nothing was written.
    Cancelled,
    Written {
        file_name: String,
        size_bytes: ByteCount,
        sha256: String,
    },
}

/// Opens the save dialog and exports `snapshot_id` there. The dialog's own overwrite prompt is the
/// user's confirmation, so `replace` is set when they chose an existing file.
pub async fn export<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    data_dir: &std::path::Path,
    snapshot_id: String,
    title: String,
) -> Result<ExportOutcome, BridgeError> {
    if !dm_domain::api::is_snapshot_id(&snapshot_id) {
        return Err(BridgeError::invalid_argument("not a diagnostics snapshot"));
    }
    let dialog = app
        .dialog()
        .file()
        .set_title(title)
        .set_file_name("downova-diagnostics.json")
        .add_filter("JSON", &["json"]);
    let picked = tauri::async_runtime::spawn_blocking(move || dialog.blocking_save_file())
        .await
        .map_err(|_| {
            let mut error = ErrorKind::Internal.to_error_data("the save dialog failed");
            error.next_action = NextAction::None;
            BridgeError::rejected(error)
        })?;
    let Some(path) = picked.and_then(|p| p.into_path().ok()) else {
        return Ok(ExportOutcome::Cancelled);
    };
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("diagnostics")
        .to_owned();
    let replace = path.exists();
    let params = DiagnosticsExportParams {
        snapshot_id,
        path: path
            .to_str()
            .ok_or_else(|| BridgeError::invalid_argument("the chosen path is not valid text"))?
            .to_owned(),
        replace,
    };
    let r: DiagnosticsExportResult =
        call_value(data_dir, methods::DIAGNOSTICS_EXPORT, &params).await?;
    Ok(ExportOutcome::Written {
        file_name,
        size_bytes: r.size_bytes,
        sha256: r.sha256,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outcomes_serialize_with_a_tag_and_no_path() {
        let w = ExportOutcome::Written {
            file_name: "d.json".into(),
            size_bytes: ByteCount(3),
            sha256: "a".repeat(64),
        };
        let v = serde_json::to_value(&w).unwrap();
        assert_eq!(v["outcome"], "written");
        assert_eq!(v["file_name"], "d.json");
        assert!(v.get("path").is_none());
        assert_eq!(
            serde_json::to_value(ExportOutcome::Cancelled).unwrap()["outcome"],
            "cancelled"
        );
    }
}
