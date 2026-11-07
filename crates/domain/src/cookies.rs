//! Cookie applicability for a scoped browser session (BRW-05, ADR 0027), RFC 6265 §5.1.3–5.1.4 and
//! §5.4: domain match (host-only versus domain cookies), path match, `Secure` only on https, and
//! expiry at the moment of the request. Pure: no I/O, values stay [`Secret`].

use crate::redact::Secret;

/// One cookie of a session, with its scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopedCookie {
    pub name: String,
    pub value: Secret,
    /// Lower-case, without a leading dot.
    pub domain: String,
    pub host_only: bool,
    pub path: String,
    pub secure: bool,
    pub expires_at_ms: Option<u64>,
}

/// A session bound to exactly one origin (`scheme://host[:port]`, effective port included).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionScope {
    pub origin: Origin,
    pub cookies: Vec<ScopedCookie>,
}

/// Scheme, host and effective port.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Origin {
    pub scheme: String,
    pub host: String,
    pub port: u16,
}

impl Origin {
    /// Parses `scheme://host[:port]` (or a full URL, whose path is ignored). `None` for anything
    /// but http(s) with a host; userinfo is refused.
    pub fn parse(url: &str) -> Option<Self> {
        let (scheme, rest) = url.split_once("://")?;
        let scheme = scheme.to_ascii_lowercase();
        let default = match scheme.as_str() {
            "https" => 443,
            "http" => 80,
            _ => return None,
        };
        let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
        let authority = rest.get(..end)?;
        if authority.is_empty() || authority.contains('@') {
            return None;
        }
        let (host, port) = match authority.rsplit_once(':') {
            Some((h, p)) if !h.ends_with(']') || authority.starts_with('[') => match p.parse() {
                Ok(p) if !h.is_empty() && !(h.starts_with('[') && !h.ends_with(']')) => (h, p),
                _ if authority.starts_with('[') && authority.ends_with(']') => (authority, default),
                _ => return None,
            },
            _ => (authority, default),
        };
        Some(Self {
            scheme,
            host: host.to_ascii_lowercase(),
            port,
        })
    }

    pub fn display(&self) -> String {
        let default = if self.scheme == "https" { 443 } else { 80 };
        if self.port == default {
            format!("{}://{}", self.scheme, self.host)
        } else {
            format!("{}://{}:{}", self.scheme, self.host, self.port)
        }
    }
}

/// RFC 6265 §5.1.3.
fn domain_matches(host: &str, cookie: &ScopedCookie) -> bool {
    if cookie.host_only {
        return host == cookie.domain;
    }
    host == cookie.domain
        || (host.ends_with(&cookie.domain)
            && host
                .len()
                .checked_sub(cookie.domain.len())
                .and_then(|i| i.checked_sub(1))
                .and_then(|i| host.as_bytes().get(i))
                == Some(&b'.')
            && host.parse::<std::net::IpAddr>().is_err())
}

/// RFC 6265 §5.1.4.
fn path_matches(request_path: &str, cookie_path: &str) -> bool {
    request_path == cookie_path
        || (request_path.starts_with(cookie_path)
            && (cookie_path.ends_with('/')
                || request_path.as_bytes().get(cookie_path.len()) == Some(&b'/')))
}

/// The request path of a URL (`/` when empty), without query or fragment.
fn request_path(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let rest = rest.find('/').map_or("", |i| rest.get(i..).unwrap_or(""));
    let end = rest.find(['?', '#']).unwrap_or(rest.len());
    match rest.get(..end) {
        Some(p) if !p.is_empty() => p,
        _ => "/",
    }
}

impl SessionScope {
    /// The `Cookie` header for a request to `url` at `now_ms`, or `None` when no cookie applies.
    /// A URL outside the session's origin never gets one (callers stop such requests first).
    pub fn cookie_header(&self, url: &str, now_ms: u64) -> Option<Secret> {
        let target = Origin::parse(url)?;
        if target != self.origin {
            return None;
        }
        let path = request_path(url);
        let https = target.scheme == "https";
        let mut chosen: Vec<&ScopedCookie> = self
            .cookies
            .iter()
            .filter(|c| domain_matches(&target.host, c))
            .filter(|c| path_matches(path, &c.path))
            .filter(|c| !c.secure || https)
            .filter(|c| c.expires_at_ms.is_none_or(|e| e > now_ms))
            .collect();
        if chosen.is_empty() {
            return None;
        }
        // RFC 6265 §5.4: longer paths first.
        chosen.sort_by_key(|c| std::cmp::Reverse(c.path.len()));
        let header = chosen
            .iter()
            .map(|c| format!("{}={}", c.name, c.value.expose()))
            .collect::<Vec<_>>()
            .join("; ");
        Some(Secret::new(header))
    }

    /// Whether every cookie has expired at `now_ms` (the session can no longer authenticate).
    pub fn expired(&self, now_ms: u64) -> bool {
        self.cookies
            .iter()
            .all(|c| c.expires_at_ms.is_some_and(|e| e <= now_ms))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(
        name: &str,
        domain: &str,
        host_only: bool,
        path: &str,
        secure: bool,
        exp: Option<u64>,
    ) -> ScopedCookie {
        ScopedCookie {
            name: name.into(),
            value: Secret::new(format!("{name}-SYNTH")),
            domain: domain.into(),
            host_only,
            path: path.into(),
            secure,
            expires_at_ms: exp,
        }
    }

    fn scope() -> SessionScope {
        SessionScope {
            origin: Origin::parse("https://files.example.org").unwrap(),
            cookies: vec![
                c("root", "files.example.org", true, "/", true, None),
                c("reports", "files.example.org", true, "/reports", true, None),
                c("wide", "example.org", false, "/", true, None),
                c("old", "files.example.org", true, "/", true, Some(10)),
                c("other", "other.example.org", true, "/", true, None),
                c("plain", "files.example.org", true, "/", false, None),
            ],
        }
    }

    fn header(url: &str) -> Option<String> {
        scope()
            .cookie_header(url, 100)
            .map(|s| s.expose().to_owned())
    }

    #[test]
    fn domain_path_secure_and_expiry_rules() {
        assert_eq!(
            header("https://files.example.org/reports/q3.pdf?x=1").unwrap(),
            "reports=reports-SYNTH; root=root-SYNTH; wide=wide-SYNTH; plain=plain-SYNTH"
        );
        // A same-origin redirect to another path: the /reports cookie no longer applies.
        assert_eq!(
            header("https://files.example.org/download/q3.pdf").unwrap(),
            "root=root-SYNTH; wide=wide-SYNTH; plain=plain-SYNTH"
        );
        // "/reportsX" does not path-match "/reports".
        assert!(
            !header("https://files.example.org/reportsX")
                .unwrap()
                .contains("reports=")
        );
    }

    #[test]
    fn another_origin_gets_nothing() {
        for url in [
            "http://files.example.org/reports/q3.pdf",
            "https://files.example.org:8443/reports/q3.pdf",
            "https://cdn.example.org/q3.pdf",
            "https://other.example.org/",
            "https://evilfiles.example.org/",
        ] {
            assert_eq!(header(url), None, "{url}");
        }
    }

    #[test]
    fn origins_include_the_effective_port() {
        assert_eq!(
            Origin::parse("https://H.example:443/x"),
            Origin::parse("https://h.example")
        );
        assert_ne!(
            Origin::parse("https://h.example:444"),
            Origin::parse("https://h.example")
        );
        assert_ne!(
            Origin::parse("http://h.example"),
            Origin::parse("https://h.example")
        );
        assert_eq!(Origin::parse("https://u:p@h.example/"), None);
        assert_eq!(Origin::parse("ftp://h.example/"), None);
        assert_eq!(Origin::parse("https://[::1]:8443/x").unwrap().port, 8443);
        assert_eq!(
            Origin::parse("https://h.example:8443").unwrap().display(),
            "https://h.example:8443"
        );
    }

    #[test]
    fn expiry_is_judged_at_request_time() {
        let s = scope();
        assert!(!s.expired(100));
        let all_old = SessionScope {
            origin: s.origin.clone(),
            cookies: vec![c("a", "files.example.org", true, "/", true, Some(5))],
        };
        assert!(all_old.expired(6));
        assert_eq!(all_old.cookie_header("https://files.example.org/", 6), None);
    }

    #[test]
    fn debug_never_shows_values() {
        assert!(!format!("{:?}", scope()).contains("SYNTH"));
    }
}
