//! Isolated IPC test peer: a scripted JSON-RPC endpoint on the real platform transport, published
//! through the same endpoint file the service writes. It lets CLI tests use controlled responses
//! (including successful job snapshots) without any production capability bypass.

#![allow(dead_code)]

use std::path::Path;
use std::sync::{Arc, Mutex};

use dm_domain::ErrorKind;
use dm_domain::api::{RpcError, RpcRequest, RpcResponse, methods};
use dm_platform::endpoint::{self, EndpointInfo};
use dm_platform::ipc;
use serde_json::{Value, json};

/// What the peer does with one request.
pub enum Reply {
    Result(Value),
    Error(RpcError),
    /// Close the connection without answering (lost acknowledgement).
    Drop,
    /// Never answer (timeout).
    Hang,
}

pub type Script = Arc<dyn Fn(&RpcRequest, usize) -> Reply + Send + Sync>;

#[derive(Clone, Default)]
pub struct Log(pub Arc<Mutex<Vec<(usize, RpcRequest)>>>);

impl Log {
    pub fn requests(&self) -> Vec<(usize, RpcRequest)> {
        self.0.lock().unwrap().clone()
    }
    pub fn methods(&self) -> Vec<String> {
        self.requests().into_iter().map(|(_, r)| r.method).collect()
    }
    pub fn app_requests(&self) -> Vec<RpcRequest> {
        self.requests()
            .into_iter()
            .map(|(_, r)| r)
            .filter(|r| r.method != methods::HELLO)
            .collect()
    }
}

pub fn hello(api_version: u32) -> Value {
    json!({
        "api_version": api_version,
        "service_version": "0.1.0",
        "service_instance_id": "svc_peer",
        "backend": { "id": "peer", "version": null, "capabilities": [] }
    })
}

pub fn snapshot(job_id: &str) -> Value {
    json!({
        "job_id": job_id,
        "attempt_id": "att_peer0001",
        "display_name": "file.zip",
        "source_display": "https://cdn.example.org/file.zip?token=REDACTED",
        "desired_state": "running",
        "phase": "transferring",
        "blockers": ["metered_network"],
        "downloaded_bytes": "1048576",
        "total_bytes": "2097152",
        "error": null
    })
}

pub fn rpc_error(code: i64, kind: ErrorKind, detail: &str) -> RpcError {
    RpcError::new(code, kind.to_error_data(detail))
}

/// Starts the peer in `data_dir` (writes the endpoint file). Hello answers with `api_version 1`
/// unless the script handles it.
pub fn start(data_dir: &Path, script: Script) -> Log {
    ipc_dirs(data_dir);
    let bound = ipc::bind(&dm_platform::paths::run_dir(data_dir)).unwrap();
    endpoint::write(
        data_dir,
        &EndpointInfo {
            version: 1,
            endpoint: bound.endpoint.clone(),
            pid: std::process::id(),
        },
    )
    .unwrap();
    let log = Log::default();
    let task_log = log.clone();
    tokio::spawn(async move {
        let mut index = 0usize;
        loop {
            let Ok(mut conn) = bound.accept().await else {
                return;
            };
            index += 1;
            let script = Arc::clone(&script);
            let log = task_log.clone();
            let conn_index = index;
            tokio::spawn(async move {
                while let Ok(Some(line)) = conn.read_line().await {
                    let req: RpcRequest = serde_json::from_str(&line).unwrap();
                    log.0.lock().unwrap().push((conn_index, req.clone()));
                    let id = Some(req.id.clone());
                    let reply = match script(&req, conn_index) {
                        Reply::Result(v) => RpcResponse::ok(id, v),
                        Reply::Error(e) => RpcResponse::err(id, e),
                        Reply::Drop => return,
                        Reply::Hang => {
                            std::future::pending::<()>().await;
                            return;
                        }
                    };
                    if conn.write_json(&reply).await.is_err() {
                        return;
                    }
                }
            });
        }
    });
    log
}

/// Default script: hello v1; job methods echo a snapshot for the requested id; list returns one.
pub fn happy() -> Script {
    Arc::new(|req: &RpcRequest, _| match req.method.as_str() {
        methods::HELLO => Reply::Result(hello(1)),
        methods::LIST => Reply::Result(json!({ "items": [snapshot("job_peer0001")] })),
        methods::ADD => Reply::Result(snapshot("job_peer0001")),
        _ => {
            let id = req.params["job_id"]
                .as_str()
                .unwrap_or("job_peer0001")
                .to_owned();
            Reply::Result(snapshot(&id))
        }
    })
}

fn ipc_dirs(data_dir: &Path) {
    dm_platform::paths::ensure_private_dir(data_dir).unwrap();
    dm_platform::paths::ensure_private_dir(&dm_platform::paths::run_dir(data_dir)).unwrap();
}
