//! Per-user Chrome-family native-messaging host registration (B0.6, B2.5 ownership).
//!
//! The manifest names this host (`com.downova.host`), its absolute path, `type: stdio` and an
//! exact `allowed_origins` list. Locations follow the official per-OS rules:
//! - **Windows:** the manifest file lives under `%LOCALAPPDATA%\Downova\native-messaging\`; a
//!   per-user registry key `HKCU\Software\<vendor>\NativeMessagingHosts\com.downova.host` points to
//!   it. Chrome, its channels and Chrome for Testing share Google's key and therefore one manifest;
//!   Chromium and Edge have their own.
//! - **macOS:** `~/Library/Application Support/<browser dir>/NativeMessagingHosts/`.
//! - **Linux:** `~/.config/<browser dir>/NativeMessagingHosts/` (`$XDG_CONFIG_HOME` when set).
//!
//! **Ownership (B2.5).** The host name alone does not prove an entry is ours: another Downova
//! installation (or a development build) writes the same name. An entry belongs to *this*
//! installation only when its manifest names `com.downova.host` **and** its `path` is this
//! installation's host executable. Every write and removal inspects ownership first:
//! - `register` writes over nothing, our own entry, or a stale Downova entry whose executable no
//!   longer exists. It refuses another live installation or an unreadable file unless `replace`
//!   is set, and never replaces a manifest that names a different host.
//! - `unregister` removes only entries that are ours and reports everything it kept.
//!
//! `--root` (all OSes) and `--registry-root` (Windows) redirect everything for tests, so a
//! developer's real browser configuration is never touched by them.

use std::path::{Path, PathBuf};

use serde::Serialize;

/// Native-messaging host name (reverse-DNS, lowercase as Chrome requires).
pub const HOST_NAME: &str = "com.downova.host";

/// Chrome-family browsers and channels with distinct registration locations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Browser {
    Chrome,
    ChromeBeta,
    ChromeDev,
    ChromeCanary,
    ChromeForTesting,
    Chromium,
    Edge,
}

impl Browser {
    pub const ALL: [Browser; 7] = [
        Browser::Chrome,
        Browser::ChromeBeta,
        Browser::ChromeDev,
        Browser::ChromeCanary,
        Browser::ChromeForTesting,
        Browser::Chromium,
        Browser::Edge,
    ];

    pub fn parse(s: &str) -> Option<Browser> {
        Some(match s {
            "chrome" => Browser::Chrome,
            "chrome-beta" => Browser::ChromeBeta,
            "chrome-dev" => Browser::ChromeDev,
            "chrome-canary" => Browser::ChromeCanary,
            "chrome-for-testing" => Browser::ChromeForTesting,
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
            Browser::ChromeForTesting => "chrome-for-testing",
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
            // Chrome 146+; earlier Chrome for Testing read Google Chrome's location.
            (true, Browser::ChromeForTesting) => {
                "Library/Application Support/Google/ChromeForTesting"
            }
            (true, Browser::Chromium) => "Library/Application Support/Chromium",
            (true, Browser::Edge) => "Library/Application Support/Microsoft Edge",
            (false, Browser::Chrome) => ".config/google-chrome",
            (false, Browser::ChromeBeta) => ".config/google-chrome-beta",
            (false, Browser::ChromeDev) => ".config/google-chrome-unstable",
            // Chrome Canary is not distributed for Linux.
            (false, Browser::ChromeCanary) => return None,
            (false, Browser::ChromeForTesting) => ".config/google-chrome-for-testing",
            (false, Browser::Chromium) => ".config/chromium",
            (false, Browser::Edge) => ".config/microsoft-edge",
        })
    }

    /// Registry key below `HKCU\Software` (Windows).
    fn registry_vendor(self) -> &'static str {
        match self {
            Browser::Chrome
            | Browser::ChromeBeta
            | Browser::ChromeDev
            | Browser::ChromeCanary
            | Browser::ChromeForTesting => r"Google\Chrome",
            Browser::Chromium => "Chromium",
            Browser::Edge => r"Microsoft\Edge",
        }
    }

    /// The Windows manifest directory: one per registry key, so browsers sharing a key share it.
    fn windows_dir(self) -> &'static str {
        match self.registry_vendor() {
            "Chromium" => "chromium",
            r"Microsoft\Edge" => "edge",
            _ => "chrome",
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

/// Who a registration entry belongs to, relative to this installation's host executable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "owner", content = "host", rename_all = "snake_case")]
pub enum Owner {
    Absent,
    /// A Downova manifest naming this installation's host.
    Ours,
    /// A Downova manifest whose host executable no longer exists (an uninstalled or moved copy).
    Stale(String),
    /// A Downova manifest naming another, existing host executable.
    OtherInstallation(String),
    /// Present but not readable JSON: ownership cannot be proven.
    Unreadable,
    /// Valid JSON naming a different host.
    Foreign(String),
}

impl Owner {
    fn describe(&self) -> String {
        match self {
            Owner::Absent => "absent".into(),
            Owner::Ours => "this installation".into(),
            Owner::Stale(p) => format!("a removed Downova installation ({p})"),
            Owner::OtherInstallation(p) => format!("another Downova installation ({p})"),
            Owner::Unreadable => "unreadable (ownership unknown)".into(),
            Owner::Foreign(name) => format!("a different host ({name})"),
        }
    }

    /// May `register` write over this entry?
    fn writable(&self, replace: bool) -> bool {
        match self {
            Owner::Absent | Owner::Ours | Owner::Stale(_) => true,
            Owner::OtherInstallation(_) | Owner::Unreadable => replace,
            Owner::Foreign(_) => false,
        }
    }
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
        return Ok(base
            .join("Downova")
            .join("native-messaging")
            .join(target.browser.windows_dir())
            .join(file));
    }
    let dir = target
        .browser
        .unix_dir(cfg!(target_os = "macos"))
        .ok_or_else(|| format!("{} is not available on this OS", target.browser.name()))?;
    // Linux browsers keep their user-data directory in $XDG_CONFIG_HOME when it is set.
    let xdg = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute());
    if let (None, Some(xdg), Some(rest)) = (&target.root, xdg, dir.strip_prefix(".config/")) {
        return Ok(xdg.join(rest).join("NativeMessagingHosts").join(file));
    }
    let home = match &target.root {
        Some(r) => r.clone(),
        None => PathBuf::from(std::env::var_os("HOME").ok_or("HOME is not set")?),
    };
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

/// Why `shown` is a location that disappears or moves, if it is one.
fn unstable_location(shown: &str, appdir: Option<&Path>) -> Option<&'static str> {
    if shown.contains("/.mount_") || appdir.is_some_and(|d| Path::new(shown).starts_with(d)) {
        return Some(
            "this path is inside a running AppImage mount, which disappears when the app exits; extract the AppImage or install the .deb and register that path",
        );
    }
    if shown.starts_with("/Volumes/") {
        return Some(
            "this path is on a mounted volume (for example the installer disk image); move Downova to Applications, open it once, and register from there",
        );
    }
    if shown.contains("/AppTranslocation/") {
        return Some(
            "macOS is running Downova from a temporary translocated copy; move Downova to Applications, open it once, and register from there",
        );
    }
    None
}

/// Refuses executable paths that will not survive (AppImage, disk-image or translocated mounts)
/// or are relative.
pub fn check_host_path(host: &Path) -> Result<(), String> {
    if !host.is_absolute() {
        return Err("the host path must be absolute".into());
    }
    let shown = host.display().to_string();
    let appdir = std::env::var_os("APPDIR").map(PathBuf::from);
    if let Some(why) = unstable_location(&shown, appdir.as_deref()) {
        return Err(why.into());
    }
    if !host.is_file() {
        return Err(format!("no host executable at {shown}"));
    }
    Ok(())
}

/// The same file, allowing for Windows' case-insensitive paths and differing spellings.
fn same_path(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    if cfg!(windows) {
        let norm = |p: &Path| p.display().to_string().replace('/', "\\").to_lowercase();
        if norm(a) == norm(b) {
            return true;
        }
    }
    matches!((a.canonicalize(), b.canonicalize()), (Ok(x), Ok(y)) if x == y)
}

/// Who owns the manifest at `path`, relative to `host`.
pub fn inspect(path: &Path, host: &Path) -> Owner {
    let Ok(text) = std::fs::read_to_string(path) else {
        return if path.exists() {
            Owner::Unreadable
        } else {
            Owner::Absent
        };
    };
    let Ok(doc) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Owner::Unreadable;
    };
    let name = doc["name"].as_str().unwrap_or_default();
    if name != HOST_NAME {
        return Owner::Foreign(if name.is_empty() {
            "no name".into()
        } else {
            name.to_owned()
        });
    }
    let Some(named) = doc["path"].as_str() else {
        return Owner::Unreadable;
    };
    let named = Path::new(named);
    if same_path(named, host) {
        Owner::Ours
    } else if named.is_file() {
        Owner::OtherInstallation(named.display().to_string())
    } else {
        Owner::Stale(named.display().to_string())
    }
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

/// The registry key's default value (Windows): the manifest path it points to, if any.
fn registry_value(key: &str) -> Option<String> {
    // reg.exe fails for a missing key with a localized message, so any failure reads as absent
    // (the key is under the user's own HKCU, where reading is not denied).
    let Ok(out) = reg(&["query", key, "/ve"]) else {
        return None;
    };
    out.lines().find_map(|line| {
        let (_, value) = line.split_once("REG_SZ")?;
        let value = value.trim();
        (!value.is_empty()).then(|| value.to_owned())
    })
}

/// The registry entry's owner: the owner of the manifest it points to (Windows).
fn registry_owner(value: Option<&str>, host: &Path) -> Owner {
    match value {
        None => Owner::Absent,
        // A key pointing at a missing file registers nothing; it is dangling, not anyone's host.
        Some(v) if !Path::new(v).exists() => Owner::Stale(v.to_owned()),
        Some(v) => inspect(Path::new(v), host),
    }
}

/// What `register` did for one browser.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Registered {
    pub manifest: PathBuf,
    /// The manifest's owner before the write.
    pub previous: Owner,
    /// The registry key written (Windows only).
    pub registry_key: Option<String>,
}

fn refusal(what: &str, owner: &Owner) -> String {
    let hint = match owner {
        Owner::Foreign(_) => "it is never replaced",
        _ => "pass --replace to register this installation instead",
    };
    format!("{what} belongs to {}; {hint}", owner.describe())
}

/// Writes our manifest and, on Windows, our registry value, after checking that neither belongs
/// to someone else. Repeating it is safe: it rewrites our own entries (which is also `repair`).
pub fn register(
    target: &Target,
    host: &Path,
    origins: &[String],
    replace: bool,
) -> Result<Registered, String> {
    check_host_path(host)?;
    let path = manifest_path(target)?;
    let previous = inspect(&path, host);
    if !previous.writable(replace) {
        return Err(refusal(&path.display().to_string(), &previous));
    }
    // Inspect everything before writing anything, so a refusal leaves no partial change.
    let key = cfg!(windows).then(|| registry_key(target));
    if let Some(key) = &key {
        let value = registry_value(key);
        let points_elsewhere = value
            .as_deref()
            .is_some_and(|v| !same_path(Path::new(v), &path));
        if points_elsewhere {
            let owner = registry_owner(value.as_deref(), host);
            if !owner.writable(replace) {
                return Err(refusal(key, &owner));
            }
        }
    }
    let dir = path.parent().ok_or("invalid manifest path")?;
    std::fs::create_dir_all(dir)
        .map_err(|e| format!("cannot create {}: {}", dir.display(), e.kind()))?;
    // Atomic replace: a browser never reads a half-written manifest.
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, manifest_json(host, origins))
        .map_err(|e| format!("cannot write the manifest: {}", e.kind()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Readable by the browser, writable only by the user.
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o644))
            .map_err(|e| format!("cannot set the manifest's permissions: {}", e.kind()))?;
    }
    std::fs::rename(&tmp, &path).map_err(|e| format!("cannot write the manifest: {}", e.kind()))?;
    if let Some(key) = &key {
        let value = path.display().to_string();
        if let Err(e) = reg(&["add", key, "/ve", "/t", "REG_SZ", "/d", &value, "/f"]) {
            // Undo a manifest that did not exist before, so no half registration remains.
            if previous == Owner::Absent {
                let _ = std::fs::remove_file(&path);
                return Err(format!("cannot set {key}: {e}; nothing was registered"));
            }
            return Err(format!(
                "the manifest {} was written but {key} could not be set: {e}; run `dm-browser-host repair`",
                path.display()
            ));
        }
    }
    Ok(Registered {
        manifest: path,
        previous,
        registry_key: key,
    })
}

/// What `unregister` did for one browser.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Removal {
    pub removed: Vec<String>,
    /// Entries left in place because they are not this installation's, with the reason.
    pub kept: Vec<String>,
}

/// Removes this installation's registry value and manifest; anything else is kept and reported.
pub fn unregister(target: &Target, host: &Path) -> Result<Removal, String> {
    let mut out = Removal::default();
    let path = manifest_path(target)?;
    // Decide the registry entry's owner before the manifest it points to can disappear.
    if cfg!(windows) {
        let key = registry_key(target);
        let value = registry_value(&key);
        match registry_owner(value.as_deref(), host) {
            Owner::Absent => {}
            Owner::Ours => {
                reg(&["delete", &key, "/f"]).map_err(|e| format!("cannot remove {key}: {e}"))?;
                out.removed.push(key);
            }
            other => out.kept.push(format!("{key}: {}", other.describe())),
        }
    }
    match inspect(&path, host) {
        Owner::Absent => {}
        Owner::Ours => {
            std::fs::remove_file(&path)
                .map_err(|e| format!("cannot remove {}: {}", path.display(), e.kind()))?;
            out.removed.push(path.display().to_string());
            if cfg!(windows) {
                // Our own per-browser directory; removed only when empty.
                if let Some(dir) = path.parent() {
                    let _ = std::fs::remove_dir(dir);
                }
            }
        }
        other => out
            .kept
            .push(format!("{}: {}", path.display(), other.describe())),
    }
    Ok(out)
}

/// What is registered for one browser, and whose it is.
#[derive(Debug, Clone, Serialize)]
pub struct Status {
    pub browser: &'static str,
    pub manifest: String,
    #[serde(flatten)]
    pub manifest_owner: Owner,
    pub registry_key: Option<String>,
    pub registry_value: Option<String>,
    pub registry_owner: Option<Owner>,
}

pub fn status(target: &Target, host: &Path) -> Result<Status, String> {
    let path = manifest_path(target)?;
    let (registry_key, registry_value, registry_owner) = if cfg!(windows) {
        let key = registry_key(target);
        let value = registry_value(&key);
        let owner = registry_owner(value.as_deref(), host);
        (Some(key), value, Some(owner))
    } else {
        (None, None, None)
    };
    Ok(Status {
        browser: target.browser.name(),
        manifest: path.display().to_string(),
        manifest_owner: inspect(&path, host),
        registry_key,
        registry_value,
        registry_owner,
    })
}

impl Status {
    pub fn text(&self) -> String {
        let mut s = format!(
            "{}: manifest {} — {}",
            self.browser,
            self.manifest,
            self.manifest_owner.describe()
        );
        if let (Some(key), Some(owner)) = (&self.registry_key, &self.registry_owner) {
            s.push_str(&format!(
                "\n  registry {key} → {} — {}",
                self.registry_value.as_deref().unwrap_or("(none)"),
                owner.describe()
            ));
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    const ORIGIN: &str = "chrome-extension://bkmpnfclpdmcbdgmeakmnoilmkdmdppf/";

    fn origins() -> Vec<String> {
        vec![ORIGIN.to_owned()]
    }

    /// A per-test sandbox: a file root and, on Windows, a private `HKCU\Software\…` subtree that
    /// is deleted afterwards.
    struct Sandbox {
        dir: tempfile::TempDir,
        registry_root: String,
    }

    impl Sandbox {
        fn new() -> Sandbox {
            static N: AtomicU32 = AtomicU32::new(0);
            let n = N.fetch_add(1, Ordering::Relaxed);
            Sandbox {
                dir: tempfile::tempdir().unwrap(),
                registry_root: format!(r"HKCU\Software\Downova-Test-{}-{n}", std::process::id()),
            }
        }

        fn target(&self, browser: Browser) -> Target {
            Target {
                browser,
                root: Some(self.dir.path().join("home")),
                registry_root: Some(self.registry_root.clone()),
            }
        }

        /// A fake host executable for one installation.
        fn host(&self, install: &str) -> PathBuf {
            let dir = self.dir.path().join(install);
            std::fs::create_dir_all(&dir).unwrap();
            let host = dir.join("dm-browser-host.exe");
            std::fs::write(&host, b"host").unwrap();
            host
        }
    }

    impl Drop for Sandbox {
        fn drop(&mut self) {
            if cfg!(windows) {
                let _ = reg(&["delete", &self.registry_root, "/f"]);
            }
        }
    }

    fn manifest(path: &Path) -> serde_json::Value {
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn manifest_has_exact_origins_and_stdio() {
        let host = PathBuf::from(if cfg!(windows) {
            r"C:\Apps\Downova\dm-browser-host.exe"
        } else {
            "/usr/bin/dm-browser-host"
        });
        let v: serde_json::Value = serde_json::from_str(&manifest_json(&host, &origins())).unwrap();
        assert_eq!(v["name"], HOST_NAME);
        assert_eq!(v["type"], "stdio");
        assert_eq!(v["path"], host.display().to_string());
        assert_eq!(v["allowed_origins"], serde_json::json!([ORIGIN]));
        assert!(
            v.get("allowed_extensions").is_none(),
            "Chrome uses allowed_origins only"
        );
    }

    #[test]
    fn locations_follow_the_official_layout() {
        let root = Path::new("/r");
        let t = |b| Target {
            browser: b,
            root: Some(root.to_owned()),
            registry_root: Some(r"HKCU\Software\T".into()),
        };
        // Each distinct (manifest, registry key) pair belongs to one browser family.
        let mut pairs: Vec<_> = Browser::ALL
            .iter()
            .filter_map(|b| {
                manifest_path(&t(*b))
                    .ok()
                    .map(|p| (p, registry_key(&t(*b))))
            })
            .collect();
        assert!(pairs.iter().all(|(p, _)| p.starts_with(root)));
        if cfg!(windows) {
            // Google's channels share one key, so they share one manifest.
            assert_eq!(
                manifest_path(&t(Browser::Chrome)).unwrap(),
                manifest_path(&t(Browser::ChromeForTesting)).unwrap()
            );
        } else {
            let all = pairs.len();
            pairs.sort();
            pairs.dedup_by(|a, b| a.0 == b.0);
            assert_eq!(pairs.len(), all, "one manifest per browser on Unix");
        }
        if cfg!(target_os = "linux") {
            assert!(
                manifest_path(&t(Browser::ChromeForTesting))
                    .unwrap()
                    .ends_with(".config/google-chrome-for-testing/NativeMessagingHosts/com.downova.host.json")
            );
        }
        assert_eq!(
            registry_key(&t(Browser::Edge)),
            r"HKCU\Software\T\Microsoft\Edge\NativeMessagingHosts\com.downova.host"
        );
    }

    #[test]
    fn temporary_and_relative_locations_are_refused() {
        assert!(check_host_path(Path::new("relative/host")).is_err());
        for p in [
            "/tmp/.mount_DownovXYZ/usr/bin/dm-browser-host",
            "/Volumes/Downova/Downova.app/Contents/MacOS/dm-browser-host",
            "/private/var/folders/x/T/AppTranslocation/ABC/d/Downova.app/Contents/MacOS/dm-browser-host",
        ] {
            assert!(unstable_location(p, None).is_some(), "{p}");
        }
        assert!(
            unstable_location(
                "/opt/app/usr/bin/dm-browser-host",
                Some(Path::new("/opt/app"))
            )
            .is_some(),
            "inside $APPDIR"
        );
        for p in [
            "/Applications/Downova.app/Contents/MacOS/dm-browser-host",
            "/usr/bin/dm-browser-host",
        ] {
            assert!(unstable_location(p, None).is_none(), "{p}");
        }
    }

    #[test]
    fn setup_is_repeatable_and_repair_restores_missing_or_damaged_entries() {
        let s = Sandbox::new();
        let t = s.target(Browser::Chrome);
        let host = s.host("install-a");
        let first = register(&t, &host, &origins(), false).unwrap();
        assert_eq!(first.previous, Owner::Absent);
        let written = std::fs::read(&first.manifest).unwrap();
        let again = register(&t, &host, &origins(), false).unwrap();
        assert_eq!(again.previous, Owner::Ours);
        assert_eq!(
            std::fs::read(&again.manifest).unwrap(),
            written,
            "same bytes"
        );

        // Missing manifest.
        std::fs::remove_file(&first.manifest).unwrap();
        assert_eq!(status(&t, &host).unwrap().manifest_owner, Owner::Absent);
        register(&t, &host, &origins(), false).unwrap();
        assert_eq!(std::fs::read(&first.manifest).unwrap(), written);

        // Damaged but still ours: wrong origins and type.
        let mut doc = manifest(&first.manifest);
        doc["allowed_origins"] = serde_json::json!(["chrome-extension://*/"]);
        doc["type"] = serde_json::json!("pipe");
        std::fs::write(&first.manifest, doc.to_string()).unwrap();
        assert_eq!(
            register(&t, &host, &origins(), false).unwrap().previous,
            Owner::Ours
        );
        assert_eq!(std::fs::read(&first.manifest).unwrap(), written);

        if cfg!(windows) {
            let key = registry_key(&t);
            reg(&["delete", &key, "/f"]).unwrap();
            register(&t, &host, &origins(), false).unwrap();
            assert_eq!(
                registry_value(&key).as_deref(),
                Some(first.manifest.display().to_string().as_str())
            );
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&first.manifest)
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o644);
        }
    }

    #[test]
    fn conflicts_are_refused_without_replace_and_foreign_hosts_always() {
        let s = Sandbox::new();
        let t = s.target(Browser::Edge);
        let a = s.host("install-a");
        let b = s.host("install-b");
        let path = register(&t, &a, &origins(), false).unwrap().manifest;
        let a_bytes = std::fs::read(&path).unwrap();

        // Another live installation.
        let err = register(&t, &b, &origins(), false).unwrap_err();
        assert!(err.contains("another Downova installation"), "{err}");
        assert_eq!(std::fs::read(&path).unwrap(), a_bytes, "untouched");
        let r = register(&t, &b, &origins(), true).unwrap();
        assert_eq!(
            r.previous,
            Owner::OtherInstallation(a.display().to_string())
        );
        assert_eq!(manifest(&path)["path"], b.display().to_string());

        // A stale Downova entry (its executable was removed) is taken over without --replace.
        std::fs::remove_file(&b).unwrap();
        assert!(matches!(
            register(&t, &a, &origins(), false).unwrap().previous,
            Owner::Stale(_)
        ));

        // Unreadable: ownership unknown.
        std::fs::write(&path, b"{ truncated").unwrap();
        assert!(register(&t, &a, &origins(), false).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"{ truncated");
        assert_eq!(
            register(&t, &a, &origins(), true).unwrap().previous,
            Owner::Unreadable
        );

        // A different host under our file name is never replaced.
        std::fs::write(&path, r#"{"name":"com.other.host","path":"/x"}"#).unwrap();
        for replace in [false, true] {
            assert!(register(&t, &a, &origins(), replace).is_err());
        }
        assert_eq!(manifest(&path)["name"], "com.other.host");
    }

    #[test]
    fn unregister_removes_only_this_installations_entries() {
        let s = Sandbox::new();
        let a = s.host("install-a");
        let b = s.host("install-b");
        let edge = s.target(Browser::Edge);
        let chrome = s.target(Browser::Chrome);
        register(&edge, &a, &origins(), false).unwrap();
        let chrome_path = register(&chrome, &b, &origins(), false).unwrap().manifest;

        // A's uninstall: its Edge entries go, B's Chrome entries stay.
        let out = unregister(&chrome, &a).unwrap();
        assert!(out.removed.is_empty(), "{out:?}");
        assert!(!out.kept.is_empty());
        assert!(chrome_path.is_file());
        let out = unregister(&edge, &a).unwrap();
        assert_eq!(
            out.removed.len(),
            if cfg!(windows) { 2 } else { 1 },
            "{out:?}"
        );
        assert_eq!(status(&edge, &a).unwrap().manifest_owner, Owner::Absent);
        if cfg!(windows) {
            assert_eq!(registry_value(&registry_key(&edge)), None);
            assert!(registry_value(&registry_key(&chrome)).is_some());
        }
        // Nothing left: a second run is a no-op.
        assert_eq!(unregister(&edge, &a).unwrap(), Removal::default());

        // A foreign file under our name survives and is reported, not an error.
        let path = manifest_path(&edge).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, r#"{"name":"com.other.host"}"#).unwrap();
        let out = unregister(&edge, &a).unwrap();
        assert!(out.removed.is_empty() && out.kept.len() == 1, "{out:?}");
        assert!(path.exists());
    }

    #[test]
    fn upgrade_and_uninstall_ordering_keep_the_newest_installation() {
        let s = Sandbox::new();
        let t = s.target(Browser::Chrome);
        let old = s.host("old");
        let new = s.host("new");
        register(&t, &old, &origins(), false).unwrap();
        // In-place upgrade: same path, our entry, rewritten.
        assert_eq!(
            register(&t, &old, &origins(), false).unwrap().previous,
            Owner::Ours
        );
        // Side-by-side install takes over explicitly; the old copy's later uninstall keeps it.
        register(&t, &new, &origins(), true).unwrap();
        let out = unregister(&t, &old).unwrap();
        assert!(out.removed.is_empty(), "{out:?}");
        assert_eq!(status(&t, &new).unwrap().manifest_owner, Owner::Ours);
        if cfg!(windows) {
            assert_eq!(status(&t, &new).unwrap().registry_owner, Some(Owner::Ours));
        }
        // The old copy uninstalled first, then the new one: both leave nothing behind.
        unregister(&t, &new).unwrap();
        assert_eq!(status(&t, &new).unwrap().manifest_owner, Owner::Absent);
    }

    #[test]
    fn a_registry_value_pointing_elsewhere_is_inspected_before_it_is_changed() {
        if !cfg!(windows) {
            return;
        }
        let s = Sandbox::new();
        let t = s.target(Browser::Chromium);
        let host = s.host("install-a");
        let other = s.dir.path().join("other.json");
        std::fs::write(&other, r#"{"name":"com.other.host","path":"C:\\x.exe"}"#).unwrap();
        let key = registry_key(&t);
        reg(&[
            "add",
            &key,
            "/ve",
            "/t",
            "REG_SZ",
            "/d",
            &other.display().to_string(),
            "/f",
        ])
        .unwrap();
        assert!(register(&t, &host, &origins(), true).is_err());
        assert!(!manifest_path(&t).unwrap().exists(), "nothing was written");
        let out = unregister(&t, &host).unwrap();
        assert!(out.removed.is_empty() && out.kept.len() == 1, "{out:?}");
        assert_eq!(registry_value(&key), Some(other.display().to_string()));
    }
}
