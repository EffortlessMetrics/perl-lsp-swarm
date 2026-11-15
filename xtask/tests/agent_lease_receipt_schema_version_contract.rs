//! Pins the #15324 D1 fix for `agent_lease` / `agent_receipt`: `schema_version`
//! must use the project-wide `String` contracts (`agent_lease.v1`,
//! `agent_receipt.v1`), not legacy numeric `1`.
//!
//! JSON Schemas and Rust emitters must agree so consumers never see a numeric
//! `schema_version` on these receipt surfaces.

#![allow(clippy::expect_used)]

use std::fs;
use std::path::PathBuf;

use assert_cmd::cargo::cargo_bin_cmd;
use xtask::utils::project_root;

const EXPECTED_LEASE_SCHEMA_VERSION: &str = "agent_lease.v1";
const EXPECTED_RECEIPT_SCHEMA_VERSION: &str = "agent_receipt.v1";

fn lease_schema_path() -> PathBuf {
    project_root().expect("project root").join(".ci/receipts/schemas/agent-lease.schema.json")
}

fn receipt_schema_path() -> PathBuf {
    project_root().expect("project root").join(".ci/receipts/schemas/agent-receipt.schema.json")
}

fn task_fixture() -> PathBuf {
    project_root().expect("project root").join("xtask/tests/fixtures/agent-leases/task-valid.json")
}

#[test]
fn agent_lease_schema_pins_string_schema_version_const() {
    let schema_text = fs::read_to_string(lease_schema_path()).expect("read agent-lease schema");
    let schema: serde_json::Value = serde_json::from_str(&schema_text).expect("parse schema");

    let version = schema.pointer("/properties/schema_version").expect("schema_version property");
    assert_eq!(
        version.get("type").and_then(serde_json::Value::as_str),
        Some("string"),
        "agent-lease schema_version must be typed as string (D1 / #15324)"
    );
    assert_eq!(
        version.get("const").and_then(serde_json::Value::as_str),
        Some(EXPECTED_LEASE_SCHEMA_VERSION),
        "agent-lease schema_version const must be {EXPECTED_LEASE_SCHEMA_VERSION:?}"
    );
}

#[test]
fn agent_receipt_schema_pins_string_schema_version_const() {
    let schema_text = fs::read_to_string(receipt_schema_path()).expect("read agent-receipt schema");
    let schema: serde_json::Value = serde_json::from_str(&schema_text).expect("parse schema");

    let version = schema.pointer("/properties/schema_version").expect("schema_version property");
    assert_eq!(
        version.get("type").and_then(serde_json::Value::as_str),
        Some("string"),
        "agent-receipt schema_version must be typed as string (D1 / #15324)"
    );
    assert_eq!(
        version.get("const").and_then(serde_json::Value::as_str),
        Some(EXPECTED_RECEIPT_SCHEMA_VERSION),
        "agent-receipt schema_version const must be {EXPECTED_RECEIPT_SCHEMA_VERSION:?}"
    );
}

#[test]
fn agent_lease_acquire_emits_string_schema_version_not_legacy_integer() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let lease_path = tmp.path().join("lease.json");

    let output = cargo_bin_cmd!("xtask")
        .args([
            "agent",
            "lease",
            "acquire",
            "--task",
            task_fixture().to_string_lossy().as_ref(),
            "--out",
            lease_path.to_string_lossy().as_ref(),
        ])
        .output()
        .expect("run agent lease acquire");
    assert!(output.status.success(), "acquire failed: {}", String::from_utf8_lossy(&output.stderr));

    let raw = fs::read_to_string(&lease_path).expect("read emitted lease");
    let value: serde_json::Value = serde_json::from_str(&raw).expect("parse lease JSON");
    let object = value.as_object().expect("lease is a JSON object");
    assert_eq!(
        object.get("schema_version").and_then(serde_json::Value::as_str),
        Some(EXPECTED_LEASE_SCHEMA_VERSION),
        "emitted lease must expose schema_version as a string contract",
    );
    assert!(
        object.get("schema_version").and_then(serde_json::Value::as_u64).is_none(),
        "schema_version must not serialize as a JSON number (legacy u32 drift)",
    );
}

#[test]
fn legacy_numeric_schema_version_fails_string_deserialize_like_session_receipt_d1() {
    #[derive(Debug, serde::Deserialize)]
    struct WireVersion {
        schema_version: String,
    }

    let err = serde_json::from_str::<WireVersion>(r#"{"schema_version":1}"#)
        .expect_err("legacy numeric schema_version must not deserialize as String");
    assert!(
        err.to_string().contains("invalid type"),
        "expected serde type mismatch for numeric legacy version, got {err}",
    );
}
