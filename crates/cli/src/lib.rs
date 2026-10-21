//! `dm` — Downova command-line client (B0.3).
//!
//! A thin client of `dm-service`: it parses arguments ([`args`]), talks JSON-RPC over the local
//! IPC endpoint via `dm_platform::ipc::Client` ([`run`]) and formats results ([`output`]). It never
//! downloads, touches storage or talks to an engine, and it leaves the service running on exit.

pub mod args;
pub mod output;
pub mod run;

pub use run::{CliOutput, RunOptions, run_cli};

/// Documented exit codes (see `dm --help`).
pub mod exit {
    pub const OK: u8 = 0;
    pub const UNEXPECTED: u8 = 1;
    pub const USAGE: u8 = 2;
    pub const UNAVAILABLE: u8 = 3;
    pub const REJECTED: u8 = 4;
}
