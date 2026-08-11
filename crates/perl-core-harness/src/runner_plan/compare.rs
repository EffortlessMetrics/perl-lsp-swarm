//! Exact normalized membership comparison between two runner plans.

use crate::build::{
    runner_plan_digest, validate_runner_plan, validate_runner_plan_against,
};
use crate::model::UpstreamTargetMatrix;
use crate::runner_model::{
    InvocationCaptureStatus, MembershipParityStatus, RUNNER_PARITY_SCHEMA_VERSION,
    RunnerKind, RunnerParityReport, RunnerPlan,
};
use std::collections::BTreeSet;

const INVOCATION_COMPARISON_LIMITATION: &str =
    "per_file_upstream_scan_and_effective_invocation_not_compared";
const DIRECT_FALLBACK_PARITY_LIMITATION: &str =
    "direct_fallback_cannot_establish_upstream_runner_parity";
const SAME_RUNNER_PARITY_LIMITATION: &str =
    "same_runner_comparison_cannot_establish_cross_runner_parity";
const MEMBERSHIP_DIFFERS_LIMITATION: &str = "normalized_membership_differs";

pub(crate) fn compare_runner_plans(
    left: &RunnerPlan,
    right: &RunnerPlan,
) -> Result<RunnerParityReport, String> {
    validate_runner_plan(left)?;
    validate_runner_plan(right)?;
    if left.matrix_fingerprint != right.matrix_fingerprint
        || left.target_id != right.target_id
        || left.target_contract_digest != right.target_contract_digest
    {
        return Err("runner plans do not identify the same target contract".to_string());
    }

    let left_members = left
        .normalized_membership
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let right_members = right
        .normalized_membership
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let missing_from_right = left_members
        .difference(&right_members)
        .cloned()
        .collect::<Vec<_>>();
    let extra_in_right = right_members
        .difference(&left_members)
        .cloned()
        .collect::<Vec<_>>();
    let has_difference = !missing_from_right.is_empty() || !extra_in_right.is_empty();
    let has_direct_fallback = left.runner == RunnerKind::DirectFallback
        || right.runner == RunnerKind::DirectFallback;
    let same_runner = left.runner == right.runner;
    let membership_status = if has_direct_fallback || same_runner {
        MembershipParityStatus::NotProven
    } else if has_difference {
        MembershipParityStatus::Mismatch
    } else {
        MembershipParityStatus::Parity
    };

    let mut limitations = vec![INVOCATION_COMPARISON_LIMITATION.to_string()];
    if has_direct_fallback {
        limitations.push(DIRECT_FALLBACK_PARITY_LIMITATION.to_string());
    }
    if same_runner {
        limitations.push(SAME_RUNNER_PARITY_LIMITATION.to_string());
    }
    if has_difference {
        limitations.push(MEMBERSHIP_DIFFERS_LIMITATION.to_string());
    }
    limitations.sort();

    let report = RunnerParityReport {
        schema_version: RUNNER_PARITY_SCHEMA_VERSION.to_string(),
        matrix_fingerprint: left.matrix_fingerprint.clone(),
        target_id: left.target_id.clone(),
        target_contract_digest: left.target_contract_digest.clone(),
        left_plan_digest: runner_plan_digest(left)?,
        right_plan_digest: runner_plan_digest(right)?,
        left_raw_discovery_digest: left.raw_discovery_digest.clone(),
        right_raw_discovery_digest: right.raw_discovery_digest.clone(),
        left_runner: left.runner,
        right_runner: right.runner,
        membership_status,
        missing_from_right,
        extra_in_right,
        order_equal: left.normalized_order == right.normalized_order,
        scheduling_equal: left.scheduling == right.scheduling,
        invocation_capture: InvocationCaptureStatus::NotProven,
        limitations,
        claim_boundary: "content-bound normalized target membership parity only; order and scheduling remain separate, and per-file upstream _scan_test invocation is not proved".to_string(),
    };
    validate_runner_parity(&report)?;
    Ok(report)
}

pub(crate) fn compare_runner_plans_against(
    matrix: &UpstreamTargetMatrix,
    left: &RunnerPlan,
    left_raw_discovery: &[u8],
    right: &RunnerPlan,
    right_raw_discovery: &[u8],
) -> Result<RunnerParityReport, String> {
    validate_runner_plan_against(matrix, left_raw_discovery, left)?;
    validate_runner_plan_against(matrix, right_raw_discovery, right)?;
    compare_runner_plans(left, right)
}

pub(crate) fn validate_runner_parity(report: &RunnerParityReport) -> Result<(), String> {
    if report.schema_version != RUNNER_PARITY_SCHEMA_VERSION {
        return Err(format!(
            "unsupported runner parity schema {}",
            report.schema_version
        ));
    }
    validate_stable_id(&report.target_id, "target ID")?;
    if report.claim_boundary.trim().is_empty() {
        return Err("runner parity report contains an empty claim boundary".to_string());
    }
    for (value, label) in [
        (&report.matrix_fingerprint, "matrix fingerprint"),
        (&report.target_contract_digest, "target contract digest"),
        (&report.left_plan_digest, "left plan digest"),
        (&report.right_plan_digest, "right plan digest"),
        (
            &report.left_raw_discovery_digest,
            "left raw discovery digest",
        ),
        (
            &report.right_raw_discovery_digest,
            "right raw discovery digest",
        ),
    ] {
        validate_sha256(value, label)?;
    }
    if report
        .missing_from_right
        .windows(2)
        .any(|pair| pair[0] >= pair[1])
        || report
            .extra_in_right
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
        || report.limitations.windows(2).any(|pair| pair[0] >= pair[1])
    {
        return Err("runner parity list fields must be strictly sorted and unique".to_string());
    }
    if !has_limitation(&report.limitations, INVOCATION_COMPARISON_LIMITATION) {
        return Err("runner parity must retain its invocation-comparison limitation".to_string());
    }

    let has_difference =
        !report.missing_from_right.is_empty() || !report.extra_in_right.is_empty();
    let has_direct_fallback = report.left_runner == RunnerKind::DirectFallback
        || report.right_runner == RunnerKind::DirectFallback;
    let same_runner = report.left_runner == report.right_runner;
    require_exact_limitation(
        &report.limitations,
        DIRECT_FALLBACK_PARITY_LIMITATION,
        has_direct_fallback,
    )?;
    require_exact_limitation(
        &report.limitations,
        SAME_RUNNER_PARITY_LIMITATION,
        same_runner,
    )?;
    require_exact_limitation(
        &report.limitations,
        MEMBERSHIP_DIFFERS_LIMITATION,
        has_difference,
    )?;

    match report.membership_status {
        MembershipParityStatus::Parity => {
            if has_difference || has_direct_fallback || same_runner {
                return Err(
                    "parity requires distinct upstream runners with identical membership"
                        .to_string(),
                );
            }
        }
        MembershipParityStatus::Mismatch => {
            if !has_difference || has_direct_fallback || same_runner {
                return Err(
                    "mismatch requires distinct upstream runners and a membership difference"
                        .to_string(),
                );
            }
        }
        MembershipParityStatus::NotProven => {
            if !has_direct_fallback && !same_runner {
                return Err(
                    "not-proven parity requires direct fallback or a same-runner comparison"
                        .to_string(),
                );
            }
        }
    }
    Ok(())
}

pub(crate) fn validate_runner_parity_against(
    report: &RunnerParityReport,
    left: &RunnerPlan,
    right: &RunnerPlan,
) -> Result<(), String> {
    validate_runner_parity(report)?;
    let expected = compare_runner_plans(left, right)?;
    if expected != *report {
        return Err(
            "runner parity report does not match the exact supplied plan receipts".to_string(),
        );
    }
    Ok(())
}

fn has_limitation(limitations: &[String], expected: &str) -> bool {
    limitations.iter().any(|value| value == expected)
}

fn require_exact_limitation(
    limitations: &[String],
    limitation: &str,
    required: bool,
) -> Result<(), String> {
    if has_limitation(limitations, limitation) == required {
        Ok(())
    } else if required {
        Err(format!("runner parity is missing required limitation {limitation}"))
    } else {
        Err(format!("runner parity retains inapplicable limitation {limitation}"))
    }
}

fn validate_stable_id(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        Err(format!("{label} must match [a-z0-9_]+: {value}"))
    } else {
        Ok(())
    }
}

fn validate_sha256(value: &str, label: &str) -> Result<(), String> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Err(format!(
            "{label} must be a 64-character hexadecimal digest: {value}"
        ))
    } else {
        Ok(())
    }
}
