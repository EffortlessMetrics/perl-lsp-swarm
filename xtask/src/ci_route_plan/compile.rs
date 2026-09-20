use std::collections::{BTreeMap, BTreeSet};

use super::validate::{
    validate_digest, validate_gate_id, validate_nonempty, validate_selection, validate_subject,
};
use super::{
    Applicability, CI_ROUTE_PLAN_PRODUCER, CI_ROUTE_PLAN_SCHEMA, CiRoutePlanV1,
    CompileRoutePlanInput, ExpansionStatus, GateSelectorInput, LifecycleState, PlannedOutcome,
    Resolution, RouteDispositionInput, RouteExecutionIdentity, RoutePlanRow, RoutePlanSummary,
    RouteProfileExpansionInput, RouteSelectionEvidence, SelectorPlacement, SelectorProof,
    SelectorRole,
};

/// Compose one gate's planned outcome from the projected #10176/#10178
/// authority values and the runner's selector facts.
///
/// Rule (mirroring the `gate_disposition.v1` planner seam): lifecycle
/// resolution decides first; a positive exact-subject selector proof is
/// required for `run` and `scoped_noop` and can never mutate lifecycle.
/// Nothing here re-derives lifecycle, policy role, or denominator membership.
fn compose_outcome(
    disposition: &RouteDispositionInput,
    selector: Option<&GateSelectorInput>,
    selection: &RouteSelectionEvidence,
    execution: Option<&RouteExecutionIdentity>,
) -> Result<(Applicability, PlannedOutcome), String> {
    let placement =
        selector.map(|selector| selector.placement).unwrap_or(SelectorPlacement::Skipped);
    let proof = selector.and_then(|selector| selector.proof);
    let applicability = match proof {
        Some(SelectorProof::Applicable) => Applicability::Applicable,
        Some(SelectorProof::NotApplicableToSubject) => Applicability::NotApplicable,
        None => Applicability::Unknown,
    };

    // Placement is an observed runner fact. When it contradicts a positive
    // selector proof the axes are contradictory and the row becomes a typed
    // error instead of silently trusting either side.
    match (placement, proof) {
        (SelectorPlacement::Selected, Some(SelectorProof::NotApplicableToSubject))
        | (SelectorPlacement::Skipped, Some(SelectorProof::Applicable)) => {
            return Ok((
                applicability,
                PlannedOutcome::Error {
                    code: "selector_contradiction".to_string(),
                    message: format!(
                        "gate {:?} placement {} contradicts its positive selector proof",
                        disposition.gate_id,
                        placement.as_str()
                    ),
                },
            ));
        }
        _ => {}
    }

    if disposition.lifecycle.state == LifecycleState::Active
        && disposition.lifecycle.resolution == Resolution::Current
    {
        // The only lifecycle state a selector can act on.
        return match selector.map(|selector| (selector, selector.proof)) {
            Some((selector, Some(SelectorProof::Applicable))) => {
                let Some(execution) = execution else {
                    return Ok((
                        applicability,
                        PlannedOutcome::Error {
                            code: "execution_identity_missing".to_string(),
                            message: format!(
                                "runnable gate {:?} has no executable identity",
                                disposition.gate_id
                            ),
                        },
                    ));
                };
                if execution.timeout_seconds == 0 {
                    return Ok((
                        applicability,
                        PlannedOutcome::Error {
                            code: "run_timeout_invalid".to_string(),
                            message: format!(
                                "runnable gate {:?} has a zero timeout",
                                disposition.gate_id
                            ),
                        },
                    ));
                }
                Ok((
                    applicability,
                    PlannedOutcome::Run {
                        command: execution.command.clone(),
                        timeout_seconds: execution.timeout_seconds,
                        reason: selector.reason.clone(),
                    },
                ))
            }
            Some((selector, Some(SelectorProof::NotApplicableToSubject))) => Ok((
                applicability,
                PlannedOutcome::ScopedNoop {
                    reason: selector.reason.clone(),
                    selector_digest: selection.selector_digest.clone(),
                },
            )),
            Some((_, None)) | None => Ok((
                applicability,
                PlannedOutcome::Error {
                    code: "selector_evidence_missing".to_string(),
                    message: format!(
                        "gate {:?} is active/current but carries no positive exact-subject \
                         selector proof",
                        disposition.gate_id
                    ),
                },
            )),
        };
    }

    // Lifecycle-governed rows. Expired or invalid evidence and non-runnable
    // lifecycles stay visible typed error rows; a current quarantine keeps
    // its #10176 owner/reason/review identity. A skipped quarantined gate
    // lands here — it can never become `scoped_noop`.
    if disposition.lifecycle.resolution != Resolution::Current {
        let code = if disposition.lifecycle.resolution == Resolution::Expired {
            "disposition_expired"
        } else {
            "disposition_invalid"
        };
        return Ok((
            applicability,
            PlannedOutcome::Error {
                code: code.to_string(),
                message: disposition.detail.clone().unwrap_or_else(|| {
                    format!(
                        "gate {:?} has {} disposition evidence",
                        disposition.gate_id,
                        disposition.lifecycle.resolution.as_str()
                    )
                }),
            },
        ));
    }
    if disposition.lifecycle.state == LifecycleState::Quarantined {
        let Some(quarantine) = disposition.quarantine.as_ref() else {
            return Err(format!(
                "current quarantined disposition {:?} carries no quarantine evidence; the \
                 #10176 projection is invalid",
                disposition.gate_id
            ));
        };
        return Ok((
            applicability,
            PlannedOutcome::Quarantined {
                reason: quarantine.reason_token.clone(),
                owner: quarantine.owner.clone(),
                owner_issue: quarantine.owner_issue.clone(),
                review_after: quarantine.review_after.clone(),
            },
        ));
    }
    Ok((
        applicability,
        PlannedOutcome::Error {
            code: "lifecycle_non_runnable".to_string(),
            message: format!(
                "gate {:?} lifecycle {} is non-runnable",
                disposition.gate_id,
                disposition.lifecycle.state.as_str()
            ),
        },
    ))
}

pub(super) fn compile(input: CompileRoutePlanInput) -> Result<CiRoutePlanV1, String> {
    validate_subject(&input.subject)?;
    validate_digest("workflow_digest", &input.workflow_digest)?;
    validate_digest("disposition_digest", &input.disposition_digest)?;
    validate_selection(&input.selection)?;

    // #10178: only a complete expansion carries a consumable denominator.
    // Unsupported (release) and invalid expansions fail closed here — they
    // can never alias another profile or silently drop gates.
    if input.expansion.resolution != ExpansionStatus::Complete {
        return Err(format!(
            "route-profile expansion for {:?} is not consumable ({}): {}",
            input.expansion.requested_profile,
            input.expansion.resolution.as_str(),
            input.expansion.detail.as_deref().unwrap_or("no detail")
        ));
    }
    validate_expansion_shape(&input.expansion)?;

    // Dispositions must be keyed uniquely by gate.
    let mut dispositions_by_id: BTreeMap<&str, &RouteDispositionInput> = BTreeMap::new();
    for disposition in &input.dispositions {
        validate_gate_id(&disposition.gate_id)?;
        validate_nonempty("disposition native_tier", &disposition.native_tier)?;
        if dispositions_by_id.insert(disposition.gate_id.as_str(), disposition).is_some() {
            return Err(format!("duplicate disposition identity {:?}", disposition.gate_id));
        }
    }

    // Selector inputs must be keyed uniquely by gate.
    let mut selectors_by_id: BTreeMap<&str, &GateSelectorInput> = BTreeMap::new();
    for selector in &input.selectors {
        validate_gate_id(&selector.gate_id)?;
        validate_nonempty("selector reason", &selector.reason)?;
        if selectors_by_id.insert(selector.gate_id.as_str(), selector).is_some() {
            return Err(format!("duplicate selector identity {:?}", selector.gate_id));
        }
    }

    // Executable identities must be keyed uniquely by gate.
    let mut execution_by_id: BTreeMap<&str, &RouteExecutionIdentity> = BTreeMap::new();
    for identity in &input.execution {
        validate_gate_id(&identity.gate_id)?;
        validate_nonempty("execution command", &identity.command)?;
        if execution_by_id.insert(identity.gate_id.as_str(), identity).is_some() {
            return Err(format!("duplicate execution identity {:?}", identity.gate_id));
        }
    }

    // Exactly one row per governed denominator gate, consumed from #10178;
    // membership is never recomputed here.
    let denominator = input.expansion.denominator.clone();
    let mut rows = Vec::with_capacity(denominator.len());
    for gate_id in &denominator {
        let disposition = dispositions_by_id.get(gate_id.as_str()).ok_or_else(|| {
            format!("denominator gate {gate_id:?} has no typed disposition (#10176 projection)")
        })?;
        let selector = selectors_by_id.get(gate_id.as_str()).copied();
        let execution = execution_by_id.get(gate_id.as_str()).copied();
        let (applicability, outcome) =
            compose_outcome(disposition, selector, &input.selection, execution)?;
        rows.push(RoutePlanRow {
            gate_id: gate_id.clone(),
            native_tier: disposition.native_tier.clone(),
            policy_role: disposition.policy_role,
            lifecycle: disposition.lifecycle,
            selector_role: selector
                .and_then(|selector| selector.role)
                .unwrap_or(SelectorRole::Unspecified),
            selector_placement: selector
                .map(|selector| selector.placement)
                .unwrap_or(SelectorPlacement::Skipped),
            applicability,
            outcome,
        });
    }

    let summary = summarize(&rows)?;
    let mut plan = CiRoutePlanV1 {
        schema: CI_ROUTE_PLAN_SCHEMA.to_string(),
        producer: CI_ROUTE_PLAN_PRODUCER.to_string(),
        subject: input.subject,
        requested_profile: input.expansion.requested_profile,
        included_native_tiers: input.expansion.included_native_tiers,
        expansion_fingerprint: input.expansion.semantic_fingerprint,
        policy_digest: input.expansion.policy_digest,
        disposition_digest: input.disposition_digest,
        workflow_digest: input.workflow_digest,
        denominator,
        selection: input.selection,
        rows,
        summary,
        // Computed immediately from the assembled plan below; the empty
        // placeholder can never survive validation, which recomputes and
        // compares the digest.
        semantic_fingerprint: String::new(),
    };
    plan.semantic_fingerprint = plan.semantic_fingerprint_of()?;
    super::validate::validate(&plan)?;
    Ok(plan)
}

/// Structural checks on the projected expansion before its denominator is
/// consumed. Membership and profile rules stay in #10178; this only refuses
/// internally inconsistent projections.
fn validate_expansion_shape(expansion: &RouteProfileExpansionInput) -> Result<(), String> {
    validate_nonempty("requested_profile", &expansion.requested_profile)?;
    validate_digest("expansion_fingerprint", &expansion.semantic_fingerprint)?;
    validate_digest("policy_digest", &expansion.policy_digest)?;
    if expansion.included_native_tiers.is_empty() {
        return Err("expanded profile includes no native tiers".to_string());
    }
    let mut seen = BTreeSet::new();
    for tier in &expansion.included_native_tiers {
        validate_nonempty("included native tier", tier)?;
        if !seen.insert(tier.as_str()) {
            return Err(format!("included native tier {tier:?} is duplicated"));
        }
    }
    if expansion.denominator.is_empty() {
        return Err("governed denominator is empty".to_string());
    }
    let mut sorted = expansion.denominator.clone();
    sorted.sort();
    sorted.dedup();
    if sorted.len() != expansion.denominator.len() {
        return Err("governed denominator contains duplicate gate identity".to_string());
    }
    if sorted != expansion.denominator {
        return Err("governed denominator is not in canonical order".to_string());
    }
    for gate_id in &expansion.denominator {
        validate_gate_id(gate_id)?;
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
