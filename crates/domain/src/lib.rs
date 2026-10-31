//! Shared domain types and policies.
//!
//! This crate has no I/O and no dependency on any other `dm-*` crate, the UI, Tauri, SQLite or
//! aria2. Everything else depends on it. Wire types in [`api`] mirror the canonical schema in
//! `packages/contracts/schema/service-api.v1.schema.json`; the test
//! `tests/contract_examples.rs` keeps them in sync.

pub mod api;
pub mod bytes;
pub mod capability;
pub mod error_kind;
pub mod ids;
pub mod policy;
pub mod redact;
pub mod retry;
pub mod state;
pub mod validate;

pub use bytes::ByteCount;
pub use capability::{Capability, CapabilityReport, CapabilitySupport};
pub use error_kind::{ErrorData, ErrorKind, NextAction};
pub use ids::{AttemptId, IdError, JobId};
pub use state::{Blocker, DesiredState, HoldReason, Phase, RunDecision, decide_run};
