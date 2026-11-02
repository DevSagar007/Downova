//! Local IPC transport: newline-delimited JSON-RPC 2.0 over a user-only endpoint (ARCH-06).
//!
//! - **Windows:** named pipe `\\.\pipe\downova-<random>`, DACL `D:P(A;;GA;;;OW)(A;;GA;;;SY)`
//!   (protected; owner and SYSTEM only — the default pipe DACL would grant Everyone read), and
//!   remote clients rejected (interprocess default).
//! - **Unix:** socket `downova.sock` inside the 0700 run directory; on Linux the socket file is
//!   additionally created with mode 0600 before `bind()`.
//!
//! One request and one response per line. The service accepts request lines up to [`MAX_LINE`]
//! (requests never carry file content); clients accept response and notification lines up to
//! [`MAX_RESPONSE_LINE`], because a list or snapshot of a large history is legitimately bigger
//! (10,000 jobs ≈ 6 MiB). A longer line closes the connection.
//! A subscribed connection (`events.v1.subscribe`, B2.9) also carries server notifications; the
//! server splits it ([`Connection::into_split`]) and writes every line from one writer, so a
//! response and a notification never interleave.

use std::io;
use std::path::Path;
use std::time::Duration;

use dm_domain::ErrorKind;
use dm_domain::api::{
    RpcError, RpcNotification, RpcRequest, RpcResponse, ServiceEvent, SubscribeResult, codes,
    methods,
};
use interprocess::local_socket::tokio::{Listener, RecvHalf, SendHalf, Stream, prelude::*};
use interprocess::local_socket::{ConnectOptions, ListenerOptions};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

/// Maximum request line the service accepts, in bytes. Requests never carry file content.
pub const MAX_LINE: usize = 1024 * 1024;
/// Maximum response or notification line a client accepts from its own user's service, in bytes
/// (B2.22: a 10,000-job snapshot exceeded the former shared 1 MiB bound and could never load).
pub const MAX_RESPONSE_LINE: usize = 64 * 1024 * 1024;

/// A bound, user-only listener and the endpoint string clients use to reach it.
#[derive(Debug)]
pub struct BoundListener {
    pub listener: Listener,
    pub endpoint: String,
}

impl BoundListener {
    /// Accepts the next client connection.
    pub async fn accept(&self) -> io::Result<Connection> {
        self.listener.accept().await.map(Connection::new)
    }
}

/// Creates the listener. `run_dir` must already be private (see `paths::ensure_private_dir`).
pub fn bind(run_dir: &Path) -> io::Result<BoundListener> {
    #[cfg(windows)]
    {
        use interprocess::local_socket::GenericNamespaced;
        use interprocess::os::windows::local_socket::ListenerOptionsExt;
        use interprocess::os::windows::security_descriptor::SecurityDescriptor;
        let _ = run_dir;
        let endpoint = format!("downova-{}", crate::random_hex(16)?);
        let sddl = widestring::U16CString::from_str("D:P(A;;GA;;;OW)(A;;GA;;;SY)")
            .map_err(io::Error::other)?;
        let listener = ListenerOptions::new()
            .name(endpoint.clone().to_ns_name::<GenericNamespaced>()?)
            .security_descriptor(SecurityDescriptor::deserialize(&sddl)?)
            .create_tokio()?;
        Ok(BoundListener { listener, endpoint })
    }
    #[cfg(unix)]
    {
        use interprocess::local_socket::GenericFilePath;
        let path = run_dir.join("downova.sock");
        let endpoint = path.to_string_lossy().into_owned();
        let options = ListenerOptions::new()
            .name(path.to_fs_name::<GenericFilePath>()?)
            .try_overwrite(true);
        #[cfg(target_os = "linux")]
        let options = {
            use interprocess::os::unix::local_socket::ListenerOptionsExt;
            options.mode(0o600)
        };
        let listener = options.create_tokio()?;
        Ok(BoundListener { listener, endpoint })
    }
}

fn name_of(endpoint: &str) -> io::Result<interprocess::local_socket::Name<'_>> {
    #[cfg(windows)]
    {
        use interprocess::local_socket::GenericNamespaced;
        endpoint.to_ns_name::<GenericNamespaced>()
    }
    #[cfg(unix)]
    {
        use interprocess::local_socket::GenericFilePath;
        endpoint.to_fs_name::<GenericFilePath>()
    }
}

/// Connects to a service endpoint.
pub async fn connect(endpoint: &str) -> io::Result<Connection> {
    let stream = ConnectOptions::new()
        .name(name_of(endpoint)?)
        .connect_tokio()
        .await?;
    Ok(Connection::with_limit(stream, MAX_RESPONSE_LINE))
}

/// A framed connection (either side).
#[derive(Debug)]
pub struct Connection {
    reader: BufReader<RecvHalf>,
    writer: SendHalf,
    /// The longest line this side reads.
    max_line: usize,
}

impl Connection {
    /// The service's side of an accepted connection: request lines up to [`MAX_LINE`].
    pub fn new(stream: Stream) -> Self {
        Self::with_limit(stream, MAX_LINE)
    }

    fn with_limit(stream: Stream, max_line: usize) -> Self {
        let (recv, send) = stream.split();
        Self {
            reader: BufReader::new(recv),
            writer: send,
            max_line,
        }
    }

    /// Reads one line. `Ok(None)` on clean end of stream.
    pub async fn read_line(&mut self) -> io::Result<Option<String>> {
        read_line_from(&mut self.reader, self.max_line).await
    }

    /// Writes one JSON value as a line.
    pub async fn write_json<T: serde::Serialize>(&mut self, value: &T) -> io::Result<()> {
        write_json_to(&mut self.writer, value).await
    }

    /// Separate reading and writing halves (a subscribed connection reads requests while one
    /// writer sends responses and notifications in order).
    pub fn into_split(self) -> (LineReader, LineWriter) {
        (
            LineReader {
                reader: self.reader,
                max_line: self.max_line,
            },
            LineWriter {
                writer: self.writer,
            },
        )
    }
}

/// The reading half of a [`Connection`].
#[derive(Debug)]
pub struct LineReader {
    reader: BufReader<RecvHalf>,
    max_line: usize,
}

impl LineReader {
    pub async fn read_line(&mut self) -> io::Result<Option<String>> {
        read_line_from(&mut self.reader, self.max_line).await
    }
}

/// The writing half of a [`Connection`]. Owned by exactly one task, which serializes lines.
#[derive(Debug)]
pub struct LineWriter {
    writer: SendHalf,
}

impl LineWriter {
    pub async fn write_json<T: serde::Serialize>(&mut self, value: &T) -> io::Result<()> {
        write_json_to(&mut self.writer, value).await
    }
}

async fn read_line_from(
    reader: &mut BufReader<RecvHalf>,
    max_line: usize,
) -> io::Result<Option<String>> {
    let mut buf = Vec::new();
    let mut limited = reader.take(u64::try_from(max_line).unwrap_or(u64::MAX));
    let n = limited.read_until(b'\n', &mut buf).await?;
    if n == 0 {
        return Ok(None);
    }
    if !buf.ends_with(b"\n") {
        // Either the bound was reached without a newline, or the peer stopped mid-line.
        return Err(if n >= max_line {
            io::Error::new(io::ErrorKind::InvalidData, LineTooLong { limit: max_line })
        } else {
            io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "connection ended inside a line",
            )
        });
    }
    buf.pop();
    String::from_utf8(buf)
        .map(Some)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "line is not UTF-8"))
}

/// A line longer than the reader's bound: the connection cannot continue (the rest of the line is
/// unread), and repeating the same request would fail the same way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineTooLong {
    pub limit: usize,
}

impl std::fmt::Display for LineTooLong {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "a line exceeded the {} byte limit", self.limit)
    }
}

impl std::error::Error for LineTooLong {}

/// Maps a read failure to a client error: an oversized line is its own, non-retryable failure.
fn read_failure(e: &io::Error) -> CallError {
    match e
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<LineTooLong>())
    {
        Some(too_long) => CallError::TooLarge {
            limit: too_long.limit,
        },
        None => CallError::Unavailable("connection lost".into()),
    }
}

async fn write_json_to<T: serde::Serialize>(writer: &mut SendHalf, value: &T) -> io::Result<()> {
    let mut line = serde_json::to_vec(value).map_err(io::Error::other)?;
    line.push(b'\n');
    writer.write_all(&line).await?;
    writer.flush().await
}

/// Client-side failure: either the service could not be reached/understood, or it answered with
/// a JSON-RPC error (which carries the stable [`dm_domain::ErrorData`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallError {
    /// No service, stale endpoint, broken connection or malformed reply.
    Unavailable(String),
    /// The service answered with an error.
    Rpc(RpcError),
    /// The service's answer (or event) was longer than this client accepts
    /// ([`MAX_RESPONSE_LINE`]). Not transient: repeating the call gets the same answer, so callers
    /// report it instead of reconnecting in a loop.
    TooLarge { limit: usize },
}

impl CallError {
    /// Stable kind for presentation (`SERVICE_UNAVAILABLE` for transport problems).
    pub fn kind(&self) -> ErrorKind {
        match self {
            CallError::Unavailable(_) => ErrorKind::ServiceUnavailable,
            CallError::Rpc(e) => e.data.as_ref().map_or(ErrorKind::Internal, |d| d.kind),
            CallError::TooLarge { .. } => ErrorKind::Internal,
        }
    }
}

impl std::fmt::Display for CallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CallError::Unavailable(why) => write!(f, "download service unavailable: {why}"),
            CallError::TooLarge { limit } => write!(
                f,
                "the download service's answer is larger than this client accepts ({limit} bytes)"
            ),
            CallError::Rpc(e) => match e.data.as_ref().and_then(|d| d.detail.as_deref()) {
                Some(detail) => write!(f, "{}: {detail}", e.message),
                None => f.write_str(&e.message),
            },
        }
    }
}

impl std::error::Error for CallError {}

/// Minimal JSON-RPC client over one connection.
#[derive(Debug)]
pub struct Client {
    conn: Connection,
    next_id: u64,
}

impl Client {
    /// Connects using the endpoint file in `data_dir`.
    pub async fn connect(data_dir: &Path) -> Result<Self, CallError> {
        let info = crate::endpoint::read(data_dir).map_err(|e| match e.kind() {
            io::ErrorKind::NotFound => CallError::Unavailable("service is not running".into()),
            _ => CallError::Unavailable("endpoint file unreadable".into()),
        })?;
        Self::connect_endpoint(&info.endpoint).await
    }

    /// Connects to an explicit endpoint, giving up after a short timeout.
    pub async fn connect_endpoint(endpoint: &str) -> Result<Self, CallError> {
        let conn = tokio::time::timeout(Duration::from_secs(5), connect(endpoint))
            .await
            .map_err(|_| CallError::Unavailable("connection timed out".into()))?
            .map_err(|_| {
                CallError::Unavailable("service is not running (stale endpoint)".into())
            })?;
        Ok(Self { conn, next_id: 1 })
    }

    /// Sends one request and waits for its response.
    pub async fn call(&mut self, method: &str, params: Value) -> Result<Value, CallError> {
        let id = self.next_id.to_string();
        self.next_id = self.next_id.saturating_add(1);
        self.send(method, params, &id).await?;
        let line = self
            .conn
            .read_line()
            .await
            .map_err(|e| read_failure(&e))?
            .ok_or_else(|| CallError::Unavailable("connection closed by service".into()))?;
        let response: RpcResponse = serde_json::from_str(&line)
            .map_err(|_| CallError::Unavailable("malformed response".into()))?;
        if response.id.as_deref() != Some(id.as_str()) {
            return Err(CallError::Unavailable("response id mismatch".into()));
        }
        match (response.result, response.error) {
            (_, Some(error)) => Err(CallError::Rpc(error)),
            (Some(result), None) => Ok(result),
            (None, None) => Err(CallError::Unavailable("empty response".into())),
        }
    }

    /// `events.v1.subscribe` (B2.9): the authoritative list and its sequence; the connection then
    /// delivers [`ServiceEvent`]s through [`Client::next_event`]. Use a dedicated connection.
    pub async fn subscribe(&mut self) -> Result<SubscribeResult, CallError> {
        let value = self
            .call(methods::SUBSCRIBE, Value::Object(Default::default()))
            .await?;
        serde_json::from_value(value)
            .map_err(|_| CallError::Unavailable("malformed response".into()))
    }

    /// The next notification of a subscribed connection; `Ok(None)` when the service closed the
    /// stream (shutdown, or after `snapshot_required`). Any other line is a protocol error.
    pub async fn next_event(&mut self) -> Result<Option<ServiceEvent>, CallError> {
        let Some(line) = self.conn.read_line().await.map_err(|e| read_failure(&e))? else {
            return Ok(None);
        };
        let note: RpcNotification = serde_json::from_str(&line)
            .map_err(|_| CallError::Unavailable("malformed notification".into()))?;
        if note.method != methods::EVENT {
            return Err(CallError::Unavailable("unexpected notification".into()));
        }
        Ok(Some(note.params))
    }

    /// The next raw line from the service (tests that check framing).
    pub async fn raw_line(&mut self) -> Result<Option<String>, CallError> {
        self.conn.read_line().await.map_err(|e| read_failure(&e))
    }

    /// Sends a request without waiting (used by tests to simulate a lost acknowledgement).
    pub async fn send(&mut self, method: &str, params: Value, id: &str) -> Result<(), CallError> {
        let request = RpcRequest {
            jsonrpc: "2.0".into(),
            id: id.to_owned(),
            method: method.to_owned(),
            params,
        };
        self.conn
            .write_json(&request)
            .await
            .map_err(|_| CallError::Unavailable("connection lost".into()))
    }
}

/// Builds a JSON-RPC error response for malformed input (used by servers).
pub fn protocol_error(id: Option<String>, code: i64, detail: &str) -> RpcResponse {
    let data = ErrorKind::InvalidRequest.to_error_data(detail);
    let mut error = RpcError::new(code, data);
    if code == codes::PARSE_ERROR {
        error.message = "PARSE_ERROR".into();
    }
    RpcResponse::err(id, error)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// B2.22 regression: a response larger than the request bound (a 10,000-job list) reaches the
    /// client, while the service still refuses an oversized request.
    #[tokio::test]
    async fn large_responses_reach_clients_but_oversized_requests_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        crate::paths::ensure_private_dir(dir.path()).unwrap();
        let bound = bind(dir.path()).unwrap();
        let endpoint = bound.endpoint.clone();
        let big = "x".repeat(6 * 1024 * 1024);
        let reply = big.clone();
        let server = tokio::spawn(async move {
            let stream = bound.listener.accept().await.unwrap();
            let mut conn = Connection::new(stream);
            let line = conn.read_line().await.unwrap().unwrap();
            let req: RpcRequest = serde_json::from_str(&line).unwrap();
            conn.write_json(&RpcResponse::ok(
                Some(req.id),
                serde_json::json!({ "big": reply }),
            ))
            .await
            .unwrap();
            // A second client sends an oversized request: refused.
            let stream = bound.listener.accept().await.unwrap();
            let mut conn = Connection::new(stream);
            conn.read_line().await.is_err()
        });
        let mut client = Client::connect_endpoint(&endpoint).await.unwrap();
        let reply = client
            .call(methods::LIST, serde_json::json!({}))
            .await
            .unwrap();
        assert_eq!(reply["big"].as_str().map(str::len), Some(big.len()));
        assert!(big.len() > MAX_LINE);
        let mut abuser = connect(&endpoint).await.unwrap();
        let _ = abuser
            .write_json(&serde_json::json!({ "pad": "y".repeat(MAX_LINE + 10) }))
            .await;
        assert!(server.await.unwrap(), "the oversized request was refused");
    }

    /// Framing bounds (B2.24), with a small limit instead of huge buffers: a line exactly at the
    /// limit is read; one byte more is a typed, non-retryable failure; a peer that stops inside a
    /// line, or never sends the newline before closing, is a lost connection — never a hang.
    #[tokio::test]
    async fn reads_stay_bounded_and_classify_oversized_and_truncated_lines() {
        use tokio::io::AsyncWriteExt;
        const LIMIT: usize = 64;
        let dir = tempfile::tempdir().unwrap();
        crate::paths::ensure_private_dir(dir.path()).unwrap();
        let bound = bind(dir.path()).unwrap();
        let endpoint = bound.endpoint.clone();
        let server = tokio::spawn(async move {
            // 1: a line of exactly LIMIT bytes (newline included), then one of LIMIT + 1.
            let stream = bound.listener.accept().await.unwrap();
            let (_, mut send) = stream.split();
            let fits = format!("{}\n", "a".repeat(LIMIT - 1));
            let over = format!("{}\n", "b".repeat(LIMIT));
            send.write_all(fits.as_bytes()).await.unwrap();
            send.write_all(over.as_bytes()).await.unwrap();
            send.flush().await.unwrap();
            // 2: half a line, then the peer goes away.
            let stream = bound.listener.accept().await.unwrap();
            let (_, mut send) = stream.split();
            send.write_all(b"{\"partial").await.unwrap();
            send.flush().await.unwrap();
            drop(send);
            // 3: a peer that sends more than the limit and never a newline, then stays open.
            let stream = bound.listener.accept().await.unwrap();
            let (recv, mut send) = stream.split();
            send.write_all("c".repeat(LIMIT * 4).as_bytes())
                .await
                .unwrap();
            send.flush().await.unwrap();
            tokio::time::sleep(Duration::from_secs(5)).await;
            drop((recv, send));
        });
        let open = |endpoint: String| async move {
            Client {
                conn: {
                    let mut conn = connect(&endpoint).await.unwrap();
                    conn.max_line = LIMIT;
                    conn
                },
                next_id: 1,
            }
        };
        let mut c = open(endpoint.clone()).await;
        assert_eq!(
            c.raw_line().await.unwrap().map(|l| l.len()),
            Some(LIMIT - 1)
        );
        assert_eq!(
            c.raw_line().await,
            Err(CallError::TooLarge { limit: LIMIT }),
            "one byte over the limit"
        );
        let mut c = open(endpoint.clone()).await;
        assert_eq!(
            c.raw_line().await,
            Err(CallError::Unavailable("connection lost".into())),
            "a peer that stops inside a line"
        );
        let mut c = open(endpoint).await;
        let refused = tokio::time::timeout(Duration::from_secs(2), c.next_event())
            .await
            .expect("bounded: no wait for a newline that never comes");
        assert_eq!(refused, Err(CallError::TooLarge { limit: LIMIT }));
        assert_eq!(refused.unwrap_err().kind(), ErrorKind::Internal);
        server.abort();
    }

    #[tokio::test]
    async fn line_round_trip_over_the_real_transport() {
        let dir = tempfile::tempdir().unwrap();
        crate::paths::ensure_private_dir(dir.path()).unwrap();
        let bound = bind(dir.path()).unwrap();
        let endpoint = bound.endpoint.clone();
        let server = tokio::spawn(async move {
            let stream = bound.listener.accept().await.unwrap();
            let mut conn = Connection::new(stream);
            let line = conn.read_line().await.unwrap().unwrap();
            let req: RpcRequest = serde_json::from_str(&line).unwrap();
            conn.write_json(&RpcResponse::ok(
                Some(req.id),
                serde_json::json!({"echo": req.method}),
            ))
            .await
            .unwrap();
        });
        let mut client = Client::connect_endpoint(&endpoint).await.unwrap();
        let reply = client
            .call("service.v1.hello", serde_json::json!({}))
            .await
            .unwrap();
        assert_eq!(reply["echo"], "service.v1.hello");
        server.await.unwrap();
    }

    #[tokio::test]
    async fn missing_service_is_reported_as_unavailable() {
        let dir = tempfile::tempdir().unwrap();
        let err = Client::connect(dir.path()).await.unwrap_err();
        assert_eq!(err.kind(), ErrorKind::ServiceUnavailable);
        let stale = Client::connect_endpoint(if cfg!(windows) {
            "downova-0000000000000000"
        } else {
            "/nonexistent/downova.sock"
        })
        .await
        .unwrap_err();
        assert!(matches!(stale, CallError::Unavailable(_)));
    }
}
