//! Local IPC transport: newline-delimited JSON-RPC 2.0 over a user-only endpoint (ARCH-06).
//!
//! - **Windows:** named pipe `\\.\pipe\downova-<random>`, DACL `D:P(A;;GA;;;OW)(A;;GA;;;SY)`
//!   (protected; owner and SYSTEM only — the default pipe DACL would grant Everyone read), and
//!   remote clients rejected (interprocess default).
//! - **Unix:** socket `downova.sock` inside the 0700 run directory; on Linux the socket file is
//!   additionally created with mode 0600 before `bind()`.
//!
//! One request and one response per line; a line longer than [`MAX_LINE`] closes the connection.

use std::io;
use std::path::Path;
use std::time::Duration;

use dm_domain::ErrorKind;
use dm_domain::api::{RpcError, RpcRequest, RpcResponse, codes};
use interprocess::local_socket::tokio::{Listener, RecvHalf, SendHalf, Stream, prelude::*};
use interprocess::local_socket::{ConnectOptions, ListenerOptions};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

/// Maximum accepted line (request or response) in bytes. Requests never carry file content.
pub const MAX_LINE: usize = 1024 * 1024;

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
    Ok(Connection::new(stream))
}

/// A framed connection (either side).
#[derive(Debug)]
pub struct Connection {
    reader: BufReader<RecvHalf>,
    writer: SendHalf,
}

impl Connection {
    pub fn new(stream: Stream) -> Self {
        let (recv, send) = stream.split();
        Self {
            reader: BufReader::new(recv),
            writer: send,
        }
    }

    /// Reads one line. `Ok(None)` on clean end of stream.
    pub async fn read_line(&mut self) -> io::Result<Option<String>> {
        let mut buf = Vec::new();
        let mut limited = (&mut self.reader).take(u64::try_from(MAX_LINE).unwrap_or(u64::MAX));
        let n = limited.read_until(b'\n', &mut buf).await?;
        if n == 0 {
            return Ok(None);
        }
        if !buf.ends_with(b"\n") {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "line too long or truncated",
            ));
        }
        buf.pop();
        String::from_utf8(buf)
            .map(Some)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "line is not UTF-8"))
    }

    /// Writes one JSON value as a line.
    pub async fn write_json<T: serde::Serialize>(&mut self, value: &T) -> io::Result<()> {
        let mut line = serde_json::to_vec(value).map_err(io::Error::other)?;
        line.push(b'\n');
        self.writer.write_all(&line).await?;
        self.writer.flush().await
    }
}

/// Client-side failure: either the service could not be reached/understood, or it answered with
/// a JSON-RPC error (which carries the stable [`dm_domain::ErrorData`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallError {
    /// No service, stale endpoint, broken connection or malformed reply.
    Unavailable(String),
    /// The service answered with an error.
    Rpc(RpcError),
}

impl CallError {
    /// Stable kind for presentation (`SERVICE_UNAVAILABLE` for transport problems).
    pub fn kind(&self) -> ErrorKind {
        match self {
            CallError::Unavailable(_) => ErrorKind::ServiceUnavailable,
            CallError::Rpc(e) => e.data.as_ref().map_or(ErrorKind::Internal, |d| d.kind),
        }
    }
}

impl std::fmt::Display for CallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CallError::Unavailable(why) => write!(f, "download service unavailable: {why}"),
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
            .map_err(|_| CallError::Unavailable("connection lost".into()))?
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
