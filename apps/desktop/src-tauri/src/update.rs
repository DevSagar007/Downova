//! Verified updates in the desktop shell (ADV09, ADR 0030).
//!
//! The shell owns checking, downloading, verifying and starting the installer; the service owns
//! quiescence, checkpoints and the backup (`service.v1.prepare_update`). The webview only asks
//! for fixed steps of one operation: it never supplies a URL, a key, a path or an installer.
//!
//! - **Trusted configuration** is the compiled `plugins.updater` section (merged at build time).
//!   [`check_config`] accepts it only with HTTPS endpoints, a public key and
//!   `requireSignedVersion: true`; plain-HTTP loopback endpoints are accepted only in a binary
//!   built with the `update-fixture` feature. Anything else is "not configured", never "up to date".
//! - **Package format.** The release entry is looked up only under this build's own
//!   `{os}-{arch}-{installer}` target ([`target`]); `.deb`/`.rpm` builds are updated by their
//!   package manager, and unpackaged builds have no updater.
//! - **Verification** is the plugin's: the minisign signature over the artifact, made with the
//!   compiled key, and the signed version bound to the announced one. Bytes are held in memory
//!   and the state says `verified` only after that succeeded.
//! - **One operation at a time.** Every step names its operation; a stale or duplicate request is
//!   refused. Installing first asks the service to prepare (lease, checkpoints, checked backup),
//!   then stops it (keep state) and confirms it is gone, and only then hands the verified bytes to
//!   the installer. Nothing essential is left to an exit callback.

use std::sync::Mutex;
use std::time::Duration;

use dm_domain::api::{
    CancelUpdateParams, EmptyResult, PrepareUpdateParams, PrepareUpdateResult, ShutdownParams,
    ShutdownResult, methods,
};
use dm_domain::{ErrorData, ErrorKind, NextAction};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::downloads::{self, BridgeError};

/// Window event carrying the update state after every change.
pub const UPDATE_EVENT: &str = "downova://update";
/// How long the service may take to exit before installing is abandoned.
pub const SHUTDOWN_BOUND: Duration = Duration::from_secs(60);
/// How long a check or download request may stall.
pub const NETWORK_TIMEOUT: Duration = Duration::from_secs(60);
/// Release notes shown at most (characters).
const MAX_NOTES: usize = 4000;

/// Built with the `update-fixture` feature: a test binary that may use a loopback HTTP endpoint
/// and a disposable key. Never set for normal packages (`scripts/package.mjs` refuses it).
pub const FIXTURE_BUILD: bool = cfg!(feature = "update-fixture");

/// Why there is no in-app update.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NotConfigured {
    /// This build has no updater configuration.
    Missing,
    /// The compiled configuration is not acceptable (insecure transport outside a fixture build,
    /// no key, signed-version binding off).
    Invalid,
    /// `.deb` / `.rpm`: updated by the system package manager.
    PackageManager,
    /// Not an installed package (a development build).
    Unpackaged,
}

fn is_loopback(host: &str) -> bool {
    matches!(host, "127.0.0.1" | "localhost" | "[::1]" | "::1")
}

/// Checks the compiled `plugins.updater` configuration.
pub fn check_config(value: Option<&serde_json::Value>, fixture: bool) -> Result<(), NotConfigured> {
    let Some(cfg) = value.and_then(serde_json::Value::as_object) else {
        return Err(NotConfigured::Missing);
    };
    let flag = |k: &str| {
        cfg.get(k)
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
    };
    let pubkey_ok = cfg
        .get("pubkey")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|k| !k.trim().is_empty());
    if !pubkey_ok
        || !flag("requireSignedVersion")
        || flag("dangerousAcceptInvalidCerts")
        || flag("dangerousAcceptInvalidHostnames")
        || (flag("dangerousInsecureTransportProtocol") && !fixture)
    {
        return Err(NotConfigured::Invalid);
    }
    let endpoints = cfg
        .get("endpoints")
        .and_then(serde_json::Value::as_array)
        .filter(|e| !e.is_empty())
        .ok_or(NotConfigured::Invalid)?;
    for e in endpoints {
        let url = e
            .as_str()
            .and_then(|s| tauri::Url::parse(s).ok())
            .ok_or(NotConfigured::Invalid)?;
        let ok = match url.scheme() {
            "https" => !fixture,
            "http" => fixture && url.host_str().is_some_and(is_loopback),
            _ => false,
        };
        if !ok {
            return Err(NotConfigured::Invalid);
        }
    }
    Ok(())
}

/// This build's package format, as the updater names it.
pub fn package_format() -> Result<&'static str, NotConfigured> {
    use tauri::utils::config::BundleType;
    use tauri::utils::platform::bundle_type;
    match bundle_type() {
        Some(BundleType::Nsis) => Ok("nsis"),
        Some(BundleType::Msi) => Ok("msi"),
        Some(BundleType::AppImage) => Ok("appimage"),
        Some(BundleType::App) => Ok("app"),
        Some(BundleType::Deb | BundleType::Rpm) => Err(NotConfigured::PackageManager),
        _ => Err(NotConfigured::Unpackaged),
    }
}

/// The exact manifest target of this build: `{os}-{arch}-{format}` (no `{os}-{arch}` fallback,
/// so an update for another package format is never selected).
pub fn target(format: &str) -> Option<String> {
    let os = if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        return None;
    };
    let arch = if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        return None;
    };
    Some(format!("{os}-{arch}-{format}"))
}

/// The update state shown to the user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum UpdateView {
    NotConfigured {
        reason: NotConfigured,
    },
    Idle,
    Checking {
        operation_id: String,
    },
    NoUpdate {
        operation_id: String,
    },
    Available {
        operation_id: String,
        version: String,
        date: Option<String>,
        notes: Option<String>,
    },
    Downloading {
        operation_id: String,
        version: String,
        downloaded: u64,
        total: Option<u64>,
    },
    Verifying {
        operation_id: String,
        version: String,
    },
    Verified {
        operation_id: String,
        version: String,
    },
    Preparing {
        operation_id: String,
        version: String,
    },
    ReadyToInstall {
        operation_id: String,
        version: String,
        prepared: PrepareUpdateResult,
    },
    Installing {
        operation_id: String,
        version: String,
    },
    RecoveryRequired {
        kind: String,
        backup_operation: Option<String>,
    },
    Failed {
        operation_id: Option<String>,
        /// `check`, `download`, `verify`, `prepare`, `shutdown`, `install` or `restore`.
        stage: &'static str,
        error: ErrorData,
        /// The verified update is still held: preparing can be tried again.
        can_retry_prepare: bool,
    },
}

/// What the dialog receives.
#[derive(Debug, Clone, Serialize)]
pub struct UpdateStatus {
    pub current_version: String,
    /// The package format, when this build has one.
    pub package: Option<&'static str>,
    pub fixture: bool,
    pub view: UpdateView,
}

#[derive(Default)]
struct Machine {
    view: Option<UpdateView>,
    op: Option<String>,
    update: Option<tauri_plugin_updater::Update>,
    bytes: Option<Vec<u8>>,
    busy: bool,
}

/// Managed state: whether updates are configured here, and the one operation.
pub struct Updates {
    configured: Result<String, NotConfigured>,
    machine: Mutex<Machine>,
}

impl std::fmt::Debug for Updates {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Updates")
            .field("configured", &self.configured)
            .finish_non_exhaustive()
    }
}

impl Updates {
    /// `config` is the compiled `plugins.updater` value; `Ok` carries the manifest target.
    pub fn new(config: Option<&serde_json::Value>) -> Self {
        let configured = check_config(config, FIXTURE_BUILD)
            .and_then(|()| package_format())
            .and_then(|f| target(f).ok_or(NotConfigured::Unpackaged));
        Self {
            configured,
            machine: Mutex::default(),
        }
    }

    /// The updater plugin is registered only for a usable configuration.
    pub fn enabled(&self) -> bool {
        self.configured.is_ok()
    }

    fn machine(&self) -> std::sync::MutexGuard<'_, Machine> {
        self.machine
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

fn invalid(why: &str) -> BridgeError {
    BridgeError::invalid_argument(why)
}

fn error(kind: ErrorKind, detail: &str) -> ErrorData {
    ErrorData {
        kind,
        retryable: matches!(
            kind,
            ErrorKind::NetworkOffline | ErrorKind::ServiceUnavailable
        ),
        next_action: if kind == ErrorKind::NetworkOffline {
            NextAction::Retry
        } else {
            NextAction::None
        },
        detail: Some(detail.to_owned()),
    }
}

/// A plugin error as a fixed, URL-free message (stage, error).
fn classify(e: &tauri_plugin_updater::Error, downloading: bool) -> (&'static str, ErrorData) {
    use tauri_plugin_updater::Error as E;
    match e {
        E::Minisign(_) | E::Base64(_) | E::SignatureUtf8(_) => (
            "verify",
            error(
                ErrorKind::ChecksumMismatch,
                "the update's signature does not match the trusted key; it was not installed",
            ),
        ),
        E::SignedVersionMismatch { .. } | E::MissingSignedVersion => (
            "verify",
            error(
                ErrorKind::ChecksumMismatch,
                "the update was not signed for the version it announced; it was not installed",
            ),
        ),
        E::TargetNotFound(_) | E::TargetsNotFound(_) => (
            "check",
            error(
                ErrorKind::InvalidRequest,
                "the update has no package for this installation's format",
            ),
        ),
        E::InsecureTransportProtocol => (
            "check",
            error(ErrorKind::InvalidRequest, "the update source is not secure"),
        ),
        E::Reqwest(_) | E::Network(_) | E::Io(_) => (
            if downloading { "download" } else { "check" },
            error(
                ErrorKind::NetworkOffline,
                if downloading {
                    "the update download was interrupted; nothing was installed"
                } else {
                    "the update source could not be reached"
                },
            ),
        ),
        E::ReleaseNotFound | E::Serialization(_) | E::Semver(_) | E::UrlParse(_) => (
            "check",
            error(
                ErrorKind::InvalidRequest,
                "the update source sent an invalid answer",
            ),
        ),
        _ => (
            if downloading { "download" } else { "check" },
            error(
                ErrorKind::Internal,
                "the update could not be checked or downloaded",
            ),
        ),
    }
}

fn new_operation() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    format!(
        "upd_{nanos:x}{:x}{:x}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    )
}

/// The recovery record the service left when it could not open its database (kind and backup
/// operation only; validated).
fn recovery(data_dir: &std::path::Path) -> Option<UpdateView> {
    let raw = std::fs::read(data_dir.join("update").join("recovery.json")).ok()?;
    let v: serde_json::Value = serde_json::from_slice(&raw).ok()?;
    let kind = v["kind"].as_str()?;
    if !matches!(
        kind,
        "migration_failed" | "newer_schema" | "storage_unavailable"
    ) {
        return None;
    }
    let backup = v["backup_operation"]
        .as_str()
        .filter(|op| dm_domain::validate::is_idempotency_key(op))
        .map(str::to_owned);
    Some(UpdateView::RecoveryRequired {
        kind: kind.to_owned(),
        backup_operation: backup,
    })
}

fn current_version<R: Runtime>(app: &AppHandle<R>) -> String {
    app.package_info().version.to_string()
}

fn publish<R: Runtime>(app: &AppHandle<R>, view: &UpdateView) {
    let _ = app.emit(UPDATE_EVENT, view);
}

/// Sets the view (and publishes it).
fn set<R: Runtime>(app: &AppHandle<R>, updates: &Updates, view: UpdateView) {
    updates.machine().view = Some(view.clone());
    publish(app, &view);
}

fn view_of(updates: &Updates, data_dir: Option<&std::path::Path>) -> UpdateView {
    if let Err(reason) = &updates.configured {
        return UpdateView::NotConfigured { reason: *reason };
    }
    let m = updates.machine();
    if !m.busy
        && let Some(r) = data_dir.and_then(recovery)
    {
        return r;
    }
    m.view.clone().unwrap_or(UpdateView::Idle)
}

/// `update_status`.
pub fn status<R: Runtime>(app: &AppHandle<R>) -> UpdateStatus {
    let updates = app.state::<Updates>();
    let data_dir = dm_platform::paths::data_dir().ok();
    UpdateStatus {
        current_version: current_version(app),
        package: package_format().ok(),
        fixture: FIXTURE_BUILD,
        view: view_of(&updates, data_dir.as_deref()),
    }
}

/// Claims the machine for one step of operation `op` (or a new operation when `op` is `None`).
fn begin(updates: &Updates, op: Option<&str>) -> Result<(), BridgeError> {
    let mut m = updates.machine();
    if m.busy {
        return Err(invalid("an update step is already running"));
    }
    if let Some(op) = op
        && m.op.as_deref() != Some(op)
    {
        return Err(invalid("this update operation is no longer current"));
    }
    m.busy = true;
    Ok(())
}

fn end(updates: &Updates) {
    updates.machine().busy = false;
}

/// `update_check`: a new operation; refused while a verified or prepared update is held.
pub async fn check<R: Runtime>(app: AppHandle<R>) -> Result<UpdateView, BridgeError> {
    use tauri_plugin_updater::UpdaterExt;
    let updates = app.state::<Updates>();
    let target = updates
        .configured
        .clone()
        .map_err(|_| invalid("updates are not configured for this installation"))?;
    begin(&updates, None)?;
    {
        let m = updates.machine();
        if m.bytes.is_some() {
            drop(m);
            end(&updates);
            return Err(invalid(
                "cancel the update that is already downloaded first",
            ));
        }
    }
    let op = new_operation();
    {
        let mut m = updates.machine();
        m.op = Some(op.clone());
        m.update = None;
    }
    set(
        &app,
        &updates,
        UpdateView::Checking {
            operation_id: op.clone(),
        },
    );
    let result = match app
        .updater_builder()
        .target(target)
        .timeout(NETWORK_TIMEOUT)
        .build()
    {
        Ok(updater) => updater.check().await,
        Err(e) => Err(e),
    };
    let view = match result {
        Ok(None) => UpdateView::NoUpdate {
            operation_id: op.clone(),
        },
        Ok(Some(update)) => {
            let view = UpdateView::Available {
                operation_id: op.clone(),
                version: update.version.clone(),
                date: update.date.map(|d| d.to_string()),
                notes: update
                    .body
                    .as_ref()
                    .map(|b| b.chars().take(MAX_NOTES).collect()),
            };
            updates.machine().update = Some(update);
            view
        }
        Err(e) => {
            let (stage, error) = classify(&e, false);
            UpdateView::Failed {
                operation_id: Some(op.clone()),
                stage,
                error,
                can_retry_prepare: false,
            }
        }
    };
    set(&app, &updates, view.clone());
    end(&updates);
    Ok(view)
}

/// `update_download`: downloads and verifies (signature and signed version) the available update.
pub async fn download<R: Runtime>(
    app: AppHandle<R>,
    op: String,
) -> Result<UpdateView, BridgeError> {
    let updates = app.state::<Updates>();
    begin(&updates, Some(&op))?;
    let Some(update) = updates.machine().update.clone() else {
        end(&updates);
        return Err(invalid("there is no update to download"));
    };
    let version = update.version.clone();
    set(
        &app,
        &updates,
        UpdateView::Downloading {
            operation_id: op.clone(),
            version: version.clone(),
            downloaded: 0,
            total: None,
        },
    );
    let mut downloaded: u64 = 0;
    let mut last = std::time::Instant::now();
    let progress_app = app.clone();
    let (op2, v2) = (op.clone(), version.clone());
    let finished_app = app.clone();
    let (op3, v3) = (op.clone(), version.clone());
    let result = update
        .download(
            move |chunk, total| {
                downloaded = downloaded.saturating_add(u64::try_from(chunk).unwrap_or(0));
                if last.elapsed() >= Duration::from_millis(250) {
                    last = std::time::Instant::now();
                    publish(
                        &progress_app,
                        &UpdateView::Downloading {
                            operation_id: op2.clone(),
                            version: v2.clone(),
                            downloaded,
                            total,
                        },
                    );
                }
            },
            move || {
                // The plugin verifies right after this; nothing is called verified yet.
                publish(
                    &finished_app,
                    &UpdateView::Verifying {
                        operation_id: op3,
                        version: v3,
                    },
                );
            },
        )
        .await;
    let view = match result {
        Ok(bytes) => {
            updates.machine().bytes = Some(bytes);
            UpdateView::Verified {
                operation_id: op.clone(),
                version,
            }
        }
        Err(e) => {
            let (stage, error) = classify(&e, true);
            updates.machine().bytes = None;
            UpdateView::Failed {
                operation_id: Some(op.clone()),
                stage,
                error,
                can_retry_prepare: false,
            }
        }
    };
    set(&app, &updates, view.clone());
    end(&updates);
    Ok(view)
}

/// `update_prepare`: the service quiesces and writes its checked backup (ADR 0030).
pub async fn prepare<R: Runtime>(app: AppHandle<R>, op: String) -> Result<UpdateView, BridgeError> {
    let updates = app.state::<Updates>();
    begin(&updates, Some(&op))?;
    let version = {
        let m = updates.machine();
        match (&m.update, &m.bytes) {
            (Some(u), Some(_)) => u.version.clone(),
            _ => {
                drop(m);
                end(&updates);
                return Err(invalid("the update is not downloaded and verified"));
            }
        }
    };
    set(
        &app,
        &updates,
        UpdateView::Preparing {
            operation_id: op.clone(),
            version: version.clone(),
        },
    );
    let result = match dm_platform::paths::data_dir() {
        Ok(dir) => {
            let params = PrepareUpdateParams {
                operation_id: op.clone(),
                target_version: version.clone(),
            };
            downloads::call_value::<_, PrepareUpdateResult>(&dir, methods::PREPARE_UPDATE, &params)
                .await
        }
        Err(_) => Err(BridgeError::no_data_dir()),
    };
    let view = match result {
        Ok(prepared) => UpdateView::ReadyToInstall {
            operation_id: op.clone(),
            version,
            prepared,
        },
        // The service released its lease: the running version stays usable.
        Err(e) => UpdateView::Failed {
            operation_id: Some(op.clone()),
            stage: "prepare",
            error: e.error,
            can_retry_prepare: true,
        },
    };
    set(&app, &updates, view.clone());
    end(&updates);
    Ok(view)
}

/// `update_cancel`: forgets the update; a prepared service lease ends and held work continues.
pub async fn cancel<R: Runtime>(app: AppHandle<R>, op: String) -> Result<UpdateView, BridgeError> {
    let updates = app.state::<Updates>();
    begin(&updates, Some(&op))?;
    let prepared = matches!(
        updates.machine().view,
        Some(UpdateView::ReadyToInstall { .. })
    ) || matches!(
        updates.machine().view,
        Some(UpdateView::Failed {
            stage: "shutdown",
            ..
        })
    );
    if prepared && let Ok(dir) = dm_platform::paths::data_dir() {
        let params = CancelUpdateParams {
            operation_id: op.clone(),
        };
        let _ =
            downloads::call_value::<_, EmptyResult>(&dir, methods::CANCEL_UPDATE, &params).await;
    }
    {
        let mut m = updates.machine();
        m.update = None;
        m.bytes = None;
        m.op = None;
    }
    set(&app, &updates, UpdateView::Idle);
    end(&updates);
    Ok(UpdateView::Idle)
}

/// True when no other process holds the service's instance lock.
fn lock_free(data_dir: &std::path::Path) -> bool {
    let path = dm_platform::paths::run_dir(data_dir).join("service.lock");
    match std::fs::OpenOptions::new().write(true).open(&path) {
        Ok(f) => f.try_lock().is_ok(),
        Err(e) => e.kind() == std::io::ErrorKind::NotFound,
    }
}

/// `update_install`: only after `ready_to_install` and the user's confirmation. Stops the
/// service (keep state), confirms it has gone, then starts the installer with the verified bytes.
pub async fn install<R: Runtime>(app: AppHandle<R>, op: String) -> Result<UpdateView, BridgeError> {
    let updates = app.state::<Updates>();
    begin(&updates, Some(&op))?;
    let (update, bytes, version) = {
        let m = updates.machine();
        match (&m.view, &m.update, &m.bytes) {
            (Some(UpdateView::ReadyToInstall { version, .. }), Some(u), Some(b)) => {
                (u.clone(), b.clone(), version.clone())
            }
            _ => {
                drop(m);
                end(&updates);
                return Err(invalid("the update is not ready to install"));
            }
        }
    };
    set(
        &app,
        &updates,
        UpdateView::Installing {
            operation_id: op.clone(),
            version: version.clone(),
        },
    );
    let fail = |stage: &'static str, error: ErrorData| UpdateView::Failed {
        operation_id: Some(op.clone()),
        stage,
        error,
        can_retry_prepare: false,
    };
    let Ok(data_dir) = dm_platform::paths::data_dir() else {
        let view = fail("shutdown", BridgeError::no_data_dir().error);
        set(&app, &updates, view.clone());
        end(&updates);
        return Ok(view);
    };
    app.state::<crate::stream::Streams>().stop();
    let stopped = downloads::call_value::<_, ShutdownResult>(
        &data_dir,
        methods::SHUTDOWN,
        &ShutdownParams::default(),
    )
    .await;
    let gone =
        stopped.is_ok() && crate::lifecycle::wait_until_gone(&data_dir, SHUTDOWN_BOUND).await && {
            let started = std::time::Instant::now();
            loop {
                if lock_free(&data_dir) {
                    break true;
                }
                if started.elapsed() > Duration::from_secs(10) {
                    break false;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        };
    if !gone {
        // Not installed: the service's lease expires and the running version continues.
        let view = fail(
            "shutdown",
            error(
                ErrorKind::ServiceUnavailable,
                "the download service did not stop in time, so the update was not installed",
            ),
        );
        set(&app, &updates, view.clone());
        end(&updates);
        return Ok(view);
    }
    // Every essential write happened above; on Windows this exits the app.
    let result = update.install(&bytes);
    let view = match result {
        Ok(()) => {
            // macOS / Linux: the new version runs after a restart.
            app.restart();
        }
        Err(e) => {
            let (_, error) = classify(&e, true);
            fail("install", error)
        }
    };
    set(&app, &updates, view.clone());
    end(&updates);
    Ok(view)
}

/// `update_restore`: replaces an unusable database with the recorded, checked backup by running
/// the installed service's own `--restore-update` (never while it runs).
pub async fn restore<R: Runtime>(app: AppHandle<R>, op: String) -> Result<UpdateView, BridgeError> {
    let updates = app.state::<Updates>();
    let data_dir = dm_platform::paths::data_dir().map_err(|_| BridgeError::no_data_dir())?;
    match recovery(&data_dir) {
        Some(UpdateView::RecoveryRequired {
            backup_operation: Some(b),
            ..
        }) if b == op => {}
        _ => return Err(invalid("there is no checked backup for this update")),
    }
    {
        let mut m = updates.machine();
        if m.busy {
            return Err(invalid("an update step is already running"));
        }
        m.busy = true;
    }
    let ready = matches!(
        dm_platform::launch::probe(&data_dir).await,
        dm_platform::launch::Readiness::Ready(_)
    );
    let result = if ready {
        Err("the download service is running")
    } else if let Some(service) = dm_platform::launch::sibling_service() {
        dm_platform::launch::restore_update(&service, &data_dir, &op).await
    } else {
        Err("the download service is not installed next to the app")
    };
    let view = match result {
        Ok(()) => UpdateView::Idle,
        Err(why) => UpdateView::Failed {
            operation_id: Some(op),
            stage: "restore",
            error: error(ErrorKind::Internal, why),
            can_retry_prepare: false,
        },
    };
    set(&app, &updates, view.clone());
    end(&updates);
    Ok(view)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn cfg(endpoint: &str) -> serde_json::Value {
        json!({"pubkey": "dW50cnVzdGVk", "endpoints": [endpoint], "requireSignedVersion": true})
    }

    #[test]
    fn only_a_secure_signed_version_configuration_is_trusted() {
        assert_eq!(check_config(None, false), Err(NotConfigured::Missing));
        assert_eq!(
            check_config(Some(&cfg("https://u.example/latest.json")), false),
            Ok(())
        );
        // Plain HTTP is refused outside a fixture build, even on loopback.
        assert_eq!(
            check_config(Some(&cfg("http://127.0.0.1:9/latest.json")), false),
            Err(NotConfigured::Invalid)
        );
        // A fixture build accepts loopback HTTP only, and never HTTPS production endpoints.
        assert_eq!(
            check_config(Some(&cfg("http://127.0.0.1:9/l.json")), true),
            Ok(())
        );
        assert_eq!(
            check_config(Some(&cfg("http://updates.example/l.json")), true),
            Err(NotConfigured::Invalid)
        );
        assert_eq!(
            check_config(Some(&cfg("https://u.example/l.json")), true),
            Err(NotConfigured::Invalid)
        );
        let mut no_binding = cfg("https://u.example/l.json");
        no_binding["requireSignedVersion"] = json!(false);
        assert_eq!(
            check_config(Some(&no_binding), false),
            Err(NotConfigured::Invalid)
        );
        let mut no_key = cfg("https://u.example/l.json");
        no_key["pubkey"] = json!(" ");
        assert_eq!(
            check_config(Some(&no_key), false),
            Err(NotConfigured::Invalid)
        );
        for flag in [
            "dangerousAcceptInvalidCerts",
            "dangerousAcceptInvalidHostnames",
            "dangerousInsecureTransportProtocol",
        ] {
            let mut c = cfg("https://u.example/l.json");
            c[flag] = json!(true);
            assert_eq!(
                check_config(Some(&c), false),
                Err(NotConfigured::Invalid),
                "{flag}"
            );
        }
        assert_eq!(
            check_config(
                Some(&json!({"pubkey": "k", "endpoints": [], "requireSignedVersion": true})),
                false
            ),
            Err(NotConfigured::Invalid)
        );
    }

    #[test]
    fn the_target_names_this_package_format_only() {
        let t = target("nsis").unwrap();
        assert!(t.ends_with("-nsis"), "{t}");
        assert_eq!(t.split('-').count(), 3);
        // A development build (no bundle type) has no updater.
        assert_eq!(package_format(), Err(NotConfigured::Unpackaged));
    }

    #[test]
    fn operations_are_valid_service_keys_and_unique() {
        let a = new_operation();
        let b = new_operation();
        assert_ne!(a, b);
        assert!(dm_domain::validate::is_idempotency_key(&a), "{a}");
    }
}
