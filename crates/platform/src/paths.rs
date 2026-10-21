//! Per-user locations. `DOWNOVA_HOME` / `DOWNOVA_DOWNLOAD_DIR` override them (tests, portable use).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

fn home() -> io::Result<PathBuf> {
    env_path(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "home directory not set"))
}

/// Per-user application data directory (not created).
pub fn data_dir() -> io::Result<PathBuf> {
    if let Some(dir) = env_path("DOWNOVA_HOME") {
        return Ok(dir);
    }
    if cfg!(windows) {
        return env_path("LOCALAPPDATA")
            .map(|d| d.join("Downova"))
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "LOCALAPPDATA not set"));
    }
    if cfg!(target_os = "macos") {
        return Ok(home()?.join("Library/Application Support/Downova"));
    }
    match env_path("XDG_DATA_HOME") {
        Some(xdg) => Ok(xdg.join("downova")),
        None => Ok(home()?.join(".local/share/downova")),
    }
}

/// Directory for runtime files (endpoint file, Unix socket, lock): `<data>/run`.
pub fn run_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("run")
}

/// Default destination for the `downloads` profile.
pub fn default_download_dir() -> io::Result<PathBuf> {
    if let Some(dir) = env_path("DOWNOVA_DOWNLOAD_DIR") {
        return Ok(dir);
    }
    Ok(home()?.join("Downloads"))
}

/// Creates `dir` (and parents) and restricts it to the current user on Unix (0700).
/// On Windows, per-user profile directories already carry a user-only ACL.
pub fn ensure_private_dir(dir: &Path) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Writes a file readable only by the current user (0600 on Unix), atomically via rename.
pub fn write_private_file(path: &Path, contents: &[u8]) -> io::Result<()> {
    use std::io::Write;
    let tmp = path.with_extension("tmp");
    {
        let mut options = fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&tmp)?;
        file.write_all(contents)?;
        file.sync_all()?;
    }
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_file_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("x.json");
        write_private_file(&target, b"{}").unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"{}");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&target).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
    }
}
