//! F05: SFTP downloads with verified host keys (ADR 0037).
//!
//! - The server's host key is compared with the fingerprints the service trusts for that server
//!   during the SSH handshake, before any password is sent. With none trusted it stops with
//!   `HOST_KEY_UNKNOWN`; with a different one trusted, `HOST_KEY_CHANGED`. Either message names the
//!   presented key (`<algorithm> SHA256:<base64>`, public data) and nothing is trusted here.
//! - Sign-in is the server's site login (ADR 0036); SFTP has no anonymous access.
//! - Identity: `stat` size and mtime (validator `"sftp-mtime:<seconds>"`). A resume reads on from
//!   the checkpoint offset only when both are unchanged; a changed file restarts from zero.
//! - Bytes go through the same write path as HTTP and FTP ([`write_single`]).

use std::sync::{Arc, Mutex};

use bytes::Bytes;
use dm_backend_api::BackendError;
use dm_domain::ErrorKind;
use reqwest::Url;
use reqwest::header::HeaderMap;
use russh::keys::{HashAlg, PublicKeyOrCertificate};
use russh_sftp::client::SftpSession;
use russh_sftp::protocol::StatusCode;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::sync::watch;

use crate::classify::io_error;
use crate::ftp::remote_path;
use crate::transfer::{
    Body, Ended, Identity, gate_url, or_stop, record_naming_once, within, write_single,
};
use crate::{Attempt, HttpConfig, lock};

const DEFAULT_PORT: u16 = 22;
const READ_CHUNK: usize = 64 * 1024;

/// The key a server presented: algorithm and SHA-256 fingerprint.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Presented {
    algorithm: String,
    fingerprint: String,
}

/// Compares the presented host key with the trusted fingerprints and remembers what it saw.
struct Verifier {
    trusted: Vec<String>,
    seen: Arc<Mutex<Option<Presented>>>,
}

impl russh::client::Handler for Verifier {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        // A host certificate is judged by the key it carries; no certificate authority is trusted.
        let public = match key {
            PublicKeyOrCertificate::PublicKey { key, .. } => key.clone(),
            PublicKeyOrCertificate::Certificate(cert) => {
                russh::keys::PublicKey::from(cert.public_key().clone())
            }
        };
        let presented = Presented {
            algorithm: public.algorithm().as_str().to_owned(),
            fingerprint: public.fingerprint(HashAlg::Sha256).to_string(),
        };
        let trusted = self.trusted.contains(&presented.fingerprint);
        *self
            .seen
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(presented);
        Ok(trusted)
    }
}

/// The message both host-key errors carry; the service reads the key back from it (ADR 0037).
pub(crate) fn key_detail(algorithm: &str, fingerprint: &str) -> String {
    format!("server key {algorithm} {fingerprint}")
}

fn host_key_error(trusted_any: bool, seen: Option<&Presented>) -> BackendError {
    let key = seen.map_or_else(
        || "an unknown key".to_owned(),
        |p| key_detail(&p.algorithm, &p.fingerprint),
    );
    if trusted_any {
        BackendError::new(
            ErrorKind::HostKeyChanged,
            format!(
                "the SFTP server's key changed since it was trusted; someone may be intercepting the connection, so nothing was sent ({key})"
            ),
        )
    } else {
        BackendError::new(
            ErrorKind::HostKeyUnknown,
            format!(
                "this SFTP server's key is not trusted yet; compare it with the key from the server's administrator ({key})"
            ),
        )
    }
}

fn sftp_error(e: &russh_sftp::client::error::Error) -> BackendError {
    use russh_sftp::client::error::Error as E;
    match e {
        E::Status(status) => match status.status_code {
            StatusCode::NoSuchFile => BackendError::new(
                ErrorKind::LinkRefreshRequired,
                "the file is not available on the SFTP server",
            ),
            StatusCode::PermissionDenied => BackendError::new(
                ErrorKind::AuthRequired,
                "the SFTP server does not allow this sign-in to read the file",
            ),
            StatusCode::ConnectionLost | StatusCode::NoConnection => BackendError::new(
                ErrorKind::NetworkOffline,
                "the connection to the SFTP server was lost",
            ),
            _ => BackendError::new(
                ErrorKind::InvalidRequest,
                "the SFTP server refused the request",
            ),
        },
        E::Timeout => BackendError::new(
            ErrorKind::NetworkOffline,
            "the SFTP server did not respond in time",
        ),
        _ => BackendError::new(
            ErrorKind::NetworkOffline,
            "the connection to the SFTP server failed",
        ),
    }
}

/// The path to request: `/~/x` is `x` in the user's home folder; anything else is absolute.
fn sftp_path(url: &Url) -> Result<String, BackendError> {
    let path = remote_path(url)?;
    Ok(match path.strip_prefix("/~/") {
        Some(home) if !home.is_empty() => home.to_owned(),
        _ => path,
    })
}

pub(crate) async fn fetch(
    config: &HttpConfig,
    attempt: &Attempt,
    stop: &mut watch::Receiver<bool>,
) -> Result<(), Ended> {
    let url = gate_url(&attempt.request.url, &config.scheme_gate)?;
    let path = sftp_path(&url)?;
    let Some(login) = attempt.request.login.clone() else {
        return Err(BackendError::new(
            ErrorKind::AuthRequired,
            "an SFTP server always needs a sign-in; add a site login for this server",
        )
        .into());
    };
    let resume = lock(&attempt.resume_from).take();
    if let Some(cp) = &resume
        && cp.identity.length == Some(cp.offset)
    {
        let file = tokio::fs::OpenOptions::new()
            .write(true)
            .open(&attempt.part)
            .await
            .map_err(|e| io_error("cannot open partial file", &e))?;
        file.sync_all()
            .await
            .map_err(|e| io_error("cannot flush partial file", &e))?;
        let mut shared = lock(&attempt.shared);
        shared.completed = cp.offset;
        shared.total = cp.identity.length;
        return Ok(());
    }
    let host = url
        .host_str()
        .ok_or_else(|| BackendError::new(ErrorKind::InvalidRequest, "the link has no host"))?
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_owned();
    let port = url.port().unwrap_or(DEFAULT_PORT);
    let read_timeout = attempt.read_timeout;
    let permit = or_stop(stop, attempt.limiter.acquire(&url)).await??;

    let seen = Arc::new(Mutex::new(None));
    let verifier = Verifier {
        trusted: attempt.request.trusted_host_keys.clone(),
        seen: Arc::clone(&seen),
    };
    let ssh_config = Arc::new(russh::client::Config {
        inactivity_timeout: Some(read_timeout.saturating_mul(2)),
        ..russh::client::Config::default()
    });
    let connected = or_stop(
        stop,
        within(
            config.connect_timeout.saturating_add(read_timeout),
            russh::client::connect(ssh_config, (host.as_str(), port), verifier),
        ),
    )
    .await??;
    let mut session = match connected {
        Ok(s) => s,
        Err(russh::Error::UnknownKey) => {
            let seen = seen
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone();
            return Err(host_key_error(
                !attempt.request.trusted_host_keys.is_empty(),
                seen.as_ref(),
            )
            .into());
        }
        Err(russh::Error::NoCommonAlgo { .. }) => {
            return Err(BackendError::new(
                ErrorKind::BackendCapabilityUnavailable,
                "the SFTP server offers no key or cipher Downova supports (RSA-only servers are not supported yet)",
            )
            .into());
        }
        Err(_) => {
            return Err(BackendError::new(
                ErrorKind::NetworkOffline,
                "could not connect to the SFTP server",
            )
            .into());
        }
    };
    let auth = or_stop(
        stop,
        within(
            read_timeout,
            session.authenticate_password(login.username.as_str(), login.password.expose()),
        ),
    )
    .await??
    .map_err(|_| {
        BackendError::new(
            ErrorKind::NetworkOffline,
            "the connection to the SFTP server failed during sign-in",
        )
    })?;
    if !auth.success() {
        return Err(BackendError::new(
            ErrorKind::AuthRequired,
            "the SFTP server refused the sign-in; check the site login",
        )
        .into());
    }
    let channel = or_stop(stop, within(read_timeout, session.channel_open_session()))
        .await??
        .map_err(|_| {
            BackendError::new(
                ErrorKind::NetworkOffline,
                "the SFTP session could not be opened",
            )
        })?;
    or_stop(
        stop,
        within(read_timeout, channel.request_subsystem(true, "sftp")),
    )
    .await??
    .map_err(|_| {
        BackendError::new(
            ErrorKind::BackendCapabilityUnavailable,
            "the server does not offer SFTP",
        )
    })?;
    let sftp = or_stop(
        stop,
        within(read_timeout, SftpSession::new(channel.into_stream())),
    )
    .await??
    .map_err(|e| sftp_error(&e))?;
    let meta = or_stop(stop, within(read_timeout, sftp.metadata(path.as_str())))
        .await??
        .map_err(|e| sftp_error(&e))?;
    if meta.is_dir() {
        return Err(BackendError::new(
            ErrorKind::InvalidRequest,
            "the SFTP link names a folder, not a file",
        )
        .into());
    }
    let identity = Identity {
        // Quoted like a strong ETag so the durable checkpoint format is unchanged.
        etag: meta.mtime.map(|t| format!("\"sftp-mtime:{t}\"")),
        length: meta.size,
        url: url.clone(),
        last_modified: None,
    };
    record_naming_once(attempt, &HeaderMap::new(), &url);
    let (start, hasher) = match resume {
        Some(cp) => {
            if cp.identity.etag.is_none() {
                return Err(BackendError::new(
                    ErrorKind::BackendCapabilityUnavailable,
                    "no strong validator",
                )
                .into());
            }
            if cp.identity.etag == identity.etag && cp.identity.length == identity.length {
                (cp.offset, cp.hasher)
            } else {
                (0, Sha256::new())
            }
        }
        None => (0, Sha256::new()),
    };
    let mut file = or_stop(stop, within(read_timeout, sftp.open(path.as_str())))
        .await??
        .map_err(|e| sftp_error(&e))?;
    if start > 0 {
        or_stop(
            stop,
            within(read_timeout, file.seek(std::io::SeekFrom::Start(start))),
        )
        .await??
        .map_err(|_| {
            BackendError::new(
                ErrorKind::NetworkOffline,
                "the SFTP server could not continue the file",
            )
        })?;
    }
    // The session and the SFTP channel stay alive with the body; they close when it ends.
    let body: Body = Box::pin(futures_util::stream::unfold(
        Some((session, sftp, file)),
        |state| async move {
            let (session, sftp, mut file) = state?;
            let mut buf = vec![0u8; READ_CHUNK];
            match file.read(&mut buf).await {
                Ok(0) => {
                    drop(file);
                    let _ = sftp.close().await;
                    let _ = session
                        .disconnect(russh::Disconnect::ByApplication, "", "en")
                        .await;
                    None
                }
                Ok(n) => {
                    buf.truncate(n);
                    Some((Ok(Bytes::from(buf)), Some((session, sftp, file))))
                }
                Err(_) => Some((
                    Err(BackendError::new(
                        ErrorKind::NetworkOffline,
                        "the SFTP connection failed while receiving the file",
                    )),
                    None,
                )),
            }
        },
    ));
    write_single(attempt, stop, body, identity, start, hasher, permit).await
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn home_relative_and_absolute_paths() {
        let p = |u: &str| sftp_path(&Url::parse(u).unwrap());
        assert_eq!(p("sftp://h/~/a.iso").unwrap(), "a.iso");
        assert_eq!(p("sftp://h/srv/a.iso").unwrap(), "/srv/a.iso");
        assert!(p("sftp://h/~/").is_err());
        assert!(p("sftp://h/a%0Ab").is_err());
    }

    #[test]
    fn host_key_errors_name_the_key_and_never_trust_it() {
        let seen = Presented {
            algorithm: "ssh-ed25519".into(),
            fingerprint: "SHA256:abc".into(),
        };
        let unknown = host_key_error(false, Some(&seen));
        assert_eq!(unknown.kind, ErrorKind::HostKeyUnknown);
        assert!(unknown.detail.contains("server key ssh-ed25519 SHA256:abc"));
        assert_eq!(
            host_key_error(true, Some(&seen)).kind,
            ErrorKind::HostKeyChanged
        );
    }
}
