//! Truthful adapters from current semantic fact producers into the provider port.
//!
//! These adapters are shadow infrastructure only. They do not alter live LSP
//! answers, create a second fact store, or infer proof from a producer name.
//! Every result is constructed through the request-bound contract in
//! [`super::semantic_port`], so the same canonical fact set supplies both values
//! and evidence.

use super::semantic_port::{
    ProviderCancellationState, ProviderEvidenceCompleteness, ProviderIdentity,
    ProviderProofClass, ProviderQueryContext, ProviderQueryContractError, ProviderQueryControl,
    ProviderQueryDeadline, ProviderQueryEvidenceInput, ProviderQueryFact, ProviderQueryFactRole,
    ProviderQueryKind, ProviderQueryMatchKey, ProviderQueryOutcome, ProviderQueryRequest,
    ProviderQueryResult, ProviderQuerySubject, ProviderQueryTerminalState,
    ProviderReadinessRequirement, ProviderReadinessState, ProviderSemanticPort,
};
use perl_semantic_facts::{
    AnchorFact, AnchorId, BoundaryDisposition, BoundaryKind, BoundaryLink, Confidence, EntityFact,
    EntityId, EntityKind, FactId, FileId, LifecyclePhase, OccurrenceFact, OccurrenceKind,
    Provenance, ProviderFactFreshness, ProviderFactSourceKind, ProviderFactTrace,
    ProviderFallbackState, SemanticConfidence, SemanticFactEnvelope, SemanticFactKind,
    SemanticFactStatus, SemanticFreshness, SemanticProducer, SemanticProvenance,
    SemanticReasonCode, SourceAnchor, SourceGeneration,
};
use perl_workspace::workspace::workspace_index::FileFactShard;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;

/// Completeness declared by a fact owner for one query family.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ProviderSnapshotCompleteness {
    /// The supported denominator is complete for this snapshot.
    Complete,
    /// A useful subset is present, but exact empty is not authorized.
    Partial,
    /// Completeness was not measured.
    Unknown,
}

impl From<ProviderSnapshotCompleteness> for ProviderEvidenceCompleteness {
    fn from(value: ProviderSnapshotCompleteness) -> Self {
        match value {
            ProviderSnapshotCompleteness::Complete => Self::Complete,
            ProviderSnapshotCompleteness::Partial => Self::Partial,
            ProviderSnapshotCompleteness::Unknown => Self::Unknown,
            _ => Self::Unknown,
        }
    }
}

/// Query family whose completeness is tracked independently.
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

/// Explicit snapshot identity and proof ceiling supplied by the fact owner.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderAdapterSnapshot {
    /// Source/document generation represented by the adapted facts.
    pub document_generation: SourceGeneration,
    /// Workspace/model generation represented by the adapted facts.
    pub workspace_generation: SourceGeneration,
    /// Freshness of the adapted fact set.
    pub freshness: SemanticFreshness,
    /// Lifecycle phase assigned to adapted facts.
    pub lifecycle: LifecyclePhase,
    /// How provider traces describe this path.
    pub fallback_state: ProviderFallbackState,
    /// Optional producer schema/model version.
    pub model_version: Option<u32>,
    completeness: BTreeMap<ProviderQueryCapability, ProviderSnapshotCompleteness>,
}

impl ProviderAdapterSnapshot {
    /// Construct explicit snapshot metadata.
    #[allow(clippy::too_many_arguments)] // mirrors the snapshot contract
    #[must_use]
    pub fn new(
        document_generation: SourceGeneration,
        workspace_generation: SourceGeneration,
        freshness: SemanticFreshness,
        lifecycle: LifecyclePhase,
        fallback_state: ProviderFallbackState,
        model_version: Option<u32>,
        completeness: impl IntoIterator<
            Item = (ProviderQueryCapability, ProviderSnapshotCompleteness),
        >,
    ) -> Self {
        Self {
            document_generation,
            workspace_generation,
            freshness,
            lifecycle,
            fallback_state,
            model_version,
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

    fn downgrade(&mut self, capability: ProviderQueryCapability) {
        if self.completeness(capability) == ProviderSnapshotCompleteness::Complete {
            self.completeness
                .insert(capability, ProviderSnapshotCompleteness::Partial);
        }
    }
}

/// Input for a fact that already uses the canonical semantic envelope.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalProviderFact {
    /// Canonical semantic fact.
    pub envelope: SemanticFactEnvelope,
    /// Additional source spellings that the producer actually supplies.
    pub match_keys: Vec<ProviderQueryMatchKey>,
}

impl CanonicalProviderFact {
    /// Construct a canonical fact without adding inferred symbol spellings.
    #[must_use]
    pub fn from_envelope(envelope: SemanticFactEnvelope) -> Self {
        Self {
            envelope,
            match_keys: Vec::new(),
        }
    }

    /// Construct a canonical fact with explicit producer-supplied match keys.
    #[must_use]
    pub fn new(
        envelope: SemanticFactEnvelope,
        match_keys: impl IntoIterator<Item = ProviderQueryMatchKey>,
    ) -> Self {
        Self {
            envelope,
            match_keys: match_keys.into_iter().collect(),
        }
    }
}

/// Adapter construction failure that prevents truthful attribution.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderAdapterError {
    /// A file fact shard cannot be relabeled as a compiler/framework producer.
    UnsupportedShardProducer(SemanticProducer),
    /// The trace source is inconsistent with the declared shard producer.
    UnsupportedTraceSource {
        /// Declared producer.
        producer: SemanticProducer,
        /// Requested trace source.
        source: ProviderFactSourceKind,
    },
    /// A canonical query fact could not be constructed.
    Contract(ProviderQueryContractError),
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
            Self::Contract(error) => error.fmt(formatter),
        }
    }
}

impl Error for ProviderAdapterError {}

impl From<ProviderQueryContractError> for ProviderAdapterError {
    fn from(value: ProviderQueryContractError) -> Self {
        Self::Contract(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AdapterRecord {
    envelope: SemanticFactEnvelope,
    match_keys: Vec<ProviderQueryMatchKey>,
    occurrence_kind: Option<OccurrenceKind>,
    trace_source: ProviderFactSourceKind,
    source_hash: Option<String>,
    model_version: Option<u32>,
}

impl AdapterRecord {
    fn canonicalize(&mut self) {
        self.match_keys.sort();
        self.match_keys.dedup();
    }

    fn matches_subject(&self, subject: &ProviderQuerySubject) -> bool {
        match subject {
            ProviderQuerySubject::Entity(entity_id) => {
                self.envelope.entity_id == Some(*entity_id)
                    || self
                        .match_keys
                        .contains(&ProviderQueryMatchKey::Entity(*entity_id))
            }
            ProviderQuerySubject::File(file_id) => self.envelope.anchor.file_id == *file_id,
            ProviderQuerySubject::Position {
                file_id,
                byte_offset,
            } => {
                self.envelope.anchor.file_id == *file_id
                    && range_contains(&self.envelope.anchor, *byte_offset)
            }
            ProviderQuerySubject::Package(package) => {
                self.envelope.package.as_deref() == Some(package.as_str())
                    || self
                        .match_keys
                        .contains(&ProviderQueryMatchKey::Package(package.clone()))
            }
            ProviderQuerySubject::Symbol(symbol) => self
                .match_keys
                .contains(&ProviderQueryMatchKey::Symbol(symbol.clone())),
            ProviderQuerySubject::Workspace => true,
            _ => false,
        }
    }

    fn query_fact(
        &self,
        role: ProviderQueryFactRole,
    ) -> Result<ProviderQueryFact, ProviderQueryContractError> {
        ProviderQueryFact::try_new(role, self.envelope.clone(), self.match_keys.clone())
    }

    fn trace(&self, surface: perl_semantic_facts::ProviderSurface) -> Option<ProviderFactTrace> {
        let SemanticProvenance::Known(provenance) = self.envelope.provenance else {
            return None;
        };
        let SemanticConfidence::Known(confidence) = self.envelope.confidence else {
            return None;
        };
        Some(ProviderFactTrace::new(
            surface,
            self.trace_source,
            provenance,
            confidence,
            provider_freshness(self.envelope.freshness),
            ProviderFallbackState::Primary,
            self.source_hash.clone(),
            self.envelope.anchor.anchor_id,
            self.model_version,
        ))
    }
}

/// Adapter over current parser/semantic/workspace [`FileFactShard`] values.
pub struct FileFactShardPort {
    records: Vec<AdapterRecord>,
    snapshot: ProviderAdapterSnapshot,
    producer: SemanticProducer,
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
            )?;
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
            producer,
            limitations,
        })
    }
}

impl ProviderSemanticPort for FileFactShardPort {
    fn query(
        &self,
        request: &ProviderQueryRequest,
        control: &dyn ProviderQueryControl,
    ) -> Result<ProviderQueryResult, ProviderQueryContractError> {
        query_records(
            request,
            control,
            &self.records,
            &self.snapshot,
            &[self.producer],
            &self.limitations,
        )
    }
}

/// Adapter over facts already emitted as canonical semantic envelopes.
pub struct CanonicalEnvelopePort {
    records: Vec<AdapterRecord>,
    snapshot: ProviderAdapterSnapshot,
    authority_producers: Vec<SemanticProducer>,
}

impl CanonicalEnvelopePort {
    /// Construct an adapter over already-canonical facts.
    pub fn new(
        facts: impl IntoIterator<Item = CanonicalProviderFact>,
        snapshot: ProviderAdapterSnapshot,
    ) -> Result<Self, ProviderAdapterError> {
        let mut authority_producers = Vec::new();
        let mut records = Vec::new();
        for fact in facts {
            authority_producers.push(fact.envelope.producer);
            let trace_source = source_for_producer(fact.envelope.producer);
            let mut record = AdapterRecord {
                envelope: fact.envelope,
                match_keys: fact.match_keys,
                occurrence_kind: None,
                trace_source,
                source_hash: None,
                model_version: snapshot.model_version,
            };
            record.canonicalize();
            // Build once at construction so malformed producer match keys fail early.
            let _ = record.query_fact(ProviderQueryFactRole::Supporting)?;
            records.push(record);
        }
        authority_producers.retain(|producer| *producer != SemanticProducer::Unknown);
        authority_producers.sort();
        authority_producers.dedup();
        records.sort_by_key(|record| record.envelope.fact_id);
        Ok(Self {
            records,
            snapshot,
            authority_producers,
        })
    }
}

impl ProviderSemanticPort for CanonicalEnvelopePort {
    fn query(
        &self,
        request: &ProviderQueryRequest,
        control: &dyn ProviderQueryControl,
    ) -> Result<ProviderQueryResult, ProviderQueryContractError> {
        query_records(
            request,
            control,
            &self.records,
            &self.snapshot,
            &self.authority_producers,
            &[],
        )
    }
}

#[allow(clippy::too_many_arguments)]
fn adapt_shard(
    shard: &FileFactShard,
    producer: SemanticProducer,
    trace_source: ProviderFactSourceKind,
    snapshot: &ProviderAdapterSnapshot,
    records: &mut Vec<AdapterRecord>,
    limitations: &mut Vec<String>,
    incomplete: &mut BTreeSet<ProviderQueryCapability>,
) -> Result<(), ProviderAdapterError> {
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
        let record = record_from_entity(
            entity,
            anchor,
            producer,
            trace_source,
            snapshot,
            shard,
        )?;
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
        let record = record_from_occurrence(
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
        )?;
        records.push(record);
    }
    Ok(())
}

fn record_from_entity(
    entity: &EntityFact,
    anchor: &AnchorFact,
    producer: SemanticProducer,
    trace_source: ProviderFactSourceKind,
    snapshot: &ProviderAdapterSnapshot,
    shard: &FileFactShard,
) -> Result<AdapterRecord, ProviderAdapterError> {
    let fact_id = stable_fact_id(b"entity", shard.file_id, entity.id.0);
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
    let mut match_keys = vec![
        ProviderQueryMatchKey::Entity(entity.id),
        ProviderQueryMatchKey::Symbol(entity.canonical_name.clone()),
    ];
    if let Some((package, bare)) = entity.canonical_name.rsplit_once("::") {
        if !package.is_empty() {
            match_keys.push(ProviderQueryMatchKey::Package(package.to_string()));
        }
        if !bare.is_empty() {
            match_keys.push(ProviderQueryMatchKey::Symbol(bare.to_string()));
        }
    }
    let mut record = AdapterRecord {
        envelope,
        match_keys,
        occurrence_kind: None,
        trace_source,
        source_hash: Some(format!("content:{:016x}", shard.content_hash)),
        model_version: Some(shard.producer_schema_version),
    };
    record.canonicalize();
    let _ = record.query_fact(ProviderQueryFactRole::Supporting)?;
    Ok(record)
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
) -> Result<AdapterRecord, ProviderAdapterError> {
    let fact_id = stable_fact_id(b"occurrence", shard.file_id, occurrence.id.0);
    let dynamic = occurrence.kind == OccurrenceKind::DynamicBoundary;
    let envelope = SemanticFactEnvelope::new(
        fact_id,
        occurrence.entity_id,
        if dynamic {
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
        if dynamic {
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
        if dynamic {
            SemanticReasonCode::DynamicValue
        } else {
            reason_for(occurrence.provenance)
        },
    );
    let mut match_keys = Vec::new();
    if let Some(entity_id) = occurrence.entity_id {
        match_keys.push(ProviderQueryMatchKey::Entity(entity_id));
    }
    if let Some(name) = entity_name {
        match_keys.push(ProviderQueryMatchKey::Symbol(name.clone()));
        if let Some((package, bare)) = name.rsplit_once("::") {
            if !package.is_empty() {
                match_keys.push(ProviderQueryMatchKey::Package(package.to_string()));
            }
            if !bare.is_empty() {
                match_keys.push(ProviderQueryMatchKey::Symbol(bare.to_string()));
            }
        }
    }
    let mut record = AdapterRecord {
        envelope,
        match_keys,
        occurrence_kind: Some(occurrence.kind),
        trace_source,
        source_hash: Some(format!("content:{:016x}", shard.content_hash)),
        model_version: Some(shard.producer_schema_version),
    };
    record.canonicalize();
    let _ = record.query_fact(ProviderQueryFactRole::Supporting)?;
    Ok(record)
}

fn query_records(
    request: &ProviderQueryRequest,
    control: &dyn ProviderQueryControl,
    records: &[AdapterRecord],
    snapshot: &ProviderAdapterSnapshot,
    authority_producers: &[SemanticProducer],
    base_limitations: &[String],
) -> Result<ProviderQueryResult, ProviderQueryContractError> {
    if !request.is_well_formed() {
        return Err(ProviderQueryContractError::MalformedRequest);
    }
    if request.context.cancellation == ProviderCancellationState::Cancelled
        || control.is_cancelled()
    {
        return terminal_result(
            request,
            ProviderQueryOutcome::Cancelled,
            snapshot,
            authority_producers,
            ProviderQueryTerminalState::Cancelled,
            base_limitations,
        );
    }
    if request.context.deadline == ProviderQueryDeadline::Expired || control.deadline_expired() {
        return terminal_result(
            request,
            ProviderQueryOutcome::DeadlineExceeded,
            snapshot,
            authority_producers,
            ProviderQueryTerminalState::DeadlineExceeded,
            base_limitations,
        );
    }
    if request.context.readiness_requirement == ProviderReadinessRequirement::EditAuthorizing {
        let mut limitations = base_limitations.to_vec();
        limitations.push("edit_authorization_requires_generation_guard".to_string());
        return ProviderQueryResult::try_new(
            request,
            ProviderQueryOutcome::Refused,
            Vec::new(),
            evidence_input(
                request,
                ProviderEvidenceCompleteness::Unknown,
                authority_producers,
                snapshot,
                &[],
                SemanticReasonCode::UnsupportedEffect,
                None,
                limitations,
                ProviderQueryTerminalState::Completed,
            ),
        );
    }

    let capability = ProviderQueryCapability::from_query(&request.kind);
    if capability == ProviderQueryCapability::Readiness {
        return readiness_result(
            request,
            control,
            snapshot,
            authority_producers,
            base_limitations,
        );
    }

    let values = select_value_records(request, records);
    let blockers = if capability == ProviderQueryCapability::Boundaries {
        Vec::new()
    } else {
        select_boundary_records(&request.subject, records)
    };
    let evidence_records = merge_records(&values, &blockers);

    if control.is_cancelled() {
        return terminal_result(
            request,
            ProviderQueryOutcome::Cancelled,
            snapshot,
            authority_producers,
            ProviderQueryTerminalState::Cancelled,
            base_limitations,
        );
    }
    if control.deadline_expired() {
        return terminal_result(
            request,
            ProviderQueryOutcome::DeadlineExceeded,
            snapshot,
            authority_producers,
            ProviderQueryTerminalState::DeadlineExceeded,
            base_limitations,
        );
    }

    if values.is_empty() {
        if !blockers.is_empty() {
            return no_value_result(
                request,
                ProviderQueryOutcome::Dynamic,
                snapshot,
                authority_producers,
                &evidence_records,
                SemanticReasonCode::DynamicValue,
                base_limitations,
            );
        }
        if can_claim_exact_empty(request, snapshot, authority_producers, capability, base_limitations)
        {
            return ProviderQueryResult::try_new(
                request,
                ProviderQueryOutcome::Exact,
                Vec::new(),
                evidence_input(
                    request,
                    ProviderEvidenceCompleteness::Complete,
                    authority_producers,
                    snapshot,
                    &[],
                    SemanticReasonCode::ExactSource,
                    None,
                    Vec::new(),
                    ProviderQueryTerminalState::Completed,
                ),
            );
        }
        return no_value_result(
            request,
            ProviderQueryOutcome::Unavailable,
            snapshot,
            authority_producers,
            &evidence_records,
            SemanticReasonCode::Unknown,
            base_limitations,
        );
    }

    if evidence_records
        .iter()
        .any(|record| record.envelope.status() == SemanticFactStatus::Stale)
        || snapshot.freshness == SemanticFreshness::Stale
    {
        return no_value_result(
            request,
            ProviderQueryOutcome::Stale,
            snapshot,
            authority_producers,
            &evidence_records,
            SemanticReasonCode::StaleDependency,
            base_limitations,
        );
    }
    if evidence_records
        .iter()
        .any(|record| record.envelope.status() == SemanticFactStatus::Refused)
    {
        return no_value_result(
            request,
            ProviderQueryOutcome::Refused,
            snapshot,
            authority_producers,
            &evidence_records,
            SemanticReasonCode::UnsupportedEffect,
            base_limitations,
        );
    }

    let distinct_entities: BTreeSet<_> = values
        .iter()
        .filter_map(|record| record.envelope.entity_id)
        .collect();
    if matches!(request.kind, ProviderQueryKind::Declaration)
        && distinct_entities.len() > 1
        && !matches!(request.subject, ProviderQuerySubject::Entity(_))
    {
        let mut limitations = base_limitations.to_vec();
        limitations.push("multiple_matching_entities".to_string());
        return no_value_result(
            request,
            ProviderQueryOutcome::Ambiguous,
            snapshot,
            authority_producers,
            &evidence_records,
            SemanticReasonCode::Unknown,
            &limitations,
        );
    }

    let mut limitations = base_limitations.to_vec();
    let outcome = if snapshot.fallback_state == ProviderFallbackState::Fallback {
        limitations.push("explicit_fallback_path".to_string());
        ProviderQueryOutcome::Fallback
    } else if blockers.is_empty()
        && can_claim_exact_values(
            request,
            snapshot,
            authority_producers,
            capability,
            &values,
            &limitations,
        )
    {
        ProviderQueryOutcome::Exact
    } else {
        ProviderQueryOutcome::Degraded
    };

    value_result(
        request,
        outcome,
        snapshot,
        authority_producers,
        &values,
        &evidence_records,
        limitations,
    )
}

fn readiness_result(
    request: &ProviderQueryRequest,
    control: &dyn ProviderQueryControl,
    snapshot: &ProviderAdapterSnapshot,
    authority_producers: &[SemanticProducer],
    limitations: &[String],
) -> Result<ProviderQueryResult, ProviderQueryContractError> {
    if control.is_cancelled() {
        return terminal_result(
            request,
            ProviderQueryOutcome::Cancelled,
            snapshot,
            authority_producers,
            ProviderQueryTerminalState::Cancelled,
            limitations,
        );
    }
    if control.deadline_expired() {
        return terminal_result(
            request,
            ProviderQueryOutcome::DeadlineExceeded,
            snapshot,
            authority_producers,
            ProviderQueryTerminalState::DeadlineExceeded,
            limitations,
        );
    }
    match request.context.readiness_state {
        ProviderReadinessState::Ready
            if can_claim_exact_empty(
                request,
                snapshot,
                authority_producers,
                ProviderQueryCapability::Readiness,
                limitations,
            ) => ProviderQueryResult::try_new(
                request,
                ProviderQueryOutcome::Exact,
                Vec::new(),
                evidence_input(
                    request,
                    ProviderEvidenceCompleteness::Complete,
                    authority_producers,
                    snapshot,
                    &[],
                    SemanticReasonCode::ExactSource,
                    None,
                    Vec::new(),
                    ProviderQueryTerminalState::Completed,
                ),
            ),
        ProviderReadinessState::Ready | ProviderReadinessState::ReadyLimited => {
            ProviderQueryResult::try_new(
                request,
                ProviderQueryOutcome::Degraded,
                Vec::new(),
                evidence_input(
                    request,
                    snapshot
                        .completeness(ProviderQueryCapability::Readiness)
                        .into(),
                    authority_producers,
                    snapshot,
                    &[],
                    SemanticReasonCode::CompatibilityBoundary,
                    None,
                    limitations.to_vec(),
                    ProviderQueryTerminalState::Completed,
                ),
            )
        }
        ProviderReadinessState::Stale => no_value_result(
            request,
            ProviderQueryOutcome::Stale,
            snapshot,
            authority_producers,
            &[],
            SemanticReasonCode::StaleDependency,
            limitations,
        ),
        ProviderReadinessState::Failed => terminal_result(
            request,
            ProviderQueryOutcome::Error,
            snapshot,
            authority_producers,
            ProviderQueryTerminalState::Failed,
            limitations,
        ),
        ProviderReadinessState::Building | ProviderReadinessState::Unavailable => no_value_result(
            request,
            ProviderQueryOutcome::Unavailable,
            snapshot,
            authority_producers,
            &[],
            SemanticReasonCode::Unknown,
            limitations,
        ),
        _ => no_value_result(
            request,
            ProviderQueryOutcome::Unavailable,
            snapshot,
            authority_producers,
            &[],
            SemanticReasonCode::Unknown,
            limitations,
        ),
    }
}

fn select_value_records<'a>(
    request: &ProviderQueryRequest,
    records: &'a [AdapterRecord],
) -> Vec<&'a AdapterRecord> {
    let mut selected: Vec<_> = match &request.kind {
        ProviderQueryKind::References {
            include_declaration,
        } => select_reference_records(request, records, *include_declaration),
        ProviderQueryKind::ScopeBindings => select_scope_records(request, records),
        _ => records
            .iter()
            .filter(|record| value_kind_matches(&request.kind, record))
            .filter(|record| record.matches_subject(&request.subject))
            .collect(),
    };
    selected.sort_by_key(|record| record.envelope.fact_id);
    selected.dedup_by_key(|record| record.envelope.fact_id);
    selected
}

fn select_boundary_records<'a>(
    subject: &ProviderQuerySubject,
    records: &'a [AdapterRecord],
) -> Vec<&'a AdapterRecord> {
    let mut selected: Vec<_> = records
        .iter()
        .filter(|record| record.envelope.kind == SemanticFactKind::Boundary)
        .filter(|record| record.matches_subject(subject))
        .collect();
    selected.sort_by_key(|record| record.envelope.fact_id);
    selected.dedup_by_key(|record| record.envelope.fact_id);
    selected
}

fn select_reference_records<'a>(
    request: &ProviderQueryRequest,
    records: &'a [AdapterRecord],
    include_declaration: bool,
) -> Vec<&'a AdapterRecord> {
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
    records: &'a [AdapterRecord],
) -> Vec<&'a AdapterRecord> {
    let scope_ids: BTreeSet<_> = records
        .iter()
        .filter(|record| record.matches_subject(&request.subject))
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
    records: &[AdapterRecord],
) -> BTreeSet<EntityId> {
    match subject {
        ProviderQuerySubject::Entity(entity_id) => BTreeSet::from([*entity_id]),
        _ => records
            .iter()
            .filter(|record| {
                matches!(
                    record.envelope.kind,
                    SemanticFactKind::Declaration | SemanticFactKind::Module
                ) && record.matches_subject(subject)
            })
            .filter_map(|record| record.envelope.entity_id)
            .collect(),
    }
}

fn value_kind_matches(kind: &ProviderQueryKind, record: &AdapterRecord) -> bool {
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

fn merge_records<'a>(
    first: &[&'a AdapterRecord],
    second: &[&'a AdapterRecord],
) -> Vec<&'a AdapterRecord> {
    let mut records = Vec::with_capacity(first.len() + second.len());
    records.extend_from_slice(first);
    records.extend_from_slice(second);
    records.sort_by_key(|record| record.envelope.fact_id);
    records.dedup_by_key(|record| record.envelope.fact_id);
    records
}

fn value_result(
    request: &ProviderQueryRequest,
    outcome: ProviderQueryOutcome,
    snapshot: &ProviderAdapterSnapshot,
    authority_producers: &[SemanticProducer],
    values: &[&AdapterRecord],
    evidence_records: &[&AdapterRecord],
    limitations: Vec<String>,
) -> Result<ProviderQueryResult, ProviderQueryContractError> {
    let value_ids: BTreeSet<_> = values
        .iter()
        .map(|record| record.envelope.fact_id)
        .collect();
    let mut facts = Vec::new();
    for record in evidence_records {
        let role = if value_ids.contains(&record.envelope.fact_id) {
            ProviderQueryFactRole::Value
        } else {
            ProviderQueryFactRole::Supporting
        };
        facts.push(record.query_fact(role)?);
    }
    ProviderQueryResult::try_new(
        request,
        outcome,
        facts,
        evidence_input(
            request,
            snapshot
                .completeness(ProviderQueryCapability::from_query(&request.kind))
                .into(),
            authority_producers,
            snapshot,
            evidence_records,
            reason_for_outcome(outcome, evidence_records),
            evidence_records
                .iter()
                .find_map(|record| record.envelope.boundary.clone()),
            limitations,
            ProviderQueryTerminalState::Completed,
        ),
    )
}

fn no_value_result(
    request: &ProviderQueryRequest,
    outcome: ProviderQueryOutcome,
    snapshot: &ProviderAdapterSnapshot,
    authority_producers: &[SemanticProducer],
    records: &[&AdapterRecord],
    reason: SemanticReasonCode,
    limitations: &[String],
) -> Result<ProviderQueryResult, ProviderQueryContractError> {
    let mut facts = Vec::new();
    for record in records {
        facts.push(record.query_fact(ProviderQueryFactRole::Supporting)?);
    }
    ProviderQueryResult::try_new(
        request,
        outcome,
        facts,
        evidence_input(
            request,
            snapshot
                .completeness(ProviderQueryCapability::from_query(&request.kind))
                .into(),
            authority_producers,
            snapshot,
            records,
            reason,
            records
                .iter()
                .find_map(|record| record.envelope.boundary.clone()),
            limitations.to_vec(),
            ProviderQueryTerminalState::Completed,
        ),
    )
}

fn terminal_result(
    request: &ProviderQueryRequest,
    outcome: ProviderQueryOutcome,
    snapshot: &ProviderAdapterSnapshot,
    authority_producers: &[SemanticProducer],
    terminal: ProviderQueryTerminalState,
    limitations: &[String],
) -> Result<ProviderQueryResult, ProviderQueryContractError> {
    ProviderQueryResult::try_new(
        request,
        outcome,
        Vec::new(),
        evidence_input(
            request,
            ProviderEvidenceCompleteness::Unknown,
            authority_producers,
            snapshot,
            &[],
            SemanticReasonCode::Unknown,
            None,
            limitations.to_vec(),
            terminal,
        ),
    )
}

#[allow(clippy::too_many_arguments)]
fn evidence_input(
    request: &ProviderQueryRequest,
    completeness: ProviderEvidenceCompleteness,
    authority_producers: &[SemanticProducer],
    snapshot: &ProviderAdapterSnapshot,
    records: &[&AdapterRecord],
    reason: SemanticReasonCode,
    boundary: Option<BoundaryLink>,
    limitations: Vec<String>,
    terminal: ProviderQueryTerminalState,
) -> ProviderQueryEvidenceInput {
    let envelopes: Vec<_> = records.iter().map(|record| &record.envelope).collect();
    let traces = records
        .iter()
        .filter_map(|record| record.trace(request.surface))
        .collect();
    ProviderQueryEvidenceInput::new(
        completeness,
        authority_producers.to_vec(),
        summarize_provenance(&envelopes),
        summarize_confidence(&envelopes),
        summarize_freshness(&envelopes, snapshot.freshness),
        summarize_generation(&envelopes, snapshot.document_generation.clone()),
        snapshot.workspace_generation.clone(),
        envelopes.first().map(|fact| fact.anchor),
        boundary,
        reason,
        traces,
        limitations,
        terminal,
    )
}

fn can_claim_exact_empty(
    request: &ProviderQueryRequest,
    snapshot: &ProviderAdapterSnapshot,
    authority_producers: &[SemanticProducer],
    capability: ProviderQueryCapability,
    limitations: &[String],
) -> bool {
    snapshot.completeness(capability) == ProviderSnapshotCompleteness::Complete
        && snapshot.freshness == SemanticFreshness::Fresh
        && generation_is_known(&snapshot.document_generation)
        && generation_is_known(&snapshot.workspace_generation)
        && snapshot.document_generation == request.context.document_generation
        && snapshot.workspace_generation == request.context.workspace_generation
        && request_is_exact_ready(&request.context)
        && !authority_producers.is_empty()
        && limitations.is_empty()
}

fn can_claim_exact_values(
    request: &ProviderQueryRequest,
    snapshot: &ProviderAdapterSnapshot,
    authority_producers: &[SemanticProducer],
    capability: ProviderQueryCapability,
    values: &[&AdapterRecord],
    limitations: &[String],
) -> bool {
    can_claim_exact_empty(
        request,
        snapshot,
        authority_producers,
        capability,
        limitations,
    ) && values.iter().all(|record| {
        record.envelope.status() == SemanticFactStatus::Exact
            && record.envelope.freshness == SemanticFreshness::Fresh
            && record.envelope.source_generation == request.context.document_generation
            && record.envelope.boundary.is_none()
    })
}

fn request_is_exact_ready(context: &ProviderQueryContext) -> bool {
    identity_is_known(&context.project_identity)
        && identity_is_known(&context.root_identity)
        && generation_is_known(&context.document_generation)
        && generation_is_known(&context.workspace_generation)
        && context.readiness_state == ProviderReadinessState::Ready
        && context.readiness_requirement != ProviderReadinessRequirement::EditAuthorizing
        && context.cancellation == ProviderCancellationState::Active
        && context.deadline != ProviderQueryDeadline::Expired
}

fn identity_is_known(identity: &ProviderIdentity) -> bool {
    matches!(identity, ProviderIdentity::Known(value) if !value.trim().is_empty())
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
        EntityKind::Module => SemanticFactKind::Module,
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
        _ => SemanticReasonCode::Unknown,
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

fn stable_fact_id(domain: &[u8], file_id: FileId, raw: u64) -> FactId {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in domain
        .iter()
        .copied()
        .chain(file_id.0.to_le_bytes())
        .chain(raw.to_le_bytes())
    {
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

fn source_for_producer(producer: SemanticProducer) -> ProviderFactSourceKind {
    match producer {
        SemanticProducer::Parser => ProviderFactSourceKind::ParserSyntax,
        SemanticProducer::Hir | SemanticProducer::PirA => ProviderFactSourceKind::CompilerFact,
        SemanticProducer::SemanticAnalyzer => ProviderFactSourceKind::SemanticFact,
        SemanticProducer::WorkspaceIndex => ProviderFactSourceKind::LegacyWorkspace,
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

fn range_contains(anchor: &SourceAnchor, byte_offset: u32) -> bool {
    if anchor.start_byte == anchor.end_byte {
        byte_offset == anchor.start_byte
    } else {
        anchor.start_byte <= byte_offset && byte_offset < anchor.end_byte
    }
}

fn generation_is_known(generation: &SourceGeneration) -> bool {
    matches!(generation, SourceGeneration::Known(value) if !value.trim().is_empty())
}

fn summarize_provenance(facts: &[&SemanticFactEnvelope]) -> SemanticProvenance {
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

fn summarize_confidence(facts: &[&SemanticFactEnvelope]) -> SemanticConfidence {
    let mut values = facts.iter().map(|fact| fact.confidence);
    let Some(first) = values.next() else {
        return SemanticConfidence::Unknown;
    };
    if values.all(|value| value == first) {
        first
    } else {
        SemanticConfidence::Unknown
    }
}

fn summarize_freshness(
    facts: &[&SemanticFactEnvelope],
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

fn summarize_generation(
    facts: &[&SemanticFactEnvelope],
    fallback: SourceGeneration,
) -> SourceGeneration {
    let mut values = facts.iter().map(|fact| &fact.source_generation);
    let Some(first) = values.next() else {
        return fallback;
    };
    if values.all(|value| value == first) {
        first.clone()
    } else {
        SourceGeneration::Unknown
    }
}

fn reason_for_outcome(
    outcome: ProviderQueryOutcome,
    records: &[&AdapterRecord],
) -> SemanticReasonCode {
    match outcome {
        ProviderQueryOutcome::Exact => SemanticReasonCode::ExactSource,
        ProviderQueryOutcome::Stale => SemanticReasonCode::StaleDependency,
        ProviderQueryOutcome::Dynamic => SemanticReasonCode::DynamicValue,
        ProviderQueryOutcome::Refused => SemanticReasonCode::UnsupportedEffect,
        ProviderQueryOutcome::Degraded | ProviderQueryOutcome::Fallback => records
            .iter()
            .find(|record| record.envelope.reason_code != SemanticReasonCode::ExactSource)
            .map(|record| record.envelope.reason_code)
            .unwrap_or(SemanticReasonCode::CompatibilityBoundary),
        _ => SemanticReasonCode::Unknown,
    }
}
