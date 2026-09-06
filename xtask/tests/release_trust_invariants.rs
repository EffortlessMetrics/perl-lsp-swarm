//! Discriminating tests for `release_trust_invariants.v1` (#9392).

use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::error::Error;
use std::path::{Path, PathBuf};
use xtask::release_trust_invariants::{
    REGISTRY_PATH, SCHEMA_PATH, STATUS_PATH, check, load_and_validate, render_status,
    validate_registry_value,
};

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from(".."))
}

fn canonical_registry() -> TestResult<Value> {
    let bytes = std::fs::read(repo_root().join(REGISTRY_PATH))?;
    Ok(serde_json::from_slice(&bytes)?)
}

fn expect_violation(value: &Value, needle: &str) -> TestResult {
    let error = match validate_registry_value(&repo_root(), value) {
        Ok(_) => "registry unexpectedly validated".to_string(),
        Err(error) => error.to_string(),
    };
    assert!(error.contains(needle), "expected `{needle}` in:\n{error}");
    Ok(())
}

fn invariant_mut<'a>(registry: &'a mut Value, invariant_id: &str) -> Option<&'a mut Value> {
    registry
        .get_mut("invariants")?
        .as_array_mut()?
        .iter_mut()
        .find(|row| row.get("invariant_id").and_then(Value::as_str) == Some(invariant_id))
}

fn owner_mut(registry: &mut Value, issue: u64) -> Option<&mut Value> {
    registry
        .get_mut("owner_authorities")?
        .as_array_mut()?
        .iter_mut()
        .find(|owner| owner.get("issue").and_then(Value::as_u64) == Some(issue))
}

fn producer_mut<'a>(registry: &'a mut Value, kind: &str) -> Option<&'a mut Value> {
    registry
        .get_mut("producer_authorities")?
        .as_array_mut()?
        .iter_mut()
        .find(|producer| producer.get("producer_kind").and_then(Value::as_str) == Some(kind))
}

#[test]
fn committed_registry_and_generated_status_pass() -> TestResult {
    let registry = check(&repo_root()).map_err(|error| error.to_string())?;
    assert_eq!(registry.invariants.len(), 31);
    assert!(
        registry.invariants.iter().any(|row| row.invariant_id == "false_exact")
            && registry.invariants.iter().any(|row| row.invariant_id == "unreviewed_public_claim")
            && registry
                .invariants
                .iter()
                .any(|row| { row.invariant_id == "duplicate_or_late_terminal_outcome" })
    );
    Ok(())
}

#[test]
fn status_render_is_byte_deterministic() -> TestResult {
    let first = load_and_validate(&repo_root()).map_err(|error| error.to_string())?;
    let second = load_and_validate(&repo_root()).map_err(|error| error.to_string())?;
    let rendered = render_status(&first);
    assert_eq!(rendered, render_status(&second));
    assert!(rendered.contains("`write-status`"));
    assert!(!rendered.contains("--write-status"));
    assert!(rendered.contains("does not consume live candidate receipts"));
    Ok(())
}

#[test]
fn generated_projection_diverges_from_registry() -> TestResult {
    let root = repo_root();
    let tmp = tempfile::tempdir()?;
    let dest = tmp.path();
    for rel in [SCHEMA_PATH, REGISTRY_PATH, STATUS_PATH] {
        let dst = dest.join(rel);
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(root.join(rel), &dst)?;
    }
    let drifted = dest.join(STATUS_PATH);
    let mut text = std::fs::read_to_string(&drifted)?;
    text.push_str("drift\n");
    std::fs::write(&drifted, text)?;
    let error = match check(dest) {
        Ok(_) => "check unexpectedly passed".to_string(),
        Err(error) => error.to_string(),
    };
    assert!(
        error.contains("out of date"),
        "expected generated-projection drift to fail check:\n{error}"
    );
    Ok(())
}

#[test]
fn invariant_bound_to_superseded_owner_fails_even_with_successor() -> TestResult {
    let mut registry = canonical_registry()?;
    let owner = owner_mut(&mut registry, 3099).expect("3099");
    owner["status"] = json!("superseded");
    owner["successor"] = json!(8507);
    expect_violation(&registry, "owner issue 3099 is superseded")
}

#[test]
fn controller_requirements_cover_body_zero_budget_sets() -> TestResult {
    let registry = load_and_validate(&repo_root()).map_err(|error| error.to_string())?;
    let set_for = |issue: u32| -> BTreeSet<&str> {
        registry
            .controller_requirements
            .iter()
            .find(|requirement| requirement.controller_issue == issue)
            .map(|requirement| {
                requirement.mandatory_invariant_ids.iter().map(String::as_str).collect()
            })
            .unwrap_or_default()
    };
    assert_eq!(
        set_for(5900),
        BTreeSet::from([
            "broken_documented_install",
            "false_exact",
            "orphaned_server_or_debuggee",
            "silent_startup_failure",
            "stale_exact",
            "unexplained_success_empty",
            "unsafe_edit",
            "wrong_binary_or_version",
        ])
    );
    assert_eq!(
        set_for(4346),
        BTreeSet::from([
            "blocked_or_not_proven_child_as_pass",
            "broken_documented_install",
            "false_exact",
            "false_repair_diagnosis",
            "mixed_generation_or_root_result",
            "mixed_version_readiness",
            "optional_tool_false_requirement",
            "orphaned_server_or_debuggee",
            "partial_artifact_promotion",
            "silent_startup_failure",
            "stale_exact",
            "unexplained_success_empty",
            "unsafe_edit",
            "unreachable_required_manifest_surface",
            "wrong_binary_or_version",
            "wrong_target_or_silent_fallback",
        ])
    );
    assert_eq!(
        set_for(4350),
        BTreeSet::from([
            "false_exact",
            "missing_required_subject",
            "mixed_generation_or_root_result",
            "mixed_version_readiness",
            "orphaned_candidate_process",
            "partial_or_checksum_invalid_install",
            "stale_exact",
            "unapproved_public_mutation",
            "unexplained_success_empty",
            "unsafe_edit",
            "unreviewed_public_claim",
            "wrong_binary_or_artifact",
            "wrong_target_or_version",
        ])
    );
    let union: BTreeSet<&str> = set_for(5900)
        .union(&set_for(4346))
        .copied()
        .collect::<BTreeSet<_>>()
        .union(&set_for(4350))
        .copied()
        .collect();
    assert_eq!(set_for(4343), union);
    Ok(())
}

#[test]
fn duplicate_invariant_id_fails() -> TestResult {
    let mut registry = canonical_registry()?;
    let duplicate = invariant_mut(&mut registry, "false_exact").expect("false_exact").clone();
    registry["invariants"].as_array_mut().expect("invariants").push(duplicate);
    expect_violation(&registry, "duplicate invariant_id")
}

#[test]
fn mandatory_invariant_without_producer_fails() -> TestResult {
    let mut registry = canonical_registry()?;
    invariant_mut(&mut registry, "false_exact")
        .expect("false_exact")
        .as_object_mut()
        .expect("object")
        .remove("producer_kind");
    expect_violation(&registry, "producer_kind")
}

#[test]
fn owner_issue_missing_fails() -> TestResult {
    let mut registry = canonical_registry()?;
    invariant_mut(&mut registry, "false_exact").expect("false_exact")["owner_issue"] = json!(1);
    expect_violation(&registry, "owner issue 1 is missing")
}

#[test]
fn owner_superseded_without_successor_fails() -> TestResult {
    let mut registry = canonical_registry()?;
    let owner = owner_mut(&mut registry, 3099).expect("3099");
    owner["status"] = json!("superseded");
    expect_violation(&registry, "superseded owner has no successor")
}

#[test]
fn unknown_producer_kind_fails_schema() -> TestResult {
    let mut registry = canonical_registry()?;
    invariant_mut(&mut registry, "false_exact").expect("false_exact")["producer_kind"] =
        json!("not_a_real_producer");
    expect_violation(&registry, "schema")
}

#[test]
fn superseded_producer_without_successor_fails() -> TestResult {
    let mut registry = canonical_registry()?;
    let producer = producer_mut(&mut registry, "provider_decision_receipt").expect("producer");
    producer["status"] = json!("superseded");
    expect_violation(&registry, "superseded producer has no successor")
}

#[test]
fn ownerless_producer_authority_fails() -> TestResult {
    let mut registry = canonical_registry()?;
    let producer = producer_mut(&mut registry, "provider_decision_receipt").expect("producer");
    producer["owner_issue"] = json!(1);
    expect_violation(&registry, "owner issue 1 is missing")
}

#[test]
fn denominator_omitted_fails() -> TestResult {
    let mut registry = canonical_registry()?;
    invariant_mut(&mut registry, "false_exact")
        .expect("false_exact")
        .as_object_mut()
        .expect("object")
        .remove("denominator");
    expect_violation(&registry, "denominator")
}

#[test]
fn applicability_omitted_fails() -> TestResult {
    let mut registry = canonical_registry()?;
    invariant_mut(&mut registry, "false_exact")
        .expect("false_exact")
        .as_object_mut()
        .expect("object")
        .remove("applicability");
    expect_violation(&registry, "applicability")
}

#[test]
fn unknown_negative_control_id_fails() -> TestResult {
    let mut registry = canonical_registry()?;
    invariant_mut(&mut registry, "false_exact").expect("false_exact")["negative_control_ids"] =
        json!(["not_a_named_control"]);
    expect_violation(&registry, "unknown negative_control_id")
}

#[test]
fn controller_mandatory_id_without_row_fails() -> TestResult {
    let mut registry = canonical_registry()?;
    registry["controller_requirements"][0]["mandatory_invariant_ids"]
        .as_array_mut()
        .expect("ids")
        .push(json!("not_a_seeded_invariant"));
    expect_violation(&registry, "has no row")
}
