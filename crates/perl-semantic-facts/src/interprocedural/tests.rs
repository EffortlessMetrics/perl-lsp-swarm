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
        body: BodyIdentity::Exact("sha256:caller-body".to_string()),
        source: SourceIdentity {
            document: FileId(1),
            workspace_root: Some("repo".to_string()),
            project_generation: Some(generation()),
            profile: ProfileIdentity::new(
                Some("5.38".to_string()),
                vec!["strict".to_string(), "signatures".to_string()],
                Some("linux-x86_64".to_string()),
                Some("default".to_string()),
            ),
        },
        call_phase: CallPhase::Runtime,
        world_generation: Some(generation()),
        call_edge_id: crate::EdgeId(500),
        substitutions: vec![
            ParameterSubstitution { parameter: 0, place: CallInput::ExactValue(fact(101)) },
            ParameterSubstitution { parameter: 1, place: CallInput::Omitted },
        ],
        policy_identity: ApplicationPolicyIdentity::SummaryBacked,
        component: ComponentIdentity { max_depth: 8, component_id: None },
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

#[test]
fn falsifier_identity_body_change_is_a_different_subject() {
    // The operator-review tuple: changed body must never validate as the
    // same subject — the two subjects must differ in canonical bytes.
    let mut changed = subject();
    changed.body = BodyIdentity::Exact("sha256:other-body".to_string());
    assert_ne!(must(serde_json::to_vec(&subject())), must(serde_json::to_vec(&changed)));
    assert!(changed.validate().is_ok());

    // An empty Exact body identity is not an identity.
    let mut empty = subject();
    empty.body = BodyIdentity::Exact(String::new());
    assert!(empty.validate().is_err());
}

#[test]
fn falsifier_identity_profile_change_is_a_different_subject() {
    let mut changed = subject();
    changed.source.profile = ProfileIdentity::new(
        Some("5.40".to_string()),
        vec!["strict".to_string()],
        Some("linux-x86_64".to_string()),
        Some("default".to_string()),
    );
    assert_ne!(must(serde_json::to_vec(&subject())), must(serde_json::to_vec(&changed)));

    // Source document and call-site anchor file must agree.
    let mut mismatched = subject();
    mismatched.source.document = FileId(2);
    assert!(mismatched.validate().is_err());

    // Unsorted features are rejected ("strict" sorts after "signatures").
    let mut unordered = subject();
    unordered.source.profile = ProfileIdentity {
        perl_version: None,
        features: vec!["strict".to_string(), "signatures".to_string()],
        platform: None,
        capability: None,
    };
    assert!(unordered.validate().is_err());
}

#[test]
fn falsifier_identity_substitution_shape_is_enforced() {
    // Parameter beyond the input positions is rejected.
    let mut out_of_range = subject();
    out_of_range.substitutions =
        vec![ParameterSubstitution { parameter: 7, place: CallInput::Omitted }];
    assert!(out_of_range.validate().is_err());

    // Duplicate parameters are rejected.
    let mut duplicated = subject();
    duplicated.substitutions = vec![
        ParameterSubstitution { parameter: 0, place: CallInput::Omitted },
        ParameterSubstitution { parameter: 0, place: CallInput::ExactValue(fact(101)) },
    ];
    assert!(duplicated.validate().is_err());

    // A zero-depth component bound is rejected.
    let mut zero_depth = subject();
    zero_depth.component = ComponentIdentity { max_depth: 0, component_id: None };
    assert!(zero_depth.validate().is_err());
}

#[test]
fn falsifier_identity_phase_and_policy_stay_distinct() {
    let phases = [CallPhase::CompileTime, CallPhase::Runtime, CallPhase::Unknown];
    let serialized: Vec<String> = phases.iter().map(|p| must(serde_json::to_string(p))).collect();
    assert_ne!(serialized[0], serialized[1]);
    assert_ne!(serialized[1], serialized[2]);

    let mut policy_changed = subject();
    policy_changed.policy_identity = ApplicationPolicyIdentity::Direct;
    assert_ne!(must(serde_json::to_vec(&subject())), must(serde_json::to_vec(&policy_changed)));

    // A different call edge is a different subject.
    let mut edge_changed = subject();
    edge_changed.call_edge_id = crate::EdgeId(501);
    assert_ne!(must(serde_json::to_vec(&subject())), must(serde_json::to_vec(&edge_changed)));
}

#[test]
fn falsifier_cross_generation_result_reuse_is_rejected() {
    // A G2 result must never validate while answering a G1 subject (#12672
    // operator review).
    let mut r = result();
    r.source_generation = SourceGeneration::known("gen-2");
    let violations = must_err(r.validate());
    assert!(violations.iter().any(|v| v.contains("cross-generation reuse")));

    // Unknown result generation is rejected fail-closed.
    let mut r = result();
    r.source_generation = SourceGeneration::Unknown;
    assert!(r.validate().is_err());

    // A Fresh summary whose generation disagrees with the subject is
    // rejected even when the result generation matches.
    let mut r = result();
    if let Some(summary) = &mut r.summary_ref {
        summary.currentness = SummaryCurrentness::Fresh(SourceGeneration::known("gen-2"));
    }
    let violations = must_err(r.validate());
    assert!(violations.iter().any(|v| v.contains("cross-generation reuse")));
}

// ──────────────────────────────────────────────────────────────────────────────
// callable_semantic_summary.v1 falsifiers (#12674, I02)
// ──────────────────────────────────────────────────────────────────────────────

fn facet(
    facet: SummaryFacetKind,
    status: SummaryFacetStatus,
    unsupported: u32,
    missing: u32,
    outbound_dependencies: u32,
) -> FacetCompleteness {
    FacetCompleteness {
        facet,
        status,
        planned: 1,
        selected: 1,
        terminal: 0,
        unsupported,
        missing,
        outbound_dependencies,
    }
}

fn honest_facets() -> Vec<FacetCompleteness> {
    // One entry per kind, canonical order: Result/Effect limited by one
    // unresolved outbound call, Control limited by the missing CFG, the
    // unprovable families declared NotProven with unsupported counts.
    vec![
        FacetCompleteness {
            facet: SummaryFacetKind::Result,
            status: SummaryFacetStatus::Limited,
            planned: 2,
            selected: 2,
            terminal: 1,
            unsupported: 0,
            missing: 0,
            outbound_dependencies: 1,
        },
        facet(SummaryFacetKind::ParameterBinding, SummaryFacetStatus::NotProven, 1, 0, 0),
        facet(SummaryFacetKind::Place, SummaryFacetStatus::Complete, 0, 0, 0),
        FacetCompleteness {
            facet: SummaryFacetKind::Effect,
            status: SummaryFacetStatus::Limited,
            planned: 1,
            selected: 1,
            terminal: 1,
            unsupported: 0,
            missing: 0,
            outbound_dependencies: 1,
        },
        facet(SummaryFacetKind::AliasEscape, SummaryFacetStatus::NotProven, 1, 0, 0),
        facet(SummaryFacetKind::Diagnostic, SummaryFacetStatus::NotProven, 1, 0, 0),
        facet(SummaryFacetKind::Exception, SummaryFacetStatus::NotProven, 1, 0, 0),
        facet(SummaryFacetKind::Control, SummaryFacetStatus::Limited, 0, 1, 0),
        facet(SummaryFacetKind::CompileEffect, SummaryFacetStatus::NotProven, 1, 0, 0),
        facet(SummaryFacetKind::Boundary, SummaryFacetStatus::Complete, 0, 0, 0),
        facet(SummaryFacetKind::OutboundCall, SummaryFacetStatus::Complete, 0, 0, 0),
    ]
}

fn summary_ref_for(callable: EntityId) -> CallableSemanticSummaryRef {
    CallableSemanticSummaryRef::new(
        callable,
        generation(),
        vec![],
        vec![boundary()],
        CompositionPolicy::DirectOnly,
        ResultFacets { result: true, effect: true, escape: false, control: true },
        SummaryCurrentness::Fresh(generation()),
        WorkBudget { max_units: 100 },
        RefusalCeiling::Refuse,
        ClaimCeiling::Provisional,
        PrivacyClass::PrivateSafe,
    )
}

fn summary_packet() -> CallableSemanticSummary {
    let callable = entity(2);
    CallableSemanticSummary {
        schema_version: CALLABLE_SEMANTIC_SUMMARY_SCHEMA_VERSION,
        callable,
        callable_name: Some("f".to_string()),
        body: BodyIdentity::Exact("fp:body-2".to_string()),
        source_generation: generation(),
        anchor: anchor(),
        summary_ref: summary_ref_for(callable),
        facets: honest_facets(),
        result_exits: vec![
            ResultExitRef {
                kind: ResultExitKind::ExplicitReturn,
                source: Some(CallableFactRef::PirOp { body: 1, op: 3 }),
                anchor: Some(anchor()),
            },
            ResultExitRef { kind: ResultExitKind::ImplicitFallthrough, source: None, anchor: None },
        ],
        bindings: vec![BindingPlaceRef {
            name: "$x".to_string(),
            role: PlaceRole::Write,
            source: CallableFactRef::PirOp { body: 1, op: 0 },
            anchor: Some(anchor()),
        }],
        effects: vec![EffectRef {
            kind: EffectKind::Assign,
            source: CallableFactRef::PirOp { body: 1, op: 1 },
            anchor: Some(anchor()),
        }],
        outbound_calls: vec![OutboundCallDependency::new(
            CallableFactRef::HirItem(7),
            Some(anchor()),
            OutboundCallee::Named("g".to_string()),
            vec![SummaryFacetKind::Effect, SummaryFacetKind::Result],
            CallResolution::UnresolvedTransitive,
        )],
        boundary_sites: vec![BoundarySiteRef::new(
            BoundaryKind::DynamicValue,
            CallableFactRef::HirItem(9),
            Some(anchor()),
        )],
        work: SummaryWorkLedger {
            planned_callables: 1,
            visited_callables: 1,
            planned_ops: 5,
            visited_ops: 5,
            units_consumed: 5,
            bytes_retained: 0,
        },
    }
}

/// A packet whose every facet is Complete and that carries no outbound
/// dependencies — the only shape allowed to claim Exact.
fn exact_packet() -> CallableSemanticSummary {
    let mut packet = summary_packet();
    packet.outbound_calls = vec![];
    packet.facets = SummaryFacetKind::ALL
        .iter()
        .map(|kind| facet(*kind, SummaryFacetStatus::Complete, 0, 0, 0))
        .collect();
    packet.summary_ref.claim_ceiling = ClaimCeiling::Exact;
    packet
}

#[test]
fn valid_summary_packet_passes_and_serializes_deterministically() {
    let packet = summary_packet();
    assert!(packet.validate().is_ok(), "valid packet: {:?}", packet.validate());
    // The dependency constructor canonicalized the blocked-facet order.
    assert_eq!(
        packet.outbound_calls[0].blocked_facets,
        vec![SummaryFacetKind::Result, SummaryFacetKind::Effect]
    );
    let a = must(serde_json::to_vec(&packet));
    let b = must(serde_json::to_vec(&summary_packet()));
    assert_eq!(a, b, "two assemblies of the same packet must be byte-identical");
    let exact = exact_packet();
    assert!(
        exact.validate().is_ok(),
        "all-Complete packet may claim Exact: {:?}",
        exact.validate()
    );
}

#[test]
fn falsifier_unknown_call_as_pure() {
    // An unresolved call with an empty blocked set is a purity smuggle.
    let mut packet = summary_packet();
    packet.outbound_calls[0].blocked_facets = vec![];
    let violations = must_err(packet.validate());
    assert!(
        violations.iter().any(|v| v.contains("never"))
            && violations.iter().any(|v| v.contains("blocks"))
    );

    // An Unknown callee is still an unresolved transitive dependency: it must
    // block facets, and the facets it names must not be Complete.
    let mut packet = summary_packet();
    packet.outbound_calls[0].callee = OutboundCallee::Unknown;
    assert!(packet.validate().is_ok());
    packet.facets[0].status = SummaryFacetStatus::Complete;
    packet.facets[0].outbound_dependencies = 0;
    let violations = must_err(packet.validate());
    assert!(violations.iter().any(|v| v.contains("cross-facet completeness join")));

    // A Named callee must be a real name, never an empty passthrough.
    let mut packet = summary_packet();
    packet.outbound_calls[0].callee = OutboundCallee::Named(String::new());
    assert!(packet.validate().is_err());
}

#[test]
fn falsifier_missing_as_empty_summary() {
    // A facet with declared missing evidence can never be Complete — a gap
    // must not silently become an exact empty set.
    let mut packet = summary_packet();
    packet.facets[2].status = SummaryFacetStatus::Complete;
    packet.facets[2].missing = 1;
    let violations = must_err(packet.validate());
    assert!(violations.iter().any(|v| v.contains("can never be Complete")));

    // Unsupported evidence precludes Complete just as hard (the status doc
    // says so; validation enforces it).
    let mut packet = summary_packet();
    packet.facets[2].status = SummaryFacetStatus::Complete;
    packet.facets[2].unsupported = 1;
    let violations = must_err(packet.validate());
    assert!(violations.iter().any(|v| v.contains("unsupported=1")));

    // The honest form — NotProven with the unsupported count declared —
    // passes: AliasEscape is never Complete-with-zero.
    let packet = summary_packet();
    let alias = &packet.facets[4];
    assert_eq!(alias.facet, SummaryFacetKind::AliasEscape);
    assert_eq!(alias.status, SummaryFacetStatus::NotProven);
    assert!(alias.unsupported > 0);
    assert!(packet.validate().is_ok());
}

#[test]
fn falsifier_cross_facet_completeness_summary() {
    // Facet-specific completeness: Boundary Complete while Result stays
    // Limited is valid — one facet never strengthens another.
    let packet = summary_packet();
    assert_eq!(packet.facets[9].status, SummaryFacetStatus::Complete);
    assert_eq!(packet.facets[0].status, SummaryFacetStatus::Limited);
    assert!(packet.validate().is_ok());

    // But Exact claims over a non-Complete ledger are a strengthening smuggle.
    let mut packet = summary_packet();
    packet.summary_ref.claim_ceiling = ClaimCeiling::Exact;
    let violations = must_err(packet.validate());
    assert!(violations.iter().any(|v| v.contains("facet-specific")));

    // Unsorted or duplicated facet ledgers break the canonical join.
    let mut packet = summary_packet();
    packet.facets.swap(0, 1);
    assert!(packet.validate().is_err());
    let mut packet = summary_packet();
    packet.facets.remove(0);
    assert!(packet.validate().is_err());
}

#[test]
fn falsifier_zero_work_summary() {
    let mut packet = summary_packet();
    packet.work.visited_ops = 0;
    let violations = must_err(packet.validate());
    assert!(violations.iter().any(|v| v.contains("work law")));

    // visited beyond planned is honest, never a violation: one offered
    // expression can lower to several operations.
    let mut packet = summary_packet();
    packet.work.visited_ops = 6; // beyond planned_ops = 5
    assert!(packet.validate().is_ok());

    let mut packet = summary_packet();
    packet.work.visited_callables = 0;
    assert!(packet.validate().is_err());
}

#[test]
fn falsifier_summary_ordering() {
    let canonical = must(serde_json::to_vec(&summary_packet()));

    // Normalized identity sets are canonicalized at construction: permuted
    // blocked facets and referenced boundaries produce identical bytes.
    let mut permuted = summary_packet();
    permuted.outbound_calls[0] = OutboundCallDependency::new(
        CallableFactRef::HirItem(7),
        Some(anchor()),
        OutboundCallee::Named("g".to_string()),
        vec![SummaryFacetKind::Effect, SummaryFacetKind::Result],
        CallResolution::UnresolvedTransitive,
    );
    assert_eq!(must(serde_json::to_vec(&permuted)), canonical);

    // Hand-built (non-constructor) unsorted blocked facets are rejected.
    let mut unsorted = summary_packet();
    unsorted.outbound_calls[0].blocked_facets =
        vec![SummaryFacetKind::Effect, SummaryFacetKind::Result];
    assert!(unsorted.validate().is_err());

    // Source-ordered lists are preserved verbatim, never normalized: moving
    // the fallthrough exit changes the bytes AND fails validation.
    let mut reordered = summary_packet();
    reordered.result_exits.reverse();
    assert_ne!(must(serde_json::to_vec(&reordered)), canonical);
    let violations = must_err(reordered.validate());
    assert!(violations.iter().any(|v| v.contains("ImplicitFallthrough")));

    // A missing fallthrough exit is a structural violation.
    let mut no_fallthrough = summary_packet();
    no_fallthrough.result_exits.pop();
    assert!(no_fallthrough.validate().is_err());
}

#[test]
fn falsifier_summary_stale_reuse() {
    // A stale packet must not carry Exact claims — re-checked at the packet
    // join even though the envelope already enforces it.
    let mut packet = exact_packet();
    packet.summary_ref.currentness = SummaryCurrentness::Stale;
    let violations = must_err(packet.validate());
    assert!(violations.iter().any(|v| v.contains("historical-as-current")));

    // Stale with a Provisional ceiling is the honest historical form.
    let mut packet = summary_packet();
    packet.summary_ref.currentness = SummaryCurrentness::Stale;
    assert!(packet.validate().is_ok());

    // The packet and its envelope must name one freshness identity.
    let mut packet = summary_packet();
    packet.source_generation = SourceGeneration::known("gen-2");
    let violations = must_err(packet.validate());
    assert!(violations.iter().any(|v| v.contains("one freshness identity")));

    // The envelope must describe the packet's own callable.
    let mut packet = summary_packet();
    packet.summary_ref.callable = entity(999);
    let violations = must_err(packet.validate());
    assert!(violations.iter().any(|v| v.contains("one subject, one identity")));
}

#[test]
fn falsifier_summary_schema_and_anchor_guards() {
    let mut packet = summary_packet();
    packet.schema_version = 2;
    assert!(packet.validate().is_err());

    let mut packet = summary_packet();
    packet.anchor = SourceAnchor::new(None, NO_FILE, 10, 20);
    assert!(packet.validate().is_err());

    let mut packet = summary_packet();
    packet.body = BodyIdentity::Exact(String::new());
    assert!(packet.validate().is_err());

    let mut packet = summary_packet();
    packet.callable_name = Some(String::new());
    assert!(packet.validate().is_err());
}

#[test]
fn falsifier_boundary_site_ledger_mismatch() {
    // The Boundary facet's ledger must agree with the packet's site record:
    // deduped or dropped provenance is a validation violation.
    let mut packet = summary_packet();
    assert!(packet.validate().is_ok(), "fixture: one site, selected=1");
    packet.boundary_sites = vec![];
    let violations = must_err(packet.validate());
    assert!(violations.iter().any(|v| v.contains("site/ledger mismatch")));

    // Two sites with a deduped count of one is equally dishonest.
    let mut packet = summary_packet();
    packet.boundary_sites.push(BoundarySiteRef::new(
        BoundaryKind::DynamicValue,
        CallableFactRef::HirItem(11),
        Some(anchor()),
    ));
    let violations = must_err(packet.validate());
    assert!(violations.iter().any(|v| v.contains("site/ledger mismatch")));
}
