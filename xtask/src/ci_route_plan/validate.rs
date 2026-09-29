use std::collections::BTreeSet;

use super::{
    Applicability, CI_ROUTE_PLAN_PRODUCER, CI_ROUTE_PLAN_SCHEMA, CLOSED_ERROR_CODES, CiRoutePlanV1,
    LifecycleDisposition, LifecycleState, PlannedOutcome, Resolution, RoutePlanRow,
    RoutePlanSummary, RouteSelectionEvidence, RouteSubjectRef, SelectorPlacement,
};

/// Closed requested-profile vocabulary owned by `ci_route_profile.v1`
/// (#10178). The lib-side domain cannot import the binary-side authority, so
/// this set mirrors it for validation only; the authority remains the
/// derivation owner and the adapter projects its exact value. The #10179
/// schema drift test consumes this list so the checked-in JSON-Schema
/// projection cannot silently drift from the typed vocabulary. `stack_local`
/// is the #11229 S1 advisory stack-increment profile admitted through the
/// same owner; it can never enter a protected-main denominator.
pub const KNOWN_PROFILES: &[&str] =
    &["commit", "pr_fast", "merge_gate", "nightly", "all", "release", "stack_local"];

pub(super) fn validate(plan: &CiRoutePlanV1) -> Result<(), String> {
    if plan.schema != CI_ROUTE_PLAN_SCHEMA {
        return Err(format!("unsupported route-plan schema {:?}", plan.schema));
    }
    if plan.producer != CI_ROUTE_PLAN_PRODUCER {
        return Err(format!("unsupported route-plan producer {:?}", plan.producer));
    }
    validate_subject(&plan.subject)?;
    if !KNOWN_PROFILES.contains(&plan.requested_profile.as_str()) {
        return Err(format!("unknown requested profile {:?}", plan.requested_profile));
    }
    validate_digest("expansion_fingerprint", &plan.expansion_fingerprint)?;
    validate_digest("policy_digest", &plan.policy_digest)?;
    validate_digest("disposition_digest", &plan.disposition_digest)?;
    validate_digest("workflow_digest", &plan.workflow_digest)?;
    validate_selection(&plan.selection)?;

    // Scope evidence is bound to the exact route subject: a scope computed
    // for another head SHA is stale evidence and cannot back any
    // selector-proved outcome of this subject, even though both values are
    // individually well-formed.
    if let Some(scope) = &plan.selection.scope
        && scope.head_sha != plan.subject.head_sha
    {
        return Err(format!(
            "selection scope head SHA {} does not match the route subject head SHA {}; stale \
             scope evidence cannot back selector-proved outcomes",
            scope.head_sha, plan.subject.head_sha
        ));
    }

    validate_tiers(&plan.included_native_tiers)?;
    validate_denominator(&plan.denominator)?;

    // Exactly one row per governed denominator gate, in canonical order:
    // omissions and duplicates cannot validate.
    if plan.rows.len() != plan.denominator.len() {
        return Err(format!(
            "route plan has {} rows for {} governed denominator gates",
            plan.rows.len(),
            plan.denominator.len()
        ));
    }
    for (row, gate_id) in plan.rows.iter().zip(&plan.denominator) {
        if &row.gate_id != gate_id {
            return Err(format!(
                "route-plan row order does not reconcile with the governed denominator at \
                 {gate_id:?}"
            ));
        }
        validate_row(row, &plan.selection)?;
    }

    if plan.summary != summarize(&plan.rows)? {
        return Err("route-plan summary does not reconcile to rows".to_string());
    }

    // Fingerprint agreement runs last: every field-level check above
    // surfaces its specific refusal first, and only a fully consistent
    // plan gets its semantic identity recomputed. The stored field must
    // equal the recomputed digest — fingerprint movement alone cannot
    // validate (#10179).
    let recomputed = plan.semantic_fingerprint_of()?;
    if plan.semantic_fingerprint != recomputed {
        return Err(format!(
            "semantic fingerprint {} does not equal the recomputed digest {} of the canonical \
             semantic projection",
            plan.semantic_fingerprint, recomputed
        ));
    }
    Ok(())
}

fn validate_tiers(tiers: &[String]) -> Result<(), String> {
    if tiers.is_empty() {
        return Err("route plan includes no native tiers".to_string());
    }
    let mut seen = BTreeSet::new();
    for tier in tiers {
        validate_nonempty("included native tier", tier)?;
        if !seen.insert(tier.as_str()) {
            return Err(format!("included native tier {tier:?} is duplicated"));
        }
    }
    Ok(())
}

fn validate_denominator(denominator: &[String]) -> Result<(), String> {
    if denominator.is_empty() {
        return Err("route plan has no governed denominator".to_string());
    }
    if !is_sorted_unique(denominator) {
        return Err("governed denominator is not in canonical order".to_string());
    }
    for gate_id in denominator {
        validate_gate_id(gate_id)?;
    }
    Ok(())
}

fn validate_row(row: &RoutePlanRow, selection: &RouteSelectionEvidence) -> Result<(), String> {
    validate_nonempty("native tier", &row.native_tier)?;
    match &row.outcome {
        PlannedOutcome::Run { command, timeout_seconds, reason } => {
            require(
                row.applicability == Applicability::Applicable,
                row,
                "run requires applicable",
            )?;
            require(
                row.selector_placement == SelectorPlacement::Selected,
                row,
                "run requires a selected placement",
            )?;
            require(
                row.lifecycle
                    == LifecycleDisposition {
                        state: LifecycleState::Active,
                        resolution: Resolution::Current,
                    },
                row,
                "run requires an active current lifecycle",
            )?;
            validate_nonempty("run command", command)?;
            validate_nonempty("run reason", reason)?;
            if *timeout_seconds == 0 {
                return Err(format!("run gate {:?} has zero timeout", row.gate_id));
            }
        }
        PlannedOutcome::ScopedNoop { reason, selector_digest } => {
            require(
                row.applicability == Applicability::NotApplicable,
                row,
                "scoped_noop requires not-applicable",
            )?;
            require(
                row.selector_placement == SelectorPlacement::Skipped,
                row,
                "scoped_noop requires a skipped placement",
            )?;
            require(
                row.lifecycle
                    == LifecycleDisposition {
                        state: LifecycleState::Active,
                        resolution: Resolution::Current,
                    },
                row,
                "scoped_noop requires an active current lifecycle",
            )?;
            validate_nonempty("scoped-noop reason", reason)?;
            validate_digest("selector_digest", selector_digest)?;
            if selector_digest != &selection.selector_digest {
                return Err(format!(
                    "gate {:?} scoped-noop selector digest does not match the selection \
                     evidence digest",
                    row.gate_id
                ));
            }
        }
        PlannedOutcome::Quarantined { reason, owner, owner_issue, review_after } => {
            require(
                row.lifecycle.state == LifecycleState::Quarantined
                    && row.lifecycle.resolution == Resolution::Current,
                row,
                "quarantined outcome requires a current quarantined lifecycle",
            )?;
            validate_nonempty("quarantine reason", reason)?;
            validate_nonempty("quarantine owner", owner)?;
            if let Some(owner_issue) = owner_issue {
                validate_nonempty("quarantine owner_issue", owner_issue)?;
            }
            validate_nonempty("quarantine review_after", review_after)?;
        }
        PlannedOutcome::Error { code, message } => {
            validate_reason_token("error code", code)?;
            if !CLOSED_ERROR_CODES.contains(&code.as_str()) {
                return Err(format!("gate {:?} carries unknown error code {code:?}", row.gate_id));
            }
            validate_nonempty("error message", message)?;
        }
    }
    Ok(())
}

fn require(condition: bool, row: &RoutePlanRow, message: &str) -> Result<(), String> {
    if !condition {
        return Err(format!("gate {:?}: {message}", row.gate_id));
    }
    Ok(())
}

fn summarize(rows: &[RoutePlanRow]) -> Result<RoutePlanSummary, String> {
    let mut summary =
        RoutePlanSummary { governed: rows.len() as u64, ..RoutePlanSummary::default() };
    for row in rows {
        *summary.by_policy_role.entry(row.policy_role).or_default() += 1;
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

pub(super) fn validate_subject(subject: &RouteSubjectRef) -> Result<(), String> {
    validate_reason_token("subject kind", &subject.kind)?;
    validate_sha("subject head_sha", &subject.head_sha)?;
    if let Some(base_sha) = &subject.base_sha {
        validate_sha("subject base_sha", base_sha)?;
    }
    validate_digest("subject_digest", &subject.subject_digest)
}

pub(super) fn validate_selection(selection: &RouteSelectionEvidence) -> Result<(), String> {
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
            if !is_sorted_unique_by(values, |value| value.name.clone()) {
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

fn is_sorted_unique<T: Ord>(values: &[T]) -> bool {
    values.windows(2).all(|window| window[0] < window[1])
}

fn is_sorted_unique_by<T, K: Ord>(values: &[T], key: impl Fn(&T) -> K) -> bool {
    values.windows(2).all(|window| key(&window[0]) < key(&window[1]))
}

pub(super) fn validate_gate_id(value: &str) -> Result<(), String> {
    if value.is_empty() || !value.bytes().all(valid_gate_byte) {
        return Err(format!("gate identity must match ^[a-z0-9_.-]+$: {value:?}"));
    }
    Ok(())
}

fn valid_gate_byte(byte: u8) -> bool {
    byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'.' | b'-')
}

pub(super) fn validate_reason_token(subject: &str, value: &str) -> Result<(), String> {
    if value.is_empty() || !value.bytes().all(valid_reason_byte) {
        return Err(format!("{subject} must match ^[a-z0-9_]+$: {value:?}"));
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
    if value.len() != 40 || !value.bytes().all(valid_lowercase_hex_byte) {
        return Err(format!("{subject} must be a full 40-character lowercase hexadecimal SHA"));
    }
    Ok(())
}

pub(super) fn validate_digest(subject: &str, value: &str) -> Result<(), String> {
    if value.len() != 64 || !value.bytes().all(valid_lowercase_hex_byte) {
        return Err(format!("{subject} must be a 64-character lowercase hexadecimal digest"));
    }
    Ok(())
}

fn valid_lowercase_hex_byte(byte: u8) -> bool {
    byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
}
