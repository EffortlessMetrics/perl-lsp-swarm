// Integration test: assertion helpers (`expect`/`unwrap`/`panic!`) carry the
// failure message. The workspace-wide deny is a production-code rule.
#![allow(clippy::expect_used)]
#[path = "workspace_doctor_inventory/support.rs"]
mod support;

use std::collections::BTreeSet;
use support::fixture_root;
use support::workspace_doctor_inventory::{build_inventory, validate_inventory};

#[test]
fn complete_inventory_exposes_false_success_and_mutation() {
    let temp = fixture_root();
    let inventory = build_inventory(temp.path()).expect("build inventory");
    assert_eq!(inventory.status, "DEBT_INVENTORIED");
    assert_eq!(inventory.doctor_check_headings.len(), 7);
    assert_eq!(inventory.rows.len(), 22);
    assert_eq!(inventory.active_mutations.len(), 1);
    assert_eq!(inventory.active_mutations[0].kind, "git_config_unset");
    assert_eq!(inventory.active_mutations[0].owned_by, "core-bare");
    let findings: BTreeSet<&str> =
        inventory.findings.iter().map(|finding| finding.finding_id.as_str()).collect();
    for expected in [
        "AUTO_MUTATION_IN_DIAGNOSIS",
        "REQUIRED_FINDINGS_EXIT_ZERO",
        "READY_AFTER_UNRESOLVED_DOCTOR",
        "BEHIND_ONLY_BRANCH_MOVEMENT",
        "UNTRACKED_STATE_OMITTED",
        "ADMISSION_VERDICT_EXIT_ZERO",
        "WORKTREE_DRY_RUN_PRUNES_METADATA",
    ] {
        assert!(findings.contains(expected), "missing finding {expected}");
    }
    validate_inventory(temp.path(), &inventory).expect("validate inventory");
}

#[test]
fn output_is_deterministic() {
    let temp = fixture_root();
    let first = build_inventory(temp.path()).expect("first inventory");
    let second = build_inventory(temp.path()).expect("second inventory");
    assert_eq!(first, second);
    assert_eq!(first.inventory_digest, second.inventory_digest);
}

#[test]
fn schema_version_is_project_wide_string_field() {
    // Pin the #15289 fix: `workspace_doctor_inventory` must emit a `schema_version`
    // string (project-wide convention), not the legacy short-form `schema: u32`.
    let temp = fixture_root();
    let inventory = build_inventory(temp.path()).expect("build inventory");
    assert_eq!(inventory.schema_version, "workspace_doctor_inventory.v1");

    let rendered = serde_json::to_value(&inventory).expect("serialize inventory");
    let object = rendered.as_object().expect("inventory is a JSON object");
    assert!(
        !object.contains_key("schema"),
        "legacy `schema` key must not appear in serialized inventory (object={object:?})",
    );
    assert_eq!(
        object.get("schema_version").and_then(serde_json::Value::as_str),
        Some("workspace_doctor_inventory.v1"),
        "serialized inventory must expose schema_version: String for project-wide consumers",
    );
}
