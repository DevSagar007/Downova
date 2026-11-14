//! F06 proxy settings on the wire (ADR 0034): `proxy.v1.get` / `proxy.v1.set`.
//!
//! Mirrors `ProxySettings` and `SetProxyParams` in the canonical schema. The rules are checked
//! again here because ingress is never trusted. The password is a [`SecretTransit`] on the way in
//! and never appears in a response.

use serde::{Deserialize, Deserializer, Serialize};

use crate::redact::SecretTransit;

/// `direct` (the default) or one HTTP proxy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProxyMode {
    #[default]
    Direct,
    Http,
}

/// Whether a proxy password is saved, and whether it can be read now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProxyPasswordState {
    #[default]
    None,
    Saved,
    /// Saved, but the OS credential store cannot be read now; transfers wait instead of
    /// connecting without it.
    Unavailable,
}

/// A download protocol carried through the proxy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProxyCarries {
    Http,
    Https,
}

/// `proxy.v1.get` and `proxy.v1.set` result. Never carries the password.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProxySettings {
    pub mode: ProxyMode,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub username: Option<String>,
    pub password_state: ProxyPasswordState,
    pub carries: Vec<ProxyCarries>,
}

/// What `proxy.v1.set` does with the saved password.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum PasswordChange {
    /// The field was absent.
    #[default]
    Keep,
    /// `null`.
    Remove,
    /// A new password.
    Replace(SecretTransit),
}

impl Serialize for PasswordChange {
    // Only `Remove` and `Replace` are serialized; `Keep` is skipped by the field attribute.
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Keep | Self::Remove => s.serialize_none(),
            Self::Replace(p) => p.serialize(s),
        }
    }
}

impl<'de> Deserialize<'de> for PasswordChange {
    // Called only when the field is present: `null` removes, a string replaces.
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(match Option::<SecretTransit>::deserialize(d)? {
            None => Self::Remove,
            Some(p) => Self::Replace(p),
        })
    }
}

impl PasswordChange {
    fn is_keep(&self) -> bool {
        matches!(self, Self::Keep)
    }
}

/// `proxy.v1.set` params.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetProxyParams {
    pub mode: ProxyMode,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub username: Option<String>,
    #[serde(default, skip_serializing_if = "PasswordChange::is_keep")]
    pub password: PasswordChange,
}

/// Longest proxy password accepted (fits every supported credential store).
pub const MAX_PROXY_PASSWORD: usize = 256;

fn no_controls(s: &str) -> bool {
    !s.chars().any(|c| c.is_control())
}

/// A host name (dot-separated labels of letters, digits and inner hyphens), an IPv4 address, or a
/// bracketed IPv6 address. No scheme, user info, port or path.
pub fn valid_proxy_host(host: &str) -> bool {
    if host.is_empty() || host.len() > 253 {
        return false;
    }
    if let Some(inner) = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')) {
        return !inner.is_empty() && inner.parse::<std::net::Ipv6Addr>().is_ok();
    }
    host.split('.').all(|label| {
        let b = label.as_bytes();
        !b.is_empty()
            && b.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'-')
            && b.first() != Some(&b'-')
            && b.last() != Some(&b'-')
    })
}

pub fn valid_proxy_username(name: &str) -> bool {
    (1..=128).contains(&name.chars().count()) && !name.contains(':') && no_controls(name)
}

impl SetProxyParams {
    /// The schema's rules plus the cross-field ones: http needs host and port, and a new password
    /// needs a user name.
    pub fn validate(&self) -> Result<(), &'static str> {
        if let Some(h) = &self.host
            && !valid_proxy_host(h)
        {
            return Err("the proxy host must be a host name, an IPv4 address or [IPv6]");
        }
        if self.port == Some(0) {
            return Err("the proxy port must be from 1 to 65535");
        }
        if let Some(u) = &self.username
            && !valid_proxy_username(u)
        {
            return Err("the proxy user name must be 1 to 128 characters without ':'");
        }
        if let PasswordChange::Replace(p) = &self.password {
            if p.is_empty() || p.char_len() > MAX_PROXY_PASSWORD || !p.has_no_controls() {
                return Err("the proxy password must be 1 to 256 characters");
            }
            if self.username.is_none() {
                return Err("a proxy password needs a user name");
            }
        }
        if self.mode == ProxyMode::Http && (self.host.is_none() || self.port.is_none()) {
            return Err("an HTTP proxy needs a host and a port");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn params(json: &str) -> SetProxyParams {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn password_absent_keeps_null_removes_string_replaces() {
        let base = r#""mode":"http","host":"p.test","port":3128,"username":"a""#;
        assert_eq!(
            params(&format!("{{{base}}}")).password,
            PasswordChange::Keep
        );
        assert_eq!(
            params(&format!("{{{base},\"password\":null}}")).password,
            PasswordChange::Remove
        );
        assert_eq!(
            params(&format!("{{{base},\"password\":\"x\"}}")).password,
            PasswordChange::Replace(SecretTransit::new("x"))
        );
    }

    #[test]
    fn the_password_never_shows_in_debug() {
        let p = params(
            r#"{"mode":"http","host":"p.test","port":1,"username":"a","password":"SYNTHETIC-pw"}"#,
        );
        assert!(!format!("{p:?}").contains("SYNTHETIC-pw"));
    }

    #[test]
    fn hosts_and_names_follow_the_schema() {
        for ok in [
            "proxy",
            "proxy.example.test",
            "10.0.0.8",
            "[2001:db8::1]",
            "a-b.c",
        ] {
            assert!(valid_proxy_host(ok), "{ok}");
        }
        for bad in [
            "", "http://p", "p:3128", "u@p", "p/x", "-p", "p-", "a..b", "[nothex]", "[]",
        ] {
            assert!(!valid_proxy_host(bad), "{bad}");
        }
        assert!(valid_proxy_username("alice"));
        assert!(!valid_proxy_username("a:b"));
        assert!(!valid_proxy_username("a\u{7}"));
    }

    #[test]
    fn cross_field_rules() {
        let v = |j: &str| params(j).validate();
        assert!(v(r#"{"mode":"direct","host":null,"port":null,"username":null}"#).is_ok());
        assert!(v(r#"{"mode":"http","host":"p","port":null,"username":null}"#).is_err());
        assert!(v(r#"{"mode":"http","host":"p","port":0,"username":null}"#).is_err());
        assert!(
            v(r#"{"mode":"http","host":"p","port":8080,"username":null,"password":"x"}"#).is_err()
        );
        assert!(
            v(r#"{"mode":"http","host":"p","port":8080,"username":"a","password":"x"}"#).is_ok()
        );
        assert!(
            v(r#"{"mode":"http","host":"p","port":8080,"username":"a","password":"x\ny"}"#)
                .is_err()
        );
    }
}
