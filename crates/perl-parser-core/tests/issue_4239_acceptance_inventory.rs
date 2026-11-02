//! Executable inventory for [#4239](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4239)
//! acceptance on current `main`.
//!
//! This file pins what is already satisfied (registry/receipt honesty, anchor
//! preservation on the lexical extraction path, and explicit unsupported
//! disposition for import/declaration HIR families). It does **not** claim the
//! issue is complete: dedicated declaration/import PIR operation families, a
//! production PIR→envelope adapter for all required fact kinds, and live
//! provider consumption without HIR variant matching remain open.

use perl_parser_core::Parser;
use perl_parser_core::hir::lower_ast;
use perl_parser_core::pir::{
    LexicalRole, PirOperation, PirSourceAnchor, extract_lexical_facts, lower_hir,
};
use perl_semantic_facts::{
    AnchorId, Confidence, FactId, FileId, LifecyclePhase, Provenance, ScopeId, SemanticConfidence,
    SemanticFactEnvelope, SemanticFactKind, SemanticFreshness, SemanticProducer,
    SemanticProvenance, SemanticReasonCode, SourceAnchor, SourceGeneration,
};
use perl_tdd_support::must_some;
use std::collections::BTreeSet;

fn lower_graph(source: &str) -> perl_parser_core::pir::PirGraph {
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    let hir = lower_ast(&output.ast);
    lower_hir(&hir)
}

fn extract(source: &str) -> perl_parser_core::pir::LexicalExtractorReceipt {
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    let hir = lower_ast(&output.ast);
    extract_lexical_facts(&hir)
}

/// Acceptance: PIR registries and receipt operation keys stay aligned with
/// `PirOperation::ALL_OPERATION_NAMES` (PLSP-SPEC-0025).
#[test]
fn operation_registry_matches_receipt_keys() {
    let registered: BTreeSet<&'static str> =
        PirOperation::ALL_OPERATION_NAMES.iter().copied().collect();
    let graph = lower_graph(
        r#"
package P;
use strict;
sub f { my $x = 1; $x++; foo($x); eval "1"; }
"#,
    );
    for key in graph.receipt.operation_counts.keys() {
        assert!(
            registered.contains(key),
            "receipt operation_counts key {key:?} must appear in ALL_OPERATION_NAMES"
        );
    }
}

/// Acceptance: unsupported/deferred HIR families stay receipt-visible (never silent).
#[test]
fn import_and_declaration_hir_families_are_receipt_visible() {
    let graph = lower_graph("package Foo; use strict; sub bar {}");
    let unsupported = &graph.receipt.unsupported_construct_counts;
    assert_eq!(unsupported.get("PackageDecl"), Some(&1));
    assert_eq!(unsupported.get("UseDecl"), Some(&1));
    assert_eq!(unsupported.get("SubDecl"), Some(&1));
}

/// Acceptance (partial transport): PIR source anchors survive through lexical
/// extraction so downstream envelope projection can preserve `AnchorId`.
#[test]
fn lexical_extractor_preserves_pir_anchor_ids() {
    let receipt = extract("my $x = 1; foo($x);");
    let anchored: Vec<&PirSourceAnchor> = receipt
        .bodies
        .iter()
        .flat_map(|body| body.facts.iter())
        .map(|fact| &fact.source_anchor)
        .collect();
    assert!(!anchored.is_empty());
    for anchor in anchored {
        assert!(anchor.is_anchored());
        assert!(
            anchor.anchor_id.is_some(),
            "anchored PIR nodes must retain AnchorId for semantic-fact transport"
        );
    }
}

/// Acceptance (partial #4238 transport): a PIR-derived lexical occurrence can be
/// expressed as a canonical `SemanticFactEnvelope` with producer `PirA` without
/// reopening HIR. Production wiring remains future work; this falsifies silent
/// impossibility of the envelope shape.
#[test]
fn pir_lexical_occurrence_projects_to_semantic_fact_envelope() {
    let receipt = extract("my $x = 1; foo($x);");
    let read = must_some(
        receipt
            .bodies
            .iter()
            .flat_map(|body| body.facts.iter())
            .find(|fact| fact.role == LexicalRole::Read),
    );
    let range = must_some(read.source_anchor.range.as_ref());
    let anchor_id = must_some(read.source_anchor.anchor_id);
    let start_byte = u32::try_from(range.start).expect("test fixture range fits in u32");
    let end_byte = u32::try_from(range.end).expect("test fixture range fits in u32");
    let envelope = SemanticFactEnvelope::new(
        FactId(4239),
        None,
        SemanticFactKind::Occurrence,
        SourceAnchor::new(Some(anchor_id), FileId(1), start_byte, end_byte),
        SourceGeneration::known("generation-1"),
        Some(ScopeId(0)),
        None,
        LifecyclePhase::Runtime,
        SemanticProducer::PirA,
        SemanticProvenance::Known(Provenance::ExactAst),
        SemanticConfidence::Known(Confidence::High),
        SemanticFreshness::Fresh,
        None,
        Vec::new(),
        SemanticReasonCode::ExactSource,
    );
    assert_eq!(envelope.producer, SemanticProducer::PirA);
    assert_eq!(envelope.anchor.anchor_id, Some(anchor_id));
    assert_eq!(envelope.status(), perl_semantic_facts::SemanticFactStatus::Exact);
}
