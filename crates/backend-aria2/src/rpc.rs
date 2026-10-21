//! Minimal aria2 JSON-RPC client over loopback HTTP.
//!
//! Every call carries `token:<secret>`. The client ignores system proxies: RPC must never leave
//! 127.0.0.1. Error messages from aria2 are not forwarded verbatim because they may echo URLs.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use dm_backend_api::BackendError;
use dm_domain::ErrorKind;
use dm_domain::redact::Secret;
use serde_json::{Value, json};

#[derive(Debug)]
pub struct RpcClient {
    http: reqwest::Client,
    endpoint: String,
    token: Secret,
    next_id: AtomicU64,
}

impl RpcClient {
    pub fn new(port: u16, secret: &Secret) -> Result<Self, BackendError> {
        let http = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| BackendError::new(ErrorKind::Internal, format!("rpc client: {e}")))?;
        Ok(Self {
            http,
            endpoint: format!("http://127.0.0.1:{port}/jsonrpc"),
            token: Secret::new(format!("token:{}", secret.expose())),
            next_id: AtomicU64::new(1),
        })
    }

    /// Calls `method` with the secret token prepended to `params`.
    pub async fn call(&self, method: &str, params: Vec<Value>) -> Result<Value, BackendError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let mut all = Vec::with_capacity(params.len().saturating_add(1));
        all.push(Value::String(self.token.expose().to_owned()));
        all.extend(params);
        let body =
            json!({ "jsonrpc": "2.0", "id": id.to_string(), "method": method, "params": all });

        let response = self
            .http
            .post(&self.endpoint)
            .json(&body)
            .send()
            .await
            .map_err(|e| BackendError::new(ErrorKind::ServiceUnavailable, transport(&e)))?;
        let reply: Value = response
            .json()
            .await
            .map_err(|_| BackendError::new(ErrorKind::Internal, "aria2 rpc: invalid JSON reply"))?;
        if let Some(error) = reply.get("error") {
            let code = error
                .get("code")
                .and_then(Value::as_i64)
                .unwrap_or_default();
            return Err(BackendError::new(
                ErrorKind::Internal,
                format!("aria2 rpc {method} failed (code {code})"),
            ));
        }
        reply
            .get("result")
            .cloned()
            .ok_or_else(|| BackendError::new(ErrorKind::Internal, "aria2 rpc: missing result"))
    }
}

fn transport(e: &reqwest::Error) -> String {
    if e.is_timeout() {
        "aria2 rpc timed out".to_owned()
    } else if e.is_connect() {
        "aria2 rpc not reachable".to_owned()
    } else {
        "aria2 rpc transport error".to_owned()
    }
}
