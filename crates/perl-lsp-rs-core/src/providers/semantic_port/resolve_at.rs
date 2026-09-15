//! Provider-neutral cursor occurrence identity, resolved once per request.
//!
//! Navigation providers historically reduced a request to a token spelling and
//! then applied their own lookup rule. In Perl one spelling can name different
//! lexical bindings, package subs, imports, methods, generated members, or
//! entities in another root, so a spelling is a search key and never an exact
//! identity. Two providers starting from the same spelling can therefore
//! disagree about what the cursor selected.
//!
//! This module owns the one identity step both providers share:
//!
//! ```text
//! (file, byte offset, generation basis) -> ResolveAtOutcome
//! ```
//!
//! [`ResolveAtOutcome::Exact`] names one occurrence and one canonical entity.
//! Every other state is mechanically distinct, so a caller can never confuse
//! "the cursor resolved to nothing downstream" with "the cursor itself could
//! not be resolved". The result carries the generation basis it was resolved
//! against, so a caller that resolves the cursor against one accepted
//! generation and queries targets against another can be detected rather than
//! silently trusted.
//!
//! This module introduces no occurrence producer. It composes the facts the
//! semantic layer already publishes, through the narrow [`ResolveAtSource`]
//! port. A name-keyed definition scan is deliberately *not* admitted here:
//! promoting a uniquely-matching same-name definition to an exact identity is
//! the precise error this layer exists to prevent. Callers that still need that
//! fallback keep it in their own provider, where it stays visible as a
//! spelling-based step.

use perl_semantic_facts::{
    AnchorId, Confidence, EntityFact, EntityId, EntityKind, FileId, OccurrenceFact, OccurrenceId,
    OccurrenceKind, Provenance, ScopeId, SourceGeneration,
};

/// Narrow read port supplying the occurrence facts this layer composes.
///
/// Kept narrower than the full semantic query facade so the resolution rule can
/// be proven against stubs without standing up a second workspace snapshot.
pub trait ResolveAtSource {
    /// Entity and occurrence covering `byte_offset`, when one is published.
    fn resolve_symbol_at(
        &self,
        file_id: FileId,
        byte_offset: u32,
    ) -> Option<(EntityFact, OccurrenceFact)>;

    /// Dynamic-boundary occurrence covering `byte_offset`, when one is published.
    fn resolve_dynamic_boundary_at(
        &self,
        file_id: FileId,
        byte_offset: u32,
    ) -> Option<OccurrenceFact>;
}

/// Adapts the workspace semantic facade to the narrow resolve port.
///
/// An explicit adapter rather than a blanket impl over `SemanticQueries`: a
/// blanket impl would own every type for this trait and leave no room for the
/// stub sources the resolution rule is proven against.
///
/// # Reachability of the entity-less case
///
/// `WorkspaceSemanticQueries::symbol_at` resolves the entity itself and returns
/// `None` when the covering occurrence carries no `entity_id`. Through *this*
/// adapter an entity-less occurrence therefore arrives as "no occurrence", so
/// the cursor reports [`ResolveAtOutcome::Unavailable`] and
/// [`ResolveLimitation::OccurrenceWithoutEntity`] is not reachable.
///
/// That state is kept in the model deliberately rather than deleted: the
/// distinction it draws — a producer published an occurrence but resolved no
/// entity for it — is real, and collapsing it into "nothing is here" is exactly
/// the conflation this layer exists to prevent. Exposing it through the
/// workspace facade needs an occurrence-only lookup on `SemanticQueries`, which
/// belongs to the producer, not to this layer. Until then it is modelled and
/// proven against stub sources, but not observable through this adapter.
#[derive(Debug, Clone, Copy)]
pub struct SemanticQueriesResolveSource<'queries, Q: ?Sized>(&'queries Q);

impl<'queries, Q> SemanticQueriesResolveSource<'queries, Q>
where
    Q: perl_workspace::semantic::queries::SemanticQueries + ?Sized,
{
    /// Borrow a semantic facade as a resolve source.
    ///
    /// Consumes the facts the workspace already publishes; opens no second view.
    #[must_use]
    pub fn new(queries: &'queries Q) -> Self {
        Self(queries)
    }
}

impl<Q> ResolveAtSource for SemanticQueriesResolveSource<'_, Q>
where
    Q: perl_workspace::semantic::queries::SemanticQueries + ?Sized,
{
    fn resolve_symbol_at(
        &self,
        file_id: FileId,
        byte_offset: u32,
    ) -> Option<(EntityFact, OccurrenceFact)> {
        self.0.symbol_at(file_id, byte_offset)
    }

    fn resolve_dynamic_boundary_at(
        &self,
        file_id: FileId,
        byte_offset: u32,
    ) -> Option<OccurrenceFact> {
        self.0.dynamic_boundary_at(file_id, byte_offset, None)
    }
}

/// Build the generation basis for one request from the accepted workspace view.
///
/// The one constructor every navigation provider is expected to use, so two
/// providers answering the same request do not drift onto different bases.
/// [`ResolveGenerationBasis::new`] stays public for tests and for callers that
/// genuinely hold their own generations, so this is a convention the callers
/// keep rather than something the type enforces. Both components are the *accepted* view's
/// identities, not the live document's — `indexed_generation` is the document
/// generation the index was actually built from, so an exact identity resolved
/// against it is honest about which snapshot it saw.
///
/// A URI the index has never seen yields an explicit unknown document
/// generation rather than a fabricated one.
///
/// The two halves are read under the index's documented torn-read protocol
/// (`WorkspaceIndex::write_version`, #5116): capture the write version before
/// and after reading the document generation and accept the pair only when it
/// did not move. Without that guard an interleaved index write could pair a
/// document generation from one snapshot with a workspace version from
/// another, producing a basis describing a snapshot that never existed —
/// which would silently defeat the drift detection this value exists for.
#[must_use]
pub fn accepted_generation_basis(
    index: &perl_workspace::workspace_index::WorkspaceIndex,
    uri: &str,
) -> ResolveGenerationBasis {
    stable_generation_basis(
        || index.write_version(),
        || index.indexed_generation(uri),
        uri,
        STABLE_BASIS_ATTEMPTS,
    )
}

/// Attempts allowed to observe a stable write version around the read.
///
/// A small bound: contention here is a brief index write, not a queue. When it
/// is exceeded the basis is reported unknown rather than retried forever.
const STABLE_BASIS_ATTEMPTS: u8 = 3;

/// The torn-read protocol itself, over plain closures so it can be proven
/// against a source that changes version mid-read without racing a real index.
fn stable_generation_basis<Version, Document>(
    write_version: Version,
    indexed_generation: Document,
    uri: &str,
    attempts: u8,
) -> ResolveGenerationBasis
where
    Version: Fn() -> u64,
    Document: Fn() -> Option<u32>,
{
    for _ in 0..attempts {
        let before = write_version();
        let document = indexed_generation();
        if write_version() != before {
            continue;
        }
        let document_generation = document.map_or(SourceGeneration::Unknown, |generation| {
            SourceGeneration::known(format!("{uri}@{generation}"))
        });
        return ResolveGenerationBasis::new(
            document_generation,
            SourceGeneration::known(format!("workspace-index@{before}")),
        );
    }

    // No stable pair was observable. Say so explicitly: an unknown generation
    // is a state this type models, a fabricated one is not.
    ResolveGenerationBasis::new(SourceGeneration::Unknown, SourceGeneration::Unknown)
}

/// Accepted generations one resolve result is bound to.
///
/// Definition and references answering the same request must resolve the cursor
/// against the same basis; comparing this value is how a caller proves it.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ResolveGenerationBasis {
    /// Generation of the document the cursor offset addresses.
    pub document_generation: SourceGeneration,
    /// Accepted workspace/model generation used for cross-file relationships.
    pub workspace_generation: SourceGeneration,
}

impl ResolveGenerationBasis {
    /// Construct a generation basis.
    #[must_use]
    pub fn new(
        document_generation: SourceGeneration,
        workspace_generation: SourceGeneration,
    ) -> Self {
        Self { document_generation, workspace_generation }
    }

    /// Whether both generations can identify their snapshot.
    ///
    /// An unknown generation is explicit and never counts as a known basis.
    #[must_use]
    pub fn is_known(&self) -> bool {
        self.document_generation.is_known() && self.workspace_generation.is_known()
    }
}

/// Identity-affecting boundary that survives into the resolved subject.
///
/// These are recorded rather than resolved: none of them can be turned into an
/// exact identity without runtime Perl semantics this layer does not model.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ResolveLimitation {
    /// Receiver or method selector is only known dynamically.
    DynamicSelector,
    /// Symbolic reference through a computed name.
    SymbolicReference,
    /// Typeglob or stash mutation can rebind this identity.
    TypeglobMutation,
    /// Entity is generated and has no source body of its own.
    GeneratedWithoutSourceBody,
    /// Import alias could not be resolved to its exporting entity.
    UnresolvedAlias,
    /// Occurrence carries no canonical entity identity.
    OccurrenceWithoutEntity,
    /// Producer published the occurrence below exact provenance.
    NonExactProvenance,
    /// Producer published the bound entity below exact provenance.
    NonExactEntityProvenance,
    /// The accepted view could not supply a stable generation basis.
    UnknownGeneration,
}

impl ResolveLimitation {
    /// Stable identifier for receipts and decision traces.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DynamicSelector => "dynamic_selector",
            Self::SymbolicReference => "symbolic_reference",
            Self::TypeglobMutation => "typeglob_mutation",
            Self::GeneratedWithoutSourceBody => "generated_without_source_body",
            Self::UnresolvedAlias => "unresolved_alias",
            Self::OccurrenceWithoutEntity => "occurrence_without_entity",
            Self::NonExactProvenance => "non_exact_provenance",
            Self::NonExactEntityProvenance => "non_exact_entity_provenance",
            Self::UnknownGeneration => "unknown_generation",
        }
    }
}

/// Why the semantic view could not answer yet.
///
/// Distinct from [`ResolveUnavailable`]: the request may succeed unchanged once
/// the view becomes ready.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ResolveNotReady {
    /// No workspace index was available.
    WorkspaceIndexUnavailable,
    /// Semantic queries could not be opened for the request subject.
    SemanticQueriesUnavailable,
}

impl ResolveNotReady {
    /// Stable identifier for receipts and decision traces.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::WorkspaceIndexUnavailable => "workspace_index_unavailable",
            Self::SemanticQueriesUnavailable => "semantic_queries_unavailable",
        }
    }
}

/// Why no occurrence identity exists for this cursor.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ResolveUnavailable {
    /// Offset could not be represented by the semantic query API.
    ByteOffsetOutOfRange,
    /// No occurrence is published at this position.
    NoOccurrenceAtPosition,
}

impl ResolveUnavailable {
    /// Stable identifier for receipts and decision traces.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ByteOffsetOutOfRange => "byte_offset_out_of_range",
            Self::NoOccurrenceAtPosition => "no_occurrence_at_position",
        }
    }
}

/// One occurrence identity resolved at a cursor.
///
/// Identity is `occurrence_id` plus `entity_id`; `canonical_name` is carried for
/// presentation and diagnostics only and is never the identity.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedOccurrence {
    /// Identity of the occurrence under the cursor.
    pub occurrence_id: OccurrenceId,
    /// Role this occurrence plays at the cursor.
    pub role: OccurrenceKind,
    /// Canonical entity the occurrence binds to.
    pub entity_id: EntityId,
    /// Kind of the bound entity.
    pub entity_kind: EntityKind,
    /// Canonical name of the bound entity. Presentation only, never identity.
    pub canonical_name: String,
    /// Anchor giving the exact source range of the occurrence.
    pub occurrence_anchor_id: AnchorId,
    /// Anchor of the entity's own declaration, when it has a source body.
    pub entity_anchor_id: Option<AnchorId>,
    /// Scope containing the occurrence, when the producer published one.
    pub scope_id: Option<ScopeId>,
    /// Producer provenance for the occurrence fact.
    pub provenance: Provenance,
    /// Producer confidence for the occurrence fact.
    pub confidence: Confidence,
    /// Producer provenance for the bound entity fact.
    pub entity_provenance: Provenance,
    /// Producer confidence for the bound entity fact.
    pub entity_confidence: Confidence,
    /// Generations this identity was resolved against.
    pub generation: ResolveGenerationBasis,
    /// Identity-affecting boundaries that remain after resolution.
    pub limitations: Vec<ResolveLimitation>,
}

impl ResolvedOccurrence {
    /// Whether this identity rests on exact producer evidence.
    ///
    /// The identity is the occurrence *and* the entity it binds, so both sides
    /// must be exact. An exact occurrence bound to a name-heuristic entity is
    /// not an exact identity.
    #[must_use]
    pub fn is_exact_evidence(&self) -> bool {
        is_exact_producer_evidence(self.provenance, self.confidence)
            && is_exact_producer_evidence(self.entity_provenance, self.entity_confidence)
    }
}

/// Whether one producer's provenance and confidence amount to exact evidence.
fn is_exact_producer_evidence(provenance: Provenance, confidence: Confidence) -> bool {
    confidence == Confidence::High
        && matches!(
            provenance,
            Provenance::ExactAst
                | Provenance::DesugaredAst
                | Provenance::SemanticAnalyzer
                | Provenance::LiteralRequireImport
        )
}

/// Outcome of resolving one cursor to an occurrence identity.
///
/// Every state is mechanically distinct. In particular `Exact` with no
/// downstream results is a different fact from `Unavailable`: the first proves
/// the cursor resolved and the answer is genuinely empty, the second proves the
/// cursor never resolved at all.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveAtOutcome {
    /// Exactly one occurrence and entity identity.
    Exact(ResolvedOccurrence),
    /// Several equally plausible identities; the caller must not pick one.
    Ambiguous(Vec<ResolvedOccurrence>),
    /// Identity depends on a dynamic boundary this layer does not resolve.
    Dynamic {
        /// Boundary that prevents an exact identity.
        boundary: ResolveLimitation,
        /// Occurrence identity of the boundary itself.
        occurrence_id: OccurrenceId,
        /// Entity the boundary occurrence bound to, when it published one.
        entity_id: Option<EntityId>,
        /// Generations this outcome was resolved against.
        generation: ResolveGenerationBasis,
    },
    /// An occurrence was found but is not a complete exact identity.
    Partial {
        /// Candidates carried forward, possibly empty.
        candidates: Vec<ResolvedOccurrence>,
        /// Why the result is not exact.
        limitations: Vec<ResolveLimitation>,
        /// Generations this outcome was resolved against.
        generation: ResolveGenerationBasis,
    },
    /// The semantic view cannot answer yet.
    NotReady(ResolveNotReady),
    /// The accepted view is stale relative to the request document.
    Stale,
    /// No occurrence identity exists at this cursor.
    Unavailable(ResolveUnavailable),
    /// The resolution instrument itself failed.
    InstrumentFailure(&'static str),
}

impl ResolveAtOutcome {
    /// Stable stage identifier for receipts, canaries, and decision traces.
    ///
    /// This is what lets the #4002 entity-resolution canaries report the shared
    /// resolution stage instead of a provider-private interpretation.
    #[must_use]
    pub fn stage(&self) -> &'static str {
        match self {
            Self::Exact(_) => "exact",
            Self::Ambiguous(_) => "ambiguous",
            Self::Dynamic { .. } => "dynamic",
            Self::Partial { .. } => "partial",
            Self::NotReady(_) => "not_ready",
            Self::Stale => "stale",
            Self::Unavailable(_) => "unavailable",
            Self::InstrumentFailure(_) => "instrument_failure",
        }
    }

    /// More specific reason within [`Self::stage`], when the state carries one.
    #[must_use]
    pub fn reason(&self) -> Option<&'static str> {
        match self {
            Self::NotReady(reason) => Some(reason.as_str()),
            Self::Unavailable(reason) => Some(reason.as_str()),
            Self::Dynamic { boundary, .. } => Some(boundary.as_str()),
            Self::InstrumentFailure(reason) => Some(reason),
            Self::Exact(_) | Self::Ambiguous(_) | Self::Partial { .. } | Self::Stale => None,
        }
    }

    /// The exact identity, when this outcome established one.
    #[must_use]
    pub fn exact(&self) -> Option<&ResolvedOccurrence> {
        match self {
            Self::Exact(resolved) => Some(resolved),
            _ => None,
        }
    }

    /// The published occurrence, whatever its exactness.
    ///
    /// Lets a caller read the occurrence's role and anchor without re-querying
    /// the semantic layer for facts this resolution already fetched.
    /// [`Self::Ambiguous`] yields `None`: there is no single occurrence to read.
    #[must_use]
    pub fn published_occurrence(&self) -> Option<&ResolvedOccurrence> {
        match self {
            Self::Exact(resolved) => Some(resolved),
            Self::Partial { candidates, .. } => candidates.first(),
            Self::Ambiguous(_)
            | Self::Dynamic { .. }
            | Self::NotReady(_)
            | Self::Stale
            | Self::Unavailable(_)
            | Self::InstrumentFailure(_) => None,
        }
    }

    /// Entity the published occurrence bound to, whatever its exactness.
    ///
    /// This is deliberately weaker than [`Self::exact`]: it reports what the
    /// producer published without asserting the identity is exact, so a caller
    /// that already accepts sub-exact evidence can keep doing so while still
    /// sharing this resolution stage. [`Self::Ambiguous`] yields `None` — a
    /// caller must never silently pick one of several plausible identities.
    #[must_use]
    pub fn bound_entity_id(&self) -> Option<EntityId> {
        match self {
            Self::Exact(resolved) => Some(resolved.entity_id),
            Self::Partial { candidates, .. } => candidates.first().map(|first| first.entity_id),
            Self::Dynamic { entity_id, .. } => *entity_id,
            Self::Ambiguous(_)
            | Self::NotReady(_)
            | Self::Stale
            | Self::Unavailable(_)
            | Self::InstrumentFailure(_) => None,
        }
    }

    /// Whether a producer published an occurrence covering the cursor.
    ///
    /// True even when the occurrence carried no entity, which is exactly the
    /// distinction the entity-resolution canaries report.
    #[must_use]
    pub fn occurrence_was_published(&self) -> bool {
        matches!(self, Self::Exact(_) | Self::Ambiguous(_) | Self::Dynamic { .. })
            || matches!(self, Self::Partial { limitations, .. }
                if limitations.contains(&ResolveLimitation::OccurrenceWithoutEntity))
            || matches!(self, Self::Partial { candidates, .. } if !candidates.is_empty())
    }

    /// Generation basis this outcome was resolved against, when it has one.
    ///
    /// States that never reached the semantic view carry no basis.
    #[must_use]
    pub fn generation(&self) -> Option<&ResolveGenerationBasis> {
        match self {
            Self::Exact(resolved) => Some(&resolved.generation),
            Self::Ambiguous(candidates) => candidates.first().map(|first| &first.generation),
            Self::Dynamic { generation, .. } | Self::Partial { generation, .. } => Some(generation),
            Self::NotReady(_) | Self::Stale | Self::Unavailable(_) | Self::InstrumentFailure(_) => {
                None
            }
        }
    }

    /// Whether two outcomes resolved the cursor against the same generations.
    ///
    /// Two outcomes that never reached the semantic view share no basis to
    /// compare, so this is false for them rather than vacuously true.
    #[must_use]
    pub fn shares_generation_with(&self, other: &Self) -> bool {
        match (self.generation(), other.generation()) {
            (Some(left), Some(right)) => left == right,
            _ => false,
        }
    }

    /// Whether both outcomes named the same occurrence and entity identity.
    #[must_use]
    pub fn shares_subject_with(&self, other: &Self) -> bool {
        match (self.exact(), other.exact()) {
            (Some(left), Some(right)) => {
                left.occurrence_id == right.occurrence_id && left.entity_id == right.entity_id
            }
            _ => false,
        }
    }
}

/// Limitations implied by an occurrence's own kind.
fn limitations_for_occurrence_kind(kind: OccurrenceKind) -> Option<ResolveLimitation> {
    match kind {
        OccurrenceKind::DynamicBoundary => Some(ResolveLimitation::DynamicSelector),
        OccurrenceKind::TypeglobReference => Some(ResolveLimitation::TypeglobMutation),
        OccurrenceKind::GeneratedUse => Some(ResolveLimitation::GeneratedWithoutSourceBody),
        _ => None,
    }
}

/// Limitations implied by the resolved entity.
fn limitations_for_entity(entity: &EntityFact) -> Option<ResolveLimitation> {
    // A generated member keeps its generator anchor and its generated identity;
    // it must never be presented as if it had an ordinary source body.
    (entity.kind == EntityKind::GeneratedMember && entity.anchor_id.is_none())
        .then_some(ResolveLimitation::GeneratedWithoutSourceBody)
}

fn build_resolved(
    entity: &EntityFact,
    occurrence: &OccurrenceFact,
    entity_id: EntityId,
    generation: &ResolveGenerationBasis,
) -> ResolvedOccurrence {
    let mut limitations = Vec::new();
    if let Some(limitation) = limitations_for_occurrence_kind(occurrence.kind) {
        limitations.push(limitation);
    }
    if let Some(limitation) = limitations_for_entity(entity) {
        limitations.push(limitation);
    }
    if !is_exact_producer_evidence(occurrence.provenance, occurrence.confidence) {
        limitations.push(ResolveLimitation::NonExactProvenance);
    }
    // The identity binds an entity too, so weak entity evidence is recorded
    // separately: a caller must be able to tell which half is approximate.
    if !is_exact_producer_evidence(entity.provenance, entity.confidence) {
        limitations.push(ResolveLimitation::NonExactEntityProvenance);
    }
    if !generation.is_known() {
        limitations.push(ResolveLimitation::UnknownGeneration);
    }
    limitations.sort_unstable();
    limitations.dedup();

    ResolvedOccurrence {
        occurrence_id: occurrence.id,
        role: occurrence.kind,
        entity_id,
        entity_kind: entity.kind,
        canonical_name: entity.canonical_name.clone(),
        occurrence_anchor_id: occurrence.anchor_id,
        entity_anchor_id: entity.anchor_id,
        // Never fall back to `entity.scope_id`: that is the entity's
        // declaration scope, not the cursor's. For an import, a package
        // member, or a shadowed lexical the two differ, and substituting one
        // for the other would report a use site the occurrence never had.
        // An unpublished occurrence scope stays absent.
        scope_id: occurrence.scope_id,
        provenance: occurrence.provenance,
        confidence: occurrence.confidence,
        entity_provenance: entity.provenance,
        entity_confidence: entity.confidence,
        generation: generation.clone(),
        limitations,
    }
}

/// Resolve one cursor to an occurrence identity against an accepted basis.
///
/// `stale` is supplied by the caller because staleness is a property of the
/// accepted view versus the open documents, which the semantic port does not
/// own. It is checked before any query so a stale view can never produce an
/// identity that later looks exact.
///
/// This function performs no name lookup. If the published occurrence carries no
/// entity, the result is [`ResolveAtOutcome::Partial`] with
/// [`ResolveLimitation::OccurrenceWithoutEntity`] — never an entity minted from
/// a matching spelling.
pub fn resolve_at_position<S>(
    source: &S,
    file_id: FileId,
    byte_offset: u32,
    generation: &ResolveGenerationBasis,
    stale: bool,
) -> ResolveAtOutcome
where
    S: ResolveAtSource + ?Sized,
{
    if stale {
        return ResolveAtOutcome::Stale;
    }

    let Some((entity, occurrence)) = source.resolve_symbol_at(file_id, byte_offset) else {
        return ResolveAtOutcome::Unavailable(ResolveUnavailable::NoOccurrenceAtPosition);
    };

    let Some(entity_id) = occurrence.entity_id else {
        return ResolveAtOutcome::Partial {
            candidates: Vec::new(),
            limitations: vec![ResolveLimitation::OccurrenceWithoutEntity],
            generation: generation.clone(),
        };
    };

    // The identity takes `entity_id` from the occurrence but every other field
    // from the entity, so the two must describe the same entity. `ResolveAtSource`
    // is a public trait: an implementation that returns a mismatched pair would
    // otherwise yield an "exact" hybrid — one entity's id carrying another's
    // kind, name, anchor, and evidence. That is the conflation this layer exists
    // to prevent, so it is refused as an instrument failure rather than
    // resolved. The workspace adapter cannot produce it; a future source could.
    if entity.id != entity_id {
        return ResolveAtOutcome::InstrumentFailure("source_entity_occurrence_mismatch");
    }

    // Checked before building the resolved occurrence: this path discards it,
    // and building it clones the canonical name and allocates the limitation
    // vector.
    if occurrence.kind == OccurrenceKind::DynamicBoundary {
        return ResolveAtOutcome::Dynamic {
            boundary: ResolveLimitation::DynamicSelector,
            occurrence_id: occurrence.id,
            entity_id: Some(entity_id),
            generation: generation.clone(),
        };
    }

    let resolved = build_resolved(&entity, &occurrence, entity_id, generation);

    // An identity is only exact if its snapshot is identified. Without a known
    // basis there is nothing to say *which* source the identity came from, and
    // a caller comparing bases could not detect drift — so strong occurrence
    // evidence over an unknown generation stays `Partial`.
    if resolved.is_exact_evidence() && generation.is_known() {
        ResolveAtOutcome::Exact(resolved)
    } else {
        let limitations = resolved.limitations.clone();
        ResolveAtOutcome::Partial {
            candidates: vec![resolved],
            limitations,
            generation: generation.clone(),
        }
    }
}

/// [`resolve_at_position`], additionally consulting the dynamic-boundary
/// producer when no occurrence is published at the cursor.
///
/// Kept separate rather than folded into the base rule because it asks the
/// semantic layer a second question. A caller whose receipts distinguish "an
/// occurrence was published" from "a boundary covers this position" must opt in
/// deliberately; the base rule stays exactly one query.
pub fn resolve_at_position_with_dynamic_boundary<S>(
    source: &S,
    file_id: FileId,
    byte_offset: u32,
    generation: &ResolveGenerationBasis,
    stale: bool,
) -> ResolveAtOutcome
where
    S: ResolveAtSource + ?Sized,
{
    let outcome = resolve_at_position(source, file_id, byte_offset, generation, stale);
    if !matches!(outcome, ResolveAtOutcome::Unavailable(ResolveUnavailable::NoOccurrenceAtPosition))
    {
        return outcome;
    }

    // A boundary covering this position is a different fact from "nothing is
    // here" and must stay visible rather than collapsing into unavailable.
    match source.resolve_dynamic_boundary_at(file_id, byte_offset) {
        Some(boundary) => ResolveAtOutcome::Dynamic {
            boundary: ResolveLimitation::DynamicSelector,
            occurrence_id: boundary.id,
            entity_id: boundary.entity_id,
            generation: generation.clone(),
        },
        None => outcome,
    }
}

#[cfg(test)]
mod tests;
