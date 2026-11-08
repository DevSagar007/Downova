//! Shared domain types and policies.
//!
//! This crate has no I/O and no dependency on any other `dm-*` crate, the UI, Tauri, SQLite or
//! aria2. Everything else depends on it. Wire types in [`api`] mirror the canonical schema in
//! `packages/contracts/schema/service-api.v1.schema.json`; the test
//! `tests/contract_examples.rs` keeps them in sync.

pub mod api;
pub mod bytes;
pub mod capability;
pub mod cookies;
pub mod error_kind;
pub mod ids;
pub mod policy;
pub mod redact;
pub mod retry;
pub mod schedule;
pub mod state;
pub mod validate;

pub use bytes::ByteCount;

/// The version every Downova component reports (REL-01, B3.6): the workspace version, or the
/// `DOWNOVA_BUILD_VERSION` given at compile time (`scripts/package.mjs --app-version`, fixture
/// builds only), so the desktop, service, CLI and native host of one package always agree.
pub const BUILD_VERSION: &str = match option_env!("DOWNOVA_BUILD_VERSION") {
    Some(v) => v,
    None => env!("CARGO_PKG_VERSION"),
};
pub use capability::{Capability, CapabilityReport, CapabilitySupport};
pub use error_kind::{ErrorData, ErrorKind, NextAction};
pub use ids::{AttemptId, IdError, JobId};
pub use state::{Blocker, DesiredState, HoldReason, Phase, RunDecision, decide_run};
