//! F05: anonymous FTP downloads (ADR 0035).
//!
//! - Passive data only: EPSV first, PASV if the server does not know it. The data connection
//!   always goes to the control connection's peer (never to an address a PASV reply names), so a
//!   hostile reply cannot point the download elsewhere (FTP bounce).
//! - Identity: `SIZE` is the length, `MDTM` the validator (`"ftp-mdtm:<timestamp>"`). A resume
//!   (`REST` + `RETR`) continues only when both still match; a changed file restarts from zero; a
//!   server without `MDTM` cannot resume, like HTTP without a strong ETag.
//! - Bytes go through the same write path as HTTP ([`write_single`]): partial file, checkpoints,
//!   disk budget, speed limits, stop. The transfer is complete only after the server's final
//!   `226`/`250` reply, and a known `SIZE` must be met exactly.
//! - Server text is never echoed into errors; only reply codes are classified.

use bytes::Bytes;
use dm_backend_api::BackendError;
use dm_domain::ErrorKind;
use reqwest::Url;
use reqwest::header::HeaderMap;
use sha2::{Digest, Sha256};
use suppaftp::tokio::AsyncFtpStream;
use suppaftp::types::FileType;
use suppaftp::{FtpError, Mode};
use tokio::io::AsyncReadExt;
use tokio::sync::watch;

use crate::classify::io_error;
use crate::transfer::{
    Body, Ended, Identity, gate_url, or_stop, record_naming_once, within, write_single,
};
use crate::{Attempt, HttpConfig, lock};

const ANONYMOUS_USER: &str = "anonymous";
const ANONYMOUS_PASSWORD: &str = "anonymous@";
const DEFAULT_PORT: u16 = 21;
const READ_CHUNK: usize = 64 * 1024;
/// How long a polite `QUIT` may take once the file is complete.
const QUIT_WAIT: std::time::Duration = std::time::Duration::from_secs(2);

/// Classifies an FTP failure by its reply code; the server's text never reaches the error.
pub(crate) fn ftp_error(e: &FtpError) -> BackendError {
    match e {
        FtpError::ConnectionError(_) => BackendError::new(
            ErrorKind::NetworkOffline,
            "the connection to the FTP server failed",
        ),
        FtpError::UnexpectedResponse(reply) => reply_error(reply.status.code()),
        FtpError::BadResponse => BackendError::new(
            ErrorKind::InvalidRequest,
            "the FTP server sent an invalid reply",
        ),
        FtpError::InvalidAddress(_) => BackendError::new(
            ErrorKind::InvalidRequest,
            "the FTP server named an invalid data address",
        ),
        FtpError::DataConnectionAlreadyOpen => BackendError::new(
            ErrorKind::Internal,
            "an FTP data connection was already open",
        ),
    }
}

fn reply_error(code: u32) -> BackendError {
    match code {
        530 | 532 => BackendError::new(
            ErrorKind::AuthRequired,
            "the FTP server refused the anonymous sign-in; it needs a user name and password",
        ),
        // No such file, or not a plain file the server will send: like an HTTP 404.
        550 | 553 => BackendError::new(
            ErrorKind::LinkRefreshRequired,
            format!("the file is not available on the FTP server (reply {code})"),
        ),
        421 | 425 | 426 | 450 | 451 => BackendError::new(
            ErrorKind::NetworkOffline,
            format!("the FTP server could not complete the transfer now (reply {code})"),
        ),
        _ => BackendError::new(
            ErrorKind::InvalidRequest,
            format!("the FTP server refused the request (reply {code})"),
        ),
    }
}

/// The path to request: the URL path, percent-decoded. Control characters (a CR/LF would end the
/// command early and smuggle another one) and folder links are refused before connecting.
pub(crate) fn remote_path(url: &Url) -> Result<String, BackendError> {
    let refuse = |why: &str| BackendError::new(ErrorKind::InvalidRequest, why.to_owned());
    let raw = url.path();
    if raw.is_empty() || raw == "/" || raw.ends_with('/') {
        return Err(refuse("the FTP link names a folder, not a file"));
    }
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        if b == b'%' {
            let hex = bytes
                .get(i.saturating_add(1)..i.saturating_add(3))
                .and_then(|h| std::str::from_utf8(h).ok())
                .and_then(|h| u8::from_str_radix(h, 16).ok())
                .ok_or_else(|| refuse("the FTP link has an invalid escape"))?;
            out.push(hex);
            i = i.saturating_add(3);
        } else {
            out.push(b);
            i = i.saturating_add(1);
        }
    }
    let path = String::from_utf8(out).map_err(|_| refuse("the FTP path is not valid UTF-8"))?;
    if path.chars().any(char::is_control) {
        return Err(refuse("the FTP path contains control characters"));
    }
    Ok(path)
}

/// Connects (bounded), signs in anonymously and selects binary transfers.
async fn connect(
    url: &Url,
    config: &HttpConfig,
    read_timeout: std::time::Duration,
) -> Result<AsyncFtpStream, BackendError> {
    let host = url
        .host()
        .ok_or_else(|| BackendError::new(ErrorKind::InvalidRequest, "the link has no host"))?;
    let port = url.port().unwrap_or(DEFAULT_PORT);
    let target = format!("{host}:{port}");
    let addrs: Vec<std::net::SocketAddr> = within(config.connect_timeout, async {
        tokio::net::lookup_host(target.as_str()).await
    })
    .await?
    .map_err(|_| {
        BackendError::new(
            ErrorKind::NetworkOffline,
            "could not resolve the FTP server's name",
        )
    })?
    .collect();
    let mut last = BackendError::new(
        ErrorKind::NetworkOffline,
        "could not connect to the FTP server",
    );
    for addr in addrs {
        // Connect and the welcome reply, each bounded.
        match within(
            config.connect_timeout.saturating_add(read_timeout),
            AsyncFtpStream::connect_timeout(addr, config.connect_timeout),
        )
        .await
        {
            Ok(Ok(mut ftp)) => {
                // Data connections go to this peer only, whatever a PASV reply says.
                ftp.set_passive_nat_workaround(true);
                ftp.set_mode(Mode::ExtendedPassive);
                within(read_timeout, ftp.login(ANONYMOUS_USER, ANONYMOUS_PASSWORD))
                    .await?
                    .map_err(|e| ftp_error(&e))?;
                within(read_timeout, ftp.transfer_type(FileType::Binary))
                    .await?
                    .map_err(|e| ftp_error(&e))?;
                return Ok(ftp);
            }
            Ok(Err(e)) => last = ftp_error(&e),
            Err(timed_out) => last = timed_out,
        }
    }
    Err(last)
}

/// `SIZE` / `MDTM`: `None` when the server does not offer the command (5xx other than 550).
async fn optional<T>(
    read_timeout: std::time::Duration,
    fut: impl std::future::Future<Output = Result<T, FtpError>>,
) -> Result<Option<T>, BackendError> {
    match within(read_timeout, fut).await? {
        Ok(v) => Ok(Some(v)),
        Err(FtpError::UnexpectedResponse(r)) if matches!(r.status.code(), 550 | 553) => {
            Err(reply_error(r.status.code()))
        }
        Err(FtpError::UnexpectedResponse(r)) if (500..600).contains(&r.status.code()) => Ok(None),
        Err(FtpError::BadResponse) => Ok(None),
        Err(e) => Err(ftp_error(&e)),
    }
}

/// The reply codes that mean "EPSV is not understood here": switch to PASV.
fn epsv_refused(e: &FtpError) -> bool {
    matches!(e, FtpError::UnexpectedResponse(r) if matches!(r.status.code(), 500 | 501 | 502 | 504 | 522))
}

pub(crate) async fn fetch(
    config: &HttpConfig,
    attempt: &Attempt,
    stop: &mut watch::Receiver<bool>,
) -> Result<(), Ended> {
    let url = gate_url(&attempt.request.url, &config.scheme_gate)?;
    let path = remote_path(&url)?;
    let resume = lock(&attempt.resume_from).take();
    if let Some(cp) = &resume
        && cp.identity.length == Some(cp.offset)
    {
        // Every byte was already confirmed: nothing to fetch (as for HTTP).
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
    let read_timeout = attempt.read_timeout;
    // The control connection holds the host's slot for the whole transfer.
    let permit = or_stop(stop, attempt.limiter.acquire(&url)).await??;
    let mut ftp = or_stop(stop, connect(&url, config, read_timeout)).await??;
    let size = or_stop(stop, optional(read_timeout, ftp.size(&path))).await??;
    let mdtm = or_stop(stop, optional(read_timeout, ftp.mdtm(&path))).await??;
    let identity = Identity {
        // Quoted like a strong ETag so the durable checkpoint format is unchanged; an FTP
        // validator never reaches an HTTP header (FTP links never take the HTTP path).
        etag: mdtm.map(|t| format!("\"ftp-mdtm:{}\"", t.format("%Y%m%d%H%M%S"))),
        length: size.and_then(|n| u64::try_from(n).ok()),
        url: url.clone(),
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
                // The file on the server changed: start over, like an HTTP `200` to a range.
                (0, Sha256::new())
            }
        }
        None => (0, Sha256::new()),
    };
    let offset = usize::try_from(start)
        .map_err(|_| BackendError::new(ErrorKind::Internal, "resume offset too large"))?;
    let mut opened = None;
    for mode in [Mode::ExtendedPassive, Mode::Passive] {
        ftp.set_mode(mode);
        if offset > 0 {
            or_stop(stop, within(read_timeout, ftp.resume_transfer(offset)))
                .await??
                .map_err(|e| ftp_error(&e))?;
        }
        match or_stop(stop, within(read_timeout, ftp.retr_as_stream(&path))).await?? {
            Ok(stream) => {
                opened = Some(stream);
                break;
            }
            Err(e) if mode == Mode::ExtendedPassive && epsv_refused(&e) => {}
            Err(e) => return Err(ftp_error(&e).into()),
        }
    }
    let stream = opened.ok_or_else(|| {
        BackendError::new(
            ErrorKind::InvalidRequest,
            "the FTP server offers no passive data connection",
        )
    })?;
    // Chunks until the data connection ends, then the server's completion reply.
    let body: Body = Box::pin(futures_util::stream::unfold(
        Some((ftp, stream)),
        |state| async move {
            let (mut ftp, mut stream) = state?;
            let mut buf = vec![0u8; READ_CHUNK];
            match stream.read(&mut buf).await {
                Ok(0) => {
                    let done = stream.finish().await;
                    let _ = tokio::time::timeout(QUIT_WAIT, ftp.quit()).await;
                    match done {
                        Ok(()) => None,
                        Err(e) => Some((Err(ftp_error(&e)), None)),
                    }
                }
                Ok(n) => {
                    buf.truncate(n);
                    Some((Ok(Bytes::from(buf)), Some((ftp, stream))))
                }
                Err(_) => Some((
                    Err(BackendError::new(
                        ErrorKind::NetworkOffline,
                        "the FTP data connection failed while receiving the file",
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
    fn paths_are_decoded_and_injection_or_folders_refused() {
        let p = |u: &str| remote_path(&Url::parse(u).unwrap());
        assert_eq!(p("ftp://h/pub/a%20b.iso").unwrap(), "/pub/a b.iso");
        assert_eq!(p("ftp://h/file.bin").unwrap(), "/file.bin");
        for bad in [
            "ftp://h/",
            "ftp://h/pub/",
            "ftp://h/a%0D%0ADELE%20x",
            "ftp://h/a%00b",
            "ftp://h/a%zz",
            "ftp://h/a%FF",
        ] {
            assert_eq!(p(bad).unwrap_err().kind, ErrorKind::InvalidRequest, "{bad}");
        }
    }

    #[test]
    fn reply_codes_are_classified_without_server_text() {
        assert_eq!(reply_error(530).kind, ErrorKind::AuthRequired);
        assert_eq!(reply_error(550).kind, ErrorKind::LinkRefreshRequired);
        for transient in [421, 425, 426, 450, 451] {
            assert_eq!(reply_error(transient).kind, ErrorKind::NetworkOffline);
        }
        assert_eq!(reply_error(504).kind, ErrorKind::InvalidRequest);
    }
}
