//! Per-user Chrome-family native-messaging host registration (B0.6).
//!
//! The manifest names this host (`com.downova.host`), its absolute path, `type: stdio` and an
//! exact `allowed_origins` list. Locations follow the official per-OS rules:
//! - **Windows:** the manifest file lives under `%LOCALAPPDATA%\Downova\native-messaging\`; a
//!   per-user registry key `HKCU\Software\<vendor>\NativeMessagingHosts\com.downova.host` points to
//!   it (Chrome and its Beta/Dev/Canary channels share Google's key; Chromium and Edge have their
//!   own).
//! - **macOS:** `~/Library/Application Support/<browser dir>/NativeMessagingHosts/`.
//! - **Linux:** `~/.config/<browser dir>/NativeMessagingHosts/`.
//!
//! Only Downova-owned entries are ever written or removed: our host name, and on removal only a
//! manifest file whose `name` is ours. `--root` (all OSes) and `--registry-root` (Windows) redirect
//! everything for tests, so a developer's real browser configuration is never touched by them.

use std::path::{Path, PathBuf};

/// Native-messaging host name (reverse-DNS, lowercase as Chrome requires).
pub const HOST_NAME: &str = "com.downova.host";

/// Chrome-family browsers and channels with distinct registration locations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Browser {
    Chrome,
    ChromeBeta,
    ChromeDev,
    ChromeCanary,
    Chromium,
    Edge,
}

impl Browser {
    pub const ALL: [Browser; 6] = [
        Browser::Chrome,
        Browser::ChromeBeta,
        Browser::ChromeDev,
        Browser::ChromeCanary,
        Browser::Chromium,
        Browser::Edge,
    ];

    pub fn parse(s: &str) -> Option<Browser> {
        Some(match s {
            "chrome" => Browser::Chrome,
            "chrome-beta" => Browser::ChromeBeta,
            "chrome-dev" => Browser::ChromeDev,
            "chrome-canary" => Browser::ChromeCanary,
            "chromium" => Browser::Chromium,
            "edge" => Browser::Edge,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Browser::Chrome => "chrome",
            Browser::ChromeBeta => "chrome-beta",
            Browser::ChromeDev => "chrome-dev",
            Browser::ChromeCanary => "chrome-canary",
            Browser::Chromium => "chromium",
            Browser::Edge => "edge",
        }
    }

    /// Manifest directory relative to the home directory (macOS, Linux).
    fn unix_dir(self, macos: bool) -> Option<&'static str> {
        Some(match (macos, self) {
            (true, Browser::Chrome) => "Library/Application Support/Google/Chrome",
            (true, Browser::ChromeBeta) => "Library/Application Support/Google/Chrome Beta",
            (true, Browser::ChromeDev) => "Library/Application Support/Google/Chrome Dev",
            (true, Browser::ChromeCanary) => "Library/Application Support/Google/Chrome Canary",
            (true, Browser::Chromium) => "Library/Application Support/Chromium",
            (true, Browser::Edge) => "Library/Application Support/Microsoft Edge",
            (false, Browser::Chrome) => ".config/google-chrome",
            (false, Browser::ChromeBeta) => ".config/google-chrome-beta",
            (false, Browser::ChromeDev) => ".config/google-chrome-unstable",
            // Chrome Canary is not distributed for Linux.
            (false, Browser::ChromeCanary) => return None,
            (false, Browser::Chromium) => ".config/chromium",
            (false, Browser::Edge) => ".config/microsoft-edge",
        })
    }

    /// Registry key below `HKCU\Software` (Windows).
    fn registry_vendor(self) -> &'static str {
        match self {
            Browser::Chrome | Browser::ChromeBeta | Browser::ChromeDev | Browser::ChromeCanary => {
                r"Google\Chrome"
            }
            Browser::Chromium => "Chromium",
            Browser::Edge => r"Microsoft\Edge",
        }
    }
}

/// Where registration happens.
#[derive(Debug, Clone)]
pub struct Target {
    pub browser: Browser,
    /// Replaces the home directory (macOS/Linux) or `%LOCALAPPDATA%` (Windows), for tests.
    pub root: Option<PathBuf>,
    /// Replaces `HKCU\Software` (Windows), for tests, e.g. `HKCU\Software\Downova-Test-1234`.
    pub registry_root: Option<String>,
}

/// The manifest file for this target.
pub fn manifest_path(target: &Target) -> Result<PathBuf, String> {
    let file = format!("{HOST_NAME}.json");
    if cfg!(windows) {
        let base = match &target.root {
            Some(r) => r.clone(),
            None => {
                PathBuf::from(std::env::var_os("LOCALAPPDATA").ok_or("LOCALAPPDATA is not set")?)
            }
        };
        // One manifest per browser key, so unregistering one browser never breaks another.
        return Ok(base
            .join("Downova")
            .join("native-messaging")
            .join(target.browser.name())
            .join(file));
    }
    let home = match &target.root {
        Some(r) => r.clone(),
        None => PathBuf::from(std::env::var_os("HOME").ok_or("HOME is not set")?),
    };
    let dir = target
        .browser
        .unix_dir(cfg!(target_os = "macos"))
        .ok_or_else(|| format!("{} is not available on this OS", target.browser.name()))?;
    Ok(home.join(dir).join("NativeMessagingHosts").join(file))
}

/// The registry key for this target (Windows).
pub fn registry_key(target: &Target) -> String {
    let root = target
        .registry_root
        .clone()
        .unwrap_or_else(|| r"HKCU\Software".to_owned());
    format!(
        r"{root}\{}\NativeMessagingHosts\{HOST_NAME}",
        target.browser.registry_vendor()
    )
}

/// The manifest JSON.
pub fn manifest_json(host: &Path, origins: &[String]) -> String {
    let doc = serde_json::json!({
        "name": HOST_NAME,
        "description": "Downova download manager — hands explicit public http(s) links to the per-user Downova service",
        "path": host.display().to_string(),
        "type": "stdio",
        "allowed_origins": origins,
    });
    serde_json::to_string_pretty(&doc).unwrap_or_default() + "\n"
}

/// Refuses executable paths that will not survive (temporary AppImage mounts) or are relative.
pub fn check_host_path(host: &Path) -> Result<(), String> {
    if !host.is_absolute() {
        return Err("the host path must be absolute".into());
    }
    let shown = host.display().to_string();
    let appdir = std::env::var_os("APPDIR").map(PathBuf::from);
    if shown.contains("/.mount_") || appdir.is_some_and(|d| host.starts_with(d)) {
        return Err(
            "this path is inside a running AppImage mount, which disappears when the app exits; extract the AppImage or install the .deb and register that path".into(),
        );
    }
    if !host.is_file() {
        return Err(format!("no host executable at {shown}"));
    }
    Ok(())
}

fn reg(args: &[&str]) -> Result<String, String> {
    let out = std::process::Command::new("reg")
        .args(args)
        .output()
        .map_err(|e| format!("cannot run reg.exe: {}", e.kind()))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_owned())
    }
}

/// Writes (or overwrites) our manifest and, on Windows, our registry value.
pub fn register(target: &Target, host: &Path, origins: &[String]) -> Result<PathBuf, String> {
    check_host_path(host)?;
    let path = manifest_path(target)?;
    let dir = path.parent().ok_or("invalid manifest path")?;
    std::fs::create_dir_all(dir)
        .map_err(|e| format!("cannot create {}: {}", dir.display(), e.kind()))?;
    // Atomic replace: a browser never reads a half-written manifest.
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, manifest_json(host, origins))
        .map_err(|e| format!("cannot write the manifest: {}", e.kind()))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("cannot write the manifest: {}", e.kind()))?;
    if cfg!(windows) {
        let key = registry_key(target);
        let value = path.display().to_string();
        reg(&["add", &key, "/ve", "/t", "REG_SZ", "/d", &value, "/f"])?;
    }
    Ok(path)
}

/// True when `path` is a manifest written for our host name.
fn is_ours(path: &Path) -> bool {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .is_some_and(|v| v["name"] == HOST_NAME)
}

/// Removes our registry value and our manifest; anything else is left untouched.
pub fn unregister(target: &Target) -> Result<Vec<String>, String> {
    let mut removed = Vec::new();
    if cfg!(windows) {
        let key = registry_key(target);
        if reg(&["query", &key, "/ve"]).is_ok() {
            reg(&["delete", &key, "/f"])?;
            removed.push(key);
        }
    }
    let path = manifest_path(target)?;
    if path.is_file() {
        if !is_ours(&path) {
            return Err(format!(
                "{} is not a Downova manifest; left untouched",
                path.display()
            ));
        }
        std::fs::remove_file(&path)
            .map_err(|e| format!("cannot remove {}: {}", path.display(), e.kind()))?;
        removed.push(path.display().to_string());
    }
    Ok(removed)
}

/// What is currently registered for this target (for `status`).
pub fn status(target: &Target) -> String {
    let path = manifest_path(target).map(|p| p.display().to_string());
    let manifest = match &path {
        Ok(p) if Path::new(p).is_file() => std::fs::read_to_string(p).unwrap_or_default(),
        Ok(_) => "(no manifest)".into(),
        Err(e) => e.clone(),
    };
    let registry = if cfg!(windows) {
        match reg(&["query", &registry_key(target), "/ve"]) {
            Ok(out) => out.trim().to_owned(),
            Err(_) => "(no registry value)".into(),
        }
    } else {
        "(not used on this OS)".into()
    };
    format!(
        "{}: manifest {}\n{manifest}\nregistry: {registry}\n",
        target.browser.name(),
        path.unwrap_or_default()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(root: &Path, browser: Browser) -> Target {
        Target {
            browser,
            root: Some(root.to_owned()),
            registry_root: None,
        }
    }

    #[test]
    fn manifest_has_exact_origins_and_stdio() {
        let host = PathBuf::from(if cfg!(windows) {
            r"C:\Apps\Downova\dm-browser-host.exe"
        } else {
            "/usr/bin/dm-browser-host"
        });
        let origins = vec!["chrome-extension://bkmpnfclpdmcbdgmeakmnoilmkdmdppf/".to_owned()];
        let v: serde_json::Value = serde_json::from_str(&manifest_json(&host, &origins)).unwrap();
        assert_eq!(v["name"], HOST_NAME);
        assert_eq!(v["type"], "stdio");
        assert_eq!(v["path"], host.display().to_string());
        assert_eq!(v["allowed_origins"], serde_json::json!(origins));
        assert!(
            v.get("allowed_extensions").is_none(),
            "Chrome uses allowed_origins only"
        );
    }

    #[test]
    fn locations_differ_per_browser_and_channel() {
        let root = Path::new("/r");
        let paths: Vec<_> = Browser::ALL
            .iter()
            .filter_map(|b| manifest_path(&target(root, *b)).ok())
            .collect();
        let mut unique = paths.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), paths.len(), "{paths:?}");
        assert!(paths.iter().all(|p| p.starts_with(root)));
        let t = Target {
            browser: Browser::Edge,
            root: None,
            registry_root: Some(r"HKCU\Software\T".into()),
        };
        assert_eq!(
            registry_key(&t),
            r"HKCU\Software\T\Microsoft\Edge\NativeMessagingHosts\com.downova.host"
        );
    }

    #[test]
    fn appimage_mounts_and_relative_paths_are_refused() {
        assert!(check_host_path(Path::new("relative/host")).is_err());
        assert!(
            check_host_path(Path::new("/tmp/.mount_DownovXYZ/usr/bin/dm-browser-host")).is_err()
        );
    }

    #[test]
    fn foreign_manifests_are_never_removed() {
        let dir = tempfile::tempdir().unwrap();
        let t = target(dir.path(), Browser::Chromium);
        let path = manifest_path(&t).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, r#"{"name":"com.other.host"}"#).unwrap();
        if !cfg!(windows) {
            assert!(unregister(&t).is_err());
            assert!(path.exists(), "someone else's file survives");
        }
    }
}
