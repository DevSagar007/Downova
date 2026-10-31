//! JSON-RPC 2.0 dispatch for service API v1 (closed method set; no engine passthrough).

use dm_backend_api::DownloadBackend;
use dm_domain::ErrorKind;
use dm_domain::api::{
    AddDownloadParams, CheckLinkParams, CreateQueueParams, DeleteQueueParams, EmptyResult,
    HistoryPreviewParams, HistorySettings, JobRef, ListDownloadsResult, ListQueuesResult,
    MoveJobParams, QueueRef, RenameQueueParams, ReplaceSourceParams, RetryParams, RpcError,
    RpcRequest, RpcResponse, ServiceSettings, SetJobQueueParams, SetPriorityParams,
    SetQueueLimitParams, ShutdownParams, codes, methods,
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
    let is_shutdown = request.method == methods::SHUTDOWN;
    let result = dispatch(core, &request.method, request.params).await;
    // Whatever the request changed reaches subscribers before its answer is written (B2.9).
    core.publish().await;
    match result {
        // Only an accepted shutdown stops the service; a rejected one (e.g. bad params) never does.
        Ok(result) if is_shutdown => (RpcResponse::ok(id, result), After::Shutdown),
        Ok(result) => (RpcResponse::ok(id, result), After::Continue),
        Err(error) => (RpcResponse::err(id, error), After::Continue),
    }
}

async fn dispatch<B: DownloadBackend>(
    core: &Core<B>,
    method: &str,
    params: Value,
) -> Result<Value, RpcError> {
    match method {
        methods::HELLO => to_value(core.hello()),
        methods::SHUTDOWN => {
            let params: ShutdownParams = parse(params)?;
            to_value(core.begin_shutdown(params.mode()).await)
        }
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
        methods::REMOVE => service(core.remove(&parse::<JobRef>(params)?.job_id).await),
        methods::CHECK => service(
            core.check(&parse::<CheckLinkParams>(params)?.source.url)
                .await,
        ),
        methods::CONFIRM_NAME => service(core.confirm_name(&parse::<JobRef>(params)?.job_id).await),
        methods::SET_QUEUE => {
            let p: SetJobQueueParams = parse(params)?;
            service(core.set_job_queue(&p.job_id, &p.queue_id).await)
        }
        methods::SET_PRIORITY => {
            let p: SetPriorityParams = parse(params)?;
            service(core.set_priority(&p.job_id, p.priority).await)
        }
        methods::MOVE => {
            let p: MoveJobParams = parse(params)?;
            service(core.move_job(&p.job_id, p.to).await)
        }
        methods::SETTINGS_GET => {
            parse::<EmptyResult>(params)?;
            to_value(core.settings().await)
        }
        methods::SETTINGS_SET => {
            service(core.set_settings(parse::<ServiceSettings>(params)?).await)
        }
        methods::HISTORY_GET => {
            parse::<EmptyResult>(params)?;
            to_value(core.history_settings().await)
        }
        methods::HISTORY_SET => service(core.set_history(parse::<HistorySettings>(params)?).await),
        methods::HISTORY_PREVIEW => service(
            core.preview_history(parse::<HistoryPreviewParams>(params)?.retention_days)
                .await,
        ),
        methods::RETRY => service(core.retry(parse::<RetryParams>(params)?).await),
        methods::REPLACE_SOURCE => service(
            core.replace_source(parse::<ReplaceSourceParams>(params)?)
                .await,
        ),
        methods::LOCATE => service(core.locate(&parse::<JobRef>(params)?.job_id).await),
        methods::HISTORY_CLEAN => {
            parse::<EmptyResult>(params)?;
            to_value(core.clean_history().await)
        }
        methods::QUEUES_LIST => {
            parse::<EmptyResult>(params)?;
            to_value(ListQueuesResult {
                queues: core.queues().await,
            })
        }
        methods::QUEUES_CREATE => {
            let p: CreateQueueParams = parse(params)?;
            service(core.create_queue(&p.name, p.max_active).await)
        }
        methods::QUEUES_RENAME => {
            let p: RenameQueueParams = parse(params)?;
            service(core.rename_queue(&p.queue_id, &p.name).await)
        }
        methods::QUEUES_SET_LIMIT => {
            let p: SetQueueLimitParams = parse(params)?;
            service(core.set_queue_limit(&p.queue_id, p.max_active).await)
        }
        methods::QUEUES_DELETE => {
            let p: DeleteQueueParams = parse(params)?;
            service(
                core.delete_queue(&p.queue_id, p.move_to.as_deref())
                    .await
                    .map(|()| EmptyResult {}),
            )
        }
        methods::QUEUES_START => service(
            core.set_queue_running(&parse::<QueueRef>(params)?.queue_id, true)
                .await,
        ),
        methods::QUEUES_STOP => service(
            core.set_queue_running(&parse::<QueueRef>(params)?.queue_id, false)
                .await,
        ),
        // The server takes a subscribe off this path (B2.9); one arriving here is a second
        // subscription on a connection that already streams.
        methods::SUBSCRIBE => Err(RpcError::new(
            codes::INVALID_REQUEST,
            ErrorKind::InvalidRequest.to_error_data("this connection is already subscribed"),
        )),
        _ => Err(RpcError::new(
            codes::METHOD_NOT_FOUND,
            ErrorKind::InvalidRequest.to_error_data("unknown method"),
        )),
    }
}

fn parse<T: DeserializeOwned>(params: Value) -> Result<T, RpcError> {
    // The contract's params are always an object. Serde would also accept an array as a struct
    // (field order), so `[]` could pass as `{}`; refuse it here.
    if !params.is_object() {
        return Err(RpcError::new(
            codes::INVALID_PARAMS,
            ErrorKind::InvalidRequest.to_error_data("invalid params: expected an object"),
        ));
    }
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
