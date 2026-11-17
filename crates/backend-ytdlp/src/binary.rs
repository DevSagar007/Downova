//! Locates an explicitly installed yt-dlp. No download, update or executable discovery from URLs.
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use dm_backend_api::BackendError;
use dm_domain::ErrorKind;

/// Stable release exercised by the offline fixtures and fetched by `pnpm fetch:ytdlp`.
pub const PINNED_VERSION: &str = "2026.10.26";
const EXE: &str = if cfg!(windows) {
    "yt-dlp.exe"
} else {
    "yt-dlp"
};

#[derive(Debug, Clone)]
pub struct YtdlpBinary {
    pub path: PathBuf,
    pub version: String,
}

/// Lookup order: explicit environment override, workspace tools, PATH. An invalid explicit
/// override is an error, never a silent fallback to a different program.
pub async fn locate(workspace_root: Option<&Path>) -> Result<YtdlpBinary, BackendError> {
    if let Some(path) = std::env::var_os("DM_YTDLP_PATH") {
        return locate_at(Path::new(&path)).await;
    }
    let mut paths = Vec::new();
    if let Some(root) = workspace_root {
        paths.push(root.join("tools/ytdlp").join(EXE));
    }
    if let Some(path) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&path).map(|dir| dir.join(EXE)));
    }
    let path = paths.into_iter().find(|p| p.is_file()).ok_or_else(|| {
        BackendError::new(
            ErrorKind::BackendCapabilityUnavailable,
            "yt-dlp not found; run pnpm fetch:ytdlp or set DM_YTDLP_PATH",
        )
    })?;
    locate_at(&path).await
}

pub async fn locate_at(path: &Path) -> Result<YtdlpBinary, BackendError> {
    let path = path.canonicalize().map_err(|_| {
        BackendError::new(
            ErrorKind::BackendCapabilityUnavailable,
            "yt-dlp not found at the configured path",
        )
    })?;
    if !path.is_file() {
        return Err(BackendError::new(
            ErrorKind::BackendCapabilityUnavailable,
            "the configured yt-dlp is not a file",
        ));
    }
    // The version probe never inherits input, config or logs; it is bounded and killed on timeout.
    let output = tokio::time::timeout(
        Duration::from_secs(15),
        tokio::process::Command::new(&path)
            .args(["--ignore-config", "--version"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| {
        BackendError::new(
            ErrorKind::ServiceUnavailable,
            "yt-dlp version check timed out",
        )
    })?
    .map_err(|_| {
        BackendError::new(
            ErrorKind::ServiceUnavailable,
            "cannot run the configured yt-dlp",
        )
    })?;
    let text = String::from_utf8_lossy(&output.stdout);
    let version = text.trim();
    if !output.status.success()
        || version.len() > 40
        || version.is_empty()
        || !version.bytes().all(|b| b.is_ascii_digit() || b == b'.')
    {
        return Err(BackendError::new(
            ErrorKind::ServiceUnavailable,
            "yt-dlp --version did not return a recognized version",
        ));
    }
    Ok(YtdlpBinary {
        path,
        version: version.to_owned(),
    })
}
