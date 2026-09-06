#![expect(clippy::expect_used, reason = "test fixture setup for the panic-family denominator")]

#[path = "no_panic_debt/support.rs"]
mod support;

use support::fixture_root;
use xtask::no_panic_debt::{
    InventoryRequest, SCHEMA, build_inventory, canonical_json, check_inventory, render_human,
};

#[test]
fn schema_and_producer_are_versioned() {
    let temp = fixture_root();
    let inventory = build_inventory(InventoryRequest {
        root: temp.path(),
        repository_commit: Some("fixture".to_string()),
        ..InventoryRequest::default()
    })
    .expect("inventory");
    assert_eq!(inventory.schema, SCHEMA);
    assert_eq!(inventory.producer, "cargo xtask no-panic debt inventory");
    assert_eq!(inventory.repository_commit, "fixture");
    assert!(inventory.counts.files > 0);
    assert!(render_human(&inventory).contains(SCHEMA));
}

#[test]
fn second_run_is_byte_identical() {
    let temp = fixture_root();
    let request = InventoryRequest {
        root: temp.path(),
        repository_commit: Some("fixture".to_string()),
        ..InventoryRequest::default()
    };
    let first = build_inventory(request).expect("first");
    let second = build_inventory(InventoryRequest {
        root: temp.path(),
        repository_commit: Some("fixture".to_string()),
        ..InventoryRequest::default()
    })
    .expect("second");
    assert_eq!(canonical_json(&first).expect("a"), canonical_json(&second).expect("b"));
}

#[test]
fn integrity_check_passes_on_a_complete_fixture() {
    let temp = fixture_root();
    let inventory = build_inventory(InventoryRequest {
        root: temp.path(),
        repository_commit: Some("fixture".to_string()),
        ..InventoryRequest::default()
    })
    .expect("inventory");
    let result = check_inventory(xtask::no_panic_debt::CheckRequest {
        root: temp.path(),
        current: &inventory,
        artifact: None,
        baseline: None,
    })
    .expect("check");
    assert!(result.ok, "{:?}", result.findings);
}

#[test]
fn stale_artifact_fails_canonical_source_drift() {
    let temp = fixture_root();
    let first = build_inventory(InventoryRequest {
        root: temp.path(),
        repository_commit: Some("fixture".to_string()),
        ..InventoryRequest::default()
    })
    .expect("first");
    let artifact = temp.path().join("artifact.json");
    std::fs::write(&artifact, canonical_json(&first).expect("json")).expect("artifact");
    std::fs::write(
        temp.path().join("crates/demo/tests/drift.rs"),
        "#[test]\nfn drifted() { let _ = Some(1).unwrap(); }\n",
    )
    .expect("drift");
    let current = build_inventory(InventoryRequest {
        root: temp.path(),
        repository_commit: Some("fixture".to_string()),
        ..InventoryRequest::default()
    })
    .expect("current");
    let result = check_inventory(xtask::no_panic_debt::CheckRequest {
        root: temp.path(),
        current: &current,
        artifact: Some(&artifact),
        baseline: None,
    })
    .expect("check");
    assert!(!result.ok, "stale artifact passed: {:?}", result.findings);
    assert!(
        result
            .findings
            .iter()
            .any(|finding| finding.contains("does not match current source projection")),
        "{:?}",
        result.findings
    );
}

#[test]
fn assert_macros_are_not_debt() {
    let temp = fixture_root();
    std::fs::write(
        temp.path().join("crates/demo/tests/assert.rs"),
        "#[test]\nfn asserted() { assert_eq!(1, 1); assert!(true); }\n",
    )
    .expect("assert");
    let inventory =
        build_inventory(InventoryRequest { root: temp.path(), ..InventoryRequest::default() })
            .expect("inventory");
    assert!(
        !inventory.rows.iter().any(|row| row.site_family.contains("assert")),
        "assert classified as debt: {:?}",
        inventory.rows
    );
}

#[test]
fn cli_exposes_inventory_check_and_report() {
    let output = assert_cmd::Command::cargo_bin("xtask")
        .expect("xtask bin")
        .args(["no-panic", "debt", "--help"])
        .output()
        .expect("help");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("inventory"), "{stdout}");
    assert!(stdout.contains("check"), "{stdout}");
    assert!(stdout.contains("report"), "{stdout}");
}
