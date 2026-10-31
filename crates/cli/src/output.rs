//! Presentation. stdout carries results only; diagnostics go to stderr.
//!
//! JSON shape (one object, stable):
//! ```json
//! { "ok": true,  "command": "add", "data": <DownloadSnapshot | {"items":[…]}>, "idempotency_key": "…" }
//! { "ok": false, "command": "add" | null, "exit_code": 4,
//!   "error": { "kind": "BACKEND_CAPABILITY_UNAVAILABLE", "rpc_code": -32000 | null,
//!              "retryable": false, "next_action": "none", "detail": "…" | null } }
//! ```
//! `idempotency_key` appears only for `add`. All text passes through [`sanitize`].

use dm_domain::api::DownloadSnapshot;
use dm_domain::redact::redact_url;
use dm_domain::{Blocker, ByteCount, ErrorData, ErrorKind, NextAction, Phase};
use serde_json::{Value, json};

/// Replaces every `http(s)://…` token in free text with its redacted form (defence in depth:
/// the service already redacts, but the CLI never trusts text it prints).
pub fn sanitize(text: &str) -> String {
    text.split_inclusive(char::is_whitespace)
        .map(|piece| {
            let trimmed = piece.trim_end();
            let lower = trimmed.to_ascii_lowercase();
            match lower.find("http://").or_else(|| lower.find("https://")) {
                Some(at) => {
                    let (head, url) = trimmed.split_at(at);
                    format!("{head}{}{}", redact_url(url), &piece[trimmed.len()..])
                }
                None => piece.to_owned(),
            }
        })
        .collect()
}

fn sanitize_value(value: &mut Value) {
    match value {
        Value::String(s) => *s = sanitize(s),
        Value::Array(items) => items.iter_mut().for_each(sanitize_value),
        Value::Object(map) => map.values_mut().for_each(sanitize_value),
        _ => {}
    }
}

/// Binary-unit size, never rounded up.
pub fn bytes(n: ByteCount) -> String {
    const UNITS: [&str; 7] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB"];
    let mut value = n.0;
    let mut rem = 0u64;
    let mut unit = 0usize;
    while value >= 1024 && unit < UNITS.len().saturating_sub(1) {
        rem = value % 1024;
        value /= 1024;
        unit = unit.saturating_add(1);
    }
    let name = UNITS.get(unit).copied().unwrap_or("B");
    if unit == 0 {
        format!("{value} B")
    } else {
        format!("{value}.{} {name}", rem.saturating_mul(10) / 1024)
    }
}

fn progress(s: &DownloadSnapshot) -> String {
    match s.total_bytes {
        Some(total) if total.0 > 0 => {
            let permille = u128::from(s.downloaded_bytes.0)
                .saturating_mul(1000)
                .checked_div(u128::from(total.0))
                .unwrap_or(0);
            format!(
                "{} of {} ({}.{}%)",
                bytes(s.downloaded_bytes),
                bytes(total),
                permille / 10,
                permille % 10
            )
        }
        Some(total) => format!("{} of {}", bytes(s.downloaded_bytes), bytes(total)),
        None => format!("{} (total unknown)", bytes(s.downloaded_bytes)),
    }
}

fn wire<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

fn blockers(list: &[Blocker]) -> String {
    if list.is_empty() {
        return "none".into();
    }
    list.iter()
        .map(|b| wire(b).replace('_', " "))
        .collect::<Vec<_>>()
        .join(", ")
}

/// What a failure means, in one plain sentence (CORE-11). Keyed by the stable kind only, never
/// by the free-text detail, and English only: the desktop UI has its own English/Bangla copy.
pub fn explain(kind: ErrorKind) -> &'static str {
    match kind {
        ErrorKind::NetworkOffline => "the connection to the server failed or was interrupted",
        ErrorKind::AuthRequired => "the server or a proxy requires a sign-in (HTTP 401 or 407)",
        ErrorKind::LinkRefreshRequired => {
            "the server refused the link or no longer has the file; the link may have expired, been revoked or need a sign-in"
        }
        ErrorKind::RemoteChanged => {
            "the file on the server changed after the download started, so the saved part cannot be used"
        }
        ErrorKind::RangeUnsupported => {
            "the server cannot send part of a file, so the download cannot continue where it stopped"
        }
        ErrorKind::InvalidRange => {
            "the server sent data that does not fit the part that was asked for, or the file has the wrong length; it was not used"
        }
        ErrorKind::ChecksumMismatch => {
            "the downloaded file does not match the expected SHA-256 checksum; it was deleted, not saved"
        }
        ErrorKind::DiskFull => "the destination drive does not have enough free space",
        ErrorKind::VolumeMissing => "the destination drive or folder is not available",
        ErrorKind::DestinationUnwritable => {
            "the file cannot be written in the download folder (name not allowed there, no permission, read-only drive, or another file already has the name)"
        }
        ErrorKind::HostNotInstalled => "the browser cannot reach Downova's helper program",
        ErrorKind::ScanBlocked => "a security scan blocked this file",
        ErrorKind::BackendCapabilityUnavailable => {
            "the download engine cannot do this safely (for example, resume without a strong validator, or an unsupported link type)"
        }
        ErrorKind::ServerBusy => "the server is busy or limiting requests (HTTP 408, 429 or 5xx)",
        ErrorKind::TlsHandshakeFailed => {
            "a secure connection could not be made: the certificate is not trusted, has expired or does not match the site"
        }
        ErrorKind::ServiceUnavailable => {
            "the Downova background service is not running, stopped, or did not answer"
        }
        ErrorKind::InvalidRequest => "the request or the server's response could not be accepted",
        ErrorKind::IdempotencyConflict => {
            "this request key was already used for a different download"
        }
        ErrorKind::DuplicateDownload => {
            "the same link is already in the download list, and this request asked not to add it twice"
        }
        ErrorKind::Internal => "Downova hit an unexpected internal error",
    }
}

/// Where an error is seen, because the useful next step differs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Situation {
    /// A command the service refused (nothing changed).
    Request,
    /// A download that is still alive and will continue without the user (automatic retry, or
    /// waiting for disk space or its drive).
    Waiting,
    /// A failed download: it never continues by itself, and adding its link again starts over.
    Failed,
}

/// The situation of a download that carries an error.
pub fn situation(s: &DownloadSnapshot) -> Situation {
    if s.phase == Phase::Failed {
        Situation::Failed
    } else {
        Situation::Waiting
    }
}

/// Plain-language next step for an error. It only names actions that exist: `dm` can add, pause,
/// resume and cancel; a failed download cannot be resumed; there is no sign-in, cookie hand-off
/// or diagnostics export yet.
pub fn hint(action: NextAction, situation: Situation) -> Option<&'static str> {
    use Situation::{Failed, Request, Waiting};
    Some(match (action, situation) {
        (NextAction::None, _) => return None,
        (NextAction::Retry | NextAction::Wait, Waiting) => {
            "nothing to do: Downova tries again automatically"
        }
        (NextAction::Retry, Request) => "try again in a moment",
        (NextAction::Retry, Failed) => {
            "check the connection, then add the link again (it starts over)"
        }
        (NextAction::Wait, Request) => "wait a while, then try again",
        (NextAction::Wait, Failed) => "wait a while, then add the link again (it starts over)",
        (NextAction::RefreshLink, _) => "get a new link from the website and add it",
        (NextAction::SignIn, _) => {
            "Downova cannot sign in or use browser cookies yet; download this file in your browser"
        }
        (NextAction::FreeDiskSpace, Waiting) => {
            "free space on the destination drive; the download continues by itself"
        }
        (NextAction::FreeDiskSpace, Request) => "free space on the destination drive",
        (NextAction::FreeDiskSpace, Failed) => {
            "free space on the destination drive, then add the link again (it starts over)"
        }
        (NextAction::ReconnectVolume, Waiting) => {
            "reconnect the drive or restore the folder; the download continues by itself"
        }
        (NextAction::ReconnectVolume, Request) => "reconnect the drive or restore the folder",
        (NextAction::ReconnectVolume, Failed) => {
            "reconnect the drive, then add the link again (it starts over)"
        }
        (NextAction::InstallDesktopApp, _) => "install the Downova desktop app",
        (NextAction::ChooseRestartOrKeep, Failed) => {
            "add the link again to download the file from the start"
        }
        (NextAction::ChooseRestartOrKeep, _) => {
            "the saved part cannot be continued safely; keep the download paused, or cancel it and add the link again to start over"
        }
        (NextAction::ContactSupport, _) => {
            "please report the problem to the Downova developers, with the job ID if there is one"
        }
    })
}

/// Human-readable block for one download.
pub fn snapshot_text(s: &DownloadSnapshot) -> String {
    let mut out = String::new();
    let mut line = |label: &str, value: String| {
        out.push_str(&format!("{label:<10} {}\n", sanitize(&value)));
    };
    line("Job", s.job_id.to_string());
    line("Name", s.display_name.clone());
    line("Source", s.source_display.clone());
    line(
        "State",
        format!("{} (phase: {})", wire(&s.desired_state), wire(&s.phase)),
    );
    line("Progress", progress(s));
    line("Blockers", blockers(&s.blockers));
    if s.removed {
        line(
            "Removed",
            "yes: not in the list; the file was kept".to_owned(),
        );
    }
    if let Some(a) = &s.attempt_id {
        line("Attempt", a.to_string());
    }
    if let Some(e) = &s.error {
        line(
            "Error",
            format!(
                "{}{}",
                wire(&e.kind),
                e.detail
                    .as_deref()
                    .map(|d| format!(": {d}"))
                    .unwrap_or_default()
            ),
        );
        line("Meaning", explain(e.kind).to_owned());
        if let Some(h) = hint(e.next_action, situation(s)) {
            line("Next", h.to_owned());
        }
    }
    out
}

/// Human form of a verified `shutdown`.
pub fn shutdown_text(data: &Value) -> String {
    let paused = data["paused_jobs"].as_u64().unwrap_or(0);
    if data["mode"] == "pause_downloads" {
        format!("Service stopped. {paused} download(s) paused; they stay paused until resumed.\n")
    } else {
        "Service stopped. Unfinished downloads continue after the next start.\n".to_owned()
    }
}

/// Human-readable table for `list`.
/// One line per queue: ID, state, limit and name (names are sanitized like file names).
pub fn queues_text(queues: &[dm_domain::api::QueueInfo]) -> String {
    let mut out = String::new();
    for q in queues {
        out.push_str(&format!(
            "{:<20} {:<8} max {:<3} {}\n",
            q.queue_id,
            if q.running { "running" } else { "stopped" },
            q.max_active,
            sanitize(&q.name)
        ));
    }
    out
}

pub fn list_text(items: &[DownloadSnapshot]) -> String {
    if items.is_empty() {
        return "No downloads.\n".into();
    }
    let mut out = format!(
        "{:<22} {:<10} {:<13} {:<30} {}\n",
        "JOB ID", "STATE", "PHASE", "PROGRESS", "NAME"
    );
    for s in items {
        out.push_str(&sanitize(&format!(
            "{:<22} {:<10} {:<13} {:<30} {}\n",
            s.job_id,
            wire(&s.desired_state),
            wire(&s.phase),
            progress(s),
            s.display_name
        )));
    }
    out
}

/// JSON success document.
pub fn success_json(command: &str, mut data: Value, idempotency_key: Option<&str>) -> String {
    sanitize_value(&mut data);
    let mut doc = json!({ "ok": true, "command": command, "data": data });
    if let Some(key) = idempotency_key {
        doc["idempotency_key"] = json!(key);
    }
    doc.to_string()
}

/// A failure ready for presentation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub command: Option<&'static str>,
    pub exit_code: u8,
    pub data: ErrorData,
    pub rpc_code: Option<i64>,
}

pub fn error_json(f: &Failure) -> String {
    json!({
        "ok": false,
        "command": f.command,
        "exit_code": f.exit_code,
        "error": {
            "kind": f.data.kind,
            "rpc_code": f.rpc_code,
            "retryable": f.data.retryable,
            "next_action": f.data.next_action,
            "detail": f.data.detail.as_deref().map(sanitize),
        }
    })
    .to_string()
}

pub fn error_text(f: &Failure) -> String {
    let mut out = format!("dm: error: {}", wire(&f.data.kind));
    if let Some(detail) = &f.data.detail {
        out.push_str(&format!(": {}", sanitize(detail)));
    }
    out.push('\n');
    out.push_str(&format!("meaning: {}\n", explain(f.data.kind)));
    if let Some(h) = hint(f.data.next_action, Situation::Request) {
        out.push_str(&format!("hint: {h}\n"));
    }
    out
}

/// Human form of `dm hello`.
pub fn hello_text(h: &dm_domain::api::HelloResult) -> String {
    let mut text = format!(
        "Service    dm-service {} (API v{})\nBackend    {} {}\n",
        h.service_version,
        h.api_version,
        h.backend.id,
        h.backend.version.as_deref().unwrap_or("(version unknown)")
    );
    for c in &h.backend.capabilities {
        text.push_str(&format!(
            "  {:<24} {:?}\n",
            format!("{:?}", c.capability),
            c.support
        ));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_redacts_urls_in_free_text() {
        let text = "failed for https://u:p@h/a.zip?token=SYNTHETIC-SECRET now";
        let out = sanitize(text);
        assert!(
            !out.contains("SYNTHETIC-SECRET") && !out.contains("u:p@"),
            "{out}"
        );
        assert!(out.ends_with(" now"));
        assert_eq!(sanitize("no urls here"), "no urls here");
    }

    const SITUATIONS: [Situation; 3] = [Situation::Request, Situation::Waiting, Situation::Failed];

    /// Wording for features that do not exist yet; no explanation or hint may offer them.
    const NOT_OFFERED: [&str; 4] = [
        "diagnostic",
        "export",
        "log in to downova",
        "sign in to downova",
    ];

    fn snapshot(json: &str) -> DownloadSnapshot {
        let doc: Value = serde_json::from_str(json).unwrap();
        serde_json::from_value(doc["value"].clone()).unwrap()
    }

    #[test]
    fn every_kind_and_action_has_text_that_offers_only_real_actions() {
        for kind in ErrorKind::ALL {
            let text = explain(kind);
            assert!(text.len() > 20, "{kind:?}");
            assert!(NOT_OFFERED.iter().all(|w| !text.contains(w)), "{kind:?}");
        }
        for action in NextAction::ALL {
            for situation in SITUATIONS {
                let hint = hint(action, situation);
                assert_eq!(hint.is_none(), action == NextAction::None, "{action:?}");
                let Some(hint) = hint else { continue };
                let lower = hint.to_lowercase();
                assert!(NOT_OFFERED.iter().all(|w| !lower.contains(w)), "{hint}");
                if situation == Situation::Failed {
                    // A failed download cannot be resumed; only a new download starts it again.
                    assert!(!lower.contains("resume"), "{hint}");
                    assert!(!lower.contains("by itself"), "{hint}");
                }
            }
        }
        assert!(
            hint(NextAction::SignIn, Situation::Failed)
                .unwrap()
                .contains("cannot sign in"),
            "authenticated downloads are not promised"
        );
    }

    #[test]
    fn hints_follow_the_situation() {
        for action in [NextAction::FreeDiskSpace, NextAction::ReconnectVolume] {
            assert!(
                hint(action, Situation::Waiting)
                    .unwrap()
                    .contains("by itself")
            );
            assert!(
                hint(action, Situation::Failed)
                    .unwrap()
                    .contains("add the link again")
            );
        }
        for action in [NextAction::Retry, NextAction::Wait] {
            assert!(
                hint(action, Situation::Waiting)
                    .unwrap()
                    .contains("automatically")
            );
            assert!(
                hint(action, Situation::Failed)
                    .unwrap()
                    .contains("add the link again")
            );
        }
        let paused = hint(NextAction::ChooseRestartOrKeep, Situation::Request).unwrap();
        assert!(paused.contains("paused") && paused.contains("cancel"));
    }

    #[test]
    fn a_failed_download_explains_itself_from_its_kind() {
        let mut s = snapshot(include_str!(
            "../../../packages/contracts/examples/download-snapshot.failed.json"
        ));
        let text = snapshot_text(&s);
        assert!(text.contains("Error      REMOTE_CHANGED: The remote file changed"));
        assert!(text.contains(&format!("Meaning    {}", explain(ErrorKind::RemoteChanged))));
        assert!(text.contains("Next       add the link again to download the file from the start"));
        // Still alive and waiting for space: nothing to re-add.
        s.phase = Phase::Queued;
        s.blockers = vec![Blocker::DiskUnavailable];
        s.error = Some(ErrorData {
            kind: ErrorKind::DiskFull,
            retryable: true,
            next_action: NextAction::FreeDiskSpace,
            detail: None,
        });
        assert!(snapshot_text(&s).contains("the download continues by itself"));
    }

    #[test]
    fn error_text_explains_and_never_shows_secrets() {
        for kind in ErrorKind::ALL {
            let f = Failure {
                command: Some("add"),
                exit_code: 4,
                data: kind.to_error_data(
                    "refused https://u:p@h/a.zip?token=SYNTHETIC-SECRET-b17 by the server",
                ),
                rpc_code: None,
            };
            let text = error_text(&f);
            assert!(
                !text.contains("SYNTHETIC-SECRET") && !text.contains("u:p@"),
                "{text}"
            );
            assert!(
                text.contains(&format!("meaning: {}", explain(kind))),
                "{text}"
            );
            let json = error_json(&f);
            assert!(!json.contains("SYNTHETIC-SECRET"), "{json}");
            assert!(
                !json.contains("meaning"),
                "the JSON shape is unchanged: {json}"
            );
        }
    }

    #[test]
    fn sizes_are_binary_and_truncated() {
        assert_eq!(bytes(ByteCount(0)), "0 B");
        assert_eq!(bytes(ByteCount(1536)), "1.5 KiB");
        assert_eq!(bytes(ByteCount(u64::MAX)), "15.9 EiB");
    }
}
