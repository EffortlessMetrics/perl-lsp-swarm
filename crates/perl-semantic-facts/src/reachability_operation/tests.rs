//! Falsifier suite for the reachability operation contract (#11553).
//!
//! Each test pins one mandatory distinction from the issue: terminals never
//! surface as legitimate empty or exact unreachable, classification is
//! total, budget semantics are monotone with checked arithmetic, receipts
//! are input-order deterministic, and bounded views cannot conflate
//! presentation truncation with semantic truncation.

use super::*;
use std::collections::BTreeMap;
use std::error::Error;

fn contract_error<T>(
    result: Result<T, ReachabilityContractError>,
) -> Result<ReachabilityContractError, Box<dyn Error>> {
    match result {
        Err(error) => Ok(error),
        Ok(_) => Err("expected a contract error, got success".into()),
    }
}

/// Compile-time totality proof: every identity kind is classified and no
/// unclassified fallback arm exists.
fn closed_classification(kind: ReachabilitySubjectIdentityKind) -> u8 {
    match kind {
        ReachabilitySubjectIdentityKind::WorkspaceSnapshot => 0,
        ReachabilitySubjectIdentityKind::Project => 1,
        ReachabilitySubjectIdentityKind::Root => 2,
        ReachabilitySubjectIdentityKind::ConfigurationProfile => 3,
        ReachabilitySubjectIdentityKind::Environment => 4,
        ReachabilitySubjectIdentityKind::SourceDocumentInstance => 5,
        ReachabilitySubjectIdentityKind::FactFamilySupport => 6,
        ReachabilitySubjectIdentityKind::SemanticOutcomeSchema => 7,
        ReachabilitySubjectIdentityKind::WorkBudgetProfile => 8,
        ReachabilitySubjectIdentityKind::ExternalControl => 9,
        ReachabilitySubjectIdentityKind::Instrument => 10,
        ReachabilitySubjectIdentityKind::StageOutput => 11,
    }
}

struct ScriptedControl(Option<ReachabilityTerminalObservation>);

impl ScriptedControl {
    fn cancelling() -> Result<Self, ReachabilityContractError> {
        Ok(Self(Some(ReachabilityTerminalObservation::Cancelled {
            control_identity: ReachabilitySubjectIdentity::new(
                ReachabilitySubjectIdentityKind::ExternalControl,
                "request-17",
                None,
            )?,
        })))
    }

    fn expiring() -> Result<Self, ReachabilityContractError> {
        Ok(Self(Some(ReachabilityTerminalObservation::DeadlineExceeded {
            deadline_profile: ReachabilitySubjectIdentity::new(
                ReachabilitySubjectIdentityKind::ExternalControl,
                "deadline-profile-3",
                None,
            )?,
        })))
    }

    fn superseding() -> Result<Self, ReachabilityContractError> {
        Ok(Self(Some(ReachabilityTerminalObservation::Superseded {
            expected: snapshot_identity("snapshot-a", "gen-1")?,
            observed: snapshot_identity("snapshot-a", "gen-2")?,
        })))
    }
}

impl ReachabilityOperationControl for ScriptedControl {
    fn poll(
        &self,
        _subject: &ReachabilityOperationSubject,
    ) -> Option<ReachabilityTerminalObservation> {
        self.0.clone()
    }
}

fn snapshot_identity(
    value: &str,
    generation: &str,
) -> Result<ReachabilitySubjectIdentity, ReachabilityContractError> {
    ReachabilitySubjectIdentity::new(
        ReachabilitySubjectIdentityKind::WorkspaceSnapshot,
        value,
        Some(generation.to_string()),
    )
}

fn instrument_identity() -> Result<ReachabilitySubjectIdentity, ReachabilityContractError> {
    ReachabilitySubjectIdentity::new(
        ReachabilitySubjectIdentityKind::Instrument,
        "work-meter-1",
        None,
    )
}

fn subject() -> Result<ReachabilityOperationSubject, ReachabilityContractError> {
    ReachabilityOperationSubject::new(
        ReachabilityOperationId::new("op-1")?,
        ReachabilityOperationKind::SccCondensation,
        vec![
            snapshot_identity("snapshot-a", "gen-1")?,
            ReachabilitySubjectIdentity::new(
                ReachabilitySubjectIdentityKind::Project,
                "project-1",
                None,
            )?,
            ReachabilitySubjectIdentity::new(
                ReachabilitySubjectIdentityKind::WorkBudgetProfile,
                "test-profile",
                None,
            )?,
        ],
        ReachabilityProfileId::new("test-profile")?,
    )
}

fn budget(
    dimension: ReachabilityWorkDimension,
    limit: u64,
) -> Result<ReachabilityWorkBudget, ReachabilityContractError> {
    ReachabilityWorkBudget::for_tests(
        ReachabilityProfileId::new("test-profile")?,
        ReachabilityOperationKind::SccCondensation,
        dimension,
        limit,
    )
}

fn new_tracker(
    dimension: ReachabilityWorkDimension,
    limit: u64,
) -> Result<ReachabilityWorkTracker, ReachabilityContractError> {
    ReachabilityWorkTracker::new(subject()?, budget(dimension, limit)?)
}

fn complete_ledger() -> ReachabilityFactFamilyLedger {
    let mut families = BTreeMap::new();
    if let Ok(runtime) = ReachabilityFactFamilyId::new("runtime-edges") {
        families.insert(runtime, ReachabilityFactFamilyStatus::Complete);
    }
    ReachabilityFactFamilyLedger::new(families)
}

fn required_family() -> Option<ReachabilityFactFamilyId> {
    ReachabilityFactFamilyId::new("runtime-edges").ok()
}

#[test]
fn operation_kind_classification_fails_closed_on_free_form_strings() {
    assert!(ReachabilityOperationKind::parse("ad-hoc-scan").is_err());
    assert!(ReachabilityOperationKind::parse("").is_err());
    for kind in [
        ReachabilityOperationKind::GraphAdmission,
        ReachabilityOperationKind::SccCondensation,
        ReachabilityOperationKind::ProductionClosure,
        ReachabilityOperationKind::TestClosure,
        ReachabilityOperationKind::Classification,
        ReachabilityOperationKind::EntityQuery,
        ReachabilityOperationKind::SourcePartition,
        ReachabilityOperationKind::BoundedExplanation,
        ReachabilityOperationKind::PolicyProjection,
        ReachabilityOperationKind::DiagnosticCandidateComposition,
        ReachabilityOperationKind::DiagnosticTransportProjection,
        ReachabilityOperationKind::ResultReuseRevalidation,
        ReachabilityOperationKind::SemanticProof,
        ReachabilityOperationKind::ExactProcessProof,
    ] {
        assert_eq!(ReachabilityOperationKind::parse(kind.as_str()), Ok(kind));
    }
}

#[test]
fn work_dimension_registry_fails_closed_and_has_no_wall_clock_dimension() {
    assert!(ReachabilityWorkDimension::parse("elapsed-ms").is_err());
    assert!(ReachabilityWorkDimension::parse("wall-clock-seconds").is_err());
    assert!(ReachabilityWorkDimension::parse("host-timeout").is_err());
    let names = [
        ReachabilityWorkDimension::WorkspaceSnapshotsCaptured,
        ReachabilityWorkDimension::NodesAdmitted,
        ReachabilityWorkDimension::EdgesAdmitted,
        ReachabilityWorkDimension::SccNodesVisited,
        ReachabilityWorkDimension::SccStackOperations,
        ReachabilityWorkDimension::ComponentsFormed,
        ReachabilityWorkDimension::CondensedEdgesConstructed,
        ReachabilityWorkDimension::ProductionClosureEdgesTraversed,
        ReachabilityWorkDimension::TestClosureEdgesTraversed,
        ReachabilityWorkDimension::ClassificationRows,
        ReachabilityWorkDimension::EntityQueries,
        ReachabilityWorkDimension::ExplanationPaths,
        ReachabilityWorkDimension::DiagnosticItems,
        ReachabilityWorkDimension::TransportChunks,
        ReachabilityWorkDimension::SerializedOutputBytes,
        ReachabilityWorkDimension::ValidatedReuseHits,
        ReachabilityWorkDimension::WorkAfterEligibilityLost,
    ];
    for dimension in names {
        let name = dimension.as_str();
        assert!(!name.contains("time"), "wall time is not a work dimension: {name}");
        assert!(!name.contains("elapsed"), "elapsed time is not a work dimension: {name}");
        assert_eq!(ReachabilityWorkDimension::parse(name), Ok(dimension));
    }
}

#[test]
fn subject_classification_is_total_with_no_untyped_fallback()
-> Result<(), ReachabilityContractError> {
    // Every identity kind is one closed variant; a compile-time exhaustive
    // match makes an unclassified fallback impossible.
    for kind in [
        ReachabilitySubjectIdentityKind::WorkspaceSnapshot,
        ReachabilitySubjectIdentityKind::Project,
        ReachabilitySubjectIdentityKind::Root,
        ReachabilitySubjectIdentityKind::ConfigurationProfile,
        ReachabilitySubjectIdentityKind::Environment,
        ReachabilitySubjectIdentityKind::SourceDocumentInstance,
        ReachabilitySubjectIdentityKind::FactFamilySupport,
        ReachabilitySubjectIdentityKind::SemanticOutcomeSchema,
        ReachabilitySubjectIdentityKind::WorkBudgetProfile,
        ReachabilitySubjectIdentityKind::ExternalControl,
        ReachabilitySubjectIdentityKind::Instrument,
        ReachabilitySubjectIdentityKind::StageOutput,
    ] {
        let identity = ReachabilitySubjectIdentity::new(kind, "opaque-value", None)?;
        assert_eq!(identity.kind(), kind);
        assert!(ReachabilitySubjectIdentity::new(kind, "", None).is_err());
        assert!(
            ReachabilitySubjectIdentity::new(kind, "opaque-value", Some(String::new())).is_err()
        );
        assert!(closed_classification(kind) < 12);
    }
    // No URI, path, display-name, timestamp, or thread-id kind exists: the
    // closed set above is the whole classification.
    Ok(())
}

#[test]
fn subject_rejects_conflicting_authority_and_replaces_nothing()
-> Result<(), ReachabilityContractError> {
    let conflicting = ReachabilityOperationSubject::new(
        ReachabilityOperationId::new("op-2")?,
        ReachabilityOperationKind::GraphAdmission,
        vec![snapshot_identity("snapshot-a", "gen-1")?, snapshot_identity("snapshot-b", "gen-1")?],
        ReachabilityProfileId::new("test-profile")?,
    );
    assert!(conflicting.is_err());

    let mut operation = subject()?;
    operation.append_stage_output(
        ReachabilityStageId::new("graph-admission")?,
        ReachabilitySubjectIdentity::new(
            ReachabilitySubjectIdentityKind::StageOutput,
            "graph-input-1",
            None,
        )?,
    );
    operation.append_stage_output(
        ReachabilityStageId::new("scc-condensation")?,
        ReachabilitySubjectIdentity::new(
            ReachabilitySubjectIdentityKind::StageOutput,
            "component-graph-1",
            None,
        )?,
    );
    let outputs: Vec<&str> =
        operation.stage_outputs().iter().map(|output| output.output().as_str()).collect();
    assert_eq!(outputs, ["graph-input-1", "component-graph-1"]);
    // Upstream snapshot authority survives stage appends unchanged.
    assert!(operation.authority_matches(
        ReachabilitySubjectIdentityKind::WorkspaceSnapshot,
        Some(&snapshot_identity("snapshot-a", "gen-1")?)
    ));
    assert!(!operation.authority_matches(
        ReachabilitySubjectIdentityKind::WorkspaceSnapshot,
        Some(&snapshot_identity("snapshot-a", "gen-2")?)
    ));
    assert!(!operation.authority_matches(ReachabilitySubjectIdentityKind::WorkspaceSnapshot, None));
    Ok(())
}

#[test]
fn unlimited_dimension_requires_reviewed_reason_and_safety_bound()
-> Result<(), ReachabilityContractError> {
    let dimension = ReachabilityWorkDimension::NodesAdmitted;
    assert!(
        ReachabilityUnlimitedJustification::new(dimension, "", 10_000).is_err(),
        "an unlimited dimension requires a reviewed reason"
    );
    assert!(
        ReachabilityUnlimitedJustification::new(dimension, "reviewed-2026-08", 0).is_err(),
        "an unlimited dimension requires a higher-level safety bound"
    );
    let justification =
        ReachabilityUnlimitedJustification::new(dimension, "reviewed-2026-08", 10_000)?;
    assert_eq!(justification.safety_bound(), 10_000);

    let mut unlimited = BTreeMap::new();
    unlimited.insert(dimension, justification);
    let profile = ReachabilityProfileId::new("unlimited-profile")?;
    let operation_budget = ReachabilityWorkBudget::new(
        profile,
        vec![ReachabilityOperationKind::Classification],
        BTreeMap::new(),
        unlimited,
    )?;
    assert_eq!(
        operation_budget.limit_for(dimension),
        Some(ReachabilityDimensionLimit::Unlimited { safety_bound: 10_000 })
    );
    // The safety bound is still a limit: exceeding it exhausts.
    let mut tracker =
        ReachabilityWorkTracker::new(subject_for_classification()?, operation_budget)?;
    assert!(tracker.charge(dimension, 10_000).is_ok());
    assert!(matches!(tracker.charge(dimension, 1), Err(ReachabilityChargeError::Exhausted { .. })));
    Ok(())
}

fn subject_for_classification() -> Result<ReachabilityOperationSubject, ReachabilityContractError> {
    ReachabilityOperationSubject::new(
        ReachabilityOperationId::new("op-3")?,
        ReachabilityOperationKind::Classification,
        vec![
            snapshot_identity("snapshot-a", "gen-1")?,
            ReachabilitySubjectIdentity::new(
                ReachabilitySubjectIdentityKind::WorkBudgetProfile,
                "unlimited-profile",
                None,
            )?,
        ],
        ReachabilityProfileId::new("unlimited-profile")?,
    )
}

#[test]
fn budget_requires_every_declared_dimension() -> Result<(), Box<dyn Error>> {
    let operation_budget = budget(ReachabilityWorkDimension::NodesAdmitted, 100)?;
    assert!(
        operation_budget.validate_requirements(&[ReachabilityWorkDimension::NodesAdmitted]).is_ok()
    );
    assert_eq!(
        contract_error(
            operation_budget.validate_requirements(&[ReachabilityWorkDimension::EdgesAdmitted])
        )?,
        ReachabilityContractError::MissingRequiredDimension {
            dimension: ReachabilityWorkDimension::EdgesAdmitted,
        }
    );
    Ok(())
}

#[test]
fn budget_limit_boundaries_are_limit_minus_one_at_limit_and_over()
-> Result<(), ReachabilityContractError> {
    let dimension = ReachabilityWorkDimension::NodesAdmitted;
    let mut tracker = new_tracker(dimension, 10)?;
    assert!(tracker.charge(dimension, 9).is_ok());
    assert!(tracker.charge(dimension, 1).is_ok());
    assert!(tracker.terminal().is_none());
    assert!(matches!(
        tracker.charge(dimension, 1),
        Err(ReachabilityChargeError::Exhausted { limit: 10, charged: 10, .. })
    ));
    // A refused charge never partially applies.
    assert_eq!(tracker.finish().charged().get(&dimension), Some(&10));
    Ok(())
}

#[test]
fn budget_exhaustion_is_a_typed_terminal_never_empty_or_exact() -> Result<(), Box<dyn Error>> {
    let dimension = ReachabilityWorkDimension::SccNodesVisited;
    let mut tracker = new_tracker(dimension, 5)?;
    tracker.charge(dimension, 5)?;
    let exhausted = match tracker.charge(dimension, 1) {
        Err(error) => error,
        Ok(()) => return Err("charging beyond the limit must fail".into()),
    };
    assert!(matches!(exhausted, ReachabilityChargeError::Exhausted { .. }));
    let terminal = tracker.terminal().cloned().ok_or("exhaustion must latch a terminal")?;
    assert!(matches!(terminal, ReachabilityTerminalState::ResourceExhausted { .. }));
    assert!(!terminal.is_cancellation());
    let receipt = tracker.finish();
    let outcome: ReachabilityOperationOutcome<String> =
        ReachabilityOperationOutcome::terminal_from(
            &terminal,
            ReachabilityStageId::new("scc-traversal")?,
            receipt,
        )?;
    assert!(matches!(outcome, ReachabilityOperationOutcome::ResourceExhausted { .. }));
    assert!(outcome.is_execution_terminal());
    assert!(!outcome.may_claim_exact());
    assert!(outcome.retained_partial_value().is_none());
    Ok(())
}

#[test]
fn cancellation_deadline_and_supersession_are_distinct_terminals() -> Result<(), Box<dyn Error>> {
    let stage = ReachabilityStageId::new("graph-admission")?;
    let cases: [(ScriptedControl, &str); 3] = [
        (ScriptedControl::cancelling()?, "cancelled"),
        (ScriptedControl::expiring()?, "deadline"),
        (ScriptedControl::superseding()?, "superseded"),
    ];
    for (control, label) in cases {
        let mut tracker = new_tracker(ReachabilityWorkDimension::NodesAdmitted, 100)?;
        let observed = tracker
            .poll_checkpoint(&stage, &control)
            .ok_or("a scripted terminal must be observed")?;
        let terminal = match observed {
            ReachabilityTerminalState::External(observation) => observation,
            other => return Err(format!("unexpected terminal for {label}: {other:?}").into()),
        };
        assert_eq!(terminal.is_cancellation(), label == "cancelled");
        assert_eq!(terminal.is_deadline(), label == "deadline");
        assert_eq!(terminal.is_supersession(), label == "superseded");
        let receipt = tracker.finish();
        let outcome: ReachabilityOperationOutcome<String> =
            ReachabilityOperationOutcome::terminal_from(
                &ReachabilityTerminalState::External(terminal),
                stage.clone(),
                receipt,
            )?;
        match (&outcome, label) {
            (ReachabilityOperationOutcome::Cancelled { .. }, "cancelled")
            | (ReachabilityOperationOutcome::DeadlineExceeded { .. }, "deadline")
            | (ReachabilityOperationOutcome::SupersededOrStale { .. }, "superseded") => {}
            _ => return Err(format!("terminal mapped to the wrong variant for {label}").into()),
        }
        assert!(!outcome.may_claim_exact());
    }
    Ok(())
}

#[test]
fn checked_arithmetic_overflow_is_typed_instrument_failure_never_wraparound()
-> Result<(), Box<dyn Error>> {
    let dimension = ReachabilityWorkDimension::NodesAdmitted;
    // Budget a different dimension so the charged dimension is unconstrained
    // by a bounded limit but still guarded by checked arithmetic.
    let mut tracker = new_tracker(ReachabilityWorkDimension::EdgesAdmitted, 100)?;
    assert!(tracker.charge(dimension, u64::MAX).is_ok());
    assert!(matches!(
        tracker.charge(dimension, 1),
        Err(ReachabilityChargeError::CounterOverflow { .. })
    ));
    let terminal = tracker.terminal().cloned().ok_or("overflow must latch a terminal")?;
    assert!(matches!(terminal, ReachabilityTerminalState::CounterOverflow { .. }));
    let receipt = tracker.finish();
    assert_eq!(receipt.charged().get(&dimension), Some(&u64::MAX));
    let outcome: ReachabilityOperationOutcome<String> =
        ReachabilityOperationOutcome::terminal_from(
            &terminal,
            ReachabilityStageId::new("scc-traversal")?,
            receipt,
        )?;
    assert!(matches!(outcome, ReachabilityOperationOutcome::InstrumentFailure { .. }));
    assert!(!outcome.may_claim_exact());
    Ok(())
}

#[test]
fn post_terminal_work_is_charged_but_can_never_publish() -> Result<(), Box<dyn Error>> {
    let dimension = ReachabilityWorkDimension::NodesAdmitted;
    let mut tracker = new_tracker(dimension, 5)?;
    tracker.charge(dimension, 5)?;
    assert!(tracker.charge(dimension, 1).is_err());
    // Non-interruptible stale work may finish privately on another
    // unconstrained dimension.
    assert!(tracker.charge(ReachabilityWorkDimension::EdgesAdmitted, 7).is_ok());
    let receipt = tracker.finish();
    assert_eq!(receipt.work_after_eligibility_lost(), 7);
    assert!(receipt.terminal().is_some());
    // A complete claim over a terminal receipt is impossible to construct.
    assert!(
        ReachabilityOperationOutcome::<String>::complete(
            ReachabilitySemanticOutcome::Complete,
            Some(String::from("value")),
            finish_clean_receipt()?,
        )
        .is_ok()
    );
    let terminal_receipt = {
        let mut terminal_tracker = new_tracker(dimension, 5)?;
        terminal_tracker.charge(dimension, 5)?;
        let _ = terminal_tracker.charge(dimension, 1);
        terminal_tracker.charge(ReachabilityWorkDimension::EdgesAdmitted, 7)?;
        terminal_tracker.finish()
    };
    assert_eq!(
        contract_error(ReachabilityOperationOutcome::<String>::complete(
            ReachabilitySemanticOutcome::Complete,
            Some(String::from("value")),
            terminal_receipt,
        ))?,
        ReachabilityContractError::ClaimConflictsWithLimitations
    );
    Ok(())
}

fn finish_clean_receipt() -> Result<ReachabilityWorkReceipt, Box<dyn Error>> {
    let mut tracker = new_tracker(ReachabilityWorkDimension::NodesAdmitted, 100)?;
    tracker.note_instrument_evidence(instrument_identity()?);
    Ok(tracker.finish())
}

#[test]
fn semantic_consistency_laws_fail_closed() -> Result<(), Box<dyn Error>> {
    let receipt = finish_clean_receipt()?;
    assert_eq!(
        contract_error(ReachabilityOperationOutcome::<String>::complete(
            ReachabilitySemanticOutcome::Complete,
            None,
            receipt.clone(),
        ))?,
        ReachabilityContractError::CompleteWithoutValue
    );
    assert_eq!(
        contract_error(ReachabilityOperationOutcome::<String>::complete(
            ReachabilitySemanticOutcome::LegitimateEmpty,
            Some(String::from("value")),
            receipt.clone(),
        ))?,
        ReachabilityContractError::EmptyWithRetainedValue
    );
    assert_eq!(
        contract_error(ReachabilityOperationOutcome::<String>::complete(
            ReachabilitySemanticOutcome::Partial { limitations: Vec::new() },
            Some(String::from("value")),
            receipt.clone(),
        ))?,
        ReachabilityContractError::PartialWithoutLimitation
    );
    assert_eq!(
        contract_error(ReachabilityOperationOutcome::<String>::complete(
            ReachabilitySemanticOutcome::Stale,
            Some(String::from("value")),
            receipt,
        ))?,
        ReachabilityContractError::ValueWithNonValuedTruth
    );
    Ok(())
}

#[test]
fn partial_value_is_retained_for_diagnostics_only() -> Result<(), Box<dyn Error>> {
    let receipt = finish_clean_receipt()?;
    let outcome = ReachabilityOperationOutcome::complete(
        ReachabilitySemanticOutcome::Partial {
            limitations: vec![ReachabilityClaimLimitation::PartialDenominator],
        },
        Some(String::from("partial-rows")),
        receipt,
    )?;
    assert_eq!(outcome.retained_partial_value(), Some(&String::from("partial-rows")));
    assert!(!outcome.may_claim_exact());
    Ok(())
}

#[test]
fn later_stage_cannot_erase_an_upstream_limitation() -> Result<(), Box<dyn Error>> {
    let family = ReachabilityFactFamilyId::new("activation-edges")?;
    let mut tracker = new_tracker(ReachabilityWorkDimension::NodesAdmitted, 100)?;
    tracker.complete_stage(
        ReachabilityStageId::new("graph-admission")?,
        None,
        vec![ReachabilityClaimLimitation::MissingFactFamily(family.clone())],
    );
    tracker.complete_stage(ReachabilityStageId::new("scc-condensation")?, None, Vec::new());
    let receipt = tracker.finish();
    assert_eq!(receipt.stage_limitations().len(), 1);
    // One complete later stage cannot promote the operation to exact.
    assert_eq!(
        contract_error(ReachabilityOperationOutcome::<String>::complete(
            ReachabilitySemanticOutcome::Complete,
            Some(String::from("value")),
            receipt.clone(),
        ))?,
        ReachabilityContractError::ClaimConflictsWithLimitations
    );
    let partial = ReachabilityOperationOutcome::<String>::complete(
        ReachabilitySemanticOutcome::Partial {
            limitations: vec![ReachabilityClaimLimitation::MissingFactFamily(family)],
        },
        Some(String::from("value")),
        receipt,
    )?;
    assert!(!partial.may_claim_exact());
    Ok(())
}

#[test]
fn missing_instrument_is_not_zero_work() -> Result<(), Box<dyn Error>> {
    let mut tracker = new_tracker(ReachabilityWorkDimension::NodesAdmitted, 100)?;
    let receipt_without_instrument = tracker.clone().finish();
    assert!(!receipt_without_instrument.instrument_evidence_complete());
    assert!(receipt_without_instrument.instrument_identity().is_none());
    assert_eq!(
        contract_error(ReachabilityOperationOutcome::<String>::complete(
            ReachabilitySemanticOutcome::Complete,
            Some(String::from("value")),
            receipt_without_instrument,
        ))?,
        ReachabilityContractError::MissingInstrumentEvidence
    );
    tracker.note_instrument_evidence(instrument_identity()?);
    let receipt = tracker.finish();
    assert!(receipt.instrument_evidence_complete());
    let outcome = ReachabilityOperationOutcome::<String>::complete(
        ReachabilitySemanticOutcome::Complete,
        Some(String::from("value")),
        receipt,
    )?;
    assert!(outcome.may_claim_exact());
    Ok(())
}

#[test]
fn publication_eligibility_is_exact_subject_and_fail_closed() -> Result<(), Box<dyn Error>> {
    let family = required_family().ok_or("family id")?;
    let stage = ReachabilityStageId::new("graph-admission")?;
    let mut tracker = new_tracker(ReachabilityWorkDimension::NodesAdmitted, 100)?;
    tracker.note_instrument_evidence(instrument_identity()?);
    tracker.complete_stage(
        stage.clone(),
        Some(ReachabilitySubjectIdentity::new(
            ReachabilitySubjectIdentityKind::StageOutput,
            "graph-input-1",
            None,
        )?),
        Vec::new(),
    );
    let receipt = tracker.finish();
    let outcome = ReachabilityOperationOutcome::<String>::complete(
        ReachabilitySemanticOutcome::Complete,
        Some(String::from("graph")),
        receipt,
    )?;
    let operation_subject = subject()?;

    let eligible = ReachabilityPublicationEligibility::evaluate(
        &operation_subject,
        Some(&snapshot_identity("snapshot-a", "gen-1")?),
        ReachabilitySubjectIdentityKind::WorkspaceSnapshot,
        std::slice::from_ref(&stage),
        &outcome,
        &complete_ledger(),
        std::slice::from_ref(&family),
    );
    assert!(eligible.is_eligible(), "eligible case must pass: {:?}", eligible.reasons());

    let superseded = ReachabilityPublicationEligibility::evaluate(
        &operation_subject,
        Some(&snapshot_identity("snapshot-a", "gen-2")?),
        ReachabilitySubjectIdentityKind::WorkspaceSnapshot,
        std::slice::from_ref(&stage),
        &outcome,
        &complete_ledger(),
        std::slice::from_ref(&family),
    );
    assert!(!superseded.is_eligible());
    assert!(superseded.reasons().contains(&ReachabilityIneligibilityReason::SubjectSuperseded));

    let missing_authority = ReachabilityPublicationEligibility::evaluate(
        &operation_subject,
        None,
        ReachabilitySubjectIdentityKind::WorkspaceSnapshot,
        std::slice::from_ref(&stage),
        &outcome,
        &complete_ledger(),
        std::slice::from_ref(&family),
    );
    assert!(!missing_authority.is_eligible());

    let incomplete_stage = ReachabilityPublicationEligibility::evaluate(
        &operation_subject,
        Some(&snapshot_identity("snapshot-a", "gen-1")?),
        ReachabilitySubjectIdentityKind::WorkspaceSnapshot,
        &[stage.clone(), ReachabilityStageId::new("closure")?],
        &outcome,
        &complete_ledger(),
        std::slice::from_ref(&family),
    );
    assert!(incomplete_stage.reasons().contains(
        &ReachabilityIneligibilityReason::RequiredStageIncomplete(ReachabilityStageId::new(
            "closure"
        )?)
    ));

    let mut partial_tracker = new_tracker(ReachabilityWorkDimension::NodesAdmitted, 100)?;
    partial_tracker.note_instrument_evidence(instrument_identity()?);
    let partial_outcome = ReachabilityOperationOutcome::<String>::complete(
        ReachabilitySemanticOutcome::Partial {
            limitations: vec![ReachabilityClaimLimitation::PartialDenominator],
        },
        Some(String::from("partial")),
        partial_tracker.finish(),
    )?;
    let partial_verdict = ReachabilityPublicationEligibility::evaluate(
        &operation_subject,
        Some(&snapshot_identity("snapshot-a", "gen-1")?),
        ReachabilitySubjectIdentityKind::WorkspaceSnapshot,
        std::slice::from_ref(&stage),
        &partial_outcome,
        &complete_ledger(),
        std::slice::from_ref(&family),
    );
    assert!(
        partial_verdict.reasons().contains(&ReachabilityIneligibilityReason::ClaimNotSupported)
    );

    let mut missing_family_ledger = BTreeMap::new();
    missing_family_ledger.insert(family.clone(), ReachabilityFactFamilyStatus::Missing);
    let missing_denominator = ReachabilityPublicationEligibility::evaluate(
        &operation_subject,
        Some(&snapshot_identity("snapshot-a", "gen-1")?),
        ReachabilitySubjectIdentityKind::WorkspaceSnapshot,
        std::slice::from_ref(&stage),
        &outcome,
        &ReachabilityFactFamilyLedger::new(missing_family_ledger),
        &[family],
    );
    assert!(
        missing_denominator.reasons().iter().any(|reason| matches!(
            reason,
            ReachabilityIneligibilityReason::DenominatorIncomplete(_)
        ))
    );

    let mut terminal_tracker = new_tracker(ReachabilityWorkDimension::NodesAdmitted, 5)?;
    terminal_tracker.charge(ReachabilityWorkDimension::NodesAdmitted, 5)?;
    let _ = terminal_tracker.charge(ReachabilityWorkDimension::NodesAdmitted, 1);
    terminal_tracker.note_instrument_evidence(instrument_identity()?);
    let terminal = terminal_tracker.terminal().cloned().ok_or("terminal")?;
    let terminal_outcome = ReachabilityOperationOutcome::<String>::terminal_from(
        &terminal,
        stage.clone(),
        terminal_tracker.finish(),
    )?;
    let terminal_verdict = ReachabilityPublicationEligibility::evaluate(
        &operation_subject,
        Some(&snapshot_identity("snapshot-a", "gen-1")?),
        ReachabilitySubjectIdentityKind::WorkspaceSnapshot,
        std::slice::from_ref(&stage),
        &terminal_outcome,
        &complete_ledger(),
        &[ReachabilityFactFamilyId::new("runtime-edges")?],
    );
    assert!(terminal_verdict.reasons().contains(&ReachabilityIneligibilityReason::TerminalState));

    let mut no_instrument_tracker = new_tracker(ReachabilityWorkDimension::NodesAdmitted, 100)?;
    no_instrument_tracker.complete_stage(stage.clone(), None, Vec::new());
    let no_instrument_outcome = ReachabilityOperationOutcome::<String>::complete(
        ReachabilitySemanticOutcome::Partial {
            limitations: vec![ReachabilityClaimLimitation::PartialDenominator],
        },
        None,
        no_instrument_tracker.finish(),
    )?;
    let no_instrument_verdict = ReachabilityPublicationEligibility::evaluate(
        &operation_subject,
        Some(&snapshot_identity("snapshot-a", "gen-1")?),
        ReachabilitySubjectIdentityKind::WorkspaceSnapshot,
        &[stage],
        &no_instrument_outcome,
        &complete_ledger(),
        &[ReachabilityFactFamilyId::new("runtime-edges")?],
    );
    assert!(
        no_instrument_verdict
            .reasons()
            .contains(&ReachabilityIneligibilityReason::InstrumentEvidenceIncomplete)
    );
    Ok(())
}

#[test]
fn legitimate_empty_requires_complete_denominator_and_clean_receipt() -> Result<(), Box<dyn Error>>
{
    let family = ReachabilityFactFamilyId::new("runtime-edges")?;
    let mut tracker = new_tracker(ReachabilityWorkDimension::NodesAdmitted, 100)?;
    tracker.note_instrument_evidence(instrument_identity()?);
    let receipt = tracker.finish();
    assert!(
        ReachabilityOperationOutcome::<Vec<String>>::complete(
            ReachabilitySemanticOutcome::LegitimateEmpty,
            None,
            receipt.clone(),
        )
        .is_ok()
    );
    let empty_outcome = ReachabilityOperationOutcome::<Vec<String>>::complete(
        ReachabilitySemanticOutcome::LegitimateEmpty,
        None,
        receipt,
    )?;
    assert!(empty_outcome.may_claim_exact());

    let operation_subject = subject()?;
    let mut missing = BTreeMap::new();
    missing.insert(family.clone(), ReachabilityFactFamilyStatus::Missing);
    let verdict = ReachabilityPublicationEligibility::evaluate(
        &operation_subject,
        Some(&snapshot_identity("snapshot-a", "gen-1")?),
        ReachabilitySubjectIdentityKind::WorkspaceSnapshot,
        &[],
        &empty_outcome,
        &ReachabilityFactFamilyLedger::new(missing),
        &[family],
    );
    assert!(!verdict.is_eligible());
    assert!(
        verdict.reasons().iter().any(|reason| matches!(
            reason,
            ReachabilityIneligibilityReason::DenominatorIncomplete(_)
        ))
    );
    Ok(())
}

#[test]
fn receipt_is_deterministic_under_input_order_permutation() -> Result<(), Box<dyn Error>> {
    let charge_plan = [
        (ReachabilityWorkDimension::NodesAdmitted, 3u64),
        (ReachabilityWorkDimension::EdgesAdmitted, 7),
        (ReachabilityWorkDimension::SccNodesVisited, 5),
        (ReachabilityWorkDimension::SccStackOperations, 11),
        (ReachabilityWorkDimension::ComponentsFormed, 2),
    ];
    let mut forward = new_tracker(ReachabilityWorkDimension::NodesAdmitted, 1000)?;
    let mut reverse = new_tracker(ReachabilityWorkDimension::NodesAdmitted, 1000)?;
    for (dimension, units) in charge_plan {
        assert!(forward.charge(dimension, units).is_ok());
    }
    for (dimension, units) in charge_plan.iter().rev() {
        assert!(reverse.charge(*dimension, *units).is_ok());
    }
    let forward_receipt = forward.finish();
    let reverse_receipt = reverse.finish();
    assert_eq!(forward_receipt, reverse_receipt);
    assert_eq!(serde_json::to_string(&forward_receipt)?, serde_json::to_string(&reverse_receipt)?);
    Ok(())
}

#[test]
fn tracker_rejects_a_budget_from_a_different_profile() -> Result<(), Box<dyn Error>> {
    let mismatched_budget = ReachabilityWorkBudget::for_tests(
        ReachabilityProfileId::new("another-profile")?,
        ReachabilityOperationKind::SccCondensation,
        ReachabilityWorkDimension::NodesAdmitted,
        100,
    )?;
    assert_eq!(
        contract_error(ReachabilityWorkTracker::new(subject()?, mismatched_budget))?,
        ReachabilityContractError::BudgetProfileMismatch
    );
    // The matching profile still starts cleanly.
    assert!(
        ReachabilityWorkTracker::new(
            subject()?,
            budget(ReachabilityWorkDimension::NodesAdmitted, 100)?
        )
        .is_ok()
    );
    Ok(())
}

#[test]
fn tracker_resets_across_repeated_operations() -> Result<(), Box<dyn Error>> {
    let dimension = ReachabilityWorkDimension::NodesAdmitted;
    let mut first = new_tracker(dimension, 5)?;
    first.charge(dimension, 5)?;
    let _ = first.charge(dimension, 1);
    let first_receipt = first.finish();
    assert!(first_receipt.terminal().is_some());

    let second = new_tracker(dimension, 5)?;
    let second_receipt = second.finish();
    assert!(second_receipt.charged().is_empty());
    assert!(second_receipt.terminal().is_none());
    assert!(!second_receipt.instrument_evidence_complete());
    Ok(())
}

#[test]
fn full_rebuild_is_not_reuse_without_the_declared_identity() -> Result<(), Box<dyn Error>> {
    let stage = ReachabilityStageId::new("graph-admission")?;
    let mut honest = new_tracker(ReachabilityWorkDimension::NodesAdmitted, 100)?;
    let undeclared = ReachabilitySubjectIdentity::new(
        ReachabilitySubjectIdentityKind::StageOutput,
        "graph-input-9",
        None,
    )?;
    assert!(matches!(
        honest.record_validated_reuse(
            stage.clone(),
            ReachabilityWorkPathTarget::GraphInput,
            undeclared
        ),
        Err(ReachabilityContractError::WorkHonesty(
            ReachabilityWorkHonestyError::ReuseWithoutDeclaredIdentity { .. }
        ))
    ));
    // The snapshot identity is the declared current identity a validated
    // reuse may cite.
    let snapshot = snapshot_identity("snapshot-a", "gen-1")?;
    assert!(
        honest
            .record_validated_reuse(
                stage.clone(),
                ReachabilityWorkPathTarget::ResultReuse,
                snapshot
            )
            .is_ok()
    );
    assert!(matches!(
        honest.record_full_construction(stage.clone(), ReachabilityWorkPathTarget::ResultReuse),
        Err(ReachabilityContractError::WorkHonesty(
            ReachabilityWorkHonestyError::FullConstructionAfterValidatedReuse { .. }
        ))
    ));
    let receipt = honest.finish();
    assert!(receipt.is_validated_reuse_of(&ReachabilityWorkPathTarget::ResultReuse));
    assert!(
        receipt
            .work_paths()
            .iter()
            .all(|path| path.reused_identity().is_some() || !path.is_validated_reuse())
    );
    Ok(())
}

#[test]
fn bounded_view_cannot_conflate_presentation_and_semantic_truncation() -> Result<(), Box<dyn Error>>
{
    let mut tracker = new_tracker(ReachabilityWorkDimension::NodesAdmitted, 100)?;
    tracker.note_instrument_evidence(instrument_identity()?);
    tracker.complete_stage(
        ReachabilityStageId::new("classification")?,
        Some(ReachabilitySubjectIdentity::new(
            ReachabilitySubjectIdentityKind::StageOutput,
            "classification-1",
            None,
        )?),
        Vec::new(),
    );
    let receipt = tracker.finish();
    let complete_outcome = ReachabilityOperationOutcome::<Vec<String>>::complete(
        ReachabilitySemanticOutcome::Complete,
        Some(vec![String::from("classification")]),
        receipt,
    )?;

    let mut partial_tracker = new_tracker(ReachabilityWorkDimension::NodesAdmitted, 100)?;
    partial_tracker.note_instrument_evidence(instrument_identity()?);
    let partial_outcome: ReachabilityOperationOutcome<Vec<String>> =
        ReachabilityOperationOutcome::complete(
            ReachabilitySemanticOutcome::Partial {
                limitations: vec![ReachabilityClaimLimitation::BoundedComputation],
            },
            Some(vec![String::from("partial")]),
            partial_tracker.finish(),
        )?;

    let result_identity = ReachabilitySubjectIdentity::new(
        ReachabilitySubjectIdentityKind::StageOutput,
        "classification-1",
        None,
    )?;
    let authority = snapshot_identity("snapshot-a", "gen-1")?;

    // A semantically truncated computation cannot mint the complete-result
    // token, so it cannot construct a view claiming a complete underlying.
    assert!(
        partial_outcome.bounded_view_source(result_identity.clone(), authority.clone()).is_none()
    );
    let source = complete_outcome
        .bounded_view_source(result_identity, authority)
        .ok_or("an exact complete outcome must mint the bounded-view source")?;

    // A truncated view must retain its reason.
    assert!(
        ReachabilityBoundedView::new(
            source.clone(),
            ReachableViewProfileId::new("explanation-v1")?,
            3,
            512,
            Some(10),
            None,
            true,
            None,
        )
        .is_err()
    );
    // An omitted count inconsistent with the known total must be refused.
    assert!(
        ReachabilityBoundedView::new(
            source.clone(),
            ReachableViewProfileId::new("explanation-v1")?,
            3,
            512,
            Some(10),
            Some(4),
            true,
            Some(String::from("explanation-item-limit")),
        )
        .is_err()
    );
    let view = ReachabilityBoundedView::new(
        source,
        ReachableViewProfileId::new("explanation-v1")?,
        3,
        512,
        Some(10),
        Some(7),
        true,
        Some(String::from("explanation-item-limit")),
    )?;
    assert!(view.truncated());
    assert_eq!(view.truncation_reason(), Some("explanation-item-limit"));
    assert_eq!(view.known_total(), Some(10));
    assert_eq!(view.omitted_count(), Some(7));
    // Proof/currentness fields are structural: the view cannot drop them.
    assert_eq!(view.underlying().currentness_authority().as_str(), "snapshot-a");
    assert_eq!(view.underlying().result_identity().as_str(), "classification-1");
    // And the complete underlying classification is unaffected by the
    // truncated presentation.
    assert!(complete_outcome.may_claim_exact());
    Ok(())
}

#[test]
fn outcome_serialization_round_trips() -> Result<(), Box<dyn Error>> {
    let mut tracker = new_tracker(ReachabilityWorkDimension::NodesAdmitted, 100)?;
    tracker.note_instrument_evidence(instrument_identity()?);
    let outcome = ReachabilityOperationOutcome::<String>::complete(
        ReachabilitySemanticOutcome::Complete,
        Some(String::from("graph")),
        tracker.finish(),
    )?;
    let serialized = serde_json::to_string(&outcome)?;
    let decoded: ReachabilityOperationOutcome<String> = serde_json::from_str(&serialized)?;
    assert_eq!(decoded, outcome);
    assert!(decoded.may_claim_exact());

    let mut terminal_tracker = new_tracker(ReachabilityWorkDimension::NodesAdmitted, 100)?;
    let cancelled = terminal_tracker
        .poll_checkpoint(&ReachabilityStageId::new("closure")?, &ScriptedControl::cancelling()?)
        .ok_or("cancellation must be observed")?;
    let terminal_outcome = ReachabilityOperationOutcome::<String>::terminal_from(
        &cancelled,
        ReachabilityStageId::new("closure")?,
        terminal_tracker.finish(),
    )?;
    let terminal_serialized = serde_json::to_string(&terminal_outcome)?;
    let terminal_decoded: ReachabilityOperationOutcome<String> =
        serde_json::from_str(&terminal_serialized)?;
    assert_eq!(terminal_decoded, terminal_outcome);
    assert!(!terminal_decoded.may_claim_exact());
    Ok(())
}

#[test]
fn architecture_fence_forbids_lsp_parser_graph_provider_and_scheduler_ownership() {
    let sources = [
        include_str!("mod.rs"),
        include_str!("subject.rs"),
        include_str!("budget.rs"),
        include_str!("tracker.rs"),
        include_str!("receipt.rs"),
        include_str!("outcome.rs"),
        include_str!("view.rs"),
    ];
    for source in sources {
        for forbidden in [
            "std::time::",
            "std::thread::",
            "tower_lsp",
            "lsp_types",
            "perl_parser",
            "perl_lsp",
            "perl_ast",
            "perl_workspace",
            "find_unused_symbols",
        ] {
            assert!(
                !source.contains(forbidden),
                "the reachability operation contract must not own {forbidden}"
            );
        }
    }
}

#[test]
fn execution_profile_contract_is_constructible_and_serializable() -> Result<(), Box<dyn Error>> {
    let profile = ReachabilityExecutionProfile::new(
        ReachabilityProfileId::new("interactive-liveness-v1")?,
        1,
        ReachabilityExecutionPurpose::Interactive,
        vec![ReachabilityOperationKind::ProductionClosure],
        vec![ReachabilityFactFamilyId::new("runtime-edges")?],
        ReachabilityCancellationPolling::AtDeclaredCheckpoints,
        ReachabilityRetentionLimits {
            max_explanation_items: Some(200),
            max_output_bytes: Some(64 * 1024),
        },
        "performance-authority-2026-08",
        vec![String::from("profile notes the interactive ceiling")],
    )?;
    assert_eq!(profile.profile_id().as_str(), "interactive-liveness-v1");
    assert_eq!(profile.version(), 1);
    assert_eq!(profile.purpose(), ReachabilityExecutionPurpose::Interactive);
    assert_eq!(profile.selected_operation_kinds(), &[ReachabilityOperationKind::ProductionClosure]);
    assert_eq!(profile.selected_fact_families()[0].as_str(), "runtime-edges");
    assert_eq!(
        profile.cancellation_polling(),
        ReachabilityCancellationPolling::AtDeclaredCheckpoints
    );
    assert_eq!(profile.retention().max_explanation_items, Some(200));
    assert_eq!(profile.retention().max_output_bytes, Some(64 * 1024));
    assert_eq!(profile.defaults_source(), "performance-authority-2026-08");
    assert_eq!(profile.limitations(), ["profile notes the interactive ceiling"]);

    // Validation fails closed on empty identity, zero version, and missing
    // kinds or defaults source.
    assert!(ReachabilityProfileId::new("").is_err());
    assert!(
        ReachabilityExecutionProfile::new(
            ReachabilityProfileId::new("p")?,
            0,
            ReachabilityExecutionPurpose::Proof,
            vec![ReachabilityOperationKind::Classification],
            Vec::new(),
            ReachabilityCancellationPolling::AtDeclaredCheckpoints,
            ReachabilityRetentionLimits { max_explanation_items: None, max_output_bytes: None },
            "authority",
            Vec::new(),
        )
        .is_err()
    );
    assert!(
        ReachabilityExecutionProfile::new(
            ReachabilityProfileId::new("p")?,
            1,
            ReachabilityExecutionPurpose::Batch,
            Vec::new(),
            Vec::new(),
            ReachabilityCancellationPolling::AtDeclaredCheckpoints,
            ReachabilityRetentionLimits { max_explanation_items: None, max_output_bytes: None },
            "authority",
            Vec::new(),
        )
        .is_err()
    );
    assert!(
        ReachabilityExecutionProfile::new(
            ReachabilityProfileId::new("p")?,
            1,
            ReachabilityExecutionPurpose::Batch,
            vec![ReachabilityOperationKind::Classification],
            Vec::new(),
            ReachabilityCancellationPolling::AtDeclaredCheckpoints,
            ReachabilityRetentionLimits { max_explanation_items: None, max_output_bytes: None },
            "",
            Vec::new(),
        )
        .is_err()
    );

    let serialized = serde_json::to_string(&profile)?;
    let decoded: ReachabilityExecutionProfile = serde_json::from_str(&serialized)?;
    assert_eq!(decoded, profile);
    Ok(())
}

#[test]
fn contract_surface_accessors_are_reachable_and_errors_render() -> Result<(), Box<dyn Error>> {
    // Budget surface.
    let justification = ReachabilityUnlimitedJustification::new(
        ReachabilityWorkDimension::NodesAdmitted,
        "reviewed-2026-08",
        10,
    )?;
    assert_eq!(justification.reason(), "reviewed-2026-08");
    let mut limits = BTreeMap::new();
    limits.insert(ReachabilityWorkDimension::NodesAdmitted, 4);
    let operation_budget = ReachabilityWorkBudget::new(
        ReachabilityProfileId::new("surface-profile")?,
        vec![ReachabilityOperationKind::Classification],
        limits,
        BTreeMap::new(),
    )?;
    assert_eq!(operation_budget.profile_id().as_str(), "surface-profile");
    assert_eq!(
        operation_budget.selected_operation_kinds(),
        &[ReachabilityOperationKind::Classification]
    );
    assert_eq!(
        operation_budget.limit_for(ReachabilityWorkDimension::NodesAdmitted),
        Some(ReachabilityDimensionLimit::Bounded(4))
    );
    assert_eq!(operation_budget.limit_for(ReachabilityWorkDimension::EdgesAdmitted), None);

    // Subject, tracker, and stage-output surface.
    let subject = ReachabilityOperationSubject::new(
        ReachabilityOperationId::new("surface-op")?,
        ReachabilityOperationKind::Classification,
        vec![
            snapshot_identity("snapshot-a", "gen-1")?,
            ReachabilitySubjectIdentity::new(
                ReachabilitySubjectIdentityKind::WorkBudgetProfile,
                "surface-profile",
                None,
            )?,
        ],
        ReachabilityProfileId::new("surface-profile")?,
    )?;
    let mut tracker = ReachabilityWorkTracker::new(subject.clone(), operation_budget)?;
    assert_eq!(tracker.subject().operation_id().as_str(), "surface-op");
    assert_eq!(tracker.budget().profile_id().as_str(), "surface-profile");
    tracker.charge(ReachabilityWorkDimension::NodesAdmitted, 4)?;
    let _ = tracker.charge(ReachabilityWorkDimension::NodesAdmitted, 1);
    tracker.poll_checkpoint(
        &ReachabilityStageId::new("classification")?,
        &ScriptedControl::cancelling()?,
    );
    let receipt = tracker.finish();
    assert_eq!(receipt.exhausted_attempts().len(), 1);
    assert_eq!(receipt.checkpoints_observed().len(), 1);
    assert_eq!(receipt.checkpoints_observed()[0].as_str(), "classification");
    assert!(receipt.terminal().is_some_and(|terminal| terminal.is_resource_exhausted()));

    let stage_output = ReachabilityStageOutput::new(
        ReachabilityStageId::new("classification")?,
        ReachabilitySubjectIdentity::new(
            ReachabilitySubjectIdentityKind::StageOutput,
            "classification-1",
            None,
        )?,
    );
    assert_eq!(stage_output.stage().as_str(), "classification");
    assert_eq!(stage_output.output().as_str(), "classification-1");

    // Ledger surface.
    let runtime = ReachabilityFactFamilyId::new("runtime-edges")?;
    let mut families = BTreeMap::new();
    families.insert(runtime.clone(), ReachabilityFactFamilyStatus::Complete);
    let ledger = ReachabilityFactFamilyLedger::new(families);
    assert!(ledger.requires_complete(std::slice::from_ref(&runtime)));
    assert_eq!(
        ledger.status(&ReachabilityFactFamilyId::new("missing-family")?),
        ReachabilityFactFamilyStatus::Missing
    );
    assert!(
        !ledger.requires_complete(&[runtime, ReachabilityFactFamilyId::new("missing-family")?])
    );

    // Error surface renders without panicking and stays fail-closed.
    let rendered = format!(
        "{} {} {}",
        ReachabilityContractError::BudgetProfileMismatch,
        ReachabilityChargeError::Exhausted {
            dimension: ReachabilityWorkDimension::NodesAdmitted,
            limit: 4,
            charged: 4,
        },
        ReachabilityWorkHonestyError::ReuseWithoutDeclaredIdentity {
            stage: ReachabilityStageId::new("classification")?,
        }
    );
    assert!(rendered.contains("profile does not match"));
    assert!(rendered.contains("nodes-admitted"));
    assert!(rendered.contains("without a declared current subject identity"));
    Ok(())
}

#[test]
fn bounded_view_surface_exposes_its_view_profile_and_returned_bounds() -> Result<(), Box<dyn Error>>
{
    let mut tracker = new_tracker(ReachabilityWorkDimension::NodesAdmitted, 100)?;
    tracker.note_instrument_evidence(instrument_identity()?);
    let output = ReachabilitySubjectIdentity::new(
        ReachabilitySubjectIdentityKind::StageOutput,
        "classification-1",
        None,
    )?;
    tracker.complete_stage(
        ReachabilityStageId::new("classification")?,
        Some(output.clone()),
        Vec::new(),
    );
    let outcome = ReachabilityOperationOutcome::<Vec<String>>::complete(
        ReachabilitySemanticOutcome::Complete,
        Some(vec![String::from("a"), String::from("b"), String::from("c")]),
        tracker.finish(),
    )?;
    let source = outcome
        .bounded_view_source(output, snapshot_identity("snapshot-a", "gen-1")?)
        .ok_or("complete outcome must mint the view source")?;
    let view = ReachabilityBoundedView::new(
        source,
        ReachableViewProfileId::new("source-partition-v1")?,
        2,
        128,
        Some(3),
        Some(1),
        true,
        Some(String::from("item-limit")),
    )?;
    assert_eq!(view.view_profile().as_str(), "source-partition-v1");
    assert_eq!(view.items_returned(), 2);
    assert_eq!(view.bytes_returned(), 128);
    assert_eq!(view.known_total(), Some(3));
    assert_eq!(view.omitted_count(), Some(1));
    assert!(view.truncated());
    assert_eq!(view.truncation_reason(), Some("item-limit"));
    Ok(())
}

#[test]
fn same_kind_different_generation_is_a_conflict_not_an_ordering() -> Result<(), Box<dyn Error>> {
    let conflicting = ReachabilityOperationSubject::new(
        ReachabilityOperationId::new("op-4")?,
        ReachabilityOperationKind::GraphAdmission,
        vec![snapshot_identity("snapshot-a", "gen-1")?, snapshot_identity("snapshot-a", "gen-2")?],
        ReachabilityProfileId::new("test-profile")?,
    );
    assert!(conflicting.is_err());
    Ok(())
}

#[test]
fn empty_operation_kind_selection_has_its_own_error() -> Result<(), Box<dyn Error>> {
    assert_eq!(
        contract_error(ReachabilityWorkBudget::new(
            ReachabilityProfileId::new("p")?,
            Vec::new(),
            BTreeMap::new(),
            BTreeMap::new(),
        ))?,
        ReachabilityContractError::EmptyOperationKindSelection
    );
    assert_eq!(
        contract_error(ReachabilityExecutionProfile::new(
            ReachabilityProfileId::new("p")?,
            1,
            ReachabilityExecutionPurpose::Batch,
            Vec::new(),
            Vec::new(),
            ReachabilityCancellationPolling::AtDeclaredCheckpoints,
            ReachabilityRetentionLimits { max_explanation_items: None, max_output_bytes: None },
            "authority",
            Vec::new(),
        ))?,
        ReachabilityContractError::EmptyOperationKindSelection
    );
    Ok(())
}

#[test]
fn deserialization_validates_every_opaque_id_and_identity() -> Result<(), Box<dyn Error>> {
    let empty = serde_json::Value::String(String::new());
    assert!(serde_json::from_value::<ReachabilityOperationId>(empty.clone()).is_err());
    assert!(serde_json::from_value::<ReachabilityStageId>(empty.clone()).is_err());
    assert!(serde_json::from_value::<ReachabilityFactFamilyId>(empty.clone()).is_err());
    assert!(serde_json::from_value::<ReachabilityProfileId>(empty.clone()).is_err());
    assert!(serde_json::from_value::<ReachableViewProfileId>(empty.clone()).is_err());
    let stage = ReachabilityStageId::new("closure")?;
    assert_eq!(
        serde_json::from_value::<ReachabilityStageId>(serde_json::to_value(&stage)?)?,
        stage
    );
    let empty_identity = serde_json::json!({
        "kind": "Project",
        "value": "",
        "generation": null,
    });
    assert!(serde_json::from_value::<ReachabilitySubjectIdentity>(empty_identity).is_err());
    let empty_generation = serde_json::json!({
        "kind": "Project",
        "value": "p1",
        "generation": "",
    });
    assert!(serde_json::from_value::<ReachabilitySubjectIdentity>(empty_generation).is_err());
    let identity = snapshot_identity("snapshot-a", "gen-1")?;
    assert_eq!(
        serde_json::from_value::<ReachabilitySubjectIdentity>(serde_json::to_value(&identity)?)?,
        identity
    );
    Ok(())
}

#[test]
fn bounded_view_rejects_incoherent_totals_and_zero_omission_under_truncation()
-> Result<(), Box<dyn Error>> {
    let mut tracker = new_tracker(ReachabilityWorkDimension::NodesAdmitted, 100)?;
    tracker.note_instrument_evidence(instrument_identity()?);
    let output = ReachabilitySubjectIdentity::new(
        ReachabilitySubjectIdentityKind::StageOutput,
        "classification-1",
        None,
    )?;
    tracker.complete_stage(
        ReachabilityStageId::new("classification")?,
        Some(output.clone()),
        Vec::new(),
    );
    let outcome = ReachabilityOperationOutcome::<Vec<String>>::complete(
        ReachabilitySemanticOutcome::Complete,
        Some(vec![String::from("row")]),
        tracker.finish(),
    )?;
    let source = outcome
        .bounded_view_source(output, snapshot_identity("snapshot-a", "gen-1")?)
        .ok_or("exact outcome must mint the view source")?;

    // Truncated with a known-zero omitted count claims truncation while
    // omitting nothing.
    assert!(
        ReachabilityBoundedView::new(
            source.clone(),
            ReachableViewProfileId::new("v")?,
            1,
            8,
            None,
            Some(0),
            true,
            Some(String::from("item-limit")),
        )
        .is_err()
    );
    // A known total smaller than the returned items is incoherent.
    assert!(
        ReachabilityBoundedView::new(
            source,
            ReachableViewProfileId::new("v")?,
            10,
            64,
            Some(3),
            None,
            false,
            None,
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn composite_deserialization_cannot_bypass_constructor_validation() -> Result<(), Box<dyn Error>> {
    let empty_kinds_budget = serde_json::json!({
        "profile_id": "p",
        "selected_operation_kinds": [],
        "dimension_limits": {},
        "unlimited": {},
    });
    assert!(serde_json::from_value::<ReachabilityWorkBudget>(empty_kinds_budget).is_err());
    let zero_bound = serde_json::json!({
        "profile_id": "p",
        "selected_operation_kinds": ["classification"],
        "dimension_limits": {},
        "unlimited": {
            "nodes-admitted": { "reason": "reviewed", "safety_bound": 0 },
        },
    });
    assert!(serde_json::from_value::<ReachabilityWorkBudget>(zero_bound).is_err());
    let zero_version_profile = serde_json::json!({
        "profile_id": "p",
        "version": 0,
        "purpose": "Batch",
        "selected_operation_kinds": ["classification"],
        "selected_fact_families": [],
        "cancellation_polling": "AtDeclaredCheckpoints",
        "retention": { "max_explanation_items": null, "max_output_bytes": null },
        "defaults_source": "authority",
        "limitations": [],
    });
    assert!(serde_json::from_value::<ReachabilityExecutionProfile>(zero_version_profile).is_err());
    let conflicting_subject = serde_json::json!({
        "operation_id": "op-5",
        "kind": "graph-admission",
        "identities": [
            { "kind": "WorkspaceSnapshot", "value": "snapshot-a", "generation": "gen-1" },
            { "kind": "WorkspaceSnapshot", "value": "snapshot-a", "generation": "gen-2" },
        ],
        "budget_profile_id": "test-profile",
        "stage_outputs": [],
    });
    assert!(serde_json::from_value::<ReachabilityOperationSubject>(conflicting_subject).is_err());
    let truncated_zero_view = serde_json::json!({
        "underlying": {
            "result_identity": { "kind": "StageOutput", "value": "r", "generation": null },
            "currentness_authority": { "kind": "WorkspaceSnapshot", "value": "s", "generation": "g" },
        },
        "view_profile": "v",
        "items_returned": 1,
        "bytes_returned": 8,
        "known_total": null,
        "omitted_count": 0,
        "truncated": true,
        "truncation_reason": "item-limit",
    });
    assert!(serde_json::from_value::<ReachabilityBoundedView>(truncated_zero_view).is_err());
    Ok(())
}
