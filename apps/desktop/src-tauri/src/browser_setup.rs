//! First-launch browser registration (B2.5, BRW-03, ADR 0014).
//!
//! On every OS the installed app registers its native-messaging host for the *current* user the
//! first time it runs from a given location: the sibling `dm-browser-host register --browser
//! chrome --browser edge --browser firefox` (Firefox since B3.4). This is the per-user step that root package scripts (Linux), per-machine
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
/// Registered at first launch: Chrome, Edge and (B3.4, ADR 0029) Firefox. A browser that is not
/// installed only gets a small per-user manifest it never reads.
const BROWSERS: [&str; 3] = ["chrome", "edge", "firefox"];

/// Browsers the connections view may name (the host's own list).
pub const KNOWN: [&str; 8] = [
    "chrome",
    "chrome-beta",
    "chrome-dev",
    "chrome-canary",
    "chrome-for-testing",
    "chromium",
    "edge",
    "firefox",
];

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

/// One verdict for one browser package (BRW-09), as the host reported it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Verdict {
    pub package: String,
    pub support: String,
    pub reason: String,
}

/// One browser's connection, for the webview: categories and codes only, never a path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BrowserConnection {
    pub browser: String,
    /// `ours`, `absent`, `stale`, `other_installation`, `unreadable`, `foreign` or `error`.
    pub registration: String,
    pub packages: Vec<String>,
    pub support: Vec<Verdict>,
}

/// What a repair did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum RepairOutcome {
    Repaired {
        connection: BrowserConnection,
    },
    /// Not ours to change (another installation, a different host, an unreadable file): left as
    /// it is. `reason` is the registration state that refused it.
    Refused {
        reason: String,
        connection: BrowserConnection,
    },
    Failed {
        reason: String,
    },
}

fn owner(v: &serde_json::Value) -> Option<String> {
    v.get("owner").and_then(|o| o.as_str()).map(str::to_owned)
}

/// One `status --json` line as a connection. The manifest's and (Windows) the registry entry's
/// owners combine: the browser reaches this installation only when both are ours.
pub fn connection_from(line: &serde_json::Value) -> Option<BrowserConnection> {
    let browser = line.get("browser")?.as_str()?.to_owned();
    if !KNOWN.contains(&browser.as_str()) {
        return None;
    }
    let registration = if line["ok"] != true {
        "error".to_owned()
    } else {
        let manifest = owner(&line["result"]).unwrap_or_else(|| "error".into());
        let registry = line["result"].get("registry_owner").and_then(owner);
        match registry {
            Some(r) if manifest == "ours" && r != "ours" => r,
            _ => manifest,
        }
    };
    let strings = |v: &serde_json::Value| -> Vec<String> {
        v.as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    };
    let support = line["support"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| {
                    Some(Verdict {
                        package: v.get("package")?.as_str()?.to_owned(),
                        support: v.get("support")?.as_str()?.to_owned(),
                        reason: v.get("reason")?.as_str()?.to_owned(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Some(BrowserConnection {
        browser,
        registration,
        packages: strings(&line["packages"]),
        support,
    })
}

fn host_json(host: &Path, args: &[&str]) -> Result<(Option<i32>, Vec<serde_json::Value>), String> {
    let mut cmd = Command::new(host);
    cmd.args(args).arg("--json");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let out = cmd
        .output()
        .map_err(|e| format!("cannot run the browser helper: {}", e.kind()))?;
    Ok((
        out.status.code(),
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect(),
    ))
}

/// Every browser available on this OS: registration, packages and verdicts (BRW-07/09).
pub fn connections(host: &Path) -> Result<Vec<BrowserConnection>, String> {
    let (_, lines) = host_json(host, &["status", "--browser", "all"])?;
    Ok(lines.iter().filter_map(connection_from).collect())
}

/// Repairs one browser's registration for this installation. Never takes over another
/// installation's or a different program's entry (no `--replace`): those are reported.
pub fn repair(host: &Path, browser: &str) -> RepairOutcome {
    if !KNOWN.contains(&browser) {
        return RepairOutcome::Failed {
            reason: "unknown_browser".into(),
        };
    }
    let repaired = host_json(host, &["repair", "--browser", browser]);
    let status = host_json(host, &["status", "--browser", browser])
        .ok()
        .and_then(|(_, l)| l.iter().find_map(connection_from));
    match (repaired, status) {
        (Err(_), _) | (_, None) => RepairOutcome::Failed {
            reason: "helper_unavailable".into(),
        },
        (Ok((Some(0), _)), Some(connection)) if connection.registration == "ours" => {
            RepairOutcome::Repaired { connection }
        }
        (Ok(_), Some(connection)) => RepairOutcome::Refused {
            reason: connection.registration.clone(),
            connection,
        },
    }
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
    let version = dm_domain::BUILD_VERSION;
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
    fn status_lines_become_path_free_connections() {
        let line = serde_json::json!({
            "browser": "firefox", "ok": true, "action": "status",
            "result": {"browser": "firefox", "manifest": "C:/Users/u/x.json", "owner": "ours",
                       "registry_key": "HKCU", "registry_value": "C:/Users/u/x.json",
                       "registry_owner": {"owner": "ours"}},
            "packages": ["native"],
            "support": [{"package": "native", "support": "supported", "reason": "tested_this_os"}]
        });
        let c = connection_from(&line).unwrap();
        assert_eq!(c.registration, "ours");
        assert_eq!(c.packages, vec!["native".to_owned()]);
        assert_eq!(c.support[0].reason, "tested_this_os");
        let shown = serde_json::to_string(&c).unwrap();
        assert!(
            !shown.contains("Users"),
            "no path reaches the webview: {shown}"
        );
        // A manifest that is ours but a registry value that is not: the browser does not reach us.
        let mut split = line.clone();
        split["result"]["registry_owner"] =
            serde_json::json!({"owner": "other_installation", "host": "C:/x"});
        assert_eq!(
            connection_from(&split).unwrap().registration,
            "other_installation"
        );
        let mut failed = line.clone();
        failed["ok"] = serde_json::json!(false);
        assert_eq!(connection_from(&failed).unwrap().registration, "error");
        let mut unknown = line;
        unknown["browser"] = serde_json::json!("netscape");
        assert!(connection_from(&unknown).is_none());
        assert!(matches!(
            repair(Path::new("/no/host"), "netscape"),
            RepairOutcome::Failed { .. }
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
