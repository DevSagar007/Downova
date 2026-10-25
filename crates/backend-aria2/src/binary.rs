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

/// The TLS implementation an aria2c build links, read from `aria2c --version` (B0.4.1).
///
/// It decides certificate-trust behaviour: OpenSSL and GnuTLS builds load extra CA bundles
/// (`--ca-certificate`); platform builds (WinTLS/SChannel on Windows, AppleTLS on macOS) use only
/// the OS trust store and ignore CA files (upstream `WinTLSContext::addTrustedCACertFile` logs
/// "TLS CA bundle files are not supported" and returns false).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TlsBackend {
    /// OpenSSL, with the version from the "Libraries:" line when present.
    OpenSsl(Option<String>),
    /// GnuTLS, with its version when present.
    GnuTls(Option<String>),
    /// The OS TLS stack (WinTLS on Windows, AppleTLS on macOS): no library is listed.
    Platform,
    /// Built without HTTPS.
    None,
}

impl TlsBackend {
    /// Whether `--ca-certificate` is honoured by this build.
    pub fn honours_ca_file(&self) -> bool {
        matches!(self, TlsBackend::OpenSsl(_) | TlsBackend::GnuTls(_))
    }

    /// Short human description for capability notes and reports.
    pub fn describe(&self) -> String {
        let with = |name: &str, v: &Option<String>| match v {
            Some(v) => format!("{name} {v}"),
            None => name.to_owned(),
        };
        match self {
            TlsBackend::OpenSsl(v) => with("OpenSSL", v),
            TlsBackend::GnuTls(v) => with("GnuTLS", v),
            TlsBackend::Platform if cfg!(windows) => "WinTLS (SChannel, OS trust store)".to_owned(),
            TlsBackend::Platform => "platform TLS (OS trust store)".to_owned(),
            TlsBackend::None => "none (built without HTTPS)".to_owned(),
        }
    }
}

/// An aria2c executable, the version it reports and its TLS backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Aria2Binary {
    pub path: PathBuf,
    pub version: String,
    pub tls: TlsBackend,
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
    // Packaged sidecar: next to the running executable.
    if let Some(dir) = std::env::current_exe()
        .ok()
        .as_deref()
        .and_then(Path::parent)
    {
        out.push(dir.join(EXE));
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
    let (version, tls) = read_version(&path).await?;
    Ok(Aria2Binary { path, version, tls })
}

/// Uses an explicit path (e.g. `dm-service --aria2 PATH`).
pub async fn locate_at(path: &Path) -> Result<Aria2Binary, BackendError> {
    if !path.is_file() {
        return Err(BackendError::new(
            ErrorKind::BackendCapabilityUnavailable,
            "aria2c not found at the given path",
        ));
    }
    let (version, tls) = read_version(path).await?;
    Ok(Aria2Binary {
        path: path.to_owned(),
        version,
        tls,
    })
}

async fn read_version(path: &Path) -> Result<(String, TlsBackend), BackendError> {
    let output = tokio::process::Command::new(path)
        .arg("--version")
        .kill_on_drop(true)
        .output()
        .await
        .map_err(|e| BackendError::new(ErrorKind::Internal, format!("cannot run aria2c: {e}")))?;
    let text = String::from_utf8_lossy(&output.stdout);
    let version = parse_version(&text).ok_or_else(|| {
        BackendError::new(
            ErrorKind::Internal,
            "aria2c --version output not recognised",
        )
    })?;
    Ok((version, parse_tls(&text)))
}

/// Reads the TLS backend from the "Enabled Features:" and "Libraries:" lines.
pub fn parse_tls(text: &str) -> TlsBackend {
    let line = |prefix: &str| {
        text.lines()
            .find_map(|l| l.trim().strip_prefix(prefix).map(str::to_owned))
            .unwrap_or_default()
    };
    let https = line("Enabled Features:")
        .split(',')
        .any(|f| f.trim() == "HTTPS");
    let libs = line("Libraries:");
    let lib = |name: &str| {
        libs.split_whitespace().find_map(|item| {
            let (n, v) = item.split_once('/').unwrap_or((item, ""));
            n.eq_ignore_ascii_case(name)
                .then(|| (!v.is_empty()).then(|| v.to_owned()))
        })
    };
    if let Some(v) = lib("OpenSSL") {
        TlsBackend::OpenSsl(v)
    } else if let Some(v) = lib("GnuTLS") {
        TlsBackend::GnuTls(v)
    } else if https {
        TlsBackend::Platform
    } else {
        TlsBackend::None
    }
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
    fn tls_backend_is_read_from_the_banner() {
        let pinned_windows = "aria2 version 1.37.0
** Configuration **
Enabled Features: Async DNS, BitTorrent, Firefox3 Cookie, GZip, HTTPS, Message Digest, Metalink, XML-RPC, SFTP
Libraries: zlib/1.3 expat/2.5.0 sqlite3/3.43.1 GMP/6.3.0 c-ares/1.19.1 libssh2/1.11.0
";
        assert_eq!(parse_tls(pinned_windows), TlsBackend::Platform);
        assert!(!parse_tls(pinned_windows).honours_ca_file());
        let ubuntu = "aria2 version 1.36.0
Enabled Features: Async DNS, BitTorrent, Firefox3 Cookie, GZip, HTTPS, Message Digest, Metalink, XML-RPC, SFTP
Libraries: zlib/1.2.11 libxml2/2.9.13 sqlite3/3.37.2 GnuTLS/3.7.3 nettle GMP/6.2.1 c-ares/1.18.1 libssh2/1.10.0
";
        assert_eq!(parse_tls(ubuntu), TlsBackend::GnuTls(Some("3.7.3".into())));
        assert!(parse_tls(ubuntu).honours_ca_file());
        let brew = "Enabled Features: Async DNS, GZip, HTTPS
Libraries: zlib/1.2.12 OpenSSL/3.3.1 sqlite3/3.43.2
";
        assert_eq!(parse_tls(brew), TlsBackend::OpenSsl(Some("3.3.1".into())));
        assert_eq!(
            parse_tls(
                "Enabled Features: GZip
Libraries: zlib/1.3
"
            ),
            TlsBackend::None
        );
    }

    #[test]
    fn pin_is_exact() {
        let bin = |v: &str| Aria2Binary {
            path: PathBuf::new(),
            version: v.to_owned(),
            tls: TlsBackend::Platform,
        };
        assert!(bin("1.37.0").is_pinned());
        assert!(!bin("1.36.0").is_pinned());
    }
}
