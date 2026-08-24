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

use std::collections::BTreeMap;

use perl_source_identity::ContentDigest;
use serde::Serialize;

use super::model::PirSourceAnchor;
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

/// Sigil / namespace slot of a lexical variable (#2660).
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

/// Role of one lexical occurrence (#2660). All four remain distinct;
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

/// Stable lexical binding identity (#2660): body + scope + sigil + name.
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
    /// Deterministic binding fingerprint.
    pub fingerprint: ContentDigest,
}

/// Self-contained source anchor snapshot for one occurrence.
///
/// The contribution model deliberately does not embed foreign AST/HIR/PIR
/// node types: only the stable provenance class name and the exact byte
/// range travel with the fact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OccurrenceAnchor {
    /// Stable [`PirAnchorKind`] name (provenance class).
    pub anchor_kind: String,
    /// Exact byte range (start inclusive, end exclusive).
    pub range: (usize, usize),
}

impl OccurrenceAnchor {
    /// Snapshot one source-backed PIR anchor.
    ///
    /// Returns `None` when the anchor carries no concrete range or its
    /// provenance is not source-backed; such occurrences are rejected.
    #[must_use]
    pub fn from_pir_anchor(anchor: &PirSourceAnchor) -> Option<Self> {
        let range = anchor.range?;
        if !anchor.kind.is_source_backed() {
            return None;
        }
        Some(Self { anchor_kind: anchor.kind.name().to_string(), range: (range.start, range.end) })
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
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
    /// Canonical serialization failed.
    #[error("canonical serialization failed: {message}")]
    Serialization {
        /// Underlying serializer message.
        message: String,
    },
}

/// Canonical sort keys used for deterministic output under input-order
/// variation: bindings by id, occurrences by (id, binding).
fn canonicalize(
    mut bindings: Vec<LexicalBindingIdentity>,
    mut occurrences: Vec<ContributionOccurrence>,
) -> (Vec<LexicalBindingIdentity>, Vec<ContributionOccurrence>) {
    bindings.sort_by(|a, b| a.binding_id.cmp(&b.binding_id));
    occurrences.sort_by(|a, b| {
        a.occurrence_id.cmp(&b.occurrence_id).then_with(|| a.binding_id.cmp(&b.binding_id))
    });
    (bindings, occurrences)
}

/// One immutable file-level compiler lexical contribution (#12109).
///
/// Construct exclusively through [`FilePirLexicalContributionV1::try_new`];
/// every field is validated before the fingerprint is computed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FilePirLexicalContributionV1 {
    /// Schema version (pinned).
    pub schema_version: u32,
    /// Producer provenance (descriptive).
    pub producer: CompilerProducerIdentity,
    /// Exact subject identity (load-bearing).
    pub subject: ContributionSubjectIdentity,
    /// Bindings, canonically ordered.
    pub bindings: Vec<LexicalBindingIdentity>,
    /// Occurrences, canonically ordered.
    pub occurrences: Vec<ContributionOccurrence>,
    /// Result/completeness axis.
    pub completeness: ContributionCompleteness,
    /// Limitations bounding absent fact classes.
    pub limitations: Vec<ContributionLimitation>,
    /// Work-shape observations.
    pub work: ContributionWorkShape,
    /// Terminal disposition of this record.
    pub terminal_disposition: TerminalDisposition,
    /// Optional matching semantic-snapshot join metadata.
    pub semantic_snapshot_join: Option<SemanticSnapshotJoinMetadata>,
    /// Deterministic fingerprint over the canonical serialization.
    pub fingerprint: ContentDigest,
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
    /// Fails closed on mixed subjects, collapsed bindings, mislabeled
    /// declarations, incomplete-but-complete claims, foreign joins, and
    /// unanchored occurrences. The returned record carries a deterministic
    /// fingerprint computed over the canonical serialization.
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
        if let Some(join) = &semantic_snapshot_join {
            if join.generation != subject.accepted_generation
                || join.parser_input_digest != subject.parser_input_digest
            {
                return Err(ContributionError::ForeignSemanticJoin);
            }
        }

        let mut seen_bindings = BTreeMap::new();
        for binding in &bindings {
            if binding.binding_id.is_empty() {
                return Err(ContributionError::EmptyIdentityField { field: "binding_id" });
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

        let (bindings, occurrences) = canonicalize(bindings, occurrences);
        let mut contribution = Self {
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
            fingerprint: ContentDigest::of_bytes(b""),
        };
        let canonical = contribution
            .canonical_json()
            .map_err(|error| ContributionError::Serialization { message: error.to_string() })?;
        contribution.fingerprint = ContentDigest::of_bytes(canonical.as_bytes());
        Ok(contribution)
    }

    /// Canonical JSON serialization: collections are pre-sorted, struct field
    /// order is fixed by the derive, and map keys are BTreeMap-ordered.
    pub fn canonical_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Whether this contribution proves an exact answer.
    ///
    /// Only [`ContributionCompleteness::Complete`] qualifies: partial,
    /// unavailable, stale, cancelled, budget-exhausted, instrument-failed,
    /// and invalid-subject records are never exact-empty.
    #[must_use]
    pub fn is_exact(&self) -> bool {
        self.completeness == ContributionCompleteness::Complete
    }
}
