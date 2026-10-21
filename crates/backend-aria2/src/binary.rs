//! Locating and identifying the aria2c executable.
//!
//! Lookup order: `DM_ARIA2_PATH` → `tools/aria2/aria2c[.exe]` in the workspace (written by
//! `scripts/fetch-aria2.mjs`, pinned + SHA-256 checked) → `PATH`. The version is always read from
//! the binary itself; whether it equals the pinned release is reported, never assumed.

use std::path::{Path, PathBuf};

use dm_backend_api::BackendError;
use dm_domain::ErrorKind;

/// aria2 release this project pins and tests against (REL-05).
pub const PINNED_VERSION: &str = "1.37.0";

const EXE: &str = if cfg!(windows) {
    "aria2c.exe"
} else {
    "aria2c"
};

/// An aria2c executable and the version it reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Aria2Binary {
    pub path: PathBuf,
    pub version: String,
}

impl Aria2Binary {
    /// True when the binary is exactly the pinned release.
    pub fn is_pinned(&self) -> bool {
        self.version == PINNED_VERSION
    }
}

/// Candidate paths in lookup order (existence not checked).
pub fn candidates(workspace_root: Option<&Path>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(explicit) = std::env::var_os("DM_ARIA2_PATH") {
        out.push(PathBuf::from(explicit));
    }
    if let Some(root) = workspace_root {
        out.push(root.join("tools").join("aria2").join(EXE));
    }
    if let Some(path) = std::env::var_os("PATH") {
        out.extend(std::env::split_paths(&path).map(|dir| dir.join(EXE)));
    }
    out
}

/// Finds the first existing candidate and reads its version.
pub async fn locate(workspace_root: Option<&Path>) -> Result<Aria2Binary, BackendError> {
    let path = candidates(workspace_root)
        .into_iter()
        .find(|p| p.is_file())
        .ok_or_else(|| {
            BackendError::new(
                ErrorKind::BackendCapabilityUnavailable,
                "aria2c not found (set DM_ARIA2_PATH or run scripts/fetch-aria2.mjs)",
            )
        })?;
    let version = read_version(&path).await?;
    Ok(Aria2Binary { path, version })
}

async fn read_version(path: &Path) -> Result<String, BackendError> {
    let output = tokio::process::Command::new(path)
        .arg("--version")
        .kill_on_drop(true)
        .output()
        .await
        .map_err(|e| BackendError::new(ErrorKind::Internal, format!("cannot run aria2c: {e}")))?;
    let text = String::from_utf8_lossy(&output.stdout);
    parse_version(&text).ok_or_else(|| {
        BackendError::new(
            ErrorKind::Internal,
            "aria2c --version output not recognised",
        )
    })
}

/// Parses the first line, e.g. `aria2 version 1.37.0`.
pub fn parse_version(text: &str) -> Option<String> {
    let first = text.lines().next()?;
    let version = first.strip_prefix("aria2 version ")?.trim();
    let valid = !version.is_empty() && version.chars().all(|c| c.is_ascii_digit() || c == '.');
    valid.then(|| version.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_version_banner() {
        let banner = "aria2 version 1.37.0\nCopyright (C) 2006, 2019 Tatsuhiro Tsujikawa\n";
        assert_eq!(parse_version(banner).as_deref(), Some("1.37.0"));
        assert_eq!(parse_version("something else"), None);
        assert_eq!(parse_version("aria2 version 1.37.0; rm -rf"), None);
    }

    #[test]
    fn pin_is_exact() {
        let bin = |v: &str| Aria2Binary {
            path: PathBuf::new(),
            version: v.to_owned(),
        };
        assert!(bin("1.37.0").is_pinned());
        assert!(!bin("1.36.0").is_pinned());
    }
}
