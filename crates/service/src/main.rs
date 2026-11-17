//! `dm-service` — the per-user Downova download service (B0.2).
//!
//! ```text
//! dm-service [--data-dir DIR] [--download-dir DIR] [--backend http] [--yt-dlp PATH]
//! dm-service [--data-dir DIR] --restore-update OPERATION
//! ```
//! - Engines (ADR 0043): the native engine (`dm-backend-http`) always runs. The yt-dlp media
//!   engine runs too when an executable is found: `--yt-dlp PATH`, else `DM_YTDLP_PATH`, else
//!   `tools/ytdlp` in the working directory, else `PATH`. An explicitly named executable that is
//!   unusable fails startup; when none is found, the service runs with ordinary files only. Each
//!   download chooses its engine when it is added; nothing falls back from one engine to the
//!   other. `--backend http` is still accepted; `--backend yt-dlp` is refused (media is chosen
//!   per download now).
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

use dm_backend_http::{HttpBackend, HttpConfig};
use dm_backend_ytdlp::{LaunchConfig, YtdlpBackend, binary};
use dm_platform::{endpoint, ipc, paths};
use dm_service::core::{Core, Profiles};
use dm_service::{BackendError, DownloadBackend, EngineRouter};

#[derive(Debug, Default)]
struct Args {
    data_dir: Option<PathBuf>,
    download_dir: Option<PathBuf>,
    /// `--backend http` was given (accepted for older launchers; the native engine always runs).
    backend: Option<()>,
    ytdlp: Option<PathBuf>,
    /// `--restore-update OPERATION` (ADR 0030).
    restore_update: Option<String>,
}

const USAGE: &str =
    "usage: dm-service [--data-dir DIR] [--download-dir DIR] [--backend http] [--yt-dlp PATH]
  --backend http   the built-in Rust engine (always runs; accepted for older launchers)
  --yt-dlp PATH    yt-dlp executable for video and audio downloads (ADR 0043); without it the
                   service looks for one and runs with ordinary files only if none is found
       dm-service [--data-dir DIR] --restore-update OPERATION
  --restore-update replace an unusable database with that update's checked backup, then exit
       dm-service --version";

/// Backend-specific orderly stop (network and disk activity ended, engine process reaped).
trait Stop {
    async fn stop(&self) -> Result<(), BackendError>;
}

impl Stop for YtdlpBackend {
    async fn stop(&self) -> Result<(), BackendError> {
        self.shutdown().await
    }
}

impl Stop for HttpBackend {
    async fn stop(&self) -> Result<(), BackendError> {
        self.shutdown().await
    }
}

/// Both engines stop; the native engine's error is reported first.
impl<F: DownloadBackend + Stop, M: DownloadBackend + Stop> Stop for EngineRouter<F, M> {
    async fn stop(&self) -> Result<(), BackendError> {
        let media = match self.media() {
            Some(m) => m.stop().await,
            None => Ok(()),
        };
        self.file().stop().await.and(media)
    }
}

/// The media engine (ADR 0043): `Ok(None)` when none is installed; an error only when one was
/// named explicitly (`--yt-dlp` or `DM_YTDLP_PATH`) and cannot be used.
async fn media_engine(
    explicit: Option<&std::path::Path>,
    space: &Arc<dyn dm_service::SpaceGuard>,
) -> Result<Option<(YtdlpBackend, Option<String>)>, BackendError> {
    let named = explicit.is_some() || std::env::var_os("DM_YTDLP_PATH").is_some();
    let found = match explicit {
        Some(path) => binary::locate_at(path).await,
        None => binary::locate(std::env::current_dir().ok().as_deref()).await,
    };
    let bin = match found {
        Ok(bin) => bin,
        Err(e) if named => return Err(e),
        Err(_) => return Ok(None),
    };
    let version = bin.version.clone();
    let mut config = LaunchConfig::new(bin);
    config.space = Arc::clone(space);
    Ok(Some((YtdlpBackend::new(config), Some(version))))
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
            Some("--yt-dlp") => set_once(&mut args.ytdlp, value()?, "--yt-dlp")?,
            Some("--restore-update") => {
                let op = value()?.to_string_lossy().into_owned();
                set_once(&mut args.restore_update, op, "--restore-update")?;
            }
            Some("--backend") => {
                match value()?.to_str() {
                    Some("http") => {}
                    Some("yt-dlp") => {
                        return Err(format!(
                            "--backend yt-dlp is no longer needed: video and audio are chosen per download, and the media engine runs next to the native one (ADR 0043)\n{USAGE}"
                        ));
                    }
                    _ => return Err(format!("--backend must be http\n{USAGE}")),
                }
                set_once(&mut args.backend, (), "--backend")?;
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

/// Repeating an option is refused rather than letting the last one win silently.
fn set_once<T>(slot: &mut Option<T>, value: T, flag: &str) -> Result<(), String> {
    if slot.is_some() {
        return Err(format!("{flag} given more than once\n{USAGE}"));
    }
    *slot = Some(value);
    Ok(())
}

fn main() -> ExitCode {
    // Install the provider used by the native HTTP engine.
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
    let file = match HttpBackend::new(HttpConfig {
        space: Arc::clone(&space),
        extra_roots: test_roots(),
        ..HttpConfig::default()
    }) {
        Ok(backend) => backend,
        Err(e) => return fail("http backend failed to start", &e),
    };
    let version = file.version().to_owned();
    let media = match media_engine(args.ytdlp.as_deref(), &space).await {
        Ok(media) => media,
        Err(e) => return fail("yt-dlp unavailable", &e),
    };
    let label = match &media {
        Some((_, v)) => format!(
            "http backend {version} + media engine yt-dlp {} (ADR 0043)",
            v.as_deref().unwrap_or("?")
        ),
        None => format!(
            "http backend {version} (no media engine: video and audio downloads are unavailable; ADR 0043)"
        ),
    };
    let router = EngineRouter::new(file, media);
    let code = match Core::new(router, Some(version), profiles)
        .with_space_guard(Arc::clone(&space))
        .with_notifier(system_notifier())
        .with_vault(vault_for(&site.data_dir))
        .with_update_dir(site.update_dir.clone())
        .with_store(store)
    {
        Ok(core) => serve_with(core, &label, &site).await,
        Err(e) => return fail("cannot restore stored jobs", &e),
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
