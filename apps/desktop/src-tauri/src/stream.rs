//! Service event stream into the window (B2.9, ADR 0016).
//!
//! `events_subscribe` opens a dedicated connection, subscribes (`events.v1.subscribe`) and
//! returns the authoritative list with its sequence and a subscription number. A task then
//! forwards every event to the window as [`SERVICE_EVENT`] tagged with that number, and finally
//! sends [`STREAM_ENDED`] when the stream stops (service gone, `snapshot_required`, protocol
//! error). Only one stream exists per window: subscribing again ends the previous one, and the UI
//! ignores events whose number is not its current one. Nothing here starts the service.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use dm_domain::api::{EventKind, ServiceEvent, SubscribeResult};
use dm_platform::ipc::Client;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime};

use crate::downloads::{BridgeError, CONNECT_TIMEOUT, LIST_TIMEOUT};

/// Window event carrying one service event.
pub const SERVICE_EVENT: &str = "downova://service-event";
/// Window event: the stream of a subscription ended (the UI decides whether to subscribe again).
pub const STREAM_ENDED: &str = "downova://service-stream-ended";

#[derive(Debug, Serialize)]
pub struct Subscribed {
    pub subscription: u64,
    pub start: SubscribeResult,
}

#[derive(Debug, Clone, Serialize)]
struct Forwarded {
    subscription: u64,
    event: ServiceEvent,
}

#[derive(Debug, Clone, Serialize)]
struct Ended {
    subscription: u64,
    /// `closed` (the service ended the stream), `snapshot_required`, or `lost`.
    reason: &'static str,
}

/// The window's single forwarding task.
#[derive(Debug, Default)]
pub struct Streams {
    next: AtomicU64,
    current: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
}

impl Streams {
    fn replace(&self, task: Option<tauri::async_runtime::JoinHandle<()>>) {
        if let Ok(mut slot) = self.current.lock()
            && let Some(old) = std::mem::replace(&mut *slot, task)
        {
            old.abort();
        }
    }

    /// Ends the current stream, if any.
    pub fn stop(&self) {
        self.replace(None);
    }
}

pub async fn subscribe<R: Runtime>(
    app: AppHandle<R>,
    streams: &Streams,
) -> Result<Subscribed, BridgeError> {
    let data_dir = dm_platform::paths::data_dir().map_err(|_| BridgeError::no_data_dir())?;
    let mut client = match tokio::time::timeout(CONNECT_TIMEOUT, Client::connect(&data_dir)).await {
        Ok(Ok(c)) => c,
        _ => {
            return Err(BridgeError::unavailable_now(
                "the download service is not running",
            ));
        }
    };
    let start = match tokio::time::timeout(LIST_TIMEOUT, client.subscribe()).await {
        Ok(Ok(start)) => start,
        Ok(Err(dm_platform::ipc::CallError::Rpc(rpc))) => {
            // An older service without events.v1.subscribe: the UI falls back to polling.
            return Err(BridgeError::rejected(rpc.data.unwrap_or_else(|| {
                dm_domain::ErrorKind::InvalidRequest.to_error_data("subscription refused")
            })));
        }
        _ => {
            return Err(BridgeError::unavailable_now(
                "the download service did not answer",
            ));
        }
    };
    let subscription = streams
        .next
        .fetch_add(1, Ordering::SeqCst)
        .saturating_add(1);
    let task = tauri::async_runtime::spawn(async move {
        let reason = loop {
            match client.next_event().await {
                Ok(Some(event)) => {
                    let required = event.kind == EventKind::SnapshotRequired;
                    let _ = app.emit(
                        SERVICE_EVENT,
                        Forwarded {
                            subscription,
                            event,
                        },
                    );
                    if required {
                        break "snapshot_required";
                    }
                }
                Ok(None) => break "closed",
                Err(_) => break "lost",
            }
        };
        let _ = app.emit(
            STREAM_ENDED,
            Ended {
                subscription,
                reason,
            },
        );
    });
    streams.replace(Some(task));
    Ok(Subscribed {
        subscription,
        start,
    })
}
