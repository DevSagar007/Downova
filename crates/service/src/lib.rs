//! Background download coordinator (foundation).
//!
//! The service owns job identity, queues, policy, retry classification, secret retrieval and the
//! backend lifecycle. It is the only SQLite writer. File bytes never pass through it: they flow
//! origin server → backend (aria2) → local disk. See docs/architecture.md.
//!
//! Foundation phase: only the backend boundary and capability gating exist. IPC, persistence and
//! the aria2 adapter arrive in milestone B0 (see docs/roadmap.md).

pub mod backend;
pub mod hello;

pub use backend::{BackendError, BackendHandle, DownloadBackend, TransferRequest, require_capability};
pub use hello::hello_result;
