//! F08: sleep or shut down once downloads finish. The window decides when (opt-in each time, a
//! countdown with Cancel, never while a download is still running, verifying or being scanned);
//! this module only knows each system's command and whether it is there.
//!
//! - Windows: `shutdown.exe /s /t 0`; sleep through `rundll32 powrprof.dll,SetSuspendState`
//!   (where hibernation is on, Windows may hibernate instead).
//! - macOS: `pmset sleepnow`; shut down through System Events (the system may ask for permission
//!   the first time, and refuses if it is not given).
//! - Linux: `systemctl suspend` / `systemctl poweroff` (logind policy decides).
//!
//! Debug builds only: with `DOWNOVA_POWER_DRY_RUN` set to a file path, the command is written to
//! that file instead of run, so the flow can be tested without powering the machine down.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// What the user chose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PowerAction {
    Sleep,
    Shutdown,
}

/// Which actions this system offers (the window shows the others as unavailable).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct PowerSupport {
    pub sleep: bool,
    pub shutdown: bool,
}

/// The program and arguments for an action on this system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub program: PathBuf,
    pub args: Vec<&'static str>,
}

/// The first existing program among `candidates`.
fn first_existing(candidates: &[PathBuf]) -> Option<PathBuf> {
    candidates.iter().find(|p| p.is_file()).cloned()
}

#[cfg(windows)]
fn system32(name: &str) -> Vec<PathBuf> {
    let root =
        std::env::var_os("SystemRoot").map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
    vec![root.join("System32").join(name)]
}

/// The command for `action`, or `None` when this system has no such program.
pub fn plan(action: PowerAction) -> Option<Plan> {
    #[cfg(windows)]
    {
        let (name, args): (&str, Vec<&'static str>) = match action {
            PowerAction::Shutdown => ("shutdown.exe", vec!["/s", "/t", "0"]),
            PowerAction::Sleep => (
                "rundll32.exe",
                vec!["powrprof.dll,SetSuspendState", "0,1,0"],
            ),
        };
        first_existing(&system32(name)).map(|program| Plan { program, args })
    }
    #[cfg(target_os = "macos")]
    {
        match action {
            PowerAction::Sleep => {
                first_existing(&[PathBuf::from("/usr/bin/pmset")]).map(|program| Plan {
                    program,
                    args: vec!["sleepnow"],
                })
            }
            PowerAction::Shutdown => {
                first_existing(&[PathBuf::from("/usr/bin/osascript")]).map(|program| Plan {
                    program,
                    args: vec!["-e", "tell application \"System Events\" to shut down"],
                })
            }
        }
    }
    #[cfg(target_os = "linux")]
    {
        let verb = match action {
            PowerAction::Sleep => "suspend",
            PowerAction::Shutdown => "poweroff",
        };
        first_existing(&[
            PathBuf::from("/usr/bin/systemctl"),
            PathBuf::from("/bin/systemctl"),
        ])
        .map(|program| Plan {
            program,
            args: vec![verb],
        })
    }
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        let _ = action;
        None
    }
}

pub fn support() -> PowerSupport {
    PowerSupport {
        sleep: plan(PowerAction::Sleep).is_some(),
        shutdown: plan(PowerAction::Shutdown).is_some(),
    }
}

/// Runs the action's command (detached). `false` when it is unavailable or could not start.
pub fn run(action: PowerAction) -> bool {
    let Some(plan) = plan(action) else {
        return false;
    };
    #[cfg(debug_assertions)]
    if let Some(log) = std::env::var_os("DOWNOVA_POWER_DRY_RUN") {
        return dry_run(std::path::Path::new(&log), action, &plan);
    }
    std::process::Command::new(&plan.program)
        .args(&plan.args)
        .spawn()
        .is_ok()
}

#[cfg(debug_assertions)]
fn dry_run(log: &std::path::Path, action: PowerAction, plan: &Plan) -> bool {
    use std::io::Write;
    let line = format!(
        "{action:?} {} {}\n",
        plan.program.display(),
        plan.args.join(" ")
    );
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)
        .and_then(|mut f| f.write_all(line.as_bytes()))
        .is_ok()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn every_supported_action_has_an_existing_absolute_program() {
        for action in [PowerAction::Sleep, PowerAction::Shutdown] {
            if let Some(p) = plan(action) {
                assert!(p.program.is_absolute(), "{action:?}: {p:?}");
                assert!(p.program.is_file(), "{action:?}: {p:?}");
                assert!(!p.args.is_empty());
            }
        }
        let s = support();
        assert_eq!(s.sleep, plan(PowerAction::Sleep).is_some());
        assert_eq!(s.shutdown, plan(PowerAction::Shutdown).is_some());
    }

    #[cfg(windows)]
    #[test]
    fn windows_uses_the_system_shutdown_and_sleep_commands() {
        let shut = plan(PowerAction::Shutdown).unwrap();
        assert!(
            shut.program.ends_with("System32/shutdown.exe")
                || shut.program.ends_with(r"System32\shutdown.exe")
        );
        assert_eq!(shut.args, ["/s", "/t", "0"]);
        assert_eq!(
            plan(PowerAction::Sleep).unwrap().args[0],
            "powrprof.dll,SetSuspendState"
        );
    }

    #[test]
    fn the_dry_run_writes_the_command_instead_of_running_it() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("power.log");
        let Some(p) = plan(PowerAction::Shutdown) else {
            return;
        };
        assert!(dry_run(&log, PowerAction::Shutdown, &p));
        let written = std::fs::read_to_string(&log).unwrap();
        assert!(written.starts_with("Shutdown "), "{written}");
    }
}
