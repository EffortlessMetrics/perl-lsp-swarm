//! Provider-neutral ordered structural access-hop contract (#13619).
//!
//! A structural access chain is the typed record of what
//! `$config->{groups}{staff}[0]` actually did, hop by hop. Each
//! [`StructuralAccessHop`] binds one *local* transition — the operator that
//! was written at that position, the key or index it selected, the aggregate
//! it selected out of, the value it produced, and the exact honesty state of
//! that selection. [`StructuralAccessChain`] orders those hops and proves the
//! order is intact.
//!
//! # Why the contract exists
//!
//! The compatibility receiver path in `perl-semantic-analyzer` encodes access
//! history as a mixture of broad evidence variants and heuristic strings
//! (`"array index receiver"`), keeps them in an unordered de-duplicated bag,
//! and labels nested aggregates with whatever text is available — degrading to
//! AST kind names such as `Binary` when the base is not a plain variable. That
//! is adequate for a bounded compatibility fact and insufficient as a
//! canonical contract: it cannot answer which operator was written where, in
//! what order, against which aggregate, under what budget, or with what
//! completeness.
//!
//! # Operator identity comes from the AST, never from source text
//!
//! Perl distinguishes four local subscript operators, and the parser already
//! preserves all four as distinct `Binary` operator strings — `{}`, `->{}`,
//! `[]`, `->[]`. [`StructuralAccessOperator`] mirrors exactly those four.
//! Collapsing arrow and plain forms would destroy the distinction this
//! contract exists to keep: in `$a->{b}{c}` the second hop is a *plain* hash
//! slot even though an arrow appeared earlier, and `$a{b}->[0]` differs from
//! `$a->{b}[0]` in its first hop.
//!
//! [`StructuralAccessSpelling`] carries the source text and range as
//! *evidence*. It is deliberately excluded from every fingerprint and from
//! every validation rule that decides an operator class, so no consumer can
//! reintroduce a substring scan and mistake an earlier `->{` for the current
//! local `{}`.
//!
//! # Transport trust boundary
//!
//! `Serialize`/`Deserialize` is a wire shape, not an invariant guard:
//! deserializing untrusted JSON can produce records that
//! [`StructuralAccessHop::new`] and [`StructuralAccessChain::new`] would have
//! rejected. Any consumer accepting a chain from a transport must call
//! [`StructuralAccessChain::validate`] before reuse, and must treat a
//! fingerprint match as a candidate confirmed by structural equality rather
//! than as proof of identity.
//!
//! [`StructuralAccessHop`] and [`StructuralAccessChain`] therefore keep their
//! fields private behind validated constructors and read-only accessors, as
//! [`crate::semantic_identity`] does. The laws here are cross-field — a
//! certainty is only honest relative to a disposition, an ordinal only
//! relative to an aggregate — so a record whose fields could be reassigned
//! after construction would carry no guarantee at all.
//!
//! # Ownership fence
//!
//! This module owns no LSP protocol type, parser type, AST type, provider
//! policy, or workspace storage. It performs no traversal, resolves no
//! aggregate, and changes no provider behavior — types, validation,
//! canonical serialization, deterministic fingerprints, and synthetic
//! fixtures only. Local aggregate flow analysis, callable-return shape
//! production, and the provider projection remain with #7434, #9474, and
//! #7464.

mod chain;
mod hop;

#[cfg(test)]
mod tests;

pub use chain::StructuralAccessChain;
pub use hop::StructuralAccessHop;

use serde::{Deserialize, Serialize};

use crate::semantic_identity::SemanticIdentityFingerprint;
use crate::{
    BoundaryDisposition, BoundaryKind, BoundaryLink, Confidence, FactId, FileId,
    SemanticReasonCode, SourceAnchor, SourceGeneration, ValueShape,
};

/// Shorthand for the shared deterministic fingerprint accumulator.
///
/// Every component in this contract folds itself through labelled
/// length-prefixed fields rather than being joined into one delimited string,
/// so no payload can shift content across a field boundary and make two
/// different records digest identically.
type Fingerprint = SemanticIdentityFingerprint;

/// `structural_access_chain.v1` schema version.
pub const STRUCTURAL_ACCESS_CHAIN_SCHEMA_VERSION: u32 = 1;

/// Stable schema tag folded into every fingerprint in this contract.
///
/// Producers and consumers must agree on this tag before comparing
/// fingerprints; a mismatch is an incompatible contract, not an equality
/// failure to be repaired by string comparison.
pub const STRUCTURAL_ACCESS_SCHEMA_TAG: &str = "structural-access-chain.v1";

/// Error returned by the structural access contract validators.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StructuralAccessContractError {
    /// An identity field that must identify a subject is empty or whitespace.
    EmptyIdentityField(&'static str),
    /// The selector class does not match the local operator class.
    SelectorOperatorMismatch {
        /// Operator tag written at this hop.
        operator: &'static str,
        /// Selector tag recorded against it.
        selector: &'static str,
    },
    /// The hop's aggregate does not match its position in the chain.
    AggregateChainPosition {
        /// Zero-based position of the offending hop.
        ordinal: u32,
        /// Why the aggregate cannot occupy that position.
        reason: &'static str,
    },
    /// A typed outcome/disposition/completeness combination cannot be claimed
    /// together.
    ContradictoryStatus(&'static str),
    /// A source range is inverted or otherwise structurally impossible.
    MalformedRange {
        /// Inclusive start byte recorded on the anchor.
        start_byte: u32,
        /// Exclusive end byte recorded on the anchor.
        end_byte: u32,
    },
    /// Budget accounting is not monotone, or contradicts the outcome.
    MalformedBudget(&'static str),
    /// A chain carries no hops, or hops that disagree about their subject.
    MalformedChain(&'static str),
}

impl std::fmt::Display for StructuralAccessContractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyIdentityField(field) => {
                write!(f, "structural access field `{field}` must be a non-empty identity")
            }
            Self::SelectorOperatorMismatch { operator, selector } => {
                write!(f, "operator `{operator}` cannot select through a `{selector}` selector")
            }
            Self::AggregateChainPosition { ordinal, reason } => {
                write!(f, "hop {ordinal} has an impossible input aggregate: {reason}")
            }
            Self::ContradictoryStatus(reason) => {
                write!(f, "contradictory structural access status: {reason}")
            }
            Self::MalformedRange { start_byte, end_byte } => {
                write!(f, "structural access range {start_byte}..{end_byte} is inverted")
            }
            Self::MalformedBudget(reason) => {
                write!(f, "structural access budget is malformed: {reason}")
            }
            Self::MalformedChain(reason) => {
                write!(f, "structural access chain is malformed: {reason}")
            }
        }
    }
}

impl std::error::Error for StructuralAccessContractError {}

/// One local subscript operator, exactly as written at this hop.
///
/// The four variants mirror the four distinct operator forms the parser
/// preserves. An arrow at an earlier hop never relabels a later hop: in
/// `$a->{b}{c}` the hops are [`Self::HashRefSlot`] then [`Self::HashSlot`].
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum StructuralAccessOperator {
    /// Plain hash element access, `$hash{key}` / `...{key}`.
    HashSlot,
    /// Arrow hash dereference, `$ref->{key}`.
    HashRefSlot,
    /// Plain array element access, `$array[0]` / `...[0]`.
    ArrayIndex,
    /// Arrow array dereference, `$ref->[0]`.
    ArrayRefIndex,
}

impl StructuralAccessOperator {
    /// Stable discriminant tag used inside fingerprints and diagnostics.
    #[must_use]
    pub const fn tag(self) -> &'static str {
        match self {
            Self::HashSlot => "hash-slot",
            Self::HashRefSlot => "hashref-slot",
            Self::ArrayIndex => "array-index",
            Self::ArrayRefIndex => "arrayref-index",
        }
    }

    /// Whether the operator addresses an aggregate by key rather than index.
    #[must_use]
    pub const fn is_keyed(self) -> bool {
        matches!(self, Self::HashSlot | Self::HashRefSlot)
    }

    /// Whether the operator dereferences a reference before selecting.
    ///
    /// This is a property of the written form only. It never reclassifies a
    /// later hop: `$a->{b}{c}` dereferences at the first hop and not the
    /// second, even though both select out of the same chain.
    #[must_use]
    pub const fn dereferences(self) -> bool {
        matches!(self, Self::HashRefSlot | Self::ArrayRefIndex)
    }
}

/// Canonical identity of the member this hop selected.
///
/// A dynamic key and a dynamic index are separate boundaries and never
/// collapse into one "dynamic" state: they limit different reasoning and the
/// consumer's recovery differs.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StructuralAccessSelector {
    /// A statically known hash key, already normalized to its canonical text.
    StaticKey(String),
    /// A statically known array index. Negative indices count from the end and
    /// remain exact.
    StaticIndex(i64),
    /// The key is computed at runtime; the boundary carries why.
    DynamicKey(BoundaryLink),
    /// The index is computed at runtime; the boundary carries why.
    DynamicIndex(BoundaryLink),
}

impl StructuralAccessSelector {
    /// Whether this selector stands behind a boundary that refuses promotion.
    ///
    /// A static selector names its member outright and stands behind nothing.
    #[must_use]
    pub const fn refuses_promotion(&self) -> bool {
        match self {
            Self::DynamicKey(link) | Self::DynamicIndex(link) => {
                matches!(link.disposition, BoundaryDisposition::Refuse)
            }
            Self::StaticKey(_) | Self::StaticIndex(_) => false,
        }
    }

    /// Stable discriminant tag used inside fingerprints and diagnostics.
    #[must_use]
    pub const fn tag(&self) -> &'static str {
        match self {
            Self::StaticKey(_) => "static-key",
            Self::StaticIndex(_) => "static-index",
            Self::DynamicKey(_) => "dynamic-key",
            Self::DynamicIndex(_) => "dynamic-index",
        }
    }

    /// Whether this selector addresses a keyed aggregate.
    #[must_use]
    pub const fn is_keyed(&self) -> bool {
        matches!(self, Self::StaticKey(_) | Self::DynamicKey(_))
    }

    /// Whether the selected member is only known at runtime.
    #[must_use]
    pub const fn is_dynamic(&self) -> bool {
        matches!(self, Self::DynamicKey(_) | Self::DynamicIndex(_))
    }

    /// Fold this selector's identity into a fingerprint.
    ///
    /// A dynamic selector contributes its boundary classification rather than
    /// a value, so two dynamic hops with different boundary reasons remain
    /// distinguishable without inventing a fake key.
    pub(super) fn fold(&self, accumulator: Fingerprint) -> Fingerprint {
        let accumulator = accumulator.discriminant("selector-kind", self.tag());
        match self {
            Self::StaticKey(key) => accumulator.field("selector-key", key),
            Self::StaticIndex(index) => accumulator.field("selector-index", &index.to_string()),
            Self::DynamicKey(boundary) | Self::DynamicIndex(boundary) => {
                fold_boundary("selector-boundary", boundary, accumulator)
            }
        }
    }
}

/// What this hop selected out of.
///
/// The first hop of a chain names a real input aggregate; every later hop
/// names its immediate predecessor by ordinal. That is what makes an
/// internally inconsistent order detectable rather than merely unlikely — a
/// hop naming a predecessor that is not `ordinal - 1`, or a drop or reorder
/// that leaves the ordinals non-dense.
///
/// It is a coherence check and not an integrity mechanism; see
/// [`StructuralAccessChain`] for what it deliberately cannot prove.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StructuralAccessAggregate {
    /// A source variable, identified by sigil and name (e.g. `$`, `config`).
    ///
    /// The sigil is the variable's own sigil as written, not the sigil implied
    /// by the access. This is a typed identity, never a rendered label: an
    /// unnameable base must use [`Self::Fact`] or [`Self::DynamicBoundary`]
    /// rather than degrade to an AST kind name.
    Variable {
        /// Variable sigil as written.
        sigil: String,
        /// Variable name without its sigil.
        name: String,
    },
    /// A canonical fact identity supplying the aggregate.
    Fact(FactId),
    /// The output of the immediately preceding hop in this chain.
    PrecedingHop {
        /// Zero-based ordinal of the preceding hop. Always `this ordinal - 1`.
        ordinal: u32,
    },
    /// The aggregate could not be identified; the boundary carries why.
    DynamicBoundary(BoundaryLink),
}

impl StructuralAccessAggregate {
    /// Stable discriminant tag used inside fingerprints and diagnostics.
    #[must_use]
    pub const fn tag(&self) -> &'static str {
        match self {
            Self::Variable { .. } => "variable",
            Self::Fact(_) => "fact",
            Self::PrecedingHop { .. } => "preceding-hop",
            Self::DynamicBoundary(_) => "dynamic-boundary",
        }
    }

    /// Fold this aggregate's identity into a fingerprint.
    ///
    /// The sigil and name are folded as separate fields rather than
    /// concatenated: `$` + `ab` and `$a` + `b` are different aggregates and
    /// must not share a digest.
    pub(super) fn fold(&self, accumulator: Fingerprint) -> Fingerprint {
        let accumulator = accumulator.discriminant("aggregate-kind", self.tag());
        match self {
            Self::Variable { sigil, name } => {
                accumulator.field("aggregate-sigil", sigil).field("aggregate-name", name)
            }
            Self::Fact(fact_id) => accumulator.field("aggregate-fact", &fact_id.0.to_string()),
            Self::PrecedingHop { ordinal } => {
                accumulator.field("aggregate-preceding", &ordinal.to_string())
            }
            Self::DynamicBoundary(boundary) => {
                fold_boundary("aggregate-boundary", boundary, accumulator)
            }
        }
    }

    /// Whether this aggregate stands behind a boundary that refuses promotion.
    #[must_use]
    pub const fn refuses_promotion(&self) -> bool {
        match self {
            Self::DynamicBoundary(link) => {
                matches!(link.disposition, BoundaryDisposition::Refuse)
            }
            Self::Variable { .. } | Self::Fact(_) | Self::PrecedingHop { .. } => false,
        }
    }

    fn validate(&self) -> Result<(), StructuralAccessContractError> {
        if let Self::Variable { sigil, name } = self {
            if sigil.trim().is_empty() {
                return Err(StructuralAccessContractError::EmptyIdentityField(
                    "StructuralAccessAggregate::Variable.sigil",
                ));
            }
            if name.trim().is_empty() {
                return Err(StructuralAccessContractError::EmptyIdentityField(
                    "StructuralAccessAggregate::Variable.name",
                ));
            }
            // Perl has exactly five sigils, and this field holds the
            // variable's own sigil as written, so nothing else can appear
            // here. A free-form string is the hole through which a rendered
            // label could return — an AST kind name such as `Binary` is
            // "non-blank" and would otherwise validate, which is precisely
            // the degradation this contract exists to eliminate. An
            // unnameable base has typed escapes and must use one.
            //
            // The parser HIR crate spells this same closed set as an enum,
            // but this crate cannot name it: the vocabulary is provider- and
            // parser-neutral by construction, and this module's own
            // architecture fence asserts as much by scanning these sources.
            // Hence the literal set here rather than a shared type.
            if !matches!(sigil.as_str(), "$" | "@" | "%" | "&" | "*") {
                return Err(StructuralAccessContractError::ContradictoryStatus(
                    "a variable aggregate must carry one canonical Perl sigil",
                ));
            }
        }
        Ok(())
    }
}

/// Source spelling and exact range for one hop.
///
/// This is evidence for explanation and navigation. It is excluded from every
/// fingerprint and from every rule that classifies an operator, so moving or
/// reformatting a file does not change hop identity and no consumer can
/// recover an operator by scanning text.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuralAccessSpelling {
    /// Source text of the hop as written, e.g. `->{groups}` or `[0]`.
    pub text: String,
    /// Exact source anchor for the hop.
    pub anchor: SourceAnchor,
}

impl StructuralAccessSpelling {
    /// Construct a spelling record.
    ///
    /// # Errors
    /// Returns [`StructuralAccessContractError::EmptyIdentityField`] when the
    /// text is blank, and [`StructuralAccessContractError::MalformedRange`]
    /// when the anchor range is inverted.
    pub fn new(
        text: impl Into<String>,
        anchor: SourceAnchor,
    ) -> Result<Self, StructuralAccessContractError> {
        let text = text.into();
        if text.trim().is_empty() {
            return Err(StructuralAccessContractError::EmptyIdentityField(
                "StructuralAccessSpelling.text",
            ));
        }
        if anchor.start_byte > anchor.end_byte {
            return Err(StructuralAccessContractError::MalformedRange {
                start_byte: anchor.start_byte,
                end_byte: anchor.end_byte,
            });
        }
        Ok(Self { text, anchor })
    }
}

/// Source, document, project, and root generation the hop is valid under.
///
/// Deliberately local to this contract rather than reused from the
/// interprocedural call subject: that subject additionally binds a toolchain
/// profile and a call site, neither of which identifies a structural access.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuralAccessSubject {
    /// Document the access is written in.
    pub document: FileId,
    /// Accepted source generation of that document.
    pub source_generation: SourceGeneration,
    /// Workspace root identity, when established.
    pub workspace_root: Option<String>,
    /// Accepted project/workspace generation, when established.
    pub project_generation: Option<SourceGeneration>,
}

impl StructuralAccessSubject {
    /// Construct a subject.
    ///
    /// # Errors
    /// Returns [`StructuralAccessContractError::EmptyIdentityField`] when the
    /// workspace root is present but blank.
    pub fn new(
        document: FileId,
        source_generation: SourceGeneration,
        workspace_root: Option<String>,
        project_generation: Option<SourceGeneration>,
    ) -> Result<Self, StructuralAccessContractError> {
        let subject = Self { document, source_generation, workspace_root, project_generation };
        subject.validate()?;
        Ok(subject)
    }

    /// Validate the subject.
    ///
    /// Separate from [`Self::new`] because a subject can also arrive through
    /// serde, which reconstructs the shape without running the constructor.
    /// [`StructuralAccessChain::validate`] calls this so the documented
    /// transport trust boundary actually holds.
    ///
    /// # Errors
    /// Returns [`StructuralAccessContractError::EmptyIdentityField`] when the
    /// workspace root is present but blank.
    pub fn validate(&self) -> Result<(), StructuralAccessContractError> {
        if let Some(root) = self.workspace_root.as_ref()
            && root.trim().is_empty()
        {
            return Err(StructuralAccessContractError::EmptyIdentityField(
                "StructuralAccessSubject.workspace_root",
            ));
        }
        Ok(())
    }

    /// Fold this subject's identity into a fingerprint.
    ///
    /// Every component is folded as its own labelled field. Joining them into
    /// one delimited string would let a delimiter inside a workspace root or a
    /// generation shift content across a field boundary, so a root of `b|c`
    /// under generation `a` would digest identically to a root of `c` under
    /// generation `a|b`.
    pub(super) fn fold(&self, accumulator: Fingerprint) -> Fingerprint {
        let accumulator = accumulator
            .field("subject-document", &self.document.0.to_string())
            .field("subject-root", self.workspace_root.as_deref().unwrap_or_default())
            .discriminant("subject-root-present", present_tag(self.workspace_root.is_some()));
        let accumulator =
            fold_generation("subject-generation", &self.source_generation, accumulator);
        match self.project_generation.as_ref() {
            Some(generation) => {
                fold_generation("subject-project-generation", generation, accumulator)
            }
            None => accumulator.discriminant("subject-project-generation", "absent"),
        }
    }
}

/// Fold a source generation, keeping `Unknown` distinct from a known-but-empty
/// value: they are different states and must not share a digest.
fn fold_generation(
    label: &str,
    generation: &SourceGeneration,
    accumulator: Fingerprint,
) -> Fingerprint {
    match generation {
        SourceGeneration::Known(value) => {
            accumulator.discriminant(label, "known").field(label, value)
        }
        SourceGeneration::Unknown => accumulator.discriminant(label, "unknown"),
    }
}

/// Fold every part of a boundary link as its own labelled field.
///
/// All four parts participate: two links that agree on kind and reason but
/// disagree on disposition are different boundaries — one degrades and one
/// refuses — and must not share a digest.
fn fold_boundary(label: &str, boundary: &BoundaryLink, accumulator: Fingerprint) -> Fingerprint {
    let accumulator = accumulator
        .field(label, boundary_kind_tag(boundary.kind))
        .field(label, reason_code_tag(boundary.reason_code))
        .field(label, boundary_disposition_tag(boundary.disposition));
    match boundary.boundary_id {
        Some(fact) => {
            accumulator.discriminant(label, "boundary-id").field(label, &fact.0.to_string())
        }
        None => accumulator.discriminant(label, "no-boundary-id"),
    }
}

/// Stable tag for an optional field's presence.
const fn present_tag(present: bool) -> &'static str {
    if present { "present" } else { "absent" }
}

/// Bounded work accounting across one hop.
///
/// Units are contract-neutral: the producer decides what one unit means and
/// must only keep the accounting monotone.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct StructuralAccessBudget {
    /// Units remaining before the hop was attempted.
    pub units_before: u32,
    /// Units remaining after the hop was attempted.
    pub units_after: u32,
}

impl StructuralAccessBudget {
    /// Construct a budget record.
    ///
    /// # Errors
    /// Returns [`StructuralAccessContractError::MalformedBudget`] when the
    /// remaining units increased across the hop.
    pub const fn new(
        units_before: u32,
        units_after: u32,
    ) -> Result<Self, StructuralAccessContractError> {
        let budget = Self { units_before, units_after };
        match budget.validate() {
            Ok(()) => Ok(budget),
            Err(error) => Err(error),
        }
    }

    /// Validate the accounting.
    ///
    /// Separate from [`Self::new`] because a budget can also arrive through
    /// serde, which reconstructs the shape without running the constructor.
    /// [`StructuralAccessHop::validate`] calls this so the documented
    /// transport trust boundary actually holds.
    ///
    /// # Errors
    /// Returns [`StructuralAccessContractError::MalformedBudget`] when the
    /// remaining units increased across the hop.
    pub const fn validate(&self) -> Result<(), StructuralAccessContractError> {
        if self.units_after > self.units_before {
            return Err(StructuralAccessContractError::MalformedBudget(
                "remaining units cannot increase across a hop",
            ));
        }
        Ok(())
    }

    /// Whether the hop consumed the last remaining unit.
    #[must_use]
    pub const fn is_exhausted(self) -> bool {
        self.units_after == 0
    }
}

/// Whether the aggregate's member set is closed.
///
/// This is what makes definite absence sayable at all: a member missing from
/// an open aggregate is unknown, not absent.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum StructuralAggregateCompleteness {
    /// Every member of the aggregate is known.
    Closed,
    /// Members may exist that the producer did not observe.
    Open,
}

impl StructuralAggregateCompleteness {
    /// Stable discriminant tag used inside fingerprints and diagnostics.
    #[must_use]
    pub const fn tag(self) -> &'static str {
        match self {
            Self::Closed => "closed",
            Self::Open => "open",
        }
    }
}

/// Whether the aggregate escaped the producer's view or was mutated.
///
/// Escape and mutation are separate facts and are both retained: an aggregate
/// may be handed to unanalyzed code without being written, and may be written
/// without escaping.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum StructuralAggregateDisposition {
    /// The aggregate neither escaped nor was mutated after construction.
    Stable,
    /// The aggregate reached code the producer did not analyze.
    Escaped,
    /// The aggregate was written after construction.
    Mutated,
    /// Both escape and mutation were observed.
    EscapedAndMutated,
}

impl StructuralAggregateDisposition {
    /// Stable discriminant tag used inside fingerprints and diagnostics.
    #[must_use]
    pub const fn tag(self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::Escaped => "escaped",
            Self::Mutated => "mutated",
            Self::EscapedAndMutated => "escaped-and-mutated",
        }
    }

    /// Whether the aggregate is still exactly the one the producer observed.
    #[must_use]
    pub const fn is_stable(self) -> bool {
        matches!(self, Self::Stable)
    }
}

/// Whether the hop's outcome holds on every path or only on some path.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum StructuralHopCertainty {
    /// The outcome holds on every admitted path.
    Definite,
    /// The outcome holds on at least one admitted path.
    Possible,
}

impl StructuralHopCertainty {
    /// Stable discriminant tag used inside fingerprints and diagnostics.
    #[must_use]
    pub const fn tag(self) -> &'static str {
        match self {
            Self::Definite => "definite",
            Self::Possible => "possible",
        }
    }
}

/// What the hop produced, or the exact reason it produced nothing.
///
/// Every non-selecting state is distinct. In particular
/// [`Self::AbsentMember`] (a definite answer: the member is not there) never
/// collapses into [`Self::UnknownMember`] (no answer: the aggregate is open),
/// and neither collapses into a boundary, a stale generation, or an exhausted
/// budget.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StructuralHopOutcome {
    /// The hop selected a value.
    Selected {
        /// Shape of the selected value.
        shape: ValueShape,
        /// Canonical fact identity for the selected value, when promoted.
        value_fact: Option<FactId>,
    },
    /// The member is definitely not present. Requires a closed aggregate.
    AbsentMember,
    /// The member was not observed, and the aggregate is open, so absence is
    /// not established.
    UnknownMember,
    /// The operator does not match the aggregate's actual shape, e.g. an
    /// array index applied to a hash.
    ShapeMismatch {
        /// The shape the aggregate actually had.
        observed: ValueShape,
    },
    /// The aggregate's generation no longer matches the subject.
    StaleGeneration,
    /// The work budget was exhausted before the hop could be decided.
    BudgetExhausted,
    /// A typed boundary stopped the hop; the link carries which.
    Boundary(BoundaryLink),
}

impl StructuralHopOutcome {
    /// Stable discriminant tag used inside fingerprints and diagnostics.
    #[must_use]
    pub const fn tag(&self) -> &'static str {
        match self {
            Self::Selected { .. } => "selected",
            Self::AbsentMember => "absent-member",
            Self::UnknownMember => "unknown-member",
            Self::ShapeMismatch { .. } => "shape-mismatch",
            Self::StaleGeneration => "stale-generation",
            Self::BudgetExhausted => "budget-exhausted",
            Self::Boundary(_) => "boundary",
        }
    }

    /// Whether the hop produced a value another hop can select out of.
    #[must_use]
    pub const fn is_selecting(&self) -> bool {
        matches!(self, Self::Selected { .. })
    }

    /// Whether this outcome's truth depends on what the aggregate contains.
    ///
    /// Selecting a value, finding a member absent or unobserved, and finding a
    /// shape mismatch are all claims about the aggregate's contents, so an
    /// aggregate that escaped or was mutated undermines them.
    ///
    /// A stale generation, an exhausted budget and a boundary are not: each
    /// holds regardless of what the aggregate turned out to contain, and can
    /// be definite even over an aggregate that moved.
    #[must_use]
    pub const fn depends_on_aggregate_contents(&self) -> bool {
        match self {
            Self::Selected { .. }
            | Self::AbsentMember
            | Self::UnknownMember
            | Self::ShapeMismatch { .. } => true,
            Self::StaleGeneration | Self::BudgetExhausted | Self::Boundary(_) => false,
        }
    }

    /// Whether this outcome claims an answer about a *member* of the aggregate.
    ///
    /// [`Self::Selected`], [`Self::AbsentMember`] and [`Self::UnknownMember`]
    /// all assert something about looking a member up — that one was found,
    /// that none is there, or that it could not be established. Reaching any
    /// of them means the operator applied to the aggregate in the first place.
    ///
    /// The rest are excluded, each for its own reason:
    ///
    /// - [`Self::ShapeMismatch`] is the honest record of the operator *not*
    ///   applying, so it must remain available exactly where the others are
    ///   refused; forbidding it would leave no way to record the conflict.
    /// - [`Self::StaleGeneration`], [`Self::BudgetExhausted`] and
    ///   [`Self::Boundary`] stopped before any member lookup happened, so
    ///   they say nothing about members at all.
    ///
    /// This is deliberately *not* [`Self::depends_on_aggregate_contents`],
    /// which includes `ShapeMismatch`. The two predicates answer different
    /// questions — "is this claim about what the aggregate holds?" versus
    /// "did this claim require the operator to apply?" — and collapsing them
    /// would either forbid honest mismatches or admit dishonest absences.
    #[must_use]
    pub const fn claims_member_answer(&self) -> bool {
        match self {
            Self::Selected { .. } | Self::AbsentMember | Self::UnknownMember => true,
            Self::ShapeMismatch { .. }
            | Self::StaleGeneration
            | Self::BudgetExhausted
            | Self::Boundary(_) => false,
        }
    }

    /// Fold this outcome's identity into a fingerprint.
    pub(super) fn fold(&self, accumulator: Fingerprint) -> Fingerprint {
        let accumulator = accumulator.discriminant("outcome-kind", self.tag());
        match self {
            Self::Selected { shape, value_fact } => {
                let accumulator = fold_value_shape("outcome-shape", shape, accumulator);
                match value_fact {
                    Some(fact) => accumulator
                        .discriminant("outcome-fact", "present")
                        .field("outcome-fact", &fact.0.to_string()),
                    None => accumulator.discriminant("outcome-fact", "absent"),
                }
            }
            Self::ShapeMismatch { observed } => {
                fold_value_shape("outcome-observed-shape", observed, accumulator)
            }
            Self::Boundary(boundary) => fold_boundary("outcome-boundary", boundary, accumulator),
            Self::AbsentMember
            | Self::UnknownMember
            | Self::StaleGeneration
            | Self::BudgetExhausted => accumulator,
        }
    }
}

/// Fold a value shape, keeping each payload in its own labelled field.
fn fold_value_shape(label: &str, shape: &ValueShape, accumulator: Fingerprint) -> Fingerprint {
    match shape {
        ValueShape::Unknown => accumulator.discriminant(label, "unknown"),
        ValueShape::Scalar => accumulator.discriminant(label, "scalar"),
        ValueShape::ArrayRef => accumulator.discriminant(label, "array-ref"),
        ValueShape::HashRef => accumulator.discriminant(label, "hash-ref"),
        ValueShape::CodeRef => accumulator.discriminant(label, "code-ref"),
        ValueShape::PackageName { package } => {
            accumulator.discriminant(label, "package-name").field(label, package)
        }
        ValueShape::Object { package, confidence } => accumulator
            .discriminant(label, "object")
            .field(label, package)
            .field(label, confidence_tag(*confidence)),
    }
}

/// Context or source limitation retained by one hop.
///
/// Every limitation here narrows how far a *source-anchored* hop can be
/// trusted. None of them removes the source: a hop always carries a
/// [`StructuralAccessSpelling`] with non-blank text and an anchor in the
/// chain subject's own document, because spelling is this contract's evidence
/// that the access was written at all.
///
/// There is deliberately no source-free limitation. The repository's
/// established meaning for that case is `GeneratedNoSource`, which
/// PLSP-SPEC-0017 defines as a candidate *without a source declaration
/// anchor*; the parser's `PirAnchorKind::GeneratedNoSource` reports
/// `is_source_backed() == false` for the same reason. A hop cannot satisfy
/// that definition and this contract's anchoring law at once, so offering the
/// variant here would advertise a record no honest producer could build — it
/// could only be built by fabricating a spelling and an anchor, which is
/// exactly the substitution the module doc forbids. Recording a source-free
/// structural access is a real capability, not an oversight, and needs an
/// optional spelling plus a producer that requires it.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum StructuralAccessLimitation {
    /// The selector is computed at runtime.
    DynamicSelector,
    /// The aggregate's member set is not closed.
    OpenAggregate,
    /// The aggregate reached unanalyzed code.
    EscapedAggregate,
    /// The aggregate was written after construction.
    MutatedAggregate,
    /// The hop was reconstructed from recovered syntax.
    ///
    /// Recovered syntax is still written syntax: the hop keeps its spelling and
    /// anchor, and only the confidence in the reconstruction is reduced. A hop
    /// with no source at all is outside this contract; see the type's own
    /// documentation.
    RecoveredSyntax,
    /// The work budget was exhausted.
    BudgetExhausted,
    /// A dependency generation is no longer current.
    StaleDependency,
    /// The hop came through the compatibility receiver bridge rather than the
    /// canonical structural producer.
    CompatibilityBridge,
    /// The construct is not supported by the producer.
    Unsupported,
}

impl StructuralAccessLimitation {
    /// Stable discriminant tag used inside fingerprints and diagnostics.
    #[must_use]
    pub const fn tag(self) -> &'static str {
        match self {
            Self::DynamicSelector => "dynamic-selector",
            Self::OpenAggregate => "open-aggregate",
            Self::EscapedAggregate => "escaped-aggregate",
            Self::MutatedAggregate => "mutated-aggregate",
            Self::RecoveredSyntax => "recovered-syntax",
            Self::BudgetExhausted => "budget-exhausted",
            Self::StaleDependency => "stale-dependency",
            Self::CompatibilityBridge => "compatibility-bridge",
            Self::Unsupported => "unsupported",
        }
    }
}

// ── Stable tags for borrowed vocabulary ───────────────────────────────────
//
// These enums live in this crate but outside this contract, so they carry no
// `tag()` of their own. Fingerprints must not fold their `Debug` output: a
// variant rename would then silently change every persisted digest under an
// unchanged `structural_access_chain.v1`. Spelling the tags out here fixes the
// wire text independently of the Rust identifiers, and because the enums are
// crate-local these matches are exhaustive — adding a variant is a compile
// error that forces a deliberate choice rather than a silent digest change.

/// Stable fingerprint tag for a boundary kind.
fn boundary_kind_tag(kind: BoundaryKind) -> &'static str {
    match kind {
        BoundaryKind::DynamicValue => "dynamic-value",
        BoundaryKind::DynamicRequire => "dynamic-require",
        BoundaryKind::DynamicIncludePath => "dynamic-include-path",
        BoundaryKind::CompileTimeExecution => "compile-time-execution",
        BoundaryKind::SymbolicReference => "symbolic-reference",
        BoundaryKind::Compatibility => "compatibility",
        BoundaryKind::ExternalEnvironment => "external-environment",
        BoundaryKind::Unsupported => "unsupported",
    }
}

/// Stable fingerprint tag for a boundary disposition.
fn boundary_disposition_tag(disposition: BoundaryDisposition) -> &'static str {
    match disposition {
        BoundaryDisposition::Degrade => "degrade",
        BoundaryDisposition::Refuse => "refuse",
    }
}

/// Stable fingerprint tag for a semantic reason code.
fn reason_code_tag(reason: SemanticReasonCode) -> &'static str {
    match reason {
        SemanticReasonCode::ExactSource => "exact-source",
        SemanticReasonCode::GeneratedFromSource => "generated-from-source",
        SemanticReasonCode::DynamicValue => "dynamic-value",
        SemanticReasonCode::CompatibilityBoundary => "compatibility-boundary",
        SemanticReasonCode::UnsupportedEffect => "unsupported-effect",
        SemanticReasonCode::MissingGeneration => "missing-generation",
        SemanticReasonCode::UnknownProvenance => "unknown-provenance",
        SemanticReasonCode::UnknownConfidence => "unknown-confidence",
        SemanticReasonCode::UnknownLifecycle => "unknown-lifecycle",
        SemanticReasonCode::StaleDependency => "stale-dependency",
        SemanticReasonCode::Unknown => "unknown",
    }
}

/// Stable fingerprint tag for a confidence level.
fn confidence_tag(confidence: Confidence) -> &'static str {
    match confidence {
        Confidence::High => "high",
        Confidence::Medium => "medium",
        Confidence::Low => "low",
    }
}

/// Whether a known value shape can carry the next hop's operator class.
///
/// A hash reference carries keyed operators, an array reference indexed ones.
///
/// A code reference and a package name carry neither, and both are decisive
/// about it: each is a defined value that cannot become something else, so
/// subscripting one is an error rather than an access. Verified against the
/// interpreter — `$coderef->{k}` is `Not a HASH reference`, and `$str->{k}`
/// for a defined string is `Can't use string ("Foo") as a HASH ref while
/// "strict refs" in use`. A symbolic dereference belongs at this contract's
/// own symbolic-reference boundary rather than in a plain selection.
///
/// `Scalar`, `Object` and `Unknown` carry everything, and each does so because
/// it asserts too little to contradict anything — never because every operator
/// truly applies:
///
/// - `Scalar` does not distinguish `undef` from a defined non-reference, and
///   `undef` *is* subscriptable: Perl autovivifies it. `my $x; $x->{k} = 1`
///   leaves `$x` a hash reference, and even the rvalue `$z->{k}` on an undef
///   `$z` succeeds and autovivifies. Treating every `Scalar` as decisively
///   non-subscriptable rejected that honest chain.
/// - `Object` is a blessed reference that may be a blessed hash or a blessed
///   array.
/// - `Unknown` asserts nothing at all.
pub(super) fn shape_carries(shape: &ValueShape, next_is_keyed: bool) -> bool {
    match shape {
        ValueShape::HashRef => next_is_keyed,
        ValueShape::ArrayRef => !next_is_keyed,
        ValueShape::CodeRef | ValueShape::PackageName { .. } => false,
        ValueShape::Scalar | ValueShape::Object { .. } | ValueShape::Unknown => true,
    }
}

/// Whether a shape says enough to decide what an operator may do with it.
///
/// The permissive shapes in [`shape_carries`] return `true` for every operator
/// because they assert too little, not because they carry everything — so a
/// mismatch recorded against one of them cannot be contradicted either.
pub(super) fn shape_is_decisive(shape: &ValueShape) -> bool {
    matches!(
        shape,
        ValueShape::HashRef
            | ValueShape::ArrayRef
            | ValueShape::CodeRef
            | ValueShape::PackageName { .. }
    )
}

/// Whether two shapes can honestly describe one and the same value.
///
/// This is a different question from [`shape_is_decisive`], which asks what a
/// shape can carry. A shape may decide no operator and still be a definite
/// claim about what the value *is*: `ValueShape::Scalar` decides nothing about
/// subscripting, because `undef` is a scalar and Perl autovivifies it, yet it
/// still says the value is a plain scalar rather than a hash reference.
///
/// The relation is symmetric on purpose. A blessed reference carries both a
/// class and an underlying reference kind — `bless {}, "Foo"` has
/// `ref` `Foo` and `reftype` `HASH` — so `Object` and `HashRef` are two honest
/// descriptions of one value, and which one a producer recorded first must not
/// decide whether the record is valid. The same holds for blessed arrays and
/// blessed code references.
///
/// What remains a contradiction:
///
/// - two different classes, since one value has one class;
/// - `Object` against `Scalar` or `PackageName`, since a blessed value is
///   always a reference and neither of those is one;
/// - any two different concrete representations, such as `HashRef` against
///   `ArrayRef`.
///
/// `Unknown` asserts nothing, so nothing can disagree with it.
pub(super) fn shapes_may_describe_one_value(left: &ValueShape, right: &ValueShape) -> bool {
    match (left, right) {
        // Two observations of the same class agree however sure each producer
        // was. `confidence` is documented as "confidence in the inferred
        // package" — it records how well the claim is known, not what the
        // value is — so comparing whole values here would let an epistemic
        // field decide a question about identity. Different packages remain a
        // contradiction, because one value has one class.
        (
            ValueShape::Object { package: left_package, .. },
            ValueShape::Object { package: right_package, .. },
        ) => left_package == right_package,
        // `Unknown` asserts nothing, so nothing can disagree with it.
        (ValueShape::Unknown, _) | (_, ValueShape::Unknown) => true,
        // A blessed reference carries both a class and an underlying reference
        // kind, symmetrically.
        (
            ValueShape::Object { .. },
            ValueShape::HashRef | ValueShape::ArrayRef | ValueShape::CodeRef,
        )
        | (
            ValueShape::HashRef | ValueShape::ArrayRef | ValueShape::CodeRef,
            ValueShape::Object { .. },
        ) => true,
        // Every remaining shape carries only value-identifying content, so
        // equality is the right question for those.
        _ => left == right,
    }
}
