//! Truthful adapters from current semantic fact producers into the provider port.
//!
//! The adapters in this module do not change live provider behavior. They turn
//! existing [`perl_workspace::workspace::workspace_index::FileFactShard`] data
//! and already-canonical [`SemanticFactEnvelope`] values into unchecked drafts
//! for the query contract from [`super::semantic_port`]; only
//! `execute_provider_query` turns a draft into a checked result. Every adapter
//! requires explicit generation, freshness, lifecycle, and completeness inputs;
//! no producer name upgrades proof and no missing source anchor is fabricated.
//!
//! Exactness boundaries enforced here, before the checked boundary re-verifies
//! them:
//!
//! - a request is answered exactly only when the snapshot generations equal the
//!   request generations and the request context is exact-ready;
//! - exact-empty additionally requires a request-bound completeness grant issued
//!   from the adapter's concrete denominator (validated producer, covered units
//!   that are each exact-grade and current for the request, snapshot identity),
//!   never from caller-predeclared labels;
//! - conflicting duplicate identities fail closed: canonical inputs are rejected
//!   at construction, shard identities are tombstoned so no later row can
//!   resurrect a contested binding (identical duplicate shard rows carry no new
//!   information and collapse to the first row), and shard fact ids are bound
//!   to the owning file so two shards cannot collide;
//! - position queries resolve the cursor record to its entity before selecting
//!   declarations or references, so a cursor on a reference cannot false-empty;
//! - no adapter outcome authorizes edits: edit-authorizing requests stay at or
//!   below a qualified read until #6819 supplies a guarded edit plan.

use super::semantic_port::{
    ProviderCancellationState, ProviderCompletenessGrant, ProviderFactGenerationScope,
    ProviderQueryCapability, ProviderQueryContractError, ProviderQueryControl,
    ProviderQueryDeadline, ProviderQueryEvidenceInput, ProviderQueryFact, ProviderQueryFactRole,
    ProviderQueryKind, ProviderQueryOutcome, ProviderQueryRequest, ProviderQueryResultDraft,
    ProviderQuerySubject, ProviderQueryTerminalState, ProviderReadinessRequirement,
    ProviderReadinessState, ProviderResultPath, ProviderSemanticPort, semantic_provenance_is_exact,
    validate_envelope_structure,
};
use perl_semantic_facts::{
    AnchorFact, AnchorId, BoundaryDisposition, BoundaryKind, BoundaryLink, Confidence, EntityFact,
    EntityId, FactId, FileId, LifecyclePhase, OccurrenceFact, OccurrenceId, OccurrenceKind,
    Provenance, ProviderFactFreshness, ProviderFactSourceKind, ProviderFactTrace,
    ProviderFallbackState, SemanticConfidence, SemanticFactEnvelope, SemanticFactKind,
    SemanticFactStatus, SemanticFreshness, SemanticProducer, SemanticProvenance,
    SemanticReasonCode, SourceAnchor, SourceGeneration,
};
use perl_workspace::workspace::workspace_index::FileFactShard;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;

/// Completeness of an adapter snapshot for one query capability.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum ProviderSnapshotCompleteness {
    /// The producer asserts that the capability's supported denominator is complete.
    Complete,
    /// The producer knows the snapshot is partial.
    Partial,
    /// Completeness was not measured.
    Unknown,
}

/// Explicit snapshot metadata supplied by the fact owner.
///
/// The snapshot carries no authority or proof-ceiling labels: exact-empty
/// authority is derived per query from the adapter's validated producer and
/// concrete denominator, and no adapter can authorize edits. The type is
/// serialize-only so no deserialized value can re-enter the trust boundary.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProviderAdapterSnapshot {
    /// Source/document generation represented by the adapted facts.
    pub document_generation: SourceGeneration,
    /// Workspace/model generation represented by the adapted facts.
    pub workspace_generation: SourceGeneration,
    /// Freshness of the adapted fact set.
    pub freshness: SemanticFreshness,
    /// Lifecycle phase for the adapted facts.
    pub lifecycle: LifecyclePhase,
    /// How provider traces should describe the adapter path.
    pub fallback_state: ProviderFallbackState,
    /// Optional model/schema version used by the producer.
    pub model_version: Option<u32>,
    completeness: BTreeMap<ProviderQueryCapability, ProviderSnapshotCompleteness>,
}

impl ProviderAdapterSnapshot {
    /// Construct explicit snapshot metadata and canonicalize completeness rows.
    #[must_use]
    pub fn new(
        document_generation: SourceGeneration,
        workspace_generation: SourceGeneration,
        freshness: SemanticFreshness,
        lifecycle: LifecyclePhase,
        fallback_state: ProviderFallbackState,
        model_version: Option<u32>,
        completeness: impl IntoIterator<Item = (ProviderQueryCapability, ProviderSnapshotCompleteness)>,
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
        self.completeness.get(&capability).copied().unwrap_or(ProviderSnapshotCompleteness::Unknown)
    }

    fn downgrade(&mut self, capability: ProviderQueryCapability) {
        if self.completeness(capability) == ProviderSnapshotCompleteness::Complete {
            self.completeness.insert(capability, ProviderSnapshotCompleteness::Partial);
        }
    }

    /// Whether the snapshot itself is fresh, primary, known-lifecycle, and
    /// generation-identified enough to support an exact-empty grant.
    fn can_claim_exact(&self, capability: ProviderQueryCapability) -> bool {
        self.completeness(capability) == ProviderSnapshotCompleteness::Complete
            && generation_is_known(&self.document_generation)
            && generation_is_known(&self.workspace_generation)
            && self.freshness == SemanticFreshness::Fresh
            && self.lifecycle != LifecyclePhase::Unknown
            && self.fallback_state == ProviderFallbackState::Primary
    }

    /// Whether the snapshot carries exact-grade fact metadata (fresh, primary,
    /// known lifecycle, known generations) for non-empty exact answers.
    ///
    /// A non-primary (shadow/observational) snapshot can corroborate but never
    /// originates an exact answer, same as for the exact-empty grant.
    fn facts_can_be_exact(&self) -> bool {
        self.freshness == SemanticFreshness::Fresh
            && self.lifecycle != LifecyclePhase::Unknown
            && generation_is_known(&self.document_generation)
            && generation_is_known(&self.workspace_generation)
            && self.fallback_state == ProviderFallbackState::Primary
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
    /// A canonical envelope is structurally malformed.
    MalformedEnvelope(FactId),
    /// Two inputs share one canonical fact identity with different content.
    ConflictingFactId(FactId),
}

impl fmt::Display for ProviderAdapterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedShardProducer(producer) => {
                write!(formatter, "file fact shard cannot be attributed to {producer:?}")
            }
            Self::UnsupportedTraceSource { producer, source } => {
                write!(formatter, "trace source {source:?} is invalid for producer {producer:?}")
            }
            Self::MalformedEnvelope(fact_id) => {
                write!(formatter, "canonical envelope {} is structurally malformed", fact_id.0)
            }
            Self::ConflictingFactId(fact_id) => {
                write!(formatter, "conflicting facts share identity {}", fact_id.0)
            }
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

/// How snapshot generations relate to the request generations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GenerationBinding {
    /// Snapshot and request generations are known and equal.
    Current,
    /// Both sides are known and at least one side differs.
    Mismatched,
    /// At least one side cannot identify its generation.
    Unknown,
}

fn generation_binding(
    request: &ProviderQueryRequest,
    snapshot: &ProviderAdapterSnapshot,
) -> GenerationBinding {
    let known = generation_is_known(&snapshot.document_generation)
        && generation_is_known(&snapshot.workspace_generation)
        && generation_is_known(&request.context.document_generation)
        && generation_is_known(&request.context.workspace_generation);
    if !known {
        return GenerationBinding::Unknown;
    }
    if snapshot.document_generation == request.context.document_generation
        && snapshot.workspace_generation == request.context.workspace_generation
    {
        GenerationBinding::Current
    } else {
        GenerationBinding::Mismatched
    }
}

/// Adapter over current parser/semantic/workspace [`FileFactShard`] values.
///
/// The caller supplies the actual producer. Only `Parser`, `SemanticAnalyzer`,
/// and `WorkspaceIndex` are accepted: a workspace shard is not evidence that a
/// compiler or framework producer contributed. The validated producer is the
/// sole completeness authority this adapter can derive.
pub struct FileFactShardPort {
    records: Vec<AdapterFactRecord>,
    snapshot: ProviderAdapterSnapshot,
    limitations: Vec<String>,
    producer: SemanticProducer,
    trace_source: ProviderFactSourceKind,
    denominator_scope: String,
    snapshot_identity: String,
}

impl FileFactShardPort {
    /// Adapt file fact shards with explicit producer and trace identity.
    pub fn new(
        shards: &[FileFactShard],
        producer: SemanticProducer,
        trace_source: ProviderFactSourceKind,
        mut snapshot: ProviderAdapterSnapshot,
    ) -> Result<Self, ProviderAdapterError> {
        if !matches!(
            producer,
            SemanticProducer::Parser
                | SemanticProducer::SemanticAnalyzer
                | SemanticProducer::WorkspaceIndex
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
        let mut file_ids = BTreeSet::new();
        let mut content_hashes = BTreeSet::new();
        for shard in shards {
            file_ids.insert(shard.file_id);
            content_hashes.insert(shard.content_hash);
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
        let mut kept = Vec::with_capacity(records.len());
        for mut record in records {
            record.names.retain(|name| !name.trim().is_empty());
            record.names.sort();
            record.names.dedup();
            if validate_envelope_structure(&record.envelope).is_ok() {
                kept.push(record);
            } else {
                limitations.push(format!("malformed_adapted_fact:{}", record.envelope.fact_id.0));
                downgrade_for_kind(&mut snapshot, record.envelope.kind);
            }
        }
        kept.sort_by_key(|record| record.envelope.fact_id);
        // Identical duplicate shard rows collapse to the one canonical record,
        // matching the documented decision and CanonicalEnvelopePort. The key
        // is the whole record: occurrence_kind lives outside the envelope and
        // drives reference filtering, so envelope equality alone is not
        // identity.
        kept.dedup_by(|left, right| {
            left.envelope == right.envelope
                && left.names == right.names
                && left.occurrence_kind == right.occurrence_kind
        });
        // After the full-record collapse, any surviving same-identity pair is
        // contradictory producer data and fails closed. The per-row
        // tombstones in adapt_shard degrade the common cases with named
        // limitations before this backstop is needed.
        for window in kept.windows(2) {
            let [left, right] = window else { continue };
            if left.envelope.fact_id == right.envelope.fact_id {
                return Err(ProviderAdapterError::ConflictingFactId(right.envelope.fact_id));
            }
        }
        limitations.sort();
        limitations.dedup();
        let denominator_scope = file_scope(file_ids.iter().copied());
        let snapshot_identity = format!(
            "{}:{}:content:{}",
            generation_label(&snapshot.document_generation),
            generation_label(&snapshot.workspace_generation),
            content_hashes.iter().map(|hash| format!("{hash:016x}")).collect::<Vec<_>>().join(",")
        );
        Ok(Self {
            records: kept,
            snapshot,
            limitations,
            producer,
            trace_source,
            denominator_scope,
            snapshot_identity,
        })
    }
}

impl ProviderSemanticPort for FileFactShardPort {
    fn query(
        &self,
        request: &ProviderQueryRequest,
        control: &dyn ProviderQueryControl,
    ) -> Result<ProviderQueryResultDraft, ProviderQueryContractError> {
        query_records(
            request,
            control,
            &self.records,
            &self.snapshot,
            &self.limitations,
            self.producer,
            self.trace_source,
            &self.denominator_scope,
            &self.snapshot_identity,
        )
    }
}

/// Adapter over facts that already use the canonical semantic envelope.
///
/// This is the only adapter in this slice that may carry `Hir`, `PirA`, or
/// `FrameworkAdapter` producer identities, and it preserves each envelope's
/// producer verbatim. It cannot manufacture compiler attribution when no
/// compiler envelope is present: completeness authority is derived from the
/// actual envelopes, so a mixed-producer or empty set can never issue an
/// exact-empty grant.
pub struct CanonicalEnvelopePort {
    records: Vec<AdapterFactRecord>,
    snapshot: ProviderAdapterSnapshot,
    limitations: Vec<String>,
    producer: Option<SemanticProducer>,
    denominator_scope: String,
    snapshot_identity: String,
}

impl CanonicalEnvelopePort {
    /// Construct an adapter over already-canonical facts.
    ///
    /// Envelopes are validated at construction. Duplicates with identical
    /// content collapse to one canonical fact; conflicting duplicates are
    /// rejected regardless of input order.
    pub fn new(
        envelopes: &[SemanticFactEnvelope],
        snapshot: ProviderAdapterSnapshot,
    ) -> Result<Self, ProviderAdapterError> {
        for envelope in envelopes {
            validate_envelope_structure(envelope)
                .map_err(|_| ProviderAdapterError::MalformedEnvelope(envelope.fact_id))?;
        }
        let mut sorted: Vec<&SemanticFactEnvelope> = envelopes.iter().collect();
        sorted.sort_by_key(|envelope| envelope.fact_id);
        reject_conflicting_fact_ids(sorted.iter().copied())?;

        let mut producers: BTreeSet<SemanticProducer> =
            sorted.iter().map(|envelope| envelope.producer).collect();
        producers.remove(&SemanticProducer::Unknown);
        let producer = if producers.len() == 1 { producers.iter().next().copied() } else { None };

        let mut seen = BTreeSet::new();
        let mut records = Vec::with_capacity(sorted.len());
        for envelope in sorted {
            // Identical duplicates collapse to the one canonical fact; the
            // conflicting case was rejected above.
            if !seen.insert(envelope.fact_id) {
                continue;
            }
            let trace = trace_from_envelope(
                envelope,
                ProviderFactSourceKind::SemanticFact,
                snapshot.fallback_state,
                snapshot.model_version,
            );
            let mut names: Vec<String> = envelope.package.iter().cloned().collect();
            names.retain(|name| !name.trim().is_empty());
            records.push(AdapterFactRecord {
                envelope: envelope.clone(),
                names,
                occurrence_kind: None,
                trace,
            });
        }
        let denominator_scope =
            file_scope(records.iter().map(|record| record.envelope.anchor.file_id));
        let snapshot_identity = format!(
            "{}:{}:envelopes-{}",
            generation_label(&snapshot.document_generation),
            generation_label(&snapshot.workspace_generation),
            records.len()
        );
        Ok(Self {
            records,
            snapshot,
            limitations: Vec::new(),
            producer,
            denominator_scope,
            snapshot_identity,
        })
    }
}

impl ProviderSemanticPort for CanonicalEnvelopePort {
    fn query(
        &self,
        request: &ProviderQueryRequest,
        control: &dyn ProviderQueryControl,
    ) -> Result<ProviderQueryResultDraft, ProviderQueryContractError> {
        query_records(
            request,
            control,
            &self.records,
            &self.snapshot,
            &self.limitations,
            self.producer.unwrap_or(SemanticProducer::Unknown),
            ProviderFactSourceKind::SemanticFact,
            &self.denominator_scope,
            &self.snapshot_identity,
        )
    }
}

fn file_scope(file_ids: impl IntoIterator<Item = FileId>) -> String {
    let mut ids: BTreeSet<u64> = file_ids.into_iter().map(|file_id| file_id.0).collect();
    let joined = ids.iter().map(u64::to_string).collect::<Vec<_>>().join(",");
    ids.clear();
    format!("files:{joined}")
}

fn reject_conflicting_fact_ids<'a>(
    envelopes: impl IntoIterator<Item = &'a SemanticFactEnvelope>,
) -> Result<(), ProviderAdapterError> {
    let mut seen: BTreeMap<FactId, &SemanticFactEnvelope> = BTreeMap::new();
    for envelope in envelopes {
        match seen.get(&envelope.fact_id) {
            Some(existing) if *existing != envelope => {
                return Err(ProviderAdapterError::ConflictingFactId(envelope.fact_id));
            }
            Some(_) => {}
            None => {
                seen.insert(envelope.fact_id, envelope);
            }
        }
    }
    Ok(())
}

fn generation_label(generation: &SourceGeneration) -> String {
    match generation {
        SourceGeneration::Known(value) => value.clone(),
        _ => "unknown".to_string(),
    }
}

fn downgrade_for_kind(snapshot: &mut ProviderAdapterSnapshot, kind: SemanticFactKind) {
    // Every suppressed record downgrades both its own capability family and
    // the sibling that uses its records for cursor resolution: declaration
    // records select position references, occurrence-derived records select
    // position declarations (and references, via their entity binding).
    match kind {
        SemanticFactKind::Declaration | SemanticFactKind::Module => {
            snapshot.downgrade(ProviderQueryCapability::Declarations);
            snapshot.downgrade(ProviderQueryCapability::References);
            snapshot.downgrade(ProviderQueryCapability::Visibility);
            snapshot.downgrade(ProviderQueryCapability::ScopeBindings);
        }
        SemanticFactKind::Occurrence => {
            snapshot.downgrade(ProviderQueryCapability::Declarations);
            snapshot.downgrade(ProviderQueryCapability::References);
            snapshot.downgrade(ProviderQueryCapability::Visibility);
            snapshot.downgrade(ProviderQueryCapability::ScopeBindings);
        }
        SemanticFactKind::Import => {
            snapshot.downgrade(ProviderQueryCapability::Declarations);
            snapshot.downgrade(ProviderQueryCapability::References);
            snapshot.downgrade(ProviderQueryCapability::Visibility);
        }
        SemanticFactKind::Boundary => {
            snapshot.downgrade(ProviderQueryCapability::Boundaries);
        }
        _ => {}
    }
}

fn adapt_shard(
    shard: &FileFactShard,
    producer: SemanticProducer,
    trace_source: ProviderFactSourceKind,
    snapshot: &ProviderAdapterSnapshot,
    records: &mut Vec<AdapterFactRecord>,
    limitations: &mut Vec<String>,
    incomplete: &mut BTreeSet<ProviderQueryCapability>,
) {
    // Identical duplicate rows carry no new information and collapse to the
    // first row. A contradictory pair tombstones the identity for the rest of
    // the shard: no later row may resurrect a contested binding.
    let mut anchors: BTreeMap<AnchorId, &AnchorFact> = BTreeMap::new();
    let mut rejected_anchors: BTreeSet<AnchorId> = BTreeSet::new();
    for anchor in &shard.anchors {
        if rejected_anchors.contains(&anchor.id) {
            continue;
        }
        match anchors.get(&anchor.id) {
            Some(existing) if *existing != anchor => {
                anchors.remove(&anchor.id);
                rejected_anchors.insert(anchor.id);
                limitations.push(format!(
                    "anchor:{}:conflicting_duplicate:{}",
                    anchor.id.0, shard.file_id.0
                ));
                incomplete.extend([
                    ProviderQueryCapability::Declarations,
                    ProviderQueryCapability::References,
                    ProviderQueryCapability::Visibility,
                    ProviderQueryCapability::ScopeBindings,
                    ProviderQueryCapability::Boundaries,
                ]);
            }
            Some(_) => {}
            None => {
                anchors.insert(anchor.id, anchor);
            }
        }
    }

    // Conflicting duplicate entity identities cannot bind a truthful name and
    // are tombstoned on the same terms as anchors.
    let mut entity_names: BTreeMap<EntityId, String> = BTreeMap::new();
    let mut rejected_entities: BTreeSet<EntityId> = BTreeSet::new();
    for entity in &shard.entities {
        if rejected_entities.contains(&entity.id) {
            continue;
        }
        match entity_names.get(&entity.id) {
            Some(existing) if *existing != entity.canonical_name => {
                entity_names.remove(&entity.id);
                rejected_entities.insert(entity.id);
                limitations.push(format!(
                    "entity:{}:conflicting_duplicate:{}",
                    entity.id.0, shard.file_id.0
                ));
                incomplete.extend([
                    ProviderQueryCapability::Declarations,
                    ProviderQueryCapability::References,
                    ProviderQueryCapability::Visibility,
                    ProviderQueryCapability::ScopeBindings,
                ]);
            }
            Some(_) => {}
            None => {
                entity_names.insert(entity.id, entity.canonical_name.clone());
            }
        }
    }

    for entity in &shard.entities {
        if entity_names.get(&entity.id) != Some(&entity.canonical_name) {
            // Conflicting duplicate identity; already limited above.
            continue;
        }
        let Some(anchor_id) = entity.anchor_id else {
            limitations.push(format!("entity:{}:missing_source_anchor", entity.id.0));
            incomplete.extend([
                // References is included because declaration records are
                // cursor selectors for position-subject references queries;
                // a suppressed declaration must not leave a Complete
                // references denominator that could grant exact-empty there.
                ProviderQueryCapability::Declarations,
                ProviderQueryCapability::References,
                ProviderQueryCapability::Visibility,
                ProviderQueryCapability::ScopeBindings,
            ]);
            continue;
        };
        let Some(anchor) = anchors.get(&anchor_id).copied() else {
            limitations
                .push(format!("entity:{}:unresolved_source_anchor:{}", entity.id.0, anchor_id.0));
            incomplete.extend([
                ProviderQueryCapability::Declarations,
                ProviderQueryCapability::References,
                ProviderQueryCapability::Visibility,
                ProviderQueryCapability::ScopeBindings,
            ]);
            continue;
        };
        records.push(record_from_entity(entity, anchor, producer, trace_source, snapshot, shard));
    }

    // Occurrence identities tombstone on the same terms as anchors and
    // entities: two rows sharing one OccurrenceId with different content
    // (e.g. Definition vs Call) are contradictory producer data. Their
    // adapted envelopes can be EQUAL because occurrence_kind lives outside
    // the envelope, so the conflict must be settled here — collapsing them
    // would make the include_declaration filter order-dependent and could
    // false-empty a references query.
    let mut occurrences_by_id: BTreeMap<OccurrenceId, &OccurrenceFact> = BTreeMap::new();
    let mut rejected_occurrences: BTreeSet<OccurrenceId> = BTreeSet::new();
    for occurrence in &shard.occurrences {
        if rejected_occurrences.contains(&occurrence.id) {
            continue;
        }
        match occurrences_by_id.get(&occurrence.id) {
            Some(existing) if *existing != occurrence => {
                occurrences_by_id.remove(&occurrence.id);
                rejected_occurrences.insert(occurrence.id);
                limitations.push(format!(
                    "occurrence:{}:conflicting_duplicate:{}",
                    occurrence.id.0, shard.file_id.0
                ));
                incomplete.extend([
                    // Declarations is included because position-subject
                    // declaration queries resolve the cursor through
                    // occurrence records; a contested occurrence must not
                    // leave a Complete declarations denominator that could
                    // grant exact-empty at that cursor.
                    ProviderQueryCapability::Declarations,
                    ProviderQueryCapability::References,
                    ProviderQueryCapability::Visibility,
                    ProviderQueryCapability::ScopeBindings,
                    ProviderQueryCapability::Boundaries,
                ]);
            }
            Some(_) => {}
            None => {
                occurrences_by_id.insert(occurrence.id, occurrence);
            }
        }
    }

    for occurrence in &shard.occurrences {
        if occurrences_by_id.get(&occurrence.id) != Some(&occurrence) {
            // Conflicting duplicate identity; already limited above.
            continue;
        }
        let Some(anchor) = anchors.get(&occurrence.anchor_id).copied() else {
            limitations.push(format!(
                "occurrence:{}:unresolved_source_anchor:{}",
                occurrence.id.0, occurrence.anchor_id.0
            ));
            incomplete.extend([
                // Declarations is included because occurrence records are
                // cursor selectors for position-subject declaration queries;
                // a suppressed occurrence must not leave a Complete
                // declarations denominator that could grant exact-empty there.
                ProviderQueryCapability::Declarations,
                ProviderQueryCapability::References,
                ProviderQueryCapability::Visibility,
                ProviderQueryCapability::ScopeBindings,
                ProviderQueryCapability::Boundaries,
            ]);
            continue;
        };
        let entity_name =
            occurrence.entity_id.and_then(|entity_id| entity_names.get(&entity_id)).cloned();
        records.push(record_from_occurrence(
            occurrence,
            anchor,
            entity_name,
            producer,
            trace_source,
            snapshot,
            shard,
        ));
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
    let fact_id = stable_fact_id(b"entity", anchor.file_id, entity.id.0);
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
    AdapterFactRecord { envelope, names, occurrence_kind: None, trace }
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
    let fact_id = stable_fact_id(b"occurrence", anchor.file_id, occurrence.id.0);
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
    SourceAnchor::new(Some(anchor.id), anchor.file_id, anchor.span_start_byte, anchor.span_end_byte)
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

/// Stable fact identity bound to the file shard that owns the local id.
///
/// Two ordinary file shards may both contain `EntityId(30)`; including the file
/// identity keeps their canonical fact ids distinct.
fn stable_fact_id(domain: &[u8], file_id: FileId, raw: u64) -> FactId {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in domain.iter().copied().chain(file_id.0.to_le_bytes()).chain(raw.to_le_bytes()) {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    FactId(hash)
}

fn trace_source_allowed(producer: SemanticProducer, source: ProviderFactSourceKind) -> bool {
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
        // The surface is rebound to the requesting surface at query time.
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

/// Records selected for one request, with selection and value roles separated
/// so the checked boundary can verify position and subject relations.
struct Selection<'a> {
    /// Records that select the target at the request subject.
    selectors: Vec<&'a AdapterFactRecord>,
    /// Records returned as semantic values.
    values: Vec<&'a AdapterFactRecord>,
}

enum EntityTargets {
    Unqualified(BTreeSet<EntityId>),
    FileQualified(BTreeSet<(FileId, EntityId)>),
}

impl EntityTargets {
    fn contains(&self, record: &AdapterFactRecord) -> bool {
        let Some(entity_id) = record.envelope.entity_id else {
            return false;
        };
        match self {
            Self::Unqualified(targets) => targets.contains(&entity_id),
            Self::FileQualified(targets) => {
                targets.contains(&(record.envelope.anchor.file_id, entity_id))
            }
        }
    }
}

impl Selection<'_> {
    fn is_empty(&self) -> bool {
        self.selectors.is_empty() && self.values.is_empty()
    }

    fn all(&self) -> impl Iterator<Item = &AdapterFactRecord> {
        let mut seen = BTreeSet::new();
        self.selectors
            .iter()
            .chain(self.values.iter())
            .filter(move |record| seen.insert(record.envelope.fact_id))
            .copied()
    }
}

#[allow(clippy::too_many_arguments)]
fn query_records(
    request: &ProviderQueryRequest,
    control: &dyn ProviderQueryControl,
    records: &[AdapterFactRecord],
    snapshot: &ProviderAdapterSnapshot,
    limitations: &[String],
    producer: SemanticProducer,
    trace_source: ProviderFactSourceKind,
    denominator_scope: &str,
    snapshot_id: &str,
) -> Result<ProviderQueryResultDraft, ProviderQueryContractError> {
    if !request.is_well_formed() {
        return Err(ProviderQueryContractError::MalformedRequest);
    }
    if control.is_cancelled()
        || request.context.cancellation == ProviderCancellationState::Cancelled
    {
        return Ok(terminal_draft(
            ProviderQueryOutcome::Cancelled,
            ProviderQueryTerminalState::Cancelled,
        ));
    }
    if control.deadline_expired() || request.context.deadline == ProviderQueryDeadline::Expired {
        return Ok(terminal_draft(
            ProviderQueryOutcome::DeadlineExceeded,
            ProviderQueryTerminalState::DeadlineExceeded,
        ));
    }

    match request.context.readiness_state {
        ProviderReadinessState::Failed => {
            return Ok(terminal_draft(
                ProviderQueryOutcome::Error,
                ProviderQueryTerminalState::Failed,
            ));
        }
        ProviderReadinessState::Stale => {
            return Ok(no_value_draft(
                ProviderQueryOutcome::Stale,
                Vec::new(),
                None,
                Vec::new(),
                limitations.to_vec(),
            ));
        }
        ProviderReadinessState::Unavailable => {
            return Ok(unavailable_draft(
                request,
                snapshot,
                producer,
                trace_source,
                records.len(),
                limitations,
                "readiness_unavailable",
            ));
        }
        ProviderReadinessState::Building
            if snapshot.fallback_state != ProviderFallbackState::Fallback =>
        {
            return Ok(unavailable_draft(
                request,
                snapshot,
                producer,
                trace_source,
                records.len(),
                limitations,
                "readiness_building",
            ));
        }
        _ => {}
    }

    let capability = ProviderQueryCapability::from_query(&request.kind);
    if capability == ProviderQueryCapability::Readiness {
        return Ok(readiness_draft(
            request,
            records,
            snapshot,
            limitations,
            producer,
            trace_source,
            denominator_scope,
            snapshot_id,
        ));
    }

    let binding = generation_binding(request, snapshot);
    if binding != GenerationBinding::Current {
        let mut notes = limitations.to_vec();
        notes.push(match binding {
            GenerationBinding::Mismatched => "generation_mismatch".to_string(),
            GenerationBinding::Unknown => "generation_unknown".to_string(),
            GenerationBinding::Current => String::new(),
        });
        notes.retain(|note| !note.is_empty());
        let selection = select_records(request, records);
        if binding == GenerationBinding::Mismatched && !selection.is_empty() {
            // Facts exist but belong to another generation, so they are stale
            // for this request. Freshness is request-relative; the supporting
            // copies carry the staleness explicitly.
            let facts = selection_as_supporting(&selection, request, true)?;
            return Ok(no_value_draft(ProviderQueryOutcome::Stale, facts, None, Vec::new(), notes));
        }
        return Ok(unavailable_draft(
            request,
            snapshot,
            producer,
            trace_source,
            records.len(),
            &notes,
            "generation_not_bound",
        ));
    }

    let selection = select_records(request, records);
    let blockers = if capability == ProviderQueryCapability::Boundaries {
        Vec::new()
    } else {
        select_boundary_records(&request.subject, records)
    };

    let any_stale = selection
        .all()
        .chain(blockers.iter().copied())
        .any(|record| record.envelope.status() == SemanticFactStatus::Stale);
    let any_out_of_generation = selection.all().any(|record| !record_is_current(record, request));
    if any_stale || any_out_of_generation {
        let facts = selection_as_supporting(&selection, request, true)?;
        return Ok(no_value_draft(
            ProviderQueryOutcome::Stale,
            facts,
            None,
            Vec::new(),
            limitations.to_vec(),
        ));
    }
    if selection
        .all()
        .chain(blockers.iter().copied())
        .any(|record| record.envelope.status() == SemanticFactStatus::Refused)
    {
        let facts = selection_as_supporting(&selection, request, false)?;
        return Ok(no_value_draft(
            ProviderQueryOutcome::Refused,
            facts,
            None,
            Vec::new(),
            limitations.to_vec(),
        ));
    }

    if selection.values.is_empty() {
        if !blockers.is_empty() {
            let mut facts = selection_as_supporting(&selection, request, false)?;
            let mut present: BTreeSet<FactId> =
                facts.iter().map(|fact| fact.envelope().fact_id).collect();
            facts.extend(blocker_facts(request, &blockers, &mut present)?);
            let boundary = blockers.iter().find_map(|record| record.envelope.boundary.clone());
            let traces = traces_for(request, &facts, records);
            return Ok(no_value_draft(
                ProviderQueryOutcome::Dynamic,
                facts,
                boundary,
                traces,
                limitations.to_vec(),
            ));
        }
        if let Some(grant) = issue_completeness_grant(
            request,
            capability,
            records,
            snapshot,
            producer,
            denominator_scope,
            snapshot_id,
        ) {
            return Ok(ProviderQueryResultDraft::new(
                ProviderQueryOutcome::Exact,
                Vec::new(),
                Some(grant),
                ProviderQueryEvidenceInput::primary_completed(),
            ));
        }
        return Ok(unavailable_draft(
            request,
            snapshot,
            producer,
            trace_source,
            records.len(),
            limitations,
            "no_supporting_denominator",
        ));
    }

    if ambiguity_applies(request) && distinct_value_entities(&selection.values).len() > 1 {
        let mut facts = Vec::new();
        for record in selection.all() {
            // Every candidate selects the ambiguity; none is a value.
            facts.push(record_fact(record, ProviderQueryFactRole::Selector)?);
        }
        return Ok(no_value_draft(
            ProviderQueryOutcome::Ambiguous,
            facts,
            None,
            Vec::new(),
            limitations.to_vec(),
        ));
    }

    if !blockers.is_empty() {
        let mut facts = selection_facts(&selection)?;
        let mut present: BTreeSet<FactId> =
            facts.iter().map(|fact| fact.envelope().fact_id).collect();
        facts.extend(blocker_facts(request, &blockers, &mut present)?);
        let boundary = blockers.iter().find_map(|record| record.envelope.boundary.clone());
        let traces = traces_for(request, &facts, records);
        return Ok(value_draft(
            ProviderQueryOutcome::Degraded,
            facts,
            boundary,
            traces,
            limitations.to_vec(),
            ProviderResultPath::Primary,
        ));
    }

    if snapshot.fallback_state == ProviderFallbackState::Fallback {
        let facts = selection_facts(&selection)?;
        let mut traces = traces_for(request, &facts, records);
        traces.push(coverage_trace(
            request,
            snapshot,
            producer,
            trace_source,
            ProviderFallbackState::Fallback,
            records.len(),
        ));
        return Ok(value_draft(
            ProviderQueryOutcome::Fallback,
            facts,
            None,
            traces,
            limitations.to_vec(),
            ProviderResultPath::Fallback,
        ));
    }

    let exact_grade =
        selection.all().all(|record| record.envelope.status() == SemanticFactStatus::Exact);
    let uniform_provenance =
        uniform_exact_provenance(selection.all().map(|record| &record.envelope));
    let exact_eligible = snapshot.facts_can_be_exact()
        && request.context.is_exact_ready()
        && exact_grade
        && uniform_provenance.is_some()
        // The checked boundary rejects Exact-with-limitations; an imperfect
        // shard degrades with its limitations named instead.
        && limitations.is_empty();
    if exact_eligible {
        let facts = selection_facts(&selection)?;
        let traces = traces_for(request, &facts, records);
        return Ok(value_draft(
            ProviderQueryOutcome::Exact,
            facts,
            None,
            traces,
            limitations.to_vec(),
            ProviderResultPath::Primary,
        ));
    }

    // Degraded must name why it is not exact whenever the facts themselves
    // would otherwise qualify.
    let mut notes = limitations.to_vec();
    if exact_grade && uniform_provenance.is_none() {
        notes.push("mixed_exact_provenance".to_string());
    }
    if request.context.readiness_requirement == ProviderReadinessRequirement::EditAuthorizing {
        notes.push("edit_authorization_requires_guarded_plan:#6819".to_string());
    }
    if exact_grade
        && notes.is_empty()
        && !request.context.is_exact_ready()
        && request.context.readiness_state == ProviderReadinessState::Ready
    {
        notes.push("request_context_not_exact_ready".to_string());
    }
    if exact_grade && notes.is_empty() && !snapshot.facts_can_be_exact() {
        notes.push("snapshot_not_exact_grade".to_string());
    }
    let facts = selection_facts(&selection)?;
    let traces = traces_for(request, &facts, records);
    Ok(value_draft(
        ProviderQueryOutcome::Degraded,
        facts,
        None,
        traces,
        notes,
        ProviderResultPath::Primary,
    ))
}

#[allow(clippy::too_many_arguments)]
fn readiness_draft(
    request: &ProviderQueryRequest,
    records: &[AdapterFactRecord],
    snapshot: &ProviderAdapterSnapshot,
    limitations: &[String],
    producer: SemanticProducer,
    trace_source: ProviderFactSourceKind,
    denominator_scope: &str,
    snapshot_id: &str,
) -> ProviderQueryResultDraft {
    if generation_binding(request, snapshot) == GenerationBinding::Current
        && let Some(grant) = issue_completeness_grant(
            request,
            ProviderQueryCapability::Readiness,
            records,
            snapshot,
            producer,
            denominator_scope,
            snapshot_id,
        )
    {
        return ProviderQueryResultDraft::new(
            ProviderQueryOutcome::Exact,
            Vec::new(),
            Some(grant),
            ProviderQueryEvidenceInput::primary_completed(),
        );
    }
    unavailable_draft(
        request,
        snapshot,
        producer,
        trace_source,
        records.len(),
        limitations,
        "readiness_not_exact",
    )
}

/// Issue an exact-empty completeness grant from the adapter's concrete
/// denominator, or return `None` when any input cannot support exact authority.
fn issue_completeness_grant(
    request: &ProviderQueryRequest,
    capability: ProviderQueryCapability,
    records: &[AdapterFactRecord],
    snapshot: &ProviderAdapterSnapshot,
    producer: SemanticProducer,
    denominator_scope: &str,
    snapshot_id: &str,
) -> Option<ProviderCompletenessGrant> {
    if producer == SemanticProducer::Unknown
        || !snapshot.can_claim_exact(capability)
        || !request.context.is_exact_ready()
        || generation_binding(request, snapshot) != GenerationBinding::Current
    {
        return None;
    }
    let requested_file = match &request.subject {
        ProviderQuerySubject::File(file_id) | ProviderQuerySubject::Position { file_id, .. } => {
            Some(*file_id)
        }
        _ => None,
    };
    if requested_file.is_some_and(|file_id| {
        !records.iter().any(|record| record.envelope.anchor.file_id == file_id)
    }) {
        return None;
    }
    let units: Vec<_> =
        records.iter().filter(|record| capability_covers(capability, record)).collect();
    if units.is_empty() {
        return None;
    }
    // Every denominator unit must itself be exact-grade and current for this
    // request: a stale, refused, or out-of-generation fact cannot vouch for
    // the emptiness of its capability family.
    if !units.iter().all(|record| {
        record.envelope.status() == SemanticFactStatus::Exact && record_is_current(record, request)
    }) {
        return None;
    }
    let provenance = uniform_exact_provenance(units.iter().map(|record| &record.envelope))?;
    if !units
        .iter()
        .all(|record| record.envelope.confidence == SemanticConfidence::Known(Confidence::High))
    {
        return None;
    }
    ProviderCompletenessGrant::issue_for_request(
        request,
        producer,
        format!("{producer:?}:{capability:?}:{denominator_scope}"),
        snapshot_id.to_string(),
        units.len() as u64,
        provenance,
        SemanticConfidence::Known(Confidence::High),
        snapshot.freshness,
    )
    .ok()
}

/// Uniform exact-grade provenance across the denominator, when one exists.
fn uniform_exact_provenance<'a>(
    envelopes: impl IntoIterator<Item = &'a SemanticFactEnvelope>,
) -> Option<SemanticProvenance> {
    let mut provenance: Option<SemanticProvenance> = None;
    for envelope in envelopes {
        let current = envelope.provenance;
        if !semantic_provenance_is_exact(current) {
            return None;
        }
        match provenance {
            None => provenance = Some(current),
            Some(existing) if existing == current => {}
            Some(_) => return None,
        }
    }
    provenance
}

fn capability_covers(capability: ProviderQueryCapability, record: &AdapterFactRecord) -> bool {
    match capability {
        ProviderQueryCapability::Declarations => {
            matches!(record.envelope.kind, SemanticFactKind::Declaration | SemanticFactKind::Module)
        }
        ProviderQueryCapability::References => record.envelope.kind == SemanticFactKind::Occurrence,
        ProviderQueryCapability::Visibility => {
            matches!(record.envelope.kind, SemanticFactKind::Import | SemanticFactKind::Module)
        }
        ProviderQueryCapability::ScopeBindings => {
            matches!(
                record.envelope.kind,
                SemanticFactKind::Declaration | SemanticFactKind::Occurrence
            )
        }
        ProviderQueryCapability::Boundaries => record.envelope.kind == SemanticFactKind::Boundary,
        _ => true,
    }
}

fn record_is_current(record: &AdapterFactRecord, request: &ProviderQueryRequest) -> bool {
    generation_is_known(&record.envelope.source_generation)
        && record.envelope.source_generation == request.context.document_generation
}

fn ambiguity_applies(request: &ProviderQueryRequest) -> bool {
    matches!(request.kind, ProviderQueryKind::Declaration | ProviderQueryKind::References { .. })
        && matches!(
            request.subject,
            ProviderQuerySubject::Symbol(_) | ProviderQuerySubject::Position { .. }
        )
}

fn distinct_value_entities(values: &[&AdapterFactRecord]) -> BTreeSet<EntityId> {
    values.iter().filter_map(|record| record.envelope.entity_id).collect()
}

fn select_records<'a>(
    request: &ProviderQueryRequest,
    records: &'a [AdapterFactRecord],
) -> Selection<'a> {
    match &request.kind {
        ProviderQueryKind::Declaration => select_declaration_records(request, records),
        ProviderQueryKind::References { include_declaration } => {
            select_reference_records(request, records, *include_declaration)
        }
        ProviderQueryKind::Visibility => select_direct_records(request, records, |record| {
            matches!(record.envelope.kind, SemanticFactKind::Import | SemanticFactKind::Module)
        }),
        ProviderQueryKind::ScopeBindings => select_direct_records(request, records, |record| {
            matches!(
                record.envelope.kind,
                SemanticFactKind::Declaration | SemanticFactKind::Occurrence
            )
        }),
        ProviderQueryKind::Boundaries => select_direct_records(request, records, |record| {
            record.envelope.kind == SemanticFactKind::Boundary
        }),
        _ => Selection { selectors: Vec::new(), values: Vec::new() },
    }
}

/// Direct-matching values for one kind family.
///
/// Position subjects double as their own selector; the checked boundary
/// rejects sibling facts that are not related to a cursor-bound selector, so
/// no scope fan-out is attempted here.
fn select_direct_records<'a>(
    request: &ProviderQueryRequest,
    records: &'a [AdapterFactRecord],
    kind_matches: impl Fn(&AdapterFactRecord) -> bool,
) -> Selection<'a> {
    let values: Vec<_> = records
        .iter()
        .filter(|record| kind_matches(record))
        .filter(|record| subject_matches(&request.subject, record))
        .collect();
    let selectors = if matches!(request.subject, ProviderQuerySubject::Position { .. }) {
        values.clone()
    } else {
        Vec::new()
    };
    Selection { selectors, values }
}

fn select_declaration_records<'a>(
    request: &ProviderQueryRequest,
    records: &'a [AdapterFactRecord],
) -> Selection<'a> {
    if matches!(request.subject, ProviderQuerySubject::Position { .. }) {
        let (selectors, targets) = cursor_targets(request, records);
        let values = records
            .iter()
            .filter(|record| {
                matches!(
                    record.envelope.kind,
                    SemanticFactKind::Declaration | SemanticFactKind::Module
                ) && record.envelope.entity_id.is_some_and(|entity_id| {
                    targets.contains(&(record.envelope.anchor.file_id, entity_id))
                })
            })
            .collect();
        return Selection { selectors, values };
    }
    Selection {
        selectors: Vec::new(),
        values: records
            .iter()
            .filter(|record| {
                matches!(
                    record.envelope.kind,
                    SemanticFactKind::Declaration | SemanticFactKind::Module
                ) && subject_matches(&request.subject, record)
            })
            .collect(),
    }
}

fn select_reference_records<'a>(
    request: &ProviderQueryRequest,
    records: &'a [AdapterFactRecord],
    include_declaration: bool,
) -> Selection<'a> {
    let (selectors, targets) = match &request.subject {
        ProviderQuerySubject::Entity(entity_id) => {
            (Vec::new(), EntityTargets::Unqualified(BTreeSet::from([*entity_id])))
        }
        ProviderQuerySubject::Position { .. } => {
            let (selectors, targets) = cursor_targets(request, records);
            (selectors, EntityTargets::FileQualified(targets))
        }
        _ => {
            let selectors: Vec<_> = records
                .iter()
                .filter(|record| subject_matches(&request.subject, record))
                .filter(|record| record.envelope.entity_id.is_some())
                .collect();
            let targets = selectors.iter().filter_map(|record| record.envelope.entity_id).collect();
            (selectors, EntityTargets::Unqualified(targets))
        }
    };
    let values = records
        .iter()
        .filter(|record| {
            let occurrence = record.envelope.kind == SemanticFactKind::Occurrence
                && (include_declaration
                    || record.occurrence_kind != Some(OccurrenceKind::Definition));
            let declaration = include_declaration
                && matches!(
                    record.envelope.kind,
                    SemanticFactKind::Declaration | SemanticFactKind::Module
                );
            (occurrence || declaration) && targets.contains(record)
        })
        .collect();
    Selection { selectors, values }
}

/// Resolve the target entity set through the records that actually sit at the
/// cursor, including reference occurrences.
///
/// Resolving through the occurrence first is what keeps a cursor on a
/// reference from false-emptying declaration and references queries. Boundary
/// records never resolve a target: their entity binding is the dynamic or
/// compatibility boundary itself, not evidence of a concrete target.
fn cursor_targets<'a>(
    request: &ProviderQueryRequest,
    records: &'a [AdapterFactRecord],
) -> (Vec<&'a AdapterFactRecord>, BTreeSet<(FileId, EntityId)>) {
    let selectors: Vec<_> = records
        .iter()
        .filter(|record| subject_matches(&request.subject, record))
        .filter(|record| record.envelope.kind != SemanticFactKind::Boundary)
        .filter(|record| record.envelope.entity_id.is_some())
        .collect();
    let targets = selectors
        .iter()
        .filter_map(|record| {
            record.envelope.entity_id.map(|entity_id| (record.envelope.anchor.file_id, entity_id))
        })
        .collect();
    (selectors, targets)
}

fn select_boundary_records<'a>(
    subject: &ProviderQuerySubject,
    records: &'a [AdapterFactRecord],
) -> Vec<&'a AdapterFactRecord> {
    records
        .iter()
        .filter(|record| record.envelope.kind == SemanticFactKind::Boundary)
        .filter(|record| subject_matches(subject, record))
        .collect()
}

fn subject_matches(subject: &ProviderQuerySubject, record: &AdapterFactRecord) -> bool {
    match subject {
        ProviderQuerySubject::Entity(entity_id) => record.envelope.entity_id == Some(*entity_id),
        ProviderQuerySubject::File(file_id) => record.envelope.anchor.file_id == *file_id,
        ProviderQuerySubject::Position { file_id, byte_offset } => {
            record.envelope.anchor.file_id == *file_id
                && range_contains(&record.envelope.anchor, *byte_offset)
        }
        ProviderQuerySubject::Package(package) => {
            record.envelope.package.as_deref() == Some(package.as_str())
                || record.names.iter().any(|name| name == package)
        }
        ProviderQuerySubject::Symbol(symbol) => record.names.iter().any(|name| name == symbol),
        ProviderQuerySubject::Workspace => true,
    }
}

fn range_contains(anchor: &SourceAnchor, byte_offset: u32) -> bool {
    if anchor.start_byte == anchor.end_byte {
        byte_offset == anchor.start_byte
    } else {
        anchor.start_byte <= byte_offset && byte_offset < anchor.end_byte
    }
}

/// Build value/selector facts for an outcome that returns values.
fn selection_facts(
    selection: &Selection<'_>,
) -> Result<Vec<ProviderQueryFact>, ProviderQueryContractError> {
    let mut facts = Vec::new();
    for record in &selection.values {
        let role = if selection.selectors.contains(record) {
            ProviderQueryFactRole::SelectorValue
        } else {
            ProviderQueryFactRole::Value
        };
        facts.push(record_fact(record, role)?);
    }
    for record in &selection.selectors {
        if !selection.values.contains(record) {
            facts.push(record_fact(record, ProviderQueryFactRole::Selector)?);
        }
    }
    Ok(facts)
}

/// Build facts for a no-value outcome.
///
/// Position queries keep their cursor-bound selector so the checked boundary
/// can still verify the subject relation; every other record is supporting.
/// When `mark_stale` is set, records that are stale or out of generation for
/// this request carry that staleness explicitly (freshness is request-relative)
/// while current records keep their own metadata.
fn selection_as_supporting(
    selection: &Selection<'_>,
    request: &ProviderQueryRequest,
    mark_stale: bool,
) -> Result<Vec<ProviderQueryFact>, ProviderQueryContractError> {
    let mut facts = Vec::new();
    for record in selection.all() {
        let stale = record.envelope.status() == SemanticFactStatus::Stale
            || !record_is_current(record, request);
        let envelope = if mark_stale && stale {
            let mut envelope = record.envelope.clone();
            envelope.freshness = SemanticFreshness::Stale;
            envelope.reason_code = SemanticReasonCode::StaleDependency;
            envelope
        } else {
            record.envelope.clone()
        };
        let role = if selection.selectors.contains(&record) {
            ProviderQueryFactRole::Selector
        } else {
            ProviderQueryFactRole::Supporting
        };
        facts.push(ProviderQueryFact::try_new(
            role,
            ProviderFactGenerationScope::Document,
            envelope,
            record.names.clone(),
        )?);
    }
    Ok(facts)
}

/// Boundary facts join a draft without duplicating the cursor selector.
///
/// For position subjects a boundary sits at the cursor by selection, so it
/// carries the selector role and satisfies the cursor-binding requirement.
fn blocker_facts(
    request: &ProviderQueryRequest,
    blockers: &[&AdapterFactRecord],
    present: &mut BTreeSet<FactId>,
) -> Result<Vec<ProviderQueryFact>, ProviderQueryContractError> {
    let mut facts = Vec::new();
    for record in blockers {
        if !present.insert(record.envelope.fact_id) {
            continue;
        }
        let role = if matches!(request.subject, ProviderQuerySubject::Position { .. }) {
            ProviderQueryFactRole::Selector
        } else {
            ProviderQueryFactRole::Supporting
        };
        facts.push(record_fact(record, role)?);
    }
    Ok(facts)
}

fn record_fact(
    record: &AdapterFactRecord,
    role: ProviderQueryFactRole,
) -> Result<ProviderQueryFact, ProviderQueryContractError> {
    ProviderQueryFact::try_new(
        role,
        ProviderFactGenerationScope::Document,
        record.envelope.clone(),
        record.names.clone(),
    )
}

fn traces_for(
    request: &ProviderQueryRequest,
    facts: &[ProviderQueryFact],
    records: &[AdapterFactRecord],
) -> Vec<ProviderFactTrace> {
    let ids: BTreeSet<FactId> = facts.iter().map(|fact| fact.envelope().fact_id).collect();
    records
        .iter()
        .filter(|record| ids.contains(&record.envelope.fact_id))
        .filter_map(|record| record.trace.clone())
        .map(|mut trace| {
            trace.surface = request.surface;
            trace
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn value_draft(
    outcome: ProviderQueryOutcome,
    facts: Vec<ProviderQueryFact>,
    boundary: Option<BoundaryLink>,
    traces: Vec<ProviderFactTrace>,
    limitations: Vec<String>,
    result_path: ProviderResultPath,
) -> ProviderQueryResultDraft {
    ProviderQueryResultDraft::new(
        outcome,
        facts,
        None,
        ProviderQueryEvidenceInput::new(
            result_path,
            boundary,
            SemanticReasonCode::Unknown,
            traces,
            limitations,
            ProviderQueryTerminalState::Completed,
        ),
    )
}

fn no_value_draft(
    outcome: ProviderQueryOutcome,
    facts: Vec<ProviderQueryFact>,
    boundary: Option<BoundaryLink>,
    traces: Vec<ProviderFactTrace>,
    limitations: Vec<String>,
) -> ProviderQueryResultDraft {
    ProviderQueryResultDraft::new(
        outcome,
        facts,
        None,
        ProviderQueryEvidenceInput::new(
            ProviderResultPath::Primary,
            boundary,
            reason_for_outcome(outcome),
            traces,
            limitations,
            ProviderQueryTerminalState::Completed,
        ),
    )
}

#[allow(clippy::too_many_arguments)]
fn unavailable_draft(
    request: &ProviderQueryRequest,
    snapshot: &ProviderAdapterSnapshot,
    producer: SemanticProducer,
    trace_source: ProviderFactSourceKind,
    record_count: usize,
    limitations: &[String],
    reason: &str,
) -> ProviderQueryResultDraft {
    let mut notes = limitations.to_vec();
    notes.push(reason.to_string());
    ProviderQueryResultDraft::new(
        ProviderQueryOutcome::Unavailable,
        Vec::new(),
        None,
        ProviderQueryEvidenceInput::new(
            ProviderResultPath::Primary,
            None,
            SemanticReasonCode::Unknown,
            vec![coverage_trace(
                request,
                snapshot,
                producer,
                trace_source,
                ProviderFallbackState::Unavailable,
                record_count,
            )],
            notes,
            ProviderQueryTerminalState::Completed,
        ),
    )
}

/// Coverage trace proving the adapter examined its denominator instead of
/// manufacturing empty authority or hiding a fallback.
fn coverage_trace(
    request: &ProviderQueryRequest,
    snapshot: &ProviderAdapterSnapshot,
    producer: SemanticProducer,
    trace_source: ProviderFactSourceKind,
    fallback_state: ProviderFallbackState,
    record_count: usize,
) -> ProviderFactTrace {
    let source = if producer == SemanticProducer::Unknown {
        trace_source
    } else {
        source_for_producer(producer, trace_source)
    };
    ProviderFactTrace::new(
        request.surface,
        source,
        Provenance::SearchFallback,
        Confidence::Low,
        provider_freshness(snapshot.freshness),
        fallback_state,
        Some(format!("adapter-coverage:{record_count}")),
        None,
        snapshot.model_version,
    )
}

fn terminal_draft(
    outcome: ProviderQueryOutcome,
    terminal: ProviderQueryTerminalState,
) -> ProviderQueryResultDraft {
    ProviderQueryResultDraft::new(
        outcome,
        Vec::new(),
        None,
        ProviderQueryEvidenceInput::new(
            ProviderResultPath::Primary,
            None,
            SemanticReasonCode::Unknown,
            Vec::new(),
            Vec::new(),
            terminal,
        ),
    )
}

fn reason_for_outcome(outcome: ProviderQueryOutcome) -> SemanticReasonCode {
    match outcome {
        ProviderQueryOutcome::Stale => SemanticReasonCode::StaleDependency,
        ProviderQueryOutcome::Dynamic => SemanticReasonCode::DynamicValue,
        ProviderQueryOutcome::Refused => SemanticReasonCode::UnsupportedEffect,
        _ => SemanticReasonCode::Unknown,
    }
}
