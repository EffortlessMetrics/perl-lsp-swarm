//! Mutation fixtures for gate activation truth (#9210).

use serde_json::{Value, json};
use std::error::Error;
use std::path::{Path, PathBuf};
use xtask::activation::{Verdict, evaluate_raw_row};

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from(".."))
}

fn gate_row(surface_id: &str, detail: &str, notes: Option<&str>) -> Value {
    let gate_name = surface_id.strip_prefix("gate:").unwrap_or(surface_id);
    json!({
        "surface_id": surface_id,
        "class": "gate",
        "class_authority": {
            "kind": "derived",
            "authority": ".ci/gate-policy.yaml",
            "rule": "gate-policy-gates"
        },
        "semantic_authority": format!(".ci/gate-policy.yaml#{gate_name}"),
        "consumers": [],
        "compile_profiles": [],
        "registration": {
            "state": "established",
            "authority": ".ci/gate-policy.yaml",
            "detail": detail
        },
        "proof_references": [],
        "publication": { "state": "not_applicable", "authority": ".ci/gate-policy.yaml" },
        "owner": "release/ci",
        "promotion": { "state": "not_evaluated" },
        "notes": notes
    })
}

fn expect_fail(row: &Value, needle: &str) -> TestResult {
    let finding = evaluate_raw_row(row, Some(&repo_root()));
    assert_eq!(finding.verdict, Verdict::Fail, "expected fail, got {finding:?}");
    assert!(
        finding.reasons.iter().any(|reason| reason.contains(needle)),
        "expected `{needle}` in {:?}",
        finding.reasons
    );
    Ok(())
}

fn expect_pass(row: &Value) -> TestResult {
    let finding = evaluate_raw_row(row, Some(&repo_root()));
    assert_eq!(finding.verdict, Verdict::Pass, "expected pass, got fail: {:?}", finding.reasons);
    Ok(())
}

#[test]
fn reachable_required_gate_passes_control_chain() -> TestResult {
    expect_pass(&gate_row("gate:fmt", "tier = \"pr_fast\"; required = true", None))
}

#[test]
fn unreachable_required_gate_registration_fails() -> TestResult {
    expect_fail(
        &gate_row(
            "gate:parser_audit_closeout",
            "tier = \"merge_gate\"; required = true",
            None,
        ),
        "claims required enforcement",
    )
}

#[test]
fn advisory_only_gate_cannot_claim_required_enforcement() -> TestResult {
    expect_fail(
        &gate_row(
            "gate:security_audit",
            "tier = \"merge_gate\"; required = true",
            None,
        ),
        "claims required enforcement",
    )
}

#[test]
fn renamed_registration_context_fails_when_policy_is_advisory() -> TestResult {
    expect_fail(
        &gate_row("gate:security_audit", "tier = \"merge_gate\"; required = true", None),
        "claims required enforcement",
    )
}

#[test]
fn quarantined_advisory_gate_passes_without_claiming_active_required_path() -> TestResult {
    expect_pass(&gate_row(
        "gate:security_audit",
        "tier = \"merge_gate\"; required = false",
        None,
    ))
}

#[test]
fn merge_proof_live_enforcement_claim_before_9197_fails() -> TestResult {
    expect_fail(
        &gate_row(
            "gate:fmt",
            "tier = \"pr_fast\"; required = true",
            Some("Merge Proof Finalizer has live enforcement on main via github-ruleset"),
        ),
        "merge-proof transition",
    )
}
