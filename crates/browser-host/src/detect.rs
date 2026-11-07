//! Browser package detection and the support verdict per browser package (BRW-09, ADR 0029).
//!
//! A browser's *package* decides whether the native host can be reached at all: a Snap or Flatpak
//! browser runs in a sandbox that looks for host manifests elsewhere (or not at all), so "Linux
//! works" never implies "Snap Firefox works". Detection is **bounded and read-only**: a fixed list
//! of executable locations per browser is checked (existence, and on Linux the first four bytes,
//! to tell a real ELF binary from a launcher script such as Ubuntu's Snap shim). It never lists
//! directories, never reads browser profiles, and reports categories only, never paths.
//!
//! The verdict is a static, evidence-backed matrix ([`support`]): `supported` only where the
//! browser → host → service path was tested end to end for that browser and package,
//! `unsupported` where Downova's registration cannot reach the browser's sandbox, and `untested`
//! everywhere else, each with a stable reason code that clients translate.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::registration::Browser;

/// How a browser was installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Package {
    /// An ordinary installation (installer, system package, application bundle).
    Native,
    Snap,
    Flatpak,
    /// Something is there, but which kind cannot be told reliably (for example a launcher script
    /// that is not a known Snap shim).
    Unknown,
}

/// The support verdict for one browser package.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Support {
    Supported,
    Unsupported,
    Untested,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Verdict {
    pub package: Package,
    pub support: Support,
    /// Stable reason code (translated by clients).
    pub reason: &'static str,
}

/// Where detection looks: the real system, or a redirected root for tests.
#[derive(Debug, Clone)]
pub struct Probe {
    /// Prefix for absolute system paths ("/" or a test directory).
    pub root: PathBuf,
    pub home: Option<PathBuf>,
    /// Windows program directories (`ProgramFiles`, `ProgramFiles(x86)`, `LOCALAPPDATA`).
    pub windows_dirs: Vec<PathBuf>,
}

impl Probe {
    pub fn system() -> Probe {
        let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
        Probe {
            root: PathBuf::from("/"),
            home: env("HOME"),
            windows_dirs: ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"]
                .iter()
                .filter_map(|k| env(k))
                .collect(),
        }
    }

    fn sys(&self, absolute: &str) -> PathBuf {
        self.root.join(absolute.trim_start_matches('/'))
    }
}

/// The OS the verdict is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Windows,
    Macos,
    Linux,
}

impl Os {
    pub fn current() -> Os {
        if cfg!(windows) {
            Os::Windows
        } else if cfg!(target_os = "macos") {
            Os::Macos
        } else {
            Os::Linux
        }
    }
}

/// The browser family a channel belongs to, for detection and the matrix.
fn family(b: Browser) -> Browser {
    match b {
        Browser::ChromeBeta
        | Browser::ChromeDev
        | Browser::ChromeCanary
        | Browser::ChromeForTesting => Browser::Chrome,
        other => other,
    }
}

fn is_elf(path: &Path) -> bool {
    use std::io::Read;
    let mut magic = [0u8; 4];
    std::fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut magic))
        .is_ok_and(|()| magic == *b"\x7fELF")
}

/// A small launcher script that hands over to a Snap (Ubuntu's transitional packages).
fn is_snap_shim(path: &Path) -> bool {
    use std::io::Read;
    let mut head = Vec::new();
    let read = std::fs::File::open(path).and_then(|f| f.take(4096).read_to_end(&mut head));
    read.is_ok() && head.starts_with(b"#!") && head.windows(4).any(|w| w == b"snap")
}

/// The packages in which `browser` is installed (sorted, without duplicates; empty when none of
/// the known locations has it). Only the main release of each family is probed; channels report
/// their family's packages.
pub fn detect(browser: Browser, os: Os, probe: &Probe) -> Vec<Package> {
    let fam = family(browser);
    let mut out = Vec::new();
    match os {
        Os::Windows => {
            let rel: &[&str] = match fam {
                Browser::Chrome => &[r"Google\Chrome\Application\chrome.exe"],
                Browser::Edge => &[r"Microsoft\Edge\Application\msedge.exe"],
                Browser::Firefox => &[r"Mozilla Firefox\firefox.exe"],
                Browser::Chromium => &[r"Chromium\Application\chrome.exe"],
                _ => &[],
            };
            let found = probe
                .windows_dirs
                .iter()
                .any(|d| rel.iter().any(|r| d.join(r).is_file()));
            if found {
                out.push(Package::Native);
            }
        }
        Os::Macos => {
            let app = match fam {
                Browser::Chrome => "Google Chrome.app",
                Browser::Edge => "Microsoft Edge.app",
                Browser::Firefox => "Firefox.app",
                Browser::Chromium => "Chromium.app",
                _ => "",
            };
            let mut places = vec![probe.sys("/Applications").join(app)];
            if let Some(h) = &probe.home {
                places.push(h.join("Applications").join(app));
            }
            if !app.is_empty() && places.iter().any(|p| p.is_dir()) {
                out.push(Package::Native);
            }
        }
        Os::Linux => {
            let (binaries, launchers, snap, flatpak): (
                &[&str],
                &[&str],
                Option<&str>,
                Option<&str>,
            ) = match fam {
                Browser::Chrome => (
                    &["/opt/google/chrome/chrome"],
                    &["/usr/bin/google-chrome", "/usr/bin/google-chrome-stable"],
                    None,
                    Some("com.google.Chrome"),
                ),
                Browser::Edge => (
                    &["/opt/microsoft/msedge/msedge"],
                    &["/usr/bin/microsoft-edge", "/usr/bin/microsoft-edge-stable"],
                    None,
                    Some("com.microsoft.Edge"),
                ),
                Browser::Firefox => (
                    &[
                        "/usr/lib/firefox/firefox",
                        "/usr/lib64/firefox/firefox",
                        "/usr/lib/firefox-esr/firefox-esr",
                        "/opt/firefox/firefox",
                    ],
                    &["/usr/bin/firefox"],
                    Some("firefox"),
                    Some("org.mozilla.firefox"),
                ),
                Browser::Chromium => (
                    &[
                        "/usr/lib/chromium/chromium",
                        "/usr/lib64/chromium-browser/chromium-browser",
                    ],
                    &["/usr/bin/chromium", "/usr/bin/chromium-browser"],
                    Some("chromium"),
                    Some("org.chromium.Chromium"),
                ),
                _ => (&[], &[], None, None),
            };
            if binaries.iter().any(|b| is_elf(&probe.sys(b))) {
                out.push(Package::Native);
            }
            let snapped = snap.is_some_and(|name| {
                ["/snap", "/var/lib/snapd/snap"]
                    .iter()
                    .any(|base| probe.sys(&format!("{base}/{name}/current")).exists())
            });
            if snapped {
                out.push(Package::Snap);
            }
            if let Some(id) = flatpak {
                let mut places = vec![probe.sys(&format!("/var/lib/flatpak/app/{id}/current"))];
                if let Some(h) = &probe.home {
                    places.push(h.join(format!(".local/share/flatpak/app/{id}/current")));
                }
                if places.iter().any(|p| p.exists()) {
                    out.push(Package::Flatpak);
                }
            }
            // A launcher that is neither a real binary nor a known Snap shim, with nothing else
            // found: something is installed, but its kind is unknown.
            if out.is_empty() {
                let launcher = launchers.iter().map(|l| probe.sys(l)).find(|l| l.is_file());
                if let Some(l) = launcher {
                    out.push(if is_snap_shim(&l) {
                        Package::Snap
                    } else if is_elf(&l) {
                        Package::Native
                    } else {
                        Package::Unknown
                    });
                }
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// The evidence-backed verdict for `browser` installed as `package` on `os` (ADR 0029 matrix).
pub fn support(browser: Browser, os: Os, package: Package) -> Verdict {
    let fam = family(browser);
    let (support, reason) = match (os, package, fam) {
        // Sandboxed packages: Downova's registration is outside the sandbox.
        (Os::Linux, Package::Flatpak, _) => (Support::Unsupported, "flatpak_sandbox"),
        (Os::Linux, Package::Snap, Browser::Firefox) => (Support::Untested, "snap_portal_untested"),
        (Os::Linux, Package::Snap, _) => (Support::Unsupported, "snap_sandbox"),
        (_, Package::Unknown, _) => (Support::Untested, "package_unknown"),
        // Native installations.
        (Os::Windows, Package::Native, Browser::Chrome | Browser::Edge | Browser::Firefox) => {
            (Support::Supported, "tested_this_os")
        }
        (Os::Macos | Os::Linux, Package::Native, Browser::Chrome | Browser::Edge) => {
            (Support::Supported, "tested_package_smoke")
        }
        (_, Package::Native, _) => (Support::Untested, "not_tested_on_this_os"),
        (Os::Windows | Os::Macos, Package::Snap | Package::Flatpak, _) => {
            (Support::Untested, "package_unknown")
        }
    };
    Verdict {
        package,
        support,
        reason,
    }
}

/// Detection plus a verdict per detected package. Nothing detected is reported as one
/// `untested` verdict with reason `not_detected` (it may be installed where Downova does not look).
pub fn compatibility(browser: Browser, os: Os, probe: &Probe) -> (Vec<Package>, Vec<Verdict>) {
    let packages = detect(browser, os, probe);
    let verdicts = if packages.is_empty() {
        vec![Verdict {
            package: Package::Unknown,
            support: Support::Untested,
            reason: "not_detected",
        }]
    } else {
        packages.iter().map(|p| support(browser, os, *p)).collect()
    };
    (packages, verdicts)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe(dir: &Path) -> Probe {
        Probe {
            root: dir.join("root"),
            home: Some(dir.join("home")),
            windows_dirs: vec![dir.join("pf"), dir.join("pf86"), dir.join("local")],
        }
    }

    fn put(path: &Path, bytes: &[u8]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }

    #[test]
    fn linux_packages_are_told_apart_and_a_snap_shim_is_not_native() {
        let d = tempfile::tempdir().unwrap();
        let p = probe(d.path());
        let root = p.root.clone();
        assert!(detect(Browser::Firefox, Os::Linux, &p).is_empty());
        // Ubuntu: /usr/bin/firefox is a launcher script for the Snap.
        put(
            &root.join("usr/bin/firefox"),
            b"#!/bin/sh\nexec /snap/bin/firefox \"$@\"\n",
        );
        assert_eq!(detect(Browser::Firefox, Os::Linux, &p), vec![Package::Snap]);
        // A launcher of unknown kind.
        put(
            &root.join("usr/bin/chromium"),
            b"#!/bin/sh\nexec something\n",
        );
        assert_eq!(
            detect(Browser::Chromium, Os::Linux, &p),
            vec![Package::Unknown]
        );
        // A real binary, a Snap and a Flatpak side by side: each is reported.
        put(&root.join("usr/lib/firefox/firefox"), b"\x7fELF....");
        std::fs::create_dir_all(root.join("snap/firefox/current")).unwrap();
        std::fs::create_dir_all(
            p.home
                .as_ref()
                .unwrap()
                .join(".local/share/flatpak/app/org.mozilla.firefox/current"),
        )
        .unwrap();
        assert_eq!(
            detect(Browser::Firefox, Os::Linux, &p),
            vec![Package::Native, Package::Snap, Package::Flatpak]
        );
        // A non-ELF file at a binary location is not "native".
        put(&root.join("opt/google/chrome/chrome"), b"not a binary");
        assert!(detect(Browser::Chrome, Os::Linux, &p).is_empty());
        std::fs::create_dir_all(root.join("var/lib/flatpak/app/com.google.Chrome/current"))
            .unwrap();
        assert_eq!(
            detect(Browser::Chrome, Os::Linux, &p),
            vec![Package::Flatpak]
        );
    }

    #[test]
    fn windows_and_macos_report_native_installations_only() {
        let d = tempfile::tempdir().unwrap();
        let p = probe(d.path());
        assert!(detect(Browser::Firefox, Os::Windows, &p).is_empty());
        put(&d.path().join("pf/Mozilla Firefox/firefox.exe"), b"MZ");
        assert_eq!(
            detect(Browser::Firefox, Os::Windows, &p),
            vec![Package::Native]
        );
        put(
            &d.path().join("local/Google/Chrome/Application/chrome.exe"),
            b"MZ",
        );
        assert_eq!(
            detect(Browser::ChromeBeta, Os::Windows, &p),
            vec![Package::Native]
        );
        std::fs::create_dir_all(p.root.join("Applications/Firefox.app")).unwrap();
        assert_eq!(
            detect(Browser::Firefox, Os::Macos, &p),
            vec![Package::Native]
        );
        assert!(detect(Browser::Edge, Os::Macos, &p).is_empty());
    }

    #[test]
    fn the_matrix_never_calls_an_untested_package_supported() {
        use Package::*;
        let v = |b, os, p| support(b, os, p).support;
        assert_eq!(v(Browser::Firefox, Os::Windows, Native), Support::Supported);
        assert_eq!(v(Browser::Firefox, Os::Linux, Native), Support::Untested);
        assert_eq!(v(Browser::Firefox, Os::Macos, Native), Support::Untested);
        assert_eq!(v(Browser::Firefox, Os::Linux, Snap), Support::Untested);
        assert_eq!(
            v(Browser::Firefox, Os::Linux, Flatpak),
            Support::Unsupported
        );
        assert_eq!(v(Browser::Chromium, Os::Linux, Snap), Support::Unsupported);
        assert_eq!(v(Browser::Chrome, Os::Linux, Flatpak), Support::Unsupported);
        assert_eq!(v(Browser::Chrome, Os::Linux, Unknown), Support::Untested);
        assert_eq!(v(Browser::Chromium, Os::Windows, Native), Support::Untested);
        assert_eq!(
            v(Browser::ChromeBeta, Os::Windows, Native),
            Support::Supported
        );
        let d = tempfile::tempdir().unwrap();
        let (found, verdicts) = compatibility(Browser::Edge, Os::Linux, &probe(d.path()));
        assert!(found.is_empty());
        assert_eq!(verdicts[0].reason, "not_detected");
        assert_eq!(verdicts[0].support, Support::Untested);
        // Every reason is a stable code, never a path.
        for b in Browser::ALL {
            for os in [Os::Windows, Os::Macos, Os::Linux] {
                for p in [Native, Snap, Flatpak, Unknown] {
                    let r = support(b, os, p).reason;
                    assert!(r.chars().all(|c| c.is_ascii_lowercase() || c == '_'), "{r}");
                }
            }
        }
    }
}
