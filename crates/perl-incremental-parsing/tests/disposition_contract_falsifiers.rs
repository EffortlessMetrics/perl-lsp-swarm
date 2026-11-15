#![allow(clippy::expect_used)]
#[path = "disposition_contract/validate.rs"]
mod validate;

use std::path::{Path, PathBuf};
use validate::{
    CargoTargetSnapshot, SCHEMA_VERSION, cargo_snapshot_from_metadata, load_inventory_from_path,
    source_files_with_tests, validate_inventory,
};

fn fixture_inventory(name: &str) -> serde_json::Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/behavior_disposition")
        .join(name);
    load_inventory_from_path(&path).expect("read fixture inventory")
}

fn production_inventory() -> serde_json::Value {
    load_inventory_from_path(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("behavior_disposition.json"),
    )
    .expect("read production inventory")
}

fn cargo_snapshot() -> CargoTargetSnapshot {
    cargo_snapshot_from_metadata(&package_root()).expect("cargo metadata snapshot")
}

fn source_tests() -> std::collections::BTreeSet<String> {
    source_files_with_tests(&package_root().join("src"), Path::new("src")).expect("source tests")
}

fn package_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn production_inventory_passes_with_live_cargo_graph() {
    let inventory = production_inventory();
    assert_eq!(inventory["schema_version"], SCHEMA_VERSION);
    validate_inventory(
        &inventory,
        &package_root(),
        "perl-incremental-parsing",
        &cargo_snapshot(),
        &source_tests(),
    )
    .expect("production inventory must validate against live Cargo metadata");
}

#[test]
fn rejects_custom_integration_test_omitted_from_ledger() {
    let inventory = production_inventory();
    let mut cargo = cargo_snapshot();
    cargo.tests.insert("tests/qa/custom.rs".to_string(), "default".to_string());
    assert!(
        validate_inventory(
            &inventory,
            &package_root(),
            "perl-incremental-parsing",
            &cargo,
            &source_tests(),
        )
        .is_err()
    );
}

#[test]
fn rejects_phantom_conventional_test_when_autotests_disabled() {
    let inventory = fixture_inventory("phantom_autotest_path.json");
    assert!(
        validate_inventory(
            &inventory,
            &package_root(),
            "perl-incremental-parsing",
            &cargo_snapshot(),
            &source_tests(),
        )
        .is_err()
    );
}

#[test]
fn rejects_custom_benchmark_target_omitted_from_ledger() {
    let inventory = production_inventory();
    let mut cargo = cargo_snapshot();
    cargo.benches.insert("benches/custom_bench.rs".to_string(), "default".to_string());
    assert!(
        validate_inventory(
            &inventory,
            &package_root(),
            "perl-incremental-parsing",
            &cargo,
            &source_tests(),
        )
        .is_err()
    );
}

#[test]
fn rejects_unlisted_source_unit_test_module() {
    let inventory = production_inventory();
    let mut sources = source_tests();
    sources.insert("src/nested/module.rs".to_string());
    assert!(
        validate_inventory(
            &inventory,
            &package_root(),
            "perl-incremental-parsing",
            &cargo_snapshot(),
            &sources,
        )
        .is_err()
    );
}

#[test]
fn rejects_duplicate_target_rows() {
    let inventory = fixture_inventory("duplicate_test_target_path.json");
    assert!(
        validate_inventory(
            &inventory,
            &package_root(),
            "perl-incremental-parsing",
            &cargo_snapshot(),
            &source_tests(),
        )
        .is_err()
    );
}

#[test]
fn rejects_unknown_disposition_token() {
    let inventory = fixture_inventory("unknown_disposition_token.json");
    assert!(
        validate_inventory(
            &inventory,
            &package_root(),
            "perl-incremental-parsing",
            &cargo_snapshot(),
            &source_tests(),
        )
        .is_err()
    );
}

#[test]
fn rejects_unknown_plane_token() {
    let inventory = fixture_inventory("unknown_plane_token.json");
    assert!(
        validate_inventory(
            &inventory,
            &package_root(),
            "perl-incremental-parsing",
            &cargo_snapshot(),
            &source_tests(),
        )
        .is_err()
    );
}

#[test]
fn rejects_migrate_disposition_without_canonical_destination() {
    let inventory = fixture_inventory("migrate_without_destination.json");
    assert!(
        validate_inventory(
            &inventory,
            &package_root(),
            "perl-incremental-parsing",
            &cargo_snapshot(),
            &source_tests(),
        )
        .is_err()
    );
}

#[test]
fn rejects_retain_compatibility_row_exercising_experimental_generation() {
    let inventory = fixture_inventory("retain_imports_experimental_generation.json");
    assert!(
        validate_inventory(
            &inventory,
            &package_root(),
            "perl-incremental-parsing",
            &cargo_snapshot(),
            &source_tests(),
        )
        .is_err()
    );
}

#[test]
fn rejects_benchmark_quarantine_without_owner_issue() {
    let inventory = fixture_inventory("benchmark_quarantine_missing_owner.json");
    assert!(
        validate_inventory(
            &inventory,
            &package_root(),
            "perl-incremental-parsing",
            &cargo_snapshot(),
            &source_tests(),
        )
        .is_err()
    );
}

#[test]
fn rejects_unknown_top_level_field() {
    let inventory = fixture_inventory("unknown_top_level_field.json");
    assert!(
        validate_inventory(
            &inventory,
            &package_root(),
            "perl-incremental-parsing",
            &cargo_snapshot(),
            &source_tests(),
        )
        .is_err()
    );
}

#[test]
fn rejects_cargo_feature_gate_mismatch_on_integration_target() {
    let mut inventory = production_inventory();
    let test_targets = inventory
        .get_mut("test_targets")
        .and_then(|value| value.as_array_mut())
        .expect("test_targets array");
    for row in test_targets {
        if row.get("path").and_then(|value| value.as_str())
            == Some("tests/compatibility_forwarding.rs")
        {
            row.as_object_mut().expect("object row").insert(
                "required_features".to_string(),
                serde_json::Value::String("experimental".to_string()),
            );
            break;
        }
    }
    assert!(
        validate_inventory(
            &inventory,
            &package_root(),
            "perl-incremental-parsing",
            &cargo_snapshot(),
            &source_tests(),
        )
        .is_err()
    );
}

#[test]
fn rejects_ledger_default_when_cargo_target_requires_features() {
    let inventory = production_inventory();
    let mut cargo = cargo_snapshot();
    cargo.tests.insert("tests/feature_gated_probe.rs".to_string(), "experimental".to_string());
    assert!(
        validate_inventory(
            &inventory,
            &package_root(),
            "perl-incremental-parsing",
            &cargo,
            &source_tests(),
        )
        .is_err()
    );
}
