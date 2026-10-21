//! Controlled HTTP/1.1 fixture server for engine tests (docs/testing.md §1).
//!
//! Deterministic payload, scripted misbehaviour, and a log of what the client asked for.
//! It is intentionally small and hand-written so tests can produce responses a well-behaved server
//! library would refuse to send (e.g. 200 to a Range request, a body shorter than Content-Length).

#![allow(dead_code)]

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

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

#[derive(Debug, Clone)]
pub struct Fixture {
    pub addr: SocketAddr,
    pub body: Arc<Vec<u8>>,
    pub requests: Arc<Mutex<Vec<Request>>>,
}

impl Fixture {
    pub fn url(&self, name: &str) -> String {
        format!("http://{}/{name}", self.addr)
    }

    pub fn requests(&self) -> Vec<Request> {
        self.requests.lock().unwrap().clone()
    }
}

/// Deterministic, non-repeating-at-small-scale payload.
pub fn payload(len: usize) -> Vec<u8> {
    let mut state: u32 = 0x9E37_79B9;
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            (state & 0xFF) as u8
        })
        .collect()
}

pub async fn start(behaviour: Behaviour, len: usize) -> Fixture {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let fixture = Fixture {
        addr: listener.local_addr().unwrap(),
        body: Arc::new(payload(len)),
        requests: Arc::new(Mutex::new(Vec::new())),
    };
    let server = fixture.clone();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            let server = server.clone();
            tokio::spawn(async move {
                let _ = serve(stream, behaviour, server).await;
            });
        }
    });
    fixture
}

async fn serve(mut stream: TcpStream, behaviour: Behaviour, fx: Fixture) -> std::io::Result<()> {
    loop {
        let Some(head) = read_head(&mut stream).await? else {
            return Ok(());
        };
        let mut request = Request::default();
        for line in head.lines().skip(1) {
            if let Some((name, value)) = line.split_once(':') {
                match name.trim().to_ascii_lowercase().as_str() {
                    "range" => request.range = Some(value.trim().to_owned()),
                    "authorization" => request.authorization = Some(value.trim().to_owned()),
                    _ => {}
                }
            }
        }
        let is_head = head.starts_with("HEAD ");
        fx.requests.lock().unwrap().push(request.clone());

        let total = fx.body.len();
        let range = match behaviour {
            Behaviour::IgnoreRange | Behaviour::Truncate => None,
            Behaviour::Ranges | Behaviour::Slow => {
                request.range.as_deref().and_then(|r| parse_range(r, total))
            }
        };
        let (status, start, end) = match range {
            Some((s, e)) => ("206 Partial Content", s, e),
            None => ("200 OK", 0, total),
        };
        let mut headers = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nAccept-Ranges: {}\r\nETag: \"fixture-v1\"\r\nContent-Type: application/octet-stream\r\n",
            end - start,
            if behaviour == Behaviour::IgnoreRange || behaviour == Behaviour::Truncate {
                "none"
            } else {
                "bytes"
            },
        );
        if range.is_some() {
            headers.push_str(&format!(
                "Content-Range: bytes {start}-{}/{total}\r\n",
                end - 1
            ));
        }
        headers.push_str("\r\n");
        stream.write_all(headers.as_bytes()).await?;
        if is_head {
            continue;
        }
        let body = &fx.body[start..end];
        match behaviour {
            Behaviour::Truncate => {
                stream.write_all(&body[..body.len() / 2]).await?;
                stream.shutdown().await?;
                return Ok(());
            }
            Behaviour::Slow => {
                for chunk in body.chunks(16 * 1024) {
                    stream.write_all(chunk).await?;
                    tokio::time::sleep(Duration::from_millis(40)).await;
                }
            }
            _ => stream.write_all(body).await?,
        }
    }
}

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

async fn read_head(stream: &mut TcpStream) -> std::io::Result<Option<String>> {
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
