//! Keeps the Rust wire types in sync with the canonical contract schema.
//!
//! Every `packages/contracts/examples/*.json` must deserialize into the Rust type named by its
//! `$contract` field and serialize back to an equivalent JSON value. Every file in
//! `examples/invalid/` must be rejected.

#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::{Path, PathBuf};

use dm_domain::api::{
    AddDownloadParams, CheckLinkParams, CheckLinkResult, DeleteQueueParams, DownloadSnapshot,
    HelloResult, HistoryCleanResult, HistoryPreview, HistorySettings, JobRef, ListDownloadsResult,
    ListQueuesResult, LocateResult, MoveJobParams, NativeHostRequest, NativeHostResponse,
    ReplaceSourceParams, ReplaceSourceResult, RetryParams, RpcNotification, RpcRequest,
    RpcResponse, ServiceEvent, ServiceSettings, SetPriorityParams, ShutdownParams, ShutdownResult,
    SubscribeResult, methods,
};
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
        "JobRef" => round_trip::<JobRef>(value),
        "ListDownloadsResult" => round_trip::<ListDownloadsResult>(value),
        "ShutdownParams" => round_trip::<ShutdownParams>(value),
        "ShutdownResult" => round_trip::<ShutdownResult>(value),
        "CheckLinkParams" => round_trip::<CheckLinkParams>(value),
        "CheckLinkResult" => round_trip::<CheckLinkResult>(value),
        "ServiceSettings" => round_trip::<ServiceSettings>(value),
        "ListQueuesResult" => round_trip::<ListQueuesResult>(value),
        "DeleteQueueParams" => round_trip::<DeleteQueueParams>(value),
        "MoveJobParams" => round_trip::<MoveJobParams>(value),
        "SetPriorityParams" => round_trip::<SetPriorityParams>(value),
        "SubscribeResult" => round_trip::<SubscribeResult>(value),
        "HistorySettings" => round_trip::<HistorySettings>(value),
        "HistoryPreview" => round_trip::<HistoryPreview>(value),
        "HistoryCleanResult" => round_trip::<HistoryCleanResult>(value),
        "RetryParams" => round_trip::<RetryParams>(value),
        "ReplaceSourceParams" => round_trip::<ReplaceSourceParams>(value),
        "ReplaceSourceResult" => round_trip::<ReplaceSourceResult>(value),
        "LocateResult" => round_trip::<LocateResult>(value),
        "RpcNotification" => round_trip::<RpcNotification>(value).and_then(|v| {
            if v["method"] == methods::EVENT {
                Ok(v)
            } else {
                Err("not an event notification".into())
            }
        }),
        // The schema also restricts URL scheme/length and the key alphabet; the native host
        // enforces those (crates/browser-host), so only the shape is mirrored here.
        "NativeHostRequest" => round_trip::<NativeHostRequest>(value).and_then(|v| {
            let url = v["url"].as_str().unwrap_or("https://").to_ascii_lowercase();
            if url.starts_with("http://") || url.starts_with("https://") {
                Ok(v)
            } else {
                Err("unsupported scheme".into())
            }
        }),
        "NativeHostResponse" => round_trip::<NativeHostResponse>(value),
        "RpcRequest" => round_trip::<RpcRequest>(value).and_then(|v| {
            // The schema closes the method set; mirror that check in Rust.
            let method = v["method"].as_str().unwrap_or_default();
            if methods::ALL.contains(&method) {
                Ok(v)
            } else {
                Err(format!("unknown method {method}"))
            }
        }),
        "RpcResponse" => round_trip::<RpcResponse>(value).and_then(|v| {
            if v.get("result").is_some() == v.get("error").is_some() {
                Err("exactly one of result/error".into())
            } else {
                Ok(v)
            }
        }),
        other => Err(format!(
            "no Rust mapping for $contract {other:?}; add one here"
        )),
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
        assert_eq!(
            back,
            value,
            "{} did not round-trip unchanged",
            path.display()
        );
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

/// The error catalogue (CORE-11) is keyed by these lists, so they must equal the schema's enums.
#[test]
fn error_kinds_and_next_actions_match_the_schema() {
    let schema: Value = serde_json::from_str(
        &fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../packages/contracts/schema/service-api.v1.schema.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let names = |def: &str| -> Vec<String> {
        schema["$defs"][def]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_owned())
            .collect()
    };
    let wire = |v: Value| v.as_str().unwrap().to_owned();
    let kinds: Vec<String> = dm_domain::ErrorKind::ALL
        .iter()
        .map(|k| wire(serde_json::to_value(k).unwrap()))
        .collect();
    let actions: Vec<String> = dm_domain::NextAction::ALL
        .iter()
        .map(|a| wire(serde_json::to_value(a).unwrap()))
        .collect();
    assert_eq!(kinds, names("ErrorKind"));
    assert_eq!(actions, names("NextAction"));
}
