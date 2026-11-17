//! "Add to browser" (ADR 0041): without a Chrome Web Store listing, Chrome and Edge accept the
//! extension only through their own extensions page ("Load unpacked", developer mode). Downova
//! ships the extension beside itself (`<resources>/extension`) and helps the user do it:
//! - [`open_browser`] starts the installed browser **with no argument** (a new window or tab).
//!   Chrome and Edge ignore their own `chrome://`/`edge://` pages given on the command line
//!   (checked on Chrome 157 and Edge 157), so the webview offers to copy the extensions page
//!   address for the user to paste;
//! - the folder to pick is shown with the opener plugin (never executed).
//!
//! The webview receives categories only, never a path.

use std::path::{Path, PathBuf};

use serde::Serialize;

/// Browsers this guide covers (Firefox needs a signed add-on: not offered here).
pub const GUIDED: [&str; 2] = ["chrome", "edge"];

/// What the guide can offer on this computer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AddInfo {
    /// The extension folder is installed beside Downova (packaged builds).
    pub extension_bundled: bool,
    /// Guided browsers whose executable was found.
    pub browsers_found: Vec<String>,
}

/// The browser's own extensions page. Chrome and Edge ignore it on the command line, so it is only
/// copied for the user to paste (never passed to the browser).
pub fn extensions_page(browser: &str) -> Option<&'static str> {
    match browser {
        "chrome" => Some("chrome://extensions"),
        "edge" => Some("edge://extensions"),
        _ => None,
    }
}

/// Fixed install locations of each browser (per user first, then per machine). No PATH search,
/// no registry-provided command line: only these absolute paths.
pub fn candidates(browser: &str, env: &dyn Fn(&str) -> Option<String>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if cfg!(windows) {
        let roots = [
            env("LOCALAPPDATA"),
            env("ProgramFiles"),
            env("ProgramFiles(x86)"),
        ];
        let rel = match browser {
            "chrome" => r"Google\Chrome\Application\chrome.exe",
            "edge" => r"Microsoft\Edge\Application\msedge.exe",
            _ => return out,
        };
        for root in roots.into_iter().flatten() {
            out.push(Path::new(&root).join(rel));
        }
    } else if cfg!(target_os = "macos") {
        let app = match browser {
            "chrome" => "Google Chrome.app/Contents/MacOS/Google Chrome",
            "edge" => "Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
            _ => return out,
        };
        out.push(Path::new("/Applications").join(app));
        if let Some(home) = env("HOME") {
            out.push(Path::new(&home).join("Applications").join(app));
        }
    } else {
        let names: &[&str] = match browser {
            "chrome" => &["google-chrome", "google-chrome-stable"],
            "edge" => &["microsoft-edge", "microsoft-edge-stable"],
            _ => return out,
        };
        for n in names {
            out.push(Path::new("/usr/bin").join(n));
        }
    }
    out
}

fn real_env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

/// The first existing executable for `browser`.
pub fn find_browser(browser: &str) -> Option<PathBuf> {
    candidates(browser, &real_env)
        .into_iter()
        .find(|p| p.is_file())
}

/// The bundled extension folder, when it holds a manifest.
pub fn extension_dir(resource_dir: Option<PathBuf>) -> Option<PathBuf> {
    let dir = resource_dir?.join("extension");
    dir.join("manifest.json").is_file().then_some(dir)
}

pub fn info(resource_dir: Option<PathBuf>) -> AddInfo {
    AddInfo {
        extension_bundled: extension_dir(resource_dir).is_some(),
        browsers_found: GUIDED
            .iter()
            .filter(|b| find_browser(b).is_some())
            .map(|b| (*b).to_owned())
            .collect(),
    }
}

/// Starts the installed browser with no argument (a new window, or a new tab when it runs).
pub fn open_browser(browser: &str) -> Result<(), &'static str> {
    if !GUIDED.contains(&browser) {
        return Err("not a guided browser");
    }
    let exe = find_browser(browser).ok_or("the browser was not found on this computer")?;
    std::process::Command::new(exe)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|_| "the browser could not be started")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_guided_browsers_open_and_never_with_an_argument() {
        assert_eq!(extensions_page("chrome"), Some("chrome://extensions"));
        assert_eq!(extensions_page("edge"), Some("edge://extensions"));
        for other in ["firefox", "chromium", "", "chrome://settings"] {
            assert_eq!(extensions_page(other), None, "{other}");
        }
        assert!(open_browser("firefox").is_err());
        assert!(open_browser("../chrome").is_err());
        // The source passes no argument to the browser.
        let src = include_str!("browser_add.rs");
        let body =
            &src[src.find("pub fn open_browser").unwrap()..src.find("#[cfg(test)]").unwrap()];
        assert!(!body.contains(".arg("), "no argument is ever passed");
    }

    #[test]
    fn browsers_are_looked_up_only_at_fixed_absolute_locations() {
        let env = |k: &str| match k {
            "LOCALAPPDATA" => Some(r"C:\Users\u\AppData\Local".to_owned()),
            "ProgramFiles" => Some(r"C:\Program Files".to_owned()),
            "ProgramFiles(x86)" => None,
            "HOME" => Some("/home/u".to_owned()),
            _ => None,
        };
        for b in GUIDED {
            let c = candidates(b, &env);
            assert!(!c.is_empty(), "{b}");
            assert!(
                c.iter()
                    .all(|p| p.is_absolute() || !cfg!(unix) || p.starts_with("/"))
            );
        }
        assert!(candidates("firefox", &env).is_empty());
        if cfg!(windows) {
            assert_eq!(
                candidates("edge", &env)[1],
                PathBuf::from(r"C:\Program Files\Microsoft\Edge\Application\msedge.exe")
            );
        }
    }

    #[test]
    fn the_extension_folder_counts_only_with_a_manifest() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(extension_dir(Some(dir.path().to_path_buf())), None);
        assert_eq!(extension_dir(None), None);
        std::fs::create_dir_all(dir.path().join("extension")).unwrap();
        std::fs::write(dir.path().join("extension/manifest.json"), "{}").unwrap();
        assert_eq!(
            extension_dir(Some(dir.path().to_path_buf())),
            Some(dir.path().join("extension"))
        );
        let i = info(Some(dir.path().to_path_buf()));
        assert!(i.extension_bundled);
        let json = serde_json::to_string(&i).unwrap();
        assert!(
            !json.contains(&*dir.path().to_string_lossy()),
            "no path reaches the webview"
        );
    }
}
