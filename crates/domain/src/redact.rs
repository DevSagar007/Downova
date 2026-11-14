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
/// The origin given to the OS as a download's source for origin marking (F07, ADR 0026):
/// `scheme://host[:port]/` only. Userinfo, path, query and fragment are dropped, so signed-URL
/// tokens (in the query or the path), credentials and fragments never reach a `Zone.Identifier`
/// stream, a log or a diagnostics file. `None` for anything but http(s) with a host.
pub fn marking_origin(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return None;
    }
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = rest.get(..end)?;
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host)
        .to_ascii_lowercase();
    let valid = !host.is_empty()
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':' | '[' | ']' | '_'));
    valid.then(|| format!("{scheme}://{host}/"))
}

/// Defence in depth for anything exported (E05, ADR 0026): the names of the secret or identity
/// patterns found in `text`. Empty means none. Checked on the final serialized text, after the
/// collection already admitted only allowlisted fields.
pub fn export_leaks(text: &str) -> Vec<&'static str> {
    let lower = text.to_ascii_lowercase();
    let mut found = Vec::new();
    for (name, needle) in [
        // Header and parameter forms, not bare words: "cookies" in prose is not a secret.
        ("authorization", "authorization:"),
        ("authorization", "authorization="),
        ("cookie", "cookie:"),
        ("cookie", "cookie="),
        ("cookie", "set-cookie"),
        ("bearer token", "bearer "),
        ("token parameter", "token="),
        ("password", "password="),
        ("password", "password:"),
        ("signature parameter", "sig="),
        ("windows home path", "c:\\users\\"),
        ("windows home path", "c:\\\\users\\\\"),
        ("unix home path", "/home/"),
        ("macos home path", "/users/"),
    ] {
        if lower.contains(needle) && !found.contains(&name) {
            found.push(name);
        }
    }
    // A URL that carries userinfo, a query or a fragment.
    for scheme in ["http://", "https://"] {
        let mut rest = lower.as_str();
        while let Some(i) = rest.find(scheme) {
            let tail = rest
                .get(i.saturating_add(scheme.len())..)
                .unwrap_or_default();
            let end = tail
                .find(|c: char| c.is_whitespace() || c == '"' || c == '\\')
                .unwrap_or(tail.len());
            let url = tail.get(..end).unwrap_or_default();
            let authority = url.split(['/', '?', '#']).next().unwrap_or_default();
            if authority.contains('@') && !found.contains(&"url userinfo") {
                found.push("url userinfo");
            }
            if (url.contains('?') || url.contains('#')) && !found.contains(&"url query or fragment")
            {
                found.push("url query or fragment");
            }
            rest = tail.get(end..).unwrap_or_default();
        }
    }
    found
}

/// A secret in transit across the trusted ingress hops only (extension → native host → service,
/// ADR 0027). Unlike [`Secret`] it can be (de)serialized, because those hops are JSON; its `Debug`
/// is redacted, and the service turns it into a [`Secret`] at once. Never used in responses,
/// snapshots, events or storage.
#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct SecretTransit(String);

impl SecretTransit {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
    pub fn into_secret(self) -> Secret {
        Secret::new(self.0)
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    /// Length in characters (the schema counts characters, not bytes).
    pub fn char_len(&self) -> usize {
        self.0.chars().count()
    }
    /// True when the value holds no control characters (checked without exposing it).
    pub fn has_no_controls(&self) -> bool {
        !self.0.chars().any(char::is_control)
    }
}

impl fmt::Debug for SecretTransit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SecretTransit({REDACTED})")
    }
}

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
    fn the_marking_origin_keeps_only_scheme_host_and_port() {
        assert_eq!(
            marking_origin(&format!(
                "HTTPS://user:{SYNTHETIC}@CDN.Example.org:8443/{SYNTHETIC}/b.zip?token={SYNTHETIC}#{SYNTHETIC}"
            ))
            .as_deref(),
            Some("https://cdn.example.org:8443/")
        );
        assert_eq!(
            marking_origin("http://[::1]:80/x").as_deref(),
            Some("http://[::1]:80/")
        );
        for bad in [
            "ftp://h/x",
            "file:///c:/x",
            "https://",
            "not a url",
            "https://h o/",
        ] {
            assert_eq!(marking_origin(bad), None, "{bad}");
        }
    }

    #[test]
    fn export_leaks_finds_secrets_and_personal_paths() {
        for (text, what) in [
            ("Authorization: x", "authorization"),
            ("Cookie: sid=1", "cookie"),
            ("cookie=abc", "cookie"),
            ("Bearer abc", "bearer token"),
            ("x?token=abc", "token parameter"),
            ("proxy password=x", "password"),
            ("\"C:\\\\Users\\\\me\\\\x\"", "windows home path"),
            ("/home/me/x", "unix home path"),
            ("/Users/me/x", "macos home path"),
            ("see https://u:p@h/x", "url userinfo"),
            ("see https://h/x?a=1", "url query or fragment"),
            ("see https://h/x#frag", "url query or fragment"),
        ] {
            assert!(
                export_leaks(text).contains(&what),
                "{text} → {:?}",
                export_leaks(text)
            );
        }
        assert!(
            export_leaks("{\"backend\":\"http\",\"origin\":\"https://cdn.example.org/\"}")
                .is_empty()
        );
        assert!(export_leaks("No cookies, headers or passwords are included.").is_empty());
    }

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
