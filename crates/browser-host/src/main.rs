//! `dm-browser-host` — Downova's Chrome-family native-messaging host (see the library docs).
//!
//! ```text
//! dm-browser-host chrome-extension://<id>/ [--parent-window=<n>]   (started by the browser)
//! dm-browser-host register   [--browser NAME]... [--host-path PATH] [--root DIR] [--registry-root KEY]
//! dm-browser-host repair     (same options as register)
//! dm-browser-host unregister [--browser NAME]... [--root DIR] [--registry-root KEY]
//! dm-browser-host status     [--browser NAME]... [--root DIR] [--registry-root KEY]
//! dm-browser-host --version
//! ```
//! Browsers: chrome, chrome-beta, chrome-dev, chrome-canary, chromium, edge (default: chrome).
//! Exit codes: 0 ok, 1 refused caller or protocol failure, 2 invalid arguments, 3 registration
//! failure.

use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use dm_browser_host::framing::{self, ReadError};
use dm_browser_host::handler::{self, Config};
use dm_browser_host::registration::{self, Browser, Target};
use dm_browser_host::{allowed_origins, origin_allowed};
use dm_domain::api::{NativeHostOutcome, NativeHostResponse};
use dm_domain::{ErrorData, ErrorKind};

const USAGE: &str = "usage: dm-browser-host register|repair|unregister|status [--browser NAME]... [--host-path PATH] [--root DIR] [--registry-root KEY]
       dm-browser-host --version
(the browser starts it as: dm-browser-host chrome-extension://<id>/)";

fn log(msg: &str) {
    // stderr only; never a URL or header. Chrome shows host stderr in its own log at most.
    let _ = writeln!(io::stderr(), "dm-browser-host: {msg}");
}

fn write(resp: &NativeHostResponse) -> io::Result<()> {
    let body = serde_json::to_vec(resp).map_err(io::Error::other)?;
    framing::write_frame(&mut io::stdout().lock(), &body)
}

fn refusal(error: ErrorData) -> NativeHostResponse {
    NativeHostResponse {
        v: 1,
        request_id: None,
        outcome: NativeHostOutcome::Rejected,
        job: None,
        hello: None,
        error: Some(error),
    }
}

fn host_mode(origin: &str) -> ExitCode {
    if !origin_allowed(origin) {
        log("refused: the calling extension is not on the allowlist");
        let _ = write(&refusal(
            ErrorKind::InvalidRequest.to_error_data("this extension is not allowed to use Downova"),
        ));
        return ExitCode::from(1);
    }
    let Ok(data_dir) = dm_platform::paths::data_dir() else {
        log("no per-user data directory");
        return ExitCode::from(1);
    };
    let config = Config {
        data_dir,
        call_timeout: Duration::from_secs(10),
    };
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        log("cannot start the runtime");
        return ExitCode::from(1);
    };
    let mut stdin = io::stdin().lock();
    loop {
        match framing::read_frame(&mut stdin) {
            Ok(body) => {
                let resp = runtime.block_on(handler::handle(&body, &config));
                log(&format!(
                    "request {} → {:?}",
                    resp.request_id.as_deref().unwrap_or("-"),
                    resp.outcome
                ));
                if write(&resp).is_err() {
                    return ExitCode::from(1);
                }
            }
            Err(ReadError::Eof) => return ExitCode::SUCCESS,
            Err(ReadError::TooLarge(n)) => {
                log(&format!("refused a {n}-byte message"));
                let _ = write(&refusal(ErrorKind::InvalidRequest.to_error_data(format!(
                    "message too large (limit {} bytes)",
                    framing::MAX_REQUEST
                ))));
                return ExitCode::from(1);
            }
            Err(ReadError::Truncated) => {
                log("input ended inside a message");
                return ExitCode::from(1);
            }
            Err(ReadError::Io(kind)) => {
                log(&format!("input error: {kind}"));
                return ExitCode::from(1);
            }
        }
    }
}

struct RegArgs {
    browsers: Vec<Browser>,
    host_path: Option<PathBuf>,
    root: Option<PathBuf>,
    registry_root: Option<String>,
}

fn parse_reg(args: &[String]) -> Result<RegArgs, String> {
    let mut out = RegArgs {
        browsers: Vec::new(),
        host_path: None,
        root: None,
        registry_root: None,
    };
    let mut it = args.iter();
    while let Some(flag) = it.next() {
        let mut value = || it.next().cloned().ok_or(format!("{flag} needs a value"));
        match flag.as_str() {
            "--browser" => {
                let v = value()?;
                let b = Browser::parse(&v).ok_or(format!("unknown browser {v}"))?;
                if !out.browsers.contains(&b) {
                    out.browsers.push(b);
                }
            }
            "--host-path" => out.host_path = Some(PathBuf::from(value()?)),
            "--root" => out.root = Some(PathBuf::from(value()?)),
            "--registry-root" => out.registry_root = Some(value()?),
            other => return Err(format!("unknown argument {other}")),
        }
    }
    if out.browsers.is_empty() {
        out.browsers.push(Browser::Chrome);
    }
    Ok(out)
}

// Registration commands report to the terminal; host mode never prints (stdout is the protocol).
#[allow(clippy::print_stdout)]
fn registration_mode(command: &str, rest: &[String]) -> ExitCode {
    let args = match parse_reg(rest) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let host = match args
        .host_path
        .clone()
        .or_else(|| std::env::current_exe().ok())
    {
        Some(h) => h,
        None => {
            eprintln!("cannot determine the host path; pass --host-path");
            return ExitCode::from(2);
        }
    };
    let mut ok = true;
    for browser in args.browsers {
        let target = Target {
            browser,
            root: args.root.clone(),
            registry_root: args.registry_root.clone(),
        };
        let result = match command {
            "register" | "repair" => registration::register(&target, &host, &allowed_origins())
                .map(|p| format!("{}: registered {}", browser.name(), p.display())),
            "unregister" => registration::unregister(&target).map(|removed| {
                if removed.is_empty() {
                    format!("{}: nothing to remove", browser.name())
                } else {
                    format!("{}: removed {}", browser.name(), removed.join(", "))
                }
            }),
            _ => Ok(registration::status(&target)),
        };
        match result {
            Ok(line) => println!("{line}"),
            Err(e) => {
                ok = false;
                eprintln!("{}: {e}", browser.name());
            }
        }
    }
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(3)
    }
}

#[allow(clippy::print_stdout)] // `--version` only
fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(first) = args.first() else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    match first.as_str() {
        // Chrome passes the caller origin first (and `--parent-window=<n>` on Windows).
        o if o.starts_with("chrome-extension://") => host_mode(o),
        "register" | "repair" | "unregister" | "status" => {
            registration_mode(first, args.get(1..).unwrap_or_default())
        }
        "--version" | "-V" => {
            println!("dm-browser-host {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}
