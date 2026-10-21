//! Background download coordinator (foundation).
//!
//! The service owns job identity, queues, policy, retry classification, secret retrieval and the
//! backend lifecycle. It is the only SQLite writer. File bytes never pass through it: they flow
//! origin server → backend (aria2) → local disk. See docs/architecture.md.
//!
//! The backend interface lives in `dm-backend-api` (ADR 0002) so engine adapters never depend on
//! the service. IPC and persistence arrive in milestone B0.2/B1 (see docs/roadmap.md).

pub mod hello;

pub use dm_backend_api::{
    BackendError, BackendHandle, DownloadBackend, TransferRequest, require_capability,
};
pub use hello::hello_result;
