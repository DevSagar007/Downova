//! A minimal HTTP proxy for engine tests (F06, ADR 0034).
//!
//! - Absolute-form requests (`GET http://host:port/path`) are forwarded in origin form, without
//!   `Proxy-Authorization`; the response is relayed until either side closes.
//! - `CONNECT host:port` opens a tunnel (`200`), or answers `407` / `502` as configured.
//! - With `credentials`, a request without the matching `Proxy-Authorization: Basic …` gets `407`.
//!
//! It counts what it saw so tests can prove traffic went through it (and never around it).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// How the proxy behaves.
#[derive(Debug, Clone, Default)]
pub struct ProxyBehaviour {
    /// `user:password` expected in Basic sign-in; `None` accepts every request.
    pub credentials: Option<(String, String)>,
    /// Answer every `CONNECT` with `502` (a refused tunnel).
    pub refuse_connect: bool,
}

#[derive(Debug, Default)]
struct Seen {
    forwarded: AtomicUsize,
    tunnels: AtomicUsize,
    refused: AtomicUsize,
    /// Raw `Proxy-Authorization` values received (to check what the client sent).
    auth: Mutex<Vec<String>>,
}

/// A running test proxy on 127.0.0.1.
#[derive(Debug)]
pub struct Proxy {
    port: u16,
    seen: Arc<Seen>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Proxy {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Proxy {
    pub async fn start(behaviour: ProxyBehaviour) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind proxy");
        let port = listener.local_addr().expect("proxy address").port();
        let seen = Arc::new(Seen::default());
        let s = Arc::clone(&seen);
        let behaviour = Arc::new(behaviour);
        let task = tokio::spawn(async move {
            while let Ok((conn, _)) = listener.accept().await {
                let s = Arc::clone(&s);
                let b = Arc::clone(&behaviour);
                tokio::spawn(async move {
                    let _ = serve(conn, &b, &s).await;
                });
            }
        });
        Self { port, seen, task }
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// Plain-HTTP requests forwarded to an origin.
    pub fn forwarded(&self) -> usize {
        self.seen.forwarded.load(Ordering::SeqCst)
    }

    /// `CONNECT` tunnels opened.
    pub fn tunnels(&self) -> usize {
        self.seen.tunnels.load(Ordering::SeqCst)
    }

    /// Requests answered with `407` or `502`.
    pub fn refused(&self) -> usize {
        self.seen.refused.load(Ordering::SeqCst)
    }

    /// Every `Proxy-Authorization` value received.
    pub fn auth_headers(&self) -> Vec<String> {
        self.seen
            .auth
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

/// Standard base64 (for the expected Basic value).
pub fn base64(input: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in input.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(T[((n >> (18 - 6 * i)) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Reads up to the end of the request head (`\r\n\r\n`); returns the head and any bytes after it.
async fn read_head(conn: &mut TcpStream) -> std::io::Result<(String, Vec<u8>)> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = conn.read(&mut chunk).await?;
        if n == 0 {
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let rest = buf.split_off(end + 4);
            return Ok((String::from_utf8_lossy(&buf).into_owned(), rest));
        }
        if buf.len() > 64 * 1024 {
            return Err(std::io::ErrorKind::InvalidData.into());
        }
    }
}

async fn serve(mut conn: TcpStream, b: &ProxyBehaviour, seen: &Seen) -> std::io::Result<()> {
    let (head, rest) = read_head(&mut conn).await?;
    let mut lines = head.split("\r\n");
    let first = lines.next().unwrap_or_default().to_owned();
    let headers: Vec<&str> = lines.filter(|l| !l.is_empty()).collect();
    let auth = headers.iter().find_map(|h| {
        let (name, value) = h.split_once(':')?;
        name.eq_ignore_ascii_case("proxy-authorization")
            .then(|| value.trim().to_owned())
    });
    if let Some(a) = &auth {
        seen.auth
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(a.clone());
    }
    if let Some((user, pass)) = &b.credentials {
        let expected = format!("Basic {}", base64(format!("{user}:{pass}").as_bytes()));
        if auth.as_deref() != Some(expected.as_str()) {
            seen.refused.fetch_add(1, Ordering::SeqCst);
            conn.write_all(
                b"HTTP/1.1 407 Proxy Authentication Required\r\nProxy-Authenticate: Basic realm=\"test\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            )
            .await?;
            return Ok(());
        }
    }
    let mut parts = first.split(' ');
    let method = parts.next().unwrap_or_default().to_owned();
    let target = parts.next().unwrap_or_default().to_owned();
    if method == "CONNECT" {
        if b.refuse_connect {
            seen.refused.fetch_add(1, Ordering::SeqCst);
            conn.write_all(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\n\r\n")
                .await?;
            return Ok(());
        }
        let mut upstream = TcpStream::connect(&target).await?;
        seen.tunnels.fetch_add(1, Ordering::SeqCst);
        conn.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
            .await?;
        upstream.write_all(&rest).await?;
        let _ = tokio::io::copy_bidirectional(&mut conn, &mut upstream).await;
        return Ok(());
    }
    // Absolute form: http://host:port/path → origin form to host:port.
    let Some(after) = target.strip_prefix("http://") else {
        conn.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n")
            .await?;
        return Ok(());
    };
    let (authority, path) = after
        .split_once('/')
        .map_or((after, "/".to_owned()), |(a, p)| (a, format!("/{p}")));
    let mut upstream = TcpStream::connect(authority).await?;
    seen.forwarded.fetch_add(1, Ordering::SeqCst);
    let version = parts.next().unwrap_or("HTTP/1.1");
    let mut out = format!("{method} {path} {version}\r\n");
    for h in headers {
        let name = h.split(':').next().unwrap_or_default();
        if !name.eq_ignore_ascii_case("proxy-authorization")
            && !name.eq_ignore_ascii_case("proxy-connection")
        {
            out.push_str(h);
            out.push_str("\r\n");
        }
    }
    out.push_str("\r\n");
    upstream.write_all(out.as_bytes()).await?;
    upstream.write_all(&rest).await?;
    let _ = tokio::io::copy_bidirectional(&mut conn, &mut upstream).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::base64;

    #[test]
    fn base64_matches_rfc_4648_vectors() {
        for (input, out) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("Aladdin:open sesame", "QWxhZGRpbjpvcGVuIHNlc2FtZQ=="),
        ] {
            assert_eq!(base64(input.as_bytes()), out);
        }
    }
}
