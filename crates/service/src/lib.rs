//! Background download coordinator.
//!
//! The service owns job identity, queues, policy, retry classification, secret retrieval and the
//! backend lifecycle. It will be the only SQLite writer (B1). File bytes never pass through it:
//! they flow origin server → backend → local disk. See docs/architecture.md.
//!
//! B0.2/B1.1: [`core`] (jobs and idempotency, durable through `dm-storage` since B1.1; capability gates, attempt guards),
//! [`rpc`] (JSON-RPC 2.0 dispatch) and [`server`] (IPC accept loop + progress poller). The
//! `dm-service` binary in `src/main.rs` wires them to [`router::EngineRouter`]: the native engine,
//! plus the yt-dlp media engine when installed (ADR 0043).
//! The backend interface lives in `dm-backend-api` (ADR 0002).

pub mod core;
pub mod events;
pub mod hello;
mod naming;
pub mod notify;
pub mod router;
pub mod rpc;
pub mod server;
pub mod space;

#[cfg(test)]
pub(crate) mod fake;

pub use dm_backend_api::{
    BackendError, BackendHandle, DownloadBackend, SpaceGuard, TransferRequest, require_capability,
};
pub use hello::hello_result;
pub use router::EngineRouter;
