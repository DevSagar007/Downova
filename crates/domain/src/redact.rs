//! Redaction of secrets before anything reaches logs, events, exports or the UI.
//!
//! Rules (research §10, §17.6): URL userinfo, query values and fragments are removed; cookie,
//! authorization and proxy-authorization headers are never written out. [`Secret`] makes the
//! boundary explicit in types: it cannot be serialized and its `Debug` output is redacted.

use std::fmt;

/// Placeholder written in place of removed secret material.
pub const REDACTED: &str = "REDACTED";

/// Returns a display-safe form of a URL: no userinfo, every query *value* replaced with
/// [`REDACTED`] (keys are kept for diagnosis), and no fragment.
///
/// Works on any string; inputs that are not `scheme://` URLs are fully redacted.
pub fn redact_url(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else {
        return REDACTED.to_owned();
    };
    let rest = rest.split_once('#').map_or(rest, |(before, _)| before);
    let (authority_and_path, query) = match rest.split_once('?') {
        Some((before, query)) => (before, Some(query)),
        None => (rest, None),
    };
    let (authority, path) = match authority_and_path.find('/') {
        Some(idx) => authority_and_path.split_at(idx),
        None => (authority_and_path, ""),
    };
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);

    let mut out = format!("{scheme}://{host}{path}");
    if let Some(query) = query {
        let redacted: Vec<String> = query
            .split('&')
            .filter(|pair| !pair.is_empty())
            .map(|pair| {
                let key = pair.split_once('=').map_or(pair, |(key, _)| key);
                format!("{key}={REDACTED}")
            })
            .collect();
        if !redacted.is_empty() {
            out.push('?');
            out.push_str(&redacted.join("&"));
        }
    }
    out
}

/// Headers whose values are credentials and must never be logged or exported.
pub fn is_sensitive_header(name: &str) -> bool {
    const SENSITIVE: [&str; 6] = [
        "authorization",
        "proxy-authorization",
        "cookie",
        "set-cookie",
        "x-api-key",
        "x-auth-token",
    ];
    SENSITIVE.iter().any(|h| h.eq_ignore_ascii_case(name))
}

/// A secret value (cookie, token, password). Deliberately implements neither `Serialize` nor
/// `Display`; `Debug` prints a placeholder. Use [`Secret::expose`] only at the point of use.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    /// Wraps a secret value.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Returns the raw value. Callers must not log or persist it outside the credential store.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Secret({REDACTED})")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SYNTHETIC: &str = "SYNTHETIC-SECRET-7f3a";

    #[test]
    fn removes_userinfo_query_values_and_fragment() {
        let url = format!(
            "https://user:{SYNTHETIC}@cdn.example.org/a/b.zip?token={SYNTHETIC}&x=1#{SYNTHETIC}"
        );
        let safe = redact_url(&url);
        assert_eq!(
            safe,
            "https://cdn.example.org/a/b.zip?token=REDACTED&x=REDACTED"
        );
        assert!(!safe.contains(SYNTHETIC));
    }

    #[test]
    fn keeps_plain_urls_readable() {
        assert_eq!(
            redact_url("https://example.org/file.zip"),
            "https://example.org/file.zip"
        );
        assert_eq!(redact_url("https://example.org"), "https://example.org");
    }

    #[test]
    fn non_urls_are_fully_redacted() {
        assert_eq!(redact_url(SYNTHETIC), REDACTED);
    }

    #[test]
    fn secret_debug_never_leaks() {
        let secret = Secret::new(SYNTHETIC);
        assert!(!format!("{secret:?}").contains(SYNTHETIC));
        assert_eq!(secret.expose(), SYNTHETIC);
    }

    #[test]
    fn credential_headers_are_sensitive() {
        assert!(is_sensitive_header("Cookie"));
        assert!(is_sensitive_header("AUTHORIZATION"));
        assert!(!is_sensitive_header("content-range"));
    }
}
