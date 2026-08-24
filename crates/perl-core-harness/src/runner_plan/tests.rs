//! Runner-plan and parity falsifiers over the pinned target matrix.

use crate::build::{
    build_runner_plan, runner_plan_digest, uniform_discovery_frame, validate_runner_plan,
    validate_runner_plan_against,
};
use crate::compare::{
    compare_runner_plans, compare_runner_plans_against, validate_runner_parity,
    validate_runner_parity_against,
};
use crate::io::read_matrix;
use crate::runner_model::{
    DiscoveryFrame, MembershipParityStatus, RUNNER_PLAN_V1_SCHEMA_VERSION, RunnerKind, RunnerPlan,
    RunnerPlanV1, RunnerScheduling, SourceForm,
};
use color_eyre::eyre::{Result, bail};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn repo_file(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(relative)
}

fn matrix() -> Result<crate::model::UpstreamTargetMatrix> {
    read_matrix(&repo_file(".ci/perl-core-harness/upstream-targets-5.42.2.v1"))
}

fn base_plan(
    matrix: &crate::model::UpstreamTargetMatrix,
    runner: RunnerKind,
    raw: &[u8],
) -> Result<RunnerPlan> {
    // These fixtures spell members as canonical repository paths (`t/...`).
    // Declaring that frame explicitly is part of the reviewed contract.
    build_runner_plan(
        matrix,
        "component_base",
        runner,
        DiscoveryFrame::CanonicalRepositoryPath,
        raw,
        RunnerScheduling::default(),
    )
    .map_err(|error| color_eyre::eyre::eyre!(error))
}

#[test]
fn test_and_harness_membership_can_match_with_different_order() -> Result<()> {
    let matrix = matrix()?;
    let test_raw = b"t/base/cond.t\nt/base/if.t\n";
    let harness_raw = b"t/base/if.t\nt/base/cond.t\n";
    let test_plan = base_plan(&matrix, RunnerKind::Test, test_raw)?;
    let harness_plan = build_runner_plan(
        &matrix,
        "component_base",
        RunnerKind::Harness,
        DiscoveryFrame::CanonicalRepositoryPath,
        harness_raw,
        RunnerScheduling {
            jobs: Some(2),
            asap: false,
            state_ordering: true,
            properties: BTreeMap::new(),
        },
    )
    .map_err(|error| color_eyre::eyre::eyre!(error))?;
    let parity =
        compare_runner_plans_against(&matrix, &test_plan, test_raw, &harness_plan, harness_raw)
            .map_err(|error| color_eyre::eyre::eyre!(error))?;
    assert_eq!(parity.membership_status, MembershipParityStatus::Parity);
    assert!(!parity.order_equal);
    assert!(!parity.scheduling_equal);
    assert_ne!(parity.left_plan_digest, parity.right_plan_digest);
    assert_eq!(parity.left_raw_discovery_digest, test_plan.raw_discovery_digest);
    assert_eq!(parity.right_raw_discovery_digest, harness_plan.raw_discovery_digest);
    assert!(
        harness_plan
            .limitations
            .iter()
            .any(|value| value == "alternate_runner_requires_membership_parity_evidence")
    );
    assert!(
        test_plan
            .limitations
            .iter()
            .any(|value| { value == "scheduling_inputs_are_declared_not_observed" })
    );
    assert!(parity.limitations.iter().any(|value| {
        value == "scheduling_equality_compares_declared_inputs_not_observed_runner_state"
    }));
    Ok(())
}

#[test]
fn nested_op_hook_is_not_absorbed_by_direct_op() -> Result<()> {
    let matrix = matrix()?;
    let nested = build_runner_plan(
        &matrix,
        "component_op_hook",
        RunnerKind::Harness,
        DiscoveryFrame::CanonicalRepositoryPath,
        b"t/op/hook/hook.t\n",
        RunnerScheduling::default(),
    );
    assert!(nested.is_ok());
    let direct = build_runner_plan(
        &matrix,
        "component_op",
        RunnerKind::Harness,
        DiscoveryFrame::CanonicalRepositoryPath,
        b"t/op/hook/hook.t\n",
        RunnerScheduling::default(),
    );
    assert!(direct.is_err());
    Ok(())
}

#[test]
fn reonly_keeps_local_and_root_external_members() -> Result<()> {
    let matrix = matrix()?;
    let plan = build_runner_plan(
        &matrix,
        "make_test_reonly",
        RunnerKind::Harness,
        DiscoveryFrame::CanonicalRepositoryPath,
        b"t/re/basic.t\next/re/t/qr.t\n",
        RunnerScheduling {
            jobs: None,
            asap: true,
            state_ordering: false,
            properties: BTreeMap::new(),
        },
    )
    .map_err(|error| color_eyre::eyre::eyre!(error))?;
    assert_eq!(
        plan.normalized_membership,
        vec!["ext/re/t/qr.t".to_string(), "t/re/basic.t".to_string()]
    );
    Ok(())
}

#[test]
fn manifest_population_accepts_dot_t_and_test_pl_with_typed_provenance() -> Result<()> {
    let matrix = matrix()?;
    let plan = build_runner_plan(
        &matrix,
        "manifest_cpan",
        RunnerKind::Test,
        DiscoveryFrame::RepositoryRootRelative,
        b"cpan/Foo/t/basic.t\ncpan/Foo/test.pl\n",
        RunnerScheduling::default(),
    )
    .map_err(|error| color_eyre::eyre::eyre!(error))?;
    let forms = plan.source_items.iter().map(|item| item.source_form).collect::<Vec<_>>();
    assert_eq!(forms, vec![SourceForm::DotT, SourceForm::TestPl]);
    for item in &plan.source_items {
        assert_eq!(item.discovery_frame, DiscoveryFrame::RepositoryRootRelative);
        assert_eq!(item.normalization_version, "perl_core_harness.runner_source_normalization.v2");
        assert_eq!(plan.schema_version, "perl_core_harness.runner_plan.v2");
    }
    Ok(())
}

#[test]
fn serialized_source_form_is_recomputed_from_the_raw_path() -> Result<()> {
    let matrix = matrix()?;
    let mut plan = build_runner_plan(
        &matrix,
        "manifest_cpan",
        RunnerKind::Test,
        DiscoveryFrame::RepositoryRootRelative,
        b"cpan/Foo/test.pl\n",
        RunnerScheduling::default(),
    )
    .map_err(|error| color_eyre::eyre::eyre!(error))?;
    plan.source_items[0].source_form = SourceForm::DotT;
    assert!(validate_runner_plan(&plan).is_err());
    Ok(())
}

#[test]
fn real_membership_difference_is_not_hidden_by_order_normalization() -> Result<()> {
    let matrix = matrix()?;
    let left = base_plan(&matrix, RunnerKind::Test, b"t/base/cond.t\nt/base/if.t\n")?;
    let right = base_plan(&matrix, RunnerKind::Harness, b"t/base/if.t\n")?;
    let parity =
        compare_runner_plans(&left, &right).map_err(|error| color_eyre::eyre::eyre!(error))?;
    assert_eq!(parity.membership_status, MembershipParityStatus::Mismatch);
    assert_eq!(parity.missing_from_right, vec!["t/base/cond.t".to_string()]);
    Ok(())
}

#[test]
fn cross_frame_t_relative_aliases_cannot_collapse_into_false_parity() -> Result<()> {
    // Strongest reviewed mutation: under the pre-frame v1 normalizer both raw
    // spellings collapsed to `lib/diagnostics.t` and parity was falsely
    // proven. With explicit frames the canonical identities differ.
    let matrix = matrix()?;
    let left = build_runner_plan(
        &matrix,
        "make_test_choose",
        RunnerKind::Test,
        DiscoveryFrame::RunnerTDirectoryRelative,
        b"lib/diagnostics.t\n",
        RunnerScheduling::default(),
    )
    .map_err(|error| color_eyre::eyre::eyre!(error))?;
    let right = build_runner_plan(
        &matrix,
        "make_test_choose",
        RunnerKind::Harness,
        DiscoveryFrame::RunnerTDirectoryRelative,
        b"../lib/diagnostics.t\n",
        RunnerScheduling::default(),
    )
    .map_err(|error| color_eyre::eyre::eyre!(error))?;

    assert_eq!(left.normalized_membership, vec!["t/lib/diagnostics.t".to_string()]);
    assert_eq!(right.normalized_membership, vec!["lib/diagnostics.t".to_string()]);
    assert_eq!(left.source_items[0].path_class, crate::runner_model::SourcePathClass::LocalT);
    assert_eq!(right.source_items[0].path_class, crate::runner_model::SourcePathClass::RootLib);

    let parity = compare_runner_plans_against(
        &matrix,
        &left,
        b"lib/diagnostics.t\n",
        &right,
        b"../lib/diagnostics.t\n",
    )
    .map_err(|error| color_eyre::eyre::eyre!(error))?;
    assert_eq!(parity.membership_status, MembershipParityStatus::Mismatch);
    assert_eq!(parity.missing_from_right, vec!["t/lib/diagnostics.t".to_string()]);
    assert_eq!(parity.extra_in_right, vec!["lib/diagnostics.t".to_string()]);
    Ok(())
}

#[test]
fn same_raw_spelling_under_different_frames_is_never_one_source() -> Result<()> {
    // Repository-root `lib/**` cannot satisfy parity through a `t/lib/**`
    // spelling or vice versa; each frame names a different repository member.
    let matrix = matrix()?;
    let t_relative = build_runner_plan(
        &matrix,
        "component_t_lib",
        RunnerKind::Test,
        DiscoveryFrame::RunnerTDirectoryRelative,
        b"lib/attributes.t\n",
        RunnerScheduling::default(),
    )
    .map_err(|error| color_eyre::eyre::eyre!(error))?;
    assert_eq!(t_relative.normalized_membership, vec!["t/lib/attributes.t".to_string()]);

    // The same raw spelling against the same target under the repo-root frame
    // resolves outside the recursive `lib` root of component_t_lib...
    let outside = match build_runner_plan(
        &matrix,
        "component_t_lib",
        RunnerKind::Test,
        DiscoveryFrame::RepositoryRootRelative,
        b"lib/attributes.t\n",
        RunnerScheduling::default(),
    ) {
        Ok(plan) => {
            bail!("repo-root frame must fall outside component_t_lib: {plan:?}")
        }
        Err(error) => error,
    };
    assert_eq!(outside, "discovered path lib/attributes.t is outside target component_t_lib");

    // ...while manifest_root_lib admits exactly the root spelling.
    let root_member = build_runner_plan(
        &matrix,
        "manifest_root_lib",
        RunnerKind::Harness,
        DiscoveryFrame::RepositoryRootRelative,
        b"lib/attributes.t\n",
        RunnerScheduling::default(),
    )
    .map_err(|error| color_eyre::eyre::eyre!(error))?;
    assert_eq!(root_member.normalized_membership, vec!["lib/attributes.t".to_string()]);
    Ok(())
}

#[test]
fn frame_identity_changes_the_plan_digest_even_when_canonical_paths_match() -> Result<()> {
    // Reviewed falsifier 3: the frame changes but the plan/source digest must
    // not remain unchanged. Canonical paths agree here; only provenance moves.
    let matrix = matrix()?;
    let raw = b"lib/attributes.t\n";
    let repo_root = build_runner_plan(
        &matrix,
        "manifest_root_lib",
        RunnerKind::Test,
        DiscoveryFrame::RepositoryRootRelative,
        raw,
        RunnerScheduling::default(),
    )
    .map_err(|error| color_eyre::eyre::eyre!(error))?;
    let canonical = build_runner_plan(
        &matrix,
        "manifest_root_lib",
        RunnerKind::Test,
        DiscoveryFrame::CanonicalRepositoryPath,
        raw,
        RunnerScheduling::default(),
    )
    .map_err(|error| color_eyre::eyre::eyre!(error))?;

    assert_eq!(
        repo_root.normalized_membership, canonical.normalized_membership,
        "precondition: identical canonical members"
    );
    assert_ne!(
        runner_plan_digest(&repo_root).map_err(|error| color_eyre::eyre::eyre!(error))?,
        runner_plan_digest(&canonical).map_err(|error| color_eyre::eyre::eyre!(error))?,
        "frame identity must be load-bearing in canonical plan bytes"
    );
    Ok(())
}

#[test]
fn mixed_frame_receipts_cannot_be_rebuilt_from_one_stream() -> Result<()> {
    let matrix = matrix()?;
    let raw = b"lib/a.t\nlib/b.t\n";
    let mut plan = build_runner_plan(
        &matrix,
        "manifest_root_lib",
        RunnerKind::Test,
        DiscoveryFrame::RepositoryRootRelative,
        raw,
        RunnerScheduling::default(),
    )
    .map_err(|error| color_eyre::eyre::eyre!(error))?;
    validate_runner_plan_against(&matrix, raw, &plan)
        .map_err(|error| color_eyre::eyre::eyre!(error))?;

    // Re-point one row at an equivalent frame spelling: per-row provenance
    // stays internally coherent, so standalone validation still holds...
    plan.source_items[1].discovery_frame = DiscoveryFrame::CanonicalRepositoryPath;
    assert!(validate_runner_plan(&plan).is_ok());
    assert!(uniform_discovery_frame(&plan).is_err());
    // ...but a single declared stream can no longer reproduce the receipt.
    assert!(validate_runner_plan_against(&matrix, raw, &plan).is_err());
    Ok(())
}

#[test]
fn plan_check_rebuilds_from_matrix_and_raw_discovery() -> Result<()> {
    let matrix = matrix()?;
    let raw = b"t/base/if.t\n";
    let plan = base_plan(&matrix, RunnerKind::Test, raw)?;
    validate_runner_plan_against(&matrix, raw, &plan)
        .map_err(|error| color_eyre::eyre::eyre!(error))?;

    let mut forged_digest = plan.clone();
    forged_digest.matrix_fingerprint = "0".repeat(64);
    assert!(validate_runner_plan(&forged_digest).is_ok());
    assert!(validate_runner_plan_against(&matrix, raw, &forged_digest).is_err());

    assert!(validate_runner_plan_against(&matrix, b"t/base/cond.t\n", &plan).is_err());
    Ok(())
}

#[test]
fn scheduling_limitations_are_mandatory() -> Result<()> {
    let matrix = matrix()?;
    let raw = b"t/base/if.t\n";
    let left = base_plan(&matrix, RunnerKind::Test, raw)?;
    let right = base_plan(&matrix, RunnerKind::Harness, raw)?;

    let mut plan_without_boundary = left.clone();
    plan_without_boundary
        .limitations
        .retain(|value| value != "scheduling_inputs_are_declared_not_observed");
    assert!(validate_runner_plan(&plan_without_boundary).is_err());

    let mut parity_without_boundary =
        compare_runner_plans(&left, &right).map_err(|error| color_eyre::eyre::eyre!(error))?;
    parity_without_boundary.limitations.retain(|value| {
        value != "scheduling_equality_compares_declared_inputs_not_observed_runner_state"
    });
    assert!(validate_runner_parity(&parity_without_boundary).is_err());
    Ok(())
}

#[test]
fn discovery_frame_declaration_limitation_is_mandatory() -> Result<()> {
    let matrix = matrix()?;
    let mut stripped = base_plan(&matrix, RunnerKind::Test, b"t/base/if.t\n")?;
    assert!(
        stripped.limitations.iter().any(|value| value
            == "discovery_frames_are_declared_inputs_not_observed_upstream_runner_state")
    );
    stripped.limitations.retain(|value| {
        value != "discovery_frames_are_declared_inputs_not_observed_upstream_runner_state"
    });
    assert!(validate_runner_plan(&stripped).is_err());
    Ok(())
}

#[test]
fn copied_discovery_stream_cannot_claim_runner_observation() -> Result<()> {
    let matrix = matrix()?;
    // One hand-written byte stream is built once as `test` and once as the
    // copied discovery of a `harness` plan. Membership parity between the two
    // declared streams remains provable, but no receipt may claim that either
    // upstream runner produced or observed these bytes.
    let copied_raw = b"t/base/cond.t\nt/base/if.t\n";
    let test_plan = base_plan(&matrix, RunnerKind::Test, copied_raw)?;
    let harness_plan = build_runner_plan(
        &matrix,
        "component_base",
        RunnerKind::Harness,
        DiscoveryFrame::CanonicalRepositoryPath,
        copied_raw,
        RunnerScheduling::default(),
    )
    .map_err(|error| color_eyre::eyre::eyre!(error))?;
    assert_eq!(test_plan.raw_discovery_digest, harness_plan.raw_discovery_digest);
    for plan in [&test_plan, &harness_plan] {
        assert!(
            plan.limitations.iter().any(|value| value
                == "raw_discovery_stream_is_declared_input_not_observed_runner_output")
        );
        assert!(plan.limitations.iter().any(|value| value
            == "discovery_frames_are_declared_inputs_not_observed_upstream_runner_state"));
    }

    let parity = compare_runner_plans(&test_plan, &harness_plan)
        .map_err(|error| color_eyre::eyre::eyre!(error))?;
    assert_eq!(parity.membership_status, MembershipParityStatus::Parity);
    assert!(parity.limitations.iter().any(|value| {
        value == "membership_parity_compares_declared_discovery_streams_not_observed_runner_output"
    }));

    let mut forged_plan = test_plan;
    forged_plan.limitations.retain(|value| {
        value != "raw_discovery_stream_is_declared_input_not_observed_runner_output"
    });
    assert!(validate_runner_plan(&forged_plan).is_err());

    let mut forged_report = parity;
    forged_report.limitations.retain(|value| {
        value != "membership_parity_compares_declared_discovery_streams_not_observed_runner_output"
    });
    assert!(validate_runner_parity(&forged_report).is_err());
    Ok(())
}

#[test]
fn plan_digest_binds_canonical_typed_content_not_json_spelling() -> Result<()> {
    let matrix = matrix()?;
    let plan = base_plan(&matrix, RunnerKind::Test, b"t/base/if.t\n")?;
    let compact = serde_json::to_vec(&plan)?;
    let pretty = serde_json::to_vec_pretty(&plan)?;
    assert_ne!(compact, pretty);

    let compact_plan: RunnerPlan = serde_json::from_slice(&compact)?;
    let pretty_plan: RunnerPlan = serde_json::from_slice(&pretty)?;
    assert_eq!(
        runner_plan_digest(&compact_plan).map_err(|error| color_eyre::eyre::eyre!(error))?,
        runner_plan_digest(&pretty_plan).map_err(|error| color_eyre::eyre::eyre!(error))?
    );
    Ok(())
}

#[test]
fn v1_receipts_stay_readable_but_cannot_back_current_authority() -> Result<()> {
    let matrix = matrix()?;
    let current = base_plan(&matrix, RunnerKind::Test, b"t/base/if.t\n")?;
    assert_eq!(current.schema_version, "perl_core_harness.runner_plan.v2");

    // A merged #6772-era v1 receipt: no discovery frame, v1 field names.
    let v1_json = br#"{
        "schema_version": "perl_core_harness.runner_plan.v1",
        "matrix_fingerprint": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "target_id": "component_base",
        "target_contract_digest": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        "runner": "test",
        "runner_entrypoint": "t/TEST",
        "canonical_selection_entrypoint": "t/TEST",
        "raw_discovery_digest": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
        "source_items": [
            {
                "raw_path": "../lib/Foo/test.pl",
                "canonical_path": "lib/Foo/test.pl",
                "source_form": "test_pl",
                "path_class": "root_lib",
                "invocation_context": "root_lib_u1"
            }
        ],
        "normalized_order": ["lib/Foo/test.pl"],
        "normalized_membership": ["lib/Foo/test.pl"],
        "scheduling": {"jobs": null, "asap": false, "state_ordering": false, "properties": {}},
        "invocation_capture": "not_proven",
        "limitations": [
            "per_file_upstream_scan_and_effective_invocation_not_captured",
            "scheduling_inputs_are_declared_not_observed",
            "raw_discovery_stream_is_declared_input_not_observed_runner_output"
        ],
        "claim_boundary": "historical declared membership only"
    }"#;

    let historical =
        RunnerPlanV1::decode(v1_json).map_err(|error| color_eyre::eyre::eyre!(error))?;
    assert_eq!(historical.schema_version, RUNNER_PLAN_V1_SCHEMA_VERSION);
    assert_eq!(historical.source_items[0].canonical_path, "lib/Foo/test.pl");
    // Round-trip preserves the evidence verbatim.
    let roundtripped: RunnerPlanV1 = serde_json::from_slice(&serde_json::to_vec(&historical)?)?;
    assert_eq!(roundtripped, historical);

    // The v1 payload is not decodable as current authority: rows lack frames.
    let as_current: Result<RunnerPlan, _> = serde_json::from_slice(v1_json);
    let error = as_current.err().map(|error| error.to_string()).unwrap_or_default();
    assert!(
        error.contains("discovery_frame"),
        "v1 payload must fail closed on the missing typed frame: {error}"
    );
    assert_ne!(RUNNER_PLAN_V1_SCHEMA_VERSION, current.schema_version);
    Ok(())
}

#[test]
fn normalization_output_is_independent_of_stream_iteration_order() -> Result<()> {
    let matrix = matrix()?;
    let forward = b"t/base/cond.t\nt/base/if.t\nt/base/pat.t\n";
    let backward = b"t/base/pat.t\nt/base/if.t\nt/base/cond.t\n";
    let forward_plan = base_plan(&matrix, RunnerKind::Test, forward)?;
    let backward_plan = base_plan(&matrix, RunnerKind::Test, backward)?;

    assert_eq!(forward_plan.normalized_membership, backward_plan.normalized_membership);
    // The whole-plan digest intentionally binds the original stream order and
    // per-row raw spellings; only the membership projection is order-free.
    assert_ne!(forward_plan.raw_discovery_digest, backward_plan.raw_discovery_digest);
    assert_eq!(
        forward_plan.normalized_order,
        vec!["t/base/cond.t".to_string(), "t/base/if.t".to_string(), "t/base/pat.t".to_string()]
    );
    assert_eq!(
        backward_plan.normalized_order,
        vec!["t/base/pat.t".to_string(), "t/base/if.t".to_string(), "t/base/cond.t".to_string()]
    );
    Ok(())
}

#[test]
fn parity_check_is_bound_to_canonical_plan_content() -> Result<()> {
    let matrix = matrix()?;
    let left = base_plan(&matrix, RunnerKind::Test, b"t/base/if.t\n")?;
    let right = base_plan(&matrix, RunnerKind::Harness, b"t/base/if.t\n")?;
    let report =
        compare_runner_plans(&left, &right).map_err(|error| color_eyre::eyre::eyre!(error))?;
    validate_runner_parity_against(&report, &left, &right)
        .map_err(|error| color_eyre::eyre::eyre!(error))?;

    let mut changed_right = right;
    changed_right.scheduling.jobs = Some(2);
    assert!(validate_runner_plan(&changed_right).is_ok());
    assert!(validate_runner_parity_against(&report, &left, &changed_right).is_err());
    Ok(())
}

#[test]
fn duplicate_raw_discovery_is_structurally_invalid() -> Result<()> {
    let matrix = matrix()?;
    let result = build_runner_plan(
        &matrix,
        "component_base",
        RunnerKind::Test,
        DiscoveryFrame::CanonicalRepositoryPath,
        b"t/base/if.t\nt/base/if.t\n",
        RunnerScheduling::default(),
    );
    assert!(result.is_err());
    Ok(())
}

#[test]
fn equivalent_aliases_produce_an_explicit_duplicate_conflict() -> Result<()> {
    let matrix = matrix()?;
    let result = build_runner_plan(
        &matrix,
        "manifest_root_lib",
        RunnerKind::Test,
        DiscoveryFrame::RepositoryRootRelative,
        b"lib/x/basic.t\n./lib/x/basic.t\n",
        RunnerScheduling::default(),
    );
    let duplicate = match result {
        Ok(plan) => bail!("alias collision must be rejected: {plan:?}"),
        Err(error) => error,
    };
    assert_eq!(duplicate, "raw discovery contains duplicate path lib/x/basic.t");
    Ok(())
}

#[test]
fn absent_target_is_named_by_plan_builder() -> Result<()> {
    let matrix = matrix()?;
    let Err(error) = build_runner_plan(
        &matrix,
        "no_such_target",
        RunnerKind::Test,
        DiscoveryFrame::CanonicalRepositoryPath,
        b"t/base/if.t\n",
        RunnerScheduling::default(),
    ) else {
        bail!("absent target must be rejected");
    };
    assert_eq!(error, "target matrix has no target no_such_target");
    Ok(())
}

#[test]
fn non_physical_targets_cannot_build_runner_plans() -> Result<()> {
    let matrix = matrix()?;
    for target_id in ["prep_test", "instrument_valgrind", "legacy_custom_core_test"] {
        let Err(error) = build_runner_plan(
            &matrix,
            target_id,
            RunnerKind::Test,
            DiscoveryFrame::CanonicalRepositoryPath,
            b"t/base/if.t\n",
            RunnerScheduling::default(),
        ) else {
            bail!("non-physical target {target_id} must be rejected");
        };
        assert_eq!(error, format!("target {target_id} is not a physical runner population"));
    }
    Ok(())
}

#[test]
fn environment_variants_inherit_base_selection_and_authority_chain() -> Result<()> {
    let matrix = matrix()?;
    let raw = b"t/base/if.t\n";

    let harness_variant = build_runner_plan(
        &matrix,
        "make_test_harness_choose",
        RunnerKind::Harness,
        DiscoveryFrame::CanonicalRepositoryPath,
        raw,
        RunnerScheduling::default(),
    )
    .map_err(|error| color_eyre::eyre::eyre!(error))?;
    assert_eq!(harness_variant.canonical_selection_entrypoint, "t/harness");
    assert!(
        !harness_variant
            .limitations
            .iter()
            .any(|value| value == "alternate_runner_requires_membership_parity_evidence")
    );

    let notty_as_test = build_runner_plan(
        &matrix,
        "make_test_harness_notty",
        RunnerKind::Test,
        DiscoveryFrame::CanonicalRepositoryPath,
        raw,
        RunnerScheduling::default(),
    )
    .map_err(|error| color_eyre::eyre::eyre!(error))?;
    assert_eq!(notty_as_test.canonical_selection_entrypoint, "t/harness");
    assert!(
        notty_as_test
            .limitations
            .iter()
            .any(|value| value == "alternate_runner_requires_membership_parity_evidence")
    );

    let notty_as_harness = build_runner_plan(
        &matrix,
        "make_test_harness_notty",
        RunnerKind::Harness,
        DiscoveryFrame::CanonicalRepositoryPath,
        raw,
        RunnerScheduling::default(),
    )
    .map_err(|error| color_eyre::eyre::eyre!(error))?;
    assert!(
        !notty_as_harness
            .limitations
            .iter()
            .any(|value| value == "alternate_runner_requires_membership_parity_evidence")
    );
    Ok(())
}

#[test]
fn script_form_allowance_rejects_test_pl_and_variant_inherits_base_forms() -> Result<()> {
    let matrix = matrix()?;

    let Err(dot_t_only) = build_runner_plan(
        &matrix,
        "component_base",
        RunnerKind::Test,
        DiscoveryFrame::CanonicalRepositoryPath,
        b"cpan/Foo/test.pl\n",
        RunnerScheduling::default(),
    ) else {
        bail!("dot_t-only targets must reject test.pl discovery");
    };
    assert_eq!(
        dot_t_only,
        "target component_base does not allow source form TestPl for cpan/Foo/test.pl"
    );

    let utf8_inherits_forms = build_runner_plan(
        &matrix,
        "variant_utf8",
        RunnerKind::Test,
        DiscoveryFrame::CanonicalRepositoryPath,
        b"cpan/Foo/test.pl\n",
        RunnerScheduling::default(),
    )
    .map_err(|error| color_eyre::eyre::eyre!(error))?;
    assert_eq!(utf8_inherits_forms.source_items[0].source_form, SourceForm::TestPl);
    Ok(())
}

#[test]
fn parity_limitation_presence_is_enforced_exactly() -> Result<()> {
    use crate::compare::validate_runner_parity;

    const MEMBERSHIP_DIFFERS: &str = "normalized_membership_differs";
    const SAME_RUNNER: &str = "same_runner_comparison_cannot_establish_cross_runner_parity";

    let matrix = matrix()?;
    let left = base_plan(&matrix, RunnerKind::Test, b"t/base/cond.t\nt/base/if.t\n")?;
    let right = base_plan(&matrix, RunnerKind::Harness, b"t/base/if.t\n")?;
    let differing =
        compare_runner_plans(&left, &right).map_err(|error| color_eyre::eyre::eyre!(error))?;
    assert!(differing.limitations.iter().any(|value| value == MEMBERSHIP_DIFFERS));
    validate_runner_parity(&differing).map_err(|error| color_eyre::eyre::eyre!(error))?;

    let mut stripped = differing.clone();
    stripped.limitations.retain(|value| value != MEMBERSHIP_DIFFERS);
    let Err(missing_error) = validate_runner_parity(&stripped) else {
        bail!("missing required limitation must be rejected");
    };
    assert_eq!(
        missing_error,
        format!("runner parity is missing required limitation {MEMBERSHIP_DIFFERS}")
    );

    let same = compare_runner_plans(
        &left,
        &base_plan(&matrix, RunnerKind::Test, b"t/base/cond.t\nt/base/if.t\n")?,
    )
    .map_err(|error| color_eyre::eyre::eyre!(error))?;
    assert!(same.limitations.iter().any(|value| value == SAME_RUNNER));
    assert!(!same.limitations.iter().any(|value| value == MEMBERSHIP_DIFFERS));
    validate_runner_parity(&same).map_err(|error| color_eyre::eyre::eyre!(error))?;

    let mut injected = same.clone();
    injected.limitations.push(MEMBERSHIP_DIFFERS.to_string());
    injected.limitations.sort();
    let Err(injected_error) = validate_runner_parity(&injected) else {
        bail!("inapplicable limitation must be rejected");
    };
    assert_eq!(
        injected_error,
        format!("runner parity retains inapplicable limitation {MEMBERSHIP_DIFFERS}")
    );
    Ok(())
}
