//! Controlled HTTP/1.1 (and HTTPS) fixture server for engine tests (docs/testing.md §1).
//!
//! Deterministic payloads, scripted misbehaviour per route, and a full log of what the client
//! actually sent (method, path, every header). It is hand-written so tests can produce responses a
//! well-behaved server library would refuse to send: 200 to a Range request, a wrong
//! Content-Range, a body shorter than Content-Length, content that changes between requests.
//!
//! Two APIs:
//! - `start(Behaviour, len)` — the B0.1 single-behaviour server (unchanged).
//! - `Server::start()` / `Server::start_tls(..)` — the B0.4 scriptable server with routes.

#![allow(dead_code)]

use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpListener;

pub mod tls;

// ---------------------------------------------------------------------------------------------
// Payloads
// ---------------------------------------------------------------------------------------------

/// Deterministic, non-repeating-at-small-scale payload.
pub fn payload(len: usize) -> Vec<u8> {
    payload_seeded(len, 0x9E37_79B9)
}

/// Like [`payload`] with a different seed: same length, different bytes ("version B").
pub fn payload_seeded(len: usize, seed: u32) -> Vec<u8> {
    let mut state: u32 = seed;
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            (state & 0xFF) as u8
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// B0.4 scriptable server
// ---------------------------------------------------------------------------------------------

/// How a route treats `Range` requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeMode {
    /// 206 with a correct Content-Range.
    Honour,
    /// 200 with the full body (range ignored).
    Ignore,
    /// 206 whose Content-Range and body start 4096 bytes later than requested.
    WrongContentRange,
    /// 416 for every ranged request.
    Always416,
}

/// A response that overrides normal content serving for one request (consumed in order).
#[derive(Debug, Clone)]
pub struct Scripted {
    pub status: u16,
    pub reason: &'static str,
    pub headers: Vec<(String, String)>,
}

impl Scripted {
    pub fn status(status: u16, reason: &'static str) -> Self {
        Self {
            status,
            reason,
            headers: Vec::new(),
        }
    }
    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_owned(), value.to_owned()));
        self
    }
}

#[derive(Debug, Clone)]
pub struct Route {
    pub body: Arc<Vec<u8>>,
    pub etag: Option<String>,
    pub ranges: RangeMode,
    /// Send `Content-Length` (otherwise the body ends with connection close).
    pub content_length: bool,
    /// Close the connection after this many body bytes of *each* response.
    pub truncate_after: Option<usize>,
    /// Truncate only the first N responses, then serve normally.
    pub truncate_times: usize,
    pub content_encoding: Option<String>,
    pub content_disposition: Option<String>,
    /// Redirect target (absolute URL) with this status, instead of content.
    pub redirect: Option<(u16, String)>,
    /// Per-16-KiB delay (slow body for pause/kill tests).
    pub slow: Option<Duration>,
    /// One-shot responses served before normal content.
    pub script: VecDeque<Scripted>,
}

impl Route {
    pub fn new(body: Vec<u8>) -> Self {
        Self {
            body: Arc::new(body),
            etag: Some("\"v1\"".to_owned()),
            ranges: RangeMode::Honour,
            content_length: true,
            truncate_after: None,
            truncate_times: usize::MAX,
            content_encoding: None,
            content_disposition: None,
            redirect: None,
            slow: None,
            script: VecDeque::new(),
        }
    }
}

/// One request as received.
#[derive(Debug, Clone, Default)]
pub struct Logged {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
}

impl Logged {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

#[derive(Clone)]
pub struct Server {
    pub addr: SocketAddr,
    pub scheme: &'static str,
    routes: Arc<Mutex<HashMap<String, Route>>>,
    log: Arc<Mutex<Vec<Logged>>>,
}

impl Server {
    /// Plain HTTP server on 127.0.0.1.
    pub async fn start() -> Self {
        Self::start_with(None).await
    }

    /// HTTPS server presenting `identity` (see [`tls`]).
    pub async fn start_tls(identity: tls::Identity) -> Self {
        Self::start_with(Some(identity)).await
    }

    async fn start_with(identity: Option<tls::Identity>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let server = Server {
            addr: listener.local_addr().unwrap(),
            scheme: if identity.is_some() { "https" } else { "http" },
            routes: Arc::new(Mutex::new(HashMap::new())),
            log: Arc::new(Mutex::new(Vec::new())),
        };
        let acceptor = identity.map(|id| id.acceptor());
        let s = server.clone();
        tokio::spawn(async move {
            loop {
                let Ok((tcp, _)) = listener.accept().await else {
                    return;
                };
                let s = s.clone();
                let acceptor = acceptor.clone();
                tokio::spawn(async move {
                    match acceptor {
                        Some(acc) => {
                            // A rejected handshake (client refuses our certificate) is expected in
                            // the invalid-certificate tests; it simply ends the connection.
                            if let Ok(tls) = acc.accept(tcp).await {
                                let _ = s.serve(tls).await;
                            }
                        }
                        None => {
                            let _ = s.serve(tcp).await;
                        }
                    }
                });
            }
        });
        server
    }

    /// Absolute URL for `path` (leading slash optional). Hostname `localhost` is used for HTTPS
    /// so certificate name checks are meaningful.
    pub fn url(&self, path: &str) -> String {
        let host = if self.scheme == "https" {
            "localhost"
        } else {
            "127.0.0.1"
        };
        format!(
            "{}://{host}:{}/{}",
            self.scheme,
            self.addr.port(),
            path.trim_start_matches('/')
        )
    }

    pub fn set(&self, path: &str, route: Route) {
        self.routes
            .lock()
            .unwrap()
            .insert(format!("/{}", path.trim_start_matches('/')), route);
    }

    pub fn update(&self, path: &str, f: impl FnOnce(&mut Route)) {
        let key = format!("/{}", path.trim_start_matches('/'));
        if let Some(route) = self.routes.lock().unwrap().get_mut(&key) {
            f(route);
        }
    }

    pub fn log(&self) -> Vec<Logged> {
        self.log.lock().unwrap().clone()
    }

    pub fn log_for(&self, path: &str) -> Vec<Logged> {
        let key = format!("/{}", path.trim_start_matches('/'));
        self.log().into_iter().filter(|l| l.path == key).collect()
    }

    async fn serve<S: AsyncRead + AsyncWrite + Unpin>(&self, mut stream: S) -> std::io::Result<()> {
        loop {
            let Some(head) = read_head(&mut stream).await? else {
                return Ok(());
            };
            let mut lines = head.lines();
            let request_line = lines.next().unwrap_or_default();
            let mut parts = request_line.split_whitespace();
            let method = parts.next().unwrap_or_default().to_owned();
            let path = parts
                .next()
                .unwrap_or("/")
                .split('?')
                .next()
                .unwrap_or("/")
                .to_owned();
            let headers: Vec<(String, String)> = lines
                .filter_map(|l| l.split_once(':'))
                .map(|(n, v)| (n.trim().to_owned(), v.trim().to_owned()))
                .collect();
            let logged = Logged {
                method: method.clone(),
                path: path.clone(),
                headers,
            };
            self.log.lock().unwrap().push(logged.clone());

            // Snapshot the route state for this response (tests may mutate it concurrently).
            let found = {
                let mut routes = self.routes.lock().unwrap();
                routes.get_mut(&path).map(|route| {
                    let scripted = route.script.pop_front();
                    let truncate = if scripted.is_none() && route.truncate_times > 0 {
                        if route.truncate_after.is_some() && route.truncate_times != usize::MAX {
                            route.truncate_times -= 1;
                        }
                        route.truncate_after
                    } else {
                        None
                    };
                    (route.clone(), scripted, truncate)
                })
            };
            let Some((route, scripted, truncate)) = found else {
                write_simple(&mut stream, 404, "Not Found", &[]).await?;
                continue;
            };

            if let Some(s) = scripted {
                let headers: Vec<(&str, &str)> = s
                    .headers
                    .iter()
                    .map(|(n, v)| (n.as_str(), v.as_str()))
                    .collect();
                write_simple(&mut stream, s.status, s.reason, &headers).await?;
                continue;
            }
            if let Some((status, location)) = &route.redirect {
                write_simple(&mut stream, *status, "Redirect", &[("Location", location)]).await?;
                continue;
            }

            let total = route.body.len();
            let requested = logged.header("range").and_then(|r| parse_range(r, total));
            if requested.is_some() && route.ranges == RangeMode::Always416 {
                let cr = format!("bytes */{total}");
                write_simple(
                    &mut stream,
                    416,
                    "Range Not Satisfiable",
                    &[("Content-Range", &cr)],
                )
                .await?;
                continue;
            }
            let (status, start, end, content_range) = match (requested, route.ranges) {
                (Some((s, e)), RangeMode::Honour) => {
                    (206, s, e, Some(format!("bytes {s}-{}/{total}", e - 1)))
                }
                (Some((s, e)), RangeMode::WrongContentRange) => {
                    let shifted = (s + 4096).min(total.saturating_sub(1));
                    let end = (shifted + (e - s)).min(total);
                    (
                        206,
                        shifted,
                        end,
                        Some(format!("bytes {shifted}-{}/{total}", end - 1)),
                    )
                }
                _ => (200, 0, total, None),
            };

            let mut head = format!(
                "HTTP/1.1 {status} {}\r\nContent-Type: application/octet-stream\r\n",
                if status == 206 {
                    "Partial Content"
                } else {
                    "OK"
                }
            );
            let accept = if route.ranges == RangeMode::Ignore {
                "none"
            } else {
                "bytes"
            };
            head.push_str(&format!("Accept-Ranges: {accept}\r\n"));
            if let Some(etag) = &route.etag {
                head.push_str(&format!("ETag: {etag}\r\n"));
            }
            if let Some(cr) = &content_range {
                head.push_str(&format!("Content-Range: {cr}\r\n"));
            }
            if let Some(enc) = &route.content_encoding {
                head.push_str(&format!("Content-Encoding: {enc}\r\n"));
            }
            if let Some(cd) = &route.content_disposition {
                head.push_str(&format!("Content-Disposition: {cd}\r\n"));
            }
            let body = &route.body[start..end];
            if route.content_length {
                head.push_str(&format!("Content-Length: {}\r\n", body.len()));
            } else {
                head.push_str("Connection: close\r\n");
            }
            head.push_str("\r\n");
            stream.write_all(head.as_bytes()).await?;
            if method == "HEAD" {
                continue;
            }
            let limit = truncate.map_or(body.len(), |t| t.min(body.len()));
            match route.slow {
                Some(delay) => {
                    for chunk in body[..limit].chunks(16 * 1024) {
                        stream.write_all(chunk).await?;
                        tokio::time::sleep(delay).await;
                    }
                }
                None => stream.write_all(&body[..limit]).await?,
            }
            if truncate.is_some() || !route.content_length {
                stream.flush().await?;
                stream.shutdown().await?;
                return Ok(());
            }
        }
    }
}

async fn write_simple<S: AsyncWrite + Unpin>(
    stream: &mut S,
    status: u16,
    reason: &str,
    headers: &[(&str, &str)],
) -> std::io::Result<()> {
    let mut head = format!("HTTP/1.1 {status} {reason}\r\nContent-Length: 0\r\n");
    for (n, v) in headers {
        head.push_str(&format!("{n}: {v}\r\n"));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes()).await
}

// ---------------------------------------------------------------------------------------------
// B0.1 single-behaviour server (kept for the existing fixtures)
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Behaviour {
    /// Honours `Range: bytes=a-b` with 206 + Content-Range.
    Ranges,
    /// Ignores Range and always answers 200 with the full body.
    IgnoreRange,
    /// Advertises the full Content-Length but closes after half the body.
    Truncate,
    /// Like `Ranges`, but sends the body slowly (for pause/resume tests).
    Slow,
}

#[derive(Debug, Clone, Default)]
pub struct Request {
    pub range: Option<String>,
    pub authorization: Option<String>,
}

#[derive(Clone)]
pub struct Fixture {
    pub addr: SocketAddr,
    pub body: Arc<Vec<u8>>,
    server: Server,
}

impl Fixture {
    pub fn url(&self, name: &str) -> String {
        format!("http://{}/{name}", self.addr)
    }

    pub fn requests(&self) -> Vec<Request> {
        self.server
            .log()
            .into_iter()
            .map(|l| Request {
                range: l.header("range").map(str::to_owned),
                authorization: l.header("authorization").map(str::to_owned),
            })
            .collect()
    }
}

/// B0.1 API: every path serves the same payload with the given behaviour.
pub async fn start(behaviour: Behaviour, len: usize) -> Fixture {
    let server = Server::start().await;
    let body = payload(len);
    let mut route = Route::new(body.clone());
    route.etag = Some("\"fixture-v1\"".to_owned());
    match behaviour {
        Behaviour::Ranges => {}
        Behaviour::IgnoreRange => route.ranges = RangeMode::Ignore,
        Behaviour::Truncate => {
            route.ranges = RangeMode::Ignore;
            route.truncate_after = Some(len / 2);
        }
        Behaviour::Slow => route.slow = Some(Duration::from_millis(40)),
    }
    for name in [
        "a.bin", "s.bin", "n.bin", "t.bin", "p.bin", "w.bin", "h.bin",
    ] {
        server.set(name, route.clone());
    }
    Fixture {
        addr: server.addr,
        body: Arc::new(body),
        server,
    }
}

// ---------------------------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------------------------

/// Parses a single `bytes=a-b` / `bytes=a-` range into half-open `[start, end)`.
fn parse_range(value: &str, total: usize) -> Option<(usize, usize)> {
    let spec = value.strip_prefix("bytes=")?;
    let (a, b) = spec.split_once('-')?;
    let start: usize = a.parse().ok()?;
    let end = if b.is_empty() {
        total
    } else {
        b.parse::<usize>().ok()?.checked_add(1)?.min(total)
    };
    (start < end).then_some((start, end))
}

async fn read_head<S: AsyncRead + Unpin>(stream: &mut S) -> std::io::Result<Option<String>> {
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        if stream.read(&mut byte).await? == 0 {
            return Ok(None);
        }
        buf.push(byte[0]);
        if buf.ends_with(b"\r\n\r\n") {
            return Ok(Some(String::from_utf8_lossy(&buf).into_owned()));
        }
        if buf.len() > 64 * 1024 {
            return Ok(None);
        }
    }
}
