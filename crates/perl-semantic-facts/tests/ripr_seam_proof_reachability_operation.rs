//! External-consumer seam proof for the reachability operation contract
//! (#11553).
//!
//! Exercises the public contract surface from outside the crate — subject
//! construction, budget validation, tracked charging with cancellation and
//! exhaustion, terminal outcomes, publication eligibility, work honesty, and
//! bounded views — the way the liveness-train consumers (#10915/#10921/
//! #10928/#10935) will consume it. This file follows the
//! `ripr_seam_proof_*` harness convention; its plumbing is suppressed from
//! diff-scoped seam counting (policy/ripr-suppressions.toml, ripr#1428).
#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598): census-clean on all targets; new findings move the crate to C1.

use perl_semantic_facts::reachability_operation::{
    ReachabilityBoundedView, ReachabilityCancellationPolling, ReachabilityChargeError,
    ReachabilityClaimLimitation, ReachabilityCompleteResultRef, ReachabilityContractError,
    ReachabilityDimensionLimit, ReachabilityExecutionProfile, ReachabilityExecutionPurpose,
    ReachabilityExhaustionAttempt, ReachabilityFactFamilyId, ReachabilityFactFamilyLedger,
    ReachabilityFactFamilyStatus, ReachabilityIneligibilityReason, ReachabilityOperationControl,
    ReachabilityOperationId, ReachabilityOperationKind, ReachabilityOperationOutcome,
    ReachabilityOperationSubject, ReachabilityProfileId, ReachabilityPublicationEligibility,
    ReachabilityRetentionLimits, ReachabilitySemanticOutcome, ReachabilityStageId,
    ReachabilityStageLimitation, ReachabilityStageOutput, ReachabilitySubjectIdentity,
    ReachabilitySubjectIdentityKind, ReachabilityTerminalObservation, ReachabilityTerminalState,
    ReachabilityUnlimitedJustification, ReachabilityWorkBudget, ReachabilityWorkDimension,
    ReachabilityWorkHonestyError, ReachabilityWorkPathTarget, ReachabilityWorkReceipt,
    ReachabilityWorkTracker, ReachableViewProfileId,
};
use std::collections::BTreeMap;
use std::error::Error;

struct LspRequestControl {
    cancelled: bool,
}

impl ReachabilityOperationControl for LspRequestControl {
    fn poll(
        &self,
        _subject: &ReachabilityOperationSubject,
    ) -> Option<ReachabilityTerminalObservation> {
        if self.cancelled {
            Some(ReachabilityTerminalObservation::Cancelled {
                control_identity: ReachabilitySubjectIdentity::new(
                    ReachabilitySubjectIdentityKind::ExternalControl,
                    "lsp-request-42",
                    None,
                )
                .ok()?,
            })
        } else {
            None
        }
    }
}

struct SupersedingControl {
    expected: ReachabilitySubjectIdentity,
    observed: ReachabilitySubjectIdentity,
}

impl ReachabilityOperationControl for SupersedingControl {
    fn poll(
        &self,
        _subject: &ReachabilityOperationSubject,
    ) -> Option<ReachabilityTerminalObservation> {
        Some(ReachabilityTerminalObservation::Superseded {
            expected: self.expected.clone(),
            observed: self.observed.clone(),
        })
    }
}

fn snapshot(generation: &str) -> Result<ReachabilitySubjectIdentity, Box<dyn Error>> {
    ReachabilitySubjectIdentity::new(
        ReachabilitySubjectIdentityKind::WorkspaceSnapshot,
        "workspace-snapshot-1",
        Some(generation.to_string()),
    )
    .map_err(|error| Box::new(error) as Box<dyn Error>)
}

fn instrument() -> Result<ReachabilitySubjectIdentity, Box<dyn Error>> {
    ReachabilitySubjectIdentity::new(
        ReachabilitySubjectIdentityKind::Instrument,
        "work-meter-1",
        None,
    )
    .map_err(|error| Box::new(error) as Box<dyn Error>)
}

fn subject() -> Result<ReachabilityOperationSubject, Box<dyn Error>> {
    ReachabilityOperationSubject::new(
        ReachabilityOperationId::new("liveness-op-9")?,
        ReachabilityOperationKind::EntityQuery,
        vec![
            snapshot("gen-3")?,
            ReachabilitySubjectIdentity::new(
                ReachabilitySubjectIdentityKind::Project,
                "project-1",
                None,
            )?,
            ReachabilitySubjectIdentity::new(
                ReachabilitySubjectIdentityKind::WorkBudgetProfile,
                "query-profile-v1",
                None,
            )?,
        ],
        ReachabilityProfileId::new("query-profile-v1")?,
    )
    .map_err(|error| Box::new(error) as Box<dyn Error>)
}

fn budget() -> Result<ReachabilityWorkBudget, Box<dyn Error>> {
    let mut limits = BTreeMap::new();
    limits.insert(ReachabilityWorkDimension::EntityQueries, 10);
    limits.insert(ReachabilityWorkDimension::NodesValidated, 50);
    ReachabilityWorkBudget::new(
        ReachabilityProfileId::new("query-profile-v1")?,
        vec![ReachabilityOperationKind::EntityQuery],
        limits,
        BTreeMap::new(),
    )
    .map_err(|error| Box::new(error) as Box<dyn Error>)
}

fn tracked() -> Result<ReachabilityWorkTracker, Box<dyn Error>> {
    ReachabilityWorkTracker::new(subject()?, budget()?)
        .map_err(|error| Box::new(error) as Box<dyn Error>)
}

fn complete_ledger() -> ReachabilityFactFamilyLedger {
    let mut families = BTreeMap::new();
    if let Ok(runtime) = ReachabilityFactFamilyId::new("runtime-edges") {
        families.insert(runtime, ReachabilityFactFamilyStatus::Complete);
    }
    ReachabilityFactFamilyLedger::new(families)
}

#[test]
fn external_consumer_walks_one_operation_from_subject_to_eligible_publication()
-> Result<(), Box<dyn std::error::Error>> {
    let stage = ReachabilityStageId::new("entity-query")?;
    let output = ReachabilitySubjectIdentity::new(
        ReachabilitySubjectIdentityKind::StageOutput,
        "query-answer-1",
        None,
    )?;
    let mut tracker = tracked()?;

    let continuing = tracker.poll_checkpoint(&stage, &LspRequestControl { cancelled: false });
    assert!(continuing.is_none());

    tracker.charge(ReachabilityWorkDimension::EntityQueries, 1)?;
    tracker.charge(ReachabilityWorkDimension::NodesValidated, 12)?;
    tracker.record_full_construction(stage.clone(), ReachabilityWorkPathTarget::QueryProjection)?;
    tracker.complete_stage(stage.clone(), Some(output.clone()), Vec::new());
    tracker.note_instrument_evidence(instrument()?);

    let receipt = tracker.finish();
    assert_eq!(receipt.charged().get(&ReachabilityWorkDimension::EntityQueries), Some(&1));
    assert_eq!(receipt.charged().get(&ReachabilityWorkDimension::NodesValidated), Some(&12));
    assert!(receipt.exhausted_attempts().is_empty());
    assert_eq!(receipt.checkpoints_observed(), std::slice::from_ref(&stage));
    assert_eq!(receipt.checkpoints_observed()[0].as_str(), "entity-query");
    assert_eq!(receipt.completed_stages(), std::slice::from_ref(&stage));
    assert_eq!(receipt.completed_stages()[0].as_str(), "entity-query");
    assert!(receipt.stage_limitations().is_empty());
    assert!(receipt.terminal().is_none());
    assert_eq!(receipt.work_after_eligibility_lost(), 0);
    assert!(receipt.instrument_identity().is_some());
    assert_eq!(
        receipt.instrument_identity().ok_or("instrument")?.kind(),
        ReachabilitySubjectIdentityKind::Instrument
    );
    assert!(receipt.instrument_evidence_complete());
    assert!(!receipt.is_validated_reuse_of(&ReachabilityWorkPathTarget::GraphInput));
    assert_eq!(receipt.work_paths()[0].stage(), &stage);
    assert_eq!(receipt.work_paths()[0].target(), &ReachabilityWorkPathTarget::QueryProjection);
    assert!(!receipt.work_paths()[0].is_validated_reuse());
    assert!(receipt.work_paths()[0].reused_identity().is_none());

    let answer = ReachabilityOperationOutcome::complete(
        ReachabilitySemanticOutcome::Complete,
        Some(vec![String::from("Package::helper")]),
        receipt,
    )?;
    assert!(!answer.is_execution_terminal());
    assert!(answer.may_claim_exact());
    assert!(answer.retained_partial_value().is_none());
    assert!(answer.work_receipt().instrument_evidence_complete());

    let verdict = ReachabilityPublicationEligibility::evaluate(
        &subject()?,
        Some(&snapshot("gen-3")?),
        ReachabilitySubjectIdentityKind::WorkspaceSnapshot,
        std::slice::from_ref(&stage),
        &answer,
        &complete_ledger(),
        std::slice::from_ref(&ReachabilityFactFamilyId::new("runtime-edges")?),
    );
    assert!(verdict.is_eligible());
    assert!(verdict.reasons().is_empty());

    let source: ReachabilityCompleteResultRef =
        answer.bounded_view_source(output, snapshot("gen-3")?).ok_or("bounded view source")?;
    assert_eq!(source.result_identity().as_str(), "query-answer-1");
    assert_eq!(source.currentness_authority().generation(), Some("gen-3"));
    let view = ReachabilityBoundedView::new(
        source,
        ReachableViewProfileId::new("view-profile")?,
        1,
        64,
        None,
        None,
        false,
        None,
    )?;
    assert!(!view.truncated());
    assert!(view.truncation_reason().is_none());
    assert_eq!(view.view_profile().as_str(), "view-profile");
    assert_eq!(view.items_returned(), 1);
    assert_eq!(view.bytes_returned(), 64);
    assert!(view.known_total().is_none());
    assert!(view.omitted_count().is_none());
    assert_eq!(view.underlying().result_identity().as_str(), "query-answer-1");
    Ok(())
}

#[test]
fn external_consumer_sees_typed_terminals_for_cancel_exhaustion_and_supersession()
-> Result<(), Box<dyn std::error::Error>> {
    let cancelled_stage = ReachabilityStageId::new("closure")?;
    let mut tracker = tracked()?;
    let terminal = tracker
        .poll_checkpoint(&cancelled_stage, &LspRequestControl { cancelled: true })
        .ok_or("cancellation must be observed")?;
    assert!(terminal.is_cancellation());
    // Non-interruptible work may finish privately but stays charged.
    tracker.charge(ReachabilityWorkDimension::NodesValidated, 3)?;
    let receipt = tracker.finish();
    assert_eq!(receipt.work_after_eligibility_lost(), 3);
    assert!(!receipt.work_after_eligibility_lost_overflow());
    assert!(receipt.work_after_eligibility_lost() > 0);
    let outcome = ReachabilityOperationOutcome::<Vec<String>>::terminal_from(
        &terminal,
        cancelled_stage,
        receipt,
    )?;
    assert!(outcome.is_execution_terminal());
    assert!(!outcome.may_claim_exact());

    let exhausted_stage = ReachabilityStageId::new("classification")?;
    let mut tracker = tracked()?;
    tracker.charge(ReachabilityWorkDimension::EntityQueries, 10)?;
    let refused = tracker
        .charge(ReachabilityWorkDimension::EntityQueries, 1)
        .err()
        .ok_or("charge past the limit must be refused")?;
    let ReachabilityChargeError::Exhausted { dimension, limit, charged } = refused else {
        return Err("charging past a bounded limit must be Exhausted".into());
    };
    assert_eq!(dimension, ReachabilityWorkDimension::EntityQueries);
    assert_eq!((limit, charged), (10, 10));
    let terminal = tracker.terminal().cloned().ok_or("exhaustion must latch a terminal")?;
    assert!(terminal.is_resource_exhausted());
    assert!(!terminal.is_cancellation());
    assert!(matches!(terminal, ReachabilityTerminalState::ResourceExhausted { .. }));
    if let ReachabilityTerminalState::ResourceExhausted { dimension, limit, charged } = &terminal {
        assert_eq!(
            (*dimension, *limit, *charged),
            (ReachabilityWorkDimension::EntityQueries, 10, 10)
        );
    }
    let receipt = tracker.finish();
    assert_eq!(receipt.exhausted_attempts().len(), 1);
    assert_eq!(receipt.exhausted_attempts()[0].limit, 10);
    assert_eq!(receipt.exhausted_attempts()[0].charged, 10);
    let attempt = ReachabilityExhaustionAttempt {
        dimension: ReachabilityWorkDimension::EntityQueries,
        limit: 10,
        charged: 10,
    };
    assert_eq!(attempt.dimension, ReachabilityWorkDimension::EntityQueries);
    assert_eq!(attempt.limit, 10);
    assert_eq!(attempt.charged, 10);
    assert_eq!(receipt.exhausted_attempts()[0].dimension, attempt.dimension);
    let outcome = ReachabilityOperationOutcome::<Vec<String>>::terminal_from(
        &terminal,
        exhausted_stage,
        receipt,
    )?;
    assert!(matches!(outcome, ReachabilityOperationOutcome::ResourceExhausted { .. }));
    if let ReachabilityOperationOutcome::ResourceExhausted { dimension, limit, charged, .. } =
        &outcome
    {
        assert_eq!(
            (*dimension, *limit, *charged),
            (ReachabilityWorkDimension::EntityQueries, 10, 10)
        );
    }

    let superseded = ReachabilityTerminalObservation::Superseded {
        expected: snapshot("gen-3")?,
        observed: snapshot("gen-4")?,
    };
    assert!(superseded.is_supersession());
    assert!(!superseded.is_deadline());
    let stage = ReachabilityStageId::new("publication")?;
    let mut tracker = tracked()?;
    tracker.note_instrument_evidence(instrument()?);
    let terminal = tracker
        .poll_checkpoint(
            &stage,
            &SupersedingControl { expected: snapshot("gen-3")?, observed: snapshot("gen-4")? },
        )
        .ok_or("supersession must be observed")?;
    let outcome = ReachabilityOperationOutcome::<Vec<String>>::terminal_from(
        &terminal,
        stage,
        tracker.finish(),
    )?;
    assert!(matches!(outcome, ReachabilityOperationOutcome::SupersededOrStale { .. }));
    if let ReachabilityOperationOutcome::SupersededOrStale { expected, observed, .. } = &outcome {
        assert_eq!(expected.generation(), Some("gen-3"));
        assert_eq!(observed.generation(), Some("gen-4"));
    }

    let verdict = ReachabilityPublicationEligibility::evaluate(
        &subject()?,
        snapshot("gen-4").as_ref().ok(),
        ReachabilitySubjectIdentityKind::WorkspaceSnapshot,
        &[],
        &outcome,
        &complete_ledger(),
        &[],
    );
    assert!(!verdict.is_eligible());
    assert!(verdict.reasons().contains(&ReachabilityIneligibilityReason::SubjectSuperseded));
    Ok(())
}

#[test]
fn external_consumer_validates_budgets_profiles_and_work_honesty_fail_closed()
-> Result<(), Box<dyn std::error::Error>> {
    let operation_budget = budget()?;
    assert!(
        operation_budget
            .validate_requirements(&[
                ReachabilityWorkDimension::EntityQueries,
                ReachabilityWorkDimension::NodesValidated,
            ])
            .is_ok()
    );
    assert_eq!(
        operation_budget.validate_requirements(&[ReachabilityWorkDimension::EdgesAdmitted]),
        Err(ReachabilityContractError::MissingRequiredDimension {
            dimension: ReachabilityWorkDimension::EdgesAdmitted,
        })
    );
    assert_eq!(
        operation_budget.limit_for(ReachabilityWorkDimension::EntityQueries),
        Some(ReachabilityDimensionLimit::Bounded(10))
    );
    assert!(operation_budget.limit_for(ReachabilityWorkDimension::SccNodesVisited).is_none());
    assert_eq!(
        operation_budget.selected_operation_kinds(),
        &[ReachabilityOperationKind::EntityQuery]
    );
    assert_eq!(operation_budget.profile_id().as_str(), "query-profile-v1");

    let justification = ReachabilityUnlimitedJustification::new(
        ReachabilityWorkDimension::ExplanationPaths,
        "reviewed-2026-08 interactive explanations",
        5_000,
    )?;
    assert_eq!(justification.reason(), "reviewed-2026-08 interactive explanations");
    assert_eq!(justification.safety_bound(), 5_000);
    assert!(
        ReachabilityUnlimitedJustification::new(
            ReachabilityWorkDimension::ExplanationPaths,
            "",
            5_000,
        )
        .is_err()
    );

    let profile = ReachabilityExecutionProfile::new(
        ReachabilityProfileId::new("query-profile-v1")?,
        2,
        ReachabilityExecutionPurpose::Interactive,
        vec![ReachabilityOperationKind::EntityQuery],
        vec![ReachabilityFactFamilyId::new("runtime-edges")?],
        ReachabilityCancellationPolling::AtDeclaredCheckpoints,
        ReachabilityRetentionLimits {
            max_explanation_items: Some(100),
            max_output_bytes: Some(2_048),
        },
        "performance-authority-2026-08",
        Vec::new(),
    )?;
    assert_eq!(profile.profile_id().as_str(), "query-profile-v1");
    assert_eq!(profile.version(), 2);
    assert_eq!(profile.purpose(), ReachabilityExecutionPurpose::Interactive);
    assert_eq!(
        profile.cancellation_polling(),
        ReachabilityCancellationPolling::AtDeclaredCheckpoints
    );
    assert_eq!(profile.retention().max_explanation_items, Some(100));
    assert_eq!(profile.retention().max_output_bytes, Some(2_048));
    assert_eq!(profile.defaults_source(), "performance-authority-2026-08");
    assert_eq!(profile.selected_fact_families().len(), 1);
    assert!(profile.limitations().is_empty());

    assert_eq!(
        ReachabilityOperationKind::parse("entity-query"),
        Ok(ReachabilityOperationKind::EntityQuery)
    );
    assert!(ReachabilityOperationKind::parse("freestyle-scan").is_err());
    assert_eq!(
        ReachabilityWorkDimension::parse("entity-queries"),
        Ok(ReachabilityWorkDimension::EntityQueries)
    );
    assert!(ReachabilityWorkDimension::parse("elapsed-seconds").is_err());

    let stage = ReachabilityStageId::new("entity-query")?;
    let mut tracker = tracked()?;
    assert_eq!(
        tracker.record_validated_reuse(
            stage.clone(),
            ReachabilityWorkPathTarget::ResultReuse,
            ReachabilitySubjectIdentity::new(
                ReachabilitySubjectIdentityKind::StageOutput,
                "undeclared-output",
                None,
            )?,
        ),
        Err(ReachabilityContractError::WorkHonesty(
            ReachabilityWorkHonestyError::ReuseWithoutDeclaredIdentity { stage: stage.clone() }
        ))
    );
    assert!(
        tracker
            .record_validated_reuse(
                stage.clone(),
                ReachabilityWorkPathTarget::ResultReuse,
                snapshot("gen-3")?,
            )
            .is_ok()
    );
    assert_eq!(
        tracker.record_full_construction(stage.clone(), ReachabilityWorkPathTarget::ResultReuse),
        Err(ReachabilityContractError::WorkHonesty(
            ReachabilityWorkHonestyError::FullConstructionAfterValidatedReuse { stage }
        ))
    );
    let reuse_receipt = tracker.finish();
    assert!(reuse_receipt.is_validated_reuse_of(&ReachabilityWorkPathTarget::ResultReuse));
    assert_eq!(reuse_receipt.work_paths()[0].target(), &ReachabilityWorkPathTarget::ResultReuse);
    assert!(reuse_receipt.work_paths()[0].is_validated_reuse());
    assert_eq!(
        reuse_receipt.work_paths()[0].reused_identity().ok_or("reused identity")?.kind(),
        ReachabilitySubjectIdentityKind::WorkspaceSnapshot
    );

    let subject = subject()?;
    assert_eq!(subject.operation_id().as_str(), "liveness-op-9");
    assert_eq!(subject.kind(), ReachabilityOperationKind::EntityQuery);
    assert_eq!(subject.budget_profile_id().as_str(), "query-profile-v1");
    assert!(!subject.identities().is_empty());
    assert_eq!(
        subject
            .identity(ReachabilitySubjectIdentityKind::Project)
            .ok_or("project identity")?
            .as_str(),
        "project-1"
    );
    assert!(subject.identity(ReachabilitySubjectIdentityKind::Environment).is_none());

    let stage_output = ReachabilityStageOutput::new(
        ReachabilityStageId::new("scc-condensation")?,
        ReachabilitySubjectIdentity::new(
            ReachabilitySubjectIdentityKind::StageOutput,
            "component-graph-1",
            None,
        )?,
    );
    assert_eq!(stage_output.stage().as_str(), "scc-condensation");
    assert_eq!(stage_output.output().as_str(), "component-graph-1");
    Ok(())
}

#[test]
fn external_consumer_cannot_conflate_partial_or_empty_claims()
-> Result<(), Box<dyn std::error::Error>> {
    let clean_receipt = {
        let mut tracker = tracked()?;
        tracker.note_instrument_evidence(instrument()?);
        tracker.finish()
    };
    assert_eq!(
        ReachabilityOperationOutcome::<Vec<String>>::complete(
            ReachabilitySemanticOutcome::Complete,
            None,
            clean_receipt.clone(),
        ),
        Err(ReachabilityContractError::CompleteWithoutValue)
    );
    assert_eq!(
        ReachabilityOperationOutcome::<Vec<String>>::complete(
            ReachabilitySemanticOutcome::LegitimateEmpty,
            Some(Vec::new()),
            clean_receipt,
        ),
        Err(ReachabilityContractError::EmptyWithRetainedValue)
    );

    let partial_receipt = {
        let mut tracker = tracked()?;
        tracker.complete_stage(
            ReachabilityStageId::new("graph-admission")?,
            None,
            vec![ReachabilityClaimLimitation::PartialDenominator],
        );
        tracker.note_instrument_evidence(instrument()?);
        tracker.finish()
    };
    assert_eq!(partial_receipt.stage_limitations().len(), 1);
    let partial = ReachabilityOperationOutcome::complete(
        ReachabilitySemanticOutcome::Partial {
            limitations: vec![ReachabilityClaimLimitation::PartialDenominator],
        },
        Some(vec![String::from("partial-rows")]),
        partial_receipt,
    )?;
    assert_eq!(partial.retained_partial_value(), Some(&vec![String::from("partial-rows")]));
    assert!(!partial.may_claim_exact());
    assert!(
        partial
            .bounded_view_source(
                ReachabilitySubjectIdentity::new(
                    ReachabilitySubjectIdentityKind::StageOutput,
                    "partial-output",
                    None,
                )?,
                snapshot("gen-3")?,
            )
            .is_none()
    );

    let empty = ReachabilityOperationOutcome::<Vec<String>>::complete(
        ReachabilitySemanticOutcome::LegitimateEmpty,
        None,
        {
            let mut tracker = tracked()?;
            tracker.note_instrument_evidence(instrument()?);
            tracker.finish()
        },
    )?;
    assert!(empty.may_claim_exact());
    assert!(ReachabilitySemanticOutcome::Complete.may_carry_value());
    assert!(ReachabilitySemanticOutcome::LegitimateEmpty.is_exact());
    assert!(!ReachabilitySemanticOutcome::Stale.is_exact());

    let mut missing = BTreeMap::new();
    let runtime = ReachabilityFactFamilyId::new("runtime-edges")?;
    missing.insert(runtime.clone(), ReachabilityFactFamilyStatus::Missing);
    let ledger = ReachabilityFactFamilyLedger::new(missing);
    assert_eq!(ledger.status(&runtime), ReachabilityFactFamilyStatus::Missing);
    assert!(!ledger.requires_complete(std::slice::from_ref(&runtime)));
    let verdict = ReachabilityPublicationEligibility::evaluate(
        &subject()?,
        Some(&snapshot("gen-3")?),
        ReachabilitySubjectIdentityKind::WorkspaceSnapshot,
        &[],
        &empty,
        &ledger,
        std::slice::from_ref(&runtime),
    );
    assert!(!verdict.is_eligible());
    assert!(
        verdict.reasons().iter().any(|reason| matches!(
            reason,
            ReachabilityIneligibilityReason::DenominatorIncomplete(_)
        ))
    );

    let source: ReachabilityCompleteResultRef = empty
        .bounded_view_source(
            ReachabilitySubjectIdentity::new(
                ReachabilitySubjectIdentityKind::StageOutput,
                "empty-query-1",
                None,
            )?,
            snapshot("gen-3")?,
        )
        .ok_or("legitimate empty mints a bounded-view source")?;
    assert_eq!(source.currentness_authority().as_str(), "workspace-snapshot-1");
    assert!(
        ReachabilityBoundedView::new(
            source,
            ReachableViewProfileId::new("v")?,
            0,
            0,
            Some(0),
            Some(0),
            false,
            None,
        )
        .is_ok()
    );
    Ok(())
}

#[test]
fn receipts_serialize_identically_from_outside_the_crate() -> Result<(), Box<dyn std::error::Error>>
{
    let first_receipt = {
        let mut tracker = tracked()?;
        tracker.charge(ReachabilityWorkDimension::EntityQueries, 2)?;
        tracker.charge(ReachabilityWorkDimension::NodesValidated, 5)?;
        tracker.finish()
    };
    let second_receipt = {
        let mut tracker = tracked()?;
        tracker.charge(ReachabilityWorkDimension::NodesValidated, 5)?;
        tracker.charge(ReachabilityWorkDimension::EntityQueries, 2)?;
        tracker.finish()
    };

    assert_eq!(first_receipt, second_receipt);
    assert_eq!(serde_json::to_string(&first_receipt)?, serde_json::to_string(&second_receipt)?);
    let round_tripped: ReachabilityWorkReceipt =
        serde_json::from_str(&serde_json::to_string(&first_receipt)?)?;
    assert_eq!(round_tripped, first_receipt);
    Ok(())
}

#[test]
fn external_consumer_classifies_every_truth_limitation_and_path_variant()
-> Result<(), Box<dyn Error>> {
    // Truth variants: non-valued states never carry values; only Complete
    // and LegitimateEmpty are exact.
    for truth in [
        ReachabilitySemanticOutcome::NotReady,
        ReachabilitySemanticOutcome::Ambiguous,
        ReachabilitySemanticOutcome::Dynamic,
        ReachabilitySemanticOutcome::Unsupported,
        ReachabilitySemanticOutcome::InstrumentFailure,
    ] {
        assert!(!truth.may_carry_value());
        assert!(!truth.is_exact());
    }
    assert!(
        ReachabilitySemanticOutcome::Partial {
            limitations: vec![ReachabilityClaimLimitation::DynamicBoundary],
        }
        .may_carry_value()
    );
    assert!(
        !ReachabilitySemanticOutcome::Partial {
            limitations: vec![ReachabilityClaimLimitation::DynamicBoundary],
        }
        .is_exact()
    );

    // Claim-limitation variants stay distinct and ordered.
    let runtime = ReachabilityFactFamilyId::new("runtime-edges")?;
    let activation = ReachabilityFactFamilyId::new("activation-roots")?;
    let limitations = [
        ReachabilityClaimLimitation::MissingFactFamily(runtime.clone()),
        ReachabilityClaimLimitation::UnsupportedFamily(activation.clone()),
        ReachabilityClaimLimitation::PartialDenominator,
        ReachabilityClaimLimitation::DynamicBoundary,
        ReachabilityClaimLimitation::TerminalStage(ReachabilityStageId::new("closure")?),
        ReachabilityClaimLimitation::BoundedComputation,
    ];
    assert_eq!(limitations[0], ReachabilityClaimLimitation::MissingFactFamily(runtime.clone()));
    assert_ne!(limitations[2], ReachabilityClaimLimitation::MissingFactFamily(runtime.clone()));
    assert_eq!(limitations[1], ReachabilityClaimLimitation::UnsupportedFamily(activation));

    // Fact-family statuses: only Complete supports an exact claim.
    for status in [
        ReachabilityFactFamilyStatus::Complete,
        ReachabilityFactFamilyStatus::Partial,
        ReachabilityFactFamilyStatus::Missing,
        ReachabilityFactFamilyStatus::Unsupported,
        ReachabilityFactFamilyStatus::Stale,
    ] {
        assert_eq!(status.is_complete(), status == ReachabilityFactFamilyStatus::Complete);
    }

    // Work-path targets stay distinct.
    let targets = [
        ReachabilityWorkPathTarget::GraphInput,
        ReachabilityWorkPathTarget::ComponentGraph,
        ReachabilityWorkPathTarget::Closure,
        ReachabilityWorkPathTarget::QueryProjection,
        ReachabilityWorkPathTarget::DiagnosticProjection,
        ReachabilityWorkPathTarget::ResultReuse,
    ];
    for (index, target) in targets.iter().enumerate() {
        assert!(!matches!(
            target,
            ReachabilityWorkPathTarget::DiagnosticProjection if index != 4
        ));
    }
    assert_eq!(targets[4], ReachabilityWorkPathTarget::DiagnosticProjection);

    // Deadline observations are distinct from cancellation and supersession.
    let deadline = ReachabilityTerminalObservation::DeadlineExceeded {
        deadline_profile: ReachabilitySubjectIdentity::new(
            ReachabilitySubjectIdentityKind::ExternalControl,
            "deadline-profile-3",
            None,
        )?,
    };
    assert!(deadline.is_deadline());
    assert!(!deadline.is_cancellation());
    assert!(!deadline.is_supersession());

    // Product failures are bounded terminal outcomes with receipts.
    let mut tracker = tracked()?;
    tracker.note_instrument_evidence(instrument()?);
    let stage = ReachabilityStageId::new("classification")?;
    let failure = ReachabilityOperationOutcome::<Vec<String>>::ProductFailure {
        stage: stage.clone(),
        cause: String::from("bounded cause"),
        work_receipt: tracker.finish(),
    };
    assert!(failure.is_execution_terminal());
    assert!(!failure.may_claim_exact());
    assert_eq!(failure.work_receipt().completed_stages().len(), 0);

    // Stage limitation records expose their stage and limitation directly.
    let record = ReachabilityStageLimitation {
        stage: stage.clone(),
        limitation: ReachabilityClaimLimitation::TerminalStage(stage.clone()),
    };
    assert_eq!(record.stage.as_str(), "classification");
    assert_eq!(
        record.limitation,
        ReachabilityClaimLimitation::TerminalStage(ReachabilityStageId::new("classification")?)
    );
    Ok(())
}
