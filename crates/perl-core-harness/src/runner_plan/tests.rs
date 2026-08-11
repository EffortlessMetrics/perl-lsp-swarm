//! Runner-plan and parity falsifiers over the pinned target matrix.

use crate::build::{
    build_runner_plan, validate_runner_plan, validate_runner_plan_against,
};
use crate::compare::{
    compare_runner_plans, compare_runner_plans_against, validate_runner_parity,
    validate_runner_parity_against,
};
use crate::io::read_matrix;
use crate::runner_model::{
    MembershipParityStatus, RunnerKind, RunnerScheduling, SourceForm,
};
use color_eyre::eyre::Result;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn repo_file(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

fn matrix() -> Result<crate::model::UpstreamTargetMatrix> {
    read_matrix(&repo_file(
        ".ci/perl-core-harness/upstream-targets-5.42.2.v1",
    ))
}

fn base_plan(
    matrix: &crate::model::UpstreamTargetMatrix,
    runner: RunnerKind,
    raw: &[u8],
) -> Result<crate::runner_model::RunnerPlan> {
    build_runner_plan(
        matrix,
        "component_base",
        runner,
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
        harness_raw,
        RunnerScheduling {
            jobs: Some(2),
            asap: false,
            state_ordering: true,
            properties: BTreeMap::new(),
        },
    )
    .map_err(|error| color_eyre::eyre::eyre!(error))?;
    let parity = compare_runner_plans_against(
        &matrix,
        &test_plan,
        test_raw,
        &harness_plan,
        harness_raw,
    )
    .map_err(|error| color_eyre::eyre::eyre!(error))?;
    assert_eq!(parity.membership_status, MembershipParityStatus::Parity);
    assert!(!parity.order_equal);
    assert!(!parity.scheduling_equal);
    assert_ne!(parity.left_plan_digest, parity.right_plan_digest);
    assert_eq!(
        parity.left_raw_discovery_digest,
        test_plan.raw_discovery_digest
    );
    assert_eq!(
        parity.right_raw_discovery_digest,
        harness_plan.raw_discovery_digest
    );
    assert!(
        harness_plan
            .limitations
            .iter()
            .any(|value| value == "alternate_runner_requires_membership_parity_evidence")
    );
    assert!(test_plan.limitations.iter().any(|value| {
        value == "scheduling_inputs_are_declared_not_observed"
    }));
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
        b"t/op/hook/hook.t\n",
        RunnerScheduling::default(),
    );
    assert!(nested.is_ok());
    let direct = build_runner_plan(
        &matrix,
        "component_op",
        RunnerKind::Harness,
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
fn manifest_population_accepts_dot_t_and_test_pl() -> Result<()> {
    let matrix = matrix()?;
    let plan = build_runner_plan(
        &matrix,
        "manifest_cpan",
        RunnerKind::Test,
        b"cpan/Foo/t/basic.t\ncpan/Foo/test.pl\n",
        RunnerScheduling::default(),
    )
    .map_err(|error| color_eyre::eyre::eyre!(error))?;
    let forms = plan
        .source_items
        .iter()
        .map(|item| item.source_form)
        .collect::<Vec<_>>();
    assert_eq!(forms, vec![SourceForm::DotT, SourceForm::TestPl]);
    Ok(())
}

#[test]
fn serialized_source_form_is_recomputed_from_the_raw_path() -> Result<()> {
    let matrix = matrix()?;
    let mut plan = build_runner_plan(
        &matrix,
        "manifest_cpan",
        RunnerKind::Test,
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
    let left = base_plan(
        &matrix,
        RunnerKind::Test,
        b"t/base/cond.t\nt/base/if.t\n",
    )?;
    let right = base_plan(&matrix, RunnerKind::Harness, b"t/base/if.t\n")?;
    let parity = compare_runner_plans(&left, &right)
        .map_err(|error| color_eyre::eyre::eyre!(error))?;
    assert_eq!(parity.membership_status, MembershipParityStatus::Mismatch);
    assert_eq!(
        parity.missing_from_right,
        vec!["t/base/cond.t".to_string()]
    );
    Ok(())
}

#[test]
fn direct_fallback_cannot_claim_upstream_runner_parity() -> Result<()> {
    let matrix = matrix()?;
    let upstream = base_plan(&matrix, RunnerKind::Test, b"t/base/if.t\n")?;
    let fallback = base_plan(
        &matrix,
        RunnerKind::DirectFallback,
        b"t/base/if.t\n",
    )?;
    let parity = compare_runner_plans(&upstream, &fallback)
        .map_err(|error| color_eyre::eyre::eyre!(error))?;
    assert_eq!(parity.membership_status, MembershipParityStatus::NotProven);
    assert!(
        parity.limitations.iter().any(|value| {
            value == "direct_fallback_cannot_establish_upstream_runner_parity"
        })
    );

    let mut forged = parity;
    forged.membership_status = MembershipParityStatus::Parity;
    assert!(validate_runner_parity(&forged).is_err());
    Ok(())
}

#[test]
fn same_runner_comparison_is_not_cross_runner_parity() -> Result<()> {
    let matrix = matrix()?;
    let left = base_plan(&matrix, RunnerKind::Test, b"t/base/if.t\n")?;
    let right = base_plan(&matrix, RunnerKind::Test, b"t/base/if.t\n")?;
    let parity = compare_runner_plans(&left, &right)
        .map_err(|error| color_eyre::eyre::eyre!(error))?;
    assert_eq!(parity.membership_status, MembershipParityStatus::NotProven);
    assert!(parity.limitations.iter().any(|value| {
        value == "same_runner_comparison_cannot_establish_cross_runner_parity"
    }));

    let mut forged = parity;
    forged.membership_status = MembershipParityStatus::Parity;
    assert!(validate_runner_parity(&forged).is_err());
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

    assert!(
        validate_runner_plan_against(&matrix, b"t/base/cond.t\n", &plan).is_err()
    );
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

    let mut parity_without_boundary = compare_runner_plans(&left, &right)
        .map_err(|error| color_eyre::eyre::eyre!(error))?;
    parity_without_boundary.limitations.retain(|value| {
        value != "scheduling_equality_compares_declared_inputs_not_observed_runner_state"
    });
    assert!(validate_runner_parity(&parity_without_boundary).is_err());
    Ok(())
}

#[test]
fn parity_check_is_bound_to_exact_plan_bytes() -> Result<()> {
    let matrix = matrix()?;
    let left = base_plan(&matrix, RunnerKind::Test, b"t/base/if.t\n")?;
    let right = base_plan(&matrix, RunnerKind::Harness, b"t/base/if.t\n")?;
    let report = compare_runner_plans(&left, &right)
        .map_err(|error| color_eyre::eyre::eyre!(error))?;
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
        b"t/base/if.t\nt/base/if.t\n",
        RunnerScheduling::default(),
    );
    assert!(result.is_err());
    Ok(())
}
