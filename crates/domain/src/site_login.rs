//! Site logins (F05, ADR 0036): one user name and password per FTP server, used for every
//! download from it. Mirrors `SiteLogin`, `SetSiteLoginParams`, `RemoveSiteLoginParams` and
//! `ListSiteLoginsResult` in the canonical schema; the rules are checked again here because ingress
//! is never trusted. The password is a [`SecretTransit`] on the way in and never appears in a
//! response.

use serde::{Deserialize, Serialize};

use crate::proxy::{MAX_PROXY_PASSWORD, valid_proxy_host, valid_proxy_username};
use crate::redact::SecretTransit;

/// The most site logins kept.
pub const MAX_SITE_LOGINS: usize = 100;
/// FTP's default control port.
pub const FTP_PORT: u16 = 21;

/// The protocols a site login applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SiteLoginScheme {
    Ftp,
    /// SFTP (ADR 0037): the login is required, host keys are verified separately.
    Sftp,
}

/// Whether the saved password can be read now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SiteLoginPasswordState {
    Saved,
    /// The OS credential store cannot give it now; downloads from this server wait.
    Unavailable,
}

/// One site login, display-safe (no password).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SiteLogin {
    pub login_id: String,
    pub scheme: SiteLoginScheme,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password_state: SiteLoginPasswordState,
}

/// `site_logins.v1.list` and the result of `set` / `remove`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListSiteLoginsResult {
    pub logins: Vec<SiteLogin>,
}

/// `site_logins.v1.set`: adds or replaces the login for one server. `password` absent keeps the
/// saved one (only for an existing login).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetSiteLoginParams {
    pub scheme: SiteLoginScheme,
    pub host: String,
    pub port: u16,
    pub username: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<SecretTransit>,
}

/// `site_logins.v1.remove`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoveSiteLoginParams {
    pub login_id: String,
}

/// `login_` + 16 hex of FNV-1a 64 over `scheme|lower-case host|port`: one stable id per server.
/// Not a secret and not a security boundary (the list holds at most [`MAX_SITE_LOGINS`]).
pub fn site_login_id(scheme: SiteLoginScheme, host: &str, port: u16) -> String {
    let scheme = match scheme {
        SiteLoginScheme::Ftp => "ftp",
        SiteLoginScheme::Sftp => "sftp",
    };
    let key = format!("{scheme}|{}|{port}", host.to_ascii_lowercase());
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in key.bytes() {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("login_{hash:016x}")
}

/// `login_` + 16 lower-case hex digits.
pub fn valid_site_login_id(id: &str) -> bool {
    id.strip_prefix("login_").is_some_and(|h| {
        h.len() == 16
            && h.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}

impl SetSiteLoginParams {
    /// The schema's rules, checked again.
    pub fn validate(&self) -> Result<(), &'static str> {
        if !valid_proxy_host(&self.host) {
            return Err("the server must be a host name, an IPv4 address or [IPv6]");
        }
        if self.port == 0 {
            return Err("the port must be from 1 to 65535");
        }
        if !valid_proxy_username(&self.username) {
            return Err("the user name must be 1 to 128 characters without ':'");
        }
        if let Some(p) = &self.password
            && (p.is_empty() || p.char_len() > MAX_PROXY_PASSWORD || !p.has_no_controls())
        {
            return Err("the password must be 1 to 256 characters");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn one_id_per_server_whatever_the_case_of_the_host() {
        let a = site_login_id(SiteLoginScheme::Ftp, "Files.Example.org", 21);
        assert_eq!(
            a,
            site_login_id(SiteLoginScheme::Ftp, "files.example.org", 21)
        );
        assert_ne!(
            a,
            site_login_id(SiteLoginScheme::Ftp, "files.example.org", 2121)
        );
        assert!(valid_site_login_id(&a));
        assert!(!valid_site_login_id("login_XYZ"));
        assert!(!valid_site_login_id("cred_0123456789abcdef"));
    }

    #[test]
    fn params_follow_the_schema_and_never_show_the_password() {
        let p: SetSiteLoginParams = serde_json::from_str(
            r#"{"scheme":"ftp","host":"files.example.org","port":21,"username":"alice","password":"SYNTHETIC-pw"}"#,
        )
        .unwrap();
        assert!(p.validate().is_ok());
        assert!(!format!("{p:?}").contains("SYNTHETIC-pw"));
        let keep: SetSiteLoginParams = serde_json::from_str(
            r#"{"scheme":"ftp","host":"files.example.org","port":21,"username":"alice"}"#,
        )
        .unwrap();
        assert!(keep.password.is_none());
        for bad in [
            r#"{"scheme":"ftp","host":"ftp://x","port":21,"username":"a","password":"p"}"#,
            r#"{"scheme":"ftp","host":"x","port":0,"username":"a","password":"p"}"#,
            r#"{"scheme":"ftp","host":"x","port":21,"username":"a:b","password":"p"}"#,
            r#"{"scheme":"ftp","host":"x","port":21,"username":"a","password":"p\nq"}"#,
        ] {
            let p: SetSiteLoginParams = serde_json::from_str(bad).unwrap();
            assert!(p.validate().is_err(), "{bad}");
        }
        assert!(
            serde_json::from_str::<SetSiteLoginParams>(
                r#"{"scheme":"scp","host":"x","port":22,"username":"a"}"#
            )
            .is_err()
        );
        assert_ne!(
            site_login_id(SiteLoginScheme::Ftp, "x", 22),
            site_login_id(SiteLoginScheme::Sftp, "x", 22)
        );
    }
}
