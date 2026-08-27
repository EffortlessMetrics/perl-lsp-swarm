//! Falsifier-first contract tests for the interprocedural contracts (#12672).
//!
//! Each test names the wrong-shape it kills. The fixtures are synthetic only:
//! no fact extraction, no call graph, no composition.

use super::*;
use crate::{
    BoundaryDisposition, BoundaryKind, Confidence, LifecyclePhase, Provenance,
    SemanticFactEnvelope, SemanticFactKind, SemanticFreshness, SemanticProducer,
};
use perl_test_must::{must, must_err};

fn entity(n: u64) -> EntityId {
    EntityId(n)
}

fn fact(n: u64) -> FactId {
    FactId(n)
}

fn generation() -> SourceGeneration {
    SourceGeneration::known("gen-1")
}

fn anchor() -> SourceAnchor {
    SourceAnchor::new(None, FileId(1), 10, 20)
}

fn boundary() -> BoundaryLink {
    BoundaryLink::new(
        Some(fact(900)),
        BoundaryKind::DynamicValue,
        BoundaryDisposition::Degrade,
        SemanticReasonCode::DynamicValue,
    )
}

fn subject() -> CallApplicationSubject {
    CallApplicationSubject {
        schema_version: CALL_APPLICATION_SUBJECT_SCHEMA_VERSION,
        call_fact_id: fact(100),
        caller: entity(1),
        callee: CallTarget::Exact(entity(2)),
        anchor: anchor(),
        source_generation: generation(),
        package: Some("My::Package".to_string()),
        inputs: vec![CallInput::ExactValue(fact(101)), CallInput::Omitted],
        context: CallContext { receiver: ReceiverKind::Function, lexical_scope: Some(ScopeId(1)) },
    }
}

fn summary() -> CallableSemanticSummaryRef {
    CallableSemanticSummaryRef::new(
        entity(2),
        generation(),
        vec![fact(202), fact(201)],
        vec![boundary()],
        CompositionPolicy::Acyclic,
        ResultFacets { result: true, effect: false, escape: false, control: false },
        SummaryCurrentness::Fresh(generation()),
        WorkBudget { max_units: 100 },
        RefusalCeiling::Refuse,
        ClaimCeiling::Exact,
        PrivacyClass::PrivateSafe,
    )
}

fn envelope_fact(id: u64) -> SemanticFactEnvelope {
    SemanticFactEnvelope::new(
        fact(id),
        Some(entity(2)),
        SemanticFactKind::CallableResult,
        anchor(),
        generation(),
        Some(ScopeId(1)),
        Some("My::Package".to_string()),
        LifecyclePhase::Runtime,
        SemanticProducer::SemanticAnalyzer,
        SemanticProvenance::Known(Provenance::ExactAst),
        SemanticConfidence::Known(Confidence::High),
        SemanticFreshness::Fresh,
        None,
        vec![],
        SemanticReasonCode::ExactSource,
    )
}

fn result() -> InterproceduralFactResult {
    InterproceduralFactResult {
        schema_version: INTERPROCEDURAL_FACT_RESULT_SCHEMA_VERSION,
        subject: subject(),
        summary_ref: Some(summary()),
        outcome: InterproceduralOutcome::Composed,
        facts: vec![envelope_fact(300)],
        boundaries: vec![boundary()],
        units_consumed: 10,
        source_generation: generation(),
        confidence: SemanticConfidence::Known(Confidence::High),
        provenance: SemanticProvenance::Known(Provenance::ExactAst),
        reason_code: SemanticReasonCode::ExactSource,
    }
}

#[test]
fn valid_fixtures_pass() {
    assert!(subject().validate().is_ok());
    let summary = summary();
    assert!(summary.validate().is_ok());
    // The constructor canonicalizes reference order.
    assert_eq!(summary.referenced_facts, vec![fact(201), fact(202)]);
    assert!(result().validate().is_ok());
}

#[test]
fn falsifier_duplicate_vocabulary_wrong_schema_version_is_rejected() {
    let mut s = subject();
    s.schema_version = 99;
    assert!(s.validate().is_err());

    let mut summary = summary();
    summary.schema_version = 0;
    assert!(summary.validate().is_err());

    let mut r = result();
    r.schema_version = 2;
    assert!(r.validate().is_err());
}

#[test]
fn falsifier_name_path_output_identity_no_file_anchor_is_rejected() {
    let mut s = subject();
    s.anchor = SourceAnchor::new(None, NO_FILE, 10, 20);
    assert!(s.validate().is_err());
}

#[test]
fn falsifier_incomplete_call_subject_inverted_anchor_is_rejected() {
    let mut s = subject();
    s.anchor = SourceAnchor::new(None, FileId(1), 20, 10);
    assert!(s.validate().is_err());
}

#[test]
fn falsifier_cross_facet_strengthening_noclaim_with_facets_is_rejected() {
    let mut summary = summary();
    summary.claim_ceiling = ClaimCeiling::NoClaim;
    assert!(summary.validate().is_err());

    // NoClaim with no facets is the honest form and passes.
    summary.facets = ResultFacets { result: false, effect: false, escape: false, control: false };
    assert!(summary.validate().is_ok());
}

#[test]
fn falsifier_phase_collapse_composition_policies_stay_distinct() {
    let policies = [
        CompositionPolicy::DirectOnly,
        CompositionPolicy::Acyclic,
        CompositionPolicy::RecursiveBounded,
        CompositionPolicy::ConsumerPolicy,
    ];
    let serialized: Vec<String> = policies.iter().map(|p| must(serde_json::to_string(p))).collect();
    for (i, a) in serialized.iter().enumerate() {
        for (j, b) in serialized.iter().enumerate() {
            assert_eq!(i == j, a == b, "policies {i} and {j} collapsed");
        }
    }
}

#[test]
fn falsifier_hidden_boundary_dynamic_target_must_carry_its_boundary() {
    // The type system forces the boundary to exist; serialization must show
    // its reason, so the boundary cannot be silently dropped downstream.
    let mut s = subject();
    s.callee = CallTarget::DynamicBoundary(boundary());
    let json = must(serde_json::to_string(&s));
    assert!(json.contains("DynamicValue"), "boundary reason must be visible: {json}");
    assert!(s.validate().is_ok());
}

#[test]
fn falsifier_missing_as_empty_composed_without_facts_is_rejected() {
    let mut r = result();
    r.facts = vec![];
    let violations = must_err(r.validate());
    assert!(violations.iter().any(|v| v.contains("missing-as-empty")));
}

#[test]
fn falsifier_stale_reuse_stale_summary_cannot_claim_exact() {
    let mut summary = summary();
    summary.currentness = SummaryCurrentness::Stale;
    assert!(summary.validate().is_err());

    summary.claim_ceiling = ClaimCeiling::Provisional;
    assert!(summary.validate().is_ok());
}

#[test]
fn falsifier_refusal_as_empty_refused_or_invalid_outcome_carries_no_facts() {
    let mut r = result();
    r.outcome = InterproceduralOutcome::Refused { reason: SemanticReasonCode::DynamicValue };
    let violations = must_err(r.validate());
    assert!(violations.iter().any(|v| v.contains("refusal-as-empty")));

    r.facts = vec![];
    assert!(r.validate().is_ok());

    r.outcome = InterproceduralOutcome::Invalid { reason: "bad subject".to_string() };
    assert!(r.validate().is_ok());
}

#[test]
fn falsifier_historical_as_current_composed_from_stale_summary_is_rejected() {
    let mut r = result();
    if let Some(summary) = &mut r.summary_ref {
        summary.currentness = SummaryCurrentness::Stale;
        summary.claim_ceiling = ClaimCeiling::Provisional;
    }
    let violations = must_err(r.validate());
    assert!(violations.iter().any(|v| v.contains("historical-as-current")));

    r.outcome = InterproceduralOutcome::Stale { reason: SemanticReasonCode::DynamicValue };
    assert!(r.validate().is_ok());
}

#[test]
fn falsifier_summary_must_bind_to_the_exact_callee() {
    // A valid summary for another callable must not validate against an
    // exact-target subject (#12672 review).
    let mut r = result();
    if let Some(summary) = &mut r.summary_ref {
        summary.callable = entity(999);
    }
    let violations = must_err(r.validate());
    assert!(violations.iter().any(|v| v.contains("bind to the exact callee")));

    // A dynamic-target call carries no summary at all.
    let mut r = result();
    r.subject.callee = CallTarget::DynamicBoundary(boundary());
    let violations = must_err(r.validate());
    assert!(violations.iter().any(|v| v.contains("dynamic-target")));
}

#[test]
fn falsifier_composed_must_not_promote_stale_or_refused_facts() {
    // A stale fact inside Composed is strengthening by promotion (#12672
    // review).
    let mut r = result();
    r.facts[0].freshness = crate::SemanticFreshness::Stale;
    let violations = must_err(r.validate());
    assert!(violations.iter().any(|v| v.contains("promote")));
}

#[test]
fn falsifier_confidence_never_stronger_than_weakest_evidence() {
    let mut r = result();
    r.confidence = SemanticConfidence::Known(Confidence::High);
    r.facts[0].confidence = SemanticConfidence::Known(Confidence::Low);
    let violations = must_err(r.validate());
    assert!(violations.iter().any(|v| v.contains("confidence ceiling")));
}

#[test]
fn falsifier_fresh_currentness_must_equal_the_summary_generation() {
    let mut summary = summary();
    summary.currentness = SummaryCurrentness::Fresh(SourceGeneration::known("gen-2"));
    let violations = must_err(summary.validate());
    assert!(violations.iter().any(|v| v.contains("one freshness identity")));

    summary.currentness = SummaryCurrentness::Fresh(SourceGeneration::Unknown);
    let violations = must_err(summary.validate());
    assert!(violations.iter().any(|v| v.contains("known generation")));
}

#[test]
fn falsifier_resource_exhausted_count_must_be_authoritative() {
    let mut r = result();
    r.facts = vec![];
    r.outcome = InterproceduralOutcome::ResourceExhausted { units_consumed: 101 };
    let violations = must_err(r.validate());
    assert!(violations.iter().any(|v| v.contains("one authoritative count")));
}

#[test]
fn falsifier_ordering_unsorted_references_are_rejected_and_construction_canonicalizes() {
    // The constructor canonicalizes.
    let canonical = summary();
    assert!(canonical.validate().is_ok());

    // A hand-built (non-constructor) summary with unsorted references fails.
    let mut unordered = canonical.clone();
    unordered.referenced_facts = vec![fact(202), fact(201)];
    assert!(unordered.validate().is_err());

    // And the result's boundaries obey the same rule.
    let mut r = result();
    let b2 = BoundaryLink::new(
        Some(fact(800)),
        BoundaryKind::DynamicRequire,
        BoundaryDisposition::Degrade,
        SemanticReasonCode::DynamicValue,
    );
    r.boundaries = vec![boundary(), b2];
    assert!(r.validate().is_err());
}

#[test]
fn falsifier_privacy_private_payload_is_not_publishable() {
    let mut summary = summary();
    assert!(summary.is_publishable());
    summary.privacy = PrivacyClass::Private;
    assert!(!summary.is_publishable());
}

#[test]
fn falsifier_resource_ceiling_consumption_beyond_budget_is_rejected() {
    let mut r = result();
    r.units_consumed = 101; // budget is 100
    let violations = must_err(r.validate());
    assert!(violations.iter().any(|v| v.contains("resource ceiling")));

    // ResourceExhausted is the explicit terminal state for that class.
    r.units_consumed = 100;
    r.facts = vec![];
    r.outcome = InterproceduralOutcome::ResourceExhausted { units_consumed: 100 };
    assert!(r.validate().is_ok());
}

#[test]
fn canonical_serialization_is_deterministic_and_bounded() {
    let a = must(serde_json::to_vec(&summary()));
    let b = must(serde_json::to_vec(&summary()));
    assert_eq!(a, b);
    // Deterministic across reference insertion order via the constructor.
    let mut swapped = summary();
    swapped.referenced_facts.reverse();
    let reordered = CallableSemanticSummaryRef::new(
        swapped.callable,
        swapped.source_generation.clone(),
        swapped.referenced_facts,
        swapped.referenced_boundaries,
        swapped.composition_policy,
        swapped.facets,
        swapped.currentness,
        swapped.work,
        swapped.refusal_ceiling,
        swapped.claim_ceiling,
        swapped.privacy,
    );
    assert_eq!(must(serde_json::to_vec(&reordered)), a);
}
