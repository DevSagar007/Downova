//! `dm-service` — the per-user Downova download service (B0.2).
//!
//! ```text
//! dm-service [--data-dir DIR] [--download-dir DIR] [--backend http|aria2] [--aria2 PATH]
//! ```
//! - `--backend` selects the engine for the whole life of the process (ADR 0005). `http`
//!   (`dm-backend-http`) is the default and needs no external program. `aria2` must be chosen
//!   explicitly and keeps its own capability restrictions (ADR 0003). There is never a fallback
//!   from one backend to the other. `--aria2 PATH` is valid only together with `--backend aria2`.
//! - Single instance per user: an exclusive lock on `<data>/run/service.lock`.
//! - Disk budget: free space is checked before writing (ADR 0009); a full disk holds the job
//!   instead of failing it.
//! - Jobs and idempotency records persist in `<data>/downova.sqlite3` (B1.1, ADR 0006); the
//!   service is its only reader and writer.
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

/// The engine behind the service (ADR 0005: `http` is the default; `aria2` only when asked for).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum BackendChoice {
    #[default]
    Http,
    Aria2,
}

#[derive(Debug, Default)]
struct Args {
    data_dir: Option<PathBuf>,
    download_dir: Option<PathBuf>,
    /// `None` until `--backend` is given; the default applies after parsing.
    backend: Option<BackendChoice>,
    aria2: Option<PathBuf>,
}

const USAGE: &str =
    "usage: dm-service [--data-dir DIR] [--download-dir DIR] [--backend http|aria2] [--aria2 PATH]
  --backend http   default: built-in Rust HTTP/HTTPS backend (no external program)
  --backend aria2  aria2 sidecar (HTTP only, no resume or segmentation; ADR 0003)
  --aria2 PATH     aria2c executable; only with --backend aria2";

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
            Some("--data-dir") => set_once(&mut args.data_dir, value()?, "--data-dir")?,
            Some("--download-dir") => {
                set_once(&mut args.download_dir, value()?, "--download-dir")?;
            }
            Some("--aria2") => set_once(&mut args.aria2, value()?, "--aria2")?,
            Some("--backend") => {
                let choice = match value()?.to_str() {
                    Some("http") => BackendChoice::Http,
                    Some("aria2") => BackendChoice::Aria2,
                    _ => return Err(format!("--backend must be http or aria2\n{USAGE}")),
                };
                set_once(&mut args.backend, choice, "--backend")?;
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
    if args.aria2.is_some() && args.backend != Some(BackendChoice::Aria2) {
        return Err(format!(
            "--aria2 is only valid with --backend aria2 (the default backend is http)\n{USAGE}"
        ));
    }
    Ok(args)
}

/// Repeating an option is refused rather than letting the last one win silently.
fn set_once<T>(slot: &mut Option<T>, value: T, flag: &str) -> Result<(), String> {
    if slot.is_some() {
        return Err(format!("{flag} given more than once\n{USAGE}"));
    }
    *slot = Some(value);
    Ok(())
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

    // The single SQLite writer (ARCH-05), opened only after the instance lock is held.
    let store = match dm_storage::Store::open(&data_dir.join(dm_storage::FILE_NAME)) {
        Ok(store) => store,
        Err(e) => return fail("storage unavailable", &e),
    };

    // One disk budget policy for the backend and the coordinator (B1.4, ADR 0009).
    let space: Arc<dyn dm_service::SpaceGuard> = Arc::new(
        dm_service::space::PlatformSpaceGuard::new(dm_platform::space::default_probe()),
    );

    let mut profiles = Profiles::new();
    profiles.insert("downloads".to_owned(), download_dir.clone());
    let site = Site {
        data_dir,
        run_dir,
        download_dir,
    };
    let code = match args.backend.unwrap_or_default() {
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
            let label = format!("aria2 {version} (explicit --backend aria2, ADR 0003)");
            let core = match Core::new(backend, Some(version), profiles)
                .with_space_guard(Arc::clone(&space))
                .with_store(store)
            {
                Ok(core) => core,
                Err(e) => return fail("cannot restore stored jobs", &e),
            };
            serve_with(core, &label, &site).await
        }
        BackendChoice::Http => {
            let backend = match HttpBackend::new(HttpConfig {
                space: Arc::clone(&space),
                ..HttpConfig::default()
            }) {
                Ok(backend) => backend,
                Err(e) => return fail("http backend failed to start", &e),
            };
            let version = backend.version().to_owned();
            let label = format!("http backend {version} (default, ADR 0005)");
            let core = match Core::new(backend, Some(version), profiles)
                .with_space_guard(Arc::clone(&space))
                .with_store(store)
            {
                Ok(core) => core,
                Err(e) => return fail("cannot restore stored jobs", &e),
            };
            serve_with(core, &label, &site).await
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
    // Unfinished jobs with a durable checkpoint continue (or stay paused) before clients connect.
    let recovered = core.recover().await;
    if recovered > 0 {
        eprintln!("dm-service: restored {recovered} interrupted download(s) from checkpoints");
    }
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
    // Stop transfers with their bytes synced and their checkpoints committed, so they continue
    // after the next start.
    core.prepare_shutdown().await;
    if let Err(e) = core.backend().stop().await {
        eprintln!("dm-service: backend shutdown: {e}");
    }
    eprintln!("dm-service stopped");
    ExitCode::SUCCESS
}
