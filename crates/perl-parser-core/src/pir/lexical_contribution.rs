//! File-level compiler lexical contribution contract (PIRL-01, #12109).
//!
//! One immutable versioned model for compiler-produced initialized-lexical
//! facts per accepted document generation. This is the durable object the
//! builder successor (#9284) will construct and the runtime attachment cell
//! and provider port (#8669) will consume.
//!
//! This module defines types, strict constructors/validators, canonical
//! serialization, and a deterministic fingerprint only. It performs no
//! HIR/PIR lowering, owns no async cell, attaches nothing to live document
//! state, and changes no provider result.
//!
//! # Identity law (#12109)
//!
//! A current contribution binds *all* of its subject identities at once:
//! exact full-source digest, parser-input digest, accepted parse generation,
//! canonical body-HIR identity, and compiler implementation/profile/producer.
//! Equal URI text, display names, ranges, or fact counts never repair a mixed
//! subject; there is deliberately no URI field to key currentness on.
//!
//! The optional [`SemanticSnapshotJoinMetadata`] records that a matching
//! semantic snapshot exists; it never participates in construction,
//! validation of the compiler subject, or completeness upgrades.
//!
//! # Anchor authority boundary (#12191)
//!
//! [`OccurrenceAnchor`] is a mechanically-derived projection of the canonical
//! #12191 [`PirSourceAnchor`]: it carries the typed [`PirAnchorKind`]
//! provenance class, the exact byte range, the canonical [`AnchorId`], and the
//! lowering `hir_item` index of the anchor it was derived from. It is not a
//! fallback anchor/range vocabulary: construction rejects any anchor whose
//! identity cannot be traced back to the canonical derivation.
//!
//! # Interim binding/role vocabulary boundary (#2660)
//!
//! [`LexicalSigil`], [`OccurrenceRole`], and [`LexicalBindingIdentity`] are an
//! interim representation vocabulary only. Binding-identity and
//! `Declaration | Read | Write | Modify` role authority belongs to #2660,
//! which is still open; when #2660 lands its canonical types this envelope
//! converges onto them with a schema-version bump. The in-crate
//! `pir::extractor` `LexicalRole`/`LexicalBindingFact` are PR1 read/write
//! extraction receipts — they deliberately skip `Modify` and carry no
//! declaration role — so they are not the binding/role authority and this
//! contract does not converge on them.

use std::collections::{BTreeMap, BTreeSet};

use perl_semantic_facts::AnchorId;
use perl_source_identity::ContentDigest;
use serde::Serialize;

use super::model::{PirAnchorKind, PirSourceAnchor};
/// Current schema version for [`FilePirLexicalContributionV1`].
pub const FILE_PIR_LEXICAL_CONTRIBUTION_SCHEMA_VERSION: u32 = 1;

/// Compiler-side provenance for one contribution.
///
/// Descriptive only: producer naming never influences validation outcomes or
/// completeness decisions (producer-name upgrades are structurally impossible).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CompilerProducerIdentity {
    /// Compiler implementation identity (name + version).
    pub implementation: String,
    /// PIR profile used for lowering (e.g. `pir-v0`).
    pub pir_profile: String,
    /// Producer stage that emitted this contribution.
    pub producer: String,
}

/// Subject identity binding this contribution to one exact document instance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContributionSubjectIdentity {
    /// Digest over the full source text of the document instance.
    pub full_source_digest: ContentDigest,
    /// Digest over the exact parser input (may differ from full source when
    /// the parser consumed a bounded projection).
    pub parser_input_digest: ContentDigest,
    /// Accepted parse generation this contribution was built from.
    pub accepted_generation: u64,
    /// Canonical body-HIR identity (digest over the lowered body-HIR subject).
    pub body_hir_identity: ContentDigest,
}

/// Sigil / namespace slot of a lexical variable (interim #2660 vocabulary;
/// converges onto the canonical #2660 types when they land).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum LexicalSigil {
    /// Scalar `$x`.
    Scalar,
    /// Array `@x`.
    Array,
    /// Hash `%x`.
    Hash,
    /// Code `&x` / `my sub` lexical.
    Code,
}

/// Role of one lexical occurrence (interim #2660 vocabulary; converges onto
/// the canonical #2660 types when they land). All four remain distinct;
/// `Modify` is never folded into `Write`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum OccurrenceRole {
    /// Binding introduction site.
    Declaration,
    /// Value read.
    Read,
    /// Plain value write.
    Write,
    /// Compound read-modify-write (`+=`, `++`, ...).
    Modify,
}

/// Stable lexical binding identity (interim #2660 vocabulary; converges onto
/// the canonical #2660 types when they land): body + scope + sigil + name.
///
/// The same display name in another body/scope/sigil is a different binding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LexicalBindingIdentity {
    /// Stable binding id (unique within one contribution).
    pub binding_id: String,
    /// Owning body identity.
    pub body_id: String,
    /// Innermost-to-outermost lexical scope path within the body.
    pub scope_path: Vec<String>,
    /// Sigil / namespace slot.
    pub sigil: LexicalSigil,
    /// Variable name without sigil.
    pub name: String,
    /// Exact declaration range (start/end byte offsets in the document).
    pub declaration_range: (usize, usize),
    /// Deterministic binding fingerprint, derived from the identity fields by
    /// [`LexicalBindingIdentity::fingerprint_for`].
    ///
    /// [`FilePirLexicalContributionV1::try_new`] re-derives it and rejects any
    /// binding whose fingerprint does not match its identity fields, so a
    /// stale or colliding caller-supplied fingerprint can never be attested.
    pub fingerprint: ContentDigest,
}

/// Canonical serialization view of the binding-identity fields the binding
/// fingerprint covers.
#[derive(Serialize)]
struct BindingIdentityView<'a> {
    body_id: &'a str,
    scope_path: &'a [String],
    sigil: LexicalSigil,
    name: &'a str,
    declaration_range: (usize, usize),
}

impl LexicalBindingIdentity {
    /// Derive the deterministic binding fingerprint from the identity fields
    /// `(body_id, scope_path, sigil, name, declaration_range)`.
    ///
    /// The fingerprint hashes the canonical JSON of exactly those fields, so
    /// mutating any one of them changes the fingerprint.
    pub fn fingerprint_for(
        body_id: &str,
        scope_path: &[String],
        sigil: LexicalSigil,
        name: &str,
        declaration_range: (usize, usize),
    ) -> Result<ContentDigest, ContributionError> {
        let view = BindingIdentityView { body_id, scope_path, sigil, name, declaration_range };
        let canonical = serde_json::to_string(&view)
            .map_err(|error| ContributionError::Serialization { message: error.to_string() })?;
        Ok(ContentDigest::of_bytes(canonical.as_bytes()))
    }

    /// Construct one binding identity with its fingerprint derived from the
    /// identity fields.
    pub fn new(
        binding_id: String,
        body_id: String,
        scope_path: Vec<String>,
        sigil: LexicalSigil,
        name: String,
        declaration_range: (usize, usize),
    ) -> Result<Self, ContributionError> {
        let fingerprint =
            Self::fingerprint_for(&body_id, &scope_path, sigil, &name, declaration_range)?;
        Ok(Self { binding_id, body_id, scope_path, sigil, name, declaration_range, fingerprint })
    }

    /// Re-derive this binding's fingerprint from its identity fields.
    fn derived_fingerprint(&self) -> Result<ContentDigest, ContributionError> {
        Self::fingerprint_for(
            &self.body_id,
            &self.scope_path,
            self.sigil,
            &self.name,
            self.declaration_range,
        )
    }
}

/// Mechanically-derived projection of the canonical #12191 [`PirSourceAnchor`]
/// for one occurrence.
///
/// Every field is copied from the canonical anchor: the typed provenance
/// class, the exact byte range, the canonical [`AnchorId`], and the lowering
/// `hir_item` index. No foreign AST/HIR node type enters the fact model — only
/// the stable identities travel with the fact — and no free-form anchor
/// vocabulary can be attested: [`FilePirLexicalContributionV1::try_new`]
/// rejects anchors whose `anchor_id` does not match the canonical derivation
/// for their range, and non-source-backed provenance classes cannot
/// structurally pass as source-backed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OccurrenceAnchor {
    /// Provenance class, copied from the canonical anchor's
    /// [`PirSourceAnchor::kind`].
    pub anchor_kind: PirAnchorKind,
    /// Exact byte range (start inclusive, end exclusive), copied from the
    /// canonical anchor's range.
    pub range: (usize, usize),
    /// Canonical #12191 anchor identity, copied from the canonical anchor's
    /// [`PirSourceAnchor::anchor_id`].
    pub anchor_id: AnchorId,
    /// Lowering provenance: index of the HIR item the canonical anchor lowered
    /// from ([`PirSourceAnchor::hir_item`]), when it carries one.
    pub hir_item_index: Option<u32>,
}

impl OccurrenceAnchor {
    /// Snapshot one source-backed PIR anchor.
    ///
    /// Returns `None` when the anchor carries no concrete range, no canonical
    /// anchor id, or its provenance is not source-backed; such occurrences are
    /// rejected.
    #[must_use]
    pub fn from_pir_anchor(anchor: &PirSourceAnchor) -> Option<Self> {
        let range = anchor.range?;
        if !anchor.kind.is_source_backed() {
            return None;
        }
        let anchor_id = anchor.anchor_id?;
        Some(Self {
            anchor_kind: anchor.kind,
            range: (range.start, range.end),
            anchor_id,
            hir_item_index: anchor.hir_item.map(|hir_item| hir_item.index()),
        })
    }
}

/// One anchored occurrence of a binding in the contribution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContributionOccurrence {
    /// Stable occurrence id (unique within one contribution).
    pub occurrence_id: String,
    /// Referenced binding (must exist in the same contribution).
    pub binding_id: String,
    /// Exact role of this occurrence.
    pub role: OccurrenceRole,
    /// Source-backed anchor with a concrete byte range.
    pub anchor: OccurrenceAnchor,
    /// PIR operation provenance (stable operation name).
    pub operation_provenance: String,
}

/// Result/completeness axis (#12109). Missing, unsupported, partial, stale,
/// cancelled, budget-exhausted, or instrument-failed contributions are never
/// exact-empty: see [`FilePirLexicalContributionV1::is_exact`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub enum ContributionCompleteness {
    /// All lexical facts for the subject were produced.
    Complete,
    /// Some facts produced; limitations say which classes are absent.
    Partial,
    /// No facts could be produced for the subject.
    Unavailable,
    /// Superseded by a newer accepted generation before use.
    StaleOrSuperseded,
    /// Construction was cancelled.
    Cancelled,
    /// Construction hit its work budget.
    BudgetExhausted,
    /// An instrument failed mid-construction.
    InstrumentFailure,
    /// The subject itself was invalid (never lowerable).
    InvalidSubject,
}

/// Limitation classes bounding what a partial contribution omits (#12109).
///
/// Ordered by declaration order so limitation sets canonicalize to one
/// sequence regardless of producer discovery order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[non_exhaustive]
pub enum ContributionLimitation {
    /// Body recovered through error recovery; facts may be incomplete.
    RecoveredBody,
    /// Dynamic operation boundary blocked static classification.
    DynamicOperation,
    /// Alias/localize unsupported.
    UnsupportedAliasOrLocalize,
    /// Destructuring declaration unsupported.
    UnsupportedDestructuring,
    /// Place role unsupported for this access.
    UnsupportedPlaceRole,
    /// Occurrence without a usable source anchor.
    MissingAnchor,
    /// Verifier rejected part of the lowering.
    VerifierFailure,
}

/// One observed work quantity. Unknown or non-applicable quantities are
/// distinct from zero — they never default to a numeric value (#12109).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum WorkObservation {
    /// The builder observed this exact count.
    Observed(u64),
    /// The builder could not observe this quantity.
    Unavailable,
    /// This quantity does not apply to this construction.
    NotApplicable,
}

/// Work-receipt shape retained by the contribution (#12109).
///
/// This is the capability shape only; the builder successor owns real values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContributionWorkShape {
    /// Body-HIR inputs consumed.
    pub body_hir_inputs_consumed: WorkObservation,
    /// PIR bodies lowered.
    pub pir_bodies_lowered: WorkObservation,
    /// Verifier work performed.
    pub verifier_work: WorkObservation,
    /// Lexical operations visited.
    pub lexical_operations_visited: WorkObservation,
    /// Anchors accepted.
    pub anchors_accepted: WorkObservation,
    /// Anchors rejected (loss indicator).
    pub anchors_rejected: WorkObservation,
    /// Unsupported or dynamic operations encountered (loss indicator).
    pub unsupported_or_dynamic_operations: WorkObservation,
    /// Whether this construction was a new build or a shared cache hit.
    pub build_kind: BuildKind,
}

/// New-build versus shared-hit state for the construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum BuildKind {
    /// Constructed fresh from inputs.
    NewBuild,
    /// Reused an existing shared construction result.
    SharedHit,
    /// Build kind not observable.
    Unobservable,
}

/// Terminal disposition of this contribution record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum TerminalDisposition {
    /// Final record for its subject generation.
    Committed,
    /// Superseded by another contribution.
    SupersededBy {
        /// Fingerprint of the superseding contribution.
        successor_fingerprint: ContentDigest,
    },
    /// Withdrawn before use, with reason.
    Withdrawn {
        /// Why this record was withdrawn.
        reason: String,
    },
}

/// Optional join metadata for a matching `FileSemanticSnapshot`.
///
/// Present only as descriptive join information. It is not required to
/// construct the contribution, and semantic exactness never upgrades
/// compiler completeness (#12109).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SemanticSnapshotJoinMetadata {
    /// Snapshot content digest.
    pub snapshot_digest: ContentDigest,
    /// Snapshot's parse generation; must equal the subject's when joined.
    pub generation: u64,
    /// Snapshot's parser-input digest; must equal the subject's when joined.
    pub parser_input_digest: ContentDigest,
}

/// Validation errors for strict contribution construction (#12109).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ContributionError {
    /// Schema version other than [`FILE_PIR_LEXICAL_CONTRIBUTION_SCHEMA_VERSION`].
    #[error("unsupported schema version {found}: expected {expected}")]
    SchemaVersion {
        /// The rejected schema version.
        found: u32,
        /// The only supported schema version.
        expected: u32,
    },
    /// A required identity string was empty.
    #[error("empty required identity field: {field}")]
    EmptyIdentityField {
        /// The offending field name.
        field: &'static str,
    },
    /// Two bindings shared one `binding_id`.
    #[error("duplicate binding id {binding_id}")]
    DuplicateBindingId {
        /// The duplicated id.
        binding_id: String,
    },
    /// Two bindings collapsed onto one (body, scope, sigil, name) identity.
    #[error("bindings {first} and {second} collapse onto one body/scope/sigil/name identity")]
    CollapsedBindingIdentity {
        /// First colliding binding id.
        first: String,
        /// Second colliding binding id.
        second: String,
    },
    /// Two occurrences shared one `occurrence_id`.
    #[error("duplicate occurrence id {occurrence_id}")]
    DuplicateOccurrenceId {
        /// The duplicated id.
        occurrence_id: String,
    },
    /// An occurrence referenced an unknown binding.
    #[error("occurrence {occurrence_id} references unknown binding {binding_id}")]
    UnknownBindingReference {
        /// The orphaned occurrence.
        occurrence_id: String,
        /// The missing binding.
        binding_id: String,
    },
    /// A Declaration occurrence did not sit on its binding's declared range.
    #[error(
        "declaration occurrence {occurrence_id} does not match binding {binding_id}'s \
         declared range"
    )]
    InvalidDeclarationAnchor {
        /// The mis-anchored occurrence.
        occurrence_id: String,
        /// The binding whose declared range was contradicted.
        binding_id: String,
    },
    /// A complete contribution left a loss indicator observed or unavailable.
    #[error("completeness Complete contradicts work observation {field}")]
    IncompleteButClaimedComplete {
        /// The contradicting field.
        field: &'static str,
    },
    /// A complete contribution carried limitations.
    #[error("completeness Complete contradicts limitation {limitation:?}")]
    CompleteWithLimitations {
        /// The first contradicting limitation.
        limitation: ContributionLimitation,
    },
    /// Join metadata came from another subject generation or input.
    #[error("semantic snapshot join metadata does not match the contribution subject")]
    ForeignSemanticJoin,
    /// A non-source-backed anchor cannot carry an occurrence.
    #[error("occurrence {occurrence_id} has a non-source-backed anchor")]
    UnanchoredOccurrence {
        /// The unanchored occurrence.
        occurrence_id: String,
    },
    /// A binding's fingerprint did not match its identity fields.
    #[error("binding {binding_id} carries a fingerprint that does not match its identity fields")]
    BindingFingerprintMismatch {
        /// The binding whose fingerprint is stale or foreign.
        binding_id: String,
    },
    /// An occurrence anchor's id did not match the canonical #12191
    /// derivation for its range.
    #[error("occurrence {occurrence_id} carries an anchor id inconsistent with its range")]
    InconsistentAnchorIdentity {
        /// The occurrence whose anchor identity cannot be traced back to the
        /// canonical anchor derivation.
        occurrence_id: String,
    },
    /// Canonical serialization failed.
    #[error("canonical serialization failed: {message}")]
    Serialization {
        /// Underlying serializer message.
        message: String,
    },
}

/// Canonical sort keys used for deterministic output under input-order
/// variation: bindings by id, occurrences by (id, binding), limitations by
/// their total enum order with duplicate classes collapsed.
fn canonicalize(
    mut bindings: Vec<LexicalBindingIdentity>,
    mut occurrences: Vec<ContributionOccurrence>,
    mut limitations: Vec<ContributionLimitation>,
) -> (Vec<LexicalBindingIdentity>, Vec<ContributionOccurrence>, Vec<ContributionLimitation>) {
    bindings.sort_by(|a, b| a.binding_id.cmp(&b.binding_id));
    occurrences.sort_by(|a, b| {
        a.occurrence_id.cmp(&b.occurrence_id).then_with(|| a.binding_id.cmp(&b.binding_id))
    });
    limitations.sort();
    limitations.dedup();
    (bindings, occurrences, limitations)
}

/// One immutable file-level compiler lexical contribution (#12109).
///
/// Construct exclusively through [`FilePirLexicalContributionV1::try_new`];
/// every field is validated before the fingerprint is computed. The validated
/// fields are private and read-only through accessors, so no caller can
/// bypass validation or mutate a record after construction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FilePirLexicalContributionV1 {
    /// Schema version (pinned).
    schema_version: u32,
    /// Producer provenance (descriptive).
    producer: CompilerProducerIdentity,
    /// Exact subject identity (load-bearing).
    subject: ContributionSubjectIdentity,
    /// Bindings, canonically ordered.
    bindings: Vec<LexicalBindingIdentity>,
    /// Occurrences, canonically ordered.
    occurrences: Vec<ContributionOccurrence>,
    /// Result/completeness axis.
    completeness: ContributionCompleteness,
    /// Limitations bounding absent fact classes, canonically ordered.
    limitations: Vec<ContributionLimitation>,
    /// Work-shape observations.
    work: ContributionWorkShape,
    /// Terminal disposition of this record.
    terminal_disposition: TerminalDisposition,
    /// Optional matching semantic-snapshot join metadata.
    semantic_snapshot_join: Option<SemanticSnapshotJoinMetadata>,
    /// Deterministic fingerprint over the unsigned canonical serialization.
    fingerprint: ContentDigest,
}

/// Canonical serialization view of a contribution with the `fingerprint`
/// field omitted.
///
/// The stored [`FilePirLexicalContributionV1::fingerprint`] hashes exactly
/// this view's serialization bytes; consumers recompute them from any durable
/// envelope copy through
/// [`FilePirLexicalContributionV1::unsigned_canonical_json`].
#[derive(Serialize)]
struct UnsignedContributionView<'a> {
    schema_version: u32,
    producer: &'a CompilerProducerIdentity,
    subject: &'a ContributionSubjectIdentity,
    bindings: &'a [LexicalBindingIdentity],
    occurrences: &'a [ContributionOccurrence],
    completeness: ContributionCompleteness,
    limitations: &'a [ContributionLimitation],
    work: &'a ContributionWorkShape,
    terminal_disposition: &'a TerminalDisposition,
    semantic_snapshot_join: Option<&'a SemanticSnapshotJoinMetadata>,
}

/// Unvalidated construction input for [`FilePirLexicalContributionV1::try_new`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContributionDraft {
    /// Producer provenance (descriptive).
    pub producer: CompilerProducerIdentity,
    /// Exact subject identity (load-bearing).
    pub subject: ContributionSubjectIdentity,
    /// Bindings to validate and canonically order.
    pub bindings: Vec<LexicalBindingIdentity>,
    /// Occurrences to validate and canonically order.
    pub occurrences: Vec<ContributionOccurrence>,
    /// Claimed result/completeness axis.
    pub completeness: ContributionCompleteness,
    /// Limitations bounding absent fact classes.
    pub limitations: Vec<ContributionLimitation>,
    /// Work-shape observations.
    pub work: ContributionWorkShape,
    /// Terminal disposition of this record.
    pub terminal_disposition: TerminalDisposition,
    /// Optional matching semantic-snapshot join metadata.
    pub semantic_snapshot_join: Option<SemanticSnapshotJoinMetadata>,
}

impl FilePirLexicalContributionV1 {
    /// Strictly validate and construct one contribution.
    ///
    /// Fails closed on mixed subjects, collapsed bindings, duplicated binding
    /// ids, stale binding fingerprints, mislabeled declarations,
    /// non-source-backed occurrence anchors, anchor identities that do not
    /// trace back to the canonical #12191 derivation, incomplete-but-complete
    /// claims, foreign joins, and unanchored occurrences. The returned record
    /// carries a deterministic fingerprint computed over the unsigned
    /// canonical serialization (every field in fixed order except the
    /// fingerprint itself).
    pub fn try_new(draft: ContributionDraft) -> Result<Self, ContributionError> {
        let ContributionDraft {
            producer,
            subject,
            bindings,
            occurrences,
            completeness,
            limitations,
            work,
            terminal_disposition,
            semantic_snapshot_join,
        } = draft;
        if producer.implementation.is_empty() {
            return Err(ContributionError::EmptyIdentityField { field: "implementation" });
        }
        if producer.pir_profile.is_empty() {
            return Err(ContributionError::EmptyIdentityField { field: "pir_profile" });
        }
        if producer.producer.is_empty() {
            return Err(ContributionError::EmptyIdentityField { field: "producer" });
        }
        if let Some(join) = &semantic_snapshot_join
            && (join.generation != subject.accepted_generation
                || join.parser_input_digest != subject.parser_input_digest)
        {
            return Err(ContributionError::ForeignSemanticJoin);
        }

        let mut seen_binding_ids = BTreeSet::new();
        let mut seen_bindings = BTreeMap::new();
        for binding in &bindings {
            if binding.binding_id.is_empty() {
                return Err(ContributionError::EmptyIdentityField { field: "binding_id" });
            }
            if !seen_binding_ids.insert(binding.binding_id.as_str()) {
                return Err(ContributionError::DuplicateBindingId {
                    binding_id: binding.binding_id.clone(),
                });
            }
            let key = (
                binding.body_id.clone(),
                binding.scope_path.clone(),
                binding.sigil,
                binding.name.clone(),
            );
            if let Some(first) = seen_bindings.insert(key.clone(), binding.binding_id.as_str()) {
                return Err(ContributionError::CollapsedBindingIdentity {
                    first: first.to_string(),
                    second: binding.binding_id.clone(),
                });
            }
            if binding.derived_fingerprint()? != binding.fingerprint {
                return Err(ContributionError::BindingFingerprintMismatch {
                    binding_id: binding.binding_id.clone(),
                });
            }
        }

        let binding_by_id: BTreeMap<&str, &LexicalBindingIdentity> =
            bindings.iter().map(|b| (b.binding_id.as_str(), b)).collect();

        let mut seen_occurrences = BTreeMap::new();
        for occurrence in &occurrences {
            if occurrence.occurrence_id.is_empty() {
                return Err(ContributionError::EmptyIdentityField { field: "occurrence_id" });
            }
            if seen_occurrences.insert(occurrence.occurrence_id.as_str(), ()).is_some() {
                return Err(ContributionError::DuplicateOccurrenceId {
                    occurrence_id: occurrence.occurrence_id.clone(),
                });
            }
            if !occurrence.anchor.anchor_kind.is_source_backed() {
                return Err(ContributionError::UnanchoredOccurrence {
                    occurrence_id: occurrence.occurrence_id.clone(),
                });
            }
            if occurrence.anchor.anchor_id != AnchorId(occurrence.anchor.range.0 as u64) {
                return Err(ContributionError::InconsistentAnchorIdentity {
                    occurrence_id: occurrence.occurrence_id.clone(),
                });
            }
            let binding = binding_by_id.get(occurrence.binding_id.as_str()).ok_or_else(|| {
                ContributionError::UnknownBindingReference {
                    occurrence_id: occurrence.occurrence_id.clone(),
                    binding_id: occurrence.binding_id.clone(),
                }
            })?;
            if occurrence.role == OccurrenceRole::Declaration
                && occurrence.anchor.range != binding.declaration_range
            {
                return Err(ContributionError::InvalidDeclarationAnchor {
                    occurrence_id: occurrence.occurrence_id.clone(),
                    binding_id: occurrence.binding_id.clone(),
                });
            }
        }

        if completeness == ContributionCompleteness::Complete {
            if let Some(limitation) = limitations.first() {
                return Err(ContributionError::CompleteWithLimitations { limitation: *limitation });
            }
            let losses: [(&'static str, WorkObservation); 2] = [
                ("anchors_rejected", work.anchors_rejected),
                ("unsupported_or_dynamic_operations", work.unsupported_or_dynamic_operations),
            ];
            for (field, observation) in losses {
                match observation {
                    WorkObservation::Observed(0) | WorkObservation::NotApplicable => {}
                    _ => {
                        return Err(ContributionError::IncompleteButClaimedComplete { field });
                    }
                }
            }
            for binding in &bindings {
                let has_valid_declaration = occurrences.iter().any(|o| {
                    o.binding_id == binding.binding_id
                        && o.role == OccurrenceRole::Declaration
                        && o.anchor.range == binding.declaration_range
                });
                if !has_valid_declaration {
                    return Err(ContributionError::InvalidDeclarationAnchor {
                        occurrence_id: String::from("<missing>"),
                        binding_id: binding.binding_id.clone(),
                    });
                }
            }
        }

        let (bindings, occurrences, limitations) = canonicalize(bindings, occurrences, limitations);
        let unsigned = UnsignedContributionView {
            schema_version: FILE_PIR_LEXICAL_CONTRIBUTION_SCHEMA_VERSION,
            producer: &producer,
            subject: &subject,
            bindings: &bindings,
            occurrences: &occurrences,
            completeness,
            limitations: &limitations,
            work: &work,
            terminal_disposition: &terminal_disposition,
            semantic_snapshot_join: semantic_snapshot_join.as_ref(),
        };
        let canonical = serde_json::to_string(&unsigned)
            .map_err(|error| ContributionError::Serialization { message: error.to_string() })?;
        Ok(Self {
            schema_version: FILE_PIR_LEXICAL_CONTRIBUTION_SCHEMA_VERSION,
            producer,
            subject,
            bindings,
            occurrences,
            completeness,
            limitations,
            work,
            terminal_disposition,
            semantic_snapshot_join,
            fingerprint: ContentDigest::of_bytes(canonical.as_bytes()),
        })
    }

    /// Schema version (pinned).
    #[must_use]
    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    /// Producer provenance (descriptive).
    #[must_use]
    pub const fn producer(&self) -> &CompilerProducerIdentity {
        &self.producer
    }

    /// Exact subject identity (load-bearing).
    #[must_use]
    pub const fn subject(&self) -> &ContributionSubjectIdentity {
        &self.subject
    }

    /// Bindings, canonically ordered.
    #[must_use]
    pub fn bindings(&self) -> &[LexicalBindingIdentity] {
        &self.bindings
    }

    /// Occurrences, canonically ordered.
    #[must_use]
    pub fn occurrences(&self) -> &[ContributionOccurrence] {
        &self.occurrences
    }

    /// Result/completeness axis.
    #[must_use]
    pub const fn completeness(&self) -> ContributionCompleteness {
        self.completeness
    }

    /// Limitations bounding absent fact classes, canonically ordered and
    /// deduplicated.
    #[must_use]
    pub fn limitations(&self) -> &[ContributionLimitation] {
        &self.limitations
    }

    /// Work-shape observations.
    #[must_use]
    pub const fn work(&self) -> &ContributionWorkShape {
        &self.work
    }

    /// Terminal disposition of this record.
    #[must_use]
    pub const fn terminal_disposition(&self) -> &TerminalDisposition {
        &self.terminal_disposition
    }

    /// Optional matching semantic-snapshot join metadata.
    #[must_use]
    pub const fn semantic_snapshot_join(&self) -> Option<&SemanticSnapshotJoinMetadata> {
        self.semantic_snapshot_join.as_ref()
    }

    /// Deterministic fingerprint over the unsigned canonical serialization
    /// ([`Self::unsigned_canonical_json`]).
    #[must_use]
    pub const fn fingerprint(&self) -> &ContentDigest {
        &self.fingerprint
    }

    /// Canonical JSON serialization: collections are pre-sorted, struct field
    /// order is fixed by the derive, and map keys are BTreeMap-ordered.
    pub fn canonical_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Canonical JSON of the unsigned fingerprint input: every envelope field
    /// in fixed structural order with the `fingerprint` field omitted.
    ///
    /// The stored [`Self::fingerprint`] covers exactly these bytes, so a
    /// verifier recomputes it as
    /// `ContentDigest::of_bytes(unsigned_canonical_json()?.as_bytes())`.
    pub fn unsigned_canonical_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(&UnsignedContributionView {
            schema_version: self.schema_version,
            producer: &self.producer,
            subject: &self.subject,
            bindings: self.bindings.as_slice(),
            occurrences: self.occurrences.as_slice(),
            completeness: self.completeness,
            limitations: self.limitations.as_slice(),
            work: &self.work,
            terminal_disposition: &self.terminal_disposition,
            semantic_snapshot_join: self.semantic_snapshot_join.as_ref(),
        })
    }

    /// Whether this contribution proves an exact answer.
    ///
    /// Only [`ContributionCompleteness::Complete`] records whose terminal
    /// disposition is [`TerminalDisposition::Committed`] qualify: partial,
    /// unavailable, stale, cancelled, budget-exhausted, instrument-failed,
    /// and invalid-subject records are never exact-empty, and a superseded or
    /// withdrawn record never authorizes an exact answer no matter what its
    /// completeness axis claims.
    #[must_use]
    pub fn is_exact(&self) -> bool {
        self.completeness == ContributionCompleteness::Complete
            && self.terminal_disposition == TerminalDisposition::Committed
    }
}
