//! Focused target-matrix invariants.

use super::*;
use crate::contract::validate_external_selector_for_test;
use crate::model::{
    TARGET_MATRIX_SCHEMA_VERSION, TARGET_SELECTION_SCHEMA_VERSION,
    TARGET_TOPOLOGY_DRIFT_SCHEMA_VERSION, CompositeOverlapPolicy, ManifestPopulation,
    TargetAuthority, TargetAuthorityKind, TargetDisposition, TargetKind, TargetMatrixEntry,
    TargetPerlRuntime, TargetPreparation, TargetScriptForm, TargetSelectionContract,
    TargetSelector, TargetTerminalPolicy,
};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn repo_file(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(relative)
}

fn physical_contract() -> TargetSelectionContract {
    TargetSelectionContract {
        schema_version: TARGET_SELECTION_SCHEMA_VERSION.to_string(),
        target_id: "component_base".to_string(),
        upstream_name: "t/base".to_string(),
        aliases: Vec::new(),
        display_name: "upstream t/base".to_string(),
        perl_version_row: "fixture".to_string(),
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
        composite_overlap_policy: None,
        runner_switches: Vec::new(),
        variant_parameters: BTreeMap::new(),
        environment: BTreeMap::new(),
        terminal_policy: TargetTerminalPolicy::NotApplicable,
        capability_predicates: Vec::new(),
        exclusions: Vec::new(),
        replaces_target_id: None,
        change_reason: Some("fixture".to_string()),
    }
}

fn matrix_fixture(mut entries: Vec<TargetMatrixEntry>) -> UpstreamTargetMatrix {
    entries.sort_by(|left, right| left.contract.target_id.cmp(&right.contract.target_id));
    UpstreamTargetMatrix {
        schema_version: TARGET_MATRIX_SCHEMA_VERSION.to_string(),
        perl_version_row: "fixture".to_string(),
        perl_requested_ref: "fixture".to_string(),
        perl_resolved_ref: "0000000000000000000000000000000000000000".to_string(),
        topology_sources: BTreeMap::from([(
            "t/TEST".to_string(),
            "1111111111111111111111111111111111111111".to_string(),
        )]),
        targets: entries,
        claim_boundary: "fixture".to_string(),
    }
}

fn entry(contract: TargetSelectionContract, disposition: TargetDisposition) -> TargetMatrixEntry {
    TargetMatrixEntry {
        contract,
        disposition,
        owner_issue: Some(6660),
        claim_boundary: "fixture".to_string(),
    }
}

fn composite_contract(target_id: &str, members: &[&str]) -> TargetSelectionContract {
    let mut contract = physical_contract();
    contract.target_id = target_id.to_string();
    contract.upstream_name = target_id.to_string();
    contract.display_name = target_id.to_string();
    contract.target_kind = TargetKind::GeneratedComposite;
    contract.selectors.clear();
    contract.script_forms.clear();
    contract.preparation.make_target = None;
    contract.preparation.perl_runtime = TargetPerlRuntime::Inherited;
    contract.composite_members = members.iter().map(|member| (*member).to_string()).collect();
    contract.composite_members.sort();
    contract.composite_overlap_policy = Some(CompositeOverlapPolicy::RejectOverlap);
    contract
}

#[test]
fn checked_in_target_matrix_is_valid_and_stable() -> Result<()> {
    let matrix = read_matrix(&repo_file(
        ".ci/perl-core-harness/upstream-targets-5.42.2.v1",
    ))?;
    assert_eq!(matrix.schema_version, TARGET_MATRIX_SCHEMA_VERSION);
    let first = matrix
        .fingerprint()
        .map_err(|error| color_eyre::eyre::eyre!(error))?;
    let second = matrix
        .fingerprint()
        .map_err(|error| color_eyre::eyre::eyre!(error))?;
    assert_eq!(first, second);
    assert_eq!(
        first,
        "e693f7263add284195e96f55af9e5bae3231fc50b09c38dbcf782273a3022c0c"
    );
    Ok(())
}

#[test]
fn checked_in_blead_drift_is_bound_to_source_identity() -> Result<()> {
    let matrix = read_matrix(&repo_file(
        ".ci/perl-core-harness/upstream-targets-5.42.2.v1",
    ))?;
    let drift = read_drift(&repo_file(
        ".ci/perl-core-harness/upstream-targets-blead-drift.v1.json",
    ))?;
    assert_eq!(drift.schema_version, TARGET_TOPOLOGY_DRIFT_SCHEMA_VERSION);
    assert_eq!(drift.observed_topology_sources.len(), 3);
    let fingerprint = matrix
        .fingerprint()
        .map_err(|error| color_eyre::eyre::eyre!(error))?;
    drift
        .validate_against(&matrix, &fingerprint)
        .map_err(|error| color_eyre::eyre::eyre!(error))?;
    Ok(())
}

#[test]
fn pinned_inventory_rejects_silent_row_omission() -> Result<()> {
    let mut matrix = read_matrix(&repo_file(
        ".ci/perl-core-harness/upstream-targets-5.42.2.v1",
    ))?;
    matrix
        .targets
        .retain(|entry| entry.contract.target_id != "component_class");
    let error = match matrix.validate() {
        Ok(()) => return Err(color_eyre::eyre::eyre!("missing target was accepted")),
        Err(error) => error,
    };
    assert!(error.contains("component_class"));
    Ok(())
}

#[test]
fn upstream_core_uses_the_filtered_manifest_population() -> Result<()> {
    let matrix = read_matrix(&repo_file(
        ".ci/perl-core-harness/upstream-targets-5.42.2.v1",
    ))?;
    let contract = matrix
        .targets
        .iter()
        .find(|entry| entry.contract.target_id == "selector_test_core")
        .map(|entry| &entry.contract)
        .ok_or_else(|| color_eyre::eyre::eyre!("missing upstream core target"))?;
    assert!(contract.selectors.contains(&TargetSelector::ManifestPopulation {
        component: ManifestPopulation::CoreRootLib,
    }));
    assert!(!contract.selectors.contains(&TargetSelector::ManifestPopulation {
        component: ManifestPopulation::RootLib,
    }));
    Ok(())
}

#[test]
fn physical_contract_requires_a_denominator() {
    let mut contract = physical_contract();
    contract.selectors.clear();
    assert!(contract.validate().is_err());
}

#[test]
fn composite_contract_requires_an_overlap_policy() {
    let mut contract = composite_contract("composite", &["component_base"]);
    contract.composite_overlap_policy = None;
    assert!(contract.validate().is_err());
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
    let matrix = matrix_fixture(vec![entry(contract, TargetDisposition::Planned)]);
    assert!(matrix.validate().is_err());
}

#[test]
fn matrix_rejects_reference_cycles() -> Result<(), String> {
    let first = composite_contract("composite_a", &["composite_b"]);
    let second = composite_contract("composite_b", &["composite_a"]);
    let matrix = matrix_fixture(vec![
        entry(first, TargetDisposition::GeneratedComposite),
        entry(second, TargetDisposition::GeneratedComposite),
    ]);
    let error = match matrix.validate() {
        Ok(()) => return Err("target reference cycle was accepted".to_string()),
        Err(error) => error,
    };
    assert!(error.contains("cycle"));
    Ok(())
}

#[test]
fn runner_switch_order_is_part_of_identity() -> Result<(), String> {
    let mut contract = physical_contract();
    contract.runner_switches = vec!["-Ilib".to_string(), "-MTestInit".to_string()];
    let matrix = matrix_fixture(vec![entry(contract, TargetDisposition::Implemented)]);
    let first = matrix.fingerprint()?;

    let mut reordered = matrix.clone();
    reordered.targets[0].contract.runner_switches.reverse();
    let second = reordered.fingerprint()?;
    assert_ne!(first, second);
    Ok(())
}
