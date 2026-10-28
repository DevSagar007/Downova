//! First-launch browser registration (B2.5, BRW-03, ADR 0014).
//!
//! On every OS the installed app registers its native-messaging host for the *current* user the
//! first time it runs from a given location: the sibling `dm-browser-host register --browser
//! chrome --browser edge`. This is the per-user step that root package scripts (Linux), per-machine
//! MSI installs and drag-installed apps (macOS) cannot do. The host refuses temporary locations
//! (AppImage, disk-image and translocated mounts) and anything that belongs to someone else; the
//! shell only runs it and records the outcome.
//!
//! The outcome is written to `<data>/browser-setup.json`. A later launch from the same host path
//! and version does nothing, so a user's own `dm-browser-host unregister` is respected; a new
//! version or location registers again. `DOWNOVA_SKIP_BROWSER_SETUP=1` turns the step off
//! (managed deployments that register system-wide, and test runs that isolate it).

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

const RECORD_FILE: &str = "browser-setup.json";
const BROWSERS: [&str; 2] = ["chrome", "edge"];

/// What the last setup attempt did (no URLs or secrets: paths and the host's own messages).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetupRecord {
    pub version: String,
    pub host: String,
    /// True when every browser registered.
    pub complete: bool,
    pub exit_code: Option<i32>,
    /// One JSON line per browser, as `dm-browser-host --json` printed it.
    pub results: Vec<serde_json::Value>,
}

/// The installed host next to the running executable.
pub fn sibling_host() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let path = exe
        .parent()?
        .join(format!("dm-browser-host{}", std::env::consts::EXE_SUFFIX));
    path.is_file().then_some(path)
}

pub fn load(data_dir: &Path) -> Option<SetupRecord> {
    let bytes = std::fs::read(data_dir.join(RECORD_FILE)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// True when this host and version already registered completely.
pub fn done(record: Option<&SetupRecord>, host: &Path, version: &str) -> bool {
    record
        .is_some_and(|r| r.complete && r.version == version && r.host == host.display().to_string())
}

/// Runs the host's `register` for the default browsers and parses its JSON lines.
pub fn register(host: &Path, version: &str) -> SetupRecord {
    let mut cmd = Command::new(host);
    cmd.arg("register");
    for b in BROWSERS {
        cmd.args(["--browser", b]);
    }
    cmd.arg("--json");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // A console host started from a GUI app would otherwise flash a console window.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let (exit_code, results) = match cmd.output() {
        Ok(out) => (
            out.status.code(),
            String::from_utf8_lossy(&out.stdout)
                .lines()
                .filter_map(|l| serde_json::from_str(l).ok())
                .collect(),
        ),
        Err(e) => (
            None,
            vec![
                serde_json::json!({ "ok": false, "error": format!("cannot run the host: {}", e.kind()) }),
            ],
        ),
    };
    let complete = exit_code == Some(0)
        && results.len() == BROWSERS.len()
        && results.iter().all(|r: &serde_json::Value| r["ok"] == true);
    SetupRecord {
        version: version.to_owned(),
        host: host.display().to_string(),
        complete,
        exit_code,
        results,
    }
}

fn save(data_dir: &Path, record: &SetupRecord) -> std::io::Result<()> {
    dm_platform::paths::ensure_private_dir(data_dir)?;
    let bytes = serde_json::to_vec_pretty(record).map_err(std::io::Error::other)?;
    dm_platform::paths::write_private_file(&data_dir.join(RECORD_FILE), &bytes)
}

/// Registers in the background on launch when needed; never blocks or fails the window.
pub fn on_launch() {
    if std::env::var_os("DOWNOVA_SKIP_BROWSER_SETUP").is_some_and(|v| v == "1") {
        return;
    }
    let (Ok(data_dir), Some(host)) = (dm_platform::paths::data_dir(), sibling_host()) else {
        // Development runs have no packaged host beside them.
        return;
    };
    let version = env!("CARGO_PKG_VERSION");
    if done(load(&data_dir).as_ref(), &host, version) {
        return;
    }
    std::thread::spawn(move || {
        let record = register(&host, version);
        let _ = save(&data_dir, &record);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(complete: bool, host: &str, version: &str) -> SetupRecord {
        SetupRecord {
            version: version.into(),
            host: host.into(),
            complete,
            exit_code: Some(if complete { 0 } else { 3 }),
            results: vec![],
        }
    }

    #[test]
    fn setup_runs_again_only_for_a_new_location_version_or_an_incomplete_attempt() {
        let host = Path::new("/opt/Downova/dm-browser-host");
        let shown = host.display().to_string();
        assert!(!done(None, host, "1.0.0"));
        assert!(done(Some(&record(true, &shown, "1.0.0")), host, "1.0.0"));
        assert!(!done(Some(&record(false, &shown, "1.0.0")), host, "1.0.0"));
        assert!(!done(Some(&record(true, &shown, "0.9.0")), host, "1.0.0"));
        assert!(!done(
            Some(&record(true, "/elsewhere/dm-browser-host", "1.0.0")),
            host,
            "1.0.0"
        ));
    }

    #[test]
    fn the_record_round_trips_and_a_missing_host_is_reported_not_raised() {
        let dir = tempfile::tempdir().unwrap();
        let r = register(&dir.path().join("no-such-host"), "1.0.0");
        assert!(!r.complete);
        assert_eq!(r.exit_code, None);
        assert_eq!(r.results[0]["ok"], false);
        save(dir.path(), &r).unwrap();
        assert_eq!(load(dir.path()), Some(r));
    }
}
