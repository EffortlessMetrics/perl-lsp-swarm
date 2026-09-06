//! Focused target-matrix invariants.

use super::*;
use crate::contract::{validate_external_selector_for_test, validate_selector_for_test};
use crate::io::{read_drift, read_matrix};
use crate::model::{
    CONTROLLING_TERMINAL_CAPABILITY, CompositeOverlapPolicy, HARNESS_DISPLAY_NO_TTY_ENV,
    ManifestPopulation, RUNTESTS_NO_TTY_ENV, TARGET_MATRIX_INDEX_SCHEMA_VERSION,
    TARGET_MATRIX_PART_SCHEMA_VERSION, TARGET_MATRIX_SCHEMA_VERSION,
    TARGET_SELECTION_SCHEMA_VERSION, TARGET_TOPOLOGY_DRIFT_SCHEMA_VERSION, TargetAuthority,
    TargetAuthorityKind, TargetDisposition, TargetExclusion, TargetKind, TargetMatrixEntry,
    TargetMatrixIndex, TargetMatrixPart, TargetPerlRuntime, TargetPreparation, TargetScriptForm,
    TargetSelectionContract, TargetSelector, TargetTerminalPolicy, TargetTopologyDrift,
    TargetTopologyDriftStatus, UpstreamTargetMatrix,
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
    // A no-TTY row must name the mechanism that selects the no-terminal path.
    contract.environment = BTreeMap::from([(RUNTESTS_NO_TTY_ENV.to_string(), "1".to_string())]);
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
    assert_eq!(first, "c60ee4e521957e9ff7f2b1a85bba9ab929acae8a1004ebd3ea9e16683692eb70");
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
fn receipt_identities_reject_casing_only_mutations() -> Result<()> {
    // #7725 final control: an equivalent byte value under a different
    // textual spelling cannot produce another valid authoritative receipt.
    // The only mutation below is identity casing.
    let matrix = matrix_fixture(vec![entry(
        physical_contract_with_id("component_base", "base"),
        TargetDisposition::Implemented,
    )]);
    let fingerprint = matrix.fingerprint().map_err(|error| color_eyre::eyre::eyre!(error))?;
    let mut uppercased_fingerprint = fingerprint.clone();
    let first_nibble = uppercased_fingerprint.remove(0);
    uppercased_fingerprint.insert(0, first_nibble.to_ascii_uppercase());

    let lowercased_blob_sha = "1111111111111111111111111111111111111111".to_string();
    let mut uppercased_blob_sha = lowercased_blob_sha.clone();
    uppercased_blob_sha.replace_range(0..1, "A");

    let drift = |fingerprint: String, blob_sha: String| TargetTopologyDrift {
        schema_version: TARGET_TOPOLOGY_DRIFT_SCHEMA_VERSION.to_string(),
        status: TargetTopologyDriftStatus::NotProven,
        pinned_matrix_fingerprint: fingerprint,
        observed_matrix_fingerprint: None,
        observed_perl_ref: "blead".to_string(),
        observed_perl_resolved_ref: "2222222222222222222222222222222222222222".to_string(),
        observed_topology_sources: BTreeMap::from([("t/TEST".to_string(), blob_sha)]),
        added_target_ids: vec![],
        removed_target_ids: vec![],
        changed_target_ids: vec![],
        not_proven_reason: Some("fixture casing control".to_string()),
        claim_boundary: "fixture casing control".to_string(),
    };

    let canonical = drift(fingerprint.clone(), lowercased_blob_sha.clone());
    canonical
        .validate_against(&matrix, canonical.pinned_matrix_fingerprint.as_str(), None)
        .map_err(|error| color_eyre::eyre::eyre!(error))?;

    let fingerprint_mutation = drift(uppercased_fingerprint.clone(), lowercased_blob_sha.clone());
    assert!(fingerprint_mutation.validate_against(&matrix, &uppercased_fingerprint, None).is_err());

    let referenced_identity_mutation = drift(fingerprint.clone(), uppercased_blob_sha);
    assert!(referenced_identity_mutation.validate_against(&matrix, &fingerprint, None).is_err());
    Ok(())
}

#[test]
fn matrix_validation_rejects_duplicate_target_id_rows() -> Result<()> {
    let mut matrix = read_matrix(&repo_file(".ci/perl-core-harness/upstream-targets-5.42.2.v1"))?;
    matrix.targets.push(matrix.targets[0].clone());
    matrix.targets.sort_by(|left, right| left.contract.target_id.cmp(&right.contract.target_id));

    let error = matrix
        .validate()
        .expect_err("duplicate target ID row was accepted by structural validation");
    assert_eq!(error, "target matrix rows must be strictly sorted by target ID");
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
// Every fixture below keeps earlier guards valid and changes one input at a
// time.  This makes each assertion load-bearing: a mutation that removes or
// changes the targeted rejection cannot fall through to a different error.

fn expect_exact_error(result: Result<(), String>, expected: &str) -> Result<(), String> {
    match result {
        Ok(()) => Err(format!("expected rejection {expected:?}, but validation succeeded")),
        Err(actual) if actual == expected => Ok(()),
        Err(actual) => Err(format!("expected {expected:?}, got {actual:?}")),
    }
}

#[test]
fn validate_rejects_unsupported_schema_version_with_exact_message() -> Result<(), String> {
    let mut contract = physical_contract();
    contract.schema_version = "perl_core_harness.target_selection.v0".to_string();

    assert_eq!(
        contract.validate().expect_err("unsupported schema must be rejected"),
        "target component_base uses unsupported schema perl_core_harness.target_selection.v0"
    );
    Ok(())
}

#[test]
fn validate_rejects_repeated_upstream_alias_with_exact_message() -> Result<(), String> {
    let mut contract = physical_contract();
    contract.aliases = vec![contract.upstream_name.clone()];

    assert_eq!(
        contract.validate().expect_err("repeated upstream alias must be rejected"),
        "target component_base repeats its upstream name as an alias"
    );
    Ok(())
}

#[test]
fn validate_exact_error_variants_for_common_fields() -> Result<(), String> {
    let mut target_id = physical_contract();
    target_id.target_id = "component-base".to_string();
    expect_exact_error(target_id.validate(), "target ID must match [a-z0-9_]+: component-base")?;

    let mut upstream_name = physical_contract();
    upstream_name.upstream_name.clear();
    expect_exact_error(upstream_name.validate(), "upstream name cannot be empty")?;

    let mut aliases = physical_contract();
    aliases.aliases = vec!["duplicate".to_string(), "duplicate".to_string()];
    expect_exact_error(
        aliases.validate(),
        "target alias values must be strictly sorted and unique",
    )?;

    let mut display_name = physical_contract();
    display_name.display_name.clear();
    expect_exact_error(display_name.validate(), "display name cannot be empty")?;

    let mut perl_version = physical_contract();
    perl_version.perl_version_row.clear();
    expect_exact_error(perl_version.validate(), "Perl version row cannot be empty")?;

    let mut authority = physical_contract();
    authority.authority.entrypoint.clear();
    expect_exact_error(authority.validate(), "authority entrypoint cannot be empty")?;

    let mut selection_authority = physical_contract();
    selection_authority.selection_authority =
        Some(TargetAuthority { kind: TargetAuthorityKind::Test, entrypoint: String::new() });
    expect_exact_error(
        selection_authority.validate(),
        "selection authority entrypoint cannot be empty",
    )?;

    let mut make_authority = physical_contract();
    make_authority.selection_authority =
        Some(TargetAuthority { kind: TargetAuthorityKind::Make, entrypoint: "t/TEST".to_string() });
    expect_exact_error(
        make_authority.validate(),
        "target component_base selection authority must name a test scheduler, not a Make target",
    )?;

    let mut variant_id = physical_contract();
    variant_id.variant_of = Some("component-base".to_string());
    expect_exact_error(
        variant_id.validate(),
        "variant target ID must match [a-z0-9_]+: component-base",
    )?;

    let mut replacement_id = physical_contract();
    replacement_id.replaces_target_id = Some("component-base".to_string());
    expect_exact_error(
        replacement_id.validate(),
        "replaced target ID must match [a-z0-9_]+: component-base",
    )?;

    let mut replacement_reason = physical_contract();
    replacement_reason.replaces_target_id = Some("component_comp".to_string());
    replacement_reason.change_reason = None;
    expect_exact_error(
        replacement_reason.validate(),
        "target component_base replaces another target without a change reason",
    )?;

    let mut empty_reason = physical_contract();
    empty_reason.change_reason = Some(String::new());
    expect_exact_error(empty_reason.validate(), "change reason cannot be empty")?;

    let mut duplicate_selector = physical_contract();
    duplicate_selector.selectors = vec![
        TargetSelector::RecursiveRoot { path: "base".to_string() },
        TargetSelector::RecursiveRoot { path: "base".to_string() },
    ];
    expect_exact_error(
        duplicate_selector.validate(),
        "target component_base contains a duplicate selector",
    )?;

    let mut script_forms = physical_contract();
    script_forms.script_forms = vec![TargetScriptForm::TestPl, TargetScriptForm::DotT];
    expect_exact_error(
        script_forms.validate(),
        "target component_base script forms must be strictly sorted and unique",
    )?;

    let mut composite_members = physical_contract();
    composite_members.composite_members =
        vec!["component_b".to_string(), "component_a".to_string()];
    expect_exact_error(
        composite_members.validate(),
        "composite member values must be strictly sorted and unique",
    )?;

    let mut runner_switches = physical_contract();
    runner_switches.runner_switches = vec!["--core".to_string(), "--core".to_string()];
    expect_exact_error(runner_switches.validate(), "runner switch values must be unique")?;

    let mut capability_predicates = physical_contract();
    capability_predicates.capability_predicates = vec!["cap_b".to_string(), "cap_a".to_string()];
    expect_exact_error(
        capability_predicates.validate(),
        "capability predicate values must be strictly sorted and unique",
    )?;

    let mut required_products = physical_contract();
    required_products.preparation.required_products = vec!["b".to_string(), "a".to_string()];
    expect_exact_error(
        required_products.validate(),
        "required product values must be strictly sorted and unique",
    )?;

    let mut variant_parameters = physical_contract();
    variant_parameters.variant_parameters.insert(String::new(), "value".to_string());
    expect_exact_error(variant_parameters.validate(), "variant parameter key cannot be empty")?;

    let mut environment = physical_contract();
    environment.environment.insert("MODE".to_string(), String::new());
    expect_exact_error(environment.validate(), "environment value cannot be empty")?;

    let mut exclusion_subject = physical_contract();
    exclusion_subject.exclusions.push(TargetExclusion {
        subject: String::new(),
        reason_code: "known_gap".to_string(),
        claim_impact: "not counted".to_string(),
    });
    expect_exact_error(exclusion_subject.validate(), "exclusion subject cannot be empty")?;

    let mut exclusion_reason = physical_contract();
    exclusion_reason.exclusions.push(TargetExclusion {
        subject: "fixture".to_string(),
        reason_code: "known-gap".to_string(),
        claim_impact: "not counted".to_string(),
    });
    expect_exact_error(
        exclusion_reason.validate(),
        "exclusion reason must match [a-z0-9_]+: known-gap",
    )?;

    let mut exclusion_impact = physical_contract();
    exclusion_impact.exclusions.push(TargetExclusion {
        subject: "fixture".to_string(),
        reason_code: "known_gap".to_string(),
        claim_impact: String::new(),
    });
    expect_exact_error(exclusion_impact.validate(), "exclusion claim impact cannot be empty")?;

    Ok(())
}

#[test]
fn validate_exact_error_variants_for_target_kinds() -> Result<(), String> {
    let mut physical_authority = physical_contract();
    physical_authority.selection_authority = None;
    expect_exact_error(
        physical_authority.validate(),
        "physical target component_base requires a selection authority, selectors, and script forms",
    )?;

    let mut physical_selectors = physical_contract();
    physical_selectors.selectors.clear();
    expect_exact_error(
        physical_selectors.validate(),
        "physical target component_base requires a selection authority, selectors, and script forms",
    )?;

    let mut physical_scripts = physical_contract();
    physical_scripts.script_forms.clear();
    expect_exact_error(
        physical_scripts.validate(),
        "physical target component_base requires a selection authority, selectors, and script forms",
    )?;

    let mut physical_variant = physical_contract();
    physical_variant.variant_of = Some("component_parent".to_string());
    expect_exact_error(
        physical_variant.validate(),
        "physical target component_base cannot be a variant or composite",
    )?;

    let mut physical_composite = physical_contract();
    physical_composite.composite_members = vec!["component_parent".to_string()];
    expect_exact_error(
        physical_composite.validate(),
        "physical target component_base cannot be a variant or composite",
    )?;

    let mut physical_overlap = physical_contract();
    physical_overlap.composite_overlap_policy = Some(CompositeOverlapPolicy::RejectOverlap);
    expect_exact_error(
        physical_overlap.validate(),
        "physical target component_base cannot be a variant or composite",
    )?;

    let mut physical_parameters = physical_contract();
    physical_parameters.variant_parameters.insert("mode".to_string(), "fast".to_string());
    expect_exact_error(
        physical_parameters.validate(),
        "physical target component_base cannot define variant parameters",
    )?;

    let mut selector_base = physical_contract();
    selector_base.target_kind = TargetKind::SelectorVariant;
    selector_base.variant_of = None;
    expect_exact_error(
        selector_base.validate(),
        "selector variant component_base requires a base target, selection authority, selectors, and script forms",
    )?;

    let mut selector_selectors = physical_contract();
    selector_selectors.target_kind = TargetKind::SelectorVariant;
    selector_selectors.variant_of = Some("component_parent".to_string());
    selector_selectors.selectors.clear();
    expect_exact_error(
        selector_selectors.validate(),
        "selector variant component_base requires a base target, selection authority, selectors, and script forms",
    )?;

    let mut selector_scripts = physical_contract();
    selector_scripts.target_kind = TargetKind::SelectorVariant;
    selector_scripts.variant_of = Some("component_parent".to_string());
    selector_scripts.script_forms.clear();
    expect_exact_error(
        selector_scripts.validate(),
        "selector variant component_base requires a base target, selection authority, selectors, and script forms",
    )?;

    let mut selector_authority = physical_contract();
    selector_authority.target_kind = TargetKind::SelectorVariant;
    selector_authority.variant_of = Some("component_parent".to_string());
    selector_authority.selection_authority = None;
    expect_exact_error(
        selector_authority.validate(),
        "selector variant component_base requires a base target, selection authority, selectors, and script forms",
    )?;

    let mut selector_members = physical_contract();
    selector_members.target_kind = TargetKind::SelectorVariant;
    selector_members.variant_of = Some("component_parent".to_string());
    selector_members.composite_members = vec!["component_child".to_string()];
    expect_exact_error(
        selector_members.validate(),
        "selector variant component_base cannot contain composite state",
    )?;

    let mut selector_overlap = physical_contract();
    selector_overlap.target_kind = TargetKind::SelectorVariant;
    selector_overlap.variant_of = Some("component_parent".to_string());
    selector_overlap.composite_overlap_policy = Some(CompositeOverlapPolicy::RejectOverlap);
    expect_exact_error(
        selector_overlap.validate(),
        "selector variant component_base cannot contain composite state",
    )?;

    let mut environment_base = environment_variant("environment_child", "component_base");
    environment_base.variant_of = None;
    expect_exact_error(
        environment_base.validate(),
        "environment variant environment_child must inherit one target without new selectors",
    )?;

    let mut environment_selectors = environment_variant("environment_child", "component_base");
    environment_selectors.selectors =
        vec![TargetSelector::ExactFile { path: "base/if.t".to_string() }];
    expect_exact_error(
        environment_selectors.validate(),
        "environment variant environment_child must inherit one target without new selectors",
    )?;

    let mut environment_members = environment_variant("environment_child", "component_base");
    environment_members.composite_members = vec!["component_child".to_string()];
    expect_exact_error(
        environment_members.validate(),
        "environment variant environment_child cannot contain composite state",
    )?;

    let mut environment_overlap = environment_variant("environment_child", "component_base");
    environment_overlap.composite_overlap_policy = Some(CompositeOverlapPolicy::RejectOverlap);
    expect_exact_error(
        environment_overlap.validate(),
        "environment variant environment_child cannot contain composite state",
    )?;

    let mut environment_no_change = environment_variant("environment_child", "component_base");
    environment_no_change.environment.clear();
    environment_no_change.terminal_policy = TargetTerminalPolicy::Inherited;
    expect_exact_error(
        environment_no_change.validate(),
        "environment variant environment_child does not change any declared invocation input",
    )?;

    let mut preparation_selectors = preparation_contract("prepare");
    preparation_selectors.selectors =
        vec![TargetSelector::ExactFile { path: "base/if.t".to_string() }];
    expect_exact_error(
        preparation_selectors.validate(),
        "preparation target prepare cannot define selectors",
    )?;

    let mut preparation_scripts = preparation_contract("prepare");
    preparation_scripts.script_forms = vec![TargetScriptForm::DotT];
    expect_exact_error(
        preparation_scripts.validate(),
        "preparation target prepare cannot define script forms",
    )?;

    let mut preparation_make = preparation_contract("prepare");
    preparation_make.preparation.make_target = None;
    expect_exact_error(
        preparation_make.validate(),
        "preparation target prepare requires a Make target",
    )?;

    let mut preparation_authority = preparation_contract("prepare");
    preparation_authority.selection_authority =
        Some(TargetAuthority { kind: TargetAuthorityKind::Test, entrypoint: "t/TEST".to_string() });
    expect_exact_error(
        preparation_authority.validate(),
        "preparation target prepare cannot define a selection authority",
    )?;

    let mut preparation_variant = preparation_contract("prepare");
    preparation_variant.variant_of = Some("component_base".to_string());
    expect_exact_error(
        preparation_variant.validate(),
        "preparation target prepare cannot define a variant base",
    )?;

    let mut preparation_members = preparation_contract("prepare");
    preparation_members.composite_members = vec!["component_base".to_string()];
    expect_exact_error(
        preparation_members.validate(),
        "preparation target prepare cannot define composite members",
    )?;

    let mut preparation_overlap = preparation_contract("prepare");
    preparation_overlap.composite_overlap_policy = Some(CompositeOverlapPolicy::RejectOverlap);
    expect_exact_error(
        preparation_overlap.validate(),
        "preparation target prepare cannot define an overlap policy",
    )?;

    let mut preparation_parameters = preparation_contract("prepare");
    preparation_parameters.variant_parameters.insert("mode".to_string(), "fast".to_string());
    expect_exact_error(
        preparation_parameters.validate(),
        "preparation target prepare cannot define variant parameters",
    )?;

    let composite_members = composite_contract("composite", &[]);
    expect_exact_error(
        composite_members.validate(),
        "composite target composite requires at least one member",
    )?;

    let mut composite_selectors = composite_contract("composite", &["component_base"]);
    composite_selectors.selectors =
        vec![TargetSelector::ExactFile { path: "base/if.t".to_string() }];
    expect_exact_error(
        composite_selectors.validate(),
        "composite target composite cannot declare selectors",
    )?;

    let mut composite_scripts = composite_contract("composite", &["component_base"]);
    composite_scripts.script_forms = vec![TargetScriptForm::DotT];
    expect_exact_error(
        composite_scripts.validate(),
        "composite target composite cannot declare script forms",
    )?;

    let mut composite_variant = composite_contract("composite", &["component_base"]);
    composite_variant.variant_of = Some("component_parent".to_string());
    expect_exact_error(
        composite_variant.validate(),
        "composite target composite cannot declare a variant base",
    )?;

    let mut composite_authority = composite_contract("composite", &["component_base"]);
    composite_authority.selection_authority =
        Some(TargetAuthority { kind: TargetAuthorityKind::Test, entrypoint: "t/TEST".to_string() });
    expect_exact_error(
        composite_authority.validate(),
        "composite target composite cannot declare a selection authority",
    )?;

    let mut composite_overlap = composite_contract("composite", &["component_base"]);
    composite_overlap.composite_overlap_policy = None;
    expect_exact_error(
        composite_overlap.validate(),
        "composite target composite requires an explicit overlap policy",
    )?;

    let mut composite_parameters = composite_contract("composite", &["component_base"]);
    composite_parameters.variant_parameters.insert("mode".to_string(), "fast".to_string());
    expect_exact_error(
        composite_parameters.validate(),
        "composite target composite cannot declare variant parameters",
    )?;

    let mut instrumentation_base = instrumentation_contract("instrumented", "component_base");
    instrumentation_base.variant_of = None;
    expect_exact_error(
        instrumentation_base.validate(),
        "instrumentation target instrumented must reference one existing target",
    )?;

    let mut instrumentation_selectors = instrumentation_contract("instrumented", "component_base");
    instrumentation_selectors.selectors =
        vec![TargetSelector::ExactFile { path: "base/if.t".to_string() }];
    expect_exact_error(
        instrumentation_selectors.validate(),
        "instrumentation target instrumented must reference one existing target",
    )?;

    let mut instrumentation_scripts = instrumentation_contract("instrumented", "component_base");
    instrumentation_scripts.script_forms = vec![TargetScriptForm::DotT];
    expect_exact_error(
        instrumentation_scripts.validate(),
        "instrumentation target instrumented must reference one existing target",
    )?;

    let mut instrumentation_members = instrumentation_contract("instrumented", "component_base");
    instrumentation_members.composite_members = vec!["component_base".to_string()];
    expect_exact_error(
        instrumentation_members.validate(),
        "instrumentation target instrumented must reference one existing target",
    )?;

    let mut instrumentation_overlap = instrumentation_contract("instrumented", "component_base");
    instrumentation_overlap.composite_overlap_policy = Some(CompositeOverlapPolicy::RejectOverlap);
    expect_exact_error(
        instrumentation_overlap.validate(),
        "instrumentation target instrumented must reference one existing target",
    )?;

    let mut instrumentation_authority = instrumentation_contract("instrumented", "component_base");
    instrumentation_authority.selection_authority =
        Some(TargetAuthority { kind: TargetAuthorityKind::Test, entrypoint: "t/TEST".to_string() });
    expect_exact_error(
        instrumentation_authority.validate(),
        "instrumentation target instrumented must reference one existing target",
    )?;

    let mut instrumentation_none = instrumentation_contract("instrumented", "component_base");
    instrumentation_none.environment.clear();
    instrumentation_none.capability_predicates.clear();
    instrumentation_none.runner_switches.clear();
    instrumentation_none.variant_parameters.clear();
    expect_exact_error(
        instrumentation_none.validate(),
        "instrumentation target instrumented does not declare any instrument",
    )?;

    Ok(())
}

#[test]
fn selector_and_collection_errors_are_exact_and_load_bearing() -> Result<(), String> {
    expect_exact_error(
        validate_selector_for_test(&TargetSelector::RecursiveRoot { path: String::new() }),
        "local selector cannot be empty",
    )?;
    expect_exact_error(
        validate_selector_for_test(&TargetSelector::RecursiveRoot { path: "op/*.t".to_string() }),
        "recursive-root selector cannot contain glob metacharacters: op/*.t",
    )?;
    expect_exact_error(
        validate_selector_for_test(&TargetSelector::ExactFile { path: "op//basic.t".to_string() }),
        "invalid t-relative selector op//basic.t",
    )?;
    expect_exact_error(
        validate_selector_for_test(&TargetSelector::NonRecursiveGlob {
            pattern: "op/**/*.t".to_string(),
        }),
        "non-recursive glob cannot contain **: op/**/*.t",
    )?;
    expect_exact_error(
        validate_selector_for_test(&TargetSelector::NonRecursiveGlob {
            pattern: "op/basic.t".to_string(),
        }),
        "non-recursive glob must contain a glob pattern: op/basic.t",
    )?;
    expect_exact_error(
        validate_external_selector_for_test("ext/*.t"),
        "external selector must begin with ../: ext/*.t",
    )?;
    expect_exact_error(
        validate_external_selector_for_test("../"),
        "invalid external selector ../",
    )?;
    expect_exact_error(
        validate_external_selector_for_test("../ext//re/*.t"),
        "invalid external selector ../ext//re/*.t",
    )?;

    let mut empty_runner_switch = physical_contract();
    empty_runner_switch.runner_switches = vec![String::new()];
    expect_exact_error(empty_runner_switch.validate(), "runner switch cannot be empty")?;

    let mut empty_capability = physical_contract();
    empty_capability.capability_predicates = vec![String::new()];
    expect_exact_error(empty_capability.validate(), "capability predicate cannot be empty")?;

    let mut empty_composite_member = physical_contract();
    empty_composite_member.composite_members = vec![String::new()];
    expect_exact_error(empty_composite_member.validate(), "composite member cannot be empty")?;

    let mut empty_required_product = physical_contract();
    empty_required_product.preparation.required_products = vec![String::new()];
    expect_exact_error(empty_required_product.validate(), "required product cannot be empty")?;

    let mut duplicate_capability = physical_contract();
    duplicate_capability.capability_predicates =
        vec!["capability".to_string(), "capability".to_string()];
    expect_exact_error(
        duplicate_capability.validate(),
        "capability predicate values must be strictly sorted and unique",
    )?;

    Ok(())
}

// --- Index assembly and part seams -------------------------------------------
// `read_matrix` on the checked-in bundle only reaches `assemble` with exactly
// the declared parts, so the mismatch rejection and the combination contract
// need their own focused oracles.

fn matrix_index_fixture(target_files: Vec<String>) -> TargetMatrixIndex {
    TargetMatrixIndex {
        schema_version: TARGET_MATRIX_INDEX_SCHEMA_VERSION.to_string(),
        perl_version_row: "fixture".to_string(),
        perl_requested_ref: "fixture".to_string(),
        perl_resolved_ref: "0000000000000000000000000000000000000000".to_string(),
        topology_sources: BTreeMap::from([(
            "t/TEST".to_string(),
            "1111111111111111111111111111111111111111".to_string(),
        )]),
        target_files,
        claim_boundary: "fixture index".to_string(),
    }
}

fn matrix_part_fixture(mut entries: Vec<TargetMatrixEntry>) -> TargetMatrixPart {
    entries.sort_by(|left, right| left.contract.target_id.cmp(&right.contract.target_id));
    TargetMatrixPart {
        schema_version: TARGET_MATRIX_PART_SCHEMA_VERSION.to_string(),
        targets: entries,
    }
}

#[test]
fn index_assemble_rejects_part_count_mismatch_with_exact_message() -> Result<(), String> {
    let index = matrix_index_fixture(vec!["01-components-a.json".to_string()]);

    expect_exact_error(
        index.assemble(Vec::new()).map(|_| ()),
        "target matrix loaded 0 parts but index declares 1",
    )?;
    expect_exact_error(
        index
            .assemble(vec![matrix_part_fixture(Vec::new()), matrix_part_fixture(Vec::new())])
            .map(|_| ()),
        "target matrix loaded 2 parts but index declares 1",
    )?;
    Ok(())
}

#[test]
fn index_assemble_combines_parts_into_the_declared_matrix() -> Result<(), String> {
    let index = matrix_index_fixture(vec![
        "01-components-a.json".to_string(),
        "02-components-b.json".to_string(),
    ]);
    let first_part = matrix_part_fixture(vec![entry(
        physical_contract_with_id("component_base", "base"),
        TargetDisposition::Implemented,
    )]);
    let second_part = matrix_part_fixture(vec![
        entry(physical_contract_with_id("component_comp", "comp"), TargetDisposition::Implemented),
        entry(physical_contract_with_id("component_run", "run"), TargetDisposition::Implemented),
    ]);

    let matrix = index.assemble(vec![first_part, second_part])?;
    assert_eq!(matrix.schema_version, TARGET_MATRIX_SCHEMA_VERSION);
    assert_eq!(matrix.perl_version_row, "fixture");
    assert_eq!(matrix.claim_boundary, "fixture index");
    assert_eq!(
        matrix.topology_sources.get("t/TEST").map(String::as_str),
        Some("1111111111111111111111111111111111111111")
    );
    let target_ids: Vec<&str> =
        matrix.targets.iter().map(|row| row.contract.target_id.as_str()).collect();
    assert_eq!(target_ids, vec!["component_base", "component_comp", "component_run"]);
    Ok(())
}

#[test]
fn matrix_part_rejects_empty_and_unsorted_rows_with_exact_messages() -> Result<(), String> {
    expect_exact_error(
        matrix_part_fixture(Vec::new()).validate(),
        "target matrix part contains no rows",
    )?;

    let unsorted = TargetMatrixPart {
        schema_version: TARGET_MATRIX_PART_SCHEMA_VERSION.to_string(),
        targets: vec![
            entry(
                physical_contract_with_id("component_run", "run"),
                TargetDisposition::Implemented,
            ),
            entry(
                physical_contract_with_id("component_comp", "comp"),
                TargetDisposition::Implemented,
            ),
        ],
    };
    expect_exact_error(
        unsorted.validate(),
        "target matrix part rows must be strictly sorted by target ID",
    )?;
    Ok(())
}

// #6702: a row states its terminal behavior three times — the policy, the
// capability predicate that gates which hosts may run it, and the environment
// variable that actually selects the no-terminal path. These prove the three
// statements cannot disagree, so a terminal variant can never be admitted on a
// host that cannot satisfy it and report as an ordinary pass.
//
// Upstream Perl 5.42.2 ground truth, from `Makefile.SH` and `runtests.SH`:
//
//   test / check          runtests choose
//   test_tty              runtests tty          -> stdin from /dev/tty
//   test_notty            runtests no-tty       -> PERL_SKIP_TTY_TEST=1
//   test_harness          runtests choose
//   test_harness_notty    HARNESS_NOTTY=1 ... runtests choose
//   minitest              TEST ... </dev/tty
//   minitest_notty        PERL_SKIP_TTY_TEST=1 TEST ...
//
// So `PERL_SKIP_TTY_TEST` is the only mechanism that resolves the terminal, and
// `HARNESS_NOTTY` only formats harness output.

/// Build a forced-TTY row that satisfies the gate, so each falsifier below
/// changes exactly one fact away from a known-good contract.
fn forced_tty_contract() -> TargetSelectionContract {
    let mut contract = physical_contract();
    contract.target_id = "forced_tty".to_string();
    contract.terminal_policy = TargetTerminalPolicy::Tty;
    contract.capability_predicates = vec![CONTROLLING_TERMINAL_CAPABILITY.to_string()];
    contract
}

/// Positive control: the good rows are accepted, so the falsifiers below fail
/// for the fact they mutate rather than for an unrelated defect.
#[test]
fn coherent_terminal_rows_are_accepted() -> Result<(), String> {
    forced_tty_contract().validate()?;

    let mut forced_no_tty = physical_contract();
    forced_no_tty.terminal_policy = TargetTerminalPolicy::NoTty;
    forced_no_tty.environment =
        BTreeMap::from([(RUNTESTS_NO_TTY_ENV.to_string(), "1".to_string())]);
    forced_no_tty.validate()?;

    // The shape of upstream `test_harness_notty`: it sets the harness display
    // variable and still resolves its terminal by `choose`.
    let mut harness_display = physical_contract();
    harness_display.terminal_policy = TargetTerminalPolicy::Choose;
    harness_display.environment =
        BTreeMap::from([(HARNESS_DISPLAY_NO_TTY_ENV.to_string(), "1".to_string())]);
    harness_display.validate()?;

    // An unrelated environment variable is not a terminal mechanism either.
    let mut other_env = physical_contract();
    other_env.terminal_policy = TargetTerminalPolicy::Choose;
    other_env.environment =
        BTreeMap::from([("PERL_TEST_HARNESS_ASAP".to_string(), "1".to_string())]);
    other_env.validate()?;
    Ok(())
}

/// The defect this gate exists for: a `tty` row with no terminal requirement is
/// admitted on a terminal-less host and reports as an ordinary pass.
#[test]
fn forced_tty_row_without_terminal_capability_is_rejected() -> Result<(), String> {
    let mut contract = forced_tty_contract();
    contract.capability_predicates.clear();
    expect_exact_error(
        contract.validate(),
        "forced-TTY target forced_tty must declare the controlling_terminal capability predicate",
    )?;

    // A different predicate is not a substitute for the terminal requirement.
    let mut other_predicate = forced_tty_contract();
    other_predicate.capability_predicates = vec!["miniperl".to_string()];
    expect_exact_error(
        other_predicate.validate(),
        "forced-TTY target forced_tty must declare the controlling_terminal capability predicate",
    )?;
    Ok(())
}

#[test]
fn forced_tty_row_cannot_also_select_the_no_tty_path() -> Result<(), String> {
    let mut contract = forced_tty_contract();
    contract.environment = BTreeMap::from([(RUNTESTS_NO_TTY_ENV.to_string(), "1".to_string())]);
    expect_exact_error(
        contract.validate(),
        "forced-TTY target forced_tty cannot declare PERL_SKIP_TTY_TEST",
    )?;
    Ok(())
}

#[test]
fn no_tty_row_without_a_mechanism_is_rejected() -> Result<(), String> {
    let mut contract = physical_contract();
    contract.terminal_policy = TargetTerminalPolicy::NoTty;
    expect_exact_error(
        contract.validate(),
        "no-TTY target component_base must declare PERL_SKIP_TTY_TEST",
    )?;
    Ok(())
}

#[test]
fn no_tty_row_cannot_require_a_controlling_terminal() -> Result<(), String> {
    let mut contract = physical_contract();
    contract.terminal_policy = TargetTerminalPolicy::NoTty;
    contract.environment = BTreeMap::from([(RUNTESTS_NO_TTY_ENV.to_string(), "1".to_string())]);
    contract.capability_predicates = vec![CONTROLLING_TERMINAL_CAPABILITY.to_string()];
    expect_exact_error(
        contract.validate(),
        "no-TTY target component_base cannot require a controlling terminal",
    )?;
    Ok(())
}

/// The issue's explicit non-goal, and the error the checked-in matrix actually
/// contained: `HARNESS_NOTTY` is a display variable, so a row that reaches for
/// it instead of `PERL_SKIP_TTY_TEST` has not selected the no-terminal path.
#[test]
fn the_harness_display_variable_cannot_stand_in_for_the_no_tty_mechanism() -> Result<(), String> {
    let mut contract = physical_contract();
    contract.terminal_policy = TargetTerminalPolicy::NoTty;
    contract.environment =
        BTreeMap::from([(HARNESS_DISPLAY_NO_TTY_ENV.to_string(), "1".to_string())]);
    expect_exact_error(
        contract.validate(),
        "no-TTY target component_base declares only HARNESS_NOTTY, which formats harness output and does not select the no-TTY path",
    )?;

    // Declaring both is fine — upstream can format output and skip the terminal
    // at once — because the real mechanism is present.
    let mut both = physical_contract();
    both.terminal_policy = TargetTerminalPolicy::NoTty;
    both.environment = BTreeMap::from([
        (HARNESS_DISPLAY_NO_TTY_ENV.to_string(), "1".to_string()),
        (RUNTESTS_NO_TTY_ENV.to_string(), "1".to_string()),
    ]);
    both.validate()?;
    Ok(())
}

/// Presence is not selection: Perl's only false strings are "" and "0", and
/// empty values are already rejected.
#[test]
fn a_false_valued_no_tty_mechanism_is_rejected() -> Result<(), String> {
    let mut disabled = physical_contract();
    disabled.terminal_policy = TargetTerminalPolicy::NoTty;
    disabled.environment = BTreeMap::from([(RUNTESTS_NO_TTY_ENV.to_string(), "0".to_string())]);
    expect_exact_error(
        disabled.validate(),
        "target component_base sets PERL_SKIP_TTY_TEST to a false value, which does not select the no-TTY path",
    )?;

    // "1" is the canonical enabling value and must still be accepted, so the
    // rule rejects the false value rather than the variable.
    let mut enabled = physical_contract();
    enabled.terminal_policy = TargetTerminalPolicy::NoTty;
    enabled.environment = BTreeMap::from([(RUNTESTS_NO_TTY_ENV.to_string(), "1".to_string())]);
    enabled.validate()?;
    Ok(())
}

/// A row that sets the mechanism has already resolved the terminal, so it may
/// not keep reporting as the auto-detecting or inheriting target.
#[test]
fn unresolved_policies_cannot_carry_terminal_facts() -> Result<(), String> {
    for policy in [
        TargetTerminalPolicy::Choose,
        TargetTerminalPolicy::Inherited,
        TargetTerminalPolicy::NotApplicable,
    ] {
        let mut mechanism = physical_contract();
        mechanism.terminal_policy = policy;
        mechanism.environment =
            BTreeMap::from([(RUNTESTS_NO_TTY_ENV.to_string(), "1".to_string())]);
        expect_exact_error(
            mechanism.validate(),
            "target component_base declares PERL_SKIP_TTY_TEST without the no_tty terminal policy",
        )?;

        let mut capability = physical_contract();
        capability.terminal_policy = policy;
        capability.capability_predicates = vec![CONTROLLING_TERMINAL_CAPABILITY.to_string()];
        expect_exact_error(
            capability.validate(),
            "target component_base requires a controlling terminal without the tty terminal policy",
        )?;
    }
    Ok(())
}

/// Every checked-in row that resolves a terminal must carry the matching
/// capability and mechanism, so the pinned matrix cannot drift back to a
/// terminal variant that any host appears able to run.
#[test]
fn checked_in_terminal_rows_declare_their_capability_and_mechanism() -> Result<()> {
    let matrix = read_matrix(&repo_file(".ci/perl-core-harness/upstream-targets-5.42.2.v1"))?;
    let mut forced_tty = Vec::new();
    let mut forced_no_tty = Vec::new();
    for target in &matrix.targets {
        let contract = &target.contract;
        // Assert the facts here as well as collecting the IDs. Loading the
        // matrix already runs `validate`, so a row that lost its capability or
        // mechanism could not reach this loop — but that makes the assertion
        // depend on validation having run. Restating it keeps this test
        // meaningful on its own terms.
        match contract.terminal_policy {
            TargetTerminalPolicy::Tty => {
                assert!(
                    contract
                        .capability_predicates
                        .iter()
                        .any(|predicate| predicate == CONTROLLING_TERMINAL_CAPABILITY),
                    "{} forces a terminal without requiring one",
                    contract.target_id
                );
                forced_tty.push(contract.target_id.as_str());
            }
            TargetTerminalPolicy::NoTty => {
                assert!(
                    contract.environment.contains_key(RUNTESTS_NO_TTY_ENV),
                    "{} forces the no-terminal path without declaring {RUNTESTS_NO_TTY_ENV}",
                    contract.target_id
                );
                forced_no_tty.push(contract.target_id.as_str());
            }
            _ => {}
        }
    }
    forced_tty.sort_unstable();
    forced_no_tty.sort_unstable();
    // State the denominator rather than leaving it implicit. Upstream forces a
    // terminal only for `test_tty` (`runtests tty`) and `minitest` (stdin from
    // `/dev/tty`), and forces the no-terminal path only for `test_notty`
    // (`runtests no-tty`) and `minitest_notty`. `test_harness_notty` is
    // deliberately absent: it runs `runtests choose` and only changes output
    // formatting. A new terminal row must be a deliberate edit here.
    assert_eq!(forced_tty, vec!["make_minitest_tty", "make_test_tty"]);
    assert_eq!(forced_no_tty, vec!["make_minitest_notty", "make_test_notty"]);
    Ok(())
}
