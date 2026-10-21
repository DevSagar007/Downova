//! Argument parsing for `dm`. No dependencies; errors never echo URL arguments.
//!
//! Grammar: `dm [GLOBAL…] <command> [ARGS…]`. Global options (`--json`, `--data-dir DIR`) may
//! appear anywhere — before or after the command. `--opt=value` and `--opt value` are both
//! accepted. `--` ends option parsing (for URLs that start with `-`).

use std::ffi::OsString;
use std::fmt;
use std::path::PathBuf;

use dm_domain::JobId;

/// Default destination profile.
pub const DEFAULT_PROFILE: &str = "downloads";

#[derive(Clone, PartialEq, Eq)]
pub enum Command {
    Add {
        /// Raw URL (may contain signed-query secrets). Never printed; see the `Debug` impl.
        url: String,
        profile: String,
        idempotency_key: Option<String>,
    },
    List,
    Get(JobId),
    Pause(JobId),
    Resume(JobId),
    Cancel(JobId),
}

impl Command {
    pub fn name(&self) -> &'static str {
        match self {
            Command::Add { .. } => "add",
            Command::List => "list",
            Command::Get(_) => "get",
            Command::Pause(_) => "pause",
            Command::Resume(_) => "resume",
            Command::Cancel(_) => "cancel",
        }
    }
}

impl fmt::Debug for Command {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Command::Add {
                url,
                profile,
                idempotency_key,
            } => f
                .debug_struct("Add")
                .field("url", &dm_domain::redact::redact_url(url))
                .field("profile", profile)
                .field("idempotency_key", idempotency_key)
                .finish(),
            other => write!(f, "{}", other.name()),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Global {
    pub json: bool,
    pub data_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Parsed {
    Run(Global, Command),
    Help(Global),
    Version(Global),
}

/// Invalid command line. `json` tells the caller which output mode to use for the error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageError {
    pub message: String,
    pub json: bool,
    pub command: Option<&'static str>,
}

pub const HELP: &str = "\
dm — Downova command-line client

USAGE:
    dm [--json] [--data-dir DIR] <COMMAND> [ARGS]

COMMANDS:
    add <URL> [--profile ID] [--idempotency-key KEY]
                         Ask the service to download URL (profile defaults to \"downloads\").
                         Repeating the same request with the same key returns the same job.
    list                 List downloads known to the running service.
    get <JOB-ID>         Show one download.
    pause <JOB-ID>       Pause a download (manual pause; never undone by policy).
    resume <JOB-ID>      Resume a paused download (refused when the backend cannot validate
                         partial data).
    cancel <JOB-ID>      Cancel a download.

GLOBAL OPTIONS (accepted before or after the command):
    --json               Print one JSON object on stdout (success or error).
    --data-dir DIR       Use the service data directory DIR (default: per-user, or $DOWNOVA_HOME).
    -h, --help           Show this help.
    -V, --version        Show the version.

Use `--` before a URL that starts with `-`.

EXIT CODES:
    0  success
    1  unexpected failure (malformed response, internal service error)
    2  invalid arguments
    3  service unavailable (not running, stale endpoint, timeout, incompatible API version)
    4  request rejected by the service (unknown job, invalid URL or profile, idempotency
       conflict, capability not supported, …)

The CLI never downloads by itself and leaves the service running when it exits.
";

/// Parses `args` (without the program name).
pub fn parse(args: &[OsString]) -> Result<Parsed, UsageError> {
    let mut global = Global::default();
    let mut help = false;
    let mut version = false;
    let mut profile: Option<String> = None;
    let mut key: Option<String> = None;
    let mut positionals: Vec<String> = Vec::new();
    let mut options_done = false;

    // `--json` must be known before any error is reported, so scan for it first.
    global.json = args.iter().any(|a| a == "--json");
    let usage = |message: String, json: bool| UsageError {
        message,
        json,
        command: None,
    };

    let mut iter = args.iter();
    while let Some(raw) = iter.next() {
        let arg = raw
            .to_str()
            .ok_or_else(|| usage("arguments must be valid UTF-8".into(), global.json))?;
        if options_done || !arg.starts_with('-') || arg == "-" {
            positionals.push(arg.to_owned());
            continue;
        }
        if arg == "--" {
            options_done = true;
            continue;
        }
        let (name, inline) = match arg.split_once('=') {
            Some((n, v)) if n.starts_with("--") => (n, Some(v.to_owned())),
            _ => (arg, None),
        };
        let mut value = |what: &str| -> Result<String, UsageError> {
            let v = match &inline {
                Some(v) => v.clone(),
                None => iter
                    .next()
                    .and_then(|v| v.to_str())
                    .map(str::to_owned)
                    .ok_or_else(|| usage(format!("{what} needs a value"), global.json))?,
            };
            if v.is_empty() {
                return Err(usage(
                    format!("{what} needs a non-empty value"),
                    global.json,
                ));
            }
            Ok(v)
        };
        let set_once = |slot: &mut Option<String>, what: &str, v: String| {
            if slot.replace(v).is_some() {
                return Err(usage(format!("{what} given more than once"), global.json));
            }
            Ok(())
        };
        match name {
            "--json" if inline.is_none() => {}
            "-h" | "--help" if inline.is_none() => help = true,
            "-V" | "--version" if inline.is_none() => version = true,
            "--data-dir" => {
                let v = value("--data-dir")?;
                if global.data_dir.replace(PathBuf::from(v)).is_some() {
                    return Err(usage("--data-dir given more than once".into(), global.json));
                }
            }
            "-p" | "--profile" => set_once(&mut profile, "--profile", value("--profile")?)?,
            "--idempotency-key" => {
                set_once(&mut key, "--idempotency-key", value("--idempotency-key")?)?;
            }
            _ => {
                // Only the option name is echoed, never a value.
                return Err(usage(format!("unknown option {name}"), global.json));
            }
        }
    }

    if help {
        return Ok(Parsed::Help(global));
    }
    if version {
        return Ok(Parsed::Version(global));
    }

    let mut positionals = positionals.into_iter();
    let Some(command) = positionals.next() else {
        return Err(usage(
            "missing command (try `dm --help`)".into(),
            global.json,
        ));
    };
    let rest: Vec<String> = positionals.collect();
    let json = global.json;
    let err = |command: &'static str, message: &str| UsageError {
        message: format!("{command}: {message}"),
        json,
        command: Some(command),
    };
    let one_job = |command: &'static str| -> Result<JobId, UsageError> {
        match rest.as_slice() {
            [id] => JobId::parse(id).map_err(|_| err(command, "JOB-ID must look like job_…")),
            [] => Err(err(command, "missing JOB-ID")),
            _ => Err(err(command, "expected exactly one JOB-ID")),
        }
    };
    let add_only = |command: &'static str| -> Result<(), UsageError> {
        if profile.is_some() || key.is_some() {
            return Err(err(
                command,
                "--profile and --idempotency-key are only valid for `add`",
            ));
        }
        Ok(())
    };

    let command = match command.as_str() {
        "add" => {
            let url = match rest.as_slice() {
                [url] => url.clone(),
                [] => return Err(err("add", "missing URL")),
                _ => {
                    return Err(err(
                        "add",
                        "expected exactly one URL (quote URLs containing spaces or &)",
                    ));
                }
            };
            if let Some(k) = &key {
                let ok = (8..=128).contains(&k.len()) && !k.chars().any(char::is_control);
                if !ok {
                    return Err(err(
                        "add",
                        "--idempotency-key must be 8-128 printable characters",
                    ));
                }
            }
            Command::Add {
                url,
                profile: profile.take().unwrap_or_else(|| DEFAULT_PROFILE.to_owned()),
                idempotency_key: key.take(),
            }
        }
        "list" => {
            add_only("list")?;
            if !rest.is_empty() {
                return Err(err("list", "takes no arguments"));
            }
            Command::List
        }
        "get" => {
            add_only("get")?;
            Command::Get(one_job("get")?)
        }
        "pause" => {
            add_only("pause")?;
            Command::Pause(one_job("pause")?)
        }
        "resume" => {
            add_only("resume")?;
            Command::Resume(one_job("resume")?)
        }
        "cancel" => {
            add_only("cancel")?;
            Command::Cancel(one_job("cancel")?)
        }
        _ => {
            return Err(usage(
                "unknown command (try `dm --help`)".into(),
                global.json,
            ));
        }
    };
    Ok(Parsed::Run(global, command))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(args: &[&str]) -> Result<Parsed, UsageError> {
        parse(&args.iter().map(OsString::from).collect::<Vec<_>>())
    }

    fn cmd(args: &[&str]) -> Command {
        match p(args).unwrap() {
            Parsed::Run(_, c) => c,
            other => panic!("expected command, got {other:?}"),
        }
    }

    #[test]
    fn add_defaults_and_options_in_any_position() {
        assert_eq!(
            cmd(&["add", "https://h/f.zip"]),
            Command::Add {
                url: "https://h/f.zip".into(),
                profile: "downloads".into(),
                idempotency_key: None
            }
        );
        let Parsed::Run(global, command) = p(&[
            "--json",
            "add",
            "--profile=work",
            "https://h/f",
            "--idempotency-key",
            "key-12345",
            "--data-dir",
            "D",
        ])
        .unwrap() else {
            panic!()
        };
        assert!(global.json);
        assert_eq!(global.data_dir, Some(PathBuf::from("D")));
        assert_eq!(
            command,
            Command::Add {
                url: "https://h/f".into(),
                profile: "work".into(),
                idempotency_key: Some("key-12345".into())
            }
        );
        assert!(
            matches!(cmd(&["add", "--", "-weird"]), Command::Add { url, .. } if url == "-weird")
        );
    }

    #[test]
    fn job_commands_validate_ids() {
        assert_eq!(
            cmd(&["get", "job_abc"]),
            Command::Get(JobId::parse("job_abc").unwrap())
        );
        assert!(matches!(
            cmd(&["cancel", "job_x", "--json"]),
            Command::Cancel(_)
        ));
        for bad in [
            &["get"][..],
            &["get", "abc"],
            &["pause", "job_a", "job_b"],
            &["list", "extra"],
        ] {
            assert!(p(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn invalid_arguments_are_usage_errors_without_values() {
        let secret = "https://h/x?token=SYNTHETIC-SECRET";
        for args in [
            vec!["add"],
            vec!["add", secret, secret],
            vec!["frobnicate"],
            vec!["--bogus", secret],
            vec!["add", secret, "--profile"],
            vec!["add", secret, "--profile", ""],
            vec!["add", secret, "--idempotency-key", "short"],
            vec!["list", "--profile", "x"],
            vec!["add", secret, "--profile", "a", "--profile", "b"],
            vec![],
        ] {
            let e = p(&args).unwrap_err();
            assert!(!e.message.contains("SYNTHETIC-SECRET"), "{}", e.message);
        }
        assert!(p(&["--json", "frobnicate"]).unwrap_err().json);
    }

    #[test]
    fn help_and_version_win() {
        assert!(matches!(p(&["--help"]).unwrap(), Parsed::Help(_)));
        assert!(matches!(p(&["add", "-h"]).unwrap(), Parsed::Help(_)));
        assert!(matches!(p(&["-V"]).unwrap(), Parsed::Version(_)));
    }

    #[test]
    fn debug_output_redacts_the_url() {
        let c = cmd(&["add", "https://u:p@h/x?token=SYNTHETIC-SECRET"]);
        let dbg = format!("{c:?}");
        assert!(
            !dbg.contains("SYNTHETIC-SECRET") && !dbg.contains("u:p@"),
            "{dbg}"
        );
    }
}
