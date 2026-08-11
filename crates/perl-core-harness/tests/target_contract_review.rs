//! Review falsifiers for target-selector decoding and legacy composite partitioning.

#[path = "../src/target_contracts/model.rs"]
mod model;
#[path = "../src/target_contracts/contract.rs"]
mod contract;
#[path = "../src/target_contracts/matrix.rs"]
mod matrix;
#[path = "../src/target_contracts/io.rs"]
mod io;

use model::{
    CompositeOverlapPolicy, TARGET_MATRIX_SCHEMA_VERSION, TARGET_SELECTION_SCHEMA_VERSION,
    TARGET_TOPOLOGY_DRIFT_SCHEMA_VERSION, TargetAuthority, TargetAuthorityKind,
    TargetDisposition, TargetKind, TargetMatrixEntry, TargetPerlRuntime, TargetPreparation,
    TargetScriptForm, TargetSelectionContract, TargetSelector, TargetTerminalPolicy,
    TargetTopologyDrift, TargetTopologyDriftStatus, UpstreamTargetMatrix,
};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn repo_file(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

fn physical_contract(display_name: &str) -> TargetSelectionContract {
    TargetSelectionContract {
        schema_version: TARGET_SELECTION_SCHEMA_VERSION.to_string(),
        target_id: "component_base".to_string(),
        upstream_name: "t/base".to_string(),
        aliases: Vec::new(),
        display_name: display_name.to_string(),
        perl_version_row: "fixture".to_string(),
        target_kind: TargetKind::PhysicalSeries,
        authority: TargetAuthority {
            kind: TargetAuthorityKind::Test,
            entrypoint: "t/TEST".to_string(),
        },
        selection_authority: Some(TargetAuthority {
            kind: TargetAuthorityKind::Test,
            entrypoint: "t/TEST".to_string(),
        }),
        selectors: vec![TargetSelector::RecursiveRoot {
            path: "base".to_string(),
        }],
        script_forms: vec![TargetScriptForm::DotT],
        preparation: TargetPreparation {
            make_target: Some("test_prep".to_string()),
            perl_runtime: TargetPerlRuntime::FullPerl,
            required_products: Vec::new(),
        },
        variant_of: None,
        composite_members: Vec::new(),
        composite_overlap_policy: None,
        runner_switches: Vec::new(),
        variant_parameters: BTreeMap::new(),
        environment: BTreeMap::new(),
        terminal_policy: TargetTerminalPolicy::NotApplicable,
        capability_predicates: Vec::new(),
        exclusions: Vec::new(),
        replaces_target_id: None,
        change_reason: Some("fixture target".to_string()),
    }
}

fn matrix_with_contract(
    perl_ref: &str,
    resolved_ref: &str,
    source_sha: &str,
    mut contract: TargetSelectionContract,
) -> UpstreamTargetMatrix {
    contract.perl_version_row = perl_ref.to_string();
    UpstreamTargetMatrix {
        schema_version: TARGET_MATRIX_SCHEMA_VERSION.to_string(),
        perl_version_row: perl_ref.to_string(),
        perl_requested_ref: perl_ref.to_string(),
        perl_resolved_ref: resolved_ref.to_string(),
        topology_sources: BTreeMap::from([("t/TEST".to_string(), source_sha.to_string())]),
        targets: vec![TargetMatrixEntry {
            contract,
            disposition: TargetDisposition::Implemented,
            owner_issue: Some(6660),
            claim_boundary: "fixture topology only".to_string(),
        }],
        claim_boundary: "fixture matrix".to_string(),
    }
}

fn compared_drift(
    pinned: &UpstreamTargetMatrix,
    observed: &UpstreamTargetMatrix,
    changed_target_ids: Vec<String>,
) -> Result<TargetTopologyDrift, String> {
    Ok(TargetTopologyDrift {
        schema_version: TARGET_TOPOLOGY_DRIFT_SCHEMA_VERSION.to_string(),
        status: TargetTopologyDriftStatus::Compared,
        pinned_matrix_fingerprint: pinned.fingerprint()?,
        observed_matrix_fingerprint: Some(observed.fingerprint()?),
        observed_perl_ref: observed.perl_requested_ref.clone(),
        observed_perl_resolved_ref: observed.perl_resolved_ref.clone(),
        observed_topology_sources: observed.topology_sources.clone(),
        added_target_ids: Vec::new(),
        removed_target_ids: Vec::new(),
        changed_target_ids,
        not_proven_reason: None,
        claim_boundary: "fixture compared topology".to_string(),
    })
}

#[test]
fn selector_payloads_reject_unknown_fields() {
    let misspelled_path = r#"{"kind":"recursive_root","pth":"base"}"#;
    assert!(serde_json::from_str::<TargetSelector>(misspelled_path).is_err());

    let silently_extra_field =
        r#"{"kind":"recursive_root","path":"base","scope":"recursive"}"#;
    assert!(serde_json::from_str::<TargetSelector>(silently_extra_field).is_err());
}

#[test]
fn legacy_composites_reject_overlap_and_keep_op_hook_disjoint() -> TestResult {
    let matrix = io::read_matrix(&repo_file(
        ".ci/perl-core-harness/upstream-targets-5.42.2.v1",
    ))?;

    let find = |target_id: &str| {
        matrix
            .targets
            .iter()
            .find(|entry| entry.contract.target_id == target_id)
            .map(|entry| &entry.contract)
            .ok_or_else(|| format!("missing target contract {target_id}"))
    };

    let core = find("legacy_custom_core")?;
    let full = find("legacy_custom_full")?;
    let direct_op = find("component_op")?;
    let op_hook = find("component_op_hook")?;

    assert_eq!(
        core.composite_overlap_policy,
        Some(CompositeOverlapPolicy::RejectOverlap)
    );
    assert_eq!(
        full.composite_overlap_policy,
        Some(CompositeOverlapPolicy::RejectOverlap)
    );
    assert!(
        core.composite_members
            .iter()
            .any(|member| member == "component_op")
    );
    assert!(
        core.composite_members
            .iter()
            .any(|member| member == "component_op_hook")
    );
    assert_eq!(
        direct_op.selectors,
        vec![TargetSelector::NonRecursiveGlob {
            pattern: "op/*.t".to_string(),
        }]
    );
    assert_eq!(
        op_hook.selectors,
        vec![TargetSelector::RecursiveRoot {
            path: "op/hook".to_string(),
        }]
    );
    Ok(())
}

#[test]
fn presentation_only_changes_do_not_become_topology_drift() -> TestResult {
    let pinned = matrix_with_contract(
        "fixture",
        "1111111111111111111111111111111111111111",
        "2222222222222222222222222222222222222222",
        physical_contract("original display text"),
    );
    let observed = matrix_with_contract(
        "blead",
        "3333333333333333333333333333333333333333",
        "4444444444444444444444444444444444444444",
        physical_contract("rewritten display text"),
    );
    let drift = compared_drift(&pinned, &observed, Vec::new())?;
    let pinned_fingerprint = pinned.fingerprint()?;

    drift.validate_against(&pinned, &pinned_fingerprint, Some(&observed))?;
    Ok(())
}

#[test]
fn invocation_changes_remain_topology_drift() -> TestResult {
    let pinned = matrix_with_contract(
        "fixture",
        "1111111111111111111111111111111111111111",
        "2222222222222222222222222222222222222222",
        physical_contract("display text"),
    );
    let mut changed = physical_contract("display text");
    changed.runner_switches.push("--changed".to_string());
    let observed = matrix_with_contract(
        "blead",
        "3333333333333333333333333333333333333333",
        "4444444444444444444444444444444444444444",
        changed,
    );
    let drift = compared_drift(
        &pinned,
        &observed,
        vec!["component_base".to_string()],
    )?;
    let pinned_fingerprint = pinned.fingerprint()?;

    drift.validate_against(&pinned, &pinned_fingerprint, Some(&observed))?;
    Ok(())
}
