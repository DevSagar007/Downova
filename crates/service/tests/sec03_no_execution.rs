//! SEC-03 regression guard (B2.8): no code path opens or runs a downloaded file.
//!
//! Downova never launches what it downloads: completion (`core/finalize.rs`), recovery and every
//! other path only hash, link, copy or rename files. This test keeps it that way structurally:
//! - only an explicit allowlist of source files may start a process, each for a fixed program
//!   that is never a download (the engine binary, `reg.exe`, Downova's own sibling executables);
//! - no crate may depend on an opener/shell crate or Tauri's shell/opener plugins;
//! - the desktop UI may not depend on a shell/opener plugin, and its capability grants none;
//! - no source mentions the OS "open this file" entry points.
//!
//! A new process-starting call anywhere else fails here and needs a reviewed allowlist entry.

#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn walk(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        if p.is_dir() {
            if !matches!(name, "target" | "node_modules" | "gen" | "dist") {
                walk(&p, ext, out);
            }
        } else if p.extension().is_some_and(|x| x == ext) {
            out.push(p);
        }
    }
}

/// Product (non-test) Rust sources.
fn product_sources() -> Vec<PathBuf> {
    let mut files = Vec::new();
    walk(&root().join("crates"), "rs", &mut files);
    walk(&root().join("apps/desktop/src-tauri/src"), "rs", &mut files);
    files
        .into_iter()
        .filter(|p| {
            let s = p.to_string_lossy().replace('\\', "/");
            !s.contains("/tests/")
                && !s.ends_with("/tests.rs")
                && !s.ends_with("/fake.rs")
                && !s.contains("/test-fixtures/")
                && !s.contains("/benches/")
        })
        .collect()
}

fn rel(p: &Path) -> String {
    p.strip_prefix(root())
        .unwrap_or(p)
        .to_string_lossy()
        .replace('\\', "/")
        .trim_start_matches("../")
        .to_owned()
}

/// Files allowed to start a process, and what they start.
const SPAWN_ALLOWLIST: &[(&str, &str)] = &[
    (
        "crates/backend-aria2/src/binary.rs",
        "the configured aria2c binary (version probe)",
    ),
    (
        "crates/backend-aria2/src/lib.rs",
        "the configured aria2c binary (engine)",
    ),
    (
        "crates/browser-host/src/registration.rs",
        "reg.exe (Windows registration)",
    ),
    (
        "crates/platform/src/launch.rs",
        "the sibling dm-service executable",
    ),
    (
        "apps/desktop/src-tauri/src/browser_setup.rs",
        "the sibling dm-browser-host executable",
    ),
];

#[test]
fn only_allowlisted_files_start_processes_and_none_is_a_completion_or_recovery_path() {
    let sources = product_sources();
    assert!(
        sources.len() > 30,
        "found too few sources: {}",
        sources.len()
    );
    let mut offenders = Vec::new();
    for file in &sources {
        let text = fs::read_to_string(file).unwrap();
        // Comments are stripped line by line so documentation may name these APIs.
        let code: String = text
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        let spawns = code.contains("Command::new") || code.contains("process::Command");
        let name = rel(file);
        if spawns && !SPAWN_ALLOWLIST.iter().any(|(f, _)| *f == name) {
            offenders.push(name.clone());
        }
        for forbidden in [
            "ShellExecute",
            "xdg-open",
            "open::that",
            "opener::open",
            "NSWorkspace",
            "\"explorer\"",
            "\"cmd\"",
            "\"/usr/bin/open\"",
        ] {
            if code.contains(forbidden) {
                offenders.push(format!("{name}: {forbidden}"));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "process-starting code outside the allowlist: {offenders:?}"
    );
    // The finalization and recovery paths are not on the list at all.
    for path in [
        "crates/service/src/core.rs",
        "crates/service/src/core/finalize.rs",
    ] {
        assert!(!SPAWN_ALLOWLIST.iter().any(|(f, _)| *f == path));
        assert!(sources.iter().any(|s| rel(s) == path), "{path} was scanned");
    }
}

#[test]
fn no_dependency_or_capability_can_open_files() {
    let mut manifests = Vec::new();
    walk(&root().join("crates"), "toml", &mut manifests);
    manifests.push(root().join("Cargo.toml"));
    manifests.push(root().join("apps/desktop/src-tauri/Cargo.toml"));
    for m in manifests {
        let text = fs::read_to_string(&m).unwrap_or_default();
        for dep in [
            "opener",
            "\nopen =",
            "webbrowser",
            "tauri-plugin-shell",
            "tauri-plugin-opener",
        ] {
            assert!(!text.contains(dep), "{} depends on {dep:?}", rel(&m));
        }
    }
    let package = fs::read_to_string(root().join("apps/desktop/package.json")).unwrap();
    for dep in ["@tauri-apps/plugin-shell", "@tauri-apps/plugin-opener"] {
        assert!(!package.contains(dep), "the desktop UI depends on {dep}");
    }
    let capability =
        fs::read_to_string(root().join("apps/desktop/src-tauri/capabilities/default.json"))
            .unwrap();
    for grant in ["shell:", "opener:"] {
        assert!(
            !capability.contains(grant),
            "the desktop capability grants {grant}"
        );
    }
}
