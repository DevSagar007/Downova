//! Native-messaging host for Downova (B0.6, BRW-02; Firefox since B3.4, ADR 0029).
//!
//! The browser launches this program and exchanges length-prefixed JSON messages on stdin/stdout.
//! Chrome-family browsers pass the calling extension's origin (`chrome-extension://<id>/`, plus
//! `--parent-window=<n>` on Windows); Firefox passes the path of the host manifest it read and
//! then the calling add-on's ID. The host:
//! - accepts callers only from an exact allowlist: extension origins for Chrome-family browsers,
//!   add-on IDs for Firefox (what the browser passes on the command line, never anything claimed
//!   inside a message; a manifest path is not an identity);
//! - accepts only the `NativeHostRequest` contract (`hello`, `add`) — no arbitrary service RPC;
//! - forwards `add` as `downloads.v1.add` over the private per-user IPC endpoint of an already
//!   running service (it never starts the service: see [`handler::NOT_RUNNING`]);
//! - never carries file bytes, cookies or headers; stdout carries only framed responses, and
//!   diagnostics on stderr never contain URLs.
//!
//! The same executable provides per-user `register`, `repair`, `unregister` and `status`
//! commands for development and packaging.

pub mod detect;
pub mod framing;
pub mod handler;
pub mod registration;

/// Development extension ID, derived from the public key in `apps/extension/manifest.json`.
pub const DEV_EXTENSION_ID: &str = "bkmpnfclpdmcbdgmeakmnoilmkdmdppf";

/// Development Firefox add-on ID (ADR 0029): `browser_specific_settings.gecko.id` of the Firefox
/// build. A distribution ID (AMO or self-hosted signing) is added here when one is assigned.
pub const FIREFOX_DEV_ADDON_ID: &str = "downova-dev@extension.downova.invalid";

/// Exact Firefox add-on IDs allowed to call this host (`allowed_extensions`).
pub fn allowed_extensions() -> Vec<String> {
    vec![FIREFOX_DEV_ADDON_ID.to_owned()]
}

/// True only for an exact, configured add-on ID.
pub fn extension_allowed(id: &str) -> bool {
    allowed_extensions().iter().any(|e| e == id)
}

/// Who started the host, from its command line (never from a message).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Caller {
    /// A Chrome-family extension origin on the allowlist.
    Chromium(String),
    /// A Firefox add-on ID on the allowlist.
    Firefox(String),
}

/// Why a launch was refused (for the host's stderr; never echoes the arguments).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// A Chrome-family origin that is not on the allowlist.
    UnknownOrigin,
    /// Firefox-style arguments without an allowed add-on ID (missing, malformed or unknown).
    UnknownAddon,
    /// Neither form.
    Unrecognized,
}

/// Classifies the launch arguments. Chrome: `chrome-extension://<id>/ [--parent-window=<n>]`.
/// Firefox: `<absolute path to com.downova.host.json> <add-on id>`; the path is only checked for
/// shape (it is not an identity), the ID must be exactly on the allowlist.
pub fn caller(args: &[String]) -> Result<Caller, Refusal> {
    let Some(first) = args.first() else {
        return Err(Refusal::Unrecognized);
    };
    if first.starts_with("chrome-extension://") {
        return if origin_allowed(first) {
            Ok(Caller::Chromium(first.clone()))
        } else {
            Err(Refusal::UnknownOrigin)
        };
    }
    let manifest = std::path::Path::new(first);
    let is_manifest = manifest.is_absolute()
        && manifest
            .file_name()
            .is_some_and(|n| n.eq_ignore_ascii_case("com.downova.host.json"));
    if !is_manifest {
        return Err(Refusal::Unrecognized);
    }
    match args.get(1) {
        Some(id) if args.len() == 2 && extension_allowed(id) => Ok(Caller::Firefox(id.clone())),
        _ => Err(Refusal::UnknownAddon),
    }
}

/// The caller allowlist a browser's host manifest names: add-on IDs (`allowed_extensions`) for
/// Firefox, extension origins (`allowed_origins`) for Chrome-family browsers.
pub fn callers_for(browser: registration::Browser) -> Vec<String> {
    if browser.is_firefox() {
        allowed_extensions()
    } else {
        allowed_origins()
    }
}

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

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn firefox_launches_are_identified_by_the_add_on_id_only() {
        let manifest = if cfg!(windows) {
            r"C:\Users\u\AppData\Local\Downova\native-messaging\firefox\com.downova.host.json"
        } else {
            "/home/u/.mozilla/native-messaging-hosts/com.downova.host.json"
        };
        assert_eq!(
            caller(&args(&[manifest, FIREFOX_DEV_ADDON_ID])),
            Ok(Caller::Firefox(FIREFOX_DEV_ADDON_ID.into()))
        );
        for (bad, why) in [
            (args(&[manifest]), Refusal::UnknownAddon),
            (args(&[manifest, ""]), Refusal::UnknownAddon),
            (
                args(&[manifest, "other@example.org"]),
                Refusal::UnknownAddon,
            ),
            (
                args(&[manifest, &FIREFOX_DEV_ADDON_ID.to_uppercase()]),
                Refusal::UnknownAddon,
            ),
            (
                args(&[manifest, FIREFOX_DEV_ADDON_ID, "extra"]),
                Refusal::UnknownAddon,
            ),
            // A relative path or another file is not a Firefox launch.
            (
                args(&["com.downova.host.json", FIREFOX_DEV_ADDON_ID]),
                Refusal::Unrecognized,
            ),
            (
                args(&["/tmp/other.json", FIREFOX_DEV_ADDON_ID]),
                Refusal::Unrecognized,
            ),
            // The add-on ID alone, or as a Chrome origin, is not accepted.
            (args(&[FIREFOX_DEV_ADDON_ID]), Refusal::Unrecognized),
            (
                args(&["chrome-extension://downova-dev@extension.downova.invalid/"]),
                Refusal::UnknownOrigin,
            ),
            (args(&[]), Refusal::Unrecognized),
        ] {
            assert_eq!(caller(&bad), Err(why), "{bad:?}");
        }
        // Chrome is unchanged: its origin, with or without the Windows parent-window argument.
        let chrome = "chrome-extension://bkmpnfclpdmcbdgmeakmnoilmkdmdppf/";
        assert_eq!(
            caller(&args(&[chrome, "--parent-window=0"])),
            Ok(Caller::Chromium(chrome.into()))
        );
        assert_eq!(
            caller(&args(&["moz-extension://x/", FIREFOX_DEV_ADDON_ID])),
            Err(Refusal::Unrecognized)
        );
    }
}
