//! Exact normalized membership comparison between two runner plans.

use crate::build::validate_runner_plan;
use crate::runner_model::{
    InvocationCaptureStatus, MembershipParityStatus, RUNNER_PARITY_SCHEMA_VERSION,
    RunnerKind, RunnerParityReport, RunnerPlan,
};
use std::collections::BTreeSet;

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

    let left_members = left.normalized_membership.iter().cloned().collect::<BTreeSet<_>>();
    let right_members = right.normalized_membership.iter().cloned().collect::<BTreeSet<_>>();
    let missing_from_right = left_members.difference(&right_members).cloned().collect::<Vec<_>>();
    let extra_in_right = right_members.difference(&left_members).cloned().collect::<Vec<_>>();
    let membership_status = if matches!(
        (left.runner, right.runner),
        (RunnerKind::DirectFallback, _) | (_, RunnerKind::DirectFallback)
    ) {
        MembershipParityStatus::NotProven
    } else if missing_from_right.is_empty() && extra_in_right.is_empty() {
        MembershipParityStatus::Parity
    } else {
        MembershipParityStatus::Mismatch
    };
    let mut limitations = vec![
        "per_file_upstream_scan_and_effective_invocation_not_compared".to_string(),
    ];
    if membership_status == MembershipParityStatus::NotProven {
        limitations.push("direct_fallback_cannot_establish_upstream_runner_parity".to_string());
    }
    if membership_status == MembershipParityStatus::Mismatch {
        limitations.push("normalized_membership_differs".to_string());
    }
    limitations.sort();

    let report = RunnerParityReport {
        schema_version: RUNNER_PARITY_SCHEMA_VERSION.to_string(),
        matrix_fingerprint: left.matrix_fingerprint.clone(),
        target_id: left.target_id.clone(),
        target_contract_digest: left.target_contract_digest.clone(),
        left_runner: left.runner,
        right_runner: right.runner,
        membership_status,
        missing_from_right,
        extra_in_right,
        order_equal: left.normalized_order == right.normalized_order,
        scheduling_equal: left.scheduling == right.scheduling,
        invocation_capture: InvocationCaptureStatus::NotProven,
        limitations,
        claim_boundary: "normalized target membership parity only; order and scheduling are reported but need not match, and per-file upstream _scan_test invocation is not proved".to_string(),
    };
    validate_runner_parity(&report)?;
    Ok(report)
}

pub(crate) fn validate_runner_parity(report: &RunnerParityReport) -> Result<(), String> {
    if report.schema_version != RUNNER_PARITY_SCHEMA_VERSION {
        return Err(format!("unsupported runner parity schema {}", report.schema_version));
    }
    if report.target_id.trim().is_empty() || report.claim_boundary.trim().is_empty() {
        return Err("runner parity report contains incomplete identity or claim boundary".to_string());
    }
    for (value, label) in [
        (&report.matrix_fingerprint, "matrix fingerprint"),
        (&report.target_contract_digest, "target contract digest"),
    ] {
        if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(format!("{label} must be a 64-character hexadecimal digest"));
        }
    }
    if report.missing_from_right.windows(2).any(|pair| pair[0] >= pair[1])
        || report.extra_in_right.windows(2).any(|pair| pair[0] >= pair[1])
        || report.limitations.windows(2).any(|pair| pair[0] >= pair[1])
    {
        return Err("runner parity list fields must be strictly sorted and unique".to_string());
    }
    match report.membership_status {
        MembershipParityStatus::Parity => {
            if !report.missing_from_right.is_empty() || !report.extra_in_right.is_empty() {
                return Err("parity report cannot contain membership differences".to_string());
            }
        }
        MembershipParityStatus::Mismatch => {
            if report.missing_from_right.is_empty() && report.extra_in_right.is_empty() {
                return Err("mismatch report requires a membership difference".to_string());
            }
        }
        MembershipParityStatus::NotProven => {
            if report.left_runner != RunnerKind::DirectFallback
                && report.right_runner != RunnerKind::DirectFallback
            {
                return Err("not-proven parity requires a direct fallback plan".to_string());
            }
        }
    }
    Ok(())
}
