//! Keeps the Rust wire types in sync with the canonical contract schema.
//!
//! Every `packages/contracts/examples/*.json` must deserialize into the Rust type named by its
//! `$contract` field and serialize back to an equivalent JSON value. Every file in
//! `examples/invalid/` must be rejected.

use std::fs;
use std::path::{Path, PathBuf};

use dm_domain::api::{AddDownloadParams, DownloadSnapshot, HelloResult, ServiceEvent};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

fn examples_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packages/contracts/examples")
}

fn round_trip<T: DeserializeOwned + Serialize>(value: &Value) -> Result<Value, String> {
    let typed: T = serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
    serde_json::to_value(&typed).map_err(|e| e.to_string())
}

fn check(contract: &str, value: &Value) -> Result<Value, String> {
    match contract {
        "HelloResult" => round_trip::<HelloResult>(value),
        "AddDownloadParams" => round_trip::<AddDownloadParams>(value),
        "DownloadSnapshot" => round_trip::<DownloadSnapshot>(value),
        "ServiceEvent" => round_trip::<ServiceEvent>(value),
        other => Err(format!("no Rust mapping for $contract {other:?}; add one here")),
    }
}

fn load(dir: &Path) -> Vec<(PathBuf, String, Value)> {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "json"))
        .collect();
    entries.sort();
    entries
        .into_iter()
        .map(|path| {
            let doc: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
            let contract = doc["$contract"].as_str().unwrap().to_owned();
            (path, contract, doc["value"].clone())
        })
        .collect()
}

#[test]
fn valid_examples_round_trip() {
    let examples = load(&examples_dir());
    assert!(!examples.is_empty(), "no contract examples found");
    for (path, contract, value) in examples {
        let back = check(&contract, &value)
            .unwrap_or_else(|e| panic!("{} ({contract}) rejected: {e}", path.display()));
        assert_eq!(back, value, "{} did not round-trip unchanged", path.display());
    }
}

#[test]
fn invalid_examples_are_rejected() {
    for (path, contract, value) in load(&examples_dir().join("invalid")) {
        assert!(
            check(&contract, &value).is_err(),
            "{} ({contract}) must be rejected",
            path.display()
        );
    }
}
