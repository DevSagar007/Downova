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
use dm_domain::{Blocker, ByteCount, ErrorData, NextAction};
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

/// Plain-language next step for an error.
pub fn hint(action: NextAction) -> Option<&'static str> {
    Some(match action {
        NextAction::None => return None,
        NextAction::Retry => "try again later",
        NextAction::Wait => "wait, then try again",
        NextAction::RefreshLink => "get a fresh link from the website and add it again",
        NextAction::SignIn => "sign in on the website, then add the link again",
        NextAction::FreeDiskSpace => "free disk space, then resume",
        NextAction::ReconnectVolume => "reconnect the drive, then resume",
        NextAction::InstallDesktopApp => "install the Downova desktop app",
        NextAction::ChooseRestartOrKeep => {
            "partial data cannot be validated; cancel and add the link again to restart"
        }
        NextAction::ContactSupport => "export diagnostics and report the problem",
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
        if let Some(h) = hint(e.next_action) {
            line("Next", h.to_owned());
        }
    }
    out
}

/// Human-readable table for `list`.
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
    if let Some(h) = hint(f.data.next_action) {
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

    #[test]
    fn sizes_are_binary_and_truncated() {
        assert_eq!(bytes(ByteCount(0)), "0 B");
        assert_eq!(bytes(ByteCount(1536)), "1.5 KiB");
        assert_eq!(bytes(ByteCount(u64::MAX)), "15.9 EiB");
    }
}
