//! JSON-RPC 2.0 dispatch for service API v1 (closed method set; no engine passthrough).

use dm_backend_api::DownloadBackend;
use dm_domain::ErrorKind;
use dm_domain::api::{
    AddDownloadParams, EmptyResult, JobRef, ListDownloadsResult, RpcError, RpcRequest, RpcResponse,
    codes, methods,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::core::{Core, ServiceError};

/// What the connection loop should do after answering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum After {
    Continue,
    Shutdown,
}

/// Handles one raw request line and returns the response to send.
pub async fn handle_line<B: DownloadBackend>(core: &Core<B>, line: &str) -> (RpcResponse, After) {
    let request: RpcRequest = match serde_json::from_str(line) {
        Ok(r) => r,
        Err(_) => {
            // Try to keep the id for correlation even when the shape is wrong.
            let id = serde_json::from_str::<Value>(line)
                .ok()
                .and_then(|v| v.get("id").and_then(Value::as_str).map(str::to_owned));
            let code = if id.is_some() {
                codes::INVALID_REQUEST
            } else {
                codes::PARSE_ERROR
            };
            return (
                dm_platform::ipc::protocol_error(id, code, "malformed JSON-RPC request"),
                After::Continue,
            );
        }
    };
    let id = Some(request.id.clone());
    if request.jsonrpc != "2.0" {
        return (
            dm_platform::ipc::protocol_error(id, codes::INVALID_REQUEST, "jsonrpc must be \"2.0\""),
            After::Continue,
        );
    }
    let after = if request.method == methods::SHUTDOWN {
        After::Shutdown
    } else {
        After::Continue
    };
    let response = match dispatch(core, &request.method, request.params).await {
        Ok(result) => RpcResponse::ok(id, result),
        Err(error) => RpcResponse::err(id, error),
    };
    (response, after)
}

async fn dispatch<B: DownloadBackend>(
    core: &Core<B>,
    method: &str,
    params: Value,
) -> Result<Value, RpcError> {
    match method {
        methods::HELLO => to_value(core.hello()),
        methods::SHUTDOWN => to_value(EmptyResult {}),
        methods::ADD => {
            let params: AddDownloadParams = parse(params)?;
            service(core.add(params).await)
        }
        methods::LIST => to_value(ListDownloadsResult {
            items: core.list().await,
        }),
        methods::GET => service(core.get(&parse::<JobRef>(params)?.job_id).await),
        methods::PAUSE => service(core.pause(&parse::<JobRef>(params)?.job_id).await),
        methods::RESUME => service(core.resume(&parse::<JobRef>(params)?.job_id).await),
        methods::CANCEL => service(core.cancel(&parse::<JobRef>(params)?.job_id).await),
        _ => Err(RpcError::new(
            codes::METHOD_NOT_FOUND,
            ErrorKind::InvalidRequest.to_error_data("unknown method"),
        )),
    }
}

fn parse<T: DeserializeOwned>(params: Value) -> Result<T, RpcError> {
    serde_json::from_value(params).map_err(|e| {
        // serde messages name fields and types, never values, so they are safe to return.
        RpcError::new(
            codes::INVALID_PARAMS,
            ErrorKind::InvalidRequest.to_error_data(format!("invalid params: {e}")),
        )
    })
}

fn to_value<T: Serialize>(value: T) -> Result<Value, RpcError> {
    serde_json::to_value(value).map_err(|_| {
        RpcError::new(
            codes::SERVICE_ERROR,
            ErrorKind::Internal.to_error_data("cannot encode result"),
        )
    })
}

fn service<T: Serialize>(result: Result<T, ServiceError>) -> Result<Value, RpcError> {
    match result {
        Ok(value) => to_value(value),
        Err(ServiceError(data)) => {
            let code = if data.kind == ErrorKind::InvalidRequest {
                codes::INVALID_PARAMS
            } else {
                codes::SERVICE_ERROR
            };
            Err(RpcError::new(code, data))
        }
    }
}
