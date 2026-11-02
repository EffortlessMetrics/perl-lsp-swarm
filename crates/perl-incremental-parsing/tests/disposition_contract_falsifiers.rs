#![allow(clippy::expect_used)]
#[path = "disposition_contract/validate.rs"]
mod validate;

use std::collections::BTreeSet;
use std::path::PathBuf;
use validate::{CargoTargetSnapshot, SCHEMA_VERSION, load_inventory_from_path, validate_inventory};

fn fixture_inventory(name: &str) -> serde_json::Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/behavior_disposition")
        .join(name);
    load_inventory_from_path(&path).expect("read fixture inventory")
}

fn canonical_cargo_snapshot() -> CargoTargetSnapshot {
    let mut tests = BTreeSet::new();
    for path in [
        "tests/authority_disposition_contract.rs",
        "tests/compatibility_forwarding.rs",
        "tests/comprehensive_unit_tests.rs",
        "tests/coverage_gaps_unit_tests.rs",
        "tests/extended_unit_tests.rs",
        "tests/incremental_efficiency_tests.rs",
        "tests/incremental_full_parse_equivalence.rs",
        "tests/segment_cache_checkpoint_window_tests.rs",
        "tests/disposition_contract_falsifiers.rs",
    ] {
        tests.insert(path.to_string());
    }
    let mut benches = BTreeSet::new();
    benches.insert("benches/incremental_parsing_benchmarks.rs".to_string());
    CargoTargetSnapshot { tests, benches, examples: BTreeSet::new(), bins: BTreeSet::new() }
}

fn canonical_source_tests() -> BTreeSet<String> {
    let mut sources = BTreeSet::new();
    sources.insert("src/lib.rs".to_string());
    sources
}

fn package_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn fixture_canonical_inventory_passes_with_matching_cargo_graph() {
    let inventory = fixture_inventory("canonical_minimal.json");
    assert_eq!(inventory["schema_version"], SCHEMA_VERSION);
    validate_inventory(
        &inventory,
        &package_root(),
        "perl-incremental-parsing",
        &canonical_cargo_snapshot(),
        &canonical_source_tests(),
    )
    .expect("canonical fixture must validate");
}

#[test]
fn rejects_custom_integration_test_omitted_from_ledger() {
    let inventory = fixture_inventory("canonical_minimal.json");
    let mut cargo = canonical_cargo_snapshot();
    cargo.tests.insert("tests/qa/custom.rs".to_string());
    assert!(
        validate_inventory(
            &inventory,
            &package_root(),
            "perl-incremental-parsing",
            &cargo,
            &canonical_source_tests(),
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
            &canonical_cargo_snapshot(),
            &canonical_source_tests(),
        )
        .is_err()
    );
}

#[test]
fn rejects_custom_benchmark_target_omitted_from_ledger() {
    let inventory = fixture_inventory("canonical_minimal.json");
    let mut cargo = canonical_cargo_snapshot();
    cargo.benches.insert("benches/custom_bench.rs".to_string());
    assert!(
        validate_inventory(
            &inventory,
            &package_root(),
            "perl-incremental-parsing",
            &cargo,
            &canonical_source_tests(),
        )
        .is_err()
    );
}

#[test]
fn rejects_unlisted_source_unit_test_module() {
    let inventory = fixture_inventory("canonical_minimal.json");
    let mut sources = canonical_source_tests();
    sources.insert("src/nested/module.rs".to_string());
    assert!(
        validate_inventory(
            &inventory,
            &package_root(),
            "perl-incremental-parsing",
            &canonical_cargo_snapshot(),
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
            &canonical_cargo_snapshot(),
            &canonical_source_tests(),
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
            &canonical_cargo_snapshot(),
            &canonical_source_tests(),
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
            &canonical_cargo_snapshot(),
            &canonical_source_tests(),
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
            &canonical_cargo_snapshot(),
            &canonical_source_tests(),
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
            &canonical_cargo_snapshot(),
            &canonical_source_tests(),
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
            &canonical_cargo_snapshot(),
            &canonical_source_tests(),
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
            &canonical_cargo_snapshot(),
            &canonical_source_tests(),
        )
        .is_err()
    );
}
