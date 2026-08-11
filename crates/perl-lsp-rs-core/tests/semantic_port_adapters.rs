use perl_lsp_rs_core::providers::{
    CanonicalEnvelopePort, FileFactShardPort, ProviderAdapterError, ProviderAdapterSnapshot,
    ProviderCancellationState, ProviderIdentity, ProviderProofClass, ProviderQueryCapability,
    ProviderQueryContext, ProviderQueryDeadline, ProviderQueryKind, ProviderQueryOutcome,
    ProviderQueryRequest, ProviderQuerySubject, ProviderReadinessRequirement,
    ProviderReadinessState, ProviderSemanticPort, ProviderSnapshotCompleteness,
};
use perl_semantic_facts::{
    AnchorFact, AnchorId, Confidence, EntityFact, EntityId, EntityKind, FactId, FileId,
    LifecyclePhase, OccurrenceFact, OccurrenceId, OccurrenceKind, Provenance,
    ProviderFactSourceKind, ProviderFallbackState, ProviderSurface, ScopeId, SemanticConfidence,
    SemanticFactEnvelope, SemanticFactKind, SemanticFreshness, SemanticProducer,
    SemanticProvenance, SemanticReasonCode, SourceAnchor, SourceGeneration,
};
use perl_workspace::workspace::workspace_index::FileFactShard;
use std::error::Error;

fn snapshot(
    proof_ceiling: ProviderProofClass,
    completeness: ProviderSnapshotCompleteness,
    authorities: Vec<SemanticProducer>,
) -> ProviderAdapterSnapshot {
    ProviderAdapterSnapshot::new(
        SourceGeneration::known("document-7"),
        SourceGeneration::known("workspace-3"),
        SemanticFreshness::Fresh,
        LifecyclePhase::Runtime,
        proof_ceiling,
        ProviderFallbackState::Primary,
        Some(1),
        authorities,
        [
            (ProviderQueryCapability::Declarations, completeness),
            (ProviderQueryCapability::References, completeness),
            (ProviderQueryCapability::Visibility, completeness),
            (ProviderQueryCapability::ScopeBindings, completeness),
            (ProviderQueryCapability::Boundaries, completeness),
            (ProviderQueryCapability::Readiness, completeness),
        ],
    )
}

fn context() -> ProviderQueryContext {
    ProviderQueryContext::new(
        ProviderIdentity::known("project"),
        ProviderIdentity::known("root"),
        SourceGeneration::known("document-7"),
        SourceGeneration::known("workspace-3"),
        ProviderReadinessRequirement::ActiveDocument,
        ProviderReadinessState::Ready,
        ProviderQueryDeadline::RemainingMillis(100),
        ProviderCancellationState::Active,
    )
}

fn request(kind: ProviderQueryKind, subject: ProviderQuerySubject) -> ProviderQueryRequest {
    ProviderQueryRequest::new(
        ProviderSurface::Definition,
        "test/request",
        kind,
        subject,
        context(),
    )
}

fn shard(provenance: Provenance, confidence: Confidence) -> FileFactShard {
    let file_id = FileId(10);
    FileFactShard {
        source_uri: "file:///example.pl".to_string(),
        file_id,
        content_hash: 77,
        producer_schema_version: 1,
        anchors_hash: None,
        entities_hash: None,
        occurrences_hash: None,
        edges_hash: None,
        anchors: vec![
            AnchorFact {
                id: AnchorId(20),
                file_id,
                span_start_byte: 4,
                span_end_byte: 12,
                scope_id: Some(ScopeId(1)),
                provenance,
                confidence,
            },
            AnchorFact {
                id: AnchorId(21),
                file_id,
                span_start_byte: 20,
                span_end_byte: 24,
                scope_id: Some(ScopeId(1)),
                provenance,
                confidence,
            },
        ],
        entities: vec![EntityFact {
            id: EntityId(30),
            kind: EntityKind::Subroutine,
            canonical_name: "Example::work".to_string(),
            anchor_id: Some(AnchorId(20)),
            scope_id: Some(ScopeId(1)),
            provenance,
            confidence,
        }],
        occurrences: vec![OccurrenceFact {
            id: OccurrenceId(40),
            kind: OccurrenceKind::Call,
            entity_id: Some(EntityId(30)),
            anchor_id: AnchorId(21),
            scope_id: Some(ScopeId(1)),
            provenance,
            confidence,
        }],
        edges: Vec::new(),
    }
}

#[test]
fn exact_shard_queries_preserve_workspace_producer() -> Result<(), Box<dyn Error>> {
    let port = FileFactShardPort::new(
        &[shard(Provenance::ExactAst, Confidence::High)],
        SemanticProducer::WorkspaceIndex,
        ProviderFactSourceKind::LegacyWorkspace,
        snapshot(
            ProviderProofClass::ExactRead,
            ProviderSnapshotCompleteness::Complete,
            Vec::new(),
        ),
    )?;

    let definition = port.query(&request(
        ProviderQueryKind::Declaration,
        ProviderQuerySubject::Symbol("work".to_string()),
    ));
    assert_eq!(definition.outcome(), ProviderQueryOutcome::Exact);
    assert_eq!(definition.values().map(|values| values.len()), Some(1));
    assert_eq!(
        definition.evidence().producers(),
        &[SemanticProducer::WorkspaceIndex]
    );
    assert!(definition.is_consistent());

    let references = port.query(&request(
        ProviderQueryKind::References {
            include_declaration: false,
        },
        ProviderQuerySubject::Entity(EntityId(30)),
    ));
    assert_eq!(references.outcome(), ProviderQueryOutcome::Exact);
    assert_eq!(references.values().map(|values| values.len()), Some(1));
    assert!(references.is_consistent());
    Ok(())
}

#[test]
fn complete_and_partial_empty_results_stay_distinct() -> Result<(), Box<dyn Error>> {
    let complete = FileFactShardPort::new(
        &[shard(Provenance::ExactAst, Confidence::High)],
        SemanticProducer::Parser,
        ProviderFactSourceKind::ParserSyntax,
        snapshot(
            ProviderProofClass::ExactRead,
            ProviderSnapshotCompleteness::Complete,
            Vec::new(),
        ),
    )?;
    let complete_result = complete.query(&request(
        ProviderQueryKind::Declaration,
        ProviderQuerySubject::Symbol("missing".to_string()),
    ));
    assert!(complete_result.is_exact_empty());
    assert_eq!(
        complete_result.evidence().producers(),
        &[SemanticProducer::Parser]
    );

    let partial = FileFactShardPort::new(
        &[shard(Provenance::ExactAst, Confidence::High)],
        SemanticProducer::Parser,
        ProviderFactSourceKind::ParserSyntax,
        snapshot(
            ProviderProofClass::ExactRead,
            ProviderSnapshotCompleteness::Partial,
            Vec::new(),
        ),
    )?;
    let partial_result = partial.query(&request(
        ProviderQueryKind::Declaration,
        ProviderQuerySubject::Symbol("missing".to_string()),
    ));
    assert_eq!(partial_result.outcome(), ProviderQueryOutcome::Unavailable);
    assert_eq!(partial_result.values(), None);
    assert!(partial_result.is_consistent());
    Ok(())
}

#[test]
fn shard_adapter_rejects_false_producer_trace_and_edit_authority() {
    let compiler = FileFactShardPort::new(
        &[shard(Provenance::ExactAst, Confidence::High)],
        SemanticProducer::PirA,
        ProviderFactSourceKind::CompilerFact,
        snapshot(
            ProviderProofClass::ExactRead,
            ProviderSnapshotCompleteness::Complete,
            Vec::new(),
        ),
    );
    assert_eq!(
        compiler.err(),
        Some(ProviderAdapterError::UnsupportedShardProducer(
            SemanticProducer::PirA
        ))
    );

    let bad_trace = FileFactShardPort::new(
        &[shard(Provenance::ExactAst, Confidence::High)],
        SemanticProducer::Parser,
        ProviderFactSourceKind::CompilerFact,
        snapshot(
            ProviderProofClass::ExactRead,
            ProviderSnapshotCompleteness::Complete,
            Vec::new(),
        ),
    );
    assert_eq!(
        bad_trace.err(),
        Some(ProviderAdapterError::UnsupportedTraceSource {
            producer: SemanticProducer::Parser,
            source: ProviderFactSourceKind::CompilerFact,
        })
    );

    let edits = FileFactShardPort::new(
        &[shard(Provenance::ExactAst, Confidence::High)],
        SemanticProducer::WorkspaceIndex,
        ProviderFactSourceKind::LegacyWorkspace,
        snapshot(
            ProviderProofClass::EditAuthorizing,
            ProviderSnapshotCompleteness::Complete,
            Vec::new(),
        ),
    );
    assert_eq!(
        edits.err(),
        Some(ProviderAdapterError::EditAuthorizationRequiresPlan)
    );
}

#[test]
fn generated_and_dynamic_facts_do_not_become_exact() -> Result<(), Box<dyn Error>> {
    let generated = FileFactShardPort::new(
        &[shard(
            Provenance::FrameworkSynthesis,
            Confidence::Medium,
        )],
        SemanticProducer::SemanticAnalyzer,
        ProviderFactSourceKind::SemanticFact,
        snapshot(
            ProviderProofClass::ExactRead,
            ProviderSnapshotCompleteness::Complete,
            Vec::new(),
        ),
    )?;
    let generated_result = generated.query(&request(
        ProviderQueryKind::Declaration,
        ProviderQuerySubject::Symbol("work".to_string()),
    ));
    assert_eq!(generated_result.outcome(), ProviderQueryOutcome::Degraded);
    assert_eq!(
        generated_result.evidence().reason_code(),
        SemanticReasonCode::GeneratedFromSource
    );

    let mut dynamic_shard = shard(Provenance::DynamicBoundary, Confidence::Low);
    dynamic_shard.occurrences[0].kind = OccurrenceKind::DynamicBoundary;
    let dynamic = FileFactShardPort::new(
        &[dynamic_shard],
        SemanticProducer::SemanticAnalyzer,
        ProviderFactSourceKind::DynamicBoundary,
        snapshot(
            ProviderProofClass::ExactRead,
            ProviderSnapshotCompleteness::Complete,
            Vec::new(),
        ),
    )?;
    let dynamic_result = dynamic.query(&request(
        ProviderQueryKind::Declaration,
        ProviderQuerySubject::Position {
            file_id: FileId(10),
            byte_offset: 21,
        },
    ));
    assert_eq!(dynamic_result.outcome(), ProviderQueryOutcome::Dynamic);
    assert_eq!(dynamic_result.values(), None);
    assert_eq!(
        dynamic_result.evidence().reason_code(),
        SemanticReasonCode::DynamicValue
    );
    assert!(dynamic_result.is_consistent());
    Ok(())
}

#[test]
fn missing_anchor_downgrades_completeness_instead_of_fabricating_exact_empty(
) -> Result<(), Box<dyn Error>> {
    let mut broken = shard(Provenance::ExactAst, Confidence::High);
    broken.entities[0].anchor_id = None;
    let port = FileFactShardPort::new(
        &[broken],
        SemanticProducer::Parser,
        ProviderFactSourceKind::ParserSyntax,
        snapshot(
            ProviderProofClass::ExactRead,
            ProviderSnapshotCompleteness::Complete,
            Vec::new(),
        ),
    )?;
    let result = port.query(&request(
        ProviderQueryKind::Declaration,
        ProviderQuerySubject::Symbol("work".to_string()),
    ));
    assert_eq!(result.outcome(), ProviderQueryOutcome::Unavailable);
    assert_eq!(result.values(), None);
    assert!(result
        .evidence()
        .limitations()
        .iter()
        .any(|limitation| limitation.contains("missing_source_anchor")));
    assert!(result.is_consistent());
    Ok(())
}

fn compiler_envelope(freshness: SemanticFreshness, generation: &str) -> SemanticFactEnvelope {
    SemanticFactEnvelope::new(
        FactId(900),
        Some(EntityId(30)),
        SemanticFactKind::Declaration,
        SourceAnchor::new(Some(AnchorId(20)), FileId(10), 4, 12),
        SourceGeneration::known(generation),
        Some(ScopeId(1)),
        Some("Example".to_string()),
        LifecyclePhase::Runtime,
        SemanticProducer::PirA,
        SemanticProvenance::Known(Provenance::ExactAst),
        SemanticConfidence::Known(Confidence::High),
        freshness,
        None,
        Vec::new(),
        if freshness == SemanticFreshness::Stale {
            SemanticReasonCode::StaleDependency
        } else {
            SemanticReasonCode::ExactSource
        },
    )
}

#[test]
fn canonical_envelopes_preserve_real_compiler_producer_and_staleness() {
    let exact_port = CanonicalEnvelopePort::new(
        &[compiler_envelope(SemanticFreshness::Fresh, "document-7")],
        snapshot(
            ProviderProofClass::ExactRead,
            ProviderSnapshotCompleteness::Complete,
            Vec::new(),
        ),
    );
    let exact_result = exact_port.query(&request(
        ProviderQueryKind::Declaration,
        ProviderQuerySubject::Entity(EntityId(30)),
    ));
    assert_eq!(exact_result.outcome(), ProviderQueryOutcome::Exact);
    assert_eq!(
        exact_result.evidence().producers(),
        &[SemanticProducer::PirA]
    );
    assert!(exact_result.is_consistent());

    let stale_port = CanonicalEnvelopePort::new(
        &[compiler_envelope(SemanticFreshness::Stale, "old-document")],
        snapshot(
            ProviderProofClass::ExactRead,
            ProviderSnapshotCompleteness::Complete,
            Vec::new(),
        ),
    );
    let stale_result = stale_port.query(&request(
        ProviderQueryKind::Declaration,
        ProviderQuerySubject::Entity(EntityId(30)),
    ));
    assert_eq!(stale_result.outcome(), ProviderQueryOutcome::Stale);
    assert_eq!(stale_result.values(), None);
    assert!(stale_result.is_consistent());
}

#[test]
fn absent_compiler_envelopes_strip_unsubstantiated_compiler_authority() {
    let port = CanonicalEnvelopePort::new(
        &[],
        snapshot(
            ProviderProofClass::ExactRead,
            ProviderSnapshotCompleteness::Complete,
            vec![SemanticProducer::PirA],
        ),
    );
    let result = port.query(&request(
        ProviderQueryKind::Declaration,
        ProviderQuerySubject::Workspace,
    ));
    assert_eq!(result.outcome(), ProviderQueryOutcome::Unavailable);
    assert!(result.evidence().producers().is_empty());
    assert_eq!(result.values(), None);
    assert!(result.is_consistent());
}
