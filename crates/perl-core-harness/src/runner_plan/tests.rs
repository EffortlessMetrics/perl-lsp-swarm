//! Runner-plan and parity falsifiers over the pinned target matrix.

use crate::build::build_runner_plan;
use crate::compare::compare_runner_plans;
use crate::io::read_matrix;
use crate::runner_model::{
    MembershipParityStatus, RunnerKind, RunnerScheduling, SourceForm,
};
use color_eyre::eyre::Result;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn repo_file(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(relative)
}

fn matrix() -> Result<crate::model::UpstreamTargetMatrix> {
    read_matrix(&repo_file(
        ".ci/perl-core-harness/upstream-targets-5.42.2.v1",
    ))
}

#[test]
fn test_and_harness_membership_can_match_with_different_order() -> Result<()> {
    let matrix = matrix()?;
    let test_plan = build_runner_plan(
        &matrix,
        "component_base",
        RunnerKind::Test,
        b"t/base/cond.t\nt/base/if.t\n",
        RunnerScheduling::default(),
    )
    .map_err(|error| color_eyre::eyre::eyre!(error))?;
    let harness_plan = build_runner_plan(
        &matrix,
        "component_base",
        RunnerKind::Harness,
        b"t/base/if.t\nt/base/cond.t\n",
        RunnerScheduling {
            jobs: Some(2),
            asap: false,
            state_ordering: true,
            properties: BTreeMap::new(),
        },
    )
    .map_err(|error| color_eyre::eyre::eyre!(error))?;
    let parity = compare_runner_plans(&test_plan, &harness_plan)
        .map_err(|error| color_eyre::eyre::eyre!(error))?;
    assert_eq!(parity.membership_status, MembershipParityStatus::Parity);
    assert!(!parity.order_equal);
    assert!(!parity.scheduling_equal);
    assert!(
        harness_plan
            .limitations
            .iter()
            .any(|value| value == "alternate_runner_requires_membership_parity_evidence")
    );
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
    assert_eq!(plan.source_items[0].source_form, SourceForm::DotT);
    assert_eq!(plan.source_items[1].source_form, SourceForm::TestPl);
    Ok(())
}

#[test]
fn real_membership_difference_is_not_hidden_by_order_normalization() -> Result<()> {
    let matrix = matrix()?;
    let left = build_runner_plan(
        &matrix,
        "component_base",
        RunnerKind::Test,
        b"t/base/cond.t\nt/base/if.t\n",
        RunnerScheduling::default(),
    )
    .map_err(|error| color_eyre::eyre::eyre!(error))?;
    let right = build_runner_plan(
        &matrix,
        "component_base",
        RunnerKind::Harness,
        b"t/base/if.t\n",
        RunnerScheduling::default(),
    )
    .map_err(|error| color_eyre::eyre::eyre!(error))?;
    let parity = compare_runner_plans(&left, &right)
        .map_err(|error| color_eyre::eyre::eyre!(error))?;
    assert_eq!(parity.membership_status, MembershipParityStatus::Mismatch);
    assert_eq!(parity.missing_from_right, vec!["t/base/cond.t".to_string()]);
    Ok(())
}

#[test]
fn direct_fallback_cannot_claim_upstream_runner_parity() -> Result<()> {
    let matrix = matrix()?;
    let upstream = build_runner_plan(
        &matrix,
        "component_base",
        RunnerKind::Test,
        b"t/base/if.t\n",
        RunnerScheduling::default(),
    )
    .map_err(|error| color_eyre::eyre::eyre!(error))?;
    let fallback = build_runner_plan(
        &matrix,
        "component_base",
        RunnerKind::DirectFallback,
        b"t/base/if.t\n",
        RunnerScheduling::default(),
    )
    .map_err(|error| color_eyre::eyre::eyre!(error))?;
    let parity = compare_runner_plans(&upstream, &fallback)
        .map_err(|error| color_eyre::eyre::eyre!(error))?;
    assert_eq!(parity.membership_status, MembershipParityStatus::NotProven);
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
