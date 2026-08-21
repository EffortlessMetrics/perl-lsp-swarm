use perl_lsp_rs_core::providers::{
    CanonicalEnvelopePort, FileFactShardPort, NoopProviderQueryControl, ProviderAdapterError,
    ProviderAdapterSnapshot, ProviderCancellationState, ProviderIdentity, ProviderQueryCapability,
    ProviderQueryContext, ProviderQueryControl, ProviderQueryDeadline, ProviderQueryKind,
    ProviderQueryOutcome, ProviderQueryRequest, ProviderQueryResult, ProviderQuerySubject,
    ProviderReadinessRequirement, ProviderReadinessState, ProviderSemanticPort,
    ProviderSnapshotCompleteness, execute_provider_query,
};
use perl_semantic_facts::{
    AnchorFact, AnchorId, BoundaryDisposition, BoundaryKind, BoundaryLink, Confidence, EntityFact,
    EntityId, EntityKind, FactId, FileId, LifecyclePhase, OccurrenceFact, OccurrenceId,
    OccurrenceKind, Provenance, ProviderFactSourceKind, ProviderFallbackState, ProviderSurface,
    ScopeId, SemanticConfidence, SemanticFactEnvelope, SemanticFactKind, SemanticFreshness,
    SemanticProducer, SemanticProvenance, SemanticReasonCode, SourceAnchor, SourceGeneration,
};
use perl_workspace::workspace::workspace_index::FileFactShard;
use std::error::Error;
use std::sync::atomic::{AtomicUsize, Ordering};

fn snapshot(completeness: ProviderSnapshotCompleteness) -> ProviderAdapterSnapshot {
    ProviderAdapterSnapshot::new(
        SourceGeneration::known("document-7"),
        SourceGeneration::known("workspace-3"),
        SemanticFreshness::Fresh,
        LifecyclePhase::Runtime,
        ProviderFallbackState::Primary,
        Some(1),
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
    ProviderQueryRequest::new(ProviderSurface::Definition, "test/request", kind, subject, context())
}

fn execute(
    port: &dyn ProviderSemanticPort,
    request: &ProviderQueryRequest,
) -> Result<ProviderQueryResult, perl_lsp_rs_core::providers::ProviderQueryContractError> {
    execute_provider_query(port, request, &NoopProviderQueryControl)
}

struct ScriptedControl {
    cancelled_after: usize,
    deadline_after: usize,
    cancellation_checks: AtomicUsize,
    deadline_checks: AtomicUsize,
}

impl ScriptedControl {
    fn new(cancelled_after: usize, deadline_after: usize) -> Self {
        Self {
            cancelled_after,
            deadline_after,
            cancellation_checks: AtomicUsize::new(0),
            deadline_checks: AtomicUsize::new(0),
        }
    }
}

impl ProviderQueryControl for ScriptedControl {
    fn is_cancelled(&self) -> bool {
        self.cancellation_checks.fetch_add(1, Ordering::Relaxed) >= self.cancelled_after
    }

    fn deadline_expired(&self) -> bool {
        self.deadline_checks.fetch_add(1, Ordering::Relaxed) >= self.deadline_after
    }
}

fn shard(provenance: Provenance, confidence: Confidence) -> FileFactShard {
    shard_in_file(FileId(10), provenance, confidence)
}

fn shard_in_file(file_id: FileId, provenance: Provenance, confidence: Confidence) -> FileFactShard {
    FileFactShard {
        source_uri: format!("file:///example-{}.pl", file_id.0),
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

fn parser_port(
    shards: &[FileFactShard],
    completeness: ProviderSnapshotCompleteness,
) -> Result<FileFactShardPort, ProviderAdapterError> {
    FileFactShardPort::new(
        shards,
        SemanticProducer::Parser,
        ProviderFactSourceKind::ParserSyntax,
        snapshot(completeness),
    )
}

#[test]
fn exact_shard_queries_preserve_workspace_producer() -> Result<(), Box<dyn Error>> {
    let port = FileFactShardPort::new(
        &[shard(Provenance::ExactAst, Confidence::High)],
        SemanticProducer::WorkspaceIndex,
        ProviderFactSourceKind::LegacyWorkspace,
        snapshot(ProviderSnapshotCompleteness::Complete),
    )?;

    let definition = execute(
        &port,
        &request(ProviderQueryKind::Declaration, ProviderQuerySubject::Symbol("work".to_string())),
    )?;
    assert_eq!(definition.outcome(), ProviderQueryOutcome::Exact);
    assert_eq!(definition.value_facts().count(), 1);
    assert_eq!(definition.evidence().producers(), &[SemanticProducer::WorkspaceIndex]);

    let references = execute(
        &port,
        &request(
            ProviderQueryKind::References { include_declaration: false },
            ProviderQuerySubject::Entity(EntityId(30)),
        ),
    )?;
    assert_eq!(references.outcome(), ProviderQueryOutcome::Exact);
    assert_eq!(references.value_facts().count(), 1);
    Ok(())
}

#[test]
fn complete_and_partial_empty_results_stay_distinct() -> Result<(), Box<dyn Error>> {
    let complete = parser_port(
        &[shard(Provenance::ExactAst, Confidence::High)],
        ProviderSnapshotCompleteness::Complete,
    )?;
    let missing = request(
        ProviderQueryKind::Declaration,
        ProviderQuerySubject::Symbol("missing".to_string()),
    );
    let complete_result = execute(&complete, &missing)?;
    assert!(complete_result.is_exact_empty());
    // Exact-empty authority is derived from the concrete denominator, and the
    // fact producers stay empty: completeness never manufactures attribution.
    assert!(complete_result.evidence().producers().is_empty());
    let authority = complete_result
        .evidence()
        .completeness_authority()
        .ok_or("exact empty must retain its denominator receipt")?;
    assert_eq!(authority.producer(), SemanticProducer::Parser);
    assert_eq!(authority.capability(), ProviderQueryCapability::Declarations);
    assert!(authority.covered_unit_count() > 0);
    complete_result.validate_against(&missing)?;

    let partial = parser_port(
        &[shard(Provenance::ExactAst, Confidence::High)],
        ProviderSnapshotCompleteness::Partial,
    )?;
    let partial_result = execute(&partial, &missing)?;
    assert_eq!(partial_result.outcome(), ProviderQueryOutcome::Unavailable);
    assert_eq!(partial_result.value_facts().count(), 0);
    Ok(())
}

#[test]
fn shard_adapter_rejects_false_producer_and_trace() {
    let compiler = FileFactShardPort::new(
        &[shard(Provenance::ExactAst, Confidence::High)],
        SemanticProducer::PirA,
        ProviderFactSourceKind::CompilerFact,
        snapshot(ProviderSnapshotCompleteness::Complete),
    );
    assert_eq!(
        compiler.err(),
        Some(ProviderAdapterError::UnsupportedShardProducer(SemanticProducer::PirA))
    );

    let bad_trace = FileFactShardPort::new(
        &[shard(Provenance::ExactAst, Confidence::High)],
        SemanticProducer::Parser,
        ProviderFactSourceKind::CompilerFact,
        snapshot(ProviderSnapshotCompleteness::Complete),
    );
    assert_eq!(
        bad_trace.err(),
        Some(ProviderAdapterError::UnsupportedTraceSource {
            producer: SemanticProducer::Parser,
            source: ProviderFactSourceKind::CompilerFact,
        })
    );
}

#[test]
fn generated_and_dynamic_facts_do_not_become_exact() -> Result<(), Box<dyn Error>> {
    let generated = FileFactShardPort::new(
        &[shard(Provenance::FrameworkSynthesis, Confidence::Medium)],
        SemanticProducer::SemanticAnalyzer,
        ProviderFactSourceKind::SemanticFact,
        snapshot(ProviderSnapshotCompleteness::Complete),
    )?;
    let generated_result = execute(
        &generated,
        &request(ProviderQueryKind::Declaration, ProviderQuerySubject::Symbol("work".to_string())),
    )?;
    assert_eq!(generated_result.outcome(), ProviderQueryOutcome::Degraded);
    assert_eq!(
        generated_result.evidence().semantic_reason(),
        SemanticReasonCode::GeneratedFromSource
    );

    let mut dynamic_shard = shard(Provenance::DynamicBoundary, Confidence::Low);
    dynamic_shard.occurrences[0].kind = OccurrenceKind::DynamicBoundary;
    let dynamic = FileFactShardPort::new(
        &[dynamic_shard],
        SemanticProducer::SemanticAnalyzer,
        ProviderFactSourceKind::DynamicBoundary,
        snapshot(ProviderSnapshotCompleteness::Complete),
    )?;
    let dynamic_result = execute(
        &dynamic,
        &request(
            ProviderQueryKind::Declaration,
            ProviderQuerySubject::Position { file_id: FileId(10), byte_offset: 21 },
        ),
    )?;
    assert_eq!(dynamic_result.outcome(), ProviderQueryOutcome::Dynamic);
    assert_eq!(dynamic_result.value_facts().count(), 0);
    assert_eq!(dynamic_result.evidence().semantic_reason(), SemanticReasonCode::DynamicValue);
    Ok(())
}

#[test]
fn missing_anchor_downgrades_completeness_instead_of_fabricating_exact_empty()
-> Result<(), Box<dyn Error>> {
    let mut broken = shard(Provenance::ExactAst, Confidence::High);
    broken.entities[0].anchor_id = None;
    let port = parser_port(&[broken], ProviderSnapshotCompleteness::Complete)?;
    let result = execute(
        &port,
        &request(ProviderQueryKind::Declaration, ProviderQuerySubject::Symbol("work".to_string())),
    )?;
    assert_eq!(result.outcome(), ProviderQueryOutcome::Unavailable);
    assert_eq!(result.value_facts().count(), 0);
    assert!(
        result
            .evidence()
            .limitations()
            .iter()
            .any(|limitation| limitation.contains("missing_source_anchor"))
    );

    // The downgraded capability also cannot claim an exact-empty denominator.
    let missing = execute(
        &port,
        &request(
            ProviderQueryKind::Declaration,
            ProviderQuerySubject::Symbol("missing".to_string()),
        ),
    )?;
    assert_eq!(missing.outcome(), ProviderQueryOutcome::Unavailable);
    assert!(!missing.is_exact_empty());
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

fn boundary_envelope(freshness: SemanticFreshness, generation: &str) -> SemanticFactEnvelope {
    let mut envelope = compiler_envelope(freshness, generation);
    envelope.entity_id = None;
    envelope.kind = SemanticFactKind::Boundary;
    envelope.package = None;
    envelope.provenance = SemanticProvenance::Known(Provenance::DynamicBoundary);
    envelope.reason_code = SemanticReasonCode::DynamicValue;
    envelope.boundary = Some(BoundaryLink::new(
        Some(envelope.fact_id),
        BoundaryKind::DynamicValue,
        BoundaryDisposition::Degrade,
        SemanticReasonCode::DynamicValue,
    ));
    envelope
}

#[test]
fn canonical_envelopes_preserve_real_compiler_producer_and_staleness() -> Result<(), Box<dyn Error>>
{
    let exact_port = CanonicalEnvelopePort::new(
        &[compiler_envelope(SemanticFreshness::Fresh, "document-7")],
        snapshot(ProviderSnapshotCompleteness::Complete),
    )?;
    let exact_result = execute(
        &exact_port,
        &request(ProviderQueryKind::Declaration, ProviderQuerySubject::Entity(EntityId(30))),
    )?;
    assert_eq!(exact_result.outcome(), ProviderQueryOutcome::Exact);
    assert_eq!(exact_result.evidence().producers(), &[SemanticProducer::PirA]);

    let stale_port = CanonicalEnvelopePort::new(
        &[compiler_envelope(SemanticFreshness::Stale, "old-document")],
        snapshot(ProviderSnapshotCompleteness::Complete),
    )?;
    let stale_result = execute(
        &stale_port,
        &request(ProviderQueryKind::Declaration, ProviderQuerySubject::Entity(EntityId(30))),
    )?;
    assert_eq!(stale_result.outcome(), ProviderQueryOutcome::Stale);
    assert_eq!(stale_result.value_facts().count(), 0);
    Ok(())
}

#[test]
fn canonical_package_is_not_a_symbol_alias() -> Result<(), Box<dyn Error>> {
    let port = CanonicalEnvelopePort::new(
        &[compiler_envelope(SemanticFreshness::Fresh, "document-7")],
        snapshot(ProviderSnapshotCompleteness::Complete),
    )?;

    let symbol = execute(
        &port,
        &request(ProviderQueryKind::Declaration, ProviderQuerySubject::Symbol("Example".into())),
    )?;
    assert!(symbol.is_exact_empty(), "package identity must not answer a Symbol query");

    let package = execute(
        &port,
        &request(ProviderQueryKind::Declaration, ProviderQuerySubject::Package("Example".into())),
    )?;
    assert_eq!(package.outcome(), ProviderQueryOutcome::Exact);
    assert_eq!(package.value_facts().count(), 1);
    Ok(())
}

#[test]
fn exact_empty_authority_is_derived_from_actual_envelopes() -> Result<(), Box<dyn Error>> {
    // An empty canonical set has no denominator units and cannot claim exact
    // emptiness, regardless of the caller's completeness label.
    let empty_port =
        CanonicalEnvelopePort::new(&[], snapshot(ProviderSnapshotCompleteness::Complete))?;
    let missing = request(ProviderQueryKind::Declaration, ProviderQuerySubject::Workspace);
    let empty_result = execute(&empty_port, &missing)?;
    assert_eq!(empty_result.outcome(), ProviderQueryOutcome::Unavailable);
    assert!(empty_result.evidence().producers().is_empty());

    // A uniform producer set derives grant authority from its actual envelopes.
    let parser_envelope = SemanticFactEnvelope::new(
        FactId(901),
        Some(EntityId(30)),
        SemanticFactKind::Declaration,
        SourceAnchor::new(Some(AnchorId(20)), FileId(10), 4, 12),
        SourceGeneration::known("document-7"),
        Some(ScopeId(1)),
        Some("Example".to_string()),
        LifecyclePhase::Runtime,
        SemanticProducer::Parser,
        SemanticProvenance::Known(Provenance::ExactAst),
        SemanticConfidence::Known(Confidence::High),
        SemanticFreshness::Fresh,
        None,
        Vec::new(),
        SemanticReasonCode::ExactSource,
    );
    let uniform_port = CanonicalEnvelopePort::new(
        std::slice::from_ref(&parser_envelope),
        snapshot(ProviderSnapshotCompleteness::Complete),
    )?;
    let missing_symbol = request(
        ProviderQueryKind::Declaration,
        ProviderQuerySubject::Symbol("missing".to_string()),
    );
    let uniform_result = execute(&uniform_port, &missing_symbol)?;
    assert!(uniform_result.is_exact_empty());
    let authority = uniform_result
        .evidence()
        .completeness_authority()
        .ok_or("exact empty must retain its denominator receipt")?;
    assert_eq!(authority.producer(), SemanticProducer::Parser);

    // A mixed-producer set has no single truthful denominator authority.
    let mixed_port = CanonicalEnvelopePort::new(
        &[parser_envelope, compiler_envelope(SemanticFreshness::Fresh, "document-7")],
        snapshot(ProviderSnapshotCompleteness::Complete),
    )?;
    let mixed_result = execute(&mixed_port, &missing_symbol)?;
    assert_eq!(mixed_result.outcome(), ProviderQueryOutcome::Unavailable);
    assert!(!mixed_result.is_exact_empty());
    Ok(())
}

#[test]
fn file_scoped_exact_empty_requires_requested_file_coverage() -> Result<(), Box<dyn Error>> {
    let port = parser_port(
        &[shard(Provenance::ExactAst, Confidence::High)],
        ProviderSnapshotCompleteness::Complete,
    )?;

    let out_of_scope_file = execute(
        &port,
        &request(ProviderQueryKind::Declaration, ProviderQuerySubject::File(FileId(999))),
    )?;
    assert_eq!(out_of_scope_file.outcome(), ProviderQueryOutcome::Unavailable);
    assert!(!out_of_scope_file.is_exact_empty());

    let out_of_scope_position = execute(
        &port,
        &request(
            ProviderQueryKind::Declaration,
            ProviderQuerySubject::Position { file_id: FileId(999), byte_offset: 0 },
        ),
    )?;
    assert_eq!(out_of_scope_position.outcome(), ProviderQueryOutcome::Unavailable);
    assert!(!out_of_scope_position.is_exact_empty());

    // A position with no matching fact in a represented file still has a
    // truthful exact-empty denominator.
    let covered_position = execute(
        &port,
        &request(
            ProviderQueryKind::Declaration,
            ProviderQuerySubject::Position { file_id: FileId(10), byte_offset: 0 },
        ),
    )?;
    assert!(covered_position.is_exact_empty());

    // File 11 is represented only by reference records. A declaration query
    // must not borrow file 10's declaration denominator to mint exact-empty.
    let mut references_only = compiler_envelope(SemanticFreshness::Fresh, "document-7");
    references_only.fact_id = FactId(901);
    references_only.kind = SemanticFactKind::Occurrence;
    references_only.anchor.file_id = FileId(11);
    let port = CanonicalEnvelopePort::new(
        &[compiler_envelope(SemanticFreshness::Fresh, "document-7"), references_only],
        snapshot(ProviderSnapshotCompleteness::Complete),
    )?;
    let file_result = execute(
        &port,
        &request(ProviderQueryKind::Declaration, ProviderQuerySubject::File(FileId(11))),
    )?;
    assert_eq!(file_result.outcome(), ProviderQueryOutcome::Unavailable);
    assert!(!file_result.is_exact_empty());
    Ok(())
}

#[test]
fn two_file_shards_never_collide_fact_ids() -> Result<(), Box<dyn Error>> {
    let first = shard_in_file(FileId(10), Provenance::ExactAst, Confidence::High);
    let second = shard_in_file(FileId(11), Provenance::ExactAst, Confidence::High);
    let port = parser_port(&[first, second], ProviderSnapshotCompleteness::Complete)?;
    let result =
        execute(&port, &request(ProviderQueryKind::Declaration, ProviderQuerySubject::Workspace))?;
    assert_eq!(result.outcome(), ProviderQueryOutcome::Degraded);
    let values: Vec<_> = result.value_facts().collect();
    assert_eq!(values.len(), 2, "both file shards keep their entity facts");
    assert_ne!(values[0].fact_id, values[1].fact_id);
    let mut files: Vec<_> = values.iter().map(|value| value.anchor.file_id).collect();
    files.sort();
    assert_eq!(files, vec![FileId(10), FileId(11)]);
    assert!(
        result
            .evidence()
            .limitations()
            .iter()
            .any(|limitation| limitation == "multi_shard_single_snapshot_exactness")
    );

    // EntityId is local to a shard. A multi-file port must not answer an
    // unqualified local ID by combining both files' same-numbered entities.
    let entity_result = execute(
        &port,
        &request(ProviderQueryKind::Declaration, ProviderQuerySubject::Entity(EntityId(30))),
    )?;
    assert_eq!(entity_result.outcome(), ProviderQueryOutcome::Unavailable);
    assert_eq!(entity_result.value_facts().count(), 0);
    assert!(!entity_result.is_exact_empty());
    assert!(
        entity_result
            .evidence()
            .limitations()
            .iter()
            .any(|limitation| limitation == "unqualified_entity_multi_file")
    );
    Ok(())
}

#[test]
fn unqualified_single_shard_match_retains_multi_shard_limitation() -> Result<(), Box<dyn Error>> {
    let first = shard_in_file(FileId(10), Provenance::ExactAst, Confidence::High);
    let mut second = shard_in_file(FileId(11), Provenance::ExactAst, Confidence::High);
    second.entities[0].canonical_name = "Other::different".to_string();
    let port = parser_port(&[first, second], ProviderSnapshotCompleteness::Complete)?;

    // Only file 10 currently matches, but an unqualified symbol query still
    // needs cross-shard generation ownership before it can claim exactness.
    let result = execute(
        &port,
        &request(ProviderQueryKind::Declaration, ProviderQuerySubject::Symbol("work".into())),
    )?;
    assert_eq!(result.outcome(), ProviderQueryOutcome::Degraded);
    assert_eq!(result.value_facts().count(), 1);
    assert!(
        result
            .evidence()
            .limitations()
            .iter()
            .any(|limitation| limitation == "multi_shard_single_snapshot_exactness")
    );
    Ok(())
}

#[test]
fn cross_generation_snapshot_cannot_answer_exact() -> Result<(), Box<dyn Error>> {
    let port = parser_port(
        &[shard(Provenance::ExactAst, Confidence::High)],
        ProviderSnapshotCompleteness::Complete,
    )?;

    // The request moved to document-8 while the snapshot still describes
    // document-7: values are stale for this request, never exact.
    let mut newer =
        request(ProviderQueryKind::Declaration, ProviderQuerySubject::Symbol("work".to_string()));
    newer.context.document_generation = SourceGeneration::known("document-8");
    let newer_result = execute(&port, &newer)?;
    assert_eq!(newer_result.outcome(), ProviderQueryOutcome::Stale);
    assert_eq!(newer_result.value_facts().count(), 0);
    assert!(newer_result.supporting_facts().any(|fact| fact.freshness == SemanticFreshness::Stale));

    // With no subject-matching facts the same mismatch cannot mint exact-empty.
    let mut newer_missing = request(
        ProviderQueryKind::Declaration,
        ProviderQuerySubject::Symbol("missing".to_string()),
    );
    newer_missing.context.document_generation = SourceGeneration::known("document-8");
    let empty_result = execute(&port, &newer_missing)?;
    assert_eq!(empty_result.outcome(), ProviderQueryOutcome::Unavailable);
    assert!(!empty_result.is_exact_empty());
    assert!(
        empty_result
            .evidence()
            .limitations()
            .iter()
            .any(|limitation| limitation.contains("generation_mismatch"))
    );

    // A request admitted as stale is stale even when the snapshot is fresh.
    let mut stale_context =
        request(ProviderQueryKind::Declaration, ProviderQuerySubject::Symbol("work".to_string()));
    stale_context.context.readiness_state = ProviderReadinessState::Stale;
    let stale_result = execute(&port, &stale_context)?;
    assert_eq!(stale_result.outcome(), ProviderQueryOutcome::Stale);
    assert_eq!(stale_result.value_facts().count(), 0);
    Ok(())
}

#[test]
fn stale_boundary_blockers_are_retained_as_stale_evidence() -> Result<(), Box<dyn Error>> {
    for (freshness, generation) in
        [(SemanticFreshness::Stale, "document-7"), (SemanticFreshness::Fresh, "old-document")]
    {
        let port = CanonicalEnvelopePort::new(
            &[boundary_envelope(freshness, generation)],
            snapshot(ProviderSnapshotCompleteness::Complete),
        )?;
        let result = execute(
            &port,
            &request(
                ProviderQueryKind::Declaration,
                ProviderQuerySubject::Position { file_id: FileId(10), byte_offset: 5 },
            ),
        )?;
        assert_eq!(result.outcome(), ProviderQueryOutcome::Stale);
        assert_eq!(result.value_facts().count(), 0);
        assert!(result.facts().iter().any(|fact| {
            fact.envelope().kind == SemanticFactKind::Boundary
                && fact.envelope().freshness == SemanticFreshness::Stale
        }));
    }
    Ok(())
}

#[test]
fn live_controls_are_rechecked_after_adapter_query() -> Result<(), Box<dyn Error>> {
    let port = parser_port(
        &[shard(Provenance::ExactAst, Confidence::High)],
        ProviderSnapshotCompleteness::Complete,
    )?;
    let query =
        request(ProviderQueryKind::Declaration, ProviderQuerySubject::Symbol("work".into()));

    // The first admission check is clear; the second check, performed by the
    // checked result boundary, observes cancellation after the adapter draft.
    let cancelled = ScriptedControl::new(3, usize::MAX);
    let cancellation = execute_provider_query(&port, &query, &cancelled);
    assert_eq!(
        cancellation,
        Err(perl_lsp_rs_core::providers::ProviderQueryContractError::InvalidOutcomeEvidence(
            ProviderQueryOutcome::Exact,
        ))
    );
    assert!(cancelled.cancellation_checks.load(Ordering::Relaxed) >= 2);

    // Exercise the same post-query guard for a deadline independently.
    let deadline = ScriptedControl::new(usize::MAX, 3);
    let expiry = execute_provider_query(&port, &query, &deadline);
    assert_eq!(
        expiry,
        Err(perl_lsp_rs_core::providers::ProviderQueryContractError::InvalidOutcomeEvidence(
            ProviderQueryOutcome::Exact,
        ))
    );
    assert!(deadline.deadline_checks.load(Ordering::Relaxed) >= 2);
    Ok(())
}

#[test]
fn scripted_controls_abort_selection_completeness_and_fact_construction()
-> Result<(), Box<dyn Error>> {
    let port = parser_port(
        &[shard(Provenance::ExactAst, Confidence::High)],
        ProviderSnapshotCompleteness::Complete,
    )?;

    // Admission is active, but cancellation changes before selection can
    // construct a value. The adapter returns the typed terminal refusal.
    let selection =
        request(ProviderQueryKind::Declaration, ProviderQuerySubject::Symbol("work".into()));
    let cancelled_during_selection = ScriptedControl::new(1, usize::MAX);
    let result = execute_provider_query(&port, &selection, &cancelled_during_selection)?;
    assert_eq!(result.outcome(), ProviderQueryOutcome::Cancelled);
    assert_eq!(result.value_facts().count(), 0);

    // A missing symbol would otherwise issue an exact-empty completeness
    // grant. Deadline expiry at that issuance boundary must refuse it.
    let missing =
        request(ProviderQueryKind::Declaration, ProviderQuerySubject::Symbol("missing".into()));
    let deadline_during_completeness = ScriptedControl::new(usize::MAX, 2);
    let result = execute_provider_query(&port, &missing, &deadline_during_completeness)?;
    assert_eq!(result.outcome(), ProviderQueryOutcome::DeadlineExceeded);
    assert!(!result.is_exact_empty());

    // A live cancellation after selection but before fact construction cannot
    // allow an exact value to escape the checked adapter path.
    let fact = request(ProviderQueryKind::Declaration, ProviderQuerySubject::Symbol("work".into()));
    let cancelled_during_fact_construction = ScriptedControl::new(2, usize::MAX);
    let result = execute_provider_query(&port, &fact, &cancelled_during_fact_construction)?;
    assert_eq!(result.outcome(), ProviderQueryOutcome::Cancelled);
    assert_eq!(result.value_facts().count(), 0);
    Ok(())
}

#[test]
fn limited_building_or_failed_readiness_cannot_be_exact() -> Result<(), Box<dyn Error>> {
    let port = parser_port(
        &[shard(Provenance::ExactAst, Confidence::High)],
        ProviderSnapshotCompleteness::Complete,
    )?;

    let mut limited =
        request(ProviderQueryKind::Declaration, ProviderQuerySubject::Symbol("work".to_string()));
    limited.context.readiness_state = ProviderReadinessState::ReadyLimited;
    let limited_result = execute(&port, &limited)?;
    assert_eq!(limited_result.outcome(), ProviderQueryOutcome::Degraded);
    assert_eq!(limited_result.value_facts().count(), 1);

    let mut building =
        request(ProviderQueryKind::Declaration, ProviderQuerySubject::Symbol("work".to_string()));
    building.context.readiness_state = ProviderReadinessState::Building;
    let building_result = execute(&port, &building)?;
    assert_eq!(building_result.outcome(), ProviderQueryOutcome::Unavailable);
    assert_eq!(building_result.value_facts().count(), 0);

    let mut failed =
        request(ProviderQueryKind::Declaration, ProviderQuerySubject::Symbol("work".to_string()));
    failed.context.readiness_state = ProviderReadinessState::Failed;
    let failed_result = execute(&port, &failed)?;
    assert_eq!(failed_result.outcome(), ProviderQueryOutcome::Error);

    let mut unavailable =
        request(ProviderQueryKind::Declaration, ProviderQuerySubject::Symbol("work".to_string()));
    unavailable.context.readiness_state = ProviderReadinessState::Unavailable;
    let unavailable_result = execute(&port, &unavailable)?;
    assert_eq!(unavailable_result.outcome(), ProviderQueryOutcome::Unavailable);
    Ok(())
}

#[test]
fn edit_authorizing_requests_stay_below_exact() -> Result<(), Box<dyn Error>> {
    let port = parser_port(
        &[shard(Provenance::ExactAst, Confidence::High)],
        ProviderSnapshotCompleteness::Complete,
    )?;

    let mut edit =
        request(ProviderQueryKind::Declaration, ProviderQuerySubject::Symbol("work".to_string()));
    edit.context.readiness_requirement = ProviderReadinessRequirement::EditAuthorizing;
    let edit_result = execute(&port, &edit)?;
    assert_eq!(edit_result.outcome(), ProviderQueryOutcome::Degraded);
    assert!(
        edit_result
            .evidence()
            .limitations()
            .iter()
            .any(|limitation| limitation.contains("edit_authorization"))
    );

    // No adapter path issues an edit-authorizing exact-empty grant either.
    let mut edit_missing = request(
        ProviderQueryKind::Declaration,
        ProviderQuerySubject::Symbol("missing".to_string()),
    );
    edit_missing.context.readiness_requirement = ProviderReadinessRequirement::EditAuthorizing;
    let empty_result = execute(&port, &edit_missing)?;
    assert!(!empty_result.is_exact_empty());
    assert_eq!(empty_result.outcome(), ProviderQueryOutcome::Unavailable);
    Ok(())
}

#[test]
fn conflicting_duplicate_fact_ids_fail_closed() -> Result<(), Box<dyn Error>> {
    let conflicting = SemanticFactEnvelope::new(
        FactId(900),
        Some(EntityId(31)),
        SemanticFactKind::Declaration,
        SourceAnchor::new(Some(AnchorId(21)), FileId(10), 20, 24),
        SourceGeneration::known("document-7"),
        Some(ScopeId(1)),
        Some("Other".to_string()),
        LifecyclePhase::Runtime,
        SemanticProducer::PirA,
        SemanticProvenance::Known(Provenance::ExactAst),
        SemanticConfidence::Known(Confidence::High),
        SemanticFreshness::Fresh,
        None,
        Vec::new(),
        SemanticReasonCode::ExactSource,
    );
    let base = compiler_envelope(SemanticFreshness::Fresh, "document-7");
    let forward = CanonicalEnvelopePort::new(
        &[base.clone(), conflicting.clone()],
        snapshot(ProviderSnapshotCompleteness::Complete),
    );
    let reversed = CanonicalEnvelopePort::new(
        &[conflicting, base.clone()],
        snapshot(ProviderSnapshotCompleteness::Complete),
    );
    assert_eq!(forward.err(), Some(ProviderAdapterError::ConflictingFactId(FactId(900))));
    assert_eq!(reversed.err(), Some(ProviderAdapterError::ConflictingFactId(FactId(900))));

    // Byte-for-byte identical duplicates collapse to the one canonical fact.
    let deduped = CanonicalEnvelopePort::new(
        &[base.clone(), base],
        snapshot(ProviderSnapshotCompleteness::Complete),
    )?;
    let result = execute(
        &deduped,
        &request(ProviderQueryKind::Declaration, ProviderQuerySubject::Entity(EntityId(30))),
    )?;
    assert_eq!(result.outcome(), ProviderQueryOutcome::Exact);
    assert_eq!(result.value_facts().count(), 1);
    Ok(())
}

#[test]
fn conflicting_duplicate_shard_identities_downgrade() -> Result<(), Box<dyn Error>> {
    // A conflicting duplicate anchor cannot bind any row truthfully.
    let mut dup_anchor = shard(Provenance::ExactAst, Confidence::High);
    let mut conflicting_anchor = dup_anchor.anchors[0].clone();
    conflicting_anchor.span_start_byte = 6;
    dup_anchor.anchors.push(conflicting_anchor);
    let port = parser_port(&[dup_anchor], ProviderSnapshotCompleteness::Complete)?;
    let result = execute(
        &port,
        &request(ProviderQueryKind::Declaration, ProviderQuerySubject::Symbol("work".to_string())),
    )?;
    assert_eq!(result.outcome(), ProviderQueryOutcome::Unavailable);
    assert!(
        result
            .evidence()
            .limitations()
            .iter()
            .any(|limitation| limitation.contains("conflicting_duplicate"))
    );

    // A conflicting duplicate entity identity cannot bind a truthful name, so
    // references through it lose exact-empty authority as well.
    let mut dup_entity = shard(Provenance::ExactAst, Confidence::High);
    let mut conflicting_entity = dup_entity.entities[0].clone();
    conflicting_entity.canonical_name = "Other::work".to_string();
    dup_entity.entities.push(conflicting_entity);
    let port = parser_port(&[dup_entity], ProviderSnapshotCompleteness::Complete)?;
    let missing = execute(
        &port,
        &request(
            ProviderQueryKind::Declaration,
            ProviderQuerySubject::Symbol("missing".to_string()),
        ),
    )?;
    assert_eq!(missing.outcome(), ProviderQueryOutcome::Unavailable);
    assert!(!missing.is_exact_empty());
    Ok(())
}

#[test]
fn position_queries_resolve_through_the_cursor_occurrence() -> Result<(), Box<dyn Error>> {
    let port = parser_port(
        &[shard(Provenance::ExactAst, Confidence::High)],
        ProviderSnapshotCompleteness::Complete,
    )?;

    // Definition at a reference position resolves the occurrence's entity.
    let definition = execute(
        &port,
        &request(
            ProviderQueryKind::Declaration,
            ProviderQuerySubject::Position { file_id: FileId(10), byte_offset: 21 },
        ),
    )?;
    assert_eq!(definition.outcome(), ProviderQueryOutcome::Exact);
    let values: Vec<_> = definition.value_facts().collect();
    assert_eq!(values.len(), 1, "declaration at a reference position must not false-empty");
    assert_eq!(values[0].anchor.start_byte, 4);
    assert_eq!(values[0].anchor.end_byte, 12);

    // References at the same position return the occurrence set.
    let references = execute(
        &port,
        &request(
            ProviderQueryKind::References { include_declaration: false },
            ProviderQuerySubject::Position { file_id: FileId(10), byte_offset: 21 },
        ),
    )?;
    assert_eq!(references.outcome(), ProviderQueryOutcome::Exact);
    assert_eq!(references.value_facts().count(), 1);
    Ok(())
}

#[test]
fn position_queries_do_not_cross_file_entity_id_collisions() -> Result<(), Box<dyn Error>> {
    let first = shard_in_file(FileId(10), Provenance::ExactAst, Confidence::High);
    // Fact IDs are only meaningful within their owning file for this adapter:
    // the second shard intentionally reuses the same entity and occurrence IDs.
    let second = shard_in_file(FileId(11), Provenance::ExactAst, Confidence::High);
    let port = parser_port(&[first, second], ProviderSnapshotCompleteness::Complete)?;
    let subject = ProviderQuerySubject::Position { file_id: FileId(10), byte_offset: 21 };

    let definition = execute(&port, &request(ProviderQueryKind::Declaration, subject.clone()))?;
    assert_eq!(definition.outcome(), ProviderQueryOutcome::Exact);
    let definition_values: Vec<_> = definition.value_facts().collect();
    assert_eq!(definition_values.len(), 1);
    assert_eq!(definition_values[0].anchor.file_id, FileId(10));

    let references = execute(
        &port,
        &request(ProviderQueryKind::References { include_declaration: false }, subject),
    )?;
    assert_eq!(references.outcome(), ProviderQueryOutcome::Exact);
    let reference_values: Vec<_> = references.value_facts().collect();
    assert_eq!(reference_values.len(), 1);
    assert_eq!(reference_values[0].anchor.file_id, FileId(10));
    Ok(())
}

#[test]
fn ambiguous_symbol_declarations_are_blocked() -> Result<(), Box<dyn Error>> {
    let mut ambiguous = shard(Provenance::ExactAst, Confidence::High);
    ambiguous.entities.push(EntityFact {
        id: EntityId(31),
        kind: EntityKind::Subroutine,
        canonical_name: "Other::work".to_string(),
        anchor_id: Some(AnchorId(21)),
        scope_id: Some(ScopeId(2)),
        provenance: Provenance::ExactAst,
        confidence: Confidence::High,
    });
    let port = parser_port(&[ambiguous], ProviderSnapshotCompleteness::Complete)?;
    let result = execute(
        &port,
        &request(ProviderQueryKind::Declaration, ProviderQuerySubject::Symbol("work".to_string())),
    )?;
    assert_eq!(result.outcome(), ProviderQueryOutcome::Ambiguous);
    assert_eq!(result.value_facts().count(), 0);
    Ok(())
}

#[test]
fn adapter_results_are_deterministic_under_input_reorder() -> Result<(), Box<dyn Error>> {
    let first = shard_in_file(FileId(10), Provenance::ExactAst, Confidence::High);
    let second = shard_in_file(FileId(11), Provenance::ExactAst, Confidence::High);
    let forward =
        parser_port(&[first.clone(), second.clone()], ProviderSnapshotCompleteness::Complete)?;
    let reversed = parser_port(&[second, first], ProviderSnapshotCompleteness::Complete)?;
    let query = request(ProviderQueryKind::Declaration, ProviderQuerySubject::Workspace);
    let left = execute(&forward, &query)?;
    let right = execute(&reversed, &query)?;
    assert_eq!(serde_json::to_string(&left)?, serde_json::to_string(&right)?);
    Ok(())
}

fn snapshot_with_fallback(
    fallback: ProviderFallbackState,
    completeness: ProviderSnapshotCompleteness,
) -> ProviderAdapterSnapshot {
    ProviderAdapterSnapshot::new(
        SourceGeneration::known("document-7"),
        SourceGeneration::known("workspace-3"),
        SemanticFreshness::Fresh,
        LifecyclePhase::Runtime,
        fallback,
        Some(1),
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

#[test]
fn non_exact_denominator_units_cannot_authorize_exact_empty() -> Result<(), Box<dyn Error>> {
    let missing = || {
        request(ProviderQueryKind::Declaration, ProviderQuerySubject::Symbol("missing".to_string()))
    };

    // Stale freshness: the unit is stale, whatever the snapshot claims.
    let stale = CanonicalEnvelopePort::new(
        &[compiler_envelope(SemanticFreshness::Stale, "old-document")],
        snapshot(ProviderSnapshotCompleteness::Complete),
    )?;
    let result = execute(&stale, &missing())?;
    assert_eq!(result.outcome(), ProviderQueryOutcome::Unavailable);
    assert!(!result.is_exact_empty());

    // Fresh but out of generation for the request: currentness is
    // request-relative, so the unit cannot vouch for emptiness.
    let old_generation = CanonicalEnvelopePort::new(
        &[compiler_envelope(SemanticFreshness::Fresh, "old-document")],
        snapshot(ProviderSnapshotCompleteness::Complete),
    )?;
    let result = execute(&old_generation, &missing())?;
    assert_eq!(result.outcome(), ProviderQueryOutcome::Unavailable);
    assert!(!result.is_exact_empty());

    // Refused unit: a refused fact cannot vouch for its capability family.
    let mut refused_envelope = compiler_envelope(SemanticFreshness::Fresh, "document-7");
    refused_envelope.reason_code = SemanticReasonCode::UnsupportedEffect;
    let refused = CanonicalEnvelopePort::new(
        &[refused_envelope],
        snapshot(ProviderSnapshotCompleteness::Complete),
    )?;
    let result = execute(&refused, &missing())?;
    assert_eq!(result.outcome(), ProviderQueryOutcome::Unavailable);
    assert!(!result.is_exact_empty());

    // Control: an exact-grade, request-current denominator still issues the grant.
    let exact = CanonicalEnvelopePort::new(
        &[compiler_envelope(SemanticFreshness::Fresh, "document-7")],
        snapshot(ProviderSnapshotCompleteness::Complete),
    )?;
    let result = execute(&exact, &missing())?;
    assert!(result.is_exact_empty());
    Ok(())
}

#[test]
fn non_primary_snapshot_cannot_produce_exact_answers() -> Result<(), Box<dyn Error>> {
    let present = || {
        request(ProviderQueryKind::Declaration, ProviderQuerySubject::Symbol("work".to_string()))
    };
    let missing = || {
        request(ProviderQueryKind::Declaration, ProviderQuerySubject::Symbol("missing".to_string()))
    };
    let port_with =
        |fallback: ProviderFallbackState| -> Result<FileFactShardPort, ProviderAdapterError> {
            FileFactShardPort::new(
                &[shard(Provenance::ExactAst, Confidence::High)],
                SemanticProducer::Parser,
                ProviderFactSourceKind::ParserSyntax,
                snapshot_with_fallback(fallback, ProviderSnapshotCompleteness::Complete),
            )
        };

    // A shadow snapshot corroborates but never originates exactness.
    let shadow = port_with(ProviderFallbackState::Shadow)?;
    let value = execute(&shadow, &present())?;
    assert_eq!(value.outcome(), ProviderQueryOutcome::Degraded);
    assert_eq!(value.value_facts().count(), 1);
    assert!(
        value.evidence().limitations().iter().any(|note| note.contains("snapshot_not_exact_grade"))
    );
    let empty = execute(&shadow, &missing())?;
    assert_eq!(empty.outcome(), ProviderQueryOutcome::Unavailable);
    assert!(!empty.is_exact_empty());

    // An observational snapshot degrades on the same terms.
    let observational = port_with(ProviderFallbackState::Unavailable)?;
    let value = execute(&observational, &present())?;
    assert_eq!(value.outcome(), ProviderQueryOutcome::Degraded);
    let empty = execute(&observational, &missing())?;
    assert!(!empty.is_exact_empty());

    // A fallback snapshot stays on the fallback path for both shapes.
    let fallback = port_with(ProviderFallbackState::Fallback)?;
    let value = execute(&fallback, &present())?;
    assert_eq!(value.outcome(), ProviderQueryOutcome::Fallback);
    let empty = execute(&fallback, &missing())?;
    assert!(!empty.is_exact_empty());

    // Control: the primary snapshot keeps its exact answers.
    let primary = port_with(ProviderFallbackState::Primary)?;
    let value = execute(&primary, &present())?;
    assert_eq!(value.outcome(), ProviderQueryOutcome::Exact);
    let empty = execute(&primary, &missing())?;
    assert!(empty.is_exact_empty());
    Ok(())
}

#[test]
fn tombstoned_duplicate_identities_cannot_be_resurrected() -> Result<(), Box<dyn Error>> {
    let present = || {
        request(ProviderQueryKind::Declaration, ProviderQuerySubject::Symbol("work".to_string()))
    };

    // [A, B, A]: the third row must not re-bind the contested anchor.
    let mut triple = shard(Provenance::ExactAst, Confidence::High);
    let mut conflicting = triple.anchors[0].clone();
    conflicting.span_start_byte = 6;
    let original = triple.anchors[0].clone();
    triple.anchors.push(conflicting);
    triple.anchors.push(original);
    let port = parser_port(&[triple], ProviderSnapshotCompleteness::Complete)?;
    let result = execute(&port, &present())?;
    assert_eq!(result.outcome(), ProviderQueryOutcome::Unavailable);
    assert_eq!(result.value_facts().count(), 0);
    assert!(
        result
            .evidence()
            .limitations()
            .iter()
            .any(|limitation| limitation.contains("conflicting_duplicate"))
    );

    // [A, B, B]: agreement between later rows cannot outvote the tombstone.
    let mut triple = shard(Provenance::ExactAst, Confidence::High);
    let mut conflicting = triple.anchors[0].clone();
    conflicting.span_start_byte = 6;
    triple.anchors.push(conflicting.clone());
    triple.anchors.push(conflicting);
    let port = parser_port(&[triple], ProviderSnapshotCompleteness::Complete)?;
    let result = execute(&port, &present())?;
    assert_eq!(result.outcome(), ProviderQueryOutcome::Unavailable);
    assert_eq!(result.value_facts().count(), 0);

    // Entity identities tombstone on the same terms: [nameA, nameB, nameA].
    let mut triple = shard(Provenance::ExactAst, Confidence::High);
    let mut conflicting = triple.entities[0].clone();
    conflicting.canonical_name = "Other::work".to_string();
    let original = triple.entities[0].clone();
    triple.entities.push(conflicting);
    triple.entities.push(original);
    let port = parser_port(&[triple], ProviderSnapshotCompleteness::Complete)?;
    let result = execute(&port, &present())?;
    assert_eq!(result.outcome(), ProviderQueryOutcome::Unavailable);
    assert_eq!(result.value_facts().count(), 0);
    assert!(
        result
            .evidence()
            .limitations()
            .iter()
            .any(|limitation| limitation.contains("conflicting_duplicate"))
    );

    // Control: identical duplicate rows carry no new information and collapse,
    // so an [A, A] shard keeps its exact answer.
    let mut identical = shard(Provenance::ExactAst, Confidence::High);
    let duplicate = identical.anchors[0].clone();
    identical.anchors.push(duplicate);
    let port = parser_port(&[identical], ProviderSnapshotCompleteness::Complete)?;
    let result = execute(&port, &present())?;
    assert_eq!(result.outcome(), ProviderQueryOutcome::Exact);
    assert_eq!(result.value_facts().count(), 1);
    Ok(())
}

#[test]
fn imperfect_shard_degrades_instead_of_hard_erroring_exact() -> Result<(), Box<dyn Error>> {
    // One good anchored entity beside a limitation-bearing entity: the good
    // query must degrade with the limitation named, never hard-error.
    let mut mixed = shard(Provenance::ExactAst, Confidence::High);
    mixed.entities.push(EntityFact {
        id: EntityId(31),
        kind: EntityKind::Subroutine,
        canonical_name: "Other::broken".to_string(),
        anchor_id: None,
        scope_id: Some(ScopeId(2)),
        provenance: Provenance::ExactAst,
        confidence: Confidence::High,
    });
    let port = parser_port(&[mixed], ProviderSnapshotCompleteness::Complete)?;
    let result = execute(
        &port,
        &request(ProviderQueryKind::Declaration, ProviderQuerySubject::Symbol("work".to_string())),
    )?;
    assert_eq!(result.outcome(), ProviderQueryOutcome::Degraded);
    assert_eq!(result.value_facts().count(), 1);
    assert!(
        result
            .evidence()
            .limitations()
            .iter()
            .any(|limitation| limitation.contains("missing_source_anchor"))
    );

    // Cross-shard blast radius: a tombstoned anchor in shard 1 must not kill
    // exact-eligible answers built only from shard 2; they degrade instead.
    let mut poisoned = shard(Provenance::ExactAst, Confidence::High);
    let mut conflicting = poisoned.anchors[0].clone();
    conflicting.span_start_byte = 6;
    poisoned.anchors.push(conflicting);
    let clean = shard_in_file(FileId(11), Provenance::ExactAst, Confidence::High);
    let port = parser_port(&[poisoned, clean], ProviderSnapshotCompleteness::Complete)?;
    let result = execute(
        &port,
        &request(ProviderQueryKind::Declaration, ProviderQuerySubject::File(FileId(11))),
    )?;
    assert_eq!(result.outcome(), ProviderQueryOutcome::Degraded);
    assert_eq!(result.value_facts().count(), 1);
    assert!(
        result
            .evidence()
            .limitations()
            .iter()
            .any(|limitation| limitation.contains("conflicting_duplicate"))
    );

    // Control: with no limitations anywhere, the same shapes stay exact.
    let clean_first = shard_in_file(FileId(10), Provenance::ExactAst, Confidence::High);
    let clean_second = shard_in_file(FileId(11), Provenance::ExactAst, Confidence::High);
    let port = parser_port(&[clean_first, clean_second], ProviderSnapshotCompleteness::Complete)?;
    let result = execute(
        &port,
        &request(ProviderQueryKind::Declaration, ProviderQuerySubject::File(FileId(11))),
    )?;
    assert_eq!(result.outcome(), ProviderQueryOutcome::Exact);
    assert_eq!(result.value_facts().count(), 1);
    Ok(())
}

#[test]
fn identical_duplicate_rows_collapse_to_one_record() -> Result<(), Box<dyn Error>> {
    // Identical entity rows [A, A] carry no new information: one canonical
    // fact answers the query, not a duplicate-identity contract error.
    let mut dup_entity = shard(Provenance::ExactAst, Confidence::High);
    let duplicate = dup_entity.entities[0].clone();
    dup_entity.entities.push(duplicate);
    let port = parser_port(&[dup_entity], ProviderSnapshotCompleteness::Complete)?;
    let result = execute(
        &port,
        &request(ProviderQueryKind::Declaration, ProviderQuerySubject::Symbol("work".to_string())),
    )?;
    assert_eq!(result.outcome(), ProviderQueryOutcome::Exact);
    assert_eq!(result.value_facts().count(), 1);

    // Identical occurrence rows [A, A] collapse on the same terms.
    let mut dup_occurrence = shard(Provenance::ExactAst, Confidence::High);
    let duplicate = dup_occurrence.occurrences[0].clone();
    dup_occurrence.occurrences.push(duplicate);
    let port = parser_port(&[dup_occurrence], ProviderSnapshotCompleteness::Complete)?;
    let result = execute(
        &port,
        &request(
            ProviderQueryKind::References { include_declaration: false },
            ProviderQuerySubject::Entity(EntityId(30)),
        ),
    )?;
    assert_eq!(result.outcome(), ProviderQueryOutcome::Exact);
    assert_eq!(result.value_facts().count(), 1);
    Ok(())
}

#[test]
fn contradictory_occurrence_kinds_fail_closed_in_both_orders() -> Result<(), Box<dyn Error>> {
    // Same OccurrenceId with contradictory kinds (Definition vs Call) produces
    // EQUAL envelopes because occurrence_kind lives outside the envelope.
    // Collapsing them would make include_declaration=false order-dependent
    // and could mint an exact-empty grant hiding the real call reference.
    let references = |port: &FileFactShardPort| -> Result<ProviderQueryResult, Box<dyn Error>> {
        Ok(execute(
            port,
            &request(
                ProviderQueryKind::References { include_declaration: false },
                ProviderQuerySubject::Entity(EntityId(30)),
            ),
        )?)
    };

    let mut def_first = shard(Provenance::ExactAst, Confidence::High);
    def_first.occurrences[0].kind = OccurrenceKind::Definition;
    let mut call_row = def_first.occurrences[0].clone();
    call_row.kind = OccurrenceKind::Call;
    def_first.occurrences.push(call_row);

    let mut call_first = shard(Provenance::ExactAst, Confidence::High);
    call_first.occurrences[0].kind = OccurrenceKind::Call;
    let mut def_row = call_first.occurrences[0].clone();
    def_row.kind = OccurrenceKind::Definition;
    call_first.occurrences.push(def_row);

    let first = parser_port(&[def_first], ProviderSnapshotCompleteness::Complete)?;
    let second = parser_port(&[call_first], ProviderSnapshotCompleteness::Complete)?;
    let first_result = references(&first)?;
    let second_result = references(&second)?;

    // Both orders agree: the contested occurrence is tombstoned, the
    // capability is downgraded, and no exact-empty grant hides the reference.
    for result in [&first_result, &second_result] {
        assert_eq!(result.outcome(), ProviderQueryOutcome::Unavailable);
        assert_eq!(result.value_facts().count(), 0);
        assert!(!result.is_exact_empty());
        assert!(
            result
                .evidence()
                .limitations()
                .iter()
                .any(|limitation| limitation.contains("occurrence:40:conflicting_duplicate"))
        );
    }
    assert_eq!(serde_json::to_string(&first_result)?, serde_json::to_string(&second_result)?);
    Ok(())
}

#[test]
fn contested_occurrence_cannot_grant_exact_empty_declaration_at_cursor()
-> Result<(), Box<dyn Error>> {
    let contested = || {
        let mut shard = shard(Provenance::ExactAst, Confidence::High);
        shard.occurrences[0].kind = OccurrenceKind::Definition;
        let mut call_row = shard.occurrences[0].clone();
        call_row.kind = OccurrenceKind::Call;
        shard.occurrences.push(call_row);
        shard
    };
    let cursor = || {
        request(
            ProviderQueryKind::Declaration,
            ProviderQuerySubject::Position { file_id: FileId(10), byte_offset: 21 },
        )
    };

    // M10f: with the contested occurrence tombstoned there is no cursor
    // selector; Declarations must be downgraded so no grant claims "no
    // declaration here" over contradictory bindings.
    let port = parser_port(&[contested()], ProviderSnapshotCompleteness::Complete)?;
    let result = execute(&port, &cursor())?;
    assert_eq!(result.outcome(), ProviderQueryOutcome::Unavailable);
    assert_eq!(result.value_facts().count(), 0);
    assert!(!result.is_exact_empty());
    assert!(
        result
            .evidence()
            .limitations()
            .iter()
            .any(|limitation| limitation.contains("occurrence:40:conflicting_duplicate"))
    );

    // M10e: a declaration value query on the same contested shard degrades
    // with the limitation named rather than answering exactly.
    let result = execute(
        &port,
        &request(ProviderQueryKind::Declaration, ProviderQuerySubject::Symbol("work".to_string())),
    )?;
    assert_eq!(result.outcome(), ProviderQueryOutcome::Degraded);
    assert_eq!(result.value_facts().count(), 1);
    assert!(
        result
            .evidence()
            .limitations()
            .iter()
            .any(|limitation| limitation.contains("occurrence:40:conflicting_duplicate"))
    );

    // M10g control: on a clean shard the same cursor resolves the declaration.
    let port = parser_port(
        &[shard(Provenance::ExactAst, Confidence::High)],
        ProviderSnapshotCompleteness::Complete,
    )?;
    let result = execute(&port, &cursor())?;
    assert_eq!(result.outcome(), ProviderQueryOutcome::Exact);
    assert_eq!(result.value_facts().count(), 1);
    Ok(())
}

#[test]
fn suppressed_occurrence_cannot_grant_exact_empty_declaration_at_cursor()
-> Result<(), Box<dyn Error>> {
    // M11: an occurrence whose anchor cannot resolve is suppressed, but it
    // was the cursor selector for position declaration queries — the
    // declarations denominator must downgrade with it.
    let mut broken = shard(Provenance::ExactAst, Confidence::High);
    broken.occurrences[0].anchor_id = AnchorId(99);
    let port = parser_port(&[broken], ProviderSnapshotCompleteness::Complete)?;
    let result = execute(
        &port,
        &request(
            ProviderQueryKind::Declaration,
            ProviderQuerySubject::Position { file_id: FileId(10), byte_offset: 21 },
        ),
    )?;
    assert_eq!(result.outcome(), ProviderQueryOutcome::Unavailable);
    assert_eq!(result.value_facts().count(), 0);
    assert!(!result.is_exact_empty());
    assert!(
        result
            .evidence()
            .limitations()
            .iter()
            .any(|limitation| limitation.contains("occurrence:40:unresolved_source_anchor:99"))
    );

    // Clean control: the same cursor resolves the declaration exactly.
    let port = parser_port(
        &[shard(Provenance::ExactAst, Confidence::High)],
        ProviderSnapshotCompleteness::Complete,
    )?;
    let result = execute(
        &port,
        &request(
            ProviderQueryKind::Declaration,
            ProviderQuerySubject::Position { file_id: FileId(10), byte_offset: 21 },
        ),
    )?;
    assert_eq!(result.outcome(), ProviderQueryOutcome::Exact);
    assert_eq!(result.value_facts().count(), 1);
    Ok(())
}

#[test]
fn suppressed_declaration_cannot_grant_exact_empty_references_at_cursor()
-> Result<(), Box<dyn Error>> {
    // M12: an entity with a missing anchor is suppressed (the primary
    // documented partial-extraction path), but its declaration record was
    // the cursor selector for position references queries — the references
    // denominator must downgrade with it.
    let mut broken = shard(Provenance::ExactAst, Confidence::High);
    broken.entities[0].anchor_id = None;
    let port = parser_port(&[broken], ProviderSnapshotCompleteness::Complete)?;
    let result = execute(
        &port,
        &request(
            ProviderQueryKind::References { include_declaration: false },
            ProviderQuerySubject::Position { file_id: FileId(10), byte_offset: 5 },
        ),
    )?;
    assert_eq!(result.outcome(), ProviderQueryOutcome::Unavailable);
    assert_eq!(result.value_facts().count(), 0);
    assert!(!result.is_exact_empty());
    assert!(
        result
            .evidence()
            .limitations()
            .iter()
            .any(|limitation| limitation.contains("entity:30:missing_source_anchor"))
    );

    // Clean control: the same cursor resolves the reference set exactly.
    let port = parser_port(
        &[shard(Provenance::ExactAst, Confidence::High)],
        ProviderSnapshotCompleteness::Complete,
    )?;
    let result = execute(
        &port,
        &request(
            ProviderQueryKind::References { include_declaration: false },
            ProviderQuerySubject::Position { file_id: FileId(10), byte_offset: 5 },
        ),
    )?;
    assert_eq!(result.outcome(), ProviderQueryOutcome::Exact);
    assert_eq!(result.value_facts().count(), 1);
    Ok(())
}
