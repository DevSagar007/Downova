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
