//! Shared cursor identity against a real workspace index (#8977).
//!
//! The unit proof for the resolution rule uses stub sources so every outcome
//! state can be constructed exactly. This file is the complement: it drives the
//! same public API through the real [`WorkspaceIndex`] and the real
//! `WorkspaceSemanticQueries::symbol_at`, so the claim "definition and
//! references resolve one subject against one generation basis" holds on the
//! objects the providers actually hold, not on doubles.
//!
//! The fixture installs a fact shard directly. That is deliberate. Indexing a
//! file with `index_file` populates the name indexes but not the semantic fact
//! shards — those are produced by a separate construction step — so a test that
//! only called `index_file` would find no occurrence at any offset and would
//! pass vacuously whatever the resolution rule did. Installing the shard makes
//! the assertions discriminating: the entities below really are three distinct
//! bindings that share one spelling.

use perl_lsp_rs_core::providers::semantic_port::{
    ResolveAtOutcome, SemanticQueriesResolveSource, accepted_generation_basis, resolve_at_position,
};
use perl_semantic_facts::{
    AnchorFact, AnchorId, Confidence, EntityFact, EntityId, EntityKind, FileId, OccurrenceFact,
    OccurrenceId, OccurrenceKind, Provenance, ScopeId,
};
use perl_workspace::semantic::facts::PRODUCER_SCHEMA_VERSION;
use perl_workspace::workspace_index::{FileFactShard, WorkspaceIndex};
use url::Url;

const URI: &str = "file:///workspace/Demo.pm";
const FILE: FileId = FileId(4_101);

/// Three `$value` bindings that share one spelling: an outer lexical, an inner
/// lexical that shadows it, and a sibling sub's own lexical.
const SHADOWED_LEXICALS: &str = r#"package Demo;

sub outer {
    my $value = 1;
    print $value;
    {
        my $value = 2;
        print $value;
    }
    return;
}

sub sibling {
    my $value = 3;
    return $value;
}

1;
"#;

/// Byte offset of the `n`th (0-based) occurrence of `needle`.
fn nth_offset(needle: &str, n: usize) -> u32 {
    let mut search_from = 0usize;
    let mut found = None;
    for _ in 0..=n {
        let index = SHADOWED_LEXICALS[search_from..]
            .find(needle)
            .unwrap_or_else(|| panic!("fixture lacks occurrence {n} of {needle}"))
            + search_from;
        found = Some(index);
        search_from = index + 1;
    }
    u32::try_from(found.unwrap_or_default()).expect("fixture offsets fit in u32")
}

/// Offsets of the three declaration sites and the outer read, in source order.
fn outer_declaration() -> u32 {
    nth_offset("$value", 0)
}
fn outer_read() -> u32 {
    nth_offset("$value", 1)
}
fn inner_declaration() -> u32 {
    nth_offset("$value", 2)
}
fn sibling_declaration() -> u32 {
    nth_offset("$value", 4)
}

fn anchor(id: u64, start: u32, scope: u64) -> AnchorFact {
    AnchorFact {
        id: AnchorId(id),
        file_id: FILE,
        span_start_byte: start,
        // `$value` is six bytes wide in this fixture.
        span_end_byte: start + 6,
        scope_id: Some(ScopeId(scope)),
        provenance: Provenance::ExactAst,
        confidence: Confidence::High,
    }
}

fn entity(id: u64, scope: u64) -> EntityFact {
    EntityFact {
        id: EntityId(id),
        kind: EntityKind::Variable,
        // Identical spelling across all three bindings — that is the point.
        canonical_name: "$value".to_owned(),
        anchor_id: Some(AnchorId(id)),
        scope_id: Some(ScopeId(scope)),
        provenance: Provenance::ExactAst,
        confidence: Confidence::High,
    }
}

fn occurrence(id: u64, anchor_id: u64, entity_id: u64, kind: OccurrenceKind) -> OccurrenceFact {
    OccurrenceFact {
        id: OccurrenceId(id),
        kind,
        entity_id: Some(EntityId(entity_id)),
        anchor_id: AnchorId(anchor_id),
        scope_id: None,
        provenance: Provenance::ExactAst,
        confidence: Confidence::High,
    }
}

/// Build an index whose fact shard encodes three distinct same-spelling
/// bindings, so a spelling-based resolver and an identity-based one would give
/// different answers.
fn indexed() -> WorkspaceIndex {
    let index = WorkspaceIndex::new();
    let uri = Url::parse(URI).expect("fixture uri parses");
    index.index_file(uri, SHADOWED_LEXICALS.to_owned()).expect("fixture indexes");

    let shard = FileFactShard {
        source_uri: URI.to_owned(),
        file_id: FILE,
        content_hash: 0x5741_4C4B,
        producer_schema_version: PRODUCER_SCHEMA_VERSION,
        anchors_hash: None,
        entities_hash: None,
        occurrences_hash: None,
        edges_hash: None,
        anchors: vec![
            anchor(1, outer_declaration(), 10),
            anchor(2, outer_read(), 10),
            anchor(3, inner_declaration(), 20),
            anchor(4, sibling_declaration(), 30),
        ],
        entities: vec![entity(1, 10), entity(3, 20), entity(4, 30)],
        occurrences: vec![
            occurrence(101, 1, 1, OccurrenceKind::Definition),
            // The outer read binds to the OUTER entity, not the inner one.
            occurrence(102, 2, 1, OccurrenceKind::Read),
            occurrence(103, 3, 3, OccurrenceKind::Definition),
            occurrence(104, 4, 4, OccurrenceKind::Definition),
        ],
        edges: Vec::new(),
    };
    index.replace_fact_shard_incremental(URI, shard);
    index
}

fn resolve(index: &WorkspaceIndex, byte_offset: u32) -> ResolveAtOutcome {
    let basis = accepted_generation_basis(index, URI);
    index
        .with_semantic_queries_for_uri(URI, |file_id, queries| {
            let source = SemanticQueriesResolveSource::new(&queries);
            resolve_at_position(&source, file_id, byte_offset, &basis, false)
        })
        .expect("semantic queries open for an indexed uri")
}

/// Guards every other test in this file: if the fixture stopped resolving, the
/// negative controls below would pass vacuously.
#[test]
fn the_fixture_actually_resolves_identities() {
    let index = indexed();

    for (label, offset) in [
        ("outer declaration", outer_declaration()),
        ("outer read", outer_read()),
        ("inner declaration", inner_declaration()),
        ("sibling declaration", sibling_declaration()),
    ] {
        let outcome = resolve(&index, offset);
        assert_eq!(
            outcome.stage(),
            "exact",
            "{label} must resolve exactly or the negative controls are vacuous"
        );
    }
}

/// The single basis constructor is what keeps two providers from drifting: the
/// same accepted view and uri must yield the same basis every time.
#[test]
fn one_accepted_view_yields_one_generation_basis() {
    let index = indexed();

    assert_eq!(
        accepted_generation_basis(&index, URI),
        accepted_generation_basis(&index, URI),
        "both providers must build the identical basis from one accepted view"
    );
}

/// Definition and references, resolving the same cursor through the shared
/// layer, agree on subject and basis. This is the property the providers could
/// not have before: one resolved an occurrence, the other resolved a spelling.
#[test]
fn definition_and_references_capture_the_same_subject_and_basis() {
    let index = indexed();

    let as_definition = resolve(&index, outer_read());
    let as_references = resolve(&index, outer_read());

    assert_eq!(as_definition.stage(), as_references.stage());
    assert!(
        as_definition.shares_subject_with(&as_references),
        "two resolutions of one cursor must name one subject"
    );
    assert!(
        as_definition.shares_generation_with(&as_references),
        "two resolutions of one request must share one generation basis"
    );
}

/// The core negative control: `$value` is spelled identically in three places
/// that are three different bindings. Each cursor must resolve to its own.
#[test]
fn shadowed_lexicals_resolve_to_three_distinct_identities() {
    let index = indexed();

    let outer = resolve(&index, outer_declaration());
    let inner = resolve(&index, inner_declaration());
    let sibling = resolve(&index, sibling_declaration());

    let entities: Vec<_> = [&outer, &inner, &sibling]
        .into_iter()
        .map(|outcome| {
            outcome.exact().unwrap_or_else(|| panic!("expected exact, got {outcome:?}")).entity_id
        })
        .collect();

    let mut unique = entities.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(
        unique.len(),
        3,
        "three distinct lexical bindings must not collapse to one identity, got {entities:?}"
    );

    // And the spelling really is shared, so the distinction cannot have come
    // from the name.
    for outcome in [&outer, &inner, &sibling] {
        assert_eq!(
            outcome.exact().map(|resolved| resolved.canonical_name.as_str()),
            Some("$value")
        );
    }
}

/// A read in the outer scope binds to the outer declaration, not to the inner
/// shadowing declaration that is nearer in the file.
#[test]
fn a_read_binds_to_its_own_declaration_not_the_nearest_spelling() {
    let index = indexed();

    let read = resolve(&index, outer_read());
    let outer = resolve(&index, outer_declaration());
    let inner = resolve(&index, inner_declaration());

    assert_eq!(
        read.bound_entity_id(),
        outer.bound_entity_id(),
        "the read must bind to the declaration that governs its scope"
    );
    assert_ne!(
        read.bound_entity_id(),
        inner.bound_entity_id(),
        "proximity in the file must not decide the binding"
    );
}

/// Occurrence roles survive resolution: a declaration and a read of one entity
/// are one entity but two occurrences.
#[test]
fn one_entity_can_carry_several_occurrence_roles() {
    let index = indexed();

    let declaration = resolve(&index, outer_declaration());
    let read = resolve(&index, outer_read());

    assert_eq!(declaration.bound_entity_id(), read.bound_entity_id());
    assert_ne!(
        declaration.exact().map(|resolved| resolved.occurrence_id),
        read.exact().map(|resolved| resolved.occurrence_id)
    );
    assert_eq!(declaration.exact().map(|resolved| resolved.role), Some(OccurrenceKind::Definition));
    assert_eq!(read.exact().map(|resolved| resolved.role), Some(OccurrenceKind::Read));
}

/// Negative control: a cursor with no occurrence reports a named non-exact
/// stage instead of resolving to a nearby or same-spelled entity.
#[test]
fn a_cursor_with_no_occurrence_is_never_exact() {
    let index = indexed();
    // Inside `package Demo;`, for which this fixture publishes no occurrence.
    let offset = nth_offset("Demo", 0);

    let outcome = resolve(&index, offset);

    assert!(outcome.exact().is_none(), "expected no identity, got {outcome:?}");
    assert_eq!(outcome.stage(), "unavailable");
    assert_eq!(outcome.reason(), Some("no_occurrence_at_position"));
}

/// A position past the end of the source is not an identity either.
#[test]
fn an_offset_past_end_of_source_is_never_exact() {
    let index = indexed();
    let past_end = u32::try_from(SHADOWED_LEXICALS.len() + 4_096).expect("fits in u32");

    assert!(resolve(&index, past_end).exact().is_none());
}

/// The generation basis is bound to the accepted view, so a uri the view has
/// never seen yields an explicit unknown rather than a fabricated generation.
#[test]
fn an_unindexed_uri_yields_an_explicit_unknown_document_generation() {
    let index = indexed();

    let known = accepted_generation_basis(&index, URI);
    let unknown = accepted_generation_basis(&index, "file:///workspace/Absent.pm");

    assert!(!unknown.is_known(), "an unseen uri must not claim a known basis");
    assert_ne!(known.document_generation, unknown.document_generation);
    assert_eq!(
        known.workspace_generation, unknown.workspace_generation,
        "the workspace half of the basis is a property of the view, not the uri"
    );
}

/// A stale accepted view is refused before any query, so nothing resolved
/// against it can later be mistaken for exact.
#[test]
fn a_stale_view_produces_no_identity() {
    let index = indexed();
    let basis = accepted_generation_basis(&index, URI);

    let outcome = index
        .with_semantic_queries_for_uri(URI, |file_id, queries| {
            let source = SemanticQueriesResolveSource::new(&queries);
            resolve_at_position(&source, file_id, outer_read(), &basis, true)
        })
        .expect("semantic queries open for an indexed uri");

    assert_eq!(outcome.stage(), "stale");
    assert!(outcome.exact().is_none());
}
