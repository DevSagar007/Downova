//! Attachment safety adapter (F07, ADR 0026): what the OS offers for a finished download, without
//! any policy. `dm-service` decides what the results mean.
//!
//! Windows: Attachment Services (`IAttachmentExecute::Save`), Mark of the Web
//! (`Zone.Identifier`), volume named-stream support and file identity. Other systems: no
//! attachment integration in this version (reported as not supported), and device/inode identity.

use std::io;
use std::path::Path;

pub use dm_win_attachment::SaveResult;

/// What this OS offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    /// Windows Attachment Services and Mark of the Web.
    WindowsAttachmentServices,
    /// No attachment integration on this OS in this version.
    NotSupported,
}

pub fn capability() -> Capability {
    if cfg!(windows) {
        Capability::WindowsAttachmentServices
    } else {
        Capability::NotSupported
    }
}

/// Runs Windows Attachment Services on `path` with an already sanitized `origin`. Blocks; must
/// run on a thread of its own (it initializes COM for the calling thread).
pub fn save(path: &Path, origin: &str) -> SaveResult {
    dm_win_attachment::save(path, origin)
}

/// Whether the volume holding `path` can store named streams (so MOTW can exist there).
pub fn named_streams_supported(path: &Path) -> Option<bool> {
    dm_win_attachment::named_streams_supported(path)
}

fn zone_stream(path: &Path) -> std::path::PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(":Zone.Identifier");
    std::path::PathBuf::from(s)
}

/// Whether `path` carries a `Zone.Identifier` stream (Windows); `None` elsewhere.
pub fn zone_identifier_present(path: &Path) -> Option<bool> {
    if !cfg!(windows) {
        return None;
    }
    Some(std::fs::metadata(zone_stream(path)).is_ok())
}

/// Writes Mark of the Web (Internet zone) with the sanitized `origin` as `HostUrl`. Used only as the
/// fallback when Attachment Services failed for a reason other than a policy block (ADR 0026); never
/// when it succeeded, so a policy that does not preserve zone information is respected.
pub fn write_zone_identifier(path: &Path, origin: &str) -> io::Result<()> {
    if !cfg!(windows) {
        return Err(io::Error::from(io::ErrorKind::Unsupported));
    }
    if origin.contains(['\r', '\n']) {
        return Err(io::Error::from(io::ErrorKind::InvalidInput));
    }
    std::fs::write(
        zone_stream(path),
        format!("[ZoneTransfer]\r\nZoneId=3\r\nHostUrl={origin}\r\n"),
    )
}

/// A file's identity independent of its name (Windows: volume serial and file index; Unix: device
/// and inode). Two names of one file (a hard link) share it; a new file with the same bytes does not.
pub fn file_identity(path: &Path) -> io::Result<String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let m = std::fs::metadata(path)?;
        Ok(format!("{:x}:{:x}", m.dev(), m.ino()))
    }
    #[cfg(not(unix))]
    {
        dm_win_attachment::file_identity(path)?
            .ok_or_else(|| io::Error::from(io::ErrorKind::Unsupported))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_is_per_file_on_every_platform() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a");
        let b = dir.path().join("b");
        std::fs::write(&a, b"x").unwrap();
        std::fs::hard_link(&a, &b).unwrap();
        assert_eq!(file_identity(&a).unwrap(), file_identity(&b).unwrap());
        std::fs::remove_file(&b).unwrap();
        std::fs::copy(&a, &b).unwrap();
        assert_ne!(file_identity(&a).unwrap(), file_identity(&b).unwrap());
        assert!(file_identity(&dir.path().join("missing")).is_err());
    }

    #[test]
    fn zone_identifier_round_trip_where_supported() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("f.bin");
        std::fs::write(&f, b"x").unwrap();
        if cfg!(windows) && named_streams_supported(&f) == Some(true) {
            assert_eq!(zone_identifier_present(&f), Some(false));
            write_zone_identifier(&f, "https://h.example/").unwrap();
            assert_eq!(zone_identifier_present(&f), Some(true));
            let text = std::fs::read_to_string(zone_stream(&f)).unwrap();
            assert!(text.contains("ZoneId=3") && text.contains("HostUrl=https://h.example/"));
            assert!(write_zone_identifier(&f, "https://h/\r\nZoneId=0").is_err());
        } else {
            assert_eq!(
                zone_identifier_present(&f),
                if cfg!(windows) { Some(false) } else { None }
            );
        }
    }
}
