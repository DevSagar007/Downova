//! SEC-03 regression guard (B2.8): no code path opens or runs a downloaded file.
//!
//! Downova never launches what it downloads: completion (`core/finalize.rs`), recovery and every
//! other path only hash, link, copy or rename files. This test keeps it that way structurally:
//! - only an explicit allowlist of source files may start a process, each for a fixed program
//!   that is never a download (the engine binary, `reg.exe`, Downova's own sibling executables);
//! - no crate may depend on an opener/shell crate or Tauri's shell/opener plugins;
//! - the desktop UI may not depend on a shell/opener plugin, and its capability grants none;
//! - no source mentions the OS "open this file" entry points;
//! - B2.19 (ADR 0021): the desktop shell has exactly one opener dependency, used only by the
//!   folder-reveal adapter `apps/desktop/src-tauri/src/folders.rs`, which only the user's
//!   `downloads_reveal` command reaches. It selects files (`reveal_item_in_dir`) and opens nothing
//!   but a directory it checked; no dialog/opener permission is granted to the webview.
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
    // The desktop shell: exactly one opener dependency (the folder-reveal adapter), no shell.
    let shell = fs::read_to_string(root().join("apps/desktop/src-tauri/Cargo.toml")).unwrap();
    let opener_lines: Vec<&str> = shell.lines().filter(|l| l.contains("opener")).collect();
    assert_eq!(opener_lines.len(), 1, "{opener_lines:?}");
    assert!(opener_lines[0].starts_with("tauri-plugin-opener = "));
    for dep in ["\nopen =", "webbrowser", "tauri-plugin-shell"] {
        assert!(!shell.contains(dep), "the desktop shell depends on {dep:?}");
    }
    let package = fs::read_to_string(root().join("apps/desktop/package.json")).unwrap();
    for dep in [
        "@tauri-apps/plugin-shell",
        "@tauri-apps/plugin-opener",
        "@tauri-apps/plugin-dialog",
    ] {
        assert!(!package.contains(dep), "the desktop UI depends on {dep}");
    }
    let capability =
        fs::read_to_string(root().join("apps/desktop/src-tauri/capabilities/default.json"))
            .unwrap();
    for grant in ["shell:", "opener:", "dialog:"] {
        assert!(
            !capability.contains(grant),
            "the desktop capability grants {grant}"
        );
    }
}

/// The single, user-triggered folder-reveal adapter (B2.19, ADR 0021).
const REVEAL_ADAPTER: &str = "apps/desktop/src-tauri/src/folders.rs";

fn code_of(p: &Path) -> String {
    fs::read_to_string(p)
        .unwrap()
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn only_the_folder_adapter_uses_the_opener_and_only_a_user_command_reaches_it() {
    const LIB: &str = "apps/desktop/src-tauri/src/lib.rs";
    let mut adapter_seen = false;
    for file in product_sources() {
        let name = rel(&file);
        let code = code_of(&file);
        if name == REVEAL_ADAPTER {
            adapter_seen = true;
            continue;
        }
        for opener in [
            "OpenerExt",
            ".opener()",
            "open_path",
            "reveal_item_in_dir",
            "open_url",
        ] {
            assert!(!code.contains(opener), "{name} uses {opener}");
        }
        // lib.rs registers the plugin and nothing else; nobody else names it.
        let registrations = code.matches("tauri_plugin_opener::").count();
        if name == LIB {
            assert_eq!(registrations, 1, "lib.rs only registers the plugin");
            assert!(code.contains("tauri_plugin_opener::init()"));
        } else {
            assert_eq!(registrations, 0, "{name} names the opener plugin");
            assert!(
                !code.contains("folders::reveal"),
                "{name} reaches the folder reveal"
            );
        }
    }
    assert!(adapter_seen, "{REVEAL_ADAPTER} was scanned");
    // lib.rs: reached only from the `downloads_reveal` command (a user action).
    let lib = code_of(&root().join(LIB));
    assert_eq!(lib.matches("folders::reveal(").count(), 1);
    let before = &lib[..lib.find("folders::reveal(").unwrap()];
    let command = before.rfind("#[tauri::command]").unwrap();
    assert!(before[command..].contains("async fn downloads_reveal("));
    // The adapter: never a URL or a program; files are only revealed; open_path only receives a
    // directory checked (without following links) right before.
    let adapter = code_of(&root().join(REVEAL_ADAPTER));
    assert!(!adapter.contains("open_url"));
    assert!(!adapter.contains("Command::new"));
    assert_eq!(adapter.matches(".open_path(").count(), 1);
    let open = adapter.find(".open_path(").unwrap();
    let guard = adapter[..open].rfind("if !is_real_dir(&dir)").unwrap();
    assert!(open - guard < 700, "open_path follows its directory check");
    assert!(
        adapter[open..open + 60].contains("None::<&str>"),
        "no \"open with\" program"
    );
    assert!(
        adapter.contains("symlink_metadata"),
        "links are never followed"
    );
}
