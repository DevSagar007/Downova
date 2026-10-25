//! Chrome-family native-messaging host for Downova (B0.6, BRW-02).
//!
//! The browser launches this program with the calling extension's origin as the first argument
//! (`chrome-extension://<id>/`) and exchanges length-prefixed JSON messages on stdin/stdout. The
//! host:
//! - accepts callers only from an exact allowlist of extension origins (the origin Chrome passes
//!   on the command line, never one claimed inside a message);
//! - accepts only the `NativeHostRequest` contract (`hello`, `add`) — no arbitrary service RPC;
//! - forwards `add` as `downloads.v1.add` over the private per-user IPC endpoint, starting the
//!   `dm-service` installed beside it when none is running;
//! - never carries file bytes, cookies or headers; stdout carries only framed responses, and
//!   diagnostics on stderr never contain URLs.
//!
//! The same executable provides per-user `register`, `repair`, `unregister` and `status`
//! commands for development and packaging.

pub mod framing;
pub mod handler;
pub mod registration;

/// Development extension ID, derived from the public key in `apps/extension/manifest.json`.
pub const DEV_EXTENSION_ID: &str = "bkmpnfclpdmcbdgmeakmnoilmkdmdppf";

/// Exact origins allowed to call this host (Chrome passes them with a trailing slash).
pub fn allowed_origins() -> Vec<String> {
    vec![format!("chrome-extension://{DEV_EXTENSION_ID}/")]
}

/// True only for an exact, configured origin.
pub fn origin_allowed(origin: &str) -> bool {
    allowed_origins().iter().any(|o| o == origin)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_exact_origin_is_allowed() {
        assert!(origin_allowed(
            "chrome-extension://bkmpnfclpdmcbdgmeakmnoilmkdmdppf/"
        ));
        for bad in [
            "chrome-extension://bkmpnfclpdmcbdgmeakmnoilmkdmdppf",
            "chrome-extension://BKMPNFCLPDMCBDGMEAKMNOILMKDMDPPF/",
            "chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/",
            "chrome-extension://bkmpnfclpdmcbdgmeakmnoilmkdmdppf/x",
            "moz-extension://bkmpnfclpdmcbdgmeakmnoilmkdmdppf/",
            "",
        ] {
            assert!(!origin_allowed(bad), "{bad}");
        }
    }
}
