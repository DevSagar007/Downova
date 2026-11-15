//! Known SFTP servers and their host keys (F05, ADR 0037). Mirrors `KnownHost`,
//! `ListKnownHostsResult`, `TrustHostKeyParams` and `RemoveKnownHostParams` in the canonical
//! schema. Host keys and fingerprints are public data.

use serde::{Deserialize, Serialize};

use crate::proxy::valid_proxy_host;

/// SSH's default port.
pub const SSH_PORT: u16 = 22;
/// The most known servers kept.
pub const MAX_KNOWN_HOSTS: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnownHostState {
    Trusted,
    Pending,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnownHost {
    pub host: String,
    pub port: u16,
    pub algorithm: String,
    pub fingerprint: String,
    pub state: KnownHostState,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListKnownHostsResult {
    pub hosts: Vec<KnownHost>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrustHostKeyParams {
    pub host: String,
    pub port: u16,
    pub fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoveKnownHostParams {
    pub host: String,
    pub port: u16,
}

/// `SHA256:` and 43 standard base64 characters (an unpadded 32-byte digest).
pub fn valid_fingerprint(fp: &str) -> bool {
    fp.strip_prefix("SHA256:").is_some_and(|b| {
        b.len() == 43
            && b.bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'+' || c == b'/')
    })
}

/// An SSH key algorithm name as SSH prints it (`ssh-ed25519`, `ecdsa-sha2-nistp256`, …).
pub fn valid_algorithm(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name.bytes().all(|c| {
            c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'@' | b'.' | b'-')
        })
}

/// Reads `server key <algorithm> <fingerprint>` out of a host-key error's detail (ADR 0037).
pub fn presented_key(detail: &str) -> Option<(String, String)> {
    let rest = &detail[detail
        .find("server key ")?
        .saturating_add("server key ".len())..];
    let mut parts = rest.split([' ', ')']);
    let algorithm = parts.next()?;
    let fingerprint = parts.next()?;
    (valid_algorithm(algorithm) && valid_fingerprint(fingerprint))
        .then(|| (algorithm.to_owned(), fingerprint.to_owned()))
}

impl TrustHostKeyParams {
    pub fn validate(&self) -> Result<(), &'static str> {
        if !valid_proxy_host(&self.host) {
            return Err("the server must be a host name, an IPv4 address or [IPv6]");
        }
        if self.port == 0 {
            return Err("the port must be from 1 to 65535");
        }
        if !valid_fingerprint(&self.fingerprint) {
            return Err("the fingerprint must be SHA256: and 43 base64 characters");
        }
        Ok(())
    }
}

impl RemoveKnownHostParams {
    pub fn validate(&self) -> Result<(), &'static str> {
        if !valid_proxy_host(&self.host) || self.port == 0 {
            return Err("invalid server");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FP: &str = "SHA256:47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU";

    #[test]
    fn the_presented_key_is_read_back_from_the_engine_message() {
        let detail = format!(
            "this SFTP server's key is not trusted yet; compare it (server key ssh-ed25519 {FP})"
        );
        assert_eq!(
            presented_key(&detail),
            Some(("ssh-ed25519".into(), FP.into()))
        );
        assert_eq!(presented_key("no key here"), None);
        assert_eq!(presented_key("server key ssh-ed25519 SHA256:short"), None);
    }

    #[test]
    fn fingerprints_and_algorithms_follow_the_schema() {
        assert!(valid_fingerprint(FP));
        assert!(!valid_fingerprint("MD5:aa:bb"));
        assert!(!valid_fingerprint("SHA256:abc"));
        assert!(valid_algorithm("ecdsa-sha2-nistp256"));
        assert!(!valid_algorithm("SSH ED25519"));
    }
}
