//! Focused target-matrix invariants.

use super::*;
use crate::contract::{validate_external_selector_for_test, validate_selector_for_test};
use crate::io::{read_drift, read_matrix};
use crate::model::{
    CompositeOverlapPolicy, ManifestPopulation, TARGET_MATRIX_SCHEMA_VERSION,
    TARGET_SELECTION_SCHEMA_VERSION, TARGET_TOPOLOGY_DRIFT_SCHEMA_VERSION, TargetAuthority,
    TargetAuthorityKind, TargetDisposition, TargetKind, TargetMatrixEntry, TargetPerlRuntime,
    TargetPreparation, TargetScriptForm, TargetSelectionContract, TargetSelector,
    TargetTerminalPolicy, TargetTopologyDrift, TargetTopologyDriftStatus, UpstreamTargetMatrix,
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
        selection_authority: Some(TargetAuthority {
            kind: TargetAuthorityKind::Test,
            entrypoint: "t/TEST".to_string(),
        }),
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

fn physical_contract_with_id(target_id: &str, root: &str) -> TargetSelectionContract {
    let mut contract = physical_contract();
    contract.target_id = target_id.to_string();
    contract.upstream_name = format!("t/{root}");
    contract.display_name = format!("fixture {target_id}");
    contract.selectors = vec![TargetSelector::RecursiveRoot { path: root.to_string() }];
    contract
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
    contract.selection_authority = None;
    contract.selectors.clear();
    contract.script_forms.clear();
    contract.preparation.make_target = None;
    contract.preparation.perl_runtime = TargetPerlRuntime::Inherited;
    contract.composite_members = members.iter().map(|member| (*member).to_string()).collect();
    contract.composite_members.sort();
    contract.composite_overlap_policy = Some(CompositeOverlapPolicy::RejectOverlap);
    contract
}

fn preparation_contract(target_id: &str) -> TargetSelectionContract {
    let mut contract = physical_contract();
    contract.target_id = target_id.to_string();
    contract.upstream_name = target_id.to_string();
    contract.display_name = target_id.to_string();
    contract.target_kind = TargetKind::PreparationOnly;
    contract.selection_authority = None;
    contract.selectors.clear();
    contract.script_forms.clear();
    contract.preparation.make_target = Some(target_id.to_string());
    contract.preparation.perl_runtime = TargetPerlRuntime::Inherited;
    contract.variant_of = None;
    contract
}

fn environment_variant(target_id: &str, base: &str) -> TargetSelectionContract {
    let mut contract = physical_contract();
    contract.target_id = target_id.to_string();
    contract.upstream_name = target_id.to_string();
    contract.display_name = target_id.to_string();
    contract.target_kind = TargetKind::EnvironmentVariant;
    contract.selection_authority = None;
    contract.selectors.clear();
    contract.script_forms.clear();
    contract.preparation.make_target = None;
    contract.preparation.perl_runtime = TargetPerlRuntime::Inherited;
    contract.variant_of = Some(base.to_string());
    contract.terminal_policy = TargetTerminalPolicy::NoTty;
    contract
}

fn instrumentation_contract(target_id: &str, base: &str) -> TargetSelectionContract {
    let mut contract = physical_contract();
    contract.target_id = target_id.to_string();
    contract.upstream_name = target_id.to_string();
    contract.display_name = target_id.to_string();
    contract.target_kind = TargetKind::InstrumentationOnly;
    contract.selection_authority = None;
    contract.selectors.clear();
    contract.script_forms.clear();
    contract.preparation.make_target = None;
    contract.preparation.perl_runtime = TargetPerlRuntime::Inherited;
    contract.variant_of = Some(base.to_string());
    contract.environment = BTreeMap::from([("INSTRUMENT".to_string(), "enabled".to_string())]);
    contract.terminal_policy = TargetTerminalPolicy::Inherited;
    contract
}

#[test]
fn checked_in_target_matrix_is_valid_and_stable() -> Result<()> {
    let matrix = read_matrix(&repo_file(".ci/perl-core-harness/upstream-targets-5.42.2.v1"))?;
    assert_eq!(matrix.schema_version, TARGET_MATRIX_SCHEMA_VERSION);
    // The denominator is part of the claim, so state it here rather than
    // leaving it implicit in the fingerprint. Splitting the two runner-agnostic
    // legacy rows into four runner-bound composites moved this from 48 to 50;
    // any further move must be a deliberate edit to this number.
    assert_eq!(matrix.targets.len(), 50);
    let first = matrix.fingerprint().map_err(|error| color_eyre::eyre::eyre!(error))?;
    let second = matrix.fingerprint().map_err(|error| color_eyre::eyre::eyre!(error))?;
    assert_eq!(first, second);
    assert_eq!(first, "4182b36d39baa378790619cbf9d674de40cd895507a412750bd0e5fb0fa583fa");
    Ok(())
}

#[test]
fn checked_in_blead_drift_is_explicitly_not_proven() -> Result<()> {
    let matrix = read_matrix(&repo_file(".ci/perl-core-harness/upstream-targets-5.42.2.v1"))?;
    let drift =
        read_drift(&repo_file(".ci/perl-core-harness/upstream-targets-blead-drift.v1.json"))?;
    assert_eq!(drift.schema_version, TARGET_TOPOLOGY_DRIFT_SCHEMA_VERSION);
    assert_eq!(drift.status, TargetTopologyDriftStatus::NotProven);
    assert_eq!(drift.observed_topology_sources.len(), 3);
    let fingerprint = matrix.fingerprint().map_err(|error| color_eyre::eyre::eyre!(error))?;
    drift
        .validate_against(&matrix, &fingerprint, None)
        .map_err(|error| color_eyre::eyre::eyre!(error))?;

    let mut incomplete = drift.clone();
    incomplete.observed_topology_sources.remove("t/harness");
    assert!(incomplete.validate_against(&matrix, &fingerprint, None).is_err());

    let mut false_conclusion = drift.clone();
    false_conclusion.changed_target_ids.push("component_base".to_string());
    assert!(false_conclusion.validate_against(&matrix, &fingerprint, None).is_err());
    Ok(())
}

#[test]
fn compared_drift_is_recomputed_from_the_observed_matrix() -> Result<()> {
    let pinned = matrix_fixture(vec![
        entry(physical_contract_with_id("component_base", "base"), TargetDisposition::Implemented),
        entry(physical_contract_with_id("component_comp", "comp"), TargetDisposition::Implemented),
    ]);
    let pinned_fingerprint =
        pinned.fingerprint().map_err(|error| color_eyre::eyre::eyre!(error))?;

    let mut changed = physical_contract_with_id("component_base", "base");
    changed.runner_switches = vec!["--changed".to_string()];
    let mut observed = matrix_fixture(vec![
        entry(changed, TargetDisposition::Implemented),
        entry(physical_contract_with_id("component_run", "run"), TargetDisposition::Implemented),
    ]);
    observed.perl_requested_ref = "blead".to_string();
    observed.perl_resolved_ref = "2222222222222222222222222222222222222222".to_string();
    observed.topology_sources = BTreeMap::from([(
        "t/TEST".to_string(),
        "3333333333333333333333333333333333333333".to_string(),
    )]);
    let observed_fingerprint =
        observed.fingerprint().map_err(|error| color_eyre::eyre::eyre!(error))?;

    let drift = TargetTopologyDrift {
        schema_version: TARGET_TOPOLOGY_DRIFT_SCHEMA_VERSION.to_string(),
        status: TargetTopologyDriftStatus::Compared,
        pinned_matrix_fingerprint: pinned_fingerprint.clone(),
        observed_matrix_fingerprint: Some(observed_fingerprint),
        observed_perl_ref: observed.perl_requested_ref.clone(),
        observed_perl_resolved_ref: observed.perl_resolved_ref.clone(),
        observed_topology_sources: observed.topology_sources.clone(),
        added_target_ids: vec!["component_run".to_string()],
        removed_target_ids: vec!["component_comp".to_string()],
        changed_target_ids: vec!["component_base".to_string()],
        not_proven_reason: None,
        claim_boundary: "fixture compared drift".to_string(),
    };
    drift
        .validate_against(&pinned, &pinned_fingerprint, Some(&observed))
        .map_err(|error| color_eyre::eyre::eyre!(error))?;

    let mut false_result = drift.clone();
    false_result.changed_target_ids.clear();
    assert!(false_result.validate_against(&pinned, &pinned_fingerprint, Some(&observed)).is_err());
    Ok(())
}

#[test]
fn pinned_inventory_rejects_silent_row_omission() -> Result<()> {
    let mut matrix = read_matrix(&repo_file(".ci/perl-core-harness/upstream-targets-5.42.2.v1"))?;
    matrix.targets.retain(|entry| entry.contract.target_id != "component_class");
    let error = match matrix.validate() {
        Ok(()) => return Err(color_eyre::eyre::eyre!("missing target was accepted")),
        Err(error) => error,
    };
    assert!(error.contains("component_class"));
    Ok(())
}

#[test]
fn upstream_core_uses_the_filtered_manifest_population() -> Result<()> {
    let matrix = read_matrix(&repo_file(".ci/perl-core-harness/upstream-targets-5.42.2.v1"))?;
    let contract = matrix
        .targets
        .iter()
        .find(|entry| entry.contract.target_id == "selector_test_core")
        .map(|entry| &entry.contract)
        .ok_or_else(|| color_eyre::eyre::eyre!("missing upstream core target"))?;
    assert!(contract.selectors.contains(&TargetSelector::ManifestPopulation {
        component: ManifestPopulation::CoreRootLib,
    }));
    assert!(
        !contract.selectors.contains(&TargetSelector::ManifestPopulation {
            component: ManifestPopulation::RootLib,
        })
    );
    Ok(())
}

#[test]
fn upstream_default_test_is_a_physical_invocation() -> Result<()> {
    let matrix = read_matrix(&repo_file(".ci/perl-core-harness/upstream-targets-5.42.2.v1"))?;
    let contract = matrix
        .targets
        .iter()
        .find(|entry| entry.contract.target_id == "make_test_choose")
        .map(|entry| &entry.contract)
        .ok_or_else(|| color_eyre::eyre::eyre!("missing upstream default test target"))?;
    assert_eq!(contract.target_kind, TargetKind::PhysicalSeries);
    assert_eq!(
        contract.selection_authority.as_ref().map(|authority| authority.kind),
        Some(TargetAuthorityKind::Test)
    );
    assert!(contract.composite_members.is_empty());
    assert!(
        contract
            .selectors
            .contains(&TargetSelector::ManifestPopulation { component: ManifestPopulation::Cpan })
    );
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
fn selector_kind_grammar_accepts_valid_shapes() {
    assert!(
        validate_selector_for_test(&TargetSelector::ExactFile { path: "op/basic.t".to_string() })
            .is_ok()
    );
    assert!(
        validate_selector_for_test(&TargetSelector::RecursiveRoot { path: "op/hook".to_string() })
            .is_ok()
    );
    assert!(
        validate_selector_for_test(&TargetSelector::NonRecursiveGlob {
            pattern: "op/*.t".to_string(),
        })
        .is_ok()
    );
}

#[test]
fn selector_grammar_rejects_empty_path_components() {
    // A trailing slash or a repeated separator names the same path as its
    // trimmed form. Admitting both spellings lets one selector enter the
    // pinned authority twice under different fingerprints, so each is
    // rejected rather than normalised.
    for path in ["op/", "op//hook", "op/hook/"] {
        assert!(
            validate_selector_for_test(&TargetSelector::RecursiveRoot { path: path.to_string() })
                .is_err_and(|error| error.contains("invalid t-relative selector")),
            "recursive-root selector {path:?} must be rejected for an empty component"
        );
    }
    for path in ["op/basic.t/", "op//basic.t"] {
        assert!(
            validate_selector_for_test(&TargetSelector::ExactFile { path: path.to_string() })
                .is_err_and(|error| error.contains("invalid t-relative selector")),
            "exact-file selector {path:?} must be rejected for an empty component"
        );
    }
    assert!(
        validate_selector_for_test(&TargetSelector::NonRecursiveGlob {
            pattern: "op//*.t".to_string()
        })
        .is_err_and(|error| error.contains("invalid t-relative selector")),
        "non-recursive glob must be rejected for an empty component"
    );
    for pattern in ["../ext//re/t/*.t", "../ext/re/t/"] {
        assert!(
            validate_external_selector_for_test(pattern)
                .is_err_and(|error| error.contains("invalid external selector")),
            "external selector {pattern:?} must be rejected for an empty component"
        );
    }

    // Positive controls: the trimmed spellings of the same paths stay valid,
    // so the check rejects empty components rather than these paths.
    assert!(
        validate_selector_for_test(&TargetSelector::RecursiveRoot { path: "op/hook".to_string() })
            .is_ok()
    );
    assert!(
        validate_selector_for_test(&TargetSelector::ExactFile { path: "op/basic.t".to_string() })
            .is_ok()
    );
    assert!(validate_external_selector_for_test("../ext/re/t/*.t").is_ok());
}

#[test]
fn selector_kind_grammar_rejects_wrong_pattern_shapes() {
    assert!(
        validate_selector_for_test(&TargetSelector::ExactFile { path: "op/*.t".to_string() })
            .is_err()
    );
    assert!(
        validate_selector_for_test(&TargetSelector::RecursiveRoot { path: "op/*".to_string() })
            .is_err()
    );
    assert!(
        validate_selector_for_test(&TargetSelector::NonRecursiveGlob {
            pattern: "op/**/*.t".to_string(),
        })
        .is_err()
    );
    assert!(
        validate_selector_for_test(&TargetSelector::NonRecursiveGlob {
            pattern: "op/hook".to_string(),
        })
        .is_err()
    );
}

#[test]
fn matrix_rejects_missing_variant_parent() {
    let mut contract = environment_variant("variant_utf8", "missing_target");
    contract.runner_switches = vec!["--utf8".to_string()];
    let matrix = matrix_fixture(vec![entry(contract, TargetDisposition::Planned)]);
    let error = matrix.validate().expect_err("missing variant parent must be rejected");
    assert!(
        error.contains("references missing or self base target"),
        "unexpected rejection: {error}"
    );
}

#[test]
fn environment_variant_cannot_inherit_from_preparation() {
    let matrix = matrix_fixture(vec![
        entry(preparation_contract("prep_target"), TargetDisposition::PreparationOnly),
        entry(environment_variant("variant_target", "prep_target"), TargetDisposition::Planned),
    ]);
    let error = matrix.validate().expect_err("preparation inheritance must be rejected");
    assert!(error.contains("cannot inherit"), "unexpected rejection: {error}");
}

#[test]
fn instrumentation_cannot_inherit_from_instrumentation() {
    let matrix = matrix_fixture(vec![
        entry(physical_contract_with_id("component_base", "base"), TargetDisposition::Implemented),
        entry(
            instrumentation_contract("instrument_first", "component_base"),
            TargetDisposition::InstrumentationOnly,
        ),
        entry(
            instrumentation_contract("instrument_second", "instrument_first"),
            TargetDisposition::InstrumentationOnly,
        ),
    ]);
    let error = matrix.validate().expect_err("instrumentation inheritance must be rejected");
    assert!(error.contains("cannot inherit"), "unexpected rejection: {error}");
}

#[test]
fn instrumentation_must_declare_an_instrument() -> Result<(), String> {
    // Strip the environment that makes the row an instrument and it becomes a
    // second identity for exactly the same run of its base.
    let mut no_op = instrumentation_contract("instrument_noop", "component_base");
    no_op.environment.clear();
    let matrix = matrix_fixture(vec![
        entry(physical_contract_with_id("component_base", "base"), TargetDisposition::Implemented),
        entry(no_op, TargetDisposition::InstrumentationOnly),
    ]);
    let Err(error) = matrix.validate() else {
        return Err("a no-op instrumentation row must be rejected".to_string());
    };
    if !error.contains("does not declare any instrument") {
        return Err(format!("unexpected rejection reason: {error}"));
    }
    Ok(())
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
fn replacement_requires_a_real_predecessor() {
    let mut successor = physical_contract_with_id("component_base", "base");
    successor.replaces_target_id = Some("missing_predecessor".to_string());
    successor.change_reason = Some("fixture replacement".to_string());
    let matrix = matrix_fixture(vec![entry(successor, TargetDisposition::Implemented)]);
    let error = matrix.validate().expect_err("missing replacement predecessor must be rejected");
    assert!(
        error.contains("missing or self replacement predecessor"),
        "unexpected rejection: {error}"
    );
}

#[test]
fn replacement_requires_a_change_reason() {
    let mut successor = physical_contract_with_id("component_base", "base");
    successor.replaces_target_id = Some("component_comp".to_string());
    successor.change_reason = None;
    let error = successor.validate().expect_err("replacement reason must be required");
    assert!(error.contains("without a change reason"), "unexpected rejection: {error}");
}

#[test]
fn replacement_graph_must_be_acyclic() {
    let mut first = physical_contract_with_id("component_base", "base");
    first.replaces_target_id = Some("component_comp".to_string());
    first.change_reason = Some("fixture first".to_string());
    let mut second = physical_contract_with_id("component_comp", "comp");
    second.replaces_target_id = Some("component_base".to_string());
    second.change_reason = Some("fixture second".to_string());
    let matrix = matrix_fixture(vec![
        entry(first, TargetDisposition::Implemented),
        entry(second, TargetDisposition::Implemented),
    ]);
    let error = matrix.validate().expect_err("replacement cycle must be rejected");
    assert!(error.contains("cycle"), "unexpected rejection: {error}");
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

#[test]
fn canonical_contract_serialization_keeps_presentation_fields() -> Result<(), String> {
    let mut contract = physical_contract();
    contract.perl_version_row = "<version-row>".to_string();
    let value = serde_json::to_value(&contract).map_err(|error| error.to_string())?;
    if !value.as_object().is_some_and(|fields| fields.contains_key("display_name")) {
        return Err("canonical serialization omitted display_name".to_string());
    }
    let decoded: TargetSelectionContract =
        serde_json::from_value(value).map_err(|error| error.to_string())?;
    if decoded != contract {
        return Err("canonical contract serialization did not round-trip".to_string());
    }
    Ok(())
}

// --- Exact error-variant tests for the `validate()` return seams -------------
// Each of these targets one `Err(format!(...))` return path inside
// `TargetSelectionContract::validate()` and asserts the exact error string, so
// a wording change to a rejection reason is caught rather than silently
// shifting the contract surface.

#[test]
fn validate_rejects_unsupported_schema_version_with_exact_message() -> Result<(), String> {
    let mut contract = physical_contract();
    contract.schema_version = "perl_core_harness.target_selection.v0".to_string();
    let Err(error) = contract.validate() else {
        return Err("an unsupported schema version was accepted".to_string());
    };
    assert_eq!(
        error,
        "target component_base uses unsupported schema perl_core_harness.target_selection.v0"
    );
    Ok(())
}

#[test]
fn validate_rejects_alias_repeating_upstream_name_with_exact_message() -> Result<(), String> {
    let mut contract = physical_contract();
    contract.aliases = vec![contract.upstream_name.clone()];
    let Err(error) = contract.validate() else {
        return Err("an alias repeating the upstream name was accepted".to_string());
    };
    assert_eq!(error, "target component_base repeats its upstream name as an alias");
    Ok(())
}

#[test]
fn validate_rejects_make_selection_authority_with_exact_message() -> Result<(), String> {
    let mut contract = physical_contract();
    contract.selection_authority =
        Some(TargetAuthority { kind: TargetAuthorityKind::Make, entrypoint: "t/TEST".to_string() });
    let Err(error) = contract.validate() else {
        return Err("a Make selection authority was accepted".to_string());
    };
    assert_eq!(
        error,
        "target component_base selection authority must name a test scheduler, not a Make target"
    );
    Ok(())
}

#[test]
fn validate_rejects_replacement_without_change_reason_with_exact_message() -> Result<(), String> {
    let mut contract = physical_contract();
    contract.replaces_target_id = Some("component_comp".to_string());
    contract.change_reason = None;
    let Err(error) = contract.validate() else {
        return Err("a replacement without a change reason was accepted".to_string());
    };
    assert_eq!(error, "target component_base replaces another target without a change reason");
    Ok(())
}

#[test]
fn validate_rejects_duplicate_selector_with_exact_message() -> Result<(), String> {
    let mut contract = physical_contract();
    contract.selectors = vec![
        TargetSelector::RecursiveRoot { path: "base".to_string() },
        TargetSelector::RecursiveRoot { path: "base".to_string() },
    ];
    let Err(error) = contract.validate() else {
        return Err("a duplicate selector was accepted".to_string());
    };
    assert_eq!(error, "target component_base contains a duplicate selector");
    Ok(())
}
