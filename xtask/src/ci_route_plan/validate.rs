use std::collections::BTreeSet;

use serde::Serialize;
use sha2::{Digest, Sha256};

use super::{
    Applicability, CI_ROUTE_PLAN_PRODUCER, CI_ROUTE_PLAN_SCHEMA, CiRoutePlanV1,
    PlannedOutcome, RoutePlanRow, RoutePlanSummary, RouteSelectionEvidence, RouteSubjectRef,
};

#[derive(Serialize)]
struct SemanticPlan<'a> {
    schema: &'a str,
    producer: &'a str,
    subject: &'a RouteSubjectRef,
    profile: &'a str,
    policy_digest: &'a str,
    workflow_digest: &'a str,
    selection: &'a RouteSelectionEvidence,
    rows: &'a [RoutePlanRow],
}

pub(super) fn normalize(plan: &mut CiRoutePlanV1) -> Result<(), String> {
    plan.rows
        .sort_by(|left, right| left.gate_id.cmp(&right.gate_id));
    plan.summary = summarize(&plan.rows)?;
    plan.semantic_fingerprint = semantic_fingerprint(plan)?;
    validate(plan)
}

pub(super) fn validate(plan: &CiRoutePlanV1) -> Result<(), String> {
    if plan.schema != CI_ROUTE_PLAN_SCHEMA {
        return Err(format!("unsupported route-plan schema {:?}", plan.schema));
    }
    if plan.producer != CI_ROUTE_PLAN_PRODUCER {
        return Err(format!(
            "unsupported route-plan producer {:?}",
            plan.producer
        ));
    }
    validate_subject(&plan.subject)?;
    validate_nonempty("profile", &plan.profile)?;
    validate_digest("policy_digest", &plan.policy_digest)?;
    validate_digest("workflow_digest", &plan.workflow_digest)?;
    validate_selection(&plan.selection)?;
    validate_digest("semantic_fingerprint", &plan.semantic_fingerprint)?;
    if plan.rows.is_empty() {
        return Err("route plan has no governed rows".to_string());
    }

    let mut seen = BTreeSet::new();
    let mut previous = None;
    for row in &plan.rows {
        validate_gate_id(&row.gate_id)?;
        if !seen.insert(row.gate_id.as_str()) {
            return Err(format!("duplicate gate identity {:?}", row.gate_id));
        }
        if let Some(previous) = previous {
            if previous > row.gate_id.as_str() {
                return Err("route-plan rows are not in canonical order".to_string());
            }
        }
        previous = Some(row.gate_id.as_str());
        validate_row(row)?;
    }

    if plan.summary != summarize(&plan.rows)? {
        return Err("route-plan summary does not reconcile to rows".to_string());
    }
    if plan.semantic_fingerprint != semantic_fingerprint(plan)? {
        return Err("route-plan semantic fingerprint does not match payload".to_string());
    }
    Ok(())
}

fn semantic_fingerprint(plan: &CiRoutePlanV1) -> Result<String, String> {
    let semantic = SemanticPlan {
        schema: &plan.schema,
        producer: &plan.producer,
        subject: &plan.subject,
        profile: &plan.profile,
        policy_digest: &plan.policy_digest,
        workflow_digest: &plan.workflow_digest,
        selection: &plan.selection,
        rows: &plan.rows,
    };
    let bytes = serde_json::to_vec(&semantic).map_err(|error| error.to_string())?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn summarize(rows: &[RoutePlanRow]) -> Result<RoutePlanSummary, String> {
    let mut summary = RoutePlanSummary {
        governed: rows.len() as u64,
        ..RoutePlanSummary::default()
    };
    for row in rows {
        *summary
            .by_policy_role
            .entry(row.policy_role)
            .or_default() += 1;
        match &row.outcome {
            PlannedOutcome::Run { .. } => summary.run += 1,
            PlannedOutcome::ScopedNoop { .. } => summary.scoped_noop += 1,
            PlannedOutcome::Quarantined { .. } => summary.quarantined += 1,
            PlannedOutcome::Error { .. } => summary.error += 1,
        }
    }
    let classified = summary.run + summary.scoped_noop + summary.quarantined + summary.error;
    if classified != summary.governed {
        return Err("route-plan outcome counts do not reconcile".to_string());
    }
    Ok(summary)
}

fn validate_row(row: &RoutePlanRow) -> Result<(), String> {
    match &row.outcome {
        PlannedOutcome::Run {
            command,
            timeout_seconds,
            reason,
        } => {
            require_applicability(row, Applicability::Applicable)?;
            validate_nonempty("run command", command)?;
            validate_nonempty("run reason", reason)?;
            if *timeout_seconds == 0 {
                return Err(format!("run gate {:?} has zero timeout", row.gate_id));
            }
        }
        PlannedOutcome::ScopedNoop {
            reason,
            selector_digest,
        } => {
            require_applicability(row, Applicability::NotApplicable)?;
            validate_nonempty("scoped-noop reason", reason)?;
            validate_digest("selector_digest", selector_digest)?;
        }
        PlannedOutcome::Quarantined {
            reason,
            owner_issue,
            review_after,
        } => {
            require_applicability(row, Applicability::Applicable)?;
            validate_nonempty("quarantine reason", reason)?;
            if *owner_issue == 0 {
                return Err(format!(
                    "quarantined gate {:?} has no owner",
                    row.gate_id
                ));
            }
            if let Some(review_after) = review_after {
                validate_nonempty("quarantine review_after", review_after)?;
            }
        }
        PlannedOutcome::Error { code, message } => {
            require_applicability(row, Applicability::Unknown)?;
            validate_reason_token("error code", code)?;
            validate_nonempty("error message", message)?;
        }
    }
    Ok(())
}

fn require_applicability(row: &RoutePlanRow, expected: Applicability) -> Result<(), String> {
    if row.applicability != expected {
        return Err(format!(
            "gate {:?} has contradictory applicability/outcome",
            row.gate_id
        ));
    }
    Ok(())
}

pub(super) fn validate_subject(subject: &RouteSubjectRef) -> Result<(), String> {
    validate_reason_token("subject kind", &subject.kind)?;
    validate_sha("subject head_sha", &subject.head_sha)?;
    if let Some(base_sha) = &subject.base_sha {
        validate_sha("subject base_sha", base_sha)?;
    }
    validate_digest("subject_digest", &subject.subject_digest)
}

fn validate_selection(selection: &RouteSelectionEvidence) -> Result<(), String> {
    validate_nonempty("selection base", &selection.base)?;
    validate_digest("selection selector_digest", &selection.selector_digest)?;
    match (selection.fallback_used, &selection.fallback_reason) {
        (true, Some(reason)) => validate_nonempty("fallback_reason", reason)?,
        (true, None) => return Err("fallback_used requires fallback_reason".to_string()),
        (false, Some(_)) => {
            return Err("fallback_reason is present while fallback_used is false".to_string());
        }
        (false, None) => {}
    }
    if selection.package_args.iter().any(|arg| arg.is_empty()) {
        return Err("selection package_args contain an empty argument".to_string());
    }
    if let Some(scope) = &selection.scope {
        validate_sha("scope head_sha", &scope.head_sha)?;
        validate_reason_token("scope diff_class", &scope.diff_class)?;
        for (name, values) in [
            ("direct_crates", &scope.direct_crates),
            ("reverse_dependencies", &scope.reverse_dependencies),
            ("architecture_wideners", &scope.architecture_wideners),
        ] {
            if !values.windows(2).all(|window| window[0] < window[1]) {
                return Err(format!("scope {name} is not canonical"));
            }
            for value in values {
                validate_nonempty("scope identity name", &value.name)?;
                validate_nonempty("scope identity reason", &value.reason)?;
            }
        }
        if !is_sorted_unique(&scope.risk_tags) {
            return Err("scope risk_tags are not canonical".to_string());
        }
    }
    Ok(())
}

fn is_sorted_unique(values: &[String]) -> bool {
    values.windows(2).all(|window| window[0] < window[1])
}

pub(super) fn validate_gate_id(value: &str) -> Result<(), String> {
    if value.is_empty() || !value.bytes().all(valid_gate_byte) {
        return Err(format!(
            "gate identity must match ^[a-z0-9_.-]+$: {value:?}"
        ));
    }
    Ok(())
}

fn valid_gate_byte(byte: u8) -> bool {
    byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'.' | b'-')
}

pub(super) fn validate_reason_token(subject: &str, value: &str) -> Result<(), String> {
    if value.is_empty() || !value.bytes().all(valid_reason_byte) {
        return Err(format!(
            "{subject} must match ^[a-z0-9_]+$: {value:?}"
        ));
    }
    Ok(())
}

fn valid_reason_byte(byte: u8) -> bool {
    byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_'
}

pub(super) fn validate_nonempty(subject: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("{subject} must be non-empty"));
    }
    Ok(())
}

pub(super) fn validate_sha(subject: &str, value: &str) -> Result<(), String> {
    if value.len() != 40 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{subject} must be a full 40-character SHA"));
    }
    Ok(())
}

pub(super) fn validate_digest(subject: &str, value: &str) -> Result<(), String> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!(
            "{subject} must be a 64-character hexadecimal digest"
        ));
    }
    Ok(())
}
