#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598): census-clean on all targets; new findings move the crate to C1.
use perl_semantic_facts::{
    AnchorId, Confidence, EntityId, FactId, FileId, LifecyclePhase, Provenance, ScopeId,
    SemanticConfidence, SemanticFactEnvelope, SemanticFactKind, SemanticFactStatus,
    SemanticFreshness, SemanticProducer, SemanticProvenance, SemanticReasonCode, SourceAnchor,
    SourceGeneration,
};

#[test]
fn empty_top_level_source_generation_cannot_be_promoted_as_exact() {
    let envelope = SemanticFactEnvelope::new(
        FactId(1),
        Some(EntityId(2)),
        SemanticFactKind::Declaration,
        SourceAnchor::new(Some(AnchorId(3)), FileId(4), 10, 20),
        SourceGeneration::known(""),
        Some(ScopeId(5)),
        Some("Example".to_string()),
        LifecyclePhase::Runtime,
        SemanticProducer::PirA,
        SemanticProvenance::Known(Provenance::ExactAst),
        SemanticConfidence::Known(Confidence::High),
        SemanticFreshness::Fresh,
        None,
        Vec::new(),
        SemanticReasonCode::ExactSource,
    );

    assert_eq!(envelope.status(), SemanticFactStatus::Degraded);
}
