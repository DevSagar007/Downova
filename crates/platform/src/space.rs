//! Free-space probe for download destinations (B1.4, ADV05 basic; ADR 0009).
//!
//! [`OsSpaceProbe`] asks the operating system how many bytes the current user can still write on
//! the volume holding a directory (`fs4::available_space`: `statvfs` `f_bavail` on Unix,
//! `GetDiskFreeSpaceExW` on Windows; safe Rust). A missing directory is reported as
//! [`Space::VolumeMissing`]; any other failure as [`Space::Unavailable`], never as a number.
//!
//! The value is a **snapshot**: other programs and other downloads can consume the space a moment
//! later, so callers treat it as advisory and still handle a real "disk full" error.
//!
//! Debug builds only: `DOWNOVA_TEST_FREE_SPACE_FILE=<path>` replaces the OS probe for tests. The
//! file holds a byte count, `error` or `missing`, and is re-read on every query.

use std::fmt;
use std::path::Path;
use std::sync::Arc;

/// Result of one free-space query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Space {
    /// Bytes still writable by this user on the volume.
    Available(u64),
    /// The directory (or its volume) does not exist right now (e.g. an unplugged drive).
    VolumeMissing,
    /// The OS could not tell; the reason is diagnostic only.
    Unavailable(String),
}

/// Something that can answer free-space queries (injectable for tests).
pub trait SpaceProbe: Send + Sync + fmt::Debug {
    fn query(&self, dir: &Path) -> Space;
}

/// The operating system's view.
#[derive(Debug, Default, Clone, Copy)]
pub struct OsSpaceProbe;

impl SpaceProbe for OsSpaceProbe {
    fn query(&self, dir: &Path) -> Space {
        match std::fs::metadata(dir) {
            Ok(meta) if meta.is_dir() => {}
            Ok(_) => return Space::Unavailable("destination is not a directory".into()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Space::VolumeMissing,
            Err(e) => return Space::Unavailable(format!("{:?}", e.kind())),
        }
        match fs4::available_space(dir) {
            Ok(bytes) => Space::Available(bytes),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Space::VolumeMissing,
            Err(e) => Space::Unavailable(format!("{:?}", e.kind())),
        }
    }
}

/// Test probe: reads the answer from a file on every query (debug builds; see the module docs).
#[derive(Debug, Clone)]
pub struct FileSpaceProbe {
    pub path: std::path::PathBuf,
}

impl SpaceProbe for FileSpaceProbe {
    fn query(&self, _dir: &Path) -> Space {
        match std::fs::read_to_string(&self.path).map(|s| s.trim().to_owned()) {
            Ok(s) if s == "missing" => Space::VolumeMissing,
            Ok(s) => s.parse::<u64>().map_or_else(
                |_| Space::Unavailable(format!("test probe says {s:?}")),
                Space::Available,
            ),
            Err(e) => Space::Unavailable(format!("test probe unreadable: {:?}", e.kind())),
        }
    }
}

/// The probe the service should use: the OS probe, or the test probe when a debug build is given
/// `DOWNOVA_TEST_FREE_SPACE_FILE`.
pub fn default_probe() -> Arc<dyn SpaceProbe> {
    #[cfg(debug_assertions)]
    if let Some(path) = std::env::var_os("DOWNOVA_TEST_FREE_SPACE_FILE") {
        return Arc::new(FileSpaceProbe { path: path.into() });
    }
    Arc::new(OsSpaceProbe)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_os_reports_free_space_for_an_existing_directory() {
        let dir = tempfile::tempdir().unwrap();
        match OsSpaceProbe.query(dir.path()) {
            Space::Available(n) => assert!(n > 0, "a temp directory has some free space"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_missing_directory_is_a_missing_volume_not_a_number() {
        let dir = tempfile::tempdir().unwrap();
        let gone = dir.path().join("unplugged");
        assert_eq!(OsSpaceProbe.query(&gone), Space::VolumeMissing);
    }

    #[test]
    fn the_test_probe_reads_its_file_each_time() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("free");
        let p = FileSpaceProbe { path: f.clone() };
        std::fs::write(&f, "123").unwrap();
        assert_eq!(p.query(dir.path()), Space::Available(123));
        std::fs::write(&f, "missing").unwrap();
        assert_eq!(p.query(dir.path()), Space::VolumeMissing);
        std::fs::write(&f, "error").unwrap();
        assert!(matches!(p.query(dir.path()), Space::Unavailable(_)));
    }
}
