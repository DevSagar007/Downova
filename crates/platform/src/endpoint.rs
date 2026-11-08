//! Endpoint discovery: the running service records where it listens in a user-private file.
//!
//! On Windows the pipe name contains a random suffix, so other users cannot predict and squat on
//! it; only the owning user can read the file that reveals it. On Unix the socket path itself
//! lives in the 0700 run directory.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::paths::{run_dir, write_private_file};

/// Contents of `<data>/run/endpoint.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EndpointInfo {
    pub version: u32,
    /// Named-pipe name (Windows) or socket path (Unix).
    pub endpoint: String,
    pub pid: u32,
    /// The running service's own executable (B3.6): lets an installer stop only the service of
    /// its own installation. Absent in files written before B3.6; readers ignore unknown fields.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exe: Option<PathBuf>,
}

/// Whether the service recorded in `info` runs from inside `install_dir` (both canonicalised).
/// A record without an executable, or paths that cannot be resolved, are never "ours".
pub fn runs_from(info: &EndpointInfo, install_dir: &Path) -> bool {
    let (Some(exe), Ok(dir)) = (info.exe.as_deref(), install_dir.canonicalize()) else {
        return false;
    };
    exe.canonicalize().is_ok_and(|exe| exe.starts_with(&dir))
}

pub fn endpoint_file(data_dir: &Path) -> PathBuf {
    run_dir(data_dir).join("endpoint.json")
}

pub fn write(data_dir: &Path, info: &EndpointInfo) -> io::Result<()> {
    let bytes = serde_json::to_vec(info).map_err(io::Error::other)?;
    write_private_file(&endpoint_file(data_dir), &bytes)
}

/// Reads the endpoint file. `NotFound` means no service has started for this user.
pub fn read(data_dir: &Path) -> io::Result<EndpointInfo> {
    let bytes = std::fs::read(endpoint_file(data_dir))?;
    serde_json::from_slice(&bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Removes the endpoint file if it still names `endpoint` (never another instance's file).
pub fn remove_if_ours(data_dir: &Path, endpoint: &str) {
    if read(data_dir).is_ok_and(|info| info.endpoint == endpoint) {
        let _ = std::fs::remove_file(endpoint_file(data_dir));
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn only_a_service_inside_the_installation_is_ours() {
        let root = tempfile::tempdir().unwrap();
        let install = root.path().join("install");
        let other = root.path().join("other");
        for d in [&install, &other] {
            std::fs::create_dir_all(d).unwrap();
            std::fs::write(d.join("dm-service.exe"), b"x").unwrap();
        }
        let info = |exe: Option<PathBuf>| EndpointInfo {
            version: 1,
            endpoint: "e".into(),
            pid: 1,
            exe,
        };
        assert!(runs_from(
            &info(Some(install.join("dm-service.exe"))),
            &install
        ));
        // Same file name in another installation, a record without an executable (written
        // before B3.6), and a folder that does not exist are never ours.
        assert!(!runs_from(
            &info(Some(other.join("dm-service.exe"))),
            &install
        ));
        assert!(!runs_from(&info(None), &install));
        assert!(!runs_from(
            &info(Some(install.join("dm-service.exe"))),
            &root.path().join("missing")
        ));
        // A pre-B3.6 file (no `exe`) still parses, and `exe` is omitted when unknown.
        let old: EndpointInfo =
            serde_json::from_str(r#"{"version":1,"endpoint":"e","pid":7}"#).unwrap();
        assert_eq!(old.exe, None);
        assert!(!serde_json::to_string(&old).unwrap().contains("exe"));
    }
}
