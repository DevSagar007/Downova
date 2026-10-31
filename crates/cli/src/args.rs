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
        /// `on_duplicate: refuse` (B2.8); otherwise the field is not sent (old behaviour).
        refuse_duplicate: bool,
        /// Destination category (B2.8); the folder comes from the service's rules.
        category: Option<String>,
        /// Queue for the job (B2.11); omitted: `main`.
        queue: Option<String>,
    },
    /// The running service's `hello`: versions, backend and its capability report.
    Hello,
    List,
    Get(JobId),
    Pause(JobId),
    Resume(JobId),
    Cancel(JobId),
    /// Removes a finished download from the list (ADR 0013); never deletes files.
    Remove(JobId),
    /// Stops the service (ADR 0012); with `pause_downloads`, running downloads stay paused.
    Shutdown {
        pause_downloads: bool,
    },
    /// `queues.v1.list` (B2.11).
    Queues,
    QueueCreate {
        name: String,
        max_active: Option<u32>,
    },
    QueueStart(String),
    QueueStop(String),
    /// Never deletes downloads or files; a queue with downloads needs `move_to`.
    QueueDelete {
        queue: String,
        move_to: Option<String>,
    },
    SetQueue(JobId, String),
    SetPriority(JobId, String),
    Move(JobId, String),
    /// `history.v1.get` (B2.12).
    History,
    /// `history.v1.set`: days, or `None` to keep forever.
    HistorySet(Option<u32>),
    /// `history.v1.preview`: changes nothing.
    HistoryPreview(Option<u32>),
    /// `history.v1.clean`: list entries only, never files.
    HistoryClean,
}

impl Command {
    pub fn name(&self) -> &'static str {
        match self {
            Command::Add { .. } => "add",
            Command::Hello => "hello",
            Command::List => "list",
            Command::Get(_) => "get",
            Command::Pause(_) => "pause",
            Command::Resume(_) => "resume",
            Command::Cancel(_) => "cancel",
            Command::Remove(_) => "remove",
            Command::Shutdown { .. } => "shutdown",
            Command::Queues => "queues",
            Command::QueueCreate { .. } => "queue-create",
            Command::QueueStart(_) => "queue-start",
            Command::QueueStop(_) => "queue-stop",
            Command::QueueDelete { .. } => "queue-delete",
            Command::SetQueue(..) => "set-queue",
            Command::SetPriority(..) => "priority",
            Command::Move(..) => "move",
            Command::History => "history",
            Command::HistorySet(_) => "history-set",
            Command::HistoryPreview(_) => "history-preview",
            Command::HistoryClean => "history-clean",
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
                refuse_duplicate,
                category,
                queue,
            } => f
                .debug_struct("Add")
                .field("url", &dm_domain::redact::redact_url(url))
                .field("profile", profile)
                .field("idempotency_key", idempotency_key)
                .field("refuse_duplicate", refuse_duplicate)
                .field("category", category)
                .field("queue", queue)
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
    add <URL> [--profile ID] [--idempotency-key KEY] [--refuse-duplicate]
            [--category CAT] [--queue QUEUE-ID]
                         Ask the service to download URL (profile defaults to \"downloads\").
                         Repeating the same request with the same key returns the same job.
                         --refuse-duplicate: refuse if the same link is already listed.
                         CAT: compressed, documents, music, programs, video or other (the
                         folder comes from the service's destination rules).
    hello                Show the running service: versions, backend and capabilities.
    list                 List downloads known to the running service.
    get <JOB-ID>         Show one download.
    pause <JOB-ID>       Pause a download (manual pause; never undone by policy).
    resume <JOB-ID>      Resume a paused download (refused when the backend cannot validate
                         partial data).
    cancel <JOB-ID>      Cancel a download (nothing is deleted).
    remove <JOB-ID>      Remove a completed, failed or cancelled download from the list. The file
                         stays; unfinished downloads are refused (cancel them first).
    shutdown [--pause-downloads]
                         Stop the service. Unfinished downloads continue after the next start;
                         with --pause-downloads they are paused first and stay paused until
                         resumed. Waits up to 20 s until the service has actually exited.
    queues               List queues (ID, name, limit, running or stopped).
    queue-create <NAME> [--max-active N]
                         Create a queue (N: 1-16 downloads at once, default 3).
    queue-start <QUEUE-ID>, queue-stop <QUEUE-ID>
                         Start or stop a queue. Stopping pauses its running downloads with a
                         checkpoint; starting never resumes a download you paused yourself.
    queue-delete <QUEUE-ID> [--move-to QUEUE-ID]
                         Delete a queue. Never deletes downloads or files: a queue that still
                         has downloads needs --move-to. The main queue cannot be deleted.
    set-queue <JOB-ID> <QUEUE-ID>
                         Move a download to the end of another queue.
    priority <JOB-ID> <low|normal|high>
                         Set a download's priority (higher starts first when a place is free).
    move <JOB-ID> <up|down|top|bottom>
                         Change a download's place in its queue's execution order.
    history              Show how long finished downloads stay in the list.
    history-set <DAYS|forever>
                         Keep finished downloads in the list for DAYS (1-3650) after they
                         finished, or forever (the default). Files always stay on disk.
    history-preview <DAYS|forever>
                         Count the finished downloads a period would remove now (changes nothing).
    history-clean        Apply the saved period now: removes due list entries, never files.

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

/// Destination categories accepted by `add --category` (the contract's `FileCategory`).
const CATEGORIES: [&str; 6] = [
    "compressed",
    "documents",
    "music",
    "programs",
    "video",
    "other",
];

fn err_static(message: &str, json: bool) -> UsageError {
    UsageError {
        message: message.to_owned(),
        json,
        command: None,
    }
}

/// Parses `args` (without the program name).
pub fn parse(args: &[OsString]) -> Result<Parsed, UsageError> {
    let mut global = Global::default();
    let mut help = false;
    let mut version = false;
    let mut profile: Option<String> = None;
    let mut key: Option<String> = None;
    let mut pause_downloads = false;
    let mut refuse_duplicate = false;
    let mut category: Option<String> = None;
    let mut queue: Option<String> = None;
    let mut max_active: Option<String> = None;
    let mut move_to: Option<String> = None;
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
            "--pause-downloads" if inline.is_none() => pause_downloads = true,
            "--refuse-duplicate" if inline.is_none() => refuse_duplicate = true,
            "--category" => set_once(&mut category, "--category", value("--category")?)?,
            "--queue" => set_once(&mut queue, "--queue", value("--queue")?)?,
            "--max-active" => set_once(&mut max_active, "--max-active", value("--max-active")?)?,
            "--move-to" => set_once(&mut move_to, "--move-to", value("--move-to")?)?,
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
    if pause_downloads && command != "shutdown" {
        return Err(usage(
            "--pause-downloads is only valid for `shutdown`".into(),
            global.json,
        ));
    }
    let add_only = |command: &'static str| -> Result<(), UsageError> {
        if profile.is_some()
            || key.is_some()
            || refuse_duplicate
            || category.is_some()
            || queue.is_some()
        {
            return Err(err(
                command,
                "--profile, --idempotency-key, --refuse-duplicate, --category and --queue are only valid for `add`",
            ));
        }
        Ok(())
    };
    if max_active.is_some() && command != "queue-create" {
        return Err(err_static(
            "--max-active is only valid for `queue-create`",
            json,
        ));
    }
    if move_to.is_some() && command != "queue-delete" {
        return Err(err_static(
            "--move-to is only valid for `queue-delete`",
            json,
        ));
    }
    // A queue ID is `main` or a service-generated `q_…`; anything else is refused here.
    let queue_id = |command: &'static str, id: &str| -> Result<String, UsageError> {
        let ok = id == "main"
            || (id.starts_with("q_")
                && id.len() <= 64
                && id[2..].chars().all(|c| c.is_ascii_alphanumeric()));
        if ok {
            Ok(id.to_owned())
        } else {
            Err(err(command, "QUEUE-ID must be `main` or look like q_…"))
        }
    };
    let job_and = |command: &'static str| -> Result<(JobId, String), UsageError> {
        match rest.as_slice() {
            [id, value] => Ok((
                JobId::parse(id).map_err(|_| err(command, "JOB-ID must look like job_…"))?,
                value.clone(),
            )),
            _ => Err(err(command, "expected a JOB-ID and one value")),
        }
    };
    let one_queue = |command: &'static str| -> Result<String, UsageError> {
        match rest.as_slice() {
            [id] => queue_id(command, id),
            [] => Err(err(command, "missing QUEUE-ID")),
            _ => Err(err(command, "expected exactly one QUEUE-ID")),
        }
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
            if let Some(c) = &category
                && !CATEGORIES.contains(&c.as_str())
            {
                return Err(err(
                    "add",
                    "--category must be compressed, documents, music, programs, video or other",
                ));
            }
            if let Some(q) = &queue {
                queue_id("add", q)?;
            }
            Command::Add {
                url,
                profile: profile.take().unwrap_or_else(|| DEFAULT_PROFILE.to_owned()),
                idempotency_key: key.take(),
                refuse_duplicate,
                category: category.take(),
                queue: queue.take(),
            }
        }
        "hello" => {
            add_only("hello")?;
            if !rest.is_empty() {
                return Err(err("hello", "takes no arguments"));
            }
            Command::Hello
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
        "remove" => {
            add_only("remove")?;
            Command::Remove(one_job("remove")?)
        }
        "shutdown" => {
            add_only("shutdown")?;
            if !rest.is_empty() {
                return Err(err("shutdown", "takes no arguments"));
            }
            Command::Shutdown { pause_downloads }
        }
        "queues" => {
            add_only("queues")?;
            if !rest.is_empty() {
                return Err(err("queues", "takes no arguments"));
            }
            Command::Queues
        }
        "queue-create" => {
            add_only("queue-create")?;
            let name = match rest.as_slice() {
                [name] => name.trim().to_owned(),
                _ => {
                    return Err(err(
                        "queue-create",
                        "expected exactly one NAME (quote spaces)",
                    ));
                }
            };
            if name.is_empty() || name.chars().count() > 64 || name.chars().any(char::is_control) {
                return Err(err(
                    "queue-create",
                    "NAME must be 1-64 printable characters",
                ));
            }
            let max_active = match max_active.as_deref() {
                None => None,
                Some(n) => match n.parse::<u32>() {
                    Ok(n) if (1..=16).contains(&n) => Some(n),
                    _ => return Err(err("queue-create", "--max-active must be 1-16")),
                },
            };
            Command::QueueCreate { name, max_active }
        }
        "queue-start" => {
            add_only("queue-start")?;
            Command::QueueStart(one_queue("queue-start")?)
        }
        "queue-stop" => {
            add_only("queue-stop")?;
            Command::QueueStop(one_queue("queue-stop")?)
        }
        "queue-delete" => {
            add_only("queue-delete")?;
            let queue = one_queue("queue-delete")?;
            let move_to = match move_to.as_deref() {
                None => None,
                Some(q) => Some(queue_id("queue-delete", q)?),
            };
            Command::QueueDelete { queue, move_to }
        }
        "set-queue" => {
            add_only("set-queue")?;
            let (job, queue) = job_and("set-queue")?;
            Command::SetQueue(job, queue_id("set-queue", &queue)?)
        }
        "priority" => {
            add_only("priority")?;
            let (job, priority) = job_and("priority")?;
            if !["low", "normal", "high"].contains(&priority.as_str()) {
                return Err(err("priority", "priority must be low, normal or high"));
            }
            Command::SetPriority(job, priority)
        }
        "history" | "history-clean" => {
            add_only("history")?;
            if !rest.is_empty() {
                return Err(err("history", "takes no arguments"));
            }
            if command == "history" {
                Command::History
            } else {
                Command::HistoryClean
            }
        }
        "history-set" | "history-preview" => {
            let name = if command == "history-set" {
                "history-set"
            } else {
                "history-preview"
            };
            add_only(name)?;
            let days = match rest.as_slice() {
                [d] if d == "forever" => None,
                [d] => match d.parse::<u32>() {
                    Ok(n) if (1..=3650).contains(&n) => Some(n),
                    _ => return Err(err(name, "DAYS must be 1-3650, or forever")),
                },
                _ => return Err(err(name, "expected DAYS (1-3650) or forever")),
            };
            if name == "history-set" {
                Command::HistorySet(days)
            } else {
                Command::HistoryPreview(days)
            }
        }
        "move" => {
            add_only("move")?;
            let (job, to) = job_and("move")?;
            if !["up", "down", "top", "bottom"].contains(&to.as_str()) {
                return Err(err("move", "the target must be up, down, top or bottom"));
            }
            Command::Move(job, to)
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
                idempotency_key: None,
                refuse_duplicate: false,
                category: None,
                queue: None,
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
                idempotency_key: Some("key-12345".into()),
                refuse_duplicate: false,
                category: None,
                queue: None,
            }
        );
        assert!(
            matches!(cmd(&["add", "--", "-weird"]), Command::Add { url, .. } if url == "-weird")
        );
    }

    #[test]
    fn add_sends_b28_options_only_when_given() {
        assert_eq!(
            cmd(&[
                "add",
                "https://h/f.zip",
                "--refuse-duplicate",
                "--category",
                "video",
                "--queue=q_ab12",
            ]),
            Command::Add {
                url: "https://h/f.zip".into(),
                profile: "downloads".into(),
                idempotency_key: None,
                refuse_duplicate: true,
                category: Some("video".into()),
                queue: Some("q_ab12".into()),
            }
        );
        for bad in [
            vec!["add", "https://h/f", "--category", "movies"],
            vec!["add", "https://h/f", "--queue", "../x"],
            vec!["list", "--refuse-duplicate"],
            vec!["get", "job_1", "--queue", "main"],
        ] {
            assert!(p(&bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn history_commands_validate_their_arguments() {
        assert_eq!(cmd(&["history"]), Command::History);
        assert_eq!(cmd(&["history-set", "30"]), Command::HistorySet(Some(30)));
        assert_eq!(cmd(&["history-set", "forever"]), Command::HistorySet(None));
        assert_eq!(
            cmd(&["history-preview", "7"]),
            Command::HistoryPreview(Some(7))
        );
        assert_eq!(cmd(&["history-clean"]), Command::HistoryClean);
        for bad in [
            vec!["history-set"],
            vec!["history-set", "0"],
            vec!["history-set", "3651"],
            vec!["history-set", "-1"],
            vec!["history-set", "30", "40"],
            vec!["history-clean", "now"],
            vec!["history", "--queue", "main"],
        ] {
            assert!(p(&bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn queue_commands_validate_their_arguments() {
        assert_eq!(cmd(&["queues"]), Command::Queues);
        assert_eq!(
            cmd(&["queue-create", "Night", "--max-active", "2"]),
            Command::QueueCreate {
                name: "Night".into(),
                max_active: Some(2)
            }
        );
        assert_eq!(
            cmd(&["queue-stop", "main"]),
            Command::QueueStop("main".into())
        );
        assert_eq!(
            cmd(&["queue-delete", "q_1", "--move-to", "main"]),
            Command::QueueDelete {
                queue: "q_1".into(),
                move_to: Some("main".into())
            }
        );
        let job = JobId::parse("job_abc").unwrap();
        assert_eq!(
            cmd(&["move", "job_abc", "top"]),
            Command::Move(job.clone(), "top".into())
        );
        assert_eq!(
            cmd(&["priority", "job_abc", "high"]),
            Command::SetPriority(job.clone(), "high".into())
        );
        assert_eq!(
            cmd(&["set-queue", "job_abc", "q_9"]),
            Command::SetQueue(job, "q_9".into())
        );
        for bad in [
            vec!["queue-create", "Night", "--max-active", "0"],
            vec!["queue-create", "Night", "--max-active", "17"],
            vec!["queue-create"],
            vec!["queue-start"],
            vec!["queue-start", "Main Queue"],
            vec!["queue-delete", "q_1", "extra"],
            vec!["move", "job_abc", "sideways"],
            vec!["priority", "job_abc", "urgent"],
            vec!["priority", "not-a-job", "high"],
            vec!["list", "--max-active", "2"],
            vec!["list", "--move-to", "main"],
        ] {
            assert!(p(&bad).is_err(), "{bad:?}");
        }
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
