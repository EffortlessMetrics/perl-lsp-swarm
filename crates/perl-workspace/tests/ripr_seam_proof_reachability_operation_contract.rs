//! Workspace-layer tests for the shared reachability operation contract
//! (#11553).
//!
//! These tests exercise the contract at consumer granularity — the
//! graph-admission → SCC → closure → query stage walk planned by
//! #10915/#10921/#10928/#10935 — without implementing any consumer. They
//! pin the stage-composition law the consumers will rely on: a terminal in
//! any stage makes the combined claim non-complete, limitations are never
//! erased, and only a separate narrower operation may retain its own exact
//! result.

use perl_semantic_facts::reachability_operation::{
    ReachabilityClaimLimitation, ReachabilityFactFamilyId, ReachabilityFactFamilyLedger,
    ReachabilityFactFamilyStatus, ReachabilityIneligibilityReason, ReachabilityOperationControl,
    ReachabilityOperationId, ReachabilityOperationKind, ReachabilityOperationOutcome,
    ReachabilityOperationSubject, ReachabilityProfileId, ReachabilityPublicationEligibility,
    ReachabilitySemanticOutcome, ReachabilityStageId, ReachabilitySubjectIdentity,
    ReachabilitySubjectIdentityKind, ReachabilityTerminalObservation, ReachabilityWorkBudget,
    ReachabilityWorkDimension, ReachabilityWorkPathTarget, ReachabilityWorkTracker,
};
use std::collections::BTreeMap;
use std::error::Error;

struct ContinuingControl;

impl ReachabilityOperationControl for ContinuingControl {
    fn poll(
        &self,
        _subject: &ReachabilityOperationSubject,
    ) -> Option<ReachabilityTerminalObservation> {
        None
    }
}

struct SupersededControl {
    expected: ReachabilitySubjectIdentity,
    observed: ReachabilitySubjectIdentity,
}

impl ReachabilityOperationControl for SupersededControl {
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
    Ok(ReachabilitySubjectIdentity::new(
        ReachabilitySubjectIdentityKind::WorkspaceSnapshot,
        "workspace-snapshot-1",
        Some(generation.to_string()),
    )?)
}

fn instrument() -> Result<ReachabilitySubjectIdentity, Box<dyn Error>> {
    Ok(ReachabilitySubjectIdentity::new(
        ReachabilitySubjectIdentityKind::Instrument,
        "work-meter-1",
        None,
    )?)
}

fn liveness_subject(budget_profile: &str) -> Result<ReachabilityOperationSubject, Box<dyn Error>> {
    Ok(ReachabilityOperationSubject::new(
        ReachabilityOperationId::new("liveness-op-1")?,
        ReachabilityOperationKind::ProductionClosure,
        vec![
            snapshot("gen-7")?,
            ReachabilitySubjectIdentity::new(
                ReachabilitySubjectIdentityKind::Project,
                "project-1",
                None,
            )?,
            ReachabilitySubjectIdentity::new(
                ReachabilitySubjectIdentityKind::WorkBudgetProfile,
                budget_profile,
                None,
            )?,
        ],
        ReachabilityProfileId::new(budget_profile)?,
    )?)
}

fn component_budget(
    profile: &str,
    production_limit: Option<u64>,
) -> Result<ReachabilityWorkBudget, Box<dyn Error>> {
    let mut limits = BTreeMap::new();
    if let Some(limit) = production_limit {
        limits.insert(ReachabilityWorkDimension::ProductionClosureEdgesTraversed, limit);
    }
    limits.insert(ReachabilityWorkDimension::TestClosureEdgesTraversed, 100);
    limits.insert(ReachabilityWorkDimension::SccNodesVisited, 100);
    Ok(ReachabilityWorkBudget::new(
        ReachabilityProfileId::new(profile)?,
        vec![ReachabilityOperationKind::ProductionClosure],
        limits,
        BTreeMap::new(),
    )?)
}

fn complete_ledger() -> ReachabilityFactFamilyLedger {
    let mut families = BTreeMap::new();
    if let (Ok(runtime), Ok(activation)) = (
        ReachabilityFactFamilyId::new("runtime-edges"),
        ReachabilityFactFamilyId::new("activation-roots"),
    ) {
        families.insert(runtime, ReachabilityFactFamilyStatus::Complete);
        families.insert(activation, ReachabilityFactFamilyStatus::Complete);
    }
    ReachabilityFactFamilyLedger::new(families)
}

/// One closure completes and the other exhausts: the combined production+
/// test classification cannot become complete, while the test closure keeps
/// its own evidence inside the same receipt (#10928 composition law).
#[test]
fn reachability_operation_combined_closure_claim_cannot_complete_after_exhaustion()
-> Result<(), Box<dyn Error>> {
    let mut tracker = ReachabilityWorkTracker::new(
        liveness_subject("closure-profile-tight")?,
        component_budget("closure-profile-tight", Some(3))?,
    )?;
    let admission = ReachabilityStageId::new("graph-admission")?;
    let scc = ReachabilityStageId::new("scc-condensation")?;
    let production = ReachabilityStageId::new("production-closure")?;
    let test = ReachabilityStageId::new("test-closure")?;

    tracker.complete_stage(
        admission.clone(),
        Some(ReachabilitySubjectIdentity::new(
            ReachabilitySubjectIdentityKind::StageOutput,
            "graph-input-1",
            None,
        )?),
        Vec::new(),
    );
    tracker.complete_stage(
        scc.clone(),
        Some(ReachabilitySubjectIdentity::new(
            ReachabilitySubjectIdentityKind::StageOutput,
            "component-graph-1",
            None,
        )?),
        Vec::new(),
    );
    tracker.poll_checkpoint(&production, &ContinuingControl);

    // Production closure exhausts its edge budget; test closure then runs
    // to completion inside the same operation.
    tracker.charge(ReachabilityWorkDimension::ProductionClosureEdgesTraversed, 3)?;
    let exhausted =
        match tracker.charge(ReachabilityWorkDimension::ProductionClosureEdgesTraversed, 1) {
            Err(error) => error,
            Ok(()) => return Err("production closure must exhaust its edge budget".into()),
        };
    let (limit, charged) = match exhausted {
        perl_semantic_facts::reachability_operation::ReachabilityChargeError::Exhausted {
            limit,
            charged,
            ..
        } => (limit, charged),
        other => return Err(format!("unexpected charge error: {other:?}").into()),
    };
    assert_eq!((limit, charged), (3, 3));
    tracker.charge(ReachabilityWorkDimension::TestClosureEdgesTraversed, 9)?;
    tracker.complete_stage(test.clone(), None, Vec::new());
    tracker.note_instrument_evidence(instrument()?);

    let terminal = tracker.terminal().cloned().ok_or("exhaustion must latch a terminal state")?;
    let receipt = tracker.finish();
    let combined: ReachabilityOperationOutcome<Vec<String>> =
        ReachabilityOperationOutcome::terminal_from(&terminal, production.clone(), receipt)?;

    // The combined claim is a typed resource-exhausted terminal, never a
    // complete classification, never exact unreachable, never empty.
    assert!(matches!(combined, ReachabilityOperationOutcome::ResourceExhausted { .. }));
    assert!(!combined.may_claim_exact());
    assert!(combined.retained_partial_value().is_none());

    let subject = liveness_subject("closure-profile-tight")?;
    let verdict = ReachabilityPublicationEligibility::evaluate(
        &subject,
        Some(&snapshot("gen-7")?),
        ReachabilitySubjectIdentityKind::WorkspaceSnapshot,
        &[admission, scc, production, test],
        &combined,
        &complete_ledger(),
        &[
            ReachabilityFactFamilyId::new("runtime-edges")?,
            ReachabilityFactFamilyId::new("activation-roots")?,
        ],
    );
    assert!(!verdict.is_eligible());
    assert!(verdict.reasons().contains(&ReachabilityIneligibilityReason::TerminalState));
    Ok(())
}

/// A narrower explicitly separate operation may retain its own exact result
/// (#10928 narrowing law), consuming the shared upstream output identity.
#[test]
fn reachability_operation_narrower_operation_keeps_its_own_exact_result()
-> Result<(), Box<dyn Error>> {
    // Upstream: admission + SCC complete cleanly.
    let mut upstream = ReachabilityWorkTracker::new(
        liveness_subject("narrow-profile")?,
        component_budget("narrow-profile", Some(100))?,
    )?;
    upstream.complete_stage(
        ReachabilityStageId::new("graph-admission")?,
        Some(ReachabilitySubjectIdentity::new(
            ReachabilitySubjectIdentityKind::StageOutput,
            "graph-input-1",
            None,
        )?),
        Vec::new(),
    );
    let component_output = ReachabilitySubjectIdentity::new(
        ReachabilitySubjectIdentityKind::StageOutput,
        "component-graph-1",
        None,
    )?;
    upstream.complete_stage(
        ReachabilityStageId::new("scc-condensation")?,
        Some(component_output.clone()),
        Vec::new(),
    );
    upstream.charge(ReachabilityWorkDimension::SccNodesVisited, 12)?;
    upstream.note_instrument_evidence(instrument()?);
    let upstream_receipt = upstream.finish();
    let upstream_outcome = ReachabilityOperationOutcome::<String>::complete(
        ReachabilitySemanticOutcome::Complete,
        Some(String::from("component-graph")),
        upstream_receipt,
    )?;
    assert!(upstream_outcome.may_claim_exact());

    // Downstream: a separate query operation declares the component-graph
    // output identity and validates reuse of it rather than rebuilding.
    let mut query_subject = liveness_subject("narrow-profile")?;
    query_subject.append_stage_output(
        ReachabilityStageId::new("scc-condensation")?,
        component_output.clone(),
    );
    let mut query_tracker = ReachabilityWorkTracker::new(
        query_subject,
        component_budget("narrow-profile", Some(100))?,
    )?;
    query_tracker.record_validated_reuse(
        ReachabilityStageId::new("entity-query")?,
        ReachabilityWorkPathTarget::QueryProjection,
        component_output,
    )?;
    query_tracker.charge(ReachabilityWorkDimension::EntityQueries, 1)?;
    query_tracker.note_instrument_evidence(instrument()?);
    let query_receipt = query_tracker.finish();
    assert!(query_receipt.is_validated_reuse_of(&ReachabilityWorkPathTarget::QueryProjection));
    // A query operation charges only query-side dimensions: every charged
    // dimension of this receipt is a query counter, and the graph/SCC/closure
    // build counters stay absent while the component graph is validated for
    // reuse instead of reconstructed.
    let build_dimensions = [
        ReachabilityWorkDimension::SccNodesVisited,
        ReachabilityWorkDimension::SccEdgesVisited,
        ReachabilityWorkDimension::SccStackOperations,
        ReachabilityWorkDimension::ComponentsFormed,
        ReachabilityWorkDimension::CondensedEdgesConstructed,
        ReachabilityWorkDimension::ProductionClosureNodesTraversed,
        ReachabilityWorkDimension::ProductionClosureEdgesTraversed,
        ReachabilityWorkDimension::TestClosureNodesTraversed,
        ReachabilityWorkDimension::TestClosureEdgesTraversed,
    ];
    for dimension in build_dimensions {
        assert_eq!(query_receipt.charged().get(&dimension), None);
    }
    assert!(
        query_receipt
            .charged()
            .keys()
            .all(|dimension| matches!(dimension, ReachabilityWorkDimension::EntityQueries))
    );
    let query_outcome = ReachabilityOperationOutcome::<String>::complete(
        ReachabilitySemanticOutcome::Complete,
        Some(String::from("query-answer")),
        query_receipt,
    )?;
    assert!(query_outcome.may_claim_exact());
    Ok(())
}

/// A missing edge family in admission stays partial even though SCC could
/// run over the remaining edges (#11553 stage-composition example for
/// #10915).
#[test]
fn reachability_operation_missing_family_survives_into_every_later_stage()
-> Result<(), Box<dyn Error>> {
    let missing = ReachabilityFactFamilyId::new("runtime-edges")?;
    let mut tracker = ReachabilityWorkTracker::new(
        liveness_subject("partial-profile")?,
        component_budget("partial-profile", Some(100))?,
    )?;
    tracker.complete_stage(
        ReachabilityStageId::new("graph-admission")?,
        Some(ReachabilitySubjectIdentity::new(
            ReachabilitySubjectIdentityKind::StageOutput,
            "graph-input-1",
            None,
        )?),
        vec![ReachabilityClaimLimitation::MissingFactFamily(missing.clone())],
    );
    tracker.complete_stage(ReachabilityStageId::new("scc-condensation")?, None, Vec::new());
    tracker.note_instrument_evidence(instrument()?);
    let receipt = tracker.finish();
    assert_eq!(receipt.stage_limitations().len(), 1);
    let partial = ReachabilityOperationOutcome::<String>::complete(
        ReachabilitySemanticOutcome::Partial {
            limitations: vec![ReachabilityClaimLimitation::MissingFactFamily(missing.clone())],
        },
        Some(String::from("partial-graph")),
        receipt,
    )?;
    assert!(!partial.may_claim_exact());

    // The denominator ledger refuses exactness for the same family even
    // though the operation itself completed.
    let mut families = BTreeMap::new();
    families.insert(missing, ReachabilityFactFamilyStatus::Missing);
    let verdict = ReachabilityPublicationEligibility::evaluate(
        &liveness_subject("partial-profile")?,
        Some(&snapshot("gen-7")?),
        ReachabilitySubjectIdentityKind::WorkspaceSnapshot,
        &[],
        &partial,
        &ReachabilityFactFamilyLedger::new(families),
        &[ReachabilityFactFamilyId::new("runtime-edges")?],
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

/// Supersession immediately before publication cannot relabel the result
/// current for changed inputs, and a prior complete result stays bound to
/// its own generation (#10957 seam).
#[test]
fn reachability_operation_supersession_before_publication_blocks_reuse()
-> Result<(), Box<dyn Error>> {
    let mut tracker = ReachabilityWorkTracker::new(
        liveness_subject("stale-profile")?,
        component_budget("stale-profile", Some(100))?,
    )?;
    tracker.complete_stage(
        ReachabilityStageId::new("graph-admission")?,
        Some(ReachabilitySubjectIdentity::new(
            ReachabilitySubjectIdentityKind::StageOutput,
            "graph-input-1",
            None,
        )?),
        Vec::new(),
    );
    tracker.note_instrument_evidence(instrument()?);

    let superseded_control =
        SupersededControl { expected: snapshot("gen-7")?, observed: snapshot("gen-8")? };

    let terminal = tracker
        .poll_checkpoint(&ReachabilityStageId::new("publication")?, &superseded_control)
        .ok_or("supersession must be observed before publication")?;
    let receipt = tracker.finish();
    let outcome = ReachabilityOperationOutcome::<String>::terminal_from(
        &terminal,
        ReachabilityStageId::new("publication")?,
        receipt,
    )?;
    assert!(matches!(outcome, ReachabilityOperationOutcome::SupersededOrStale { .. }));

    let verdict = ReachabilityPublicationEligibility::evaluate(
        &liveness_subject("stale-profile")?,
        Some(&snapshot("gen-8")?),
        ReachabilitySubjectIdentityKind::WorkspaceSnapshot,
        &[],
        &outcome,
        &complete_ledger(),
        &[],
    );
    assert!(!verdict.is_eligible());
    assert!(verdict.reasons().contains(&ReachabilityIneligibilityReason::SubjectSuperseded));
    assert!(verdict.reasons().contains(&ReachabilityIneligibilityReason::TerminalState));
    Ok(())
}
