//! Executes a parsed command against the service over IPC.
//!
//! Every connection starts with `service.v1.hello` and refuses an incompatible API version before
//! any application call. Connect is bounded by the platform client (5 s); each response wait by
//! [`RunOptions::call_timeout`]. The CLI never decides job outcomes — the service does.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use dm_domain::api::{
    API_VERSION, DownloadSnapshot, EmptyResult, HelloResult, HistoryCleanResult, HistoryPreview,
    HistorySettings, ListDownloadsResult, ListQueuesResult, LocateResult, QueueInfo,
    ReplaceSourceResult, ShutdownResult, methods,
};
use dm_domain::{ErrorData, ErrorKind};
use dm_platform::ipc::{CallError, Client};
use serde_json::{Value, json};

use crate::args::{self, Command, Global, Parsed};
use crate::exit;
use crate::output::{self, Failure};

#[derive(Debug, Clone)]
pub struct RunOptions {
    /// Upper bound for one request/response round trip.
    pub call_timeout: Duration,
    /// Re-send `add` once (same idempotency key, new connection) after a lost acknowledgement.
    pub retry_add_once: bool,
    /// How long `shutdown` waits for the service to exit after acknowledging (ADR 0012).
    pub stop_timeout: Duration,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            call_timeout: Duration::from_secs(30),
            retry_add_once: true,
            stop_timeout: Duration::from_secs(20),
        }
    }
}

/// Everything the binary needs to print and return.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit: u8,
}

fn version_text() -> String {
    format!(
        "dm {} (Downova CLI, service API v{API_VERSION})\n",
        env!("CARGO_PKG_VERSION")
    )
}

/// Parses and runs `args` (without the program name).
pub async fn run_cli(args: &[OsString], options: &RunOptions) -> CliOutput {
    match args::parse(args) {
        Err(usage) => {
            let failure = Failure {
                command: usage.command,
                exit_code: exit::USAGE,
                data: ErrorKind::InvalidRequest.to_error_data(usage.message),
                rpc_code: None,
            };
            render_failure(usage.json, &failure, true)
        }
        Ok(Parsed::Help(_)) => CliOutput {
            stdout: args::HELP.to_owned(),
            stderr: String::new(),
            exit: exit::OK,
        },
        Ok(Parsed::Version(g)) => {
            let stdout = if g.json {
                json!({"ok": true, "command": "version", "data": {"version": env!("CARGO_PKG_VERSION"), "api_version": API_VERSION}}).to_string() + "\n"
            } else {
                version_text()
            };
            CliOutput {
                stdout,
                stderr: String::new(),
                exit: exit::OK,
            }
        }
        Ok(Parsed::Run(global, command)) => execute(&global, &command, options).await,
    }
}

async fn execute(global: &Global, command: &Command, options: &RunOptions) -> CliOutput {
    let name = command.name();
    let data_dir = match resolve_data_dir(global) {
        Ok(dir) => dir,
        Err(f) => {
            return render_failure(
                global.json,
                &Failure {
                    command: Some(name),
                    ..f
                },
                false,
            );
        }
    };
    match perform(&data_dir, command, options).await {
        Ok((data, key)) => render_success(global.json, command, data, key.as_deref()),
        Err(f) => render_failure(
            global.json,
            &Failure {
                command: Some(name),
                ..f
            },
            false,
        ),
    }
}

/// `--data-dir` or the same per-user default every client uses (`dm_platform::paths`).
pub fn resolve_data_dir(global: &Global) -> Result<PathBuf, Failure> {
    match &global.data_dir {
        Some(dir) => Ok(dir.clone()),
        None => dm_platform::paths::data_dir().map_err(|_| {
            failure(
                exit::UNAVAILABLE,
                ErrorKind::ServiceUnavailable,
                "cannot determine the per-user data directory; use --data-dir",
            )
        }),
    }
}

fn failure(exit_code: u8, kind: ErrorKind, detail: &str) -> Failure {
    Failure {
        command: None,
        exit_code,
        data: kind.to_error_data(detail),
        rpc_code: None,
    }
}

/// Maps a client error to a failure with the documented exit code.
fn from_call(e: CallError) -> Failure {
    match e {
        CallError::Unavailable(why) => {
            failure(exit::UNAVAILABLE, ErrorKind::ServiceUnavailable, &why)
        }
        CallError::Rpc(rpc) => {
            let data: ErrorData = rpc
                .data
                .clone()
                .unwrap_or_else(|| ErrorKind::Internal.to_error_data(rpc.message.clone()));
            let exit_code = match (data.kind, rpc.code) {
                (_, dm_domain::api::codes::METHOD_NOT_FOUND) | (ErrorKind::Internal, _) => {
                    exit::UNEXPECTED
                }
                (ErrorKind::ServiceUnavailable, _) => exit::UNAVAILABLE,
                _ => exit::REJECTED,
            };
            Failure {
                command: None,
                exit_code,
                data,
                rpc_code: Some(rpc.code),
            }
        }
    }
}

/// Connects and performs the version handshake.
async fn connect(data_dir: &Path, timeout: Duration) -> Result<Client, Failure> {
    let mut client = Client::connect(data_dir).await.map_err(from_call)?;
    let hello = call(&mut client, methods::HELLO, json!({}), timeout).await?;
    let hello: HelloResult = serde_json::from_value(hello).map_err(|_| {
        failure(
            exit::UNEXPECTED,
            ErrorKind::Internal,
            "malformed hello response",
        )
    })?;
    if hello.api_version != API_VERSION {
        return Err(failure(
            exit::UNAVAILABLE,
            ErrorKind::ServiceUnavailable,
            &format!(
                "incompatible service API version {} (this dm speaks v{API_VERSION})",
                hello.api_version
            ),
        ));
    }
    Ok(client)
}

async fn call(
    client: &mut Client,
    method: &str,
    params: Value,
    timeout: Duration,
) -> Result<Value, Failure> {
    match tokio::time::timeout(timeout, client.call(method, params)).await {
        Ok(result) => result.map_err(from_call),
        Err(_) => Err(failure(
            exit::UNAVAILABLE,
            ErrorKind::ServiceUnavailable,
            "timed out waiting for the service",
        )),
    }
}

/// Runs the command. Returns the result data and, for `add`, the idempotency key used.
async fn perform(
    data_dir: &Path,
    command: &Command,
    options: &RunOptions,
) -> Result<(Value, Option<String>), Failure> {
    let mut client = connect(data_dir, options.call_timeout).await?;
    let job = |id: &dm_domain::JobId| json!({ "job_id": id });
    let operation_id = || -> Result<String, Failure> {
        dm_platform::random_hex(16)
            .map(|h| format!("cli_{h}"))
            .map_err(|_| failure(exit::UNEXPECTED, ErrorKind::Internal, "no OS randomness"))
    };
    let (method, params, key) = match command {
        Command::Add {
            url,
            profile,
            idempotency_key,
            refuse_duplicate,
            category,
            queue,
            sha256,
        } => {
            // One key per invocation; any retry of this request reuses it.
            let key = match idempotency_key {
                Some(k) => k.clone(),
                None => format!(
                    "cli-{}",
                    dm_platform::random_hex(16).map_err(|_| failure(
                        exit::UNEXPECTED,
                        ErrorKind::Internal,
                        "no OS randomness"
                    ))?
                ),
            };
            let mut params = json!({
                "idempotency_key": key,
                "source": { "url": url },
                "destination_profile_id": profile,
                "expected_size_bytes": null,
                "expected_sha256": sha256,
            });
            // B2.8/B2.11 fields only when asked for: without them the request (and its
            // idempotency fingerprint) is exactly the one older clients send.
            if *refuse_duplicate {
                params["on_duplicate"] = json!("refuse");
            }
            if let Some(c) = category {
                params["category"] = json!(c);
            }
            if let Some(q) = queue {
                params["queue_id"] = json!(q);
            }
            (methods::ADD, params, Some(key))
        }
        Command::Hello => (methods::HELLO, json!({}), None),
        Command::List => (methods::LIST, json!({}), None),
        Command::Get(id) => (methods::GET, job(id), None),
        Command::Pause(id) => (methods::PAUSE, job(id), None),
        Command::Resume(id) => (methods::RESUME, job(id), None),
        Command::Cancel(id) => (methods::CANCEL, job(id), None),
        Command::Remove(id) => (methods::REMOVE, job(id), None),
        Command::Shutdown { pause_downloads } => {
            let params = if *pause_downloads {
                json!({ "mode": "pause_downloads" })
            } else {
                json!({})
            };
            (methods::SHUTDOWN, params, None)
        }
        Command::Queues => (methods::QUEUES_LIST, json!({}), None),
        Command::QueueCreate { name, max_active } => {
            let mut params = json!({ "name": name });
            if let Some(n) = max_active {
                params["max_active"] = json!(n);
            }
            (methods::QUEUES_CREATE, params, None)
        }
        Command::QueueStart(q) => (methods::QUEUES_START, json!({ "queue_id": q }), None),
        Command::QueueStop(q) => (methods::QUEUES_STOP, json!({ "queue_id": q }), None),
        Command::QueueDelete { queue, move_to } => {
            let mut params = json!({ "queue_id": queue });
            if let Some(m) = move_to {
                params["move_to"] = json!(m);
            }
            (methods::QUEUES_DELETE, params, None)
        }
        Command::SetQueue(id, q) => (
            methods::SET_QUEUE,
            json!({ "job_id": id, "queue_id": q }),
            None,
        ),
        Command::SetPriority(id, p) => (
            methods::SET_PRIORITY,
            json!({ "job_id": id, "priority": p }),
            None,
        ),
        Command::Move(id, to) => (methods::MOVE, json!({ "job_id": id, "to": to }), None),
        Command::History => (methods::HISTORY_GET, json!({}), None),
        Command::HistorySet(days) => (
            methods::HISTORY_SET,
            json!({ "retention_days": days }),
            None,
        ),
        Command::HistoryPreview(days) => (
            methods::HISTORY_PREVIEW,
            json!({ "retention_days": days }),
            None,
        ),
        Command::HistoryClean => (methods::HISTORY_CLEAN, json!({}), None),
        // One operation id per invocation: a repeat after a lost acknowledgement reuses it.
        Command::Retry {
            job: id,
            discard_partial,
        } => (
            methods::RETRY,
            json!({ "job_id": id, "operation_id": operation_id()?, "discard_partial": discard_partial }),
            None,
        ),
        Command::RefreshLink {
            job: id,
            url,
            restart,
        } => (
            methods::REPLACE_SOURCE,
            json!({
                "job_id": id,
                "operation_id": operation_id()?,
                "url": url,
                "mode": if *restart { "restart" } else { "keep_if_verified" },
            }),
            None,
        ),
        Command::Locate(id) => (methods::LOCATE, job(id), None),
    };

    let mut result = call(&mut client, method, params.clone(), options.call_timeout).await;
    let lost_ack =
        matches!(&result, Err(f) if f.exit_code == exit::UNAVAILABLE && f.rpc_code.is_none());
    let replayable =
        method == methods::ADD || method == methods::RETRY || method == methods::REPLACE_SOURCE;
    if lost_ack && replayable && options.retry_add_once {
        // The service may have created the job before the reply was lost: replaying the same
        // idempotency key returns that job instead of creating a duplicate (COR-09).
        let mut retry = connect(data_dir, options.call_timeout).await?;
        result = call(&mut retry, method, params, options.call_timeout).await;
    }
    let value = result?;

    // Validate the shape so malformed replies are reported, not printed.
    let shape_ok = match command {
        Command::Hello => serde_json::from_value::<HelloResult>(value.clone()).is_ok(),
        Command::List => serde_json::from_value::<ListDownloadsResult>(value.clone()).is_ok(),
        Command::Shutdown { .. } => serde_json::from_value::<ShutdownResult>(value.clone()).is_ok(),
        Command::Queues => serde_json::from_value::<ListQueuesResult>(value.clone()).is_ok(),
        Command::QueueCreate { .. } | Command::QueueStart(_) | Command::QueueStop(_) => {
            serde_json::from_value::<QueueInfo>(value.clone()).is_ok()
        }
        Command::QueueDelete { .. } => serde_json::from_value::<EmptyResult>(value.clone()).is_ok(),
        Command::History | Command::HistorySet(_) => {
            serde_json::from_value::<HistorySettings>(value.clone()).is_ok()
        }
        Command::HistoryPreview(_) => {
            serde_json::from_value::<HistoryPreview>(value.clone()).is_ok()
        }
        Command::HistoryClean => {
            serde_json::from_value::<HistoryCleanResult>(value.clone()).is_ok()
        }
        Command::RefreshLink { .. } => {
            serde_json::from_value::<ReplaceSourceResult>(value.clone()).is_ok()
        }
        Command::Locate(_) => serde_json::from_value::<LocateResult>(value.clone()).is_ok(),
        _ => serde_json::from_value::<DownloadSnapshot>(value.clone()).is_ok(),
    };
    if !shape_ok {
        return Err(failure(
            exit::UNEXPECTED,
            ErrorKind::Internal,
            "malformed response from the service",
        ));
    }
    if let Command::Shutdown { .. } = command {
        // The acknowledgement is not termination (ADR 0012): wait until the service has gone.
        drop(client);
        let ack: ShutdownResult = serde_json::from_value(value).map_err(|_| {
            failure(
                exit::UNEXPECTED,
                ErrorKind::Internal,
                "malformed response from the service",
            )
        })?;
        if !gone(data_dir, options.stop_timeout).await {
            return Err(failure(
                exit::UNEXPECTED,
                ErrorKind::ServiceUnavailable,
                &format!(
                    "the service acknowledged the stop ({} download(s) paused) but was still running after {} s",
                    ack.paused_jobs,
                    options.stop_timeout.as_secs()
                ),
            ));
        }
        let mut data = serde_json::to_value(&ack).map_err(|_| {
            failure(
                exit::UNEXPECTED,
                ErrorKind::Internal,
                "cannot encode the result",
            )
        })?;
        data["stopped"] = json!(true);
        return Ok((data, None));
    }
    Ok((value, key))
}

/// True once the service's endpoint no longer answers `hello`, within `limit`.
async fn gone(data_dir: &Path, limit: Duration) -> bool {
    let started = std::time::Instant::now();
    while started.elapsed() < limit {
        if matches!(
            dm_platform::launch::probe(data_dir).await,
            dm_platform::launch::Readiness::NotRunning(_)
        ) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    false
}

fn render_success(json: bool, command: &Command, data: Value, key: Option<&str>) -> CliOutput {
    let name = command.name();
    let stdout = if json {
        output::success_json(name, data, key) + "\n"
    } else {
        match command {
            Command::Hello => match serde_json::from_value::<HelloResult>(data) {
                Ok(h) => output::hello_text(&h),
                Err(_) => String::new(),
            },
            Command::List => {
                let list: ListDownloadsResult =
                    serde_json::from_value(data).unwrap_or(ListDownloadsResult { items: vec![] });
                output::list_text(&list.items)
            }
            Command::Shutdown { .. } => output::shutdown_text(&data),
            Command::Queues => match serde_json::from_value::<ListQueuesResult>(data) {
                Ok(list) => output::queues_text(&list.queues),
                Err(_) => String::new(),
            },
            Command::QueueCreate { .. } | Command::QueueStart(_) | Command::QueueStop(_) => {
                match serde_json::from_value::<QueueInfo>(data) {
                    Ok(q) => output::queues_text(std::slice::from_ref(&q)),
                    Err(_) => String::new(),
                }
            }
            Command::History | Command::HistorySet(_) => {
                match serde_json::from_value::<HistorySettings>(data) {
                    Ok(h) => match h.retention_days {
                        None => "Finished downloads stay in the list forever.\n".to_owned(),
                        Some(d) => format!(
                            "Finished downloads leave the list {d} day(s) after they finished. Files stay on disk.\n"
                        ),
                    },
                    Err(_) => String::new(),
                }
            }
            Command::HistoryPreview(_) => match serde_json::from_value::<HistoryPreview>(data) {
                Ok(p) => format!(
                    "{} of {} finished download(s) would leave the list now. Nothing was changed.\n",
                    p.expiring, p.finished
                ),
                Err(_) => String::new(),
            },
            Command::HistoryClean => match serde_json::from_value::<HistoryCleanResult>(data) {
                Ok(r) => format!(
                    "{} finished download(s) removed from the list; {} kept for a later retry. No file was deleted.\n",
                    r.removed, r.kept
                ),
                Err(_) => String::new(),
            },
            Command::QueueDelete { .. } => {
                "Queue deleted. No download or file was deleted.\n".to_owned()
            }
            Command::RefreshLink { .. } => {
                match serde_json::from_value::<ReplaceSourceResult>(data) {
                    Ok(r) => output::replace_text(&r),
                    Err(_) => String::new(),
                }
            }
            Command::Locate(_) => match serde_json::from_value::<LocateResult>(data) {
                Ok(l) => output::locate_text(&l),
                Err(_) => String::new(),
            },
            _ => match serde_json::from_value::<DownloadSnapshot>(data) {
                Ok(s) => {
                    let mut text = output::snapshot_text(&s);
                    if let Some(k) = key {
                        text.push_str(&format!(
                            "{:<10} {k}  (repeat with --idempotency-key to retry safely)\n",
                            "Key"
                        ));
                    }
                    text
                }
                Err(_) => String::new(),
            },
        }
    };
    CliOutput {
        stdout,
        stderr: String::new(),
        exit: exit::OK,
    }
}

fn render_failure(json: bool, f: &Failure, usage: bool) -> CliOutput {
    if json {
        CliOutput {
            stdout: output::error_json(f) + "\n",
            stderr: String::new(),
            exit: f.exit_code,
        }
    } else {
        let mut stderr = output::error_text(f);
        if usage {
            stderr.push_str("usage: dm [--json] [--data-dir DIR] <add|list|get|pause|resume|cancel|remove|shutdown> … (see `dm --help`)\n");
        }
        CliOutput {
            stdout: String::new(),
            stderr,
            exit: f.exit_code,
        }
    }
}
