//! Proof for the shared cursor occurrence identity layer.
//!
//! The realistic wrong implementation this proof targets is one that resolves a
//! cursor by *spelling*: it would happily return an exact identity for a
//! shadowed lexical, for a same-name entity in another root, or for an
//! occurrence whose entity was never resolved. Each of those has a negative
//! control below that fails such an implementation.

use super::{
    ResolveAtOutcome, ResolveAtSource, ResolveGenerationBasis, ResolveLimitation, ResolveNotReady,
    ResolveUnavailable, ResolvedOccurrence, resolve_at_position,
    resolve_at_position_with_dynamic_boundary, stable_generation_basis,
};
use perl_semantic_facts::{
    AnchorId, Confidence, EntityFact, EntityId, EntityKind, FileId, OccurrenceFact, OccurrenceId,
    OccurrenceKind, Provenance, ScopeId, SourceGeneration,
};

const FILE: FileId = FileId(7);

/// Stub carrying exactly the facts the narrow port needs, keyed by offset.
///
/// Deliberately offset-keyed rather than name-keyed: a stub that could answer by
/// spelling would let a spelling-based implementation pass.
#[derive(Default)]
struct StubSource {
    symbol_at: Vec<(u32, EntityFact, OccurrenceFact)>,
    dynamic_at: Vec<(u32, OccurrenceFact)>,
}

impl StubSource {
    fn with_symbol(mut self, offset: u32, entity: EntityFact, occurrence: OccurrenceFact) -> Self {
        self.symbol_at.push((offset, entity, occurrence));
        self
    }

    fn with_dynamic(mut self, offset: u32, occurrence: OccurrenceFact) -> Self {
        self.dynamic_at.push((offset, occurrence));
        self
    }
}

impl ResolveAtSource for StubSource {
    fn resolve_symbol_at(
        &self,
        file_id: FileId,
        byte_offset: u32,
    ) -> Option<(EntityFact, OccurrenceFact)> {
        self.symbol_at.iter().find_map(|(offset, entity, occurrence)| {
            (file_id == FILE && *offset == byte_offset)
                .then(|| (entity.clone(), occurrence.clone()))
        })
    }

    fn resolve_dynamic_boundary_at(
        &self,
        file_id: FileId,
        byte_offset: u32,
    ) -> Option<OccurrenceFact> {
        self.dynamic_at.iter().find_map(|(offset, occurrence)| {
            (file_id == FILE && *offset == byte_offset).then(|| occurrence.clone())
        })
    }
}

fn generation() -> ResolveGenerationBasis {
    ResolveGenerationBasis::new(
        SourceGeneration::known("document-1"),
        SourceGeneration::known("workspace-1"),
    )
}

fn entity(id: u64, kind: EntityKind, name: &str, anchor: Option<u64>) -> EntityFact {
    EntityFact {
        id: EntityId(id),
        kind,
        canonical_name: name.to_owned(),
        anchor_id: anchor.map(AnchorId),
        scope_id: Some(ScopeId(id)),
        provenance: Provenance::ExactAst,
        confidence: Confidence::High,
    }
}

fn occurrence(
    id: u64,
    kind: OccurrenceKind,
    entity_id: Option<u64>,
    anchor: u64,
) -> OccurrenceFact {
    OccurrenceFact {
        id: OccurrenceId(id),
        kind,
        entity_id: entity_id.map(EntityId),
        anchor_id: AnchorId(anchor),
        scope_id: Some(ScopeId(id)),
        provenance: Provenance::ExactAst,
        confidence: Confidence::High,
    }
}

fn resolve(source: &StubSource, offset: u32) -> ResolveAtOutcome {
    resolve_at_position(source, FILE, offset, &generation(), false)
}

fn expect_exact(outcome: &ResolveAtOutcome) -> &ResolvedOccurrence {
    match outcome {
        ResolveAtOutcome::Exact(resolved) => resolved,
        other => panic!("expected an exact identity, got {other:?}"),
    }
}

// ── Exact identity ──

#[test]
fn exact_occurrence_carries_occurrence_entity_and_generation() {
    let source = StubSource::default().with_symbol(
        10,
        entity(1, EntityKind::Variable, "$value", Some(100)),
        occurrence(50, OccurrenceKind::Read, Some(1), 101),
    );

    let outcome = resolve(&source, 10);
    let resolved = expect_exact(&outcome);

    assert_eq!(resolved.occurrence_id, OccurrenceId(50));
    assert_eq!(resolved.entity_id, EntityId(1));
    assert_eq!(resolved.role, OccurrenceKind::Read);
    assert_eq!(resolved.occurrence_anchor_id, AnchorId(101));
    assert_eq!(resolved.entity_anchor_id, Some(AnchorId(100)));
    assert_eq!(resolved.generation, generation());
    assert!(resolved.limitations.is_empty());
    assert_eq!(outcome.stage(), "exact");
}

/// Negative control for the whole layer: nested same-name lexicals are two
/// bindings, so the same spelling at two offsets must not collapse to one
/// identity. A spelling-based implementation fails here.
#[test]
fn nested_same_name_lexicals_resolve_to_distinct_bindings() {
    let source = StubSource::default()
        .with_symbol(
            10,
            entity(1, EntityKind::Variable, "$value", Some(100)),
            occurrence(50, OccurrenceKind::Read, Some(1), 101),
        )
        .with_symbol(
            40,
            entity(2, EntityKind::Variable, "$value", Some(200)),
            occurrence(51, OccurrenceKind::Read, Some(2), 201),
        );

    let outer = resolve(&source, 10);
    let inner = resolve(&source, 40);

    assert_eq!(expect_exact(&outer).canonical_name, expect_exact(&inner).canonical_name);
    assert_ne!(expect_exact(&outer).entity_id, expect_exact(&inner).entity_id);
    assert_ne!(expect_exact(&outer).occurrence_id, expect_exact(&inner).occurrence_id);
    assert!(
        !outer.shares_subject_with(&inner),
        "shadowed lexicals must not be reported as one shared subject"
    );
}

/// Same spelling in two packages, and the same spelling in two roots, are
/// distinct entities. Only the offset may decide which one the cursor selected.
#[test]
fn same_spelling_in_two_packages_resolves_by_position_not_name() {
    let source = StubSource::default()
        .with_symbol(
            10,
            entity(1, EntityKind::Subroutine, "Alpha::run", Some(100)),
            occurrence(50, OccurrenceKind::Call, Some(1), 101),
        )
        .with_symbol(
            40,
            entity(2, EntityKind::Subroutine, "Beta::run", Some(200)),
            occurrence(51, OccurrenceKind::Call, Some(2), 201),
        );

    assert_eq!(expect_exact(&resolve(&source, 10)).entity_id, EntityId(1));
    assert_eq!(expect_exact(&resolve(&source, 40)).entity_id, EntityId(2));
}

/// Qualified and unqualified occurrences of one entity share the entity but
/// remain separate occurrences.
#[test]
fn qualified_and_unqualified_occurrences_share_one_entity() {
    let shared = entity(1, EntityKind::Subroutine, "Alpha::run", Some(100));
    let source = StubSource::default()
        .with_symbol(10, shared.clone(), occurrence(50, OccurrenceKind::Call, Some(1), 101))
        .with_symbol(40, shared, occurrence(51, OccurrenceKind::Call, Some(1), 201));

    let qualified = resolve(&source, 10);
    let unqualified = resolve(&source, 40);

    assert_eq!(expect_exact(&qualified).entity_id, expect_exact(&unqualified).entity_id);
    assert_ne!(expect_exact(&qualified).occurrence_id, expect_exact(&unqualified).occurrence_id);
}

/// Declaration and write roles are preserved; role is part of the resolved
/// subject, not something each provider re-derives.
#[test]
fn occurrence_roles_are_preserved() {
    let source = StubSource::default()
        .with_symbol(
            10,
            entity(1, EntityKind::Variable, "$value", Some(100)),
            occurrence(50, OccurrenceKind::Definition, Some(1), 101),
        )
        .with_symbol(
            40,
            entity(1, EntityKind::Variable, "$value", Some(100)),
            occurrence(51, OccurrenceKind::Write, Some(1), 201),
        );

    assert_eq!(expect_exact(&resolve(&source, 10)).role, OccurrenceKind::Definition);
    assert_eq!(expect_exact(&resolve(&source, 40)).role, OccurrenceKind::Write);
}

// ── Non-exact states stay mechanically distinct ──

/// Negative control: an occurrence with no entity must never be promoted to an
/// exact identity by finding a same-name definition. This is the precise error
/// the layer exists to prevent.
#[test]
fn occurrence_without_entity_is_partial_not_exact() {
    let source = StubSource::default().with_symbol(
        10,
        entity(1, EntityKind::Variable, "$value", Some(100)),
        occurrence(50, OccurrenceKind::Read, None, 101),
    );

    let outcome = resolve(&source, 10);

    assert_eq!(outcome.stage(), "partial");
    assert!(outcome.exact().is_none(), "an unresolved entity must not be exact");
    match outcome {
        ResolveAtOutcome::Partial { limitations, .. } => {
            assert!(limitations.contains(&ResolveLimitation::OccurrenceWithoutEntity));
        }
        other => panic!("expected Partial, got {other:?}"),
    }
}

/// A dynamic method selector stays an explicit boundary rather than resolving to
/// whichever receiver class happens to define that name.
#[test]
fn dynamic_boundary_occurrence_is_dynamic_not_exact() {
    let source = StubSource::default().with_symbol(
        10,
        entity(1, EntityKind::Method, "run", Some(100)),
        occurrence(50, OccurrenceKind::DynamicBoundary, Some(1), 101),
    );

    let outcome = resolve(&source, 10);

    assert_eq!(outcome.stage(), "dynamic");
    assert_eq!(outcome.reason(), Some("dynamic_selector"));
    assert!(outcome.exact().is_none());
}

/// A dynamic boundary covering a position with no published occurrence is a
/// different fact from "nothing is here" — but only for a caller that opted in
/// to the second query.
#[test]
fn dynamic_boundary_without_occurrence_is_not_unavailable_when_consulted() {
    let source = StubSource::default()
        .with_dynamic(10, occurrence(90, OccurrenceKind::DynamicBoundary, None, 900));

    let outcome =
        resolve_at_position_with_dynamic_boundary(&source, FILE, 10, &generation(), false);

    assert_eq!(outcome.stage(), "dynamic");
    match outcome {
        ResolveAtOutcome::Dynamic { occurrence_id, .. } => {
            assert_eq!(occurrence_id, OccurrenceId(90));
        }
        other => panic!("expected Dynamic, got {other:?}"),
    }
}

/// The base rule asks the semantic layer exactly one question. A caller whose
/// receipts distinguish these cases must not have a second query forced on it.
#[test]
fn base_rule_does_not_consult_the_dynamic_boundary_producer() {
    let source = StubSource::default()
        .with_dynamic(10, occurrence(90, OccurrenceKind::DynamicBoundary, None, 900));

    assert_eq!(resolve(&source, 10).stage(), "unavailable");
}

/// Opting in does not override a published occurrence.
#[test]
fn dynamic_consultation_does_not_override_a_published_occurrence() {
    let source = StubSource::default()
        .with_symbol(
            10,
            entity(1, EntityKind::Variable, "$value", Some(100)),
            occurrence(50, OccurrenceKind::Read, Some(1), 101),
        )
        .with_dynamic(10, occurrence(90, OccurrenceKind::DynamicBoundary, None, 900));

    let outcome =
        resolve_at_position_with_dynamic_boundary(&source, FILE, 10, &generation(), false);

    assert_eq!(outcome.stage(), "exact");
    assert_eq!(outcome.bound_entity_id(), Some(EntityId(1)));
}

/// A generated member with no source body keeps its generated identity and is
/// never given a fabricated body range.
#[test]
fn generated_member_without_source_body_records_its_limitation() {
    let source = StubSource::default().with_symbol(
        10,
        entity(1, EntityKind::GeneratedMember, "name", None),
        occurrence(50, OccurrenceKind::MethodCall, Some(1), 101),
    );

    let outcome = resolve(&source, 10);
    let resolved = expect_exact(&outcome);

    assert_eq!(resolved.entity_anchor_id, None, "no fabricated body range");
    assert!(resolved.limitations.contains(&ResolveLimitation::GeneratedWithoutSourceBody));
}

/// Heuristic or search-fallback provenance is not exact evidence. A name-scan
/// producer cannot launder a spelling match into an exact identity.
#[test]
fn non_exact_provenance_is_partial_not_exact() {
    for provenance in [Provenance::NameHeuristic, Provenance::SearchFallback] {
        let mut fact = occurrence(50, OccurrenceKind::Read, Some(1), 101);
        fact.provenance = provenance;
        let source = StubSource::default().with_symbol(
            10,
            entity(1, EntityKind::Variable, "$value", Some(100)),
            fact,
        );

        let outcome = resolve(&source, 10);

        assert_eq!(outcome.stage(), "partial", "{provenance:?} must not be exact");
        match outcome {
            ResolveAtOutcome::Partial { limitations, .. } => {
                assert!(limitations.contains(&ResolveLimitation::NonExactProvenance));
            }
            other => panic!("expected Partial for {provenance:?}, got {other:?}"),
        }
    }
}

/// Low confidence is not exact evidence either.
#[test]
fn low_confidence_occurrence_is_partial_not_exact() {
    let mut fact = occurrence(50, OccurrenceKind::Read, Some(1), 101);
    fact.confidence = Confidence::Low;
    let source =
        StubSource::default().with_symbol(10, entity(1, EntityKind::Variable, "$v", Some(1)), fact);

    assert_eq!(resolve(&source, 10).stage(), "partial");
}

/// "Could not resolve the cursor" is distinct from "resolved, and downstream is
/// genuinely empty".
#[test]
fn no_occurrence_is_unavailable_not_exact_empty() {
    let outcome = resolve(&StubSource::default(), 10);

    assert_eq!(outcome.stage(), "unavailable");
    assert_eq!(outcome.reason(), Some("no_occurrence_at_position"));
    assert!(outcome.generation().is_none(), "a state that never queried carries no basis");
}

/// A stale accepted view is checked before any query, so it can never produce an
/// identity that later looks exact.
#[test]
fn stale_view_short_circuits_before_any_query() {
    let source = StubSource::default().with_symbol(
        10,
        entity(1, EntityKind::Variable, "$value", Some(100)),
        occurrence(50, OccurrenceKind::Read, Some(1), 101),
    );

    let outcome = resolve_at_position(&source, FILE, 10, &generation(), true);

    assert_eq!(outcome.stage(), "stale");
    assert!(outcome.exact().is_none(), "a stale view must not yield an identity");
}

// ── Accessors relied on for behaviour-preserving provider adoption ──

/// `bound_entity_id` reports what the producer published without asserting the
/// identity is exact, so a caller that already accepted sub-exact evidence keeps
/// the same answer while sharing this resolution stage.
#[test]
fn bound_entity_id_matches_the_published_occurrence_entity() {
    let mut heuristic = occurrence(50, OccurrenceKind::Read, Some(4), 101);
    heuristic.provenance = Provenance::NameHeuristic;

    let exact = StubSource::default().with_symbol(
        10,
        entity(3, EntityKind::Variable, "$v", Some(1)),
        occurrence(50, OccurrenceKind::Read, Some(3), 101),
    );
    let sub_exact = StubSource::default().with_symbol(
        10,
        entity(4, EntityKind::Variable, "$v", Some(1)),
        heuristic,
    );
    let no_entity = StubSource::default().with_symbol(
        10,
        entity(5, EntityKind::Variable, "$v", Some(1)),
        occurrence(50, OccurrenceKind::Read, None, 101),
    );

    assert_eq!(resolve(&exact, 10).bound_entity_id(), Some(EntityId(3)));
    assert_eq!(resolve(&sub_exact, 10).bound_entity_id(), Some(EntityId(4)));
    assert_eq!(resolve(&no_entity, 10).bound_entity_id(), None);
    assert_eq!(resolve(&StubSource::default(), 10).bound_entity_id(), None);
}

/// Ambiguity must never be silently collapsed to one identity.
#[test]
fn ambiguous_outcome_yields_no_bound_entity() {
    assert_eq!(ResolveAtOutcome::Ambiguous(Vec::new()).bound_entity_id(), None);
    assert!(ResolveAtOutcome::Ambiguous(Vec::new()).published_occurrence().is_none());
}

/// `published_occurrence` exposes the occurrence's role and anchor so a caller
/// need not re-query the semantic layer for facts this resolution already read.
#[test]
fn published_occurrence_exposes_role_and_anchor_without_a_second_query() {
    let mut heuristic = occurrence(51, OccurrenceKind::Definition, Some(2), 201);
    heuristic.provenance = Provenance::NameHeuristic;

    let exact = StubSource::default().with_symbol(
        10,
        entity(1, EntityKind::Variable, "$v", Some(1)),
        occurrence(50, OccurrenceKind::Definition, Some(1), 101),
    );
    let sub_exact = StubSource::default().with_symbol(
        10,
        entity(2, EntityKind::Variable, "$v", Some(2)),
        heuristic,
    );

    let from_exact = resolve(&exact, 10);
    let published = from_exact.published_occurrence().expect("exact publishes an occurrence");
    assert_eq!(published.role, OccurrenceKind::Definition);
    assert_eq!(published.occurrence_anchor_id, AnchorId(101));

    // A sub-exact occurrence is still a published occurrence: the caller that
    // reads the declaration anchor must see it exactly as it did before.
    let from_partial = resolve(&sub_exact, 10);
    let published = from_partial.published_occurrence().expect("partial publishes an occurrence");
    assert_eq!(published.role, OccurrenceKind::Definition);
    assert_eq!(published.occurrence_anchor_id, AnchorId(201));

    // Nothing published means nothing to read.
    assert!(resolve(&StubSource::default(), 10).published_occurrence().is_none());
    assert!(ResolveAtOutcome::Stale.published_occurrence().is_none());
}

/// `occurrence_was_published` separates "a producer published an occurrence but
/// it carried no entity" from "nothing is here" — the distinction the
/// entity-resolution canaries report.
#[test]
fn occurrence_was_published_tracks_producer_output_not_exactness() {
    let with_entity = StubSource::default().with_symbol(
        10,
        entity(1, EntityKind::Variable, "$v", Some(1)),
        occurrence(50, OccurrenceKind::Read, Some(1), 101),
    );
    let without_entity = StubSource::default().with_symbol(
        10,
        entity(1, EntityKind::Variable, "$v", Some(1)),
        occurrence(50, OccurrenceKind::Read, None, 101),
    );

    assert!(resolve(&with_entity, 10).occurrence_was_published());
    assert!(resolve(&without_entity, 10).occurrence_was_published());
    assert!(!resolve(&StubSource::default(), 10).occurrence_was_published());
    assert!(!ResolveAtOutcome::Stale.occurrence_was_published());
}

/// Every state has a distinct stage identifier, so a receipt can never conflate
/// two of them.
#[test]
fn every_outcome_stage_is_distinct() {
    let stages = [
        ResolveAtOutcome::Exact(
            expect_exact(&resolve(
                &StubSource::default().with_symbol(
                    10,
                    entity(1, EntityKind::Variable, "$v", Some(1)),
                    occurrence(50, OccurrenceKind::Read, Some(1), 2),
                ),
                10,
            ))
            .clone(),
        )
        .stage(),
        ResolveAtOutcome::Ambiguous(Vec::new()).stage(),
        ResolveAtOutcome::Dynamic {
            boundary: ResolveLimitation::DynamicSelector,
            occurrence_id: OccurrenceId(1),
            entity_id: None,
            generation: generation(),
        }
        .stage(),
        ResolveAtOutcome::Partial {
            candidates: Vec::new(),
            limitations: Vec::new(),
            generation: generation(),
        }
        .stage(),
        ResolveAtOutcome::NotReady(ResolveNotReady::WorkspaceIndexUnavailable).stage(),
        ResolveAtOutcome::Stale.stage(),
        ResolveAtOutcome::Unavailable(ResolveUnavailable::ByteOffsetOutOfRange).stage(),
        ResolveAtOutcome::InstrumentFailure("boom").stage(),
    ];

    let mut unique = stages.to_vec();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), stages.len(), "outcome stages must be mechanically distinct");
}

/// Not-ready reasons stay separated from "no occurrence here": the first may
/// succeed unchanged later, the second will not.
#[test]
fn not_ready_is_distinct_from_unavailable() {
    let not_ready = ResolveAtOutcome::NotReady(ResolveNotReady::SemanticQueriesUnavailable);
    let unavailable = ResolveAtOutcome::Unavailable(ResolveUnavailable::NoOccurrenceAtPosition);

    assert_ne!(not_ready.stage(), unavailable.stage());
    assert_eq!(not_ready.reason(), Some("semantic_queries_unavailable"));
    assert_eq!(unavailable.reason(), Some("no_occurrence_at_position"));
}

// ── Generation basis ──

/// Two providers answering one request must resolve the cursor against the same
/// generations. This is the mechanical check that proves it.
#[test]
fn same_basis_is_shared_and_different_basis_is_not() {
    let source = StubSource::default().with_symbol(
        10,
        entity(1, EntityKind::Variable, "$value", Some(100)),
        occurrence(50, OccurrenceKind::Read, Some(1), 101),
    );

    let definition = resolve(&source, 10);
    let references = resolve(&source, 10);
    assert!(definition.shares_generation_with(&references));
    assert!(definition.shares_subject_with(&references));

    let later_workspace = ResolveGenerationBasis::new(
        SourceGeneration::known("document-1"),
        SourceGeneration::known("workspace-2"),
    );
    let drifted = resolve_at_position(&source, FILE, 10, &later_workspace, false);

    assert!(
        !definition.shares_generation_with(&drifted),
        "a different accepted workspace generation must be detectable"
    );
    assert!(
        definition.shares_subject_with(&drifted),
        "the subject is unchanged even though the basis drifted"
    );
}

/// States that never reached the semantic view share no basis, so the check is
/// false rather than vacuously true.
#[test]
fn states_without_a_basis_do_not_report_a_shared_generation() {
    let unavailable = resolve(&StubSource::default(), 10);
    let stale = ResolveAtOutcome::Stale;

    assert!(!unavailable.shares_generation_with(&stale));
    assert!(!stale.shares_generation_with(&stale));
}

// ── Torn-read protocol around the generation basis ──

/// A quiet index yields a known basis naming the observed write version.
#[test]
fn a_stable_write_version_yields_a_known_basis() {
    let basis = stable_generation_basis(|| 7, || Some(3), "file:///a.pm", 3);

    assert!(basis.is_known());
    assert_eq!(basis.document_generation, SourceGeneration::known("file:///a.pm@3"));
    assert_eq!(basis.workspace_generation, SourceGeneration::known("workspace-index@7"));
}

/// A write that lands between the two halves is retried, and the basis that
/// survives names one snapshot — never a document generation from before the
/// write paired with a workspace version from after it.
#[test]
fn a_write_landing_mid_read_is_retried_until_the_pair_is_stable() {
    use std::cell::Cell;

    // Version moves during the first attempt, then settles.
    let reads = Cell::new(0u32);
    let version = Cell::new(10u64);
    let basis = stable_generation_basis(
        || {
            let seen = reads.get();
            reads.set(seen + 1);
            // Reads 0 and 1 straddle the first attempt: bump between them.
            if seen == 1 {
                version.set(11);
            }
            version.get()
        },
        || Some(4),
        "file:///a.pm",
        3,
    );

    assert!(basis.is_known(), "a settled index must still yield a usable basis");
    assert_eq!(
        basis.workspace_generation,
        SourceGeneration::known("workspace-index@11"),
        "the surviving basis must name the settled version, not the pre-write one"
    );
}

/// An index that never settles yields an explicit unknown basis rather than a
/// fabricated one. This is the control that matters: a torn read must never be
/// laundered into a basis that looks exact.
#[test]
fn an_index_that_never_settles_yields_an_explicit_unknown_basis() {
    use std::cell::Cell;

    let version = Cell::new(0u64);
    let basis = stable_generation_basis(
        || {
            // Every read observes a different version, so no pair is stable.
            version.set(version.get() + 1);
            version.get()
        },
        || Some(4),
        "file:///a.pm",
        3,
    );

    assert!(!basis.is_known(), "an unstable read must not claim a known basis");
    assert_eq!(basis.document_generation, SourceGeneration::Unknown);
    assert_eq!(basis.workspace_generation, SourceGeneration::Unknown);
}

/// The protocol is bounded: it does not spin forever on a busy index.
#[test]
fn the_torn_read_protocol_is_bounded() {
    use std::cell::Cell;

    let version = Cell::new(0u64);
    let reads = Cell::new(0u32);
    let _ = stable_generation_basis(
        || {
            reads.set(reads.get() + 1);
            version.set(version.get() + 1);
            version.get()
        },
        || Some(1),
        "file:///a.pm",
        3,
    );

    // Two version reads per attempt, three attempts.
    assert_eq!(reads.get(), 6, "the protocol must stop after its attempt bound");
}

/// A uri the index has never seen still yields an explicit unknown document
/// generation, while the workspace half stays known.
#[test]
fn an_unseen_uri_yields_an_unknown_document_generation_with_a_known_workspace() {
    let basis = stable_generation_basis(|| 2, || None, "file:///absent.pm", 3);

    assert!(!basis.is_known());
    assert_eq!(basis.document_generation, SourceGeneration::Unknown);
    assert_eq!(basis.workspace_generation, SourceGeneration::known("workspace-index@2"));
}

/// A source that hands back an entity which is not the one the occurrence binds
/// must be refused, not silently combined. Otherwise the result would carry one
/// entity's id alongside another's kind, name, anchor, and evidence — an
/// "exact" identity describing no real entity at all.
#[test]
fn a_source_pairing_mismatched_facts_is_refused() {
    let source = StubSource::default().with_symbol(
        10,
        // This entity is #2 …
        entity(2, EntityKind::Subroutine, "Beta::run", Some(200)),
        // … but the occurrence binds #1.
        occurrence(50, OccurrenceKind::Call, Some(1), 101),
    );

    let outcome = resolve(&source, 10);

    assert_eq!(outcome.stage(), "instrument_failure");
    assert_eq!(outcome.reason(), Some("source_entity_occurrence_mismatch"));
    assert!(outcome.exact().is_none(), "a hybrid identity must never be exact");
    assert_eq!(outcome.bound_entity_id(), None, "no entity may be reported from a mismatched pair");
    assert!(!outcome.occurrence_was_published());
}

/// The check is an equality test on identity, not a coincidence of the fixtures:
/// the same pair with matching ids resolves normally.
#[test]
fn a_source_pairing_matched_facts_still_resolves() {
    let source = StubSource::default().with_symbol(
        10,
        entity(1, EntityKind::Subroutine, "Alpha::run", Some(100)),
        occurrence(50, OccurrenceKind::Call, Some(1), 101),
    );

    assert_eq!(resolve(&source, 10).stage(), "exact");
}

// ── Exactness depends on both halves of the identity, and on the basis ──

/// The identity is occurrence *and* entity, so a weak entity cannot be laundered
/// into an exact identity by a strong occurrence.
#[test]
fn a_weak_entity_is_not_an_exact_identity() {
    for (provenance, confidence) in [
        (Provenance::NameHeuristic, Confidence::High),
        (Provenance::SearchFallback, Confidence::High),
        (Provenance::ExactAst, Confidence::Low),
    ] {
        let mut weak = entity(1, EntityKind::Variable, "$value", Some(100));
        weak.provenance = provenance;
        weak.confidence = confidence;
        // The occurrence itself is impeccable.
        let source = StubSource::default().with_symbol(
            10,
            weak,
            occurrence(50, OccurrenceKind::Read, Some(1), 101),
        );

        let outcome = resolve(&source, 10);

        assert_eq!(
            outcome.stage(),
            "partial",
            "entity evidence {provenance:?}/{confidence:?} must not be exact"
        );
        match outcome {
            ResolveAtOutcome::Partial { limitations, .. } => assert!(
                limitations.contains(&ResolveLimitation::NonExactEntityProvenance),
                "the weak half must be named"
            ),
            other => panic!("expected Partial, got {other:?}"),
        }
    }
}

/// The two halves are reported separately, so a caller can tell which one is
/// approximate rather than being told only that something is.
#[test]
fn weak_occurrence_and_weak_entity_are_named_separately() {
    let mut weak_entity_fact = entity(1, EntityKind::Variable, "$value", Some(100));
    weak_entity_fact.provenance = Provenance::NameHeuristic;
    let mut weak_occurrence = occurrence(50, OccurrenceKind::Read, Some(1), 101);
    weak_occurrence.provenance = Provenance::SearchFallback;

    let entity_only = StubSource::default().with_symbol(
        10,
        weak_entity_fact,
        occurrence(50, OccurrenceKind::Read, Some(1), 101),
    );
    let occurrence_only = StubSource::default().with_symbol(
        10,
        entity(1, EntityKind::Variable, "$value", Some(100)),
        weak_occurrence,
    );

    match resolve(&entity_only, 10) {
        ResolveAtOutcome::Partial { limitations, .. } => {
            assert!(limitations.contains(&ResolveLimitation::NonExactEntityProvenance));
            assert!(!limitations.contains(&ResolveLimitation::NonExactProvenance));
        }
        other => panic!("expected Partial, got {other:?}"),
    }
    match resolve(&occurrence_only, 10) {
        ResolveAtOutcome::Partial { limitations, .. } => {
            assert!(limitations.contains(&ResolveLimitation::NonExactProvenance));
            assert!(!limitations.contains(&ResolveLimitation::NonExactEntityProvenance));
        }
        other => panic!("expected Partial, got {other:?}"),
    }
}

/// An identity whose snapshot is unidentified is not exact, however strong the
/// producer evidence. Without a known basis a caller cannot say which source the
/// identity came from, nor detect drift by comparing bases.
#[test]
fn an_unknown_basis_cannot_produce_an_exact_identity() {
    let source = StubSource::default().with_symbol(
        10,
        entity(1, EntityKind::Variable, "$value", Some(100)),
        occurrence(50, OccurrenceKind::Read, Some(1), 101),
    );

    for basis in [
        ResolveGenerationBasis::new(SourceGeneration::Unknown, SourceGeneration::Unknown),
        ResolveGenerationBasis::new(SourceGeneration::Unknown, SourceGeneration::known("w")),
        ResolveGenerationBasis::new(SourceGeneration::known("d"), SourceGeneration::Unknown),
    ] {
        let outcome = resolve_at_position(&source, FILE, 10, &basis, false);

        assert_eq!(outcome.stage(), "partial", "unknown basis {basis:?} must not be exact");
        assert!(outcome.exact().is_none());
        match outcome {
            ResolveAtOutcome::Partial { limitations, candidates, .. } => {
                assert!(limitations.contains(&ResolveLimitation::UnknownGeneration));
                // The identity is still carried, so a caller that already
                // accepted sub-exact evidence keeps the same answer.
                assert_eq!(candidates.first().map(|first| first.entity_id), Some(EntityId(1)));
            }
            other => panic!("expected Partial, got {other:?}"),
        }
    }
}

/// The degraded state stays usable: `bound_entity_id` still reports the entity,
/// so references' acceptance is unchanged by the basis gate.
#[test]
fn an_unknown_basis_still_reports_the_bound_entity() {
    let source = StubSource::default().with_symbol(
        10,
        entity(1, EntityKind::Variable, "$value", Some(100)),
        occurrence(50, OccurrenceKind::Read, Some(1), 101),
    );
    let unknown = ResolveGenerationBasis::new(SourceGeneration::Unknown, SourceGeneration::Unknown);

    let outcome = resolve_at_position(&source, FILE, 10, &unknown, false);

    assert_eq!(outcome.bound_entity_id(), Some(EntityId(1)));
    assert!(outcome.occurrence_was_published());
}

/// An unknown generation is explicit and never counts as a known basis.
#[test]
fn unknown_generation_is_not_a_known_basis() {
    assert!(generation().is_known());
    assert!(
        !ResolveGenerationBasis::new(SourceGeneration::Unknown, SourceGeneration::known("w"))
            .is_known()
    );
    assert!(
        !ResolveGenerationBasis::new(SourceGeneration::known("d"), SourceGeneration::Unknown)
            .is_known()
    );
}

/// The file identity is part of the subject: the same offset in another file is
/// not this cursor. Guards the "another root's same-name entity" control.
#[test]
fn another_file_at_the_same_offset_does_not_satisfy_the_request() {
    let source = StubSource::default().with_symbol(
        10,
        entity(1, EntityKind::Variable, "$value", Some(100)),
        occurrence(50, OccurrenceKind::Read, Some(1), 101),
    );

    let other_file = resolve_at_position(&source, FileId(99), 10, &generation(), false);

    assert_eq!(other_file.stage(), "unavailable");
}

// ── The reported scope is the occurrence's use site, never the declaration ──

/// `scope_id` is documented as the scope *containing the occurrence*. The
/// entity's scope is where it was declared, which for an import, a package
/// member, or a shadowed lexical is a different scope entirely. An occurrence
/// that published no scope must stay absent rather than borrow the entity's,
/// or an exact identity would carry a use site the cursor never had.
#[test]
fn a_missing_occurrence_scope_is_not_filled_from_the_entity() {
    let mut declared_elsewhere = entity(1, EntityKind::Variable, "$value", Some(100));
    declared_elsewhere.scope_id = Some(ScopeId(77));
    let mut used_here = occurrence(50, OccurrenceKind::Read, Some(1), 101);
    used_here.scope_id = None;

    let source = StubSource::default().with_symbol(10, declared_elsewhere, used_here);

    let outcome = resolve(&source, 10);

    assert_eq!(
        expect_exact(&outcome).scope_id,
        None,
        "an unpublished occurrence scope must stay absent, not borrow ScopeId(77) from the entity"
    );
}

/// Positive control on the same shape: when the occurrence does publish a
/// scope, that scope is reported even though the entity names a different one.
/// Pins the rule as "the occurrence's scope" rather than "whichever is Some".
#[test]
fn a_published_occurrence_scope_wins_over_a_different_entity_scope() {
    let mut declared_elsewhere = entity(1, EntityKind::Variable, "$value", Some(100));
    declared_elsewhere.scope_id = Some(ScopeId(77));
    let mut used_here = occurrence(50, OccurrenceKind::Read, Some(1), 101);
    used_here.scope_id = Some(ScopeId(88));

    let source = StubSource::default().with_symbol(10, declared_elsewhere, used_here);

    let outcome = resolve(&source, 10);

    assert_eq!(expect_exact(&outcome).scope_id, Some(ScopeId(88)));
}
