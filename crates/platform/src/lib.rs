//! OS adapters shared by the service and its clients (docs/architecture.md §8).
//!
//! B0.2 scope: per-user data paths, the endpoint discovery file, and the local IPC transport
//! (Windows named pipe with an owner-only DACL, Unix-domain socket in a 0700 directory).
//! Later: credential store, volume identity, network cost hints, notifications, host registration.

pub mod endpoint;
pub mod ipc;
pub mod launch;
pub mod paths;

/// Random lowercase-hex string from the OS CSPRNG.
pub fn random_hex(bytes: usize) -> std::io::Result<String> {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).map_err(|e| std::io::Error::other(format!("random: {e}")))?;
    Ok(buf.iter().map(|b| format!("{b:02x}")).collect())
}
