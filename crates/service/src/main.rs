//! `dm-service` — the per-user Downova download service (B0.2).
//!
//! ```text
//! dm-service [--data-dir DIR] [--download-dir DIR] [--backend http|aria2] [--aria2 PATH]
//! dm-service [--data-dir DIR] --restore-update OPERATION
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
//! - Updates (ADV09, ADR 0030): after the database opened (migrations done), a pending update
//!   marker in `<data>/update` is closed as installed or not installed before anything is served.
//!   When the database cannot be opened, `update/recovery.json` records why and which checked backup
//!   could replace it, and the service exits with 6 instead of serving. `--restore-update OP`
//!   replaces the database with that update's checked backup (the old files are set aside), then
//!   exits; it never runs while another instance holds the lock.
//!
//! Exit codes: 0 clean stop (or restore done), 2 invalid arguments, 3 already running, 4 startup
//! failure, 5 restore refused, 6 the database needs recovery.

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
    /// `--restore-update OPERATION` (ADR 0030).
    restore_update: Option<String>,
}

const USAGE: &str =
    "usage: dm-service [--data-dir DIR] [--download-dir DIR] [--backend http|aria2] [--aria2 PATH]
  --backend http   default: built-in Rust HTTP/HTTPS backend (no external program)
  --backend aria2  aria2 sidecar (HTTP only, no resume or segmentation; ADR 0003)
  --aria2 PATH     aria2c executable; only with --backend aria2
       dm-service [--data-dir DIR] --restore-update OPERATION
  --restore-update replace an unusable database with that update's checked backup, then exit
       dm-service --version";

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
            Some("--restore-update") => {
                let op = value()?.to_string_lossy().into_owned();
                set_once(&mut args.restore_update, op, "--restore-update")?;
            }
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
    // `--version` alone: the build version (scripts/package.mjs checks that the components agree).
    let raw: Vec<_> = std::env::args_os().skip(1).collect();
    if raw.len() == 1 && (raw[0] == "--version" || raw[0] == "-V") {
        print_version();
        return ExitCode::SUCCESS;
    }
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

    let update_dir = data_dir.join("update");
    if let Some(op) = &args.restore_update {
        // ADR 0030: only under the instance lock and with no store open.
        let now = now_ms();
        return match dm_service::core::update::restore(
            &update_dir,
            &data_dir.join(dm_storage::FILE_NAME),
            op,
            dm_storage::SCHEMA_VERSION,
            now,
        ) {
            Ok(outcome) => {
                eprintln!(
                    "dm-service: restored the database from the backup of update {} (made by Downova {})",
                    outcome.operation_id, outcome.to_version
                );
                ExitCode::SUCCESS
            }
            Err(why) => {
                eprintln!("dm-service: restore refused: {why}");
                ExitCode::from(5)
            }
        };
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
        Err(e) => {
            // ADR 0030: a failed migration rolled back and left the database unchanged; record
            // why and which checked backup could replace it, and do not serve.
            let record = dm_service::core::update::record_recovery(
                &update_dir,
                &e,
                dm_storage::SCHEMA_VERSION,
                now_ms(),
            );
            eprintln!(
                "dm-service: storage unavailable ({}): {e}; recovery is required{}",
                record.kind,
                record
                    .backup_operation
                    .as_deref()
                    .map(|op| format!(" (a checked backup exists: --restore-update {op})"))
                    .unwrap_or_default()
            );
            return ExitCode::from(6);
        }
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
        update_dir,
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
                .with_notifier(system_notifier())
                .with_vault(vault_for(&site.data_dir))
                .with_update_dir(site.update_dir.clone())
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
                extra_roots: test_roots(),
                ..HttpConfig::default()
            }) {
                Ok(backend) => backend,
                Err(e) => return fail("http backend failed to start", &e),
            };
            let version = backend.version().to_owned();
            let label = format!("http backend {version} (default, ADR 0005)");
            let core = match Core::new(backend, Some(version), profiles)
                .with_space_guard(Arc::clone(&space))
                .with_notifier(system_notifier())
                .with_vault(vault_for(&site.data_dir))
                .with_update_dir(site.update_dir.clone())
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
    /// `<data>/update` (ADR 0030).
    update_dir: PathBuf,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// Debug builds only: `DOWNOVA_TEST_CA_DER` names a DER certificate trusted in addition to the
/// platform roots, so process tests can reach an HTTPS fixture (BRW-05 end-to-end). Release builds
/// trust the platform roots only.
fn test_roots() -> Vec<rustls::pki_types::CertificateDer<'static>> {
    #[cfg(debug_assertions)]
    if let Some(path) = std::env::var_os("DOWNOVA_TEST_CA_DER")
        && let Ok(der) = std::fs::read(path)
    {
        return vec![rustls::pki_types::CertificateDer::from(der)];
    }
    Vec::new()
}

/// The OS credential store, namespaced by the data folder so two profiles never share entries
/// (SEC-02, ADR 0027). Debug builds only: `DOWNOVA_VAULT=memory|off|file` replaces it for tests
/// (`file` keeps a test vault in the data folder so restarting process tests never touch the
/// developer's real store). Release builds always use the OS store.
fn vault_for(data_dir: &std::path::Path) -> Arc<dyn dm_platform::vault::Vault> {
    #[cfg(debug_assertions)]
    if let Ok(v) = std::env::var("DOWNOVA_VAULT") {
        match v.as_str() {
            "memory" => return Arc::new(dm_platform::vault::MemoryVault::default()),
            "off" => return Arc::new(dm_platform::vault::OffVault),
            "file" => {
                return Arc::new(dm_platform::vault::FileVault::new(
                    data_dir.join("test-vault.json"),
                ));
            }
            _ => {}
        }
    }
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(data_dir.to_string_lossy().as_bytes());
    let ns: String = digest.iter().take(6).map(|b| format!("{b:02x}")).collect();
    Arc::new(dm_platform::vault::OsVault::new(&ns))
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
    // ADR 0030: this version opened and migrated the database, so a pending update is closed
    // (installed, or not installed when the old version came back) before any work resumes.
    if let Some(outcome) = dm_service::core::update::accept_pending(
        &site.update_dir,
        dm_service::core::update::SERVICE_VERSION,
        now_ms(),
    ) {
        eprintln!(
            "dm-service: update {} {} ({} -> {})",
            outcome.operation_id, outcome.outcome, outcome.from_version, outcome.to_version
        );
    }
    // Unfinished jobs with a durable checkpoint continue (or stay paused) before clients connect.
    let recovered = core.recover().await;
    if recovered > 0 {
        eprintln!("dm-service: restored {recovered} interrupted download(s) from checkpoints");
    }
    // History retention (B2.12, ADR 0018): one bounded batch before clients connect; the rest
    // follow once a minute.
    let swept = core.sweep_history(dm_service::core::RETENTION_BATCH).await;
    if swept.removed > 0 || swept.kept > 0 {
        eprintln!(
            "dm-service: history retention removed {} finished download(s) from the list ({} kept for a retry)",
            swept.removed, swept.kept
        );
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
        exe: std::env::current_exe().ok(),
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
    // Nothing new starts from here on (already the case after service.v1.shutdown; Ctrl+C keeps
    // the downloads' state). Then stop transfers with their bytes synced and their checkpoints
    // committed: running jobs continue after the next start, jobs paused by "Exit and stop
    // downloads" stay paused (ADR 0012).
    core.begin_shutdown(dm_domain::api::ShutdownMode::KeepState)
        .await;
    // Bounded (B2.10): a stuck engine cannot hold the stop for ever. Cancellation only happens
    // between store transactions, so a job keeps its last committed checkpoint (at most
    // CHECKPOINT_INTERVAL old) and resumes from there.
    if tokio::time::timeout(
        dm_service::core::SHUTDOWN_CHECKPOINT_BOUND,
        core.prepare_shutdown(),
    )
    .await
    .is_err()
    {
        eprintln!(
            "dm-service: stopping transfers took too long; continuing from their last checkpoints"
        );
    }
    if let Err(e) = core.backend().stop().await {
        eprintln!("dm-service: backend shutdown: {e}");
    }
    eprintln!("dm-service stopped");
    ExitCode::SUCCESS
}

/// The desktop app's identifier: notifications are shown as the installed app (CORE-09).
const APP_ID: &str = "com.downova.desktop";

fn system_notifier() -> Arc<dyn dm_service::notify::Notifier> {
    Arc::new(SystemNotifier(dm_platform::notify::SystemNotifier::new(
        APP_ID,
    )))
}

/// Bridges the platform adapter to the service's notifier interface.
#[derive(Debug)]
struct SystemNotifier(dm_platform::notify::SystemNotifier);

impl dm_service::notify::Notifier for SystemNotifier {
    fn availability(&self) -> dm_domain::api::NotificationAvailability {
        dm_domain::api::NotificationAvailability::Available
    }
    fn show(&self, notice: &dm_service::notify::Notice) -> Result<(), String> {
        self.0.show(&notice.title, &notice.body)
    }
}

#[allow(clippy::print_stdout)] // `--version` only
fn print_version() {
    println!("dm-service {}", dm_domain::BUILD_VERSION);
}
