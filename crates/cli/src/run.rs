//! Executes a parsed command against the service over IPC.
//!
//! Every connection starts with `service.v1.hello` and refuses an incompatible API version before
//! any application call. Connect is bounded by the platform client (5 s); each response wait by
//! [`RunOptions::call_timeout`]. The CLI never decides job outcomes — the service does.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use dm_domain::api::{
    API_VERSION, DownloadSnapshot, HelloResult, ListDownloadsResult, ShutdownResult, methods,
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
    let (method, params, key) = match command {
        Command::Add {
            url,
            profile,
            idempotency_key,
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
            let params = json!({
                "idempotency_key": key,
                "source": { "url": url },
                "destination_profile_id": profile,
                "expected_size_bytes": null,
                "expected_sha256": null,
            });
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
    };

    let mut result = call(&mut client, method, params.clone(), options.call_timeout).await;
    let lost_ack =
        matches!(&result, Err(f) if f.exit_code == exit::UNAVAILABLE && f.rpc_code.is_none());
    if lost_ack && method == methods::ADD && options.retry_add_once {
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
