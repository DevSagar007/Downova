//! `dm-service` — the per-user Downova download service (B0.2).
//!
//! ```text
//! dm-service [--data-dir DIR] [--download-dir DIR] [--backend aria2|http] [--aria2 PATH]
//! ```
//! - `--backend` selects the engine (ADR 0004). `aria2` is the default; `http` is the
//!   experimental Rust HTTP backend (`dm-backend-http`). `--aria2` applies to `aria2` only.
//! - Single instance per user: an exclusive lock on `<data>/run/service.lock`.
//! - Listens on a user-only local endpoint and records it in `<data>/run/endpoint.json`.
//! - Stops on Ctrl+C or `service.v1.shutdown`; the backend is stopped and the endpoint file removed.
//!
//! Exit codes: 0 clean stop, 2 invalid arguments, 3 already running, 4 startup failure.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use dm_backend_aria2::{Aria2Backend, LaunchConfig, binary};
use dm_backend_http::{HttpBackend, HttpConfig};
use dm_platform::{endpoint, ipc, paths};
use dm_service::core::{Core, Profiles};
use dm_service::{BackendError, DownloadBackend};

/// The engine behind the service (ADR 0004: aria2 stays the default).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum BackendChoice {
    #[default]
    Aria2,
    Http,
}

#[derive(Debug, Default)]
struct Args {
    data_dir: Option<PathBuf>,
    download_dir: Option<PathBuf>,
    backend: BackendChoice,
    aria2: Option<PathBuf>,
}

const USAGE: &str =
    "usage: dm-service [--data-dir DIR] [--download-dir DIR] [--backend aria2|http] [--aria2 PATH]";

/// Backend-specific orderly stop (network and disk activity ended, engine process reaped).
trait Stop {
    async fn stop(&self) -> Result<(), BackendError>;
}

impl Stop for Aria2Backend {
    async fn stop(&self) -> Result<(), BackendError> {
        self.shutdown().await
    }
}

impl Stop for HttpBackend {
    async fn stop(&self) -> Result<(), BackendError> {
        self.shutdown().await
    }
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args::default();
    let mut it = std::env::args_os().skip(1);
    while let Some(flag) = it.next() {
        let mut value = || {
            it.next()
                .map(PathBuf::from)
                .ok_or(format!("{} needs a value", flag.to_string_lossy()))
        };
        match flag.to_str() {
            Some("--data-dir") => args.data_dir = Some(value()?),
            Some("--download-dir") => args.download_dir = Some(value()?),
            Some("--aria2") => args.aria2 = Some(value()?),
            Some("--backend") => {
                args.backend = match value()?.to_str() {
                    Some("aria2") => BackendChoice::Aria2,
                    Some("http") => BackendChoice::Http,
                    _ => return Err(format!("--backend must be aria2 or http\n{USAGE}")),
                }
            }
            Some("-h" | "--help") => return Err(USAGE.to_owned()),
            _ => {
                return Err(format!(
                    "unknown argument {}\n{USAGE}",
                    flag.to_string_lossy()
                ));
            }
        }
    }
    Ok(args)
}

fn main() -> ExitCode {
    // This binary links both backends, so Cargo unifies reqwest features: the TLS support that
    // dm-backend-http enables (rustls without a bundled provider) also applies to the aria2
    // adapter's loopback RPC client, which would otherwise panic for want of a crypto provider.
    // Install ring (the provider dm-backend-http uses) as the process default once, here at the
    // composition point. An Err only means a provider is already installed.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let args = match parse_args() {
        Ok(args) => args,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(2);
        }
    };
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("dm-service: cannot start runtime: {e}");
            return ExitCode::from(4);
        }
    };
    runtime.block_on(run(args))
}

async fn run(args: Args) -> ExitCode {
    let fail = |what: &str, e: &dyn std::fmt::Display| {
        eprintln!("dm-service: {what}: {e}");
        ExitCode::from(4)
    };
    let data_dir = match args.data_dir.map_or_else(paths::data_dir, Ok) {
        Ok(dir) => dir,
        Err(e) => return fail("no data directory", &e),
    };
    let run_dir = paths::run_dir(&data_dir);
    if let Err(e) =
        paths::ensure_private_dir(&data_dir).and_then(|()| paths::ensure_private_dir(&run_dir))
    {
        return fail("cannot prepare data directory", &e);
    }

    // Single instance per user (ARCH-01). The lock is released when the process exits.
    let lock = match std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(run_dir.join("service.lock"))
    {
        Ok(file) => file,
        Err(e) => return fail("cannot open lock file", &e),
    };
    if lock.try_lock().is_err() {
        eprintln!("dm-service: another instance is already running for this user");
        return ExitCode::from(3);
    }

    let download_dir = match args
        .download_dir
        .map_or_else(paths::default_download_dir, Ok)
    {
        Ok(dir) => dir,
        Err(e) => return fail("no download directory", &e),
    };
    if let Err(e) = std::fs::create_dir_all(&download_dir) {
        return fail("cannot create download directory", &e);
    }

    let mut profiles = Profiles::new();
    profiles.insert("downloads".to_owned(), download_dir.clone());
    let site = Site {
        data_dir,
        run_dir,
        download_dir,
    };
    let code = match args.backend {
        BackendChoice::Aria2 => {
            let bin = match args.aria2 {
                Some(path) => binary::locate_at(&path).await,
                None => binary::locate(std::env::current_dir().ok().as_deref()).await,
            };
            let bin = match bin {
                Ok(bin) => bin,
                Err(e) => return fail("aria2c unavailable", &e),
            };
            let version = bin.version.clone();
            let backend =
                match Aria2Backend::launch(LaunchConfig::new(bin, site.data_dir.join("aria2")))
                    .await
                {
                    Ok(backend) => backend,
                    Err(e) => return fail("aria2 failed to start", &e),
                };
            let label = format!("aria2 {version}");
            serve_with(Core::new(backend, Some(version), profiles), &label, &site).await
        }
        BackendChoice::Http => {
            if args.aria2.is_some() {
                eprintln!("dm-service: --aria2 applies only to --backend aria2\n{USAGE}");
                return ExitCode::from(2);
            }
            let backend = match HttpBackend::new(HttpConfig::default()) {
                Ok(backend) => backend,
                Err(e) => return fail("http backend failed to start", &e),
            };
            let version = backend.version().to_owned();
            let label = format!("http backend {version} (experimental, ADR 0004)");
            serve_with(Core::new(backend, Some(version), profiles), &label, &site).await
        }
    };
    drop(lock);
    code
}

/// Directories the service runs with.
struct Site {
    data_dir: PathBuf,
    run_dir: PathBuf,
    download_dir: PathBuf,
}

async fn serve_with<B: DownloadBackend + Stop + 'static>(
    core: Core<B>,
    label: &str,
    site: &Site,
) -> ExitCode {
    let fail = |what: &str, e: &dyn std::fmt::Display| {
        eprintln!("dm-service: {what}: {e}");
        ExitCode::from(4)
    };
    let core = Arc::new(core);
    let (data_dir, run_dir, download_dir) = (&site.data_dir, &site.run_dir, &site.download_dir);
    let bound = match ipc::bind(run_dir) {
        Ok(bound) => bound,
        Err(e) => return fail("cannot bind IPC endpoint", &e),
    };
    let endpoint_name = bound.endpoint.clone();
    let info = endpoint::EndpointInfo {
        version: 1,
        endpoint: endpoint_name.clone(),
        pid: std::process::id(),
    };
    if let Err(e) = endpoint::write(data_dir, &info) {
        return fail("cannot write endpoint file", &e);
    }
    eprintln!(
        "dm-service ready: {label}, downloads -> {}, data -> {}",
        download_dir.display(),
        data_dir.display()
    );

    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    dm_service::server::serve(Arc::clone(&core), bound, ctrl_c).await;

    endpoint::remove_if_ours(data_dir, &endpoint_name);
    if let Err(e) = core.backend().stop().await {
        eprintln!("dm-service: backend shutdown: {e}");
    }
    eprintln!("dm-service stopped");
    ExitCode::SUCCESS
}
