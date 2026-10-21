//! Background download coordinator.
//!
//! The service owns job identity, queues, policy, retry classification, secret retrieval and the
//! backend lifecycle. It will be the only SQLite writer (B1). File bytes never pass through it:
//! they flow origin server → backend (aria2) → local disk. See docs/architecture.md.
//!
//! B0.2: [`core`] (in-memory jobs, in-memory idempotency, capability gates, attempt guards),
//! [`rpc`] (JSON-RPC 2.0 dispatch) and [`server`] (IPC accept loop + progress poller). The
//! `dm-service` binary in `src/main.rs` wires them to the aria2 backend.
//! The backend interface lives in `dm-backend-api` (ADR 0002).

pub mod core;
pub mod hello;
pub mod rpc;
pub mod server;

#[cfg(test)]
pub(crate) mod fake;

pub use dm_backend_api::{
    BackendError, BackendHandle, DownloadBackend, TransferRequest, require_capability,
};
pub use hello::hello_result;
