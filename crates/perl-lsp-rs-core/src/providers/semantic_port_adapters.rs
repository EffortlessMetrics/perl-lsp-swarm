//! Truthful adapters from current semantic fact producers into the provider port.
//!
//! The adapters in this module do not change live provider behavior. They turn
//! existing [`perl_workspace::workspace::workspace_index::FileFactShard`] data
//! and already-canonical [`SemanticFactEnvelope`] values into the query contract
//! from [`super::semantic_port`]. Every adapter requires explicit generation,
//! freshness, proof-ceiling, and completeness inputs; no producer name upgrades
//! proof and no missing source anchor is fabricated.

use super::semantic_port::{
    ProviderCancellationState, ProviderProofClass, ProviderQueryDeadline, ProviderQueryEvidence,
    ProviderQueryKind, ProviderQueryOutcome, ProviderQueryRequest, ProviderQueryResult,
    ProviderQuerySubject, ProviderReadinessState, ProviderSemanticPort,
};
use perl_semantic_facts::{
    AnchorFact, AnchorId, BoundaryDisposition, BoundaryKind, BoundaryLink, Confidence, EntityFact,
    EntityId, FactId, LifecyclePhase, OccurrenceFact, OccurrenceKind, Provenance,
    ProviderFactFreshness, ProviderFactSourceKind, ProviderFactTrace, ProviderFallbackState,
    SemanticConfidence, SemanticFactEnvelope, SemanticFactKind, SemanticFactStatus,
    SemanticFreshness, SemanticProducer, SemanticProvenance, SemanticReasonCode, SourceAnchor,
    SourceGeneration,
};
use perl_workspace::workspace::workspace_index::FileFactShard;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;

/// Completeness of an adapter snapshot for one query capability.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ProviderSnapshotCompleteness {
    /// The producer asserts that the capability's supported denominator is complete.
    Complete,
    /// The producer knows the snapshot is partial.
    Partial,
    /// Completeness was not measured.
    Unknown,
}

/// Query family whose completeness can be declared independently.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ProviderQueryCapability {
    /// Declaration and entity facts.
    Declarations,
    /// Reference occurrence facts.
    References,
    /// Module/import/export visibility facts.
    Visibility,
    /// Scope and binding facts.
    ScopeBindings,
    /// Dynamic or compatibility boundaries.
    Boundaries,
    /// Readiness-only query state.
    Readiness,
}

impl ProviderQueryCapability {
    fn from_query(kind: &ProviderQueryKind) -> Self {
        match kind {
            ProviderQueryKind::Declaration => Self::Declarations,
            ProviderQueryKind::References { .. } => Self::References,
            ProviderQueryKind::Visibility => Self::Visibility,
            ProviderQueryKind::ScopeBindings => Self::ScopeBindings,
            ProviderQueryKind::Boundaries => Self::Boundaries,
            ProviderQueryKind::Readiness => Self::Readiness,
            _ => Self::Readiness,
        }
    }
}

/// Explicit snapshot metadata supplied by the fact owner.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderAdapterSnapshot {
    /// Source/document generation represented by the adapted facts.
    pub document_generation: SourceGeneration,
    /// Workspace/model generation represented by the adapted facts.
    pub workspace_generation: SourceGeneration,
    /// Freshness of the adapted fact set.
    pub freshness: SemanticFreshness,
    /// Lifecycle phase for the adapted facts.
    pub lifecycle: LifecyclePhase,
    /// Highest proof class this adapter may emit.
    pub proof_ceiling: ProviderProofClass,
    /// How provider traces should describe the adapter path.
    pub fallback_state: ProviderFallbackState,
    /// Optional model/schema version used by the producer.
    pub model_version: Option<u32>,
    authority_producers: Vec<SemanticProducer>,
    completeness: BTreeMap<ProviderQueryCapability, ProviderSnapshotCompleteness>,
}

impl ProviderAdapterSnapshot {
    /// Construct explicit snapshot metadata and canonicalize authority and completeness rows.
    #[allow(clippy::too_many_arguments)] // mirrors the adapter boundary fields
    #[must_use]
    pub fn new(
        document_generation: SourceGeneration,
        workspace_generation: SourceGeneration,
        freshness: SemanticFreshness,
        lifecycle: LifecyclePhase,
        proof_ceiling: ProviderProofClass,
        fallback_state: ProviderFallbackState,
        model_version: Option<u32>,
        authority_producers: impl IntoIterator<Item = SemanticProducer>,
        completeness: impl IntoIterator<
            Item = (ProviderQueryCapability, ProviderSnapshotCompleteness),
        >,
    ) -> Self {
        let mut authority_producers: Vec<_> = authority_producers
            .into_iter()
            .filter(|producer| *producer != SemanticProducer::Unknown)
            .collect();
        authority_producers.sort();
        authority_producers.dedup();
        Self {
            document_generation,
            workspace_generation,
            freshness,
            lifecycle,
            proof_ceiling,
            fallback_state,
            model_version,
            authority_producers,
            completeness: completeness.into_iter().collect(),
        }
    }

    /// Completeness declared for one query family.
    #[must_use]
    pub fn completeness(
        &self,
        capability: ProviderQueryCapability,
    ) -> ProviderSnapshotCompleteness {
        self.completeness
            .get(&capability)
            .copied()
            .unwrap_or(ProviderSnapshotCompleteness::Unknown)
    }

    /// Deterministically ordered producers allowed to establish query completeness.
    #[must_use]
    pub fn authority_producers(&self) -> &[SemanticProducer] {
        &self.authority_producers
    }

    fn register_authority(&mut self, producer: SemanticProducer) {
        if producer == SemanticProducer::Unknown {
            return;
        }
        self.authority_producers.push(producer);
        self.authority_producers.sort();
        self.authority_producers.dedup();
    }

    fn remove_unsubstantiated_compiler_authority(&mut self) {
        self.authority_producers.retain(|producer| {
            !matches!(
                producer,
                SemanticProducer::Hir
                    | SemanticProducer::PirA
                    | SemanticProducer::FrameworkAdapter
            )
        });
    }

    fn downgrade(&mut self, capability: ProviderQueryCapability) {
        if self.completeness(capability) == ProviderSnapshotCompleteness::Complete {
            self.completeness
                .insert(capability, ProviderSnapshotCompleteness::Partial);
        }
    }

    fn can_claim_exact(&self, capability: ProviderQueryCapability) -> bool {
        self.completeness(capability) == ProviderSnapshotCompleteness::Complete
            && generation_is_known(&self.document_generation)
            && generation_is_known(&self.workspace_generation)
            && self.freshness == SemanticFreshness::Fresh
            && !self.authority_producers.is_empty()
            && matches!(
                self.proof_ceiling,
                ProviderProofClass::ExactRead | ProviderProofClass::EditAuthorizing
            )
    }
}

/// Adapter construction error that prevents truthful attribution.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderAdapterError {
    /// A file fact shard cannot be relabeled as a compiler or framework producer.
    UnsupportedShardProducer(SemanticProducer),
    /// The trace source is inconsistent with the declared shard producer.
    UnsupportedTraceSource {
        /// Declared producer of the file fact shard.
        producer: SemanticProducer,
        /// Requested provider-trace source class.
        source: ProviderFactSourceKind,
    },
    /// A raw fact-shard adapter cannot authorize edits.
    EditAuthorizationRequiresPlan,
}

impl fmt::Display for ProviderAdapterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedShardProducer(producer) => {
                write!(formatter, "file fact shard cannot be attributed to {producer:?}")
            }
            Self::UnsupportedTraceSource { producer, source } => {
                write!(
                    formatter,
                    "trace source {source:?} is invalid for producer {producer:?}"
                )
            }
            Self::EditAuthorizationRequiresPlan => formatter.write_str(
                "file fact shard cannot authorize edits without a current guarded edit plan",
            ),
        }
    }
}

impl Error for ProviderAdapterError {}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AdapterFactRecord {
    envelope: SemanticFactEnvelope,
    names: Vec<String>,
    occurrence_kind: Option<OccurrenceKind>,
    trace: Option<ProviderFactTrace>,
}

impl AdapterFactRecord {
    fn canonicalize(&mut self) {
        self.names.retain(|name| !name.trim().is_empty());
        self.names.sort();
        self.names.dedup();
    }
}

/// Adapter over current parser/semantic/workspace [`FileFactShard`] values.
///
/// The caller supplies the actual producer. `Hir`, `PirA`,
/// `FrameworkAdapter`, and `Unknown` are rejected because a workspace shard is
/// not evidence that those producers contributed.
pub struct FileFactShardPort {
    records: Vec<AdapterFactRecord>,
    snapshot: ProviderAdapterSnapshot,
    limitations: Vec<String>,
}

impl FileFactShardPort {
    /// Adapt file fact shards with explicit producer and trace identity.
    pub fn new(
        shards: &[FileFactShard],
        producer: SemanticProducer,
        trace_source: ProviderFactSourceKind,
        mut snapshot: ProviderAdapterSnapshot,
    ) -> Result<Self, ProviderAdapterError> {
        if matches!(
            producer,
            SemanticProducer::Hir
                | SemanticProducer::PirA
                | SemanticProducer::FrameworkAdapter
                | SemanticProducer::Unknown
        ) {
            return Err(ProviderAdapterError::UnsupportedShardProducer(producer));
        }
        if !trace_source_allowed(producer, trace_source) {
            return Err(ProviderAdapterError::UnsupportedTraceSource {
                producer,
                source: trace_source,
            });
        }
        if snapshot.proof_ceiling == ProviderProofClass::EditAuthorizing {
            return Err(ProviderAdapterError::EditAuthorizationRequiresPlan);
        }
        snapshot.register_authority(producer);

        let mut records = Vec::new();
        let mut limitations = Vec::new();
        let mut incomplete = BTreeSet::new();
        for shard in shards {
            adapt_shard(
                shard,
                producer,
                trace_source,
                &snapshot,
                &mut records,
                &mut limitations,
                &mut incomplete,
            );
        }
        for capability in incomplete {
            snapshot.downgrade(capability);
        }
        records.sort_by_key(|record| record.envelope.fact_id);
        limitations.sort();
        limitations.dedup();
        Ok(Self {
            records,
            snapshot,
            limitations,
        })
    }
}

impl ProviderSemanticPort for FileFactShardPort {
    fn query(
        &self,
        request: &ProviderQueryRequest,
    ) -> ProviderQueryResult<SemanticFactEnvelope> {
        query_records(
            request,
            &self.records,
            &self.snapshot,
            &self.limitations,
        )
    }
}

/// Adapter over facts that already use the canonical semantic envelope.
///
/// This is the only adapter in this slice that may carry `Hir`, `PirA`, or
/// `FrameworkAdapter` producer identities, and it preserves each envelope's
/// producer verbatim. It cannot manufacture compiler attribution when no
/// compiler envelope is present.
pub struct CanonicalEnvelopePort {
    records: Vec<AdapterFactRecord>,
    snapshot: ProviderAdapterSnapshot,
    limitations: Vec<String>,
}

impl CanonicalEnvelopePort {
    /// Construct an adapter over already-canonical facts.
    #[must_use]
    pub fn new(
        envelopes: &[SemanticFactEnvelope],
        mut snapshot: ProviderAdapterSnapshot,
    ) -> Self {
        snapshot.remove_unsubstantiated_compiler_authority();
        for producer in envelopes.iter().map(|envelope| envelope.producer) {
            snapshot.register_authority(producer);
        }
        let mut records: Vec<_> = envelopes
            .iter()
            .cloned()
            .map(|envelope| {
                let trace = trace_from_envelope(
                    &envelope,
                    ProviderFactSourceKind::SemanticFact,
                    snapshot.fallback_state,
                    snapshot.model_version,
                );
                AdapterFactRecord {
                    names: envelope.package.iter().cloned().collect(),
                    occurrence_kind: None,
                    envelope,
                    trace,
                }
            })
            .collect();
        for record in &mut records {
            record.canonicalize();
        }
        records.sort_by_key(|record| record.envelope.fact_id);
        Self {
            records,
            snapshot,
            limitations: Vec::new(),
        }
    }
}

impl ProviderSemanticPort for CanonicalEnvelopePort {
    fn query(
        &self,
        request: &ProviderQueryRequest,
    ) -> ProviderQueryResult<SemanticFactEnvelope> {
        query_records(
            request,
            &self.records,
            &self.snapshot,
            &self.limitations,
        )
    }
}

#[allow(clippy::too_many_arguments)]
fn adapt_shard(
    shard: &FileFactShard,
    producer: SemanticProducer,
    trace_source: ProviderFactSourceKind,
    snapshot: &ProviderAdapterSnapshot,
    records: &mut Vec<AdapterFactRecord>,
    limitations: &mut Vec<String>,
    incomplete: &mut BTreeSet<ProviderQueryCapability>,
) {
    let anchors: BTreeMap<AnchorId, &AnchorFact> = shard
        .anchors
        .iter()
        .map(|anchor| (anchor.id, anchor))
        .collect();
    let entity_names: BTreeMap<EntityId, String> = shard
        .entities
        .iter()
        .map(|entity| (entity.id, entity.canonical_name.clone()))
        .collect();

    for entity in &shard.entities {
        let Some(anchor_id) = entity.anchor_id else {
            limitations.push(format!("entity:{}:missing_source_anchor", entity.id.0));
            incomplete.extend([
                ProviderQueryCapability::Declarations,
                ProviderQueryCapability::Visibility,
                ProviderQueryCapability::ScopeBindings,
            ]);
            continue;
        };
        let Some(anchor) = anchors.get(&anchor_id).copied() else {
            limitations.push(format!(
                "entity:{}:unresolved_source_anchor:{}",
                entity.id.0, anchor_id.0
            ));
            incomplete.extend([
                ProviderQueryCapability::Declarations,
                ProviderQueryCapability::Visibility,
                ProviderQueryCapability::ScopeBindings,
            ]);
            continue;
        };
        let mut record = record_from_entity(
            entity,
            anchor,
            producer,
            trace_source,
            snapshot,
            shard,
        );
        record.canonicalize();
        records.push(record);
    }

    for occurrence in &shard.occurrences {
        let Some(anchor) = anchors.get(&occurrence.anchor_id).copied() else {
            limitations.push(format!(
                "occurrence:{}:unresolved_source_anchor:{}",
                occurrence.id.0, occurrence.anchor_id.0
            ));
            incomplete.extend([
                ProviderQueryCapability::References,
                ProviderQueryCapability::Visibility,
                ProviderQueryCapability::ScopeBindings,
                ProviderQueryCapability::Boundaries,
            ]);
            continue;
        };
        let mut record = record_from_occurrence(
            occurrence,
            anchor,
            occurrence
                .entity_id
                .and_then(|entity_id| entity_names.get(&entity_id))
                .cloned(),
            producer,
            trace_source,
            snapshot,
            shard,
        );
        record.canonicalize();
        records.push(record);
    }
}

fn record_from_entity(
    entity: &EntityFact,
    anchor: &AnchorFact,
    producer: SemanticProducer,
    trace_source: ProviderFactSourceKind,
    snapshot: &ProviderAdapterSnapshot,
    shard: &FileFactShard,
) -> AdapterFactRecord {
    let fact_id = stable_fact_id(b"entity", entity.id.0);
    let envelope = SemanticFactEnvelope::new(
        fact_id,
        Some(entity.id),
        semantic_kind_for_entity(entity),
        source_anchor(anchor),
        snapshot.document_generation.clone(),
        entity.scope_id,
        package_from_canonical_name(&entity.canonical_name),
        snapshot.lifecycle,
        producer,
        SemanticProvenance::Known(entity.provenance),
        SemanticConfidence::Known(entity.confidence),
        snapshot.freshness,
        boundary_for(entity.provenance, fact_id),
        Vec::new(),
        reason_for(entity.provenance),
    );
    let trace = trace_from_fact(
        &envelope,
        trace_source,
        snapshot.fallback_state,
        Some(format!("content:{:016x}", shard.content_hash)),
        Some(shard.producer_schema_version),
    );
    let mut names = vec![entity.canonical_name.clone()];
    if let Some((_, bare)) = entity.canonical_name.rsplit_once("::") {
        names.push(bare.to_string());
    }
    AdapterFactRecord {
        envelope,
        names,
        occurrence_kind: None,
        trace,
    }
}

#[allow(clippy::too_many_arguments)]
fn record_from_occurrence(
    occurrence: &OccurrenceFact,
    anchor: &AnchorFact,
    entity_name: Option<String>,
    producer: SemanticProducer,
    trace_source: ProviderFactSourceKind,
    snapshot: &ProviderAdapterSnapshot,
    shard: &FileFactShard,
) -> AdapterFactRecord {
    let fact_id = stable_fact_id(b"occurrence", occurrence.id.0);
    let is_dynamic = occurrence.kind == OccurrenceKind::DynamicBoundary;
    let envelope = SemanticFactEnvelope::new(
        fact_id,
        occurrence.entity_id,
        if is_dynamic {
            SemanticFactKind::Boundary
        } else if occurrence.kind == OccurrenceKind::Import {
            SemanticFactKind::Import
        } else {
            SemanticFactKind::Occurrence
        },
        source_anchor(anchor),
        snapshot.document_generation.clone(),
        occurrence.scope_id,
        entity_name.as_deref().and_then(package_from_canonical_name),
        snapshot.lifecycle,
        producer,
        SemanticProvenance::Known(occurrence.provenance),
        SemanticConfidence::Known(occurrence.confidence),
        snapshot.freshness,
        if is_dynamic {
            Some(BoundaryLink::new(
                Some(fact_id),
                BoundaryKind::DynamicValue,
                BoundaryDisposition::Degrade,
                SemanticReasonCode::DynamicValue,
            ))
        } else {
            boundary_for(occurrence.provenance, fact_id)
        },
        Vec::new(),
        if is_dynamic {
            SemanticReasonCode::DynamicValue
        } else {
            reason_for(occurrence.provenance)
        },
    );
    let trace = trace_from_fact(
        &envelope,
        trace_source,
        snapshot.fallback_state,
        Some(format!("content:{:016x}", shard.content_hash)),
        Some(shard.producer_schema_version),
    );
    AdapterFactRecord {
        envelope,
        names: entity_name.into_iter().collect(),
        occurrence_kind: Some(occurrence.kind),
        trace,
    }
}

fn source_anchor(anchor: &AnchorFact) -> SourceAnchor {
    SourceAnchor::new(
        Some(anchor.id),
        anchor.file_id,
        anchor.span_start_byte,
        anchor.span_end_byte,
    )
}

fn semantic_kind_for_entity(entity: &EntityFact) -> SemanticFactKind {
    match entity.kind {
        perl_semantic_facts::EntityKind::Module => SemanticFactKind::Module,
        _ => SemanticFactKind::Declaration,
    }
}

fn package_from_canonical_name(name: &str) -> Option<String> {
    name.rsplit_once("::")
        .map(|(package, _)| package)
        .filter(|package| !package.is_empty())
        .map(str::to_string)
}

fn reason_for(provenance: Provenance) -> SemanticReasonCode {
    match provenance {
        Provenance::ExactAst
        | Provenance::DesugaredAst
        | Provenance::SemanticAnalyzer
        | Provenance::LiteralRequireImport => SemanticReasonCode::ExactSource,
        Provenance::FrameworkSynthesis
        | Provenance::ImportExportInference
        | Provenance::PragmaInference => SemanticReasonCode::GeneratedFromSource,
        Provenance::DynamicBoundary => SemanticReasonCode::DynamicValue,
        Provenance::NameHeuristic | Provenance::SearchFallback => {
            SemanticReasonCode::CompatibilityBoundary
        }
    }
}

fn boundary_for(provenance: Provenance, fact_id: FactId) -> Option<BoundaryLink> {
    match provenance {
        Provenance::DynamicBoundary => Some(BoundaryLink::new(
            Some(fact_id),
            BoundaryKind::DynamicValue,
            BoundaryDisposition::Degrade,
            SemanticReasonCode::DynamicValue,
        )),
        Provenance::NameHeuristic | Provenance::SearchFallback => Some(BoundaryLink::new(
            Some(fact_id),
            BoundaryKind::Compatibility,
            BoundaryDisposition::Degrade,
            SemanticReasonCode::CompatibilityBoundary,
        )),
        _ => None,
    }
}

fn stable_fact_id(domain: &[u8], raw: u64) -> FactId {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in domain.iter().copied().chain(raw.to_le_bytes()) {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    FactId(hash)
}

fn trace_source_allowed(
    producer: SemanticProducer,
    source: ProviderFactSourceKind,
) -> bool {
    match producer {
        SemanticProducer::Parser => source == ProviderFactSourceKind::ParserSyntax,
        SemanticProducer::SemanticAnalyzer => matches!(
            source,
            ProviderFactSourceKind::SemanticFact | ProviderFactSourceKind::DynamicBoundary
        ),
        SemanticProducer::WorkspaceIndex => matches!(
            source,
            ProviderFactSourceKind::LegacyWorkspace | ProviderFactSourceKind::SemanticFact
        ),
        _ => false,
    }
}

fn trace_from_envelope(
    envelope: &SemanticFactEnvelope,
    default_source: ProviderFactSourceKind,
    fallback_state: ProviderFallbackState,
    model_version: Option<u32>,
) -> Option<ProviderFactTrace> {
    trace_from_fact(
        envelope,
        source_for_producer(envelope.producer, default_source),
        fallback_state,
        None,
        model_version,
    )
}

fn trace_from_fact(
    envelope: &SemanticFactEnvelope,
    source: ProviderFactSourceKind,
    fallback_state: ProviderFallbackState,
    source_hash: Option<String>,
    model_version: Option<u32>,
) -> Option<ProviderFactTrace> {
    let SemanticProvenance::Known(provenance) = envelope.provenance else {
        return None;
    };
    let SemanticConfidence::Known(confidence) = envelope.confidence else {
        return None;
    };
    Some(ProviderFactTrace::new(
        perl_semantic_facts::ProviderSurface::Definition,
        source,
        provenance,
        confidence,
        provider_freshness(envelope.freshness),
        fallback_state,
        source_hash,
        envelope.anchor.anchor_id,
        model_version,
    ))
}

fn source_for_producer(
    producer: SemanticProducer,
    default_source: ProviderFactSourceKind,
) -> ProviderFactSourceKind {
    match producer {
        SemanticProducer::Parser => ProviderFactSourceKind::ParserSyntax,
        SemanticProducer::Hir | SemanticProducer::PirA => ProviderFactSourceKind::CompilerFact,
        SemanticProducer::SemanticAnalyzer => ProviderFactSourceKind::SemanticFact,
        SemanticProducer::WorkspaceIndex => default_source,
        SemanticProducer::FrameworkAdapter => ProviderFactSourceKind::FrameworkAdapter,
        SemanticProducer::Unknown => ProviderFactSourceKind::Unknown,
        _ => ProviderFactSourceKind::Unknown,
    }
}

fn provider_freshness(freshness: SemanticFreshness) -> ProviderFactFreshness {
    match freshness {
        SemanticFreshness::Fresh => ProviderFactFreshness::Fresh,
        SemanticFreshness::Stale => ProviderFactFreshness::Stale,
        SemanticFreshness::Unknown => ProviderFactFreshness::Unknown,
        SemanticFreshness::NotApplicable => ProviderFactFreshness::NotApplicable,
        _ => ProviderFactFreshness::Unknown,
    }
}

fn generation_is_known(generation: &SourceGeneration) -> bool {
    matches!(generation, SourceGeneration::Known(value) if !value.trim().is_empty())
}

fn query_records(
    request: &ProviderQueryRequest,
    records: &[AdapterFactRecord],
    snapshot: &ProviderAdapterSnapshot,
    limitations: &[String],
) -> ProviderQueryResult<SemanticFactEnvelope> {
    if !request.is_well_formed() {
        return no_value_result(
            ProviderQueryOutcome::Error,
            request,
            snapshot,
            Vec::new(),
            extended_limitations(limitations, "malformed_provider_query"),
        );
    }
    if request.context.cancellation == ProviderCancellationState::Cancelled {
        return no_value_result(
            ProviderQueryOutcome::Cancelled,
            request,
            snapshot,
            Vec::new(),
            limitations.to_vec(),
        );
    }
    if request.context.deadline == ProviderQueryDeadline::Expired {
        return no_value_result(
            ProviderQueryOutcome::DeadlineExceeded,
            request,
            snapshot,
            Vec::new(),
            limitations.to_vec(),
        );
    }

    let capability = ProviderQueryCapability::from_query(&request.kind);
    if capability == ProviderQueryCapability::Readiness {
        return readiness_result(request, snapshot, limitations);
    }

    let values = select_value_records(request, records);
    let blockers = if capability == ProviderQueryCapability::Boundaries {
        Vec::new()
    } else {
        select_boundary_records(&request.subject, records)
    };
    let evidence_records = merge_record_sets(&values, &blockers);

    if values.is_empty() {
        if !blockers.is_empty() {
            return no_value_result(
                ProviderQueryOutcome::Dynamic,
                request,
                snapshot,
                evidence_records,
                limitations.to_vec(),
            );
        }
        if snapshot.can_claim_exact(capability) {
            return value_result(
                ProviderQueryOutcome::Exact,
                request,
                snapshot,
                Vec::new(),
                evidence_records,
                limitations.to_vec(),
            );
        }
        return no_value_result(
            ProviderQueryOutcome::Unavailable,
            request,
            snapshot,
            evidence_records,
            limitations.to_vec(),
        );
    }

    let evidence_facts: Vec<_> = evidence_records
        .iter()
        .map(|record| record.envelope.clone())
        .collect();
    if evidence_facts
        .iter()
        .any(|fact| fact.status() == SemanticFactStatus::Stale)
    {
        return no_value_result(
            ProviderQueryOutcome::Stale,
            request,
            snapshot,
            evidence_records,
            limitations.to_vec(),
        );
    }
    if evidence_facts
        .iter()
        .any(|fact| fact.status() == SemanticFactStatus::Refused)
    {
        return no_value_result(
            ProviderQueryOutcome::Refused,
            request,
            snapshot,
            evidence_records,
            limitations.to_vec(),
        );
    }
    if snapshot.proof_ceiling == ProviderProofClass::FallbackOnly {
        return value_result(
            ProviderQueryOutcome::Fallback,
            request,
            snapshot,
            values,
            evidence_records,
            limitations.to_vec(),
        );
    }
    if blockers.is_empty()
        && snapshot.can_claim_exact(capability)
        && values
            .iter()
            .all(|record| record.envelope.status() == SemanticFactStatus::Exact)
    {
        value_result(
            ProviderQueryOutcome::Exact,
            request,
            snapshot,
            values,
            evidence_records,
            limitations.to_vec(),
        )
    } else {
        value_result(
            ProviderQueryOutcome::Degraded,
            request,
            snapshot,
            values,
            evidence_records,
            limitations.to_vec(),
        )
    }
}

fn readiness_result(
    request: &ProviderQueryRequest,
    snapshot: &ProviderAdapterSnapshot,
    limitations: &[String],
) -> ProviderQueryResult<SemanticFactEnvelope> {
    match request.context.readiness_state {
        ProviderReadinessState::Ready
            if snapshot.can_claim_exact(ProviderQueryCapability::Readiness) =>
        {
            value_result(
                ProviderQueryOutcome::Exact,
                request,
                snapshot,
                Vec::new(),
                Vec::new(),
                limitations.to_vec(),
            )
        }
        ProviderReadinessState::Ready | ProviderReadinessState::ReadyLimited => value_result(
            ProviderQueryOutcome::Degraded,
            request,
            snapshot,
            Vec::new(),
            Vec::new(),
            limitations.to_vec(),
        ),
        ProviderReadinessState::Stale => no_value_result(
            ProviderQueryOutcome::Stale,
            request,
            snapshot,
            Vec::new(),
            limitations.to_vec(),
        ),
        ProviderReadinessState::Failed => no_value_result(
            ProviderQueryOutcome::Error,
            request,
            snapshot,
            Vec::new(),
            limitations.to_vec(),
        ),
        ProviderReadinessState::Building | ProviderReadinessState::Unavailable => no_value_result(
            ProviderQueryOutcome::Unavailable,
            request,
            snapshot,
            Vec::new(),
            limitations.to_vec(),
        ),
        _ => no_value_result(
            ProviderQueryOutcome::Unavailable,
            request,
            snapshot,
            Vec::new(),
            limitations.to_vec(),
        ),
    }
}

fn select_value_records<'a>(
    request: &ProviderQueryRequest,
    records: &'a [AdapterFactRecord],
) -> Vec<&'a AdapterFactRecord> {
    let mut selected: Vec<_> = match &request.kind {
        ProviderQueryKind::References {
            include_declaration,
        } => select_reference_records(request, records, *include_declaration),
        ProviderQueryKind::ScopeBindings => select_scope_records(request, records),
        _ => records
            .iter()
            .filter(|record| kind_matches(&request.kind, record))
            .filter(|record| subject_matches(&request.subject, record))
            .collect(),
    };
    selected.sort_by_key(|record| record.envelope.fact_id);
    selected.dedup_by_key(|record| record.envelope.fact_id);
    selected
}

fn select_boundary_records<'a>(
    subject: &ProviderQuerySubject,
    records: &'a [AdapterFactRecord],
) -> Vec<&'a AdapterFactRecord> {
    let mut selected: Vec<_> = records
        .iter()
        .filter(|record| record.envelope.kind == SemanticFactKind::Boundary)
        .filter(|record| subject_matches(subject, record))
        .collect();
    selected.sort_by_key(|record| record.envelope.fact_id);
    selected.dedup_by_key(|record| record.envelope.fact_id);
    selected
}

fn merge_record_sets<'a>(
    first: &[&'a AdapterFactRecord],
    second: &[&'a AdapterFactRecord],
) -> Vec<&'a AdapterFactRecord> {
    let mut records = Vec::with_capacity(first.len() + second.len());
    records.extend_from_slice(first);
    records.extend_from_slice(second);
    records.sort_by_key(|record| record.envelope.fact_id);
    records.dedup_by_key(|record| record.envelope.fact_id);
    records
}

fn select_reference_records<'a>(
    request: &ProviderQueryRequest,
    records: &'a [AdapterFactRecord],
    include_declaration: bool,
) -> Vec<&'a AdapterFactRecord> {
    let target_entities = target_entity_ids(&request.subject, records);
    records
        .iter()
        .filter(|record| {
            let occurrence = record.envelope.kind == SemanticFactKind::Occurrence;
            let declaration = include_declaration
                && matches!(
                    record.envelope.kind,
                    SemanticFactKind::Declaration | SemanticFactKind::Module
                );
            (occurrence || declaration)
                && record
                    .envelope
                    .entity_id
                    .is_some_and(|entity_id| target_entities.contains(&entity_id))
        })
        .filter(|record| {
            include_declaration
                || record.occurrence_kind != Some(OccurrenceKind::Definition)
        })
        .collect()
}

fn select_scope_records<'a>(
    request: &ProviderQueryRequest,
    records: &'a [AdapterFactRecord],
) -> Vec<&'a AdapterFactRecord> {
    let scope_ids: BTreeSet<_> = records
        .iter()
        .filter(|record| subject_matches(&request.subject, record))
        .filter_map(|record| record.envelope.scope_id)
        .collect();
    records
        .iter()
        .filter(|record| {
            matches!(
                record.envelope.kind,
                SemanticFactKind::Declaration | SemanticFactKind::Occurrence
            ) && record
                .envelope
                .scope_id
                .is_some_and(|scope_id| scope_ids.contains(&scope_id))
        })
        .collect()
}

fn target_entity_ids(
    subject: &ProviderQuerySubject,
    records: &[AdapterFactRecord],
) -> BTreeSet<EntityId> {
    match subject {
        ProviderQuerySubject::Entity(entity_id) => BTreeSet::from([*entity_id]),
        _ => records
            .iter()
            .filter(|record| {
                matches!(
                    record.envelope.kind,
                    SemanticFactKind::Declaration | SemanticFactKind::Module
                ) && subject_matches(subject, record)
            })
            .filter_map(|record| record.envelope.entity_id)
            .collect(),
    }
}

fn kind_matches(kind: &ProviderQueryKind, record: &AdapterFactRecord) -> bool {
    match kind {
        ProviderQueryKind::Declaration => matches!(
            record.envelope.kind,
            SemanticFactKind::Declaration | SemanticFactKind::Module
        ),
        ProviderQueryKind::Visibility => matches!(
            record.envelope.kind,
            SemanticFactKind::Import | SemanticFactKind::Module
        ),
        ProviderQueryKind::Boundaries => record.envelope.kind == SemanticFactKind::Boundary,
        ProviderQueryKind::References { .. }
        | ProviderQueryKind::ScopeBindings
        | ProviderQueryKind::Readiness => false,
        _ => false,
    }
}

fn subject_matches(subject: &ProviderQuerySubject, record: &AdapterFactRecord) -> bool {
    match subject {
        ProviderQuerySubject::Entity(entity_id) => record.envelope.entity_id == Some(*entity_id),
        ProviderQuerySubject::File(file_id) => record.envelope.anchor.file_id == *file_id,
        ProviderQuerySubject::Position {
            file_id,
            byte_offset,
        } => {
            record.envelope.anchor.file_id == *file_id
                && range_contains(&record.envelope.anchor, *byte_offset)
        }
        ProviderQuerySubject::Package(package) => {
            record.envelope.package.as_deref() == Some(package.as_str())
                || record.names.iter().any(|name| name == package)
        }
        ProviderQuerySubject::Symbol(symbol) => record.names.iter().any(|name| name == symbol),
        ProviderQuerySubject::Workspace => true,
        _ => false,
    }
}

fn range_contains(anchor: &SourceAnchor, byte_offset: u32) -> bool {
    if anchor.start_byte == anchor.end_byte {
        byte_offset == anchor.start_byte
    } else {
        anchor.start_byte <= byte_offset && byte_offset < anchor.end_byte
    }
}

fn value_result(
    outcome: ProviderQueryOutcome,
    request: &ProviderQueryRequest,
    snapshot: &ProviderAdapterSnapshot,
    values: Vec<&AdapterFactRecord>,
    evidence_records: Vec<&AdapterFactRecord>,
    limitations: Vec<String>,
) -> ProviderQueryResult<SemanticFactEnvelope> {
    let values: Vec<_> = values
        .iter()
        .map(|record| record.envelope.clone())
        .collect();
    let evidence = evidence_for(
        outcome,
        request,
        snapshot,
        evidence_records,
        limitations,
    );
    match outcome {
        ProviderQueryOutcome::Exact => ProviderQueryResult::exact(values, evidence),
        ProviderQueryOutcome::Fallback => ProviderQueryResult::fallback(values, evidence),
        _ => ProviderQueryResult::degraded(values, evidence),
    }
}

fn no_value_result(
    outcome: ProviderQueryOutcome,
    request: &ProviderQueryRequest,
    snapshot: &ProviderAdapterSnapshot,
    evidence_records: Vec<&AdapterFactRecord>,
    limitations: Vec<String>,
) -> ProviderQueryResult<SemanticFactEnvelope> {
    let evidence = evidence_for(
        outcome,
        request,
        snapshot,
        evidence_records,
        limitations,
    );
    match outcome {
        ProviderQueryOutcome::Refused => ProviderQueryResult::refused(evidence),
        ProviderQueryOutcome::Stale => ProviderQueryResult::stale(evidence),
        ProviderQueryOutcome::Dynamic => ProviderQueryResult::dynamic(evidence),
        ProviderQueryOutcome::Ambiguous => ProviderQueryResult::ambiguous(evidence),
        ProviderQueryOutcome::Cancelled => ProviderQueryResult::cancelled(evidence),
        ProviderQueryOutcome::DeadlineExceeded => ProviderQueryResult::deadline_exceeded(evidence),
        ProviderQueryOutcome::Error => ProviderQueryResult::error(evidence),
        _ => ProviderQueryResult::unavailable(evidence),
    }
}

fn evidence_for(
    outcome: ProviderQueryOutcome,
    request: &ProviderQueryRequest,
    snapshot: &ProviderAdapterSnapshot,
    records: Vec<&AdapterFactRecord>,
    limitations: Vec<String>,
) -> ProviderQueryEvidence {
    let facts: Vec<_> = records
        .iter()
        .map(|record| record.envelope.clone())
        .collect();
    let mut traces: Vec<_> = records
        .iter()
        .filter_map(|record| record.trace.clone())
        .map(|mut trace| {
            trace.surface = request.surface;
            trace
        })
        .collect();
    traces.sort_by(|left, right| {
        left.surface
            .cmp(&right.surface)
            .then_with(|| left.source.cmp(&right.source))
            .then_with(|| left.anchor_id.cmp(&right.anchor_id))
    });
    ProviderQueryEvidence::new(
        proof_for(outcome, snapshot.proof_ceiling),
        snapshot.authority_producers.clone(),
        summarize_provenance(&facts),
        summarize_confidence(&facts),
        summarize_freshness(&facts, snapshot.freshness),
        summarize_document_generation(&facts, &snapshot.document_generation),
        snapshot.workspace_generation.clone(),
        facts.first().map(|fact| fact.anchor),
        facts.iter().find_map(|fact| fact.boundary.clone()),
        summarize_reason(outcome, &facts),
        facts,
        traces,
        limitations,
    )
}

fn proof_for(outcome: ProviderQueryOutcome, ceiling: ProviderProofClass) -> ProviderProofClass {
    match outcome {
        ProviderQueryOutcome::Exact => ceiling,
        ProviderQueryOutcome::Degraded => ProviderProofClass::QualifiedRead,
        ProviderQueryOutcome::Fallback => ProviderProofClass::FallbackOnly,
        ProviderQueryOutcome::Refused
        | ProviderQueryOutcome::Stale
        | ProviderQueryOutcome::Dynamic
        | ProviderQueryOutcome::Ambiguous
        | ProviderQueryOutcome::Unavailable
        | ProviderQueryOutcome::Cancelled
        | ProviderQueryOutcome::DeadlineExceeded => ProviderProofClass::RefusalOnly,
        ProviderQueryOutcome::Error => ProviderProofClass::Unknown,
        _ => ProviderProofClass::Unknown,
    }
}

fn summarize_provenance(facts: &[SemanticFactEnvelope]) -> SemanticProvenance {
    let mut values = facts.iter().map(|fact| fact.provenance);
    let Some(first) = values.next() else {
        return SemanticProvenance::Unknown;
    };
    if values.all(|value| value == first) {
        first
    } else {
        SemanticProvenance::Unknown
    }
}

fn summarize_confidence(facts: &[SemanticFactEnvelope]) -> SemanticConfidence {
    let mut weakest = Confidence::High;
    for fact in facts {
        let SemanticConfidence::Known(confidence) = fact.confidence else {
            return SemanticConfidence::Unknown;
        };
        weakest = match (weakest, confidence) {
            (_, Confidence::Low) => Confidence::Low,
            (Confidence::High, Confidence::Medium) => Confidence::Medium,
            (current, _) => current,
        };
    }
    if facts.is_empty() {
        SemanticConfidence::Unknown
    } else {
        SemanticConfidence::Known(weakest)
    }
}

fn summarize_freshness(
    facts: &[SemanticFactEnvelope],
    fallback: SemanticFreshness,
) -> SemanticFreshness {
    if facts
        .iter()
        .any(|fact| fact.freshness == SemanticFreshness::Stale)
    {
        SemanticFreshness::Stale
    } else if facts
        .iter()
        .any(|fact| fact.freshness == SemanticFreshness::Unknown)
    {
        SemanticFreshness::Unknown
    } else if facts.is_empty() {
        fallback
    } else if facts
        .iter()
        .all(|fact| fact.freshness == SemanticFreshness::Fresh)
    {
        SemanticFreshness::Fresh
    } else {
        SemanticFreshness::NotApplicable
    }
}

fn summarize_document_generation(
    facts: &[SemanticFactEnvelope],
    fallback: &SourceGeneration,
) -> SourceGeneration {
    let mut generations = facts.iter().map(|fact| &fact.source_generation);
    let Some(first) = generations.next() else {
        return fallback.clone();
    };
    if generations.all(|generation| generation == first) {
        first.clone()
    } else {
        SourceGeneration::Unknown
    }
}

fn summarize_reason(
    outcome: ProviderQueryOutcome,
    facts: &[SemanticFactEnvelope],
) -> SemanticReasonCode {
    match outcome {
        ProviderQueryOutcome::Exact => SemanticReasonCode::ExactSource,
        ProviderQueryOutcome::Stale => SemanticReasonCode::StaleDependency,
        ProviderQueryOutcome::Dynamic => SemanticReasonCode::DynamicValue,
        ProviderQueryOutcome::Refused => facts
            .iter()
            .find(|fact| fact.status() == SemanticFactStatus::Refused)
            .map(|fact| fact.reason_code)
            .unwrap_or(SemanticReasonCode::UnsupportedEffect),
        ProviderQueryOutcome::Degraded | ProviderQueryOutcome::Fallback => facts
            .iter()
            .find(|fact| fact.reason_code != SemanticReasonCode::ExactSource)
            .map(|fact| fact.reason_code)
            .unwrap_or(SemanticReasonCode::CompatibilityBoundary),
        ProviderQueryOutcome::Ambiguous
        | ProviderQueryOutcome::Unavailable
        | ProviderQueryOutcome::Cancelled
        | ProviderQueryOutcome::DeadlineExceeded
        | ProviderQueryOutcome::Error => SemanticReasonCode::Unknown,
        _ => SemanticReasonCode::Unknown,
    }
}

fn extended_limitations(limitations: &[String], extra: &str) -> Vec<String> {
    let mut values = limitations.to_vec();
    values.push(extra.to_string());
    values
}
