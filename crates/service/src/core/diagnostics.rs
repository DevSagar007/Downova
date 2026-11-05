//! Redacted diagnostics (E05, SEC-01, ADR 0026).
//!
//! The report is built from an **allowlist of typed fields**: nothing reaches it that is not named
//! below, so cookies, tokens, headers, URLs, hosts, file names, paths and free-text error details
//! cannot appear. The serialized text then passes [`dm_domain::redact::export_leaks`] as defence in
//! depth; a match refuses the preview or the export. Collection copies a bounded summary under the
//! coordinator lock and does everything else after releasing it. Nothing is transmitted.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

use serde::Serialize;

use dm_domain::api::{DiagnosticsExportResult, DiagnosticsPreview};
use dm_domain::{ByteCount, ErrorKind, NextAction};

/// Version of the report's layout.
pub const SCHEMA_VERSION: u32 = 1;
/// Jobs considered for the summary (most recent first); counts beyond are reported as such.
pub const MAX_JOBS: usize = 200;
/// Failures listed.
pub const MAX_FAILURES: usize = 50;
/// Largest report accepted (the contract's limit).
pub const MAX_BYTES: usize = 256 * 1024;

/// What the last startup recovered (the opt-in crash section).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct StartupSummary {
    pub journal_reconciled: usize,
    pub checkpoints_restored: usize,
}

#[derive(Debug, Serialize)]
pub(crate) struct Report {
    pub schema_version: u32,
    pub generated_at: String,
    pub app: App,
    pub backend: Backend,
    pub platform: Platform,
    pub settings: Settings,
    pub jobs: Jobs,
    pub recent_failures: Vec<Failure>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crash: Option<Crash>,
    pub limitations: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
pub(crate) struct App {
    pub service_version: &'static str,
    /// The source commit when the build recorded one (CI builds), otherwise null.
    pub build_commit: Option<&'static str>,
    pub os: &'static str,
    pub arch: &'static str,
}

#[derive(Debug, Serialize)]
pub(crate) struct Backend {
    pub id: &'static str,
    pub version: Option<String>,
    /// Capability → supported / unsupported / unverified (notes omitted).
    pub capabilities: BTreeMap<String, String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct Platform {
    /// `windows_attachment_services` or `not_supported`.
    pub attachment_check: &'static str,
    pub attachment_checks_in_flight: usize,
}

#[derive(Debug, Serialize)]
pub(crate) struct Settings {
    pub max_active_downloads: usize,
    pub global_speed_limit_bytes_per_second: Option<u64>,
    pub collision_policy: &'static str,
    pub history_retention_days: Option<u32>,
    pub notifications_enabled: bool,
    pub queues: usize,
    pub scheduled_queues: usize,
    pub destination_rules: usize,
}

#[derive(Debug, Default, Serialize)]
pub(crate) struct Jobs {
    pub total: usize,
    pub summarized: usize,
    pub by_phase: BTreeMap<String, usize>,
    pub by_blocker: BTreeMap<String, usize>,
    pub scan_by_status: BTreeMap<String, usize>,
    pub finalizing: usize,
}

#[derive(Debug, Serialize)]
pub(crate) struct Failure {
    pub job_id: String,
    pub phase: String,
    pub error_kind: ErrorKind,
    pub next_action: NextAction,
    pub scan_status: Option<String>,
    pub age_minutes: Option<u64>,
}

#[derive(Debug, Serialize)]
pub(crate) struct Crash {
    pub note: &'static str,
    pub last_startup: StartupSummary,
}

pub(crate) const LIMITATIONS: [&str; 5] = [
    "No file names, URLs, hosts, paths, cookies, headers or error details are included; failures are listed by kind only.",
    "Downova writes no crash dumps; the crash section (when included) is the last startup's recovery summary.",
    "Job lists are summarized from at most 200 recent jobs and 50 recent failures.",
    "The attachment check status says what Windows Attachment Services did; it is not a malware verdict.",
    "Nothing in this file was sent anywhere; it is shared only if you share it.",
];

/// Serializes `report`, enforces the size bound and the final redaction check, and names it.
pub(crate) fn finish(report: &Report) -> Result<(String, String), &'static str> {
    let content =
        serde_json::to_string_pretty(report).map_err(|_| "the report could not be serialized")?;
    if content.len() > MAX_BYTES {
        return Err("the report exceeds its size limit");
    }
    let leaks = dm_domain::redact::export_leaks(&content);
    if !leaks.is_empty() {
        eprintln!(
            "dm-service: diagnostics refused by the redaction check ({})",
            leaks.join(", ")
        );
        return Err("the report failed the redaction check and was not produced");
    }
    let id = format!(
        "diag_{}",
        dm_platform::random_hex(8).map_err(|_| "no randomness for the snapshot id")?
    );
    Ok((id, content))
}

pub(crate) fn preview(id: String, content: String) -> DiagnosticsPreview {
    let size = ByteCount(u64::try_from(content.len()).unwrap_or(u64::MAX));
    DiagnosticsPreview {
        snapshot_id: id,
        content,
        size_bytes: size,
    }
}

/// Why an export failed (mapped to service errors by the caller).
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ExportError {
    NotAbsolute,
    Exists,
    NoFolder,
    Refused(std::io::ErrorKind),
    Full,
}

/// Writes `content` to `path` atomically: a synced temporary file in the same folder, then a hard
/// link (which never replaces anything) or, with `replace`, a rename over the existing file. The
/// temporary file is removed on every path. The content is already sanitized: nothing
/// secret-bearing is ever written.
pub(crate) fn write(
    path: &Path,
    content: &[u8],
    replace: bool,
) -> Result<DiagnosticsExportResult, ExportError> {
    use sha2::{Digest, Sha256};
    if !path.is_absolute() {
        return Err(ExportError::NotAbsolute);
    }
    let dir = path
        .parent()
        .filter(|d| d.is_dir())
        .ok_or(ExportError::NoFolder)?;
    if !replace && path.exists() {
        return Err(ExportError::Exists);
    }
    let tmp = dir.join(format!(
        ".downova-diagnostics-{}.tmp",
        dm_platform::random_hex(6).unwrap_or_else(|_| "000000".into())
    ));
    let io = |e: std::io::Error| match e.kind() {
        std::io::ErrorKind::StorageFull | std::io::ErrorKind::QuotaExceeded => ExportError::Full,
        std::io::ErrorKind::AlreadyExists => ExportError::Exists,
        k => ExportError::Refused(k),
    };
    let written = (|| {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)?;
        f.write_all(content)?;
        f.sync_all()
    })();
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(io(e));
    }
    let placed = if replace {
        std::fs::rename(&tmp, path)
    } else {
        let linked = std::fs::hard_link(&tmp, path);
        match linked {
            Err(e) if e.kind() != std::io::ErrorKind::AlreadyExists => {
                // No hard links here: an exclusive create (never replaces) and a synced copy.
                (|| {
                    let mut f = std::fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(path)?;
                    f.write_all(content)?;
                    f.sync_all()
                })()
            }
            other => other,
        }
    };
    let _ = std::fs::remove_file(&tmp);
    placed.map_err(io)?;
    Ok(DiagnosticsExportResult {
        size_bytes: ByteCount(u64::try_from(content.len()).unwrap_or(u64::MAX)),
        sha256: Sha256::digest(content)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_is_atomic_never_replaces_unless_asked_and_leaves_no_temporary_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("d.json");
        let r = write(&p, b"{\"a\":1}", false).unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"{\"a\":1}");
        assert_eq!(r.size_bytes, ByteCount(7));
        assert_eq!(write(&p, b"other", false), Err(ExportError::Exists));
        assert_eq!(
            std::fs::read(&p).unwrap(),
            b"{\"a\":1}",
            "an existing file is kept"
        );
        write(&p, b"replaced", true).unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"replaced");
        assert_eq!(
            write(Path::new("relative.json"), b"x", false),
            Err(ExportError::NotAbsolute)
        );
        assert_eq!(
            write(&dir.path().join("missing").join("d.json"), b"x", false),
            Err(ExportError::NoFolder)
        );
        let left: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(left, vec!["d.json".to_owned()], "no temporary file is left");
    }
}
