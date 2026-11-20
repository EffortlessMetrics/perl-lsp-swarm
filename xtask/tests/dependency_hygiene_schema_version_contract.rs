//! Pins the #15324 D1 fix for `dependency_hygiene`: `schema_version` must be the
//! project-wide `String` contract (`dependency_hygiene.v1`), not legacy `u32 = 1`.
//!
//! The JSON Schema and the Rust emitter must agree so consumers never see a
//! numeric `schema_version` on this receipt surface.

#![allow(clippy::expect_used)]

use std::fs;
use std::path::PathBuf;

use xtask::utils::project_root;

const EXPECTED_SCHEMA_VERSION: &str = "dependency_hygiene.v1";

fn schema_path() -> PathBuf {
    project_root()
        .expect("project root")
        .join(".ci/receipts/schemas/dependency-hygiene.schema.json")
}

#[test]
fn dependency_hygiene_schema_pins_string_schema_version_const() {
    let schema_text = fs::read_to_string(schema_path()).expect("read dependency-hygiene schema");
    let schema: serde_json::Value = serde_json::from_str(&schema_text).expect("parse schema");

    let version = schema.pointer("/properties/schema_version").expect("schema_version property");
    assert_eq!(
        version.get("type").and_then(serde_json::Value::as_str),
        Some("string"),
        "dependency-hygiene schema_version must be typed as string (D1 / #15324)"
    );
    assert_eq!(
        version.get("const").and_then(serde_json::Value::as_str),
        Some(EXPECTED_SCHEMA_VERSION),
        "dependency-hygiene schema_version const must be {EXPECTED_SCHEMA_VERSION:?}"
    );
}
