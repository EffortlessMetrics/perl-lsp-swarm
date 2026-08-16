use std::collections::{BTreeMap, BTreeSet};

use super::validate::{validate_digest, validate_gate_id, validate_nonempty, validate_subject};
use super::{
    Applicability, CI_ROUTE_PLAN_PRODUCER, CI_ROUTE_PLAN_SCHEMA, CiRoutePlanV1,
    CompileRoutePlanInput, LegacyPlannedGate, LegacyPlanningRole, LegacyScopeInput,
    LegacySkippedGate, PlannedOutcome, PolicyRole, RoutePlanRow, RoutePlanSummary,
    RouteScopeEvidence, RouteSelectionEvidence, SelectorRole,
};

pub(super) fn compile(input: CompileRoutePlanInput) -> Result<CiRoutePlanV1, String> {
    validate_nonempty("profile", &input.plan.tier)?;
    validate_digest("policy_digest", &input.policy_digest)?;
    validate_digest("workflow_digest", &input.workflow_digest)?;
    validate_digest("selector_digest", &input.selector_digest)?;
    validate_subject(&input.subject)?;

    let mut policy_by_name = BTreeMap::new();
    let mut all_policy_names = BTreeSet::new();
    for row in input.policy.gates {
        validate_gate_id(&row.name)?;
        validate_nonempty("gate policy tier", &row.tier)?;
        if !all_policy_names.insert(row.name.clone()) {
            return Err("legacy gate policy contains duplicate gate identity".to_string());
        }
        if row.tier == input.plan.tier {
            policy_by_name.insert(row.name.clone(), row);
        }
    }

    let selected_names = selected_gate_names(&input.plan.selected);
    if selected_names.len() != input.plan.selected.len() {
        return Err("legacy plan contains duplicate selected gate identity".to_string());
    }
    let skipped_names = skipped_gate_names(&input.plan.skipped);
    if skipped_names.len() != input.plan.skipped.len() {
        return Err("legacy plan contains duplicate skipped gate identity".to_string());
    }
    if let Some(overlap) = selected_names.intersection(&skipped_names).next() {
        return Err(format!("gate {overlap:?} is both selected and skipped"));
    }

    let capacity = input.plan.selected.len() + input.plan.skipped.len();
    let mut rows = Vec::with_capacity(capacity);
    for selected in input.plan.selected {
        validate_gate_id(&selected.name)?;
        validate_nonempty("selected reason", &selected.reason)?;
        let policy = policy_by_name
            .remove(&selected.name)
            .ok_or_else(|| format!("selected gate {:?} has no policy row", selected.name))?;
        let policy_role = policy_role(policy.required, input.plan.tier.as_str());
        let outcome = if policy.quarantine {
            if let Some(owner_issue) = policy.quarantine_owner_issue {
                PlannedOutcome::Quarantined {
                    reason: selected.reason,
                    owner_issue,
                    review_after: policy.quarantine_review_after,
                }
            } else {
                PlannedOutcome::Error {
                    code: "quarantine_owner_missing".to_string(),
                    message: format!("quarantined gate {:?} has no owner issue", policy.name),
                }
            }
        } else {
            PlannedOutcome::Run {
                command: policy.command,
                timeout_seconds: policy.timeout_seconds,
                reason: selected.reason,
            }
        };
        let applicability = if matches!(&outcome, PlannedOutcome::Error { .. }) {
            Applicability::Unknown
        } else {
            Applicability::Applicable
        };
        rows.push(RoutePlanRow {
            gate_id: policy.name,
            policy_role,
            selector_role: selector_role(selected.role),
            applicability,
            outcome,
        });
    }

    for skipped in input.plan.skipped {
        validate_gate_id(&skipped.name)?;
        validate_nonempty("skipped reason", &skipped.reason)?;
        let policy = policy_by_name
            .remove(&skipped.name)
            .ok_or_else(|| format!("skipped gate {:?} has no policy row", skipped.name))?;
        let selector_role = skipped
            .role
            .map(selector_role)
            .unwrap_or(SelectorRole::Unspecified);
        rows.push(RoutePlanRow {
            gate_id: policy.name,
            policy_role: policy_role(policy.required, input.plan.tier.as_str()),
            selector_role,
            applicability: Applicability::NotApplicable,
            outcome: PlannedOutcome::ScopedNoop {
                reason: skipped.reason,
                selector_digest: input.selector_digest.clone(),
            },
        });
    }

    if !policy_by_name.is_empty() {
        return Err(format!(
            "legacy plan omits governed gate(s): {}",
            policy_by_name.keys().cloned().collect::<Vec<_>>().join(", ")
        ));
    }

    let selection = RouteSelectionEvidence {
        base: input.plan.base,
        scope_ok: input.plan.scope_ok,
        fallback_used: input.plan.fallback_used,
        fallback_reason: input.plan.fallback_reason,
        package_args: input.plan.package_args,
        scope: input.plan.scope.map(normalize_scope).transpose()?,
        selector_digest: input.selector_digest,
    };
    let mut plan = CiRoutePlanV1 {
        schema: CI_ROUTE_PLAN_SCHEMA.to_string(),
        producer: CI_ROUTE_PLAN_PRODUCER.to_string(),
        subject: input.subject,
        profile: input.plan.tier,
        policy_digest: input.policy_digest,
        workflow_digest: input.workflow_digest,
        selection,
        rows,
        summary: RoutePlanSummary::default(),
        semantic_fingerprint: String::new(),
    };
    plan.normalize()?;
    Ok(plan)
}

fn selected_gate_names(rows: &[LegacyPlannedGate]) -> BTreeSet<&str> {
    rows.iter().map(|row| row.name.as_str()).collect()
}

fn skipped_gate_names(rows: &[LegacySkippedGate]) -> BTreeSet<&str> {
    rows.iter().map(|row| row.name.as_str()).collect()
}

fn selector_role(role: LegacyPlanningRole) -> SelectorRole {
    match role {
        LegacyPlanningRole::AlwaysOn => SelectorRole::AlwaysOn,
        LegacyPlanningRole::RustScoped => SelectorRole::RustScoped,
        LegacyPlanningRole::RustFallback => SelectorRole::RustFallback,
        LegacyPlanningRole::RustPackageScoped => SelectorRole::RustPackageScoped,
        LegacyPlanningRole::Static => SelectorRole::Static,
    }
}

fn normalize_scope(mut scope: LegacyScopeInput) -> Result<RouteScopeEvidence, String> {
    super::validate::validate_sha("scope head_sha", &scope.head_sha)?;
    super::validate::validate_reason_token("scope diff_class", &scope.diff_class)?;
    for values in [
        &mut scope.direct_crates,
        &mut scope.reverse_dependencies,
        &mut scope.architecture_wideners,
    ] {
        for value in values.iter() {
            validate_nonempty("scope identity name", &value.name)?;
            validate_nonempty("scope identity reason", &value.reason)?;
        }
        values.sort();
        values.dedup();
    }
    scope.risk_tags.sort();
    scope.risk_tags.dedup();
    Ok(RouteScopeEvidence {
        head_sha: scope.head_sha,
        diff_class: scope.diff_class,
        direct_crates: scope.direct_crates,
        reverse_dependencies: scope.reverse_dependencies,
        architecture_wideners: scope.architecture_wideners,
        risk_tags: scope.risk_tags,
    })
}

fn policy_role(required: bool, profile: &str) -> PolicyRole {
    if profile == "release" {
        PolicyRole::ReleaseOnly
    } else if profile == "commit" {
        PolicyRole::LocalOnly
    } else if required {
        PolicyRole::Required
    } else {
        PolicyRole::Advisory
    }
}
