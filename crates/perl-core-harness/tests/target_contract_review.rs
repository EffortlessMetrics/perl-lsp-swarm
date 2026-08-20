//! Review falsifiers for target-selector decoding and legacy composite partitioning.

use model::{
    CompositeOverlapPolicy, TARGET_MATRIX_SCHEMA_VERSION, TARGET_SELECTION_SCHEMA_VERSION,
    TARGET_TOPOLOGY_DRIFT_SCHEMA_VERSION, TargetAuthority, TargetAuthorityKind, TargetDisposition,
    TargetKind, TargetMatrixEntry, TargetPerlRuntime, TargetPreparation, TargetScriptForm,
    TargetSelectionContract, TargetSelector, TargetTerminalPolicy, TargetTopologyDrift,
    TargetTopologyDriftStatus, UpstreamTargetMatrix,
};
use perl_core_harness::target_contracts::{io, model};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn repo_file(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(relative)
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

    let silently_extra_field = r#"{"kind":"recursive_root","path":"base","scope":"recursive"}"#;
    assert!(serde_json::from_str::<TargetSelector>(silently_extra_field).is_err());
}

#[test]
fn legacy_composites_are_partitioned_by_runner() -> TestResult {
    let matrix = io::read_matrix(&repo_file(".ci/perl-core-harness/upstream-targets-5.42.2.v1"))?;

    let find = |target_id: &str| {
        matrix
            .targets
            .iter()
            .find(|entry| entry.contract.target_id == target_id)
            .map(|entry| &entry.contract)
            .ok_or_else(|| format!("missing target contract {target_id}"))
    };

    let core_harness = find("legacy_custom_core_harness")?;
    let core_test = find("legacy_custom_core_test")?;
    let full_harness = find("legacy_custom_full_harness")?;
    let full_test = find("legacy_custom_full_test")?;
    let direct_op = find("component_op")?;
    let op_hook = find("component_op_hook")?;

    for composite in [core_harness, core_test, full_harness, full_test] {
        assert_eq!(composite.composite_overlap_policy, Some(CompositeOverlapPolicy::RejectOverlap));
    }
    assert!(core_harness.composite_members.iter().any(|member| member == "component_op"));
    assert!(!core_harness.composite_members.iter().any(|member| member == "component_op_hook"));
    assert!(core_test.composite_members.iter().any(|member| member == "component_op"));
    assert!(core_test.composite_members.iter().any(|member| member == "component_op_hook"));
    assert_eq!(
        full_harness.composite_members,
        vec!["component_uni".to_string(), "legacy_custom_core_harness".to_string(),]
    );
    assert_eq!(
        full_test.composite_members,
        vec!["component_uni".to_string(), "legacy_custom_core_test".to_string(),]
    );
    // A composite carries no parameters of its own: its denominator is exactly
    // its members and overlap policy. The runner a composite belongs to is
    // therefore read from its declared authority, not from a second copy in
    // `variant_parameters`, which `validate_composite` rejects outright.
    for composite in [core_harness, core_test, full_harness, full_test] {
        assert!(
            composite.variant_parameters.is_empty(),
            "composite {} must not restate authority as variant parameters",
            composite.target_id
        );
    }
    assert_eq!(core_harness.authority.entrypoint, "HarnessProfile::Core / HarnessRunner::Harness");
    assert_eq!(core_test.authority.entrypoint, "HarnessProfile::Core / HarnessRunner::Test");
    assert_eq!(full_harness.authority.entrypoint, "HarnessProfile::Full / HarnessRunner::Harness");
    assert_eq!(full_test.authority.entrypoint, "HarnessProfile::Full / HarnessRunner::Test");
    assert_eq!(
        direct_op.selectors,
        vec![TargetSelector::NonRecursiveGlob { pattern: "op/*.t".to_string() }]
    );
    assert_eq!(
        op_hook.selectors,
        vec![TargetSelector::RecursiveRoot { path: "op/hook".to_string() }]
    );
    Ok(())
}

#[test]
fn target_names_are_globally_unambiguous() -> TestResult {
    let mut matrix = matrix_with_contract(
        "fixture",
        "1111111111111111111111111111111111111111",
        "2222222222222222222222222222222222222222",
        physical_contract("first target"),
    );
    let mut second = physical_contract("second target");
    second.target_id = "component_comp".to_string();
    second.upstream_name = "t/comp".to_string();
    second.aliases = vec!["t/base".to_string()];
    second.perl_version_row = "fixture".to_string();
    matrix.targets.push(TargetMatrixEntry {
        contract: second,
        disposition: TargetDisposition::Implemented,
        owner_issue: Some(6660),
        claim_boundary: "second fixture topology only".to_string(),
    });

    let Err(error) = matrix.validate() else {
        return Err("ambiguous target names were accepted".into());
    };
    assert!(error.contains("is ambiguous between"), "unexpected rejection: {error}");
    Ok(())
}

/// Build a two-row matrix whose rows share `t/TEST --utf16` as their upstream
/// name, letting each caller decide how (or whether) the rows discriminate.
fn shared_upstream_name_matrix(
    decorate_second: impl FnOnce(&mut TargetSelectionContract),
) -> UpstreamTargetMatrix {
    let mut first = physical_contract("first variant");
    first.target_id = "variant_utf16_be_bom".to_string();
    first.upstream_name = "t/TEST --utf16".to_string();
    first.target_kind = TargetKind::EnvironmentVariant;
    first.selection_authority = None;
    first.selectors.clear();
    first.script_forms.clear();
    first.preparation.make_target = None;
    first.preparation.perl_runtime = TargetPerlRuntime::Inherited;
    first.variant_of = Some("component_base".to_string());
    first.runner_switches = vec!["--utf16".to_string()];
    first.variant_parameters = BTreeMap::from([("bom".to_string(), "present".to_string())]);

    let mut second = first.clone();
    second.target_id = "variant_utf16_be_no_bom".to_string();
    second.display_name = "second variant".to_string();
    decorate_second(&mut second);

    let base = physical_contract("denominator");
    let mut matrix = matrix_with_contract(
        "fixture",
        "1111111111111111111111111111111111111111",
        "2222222222222222222222222222222222222222",
        base,
    );
    // A second real denominator, so a cross-parent fixture points at a row that
    // actually exists and is rejected by the namespace rule rather than by the
    // missing-parent check.
    let mut other_base = physical_contract("other denominator");
    other_base.target_id = "component_comp".to_string();
    other_base.upstream_name = "t/comp".to_string();
    other_base.perl_version_row = "fixture".to_string();
    other_base.selectors = vec![TargetSelector::RecursiveRoot { path: "comp".to_string() }];
    matrix.targets.push(TargetMatrixEntry {
        contract: other_base,
        disposition: TargetDisposition::Implemented,
        owner_issue: Some(6660),
        claim_boundary: "variant fixture".to_string(),
    });
    for contract in [first, second] {
        matrix.targets.push(TargetMatrixEntry {
            contract,
            disposition: TargetDisposition::Implemented,
            owner_issue: Some(6660),
            claim_boundary: "variant fixture".to_string(),
        });
    }
    matrix
}

#[test]
fn sibling_variants_may_share_one_upstream_invocation() -> TestResult {
    // Upstream exposes a single `t/TEST --utf16` invocation for both BOM
    // states; the rows are discriminated by their parameters, not their name.
    let matrix = shared_upstream_name_matrix(|second| {
        second.variant_parameters = BTreeMap::from([("bom".to_string(), "absent".to_string())]);
    });
    matrix.validate()?;
    Ok(())
}

#[test]
fn shared_upstream_name_requires_distinct_variant_parameters() -> TestResult {
    // Same name, same parameters: nothing tells the two rows apart.
    let matrix = shared_upstream_name_matrix(|_| {});
    let Err(error) = matrix.validate() else {
        return Err("identical parameters must be rejected".into());
    };
    if !error.contains("identical parameters") {
        return Err(format!("unexpected rejection reason: {error}").into());
    }
    Ok(())
}

#[test]
fn shared_upstream_name_requires_a_common_parent() -> TestResult {
    // Same name, different parents: the rows do not parameterize one invocation.
    let matrix = shared_upstream_name_matrix(|second| {
        second.variant_parameters = BTreeMap::from([("bom".to_string(), "absent".to_string())]);
        second.variant_of = Some("component_comp".to_string());
    });
    let Err(error) = matrix.validate() else {
        return Err("cross-parent sharing must be rejected".into());
    };
    if !error.contains("variants of different parents") {
        return Err(format!("unexpected rejection reason: {error}").into());
    }
    Ok(())
}

#[test]
fn non_variant_rows_may_not_share_an_upstream_name() -> TestResult {
    // A plain row has no parameters to disambiguate it, so the equivalence
    // must not extend to it.
    let matrix = shared_upstream_name_matrix(|second| {
        // A physical row may not carry variant parameters at all, so it has
        // nothing that could discriminate it from the row it shares a name with.
        second.variant_parameters = BTreeMap::new();
        second.variant_of = None;
        second.target_kind = TargetKind::PhysicalSeries;
        second.selection_authority = Some(TargetAuthority {
            kind: TargetAuthorityKind::Test,
            entrypoint: "t/TEST".to_string(),
        });
        second.selectors = vec![TargetSelector::RecursiveRoot { path: "comp".to_string() }];
        second.script_forms = vec![TargetScriptForm::DotT];
    });
    let Err(error) = matrix.validate() else {
        return Err("non-variant sharing must be rejected".into());
    };
    if !error.contains("is not a variant") {
        return Err(format!("unexpected rejection reason: {error}").into());
    }
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
    let drift = compared_drift(&pinned, &observed, vec!["component_base".to_string()])?;
    let pinned_fingerprint = pinned.fingerprint()?;

    drift.validate_against(&pinned, &pinned_fingerprint, Some(&observed))?;
    Ok(())
}
