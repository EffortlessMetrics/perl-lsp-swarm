//! Focused target-matrix invariants.

use super::*;
use crate::contract::validate_external_selector_for_test;
use crate::model::{
    TARGET_MATRIX_SCHEMA_VERSION, TARGET_SELECTION_SCHEMA_VERSION,
    TARGET_TOPOLOGY_DRIFT_SCHEMA_VERSION, TargetAuthority, TargetAuthorityKind,
    TargetDisposition, TargetKind, TargetMatrixEntry, TargetPerlRuntime, TargetPreparation,
    TargetScriptForm, TargetSelectionContract, TargetSelector, TargetTerminalPolicy,
};
use std::collections::BTreeMap;

fn repo_file(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(relative)
}

fn physical_contract() -> TargetSelectionContract {
    TargetSelectionContract {
        schema_version: TARGET_SELECTION_SCHEMA_VERSION.to_string(),
        target_id: "component_base".to_string(),
        upstream_name: "t/base".to_string(),
        display_name: "upstream t/base".to_string(),
        perl_version_row: "5.42.2".to_string(),
        target_kind: TargetKind::PhysicalSeries,
        authority: TargetAuthority {
            kind: TargetAuthorityKind::Test,
            entrypoint: "t/TEST".to_string(),
        },
        selectors: vec![TargetSelector::RecursiveRoot { path: "base".to_string() }],
        script_forms: vec![TargetScriptForm::DotT],
        preparation: TargetPreparation {
            make_target: Some("test_prep".to_string()),
            perl_runtime: TargetPerlRuntime::FullPerl,
            required_products: Vec::new(),
        },
        variant_of: None,
        composite_members: Vec::new(),
        runner_switches: Vec::new(),
        environment: BTreeMap::new(),
        terminal_policy: TargetTerminalPolicy::NotApplicable,
        capability_predicates: Vec::new(),
        exclusions: Vec::new(),
        replaces_target_id: None,
        change_reason: Some("fixture".to_string()),
    }
}

#[test]
fn checked_in_target_matrix_is_valid_and_stable() -> Result<()> {
    let matrix = read_matrix(&repo_file(
        ".ci/perl-core-harness/upstream-targets-5.42.2.v1.json",
    ))?;
    assert_eq!(matrix.schema_version, TARGET_MATRIX_SCHEMA_VERSION);
    let first = matrix
        .fingerprint()
        .map_err(|error| color_eyre::eyre::eyre!(error))?;
    let second = matrix
        .fingerprint()
        .map_err(|error| color_eyre::eyre::eyre!(error))?;
    assert_eq!(first, second);
    assert_eq!(first.len(), 64);
    Ok(())
}

#[test]
fn checked_in_blead_drift_is_bound_to_the_pinned_matrix() -> Result<()> {
    let matrix = read_matrix(&repo_file(
        ".ci/perl-core-harness/upstream-targets-5.42.2.v1.json",
    ))?;
    let drift = read_drift(&repo_file(
        ".ci/perl-core-harness/upstream-targets-blead-drift.v1.json",
    ))?;
    assert_eq!(drift.schema_version, TARGET_TOPOLOGY_DRIFT_SCHEMA_VERSION);
    let fingerprint = matrix
        .fingerprint()
        .map_err(|error| color_eyre::eyre::eyre!(error))?;
    drift
        .validate_against(&matrix, &fingerprint)
        .map_err(|error| color_eyre::eyre::eyre!(error))?;
    Ok(())
}

#[test]
fn physical_contract_requires_a_denominator() {
    let mut contract = physical_contract();
    contract.selectors.clear();
    let result = contract.validate();
    assert!(result.is_err());
}

#[test]
fn external_selector_allows_one_parent_boundary() {
    assert!(validate_external_selector_for_test("../ext/re/t/*.t").is_ok());
    assert!(validate_external_selector_for_test("../../outside/*.t").is_err());
}

#[test]
fn matrix_rejects_missing_variant_parent() {
    let mut contract = physical_contract();
    contract.target_id = "variant_utf8".to_string();
    contract.target_kind = TargetKind::EnvironmentVariant;
    contract.variant_of = Some("missing_target".to_string());
    contract.selectors.clear();
    contract.runner_switches = vec!["--utf8".to_string()];
    contract.terminal_policy = TargetTerminalPolicy::Inherited;
    contract.preparation.perl_runtime = TargetPerlRuntime::Inherited;
    let matrix = UpstreamTargetMatrix {
        schema_version: TARGET_MATRIX_SCHEMA_VERSION.to_string(),
        perl_version_row: "5.42.2".to_string(),
        perl_requested_ref: "v5.42.2".to_string(),
        perl_resolved_ref: "b62845c7186b0b6a8e4e83419e6b5ef64ceef3ed".to_string(),
        topology_sources: BTreeMap::from([(
            "t/TEST".to_string(),
            "60c3f01b66a2c82062dc288aa3d336d5531d3b12".to_string(),
        )]),
        targets: vec![TargetMatrixEntry {
            contract,
            disposition: TargetDisposition::Planned,
            owner_issue: Some(6693),
            claim_boundary: "fixture".to_string(),
        }],
        claim_boundary: "fixture".to_string(),
    };
    assert!(matrix.validate().is_err());
}
