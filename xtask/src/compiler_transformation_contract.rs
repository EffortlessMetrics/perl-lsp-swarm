//! Dependency-neutral contracts for proof-carrying compiler transformations
//! (#12616, train row T01, parent controller #12575).
//!
//! This module owns the in-memory vocabulary for three versioned contracts:
//!
//! ```text
//! compiler_transformation_law.v1     -> `TransformationLaw`
//! compiler_transformation_plan.v1    -> `TransformationPlan`
//! compiler_transformation_result.v1  -> `TransformationResult`
//! ```
//!
//! A law states one reviewed semantic rewrite rule: its class, its exact input
//! and output stage, the preconditions a plan must discharge, the propositions
//! that are load-bearing for it, the changes it is permitted to make, the
//! dynamic concepts it excludes, its partial-application policy, its permitted
//! consumers, and its claim ceiling.  A plan instantiates one law against one
//! exact candidate/source/generation/profile/version/platform/capability
//! subject and one exact input IR subject, selecting stable operation
//! identities.  A result is the closed terminal vocabulary for one application
//! attempt of one plan.
//!
//! Deliberately absent (issue non-goals): no law registry, no transformation
//! implementation, no optimizer pipeline, no source refactor, no provider
//! promotion, no serde derives, no file or manifest syntax, no CLI, no
//! process, network, or compiler execution.  T02 (registry), T03
//! (implementations), and T04 (equivalence proof) instantiate this vocabulary;
//! they must not invent a second one.
//!
//! Legality laws expressed and validated here:
//!
//! - preconditions are conjunctive and exact-subject-bound: only
//!   [`PreconditionTruth::ProvenExact`] satisfies exact legality, so unknown,
//!   overloaded, tied, magical, ambient, or externally effectful state can
//!   never discharge one ([`PreconditionTruth::satisfies_exact_legality`]);
//! - one stage cannot borrow another stage's proof: a precondition's evidence
//!   must be gathered at the precondition's own [`CompilerStage`], and every
//!   selected location must sit at the plan's exact input stage;
//! - preserved propositions the law declares load-bearing must be preserved by
//!   the plan *and* carry an independent equivalence obligation naming them;
//! - a plan is identified by stable canonical operation identities, never by
//!   source text ranges or by a property of current transformed output
//!   ([`LocationSelector::validate`]);
//! - partial application is prohibited unless the law explicitly declares
//!   independent complete subplans ([`PartialApplicationPolicy`]);
//! - source projection requires a separate canonical `RefactorPlan` relation,
//!   and no internal class may name [`ConsumerClass::SourceEdit`]
//!   ([`TransformationClass::permitted_consumers`]);
//! - refusal is a terminal result, never an applied-empty transformation, and
//!   measured speed is evaluated strictly after legality, verifier, and
//!   equivalence ([`TransformationPlan::evaluate`]);
//! - canonical bytes are deterministic under location and obligation order,
//!   bounded, and private-safe: [`SourceProvenance`] retains a relative path
//!   and a byte span, never source text or a host path.

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

/// Maximum length of any free-text field retained by these contracts.
///
/// Bounding every text field is what makes canonical bytes bounded without
/// truncating them at render time (truncation would make two different
/// subjects share one fingerprint).
pub const MAX_TEXT_LEN: usize = 512;

/// Maximum number of operation locations one plan may select.
pub const MAX_SELECTED_LOCATIONS: usize = 1024;

/// Maximum size of the canonical semantic text of one law or plan.
pub const MAX_CANONICAL_TEXT_BYTES: usize = 65_536;

// ---------------------------------------------------------------------------
// Identity types
// ---------------------------------------------------------------------------

fn non_empty(field: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        bail!("{field} must not be empty");
    }
    if value.len() > MAX_TEXT_LEN {
        bail!("{field} must be at most {MAX_TEXT_LEN} bytes, got {}", value.len());
    }
    Ok(())
}

/// Exact identity of one reviewed transformation law.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct LawId(String);

impl LawId {
    /// Construct a non-empty bounded law identity.
    pub fn new(value: &str) -> Result<Self> {
        non_empty("transformation law id", value)?;
        Ok(Self(value.to_owned()))
    }

    /// Borrow the identity text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Exact version of one reviewed transformation law.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct LawVersion(String);

impl LawVersion {
    /// Construct a non-empty `v`-prefixed law version.
    pub fn new(value: &str) -> Result<Self> {
        non_empty("transformation law version", value)?;
        if !value.starts_with('v') {
            bail!("transformation law version {value:?} must start with 'v'");
        }
        Ok(Self(value.to_owned()))
    }

    /// Borrow the version text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Deterministic semantic fingerprint of a law or plan.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ContractDigest(String);

impl ContractDigest {
    /// Construct a digest from exactly 64 lowercase hex characters.
    pub fn from_hex(value: &str) -> Result<Self> {
        if value.len() != 64
            || !value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            bail!("contract digest must be 64 lowercase hex characters, got {value:?}");
        }
        Ok(Self(value.to_owned()))
    }

    /// Borrow the digest text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Exact identity of one transformation plan.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PlanId(String);

impl PlanId {
    /// Construct a non-empty bounded plan identity.
    pub fn new(value: &str) -> Result<Self> {
        non_empty("transformation plan id", value)?;
        Ok(Self(value.to_owned()))
    }

    /// Borrow the identity text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Stable identity of one operation inside a fixed canonical IR subject.
///
/// This is the only admissible way to select a transformation location; a
/// source range is provenance, not identity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct OperationId(String);

impl OperationId {
    /// Construct a non-empty bounded operation identity.
    pub fn new(value: &str) -> Result<Self> {
        non_empty("operation id", value)?;
        Ok(Self(value.to_owned()))
    }

    /// Borrow the identity text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Exact identity of one precondition named by a law and instantiated by a plan.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PreconditionId(String);

impl PreconditionId {
    /// Construct a non-empty bounded precondition identity.
    pub fn new(value: &str) -> Result<Self> {
        non_empty("precondition id", value)?;
        Ok(Self(value.to_owned()))
    }

    /// Borrow the identity text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Exact named subject or evidence reference.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SubjectRef(String);

impl SubjectRef {
    /// Construct a non-empty bounded subject reference.
    pub fn new(value: &str) -> Result<Self> {
        non_empty("subject reference", value)?;
        Ok(Self(value.to_owned()))
    }

    /// Borrow the subject text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Non-empty scope of required or measured work.  Zero-work scope cannot be
/// constructed, so "no work" is never expressible as a work contract.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct WorkScope(String);

impl WorkScope {
    /// Construct a non-empty bounded work scope.
    pub fn new(value: &str) -> Result<Self> {
        non_empty("work scope", value)?;
        Ok(Self(value.to_owned()))
    }

    /// Borrow the work scope text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Monotonic compiler generation of an input subject.
///
/// A plan is bound to the generation it was built against; a later generation
/// makes the plan stale rather than reusable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Generation(pub u64);

// ---------------------------------------------------------------------------
// Closed independent dimensions
// ---------------------------------------------------------------------------

/// Exact compiler stage that owns a fact, a location, or a proof.
///
/// The five stages are independent proof planes.  Evidence gathered at one
/// stage can never discharge a precondition stated at another.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CompilerStage {
    /// Syntactic parser stage.
    Parser,
    /// Canonical high-level IR.
    Hir,
    /// Canonical place/access IR.
    PirA,
    /// Compile-effect stage.
    Effects,
    /// Canonical execution IR.
    Eir,
}

impl CompilerStage {
    /// Closed list of every compiler stage.
    pub const ALL: [Self; 5] = [Self::Parser, Self::Hir, Self::PirA, Self::Effects, Self::Eir];

    /// Stable canonical tag.
    pub fn tag(self) -> &'static str {
        match self {
            Self::Parser => "parser",
            Self::Hir => "hir",
            Self::PirA => "pir_a",
            Self::Effects => "effects",
            Self::Eir => "eir",
        }
    }
}

/// Closed transformation class.  Each class declares the consumers it may
/// serve; a class never widens because source mapping happens to exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TransformationClass {
    /// Canonicalization internal to one stage.
    InternalCanonicalization,
    /// Simplification that preserves every analysis-visible proposition.
    AnalysisPreservingSimplification,
    /// Rewrite whose purpose is bounded execution cost.
    ExecutionOptimization,
    /// Strengthening of facts without rewriting any IR.
    FactStrengtheningWithoutIrRewrite,
    /// Candidate for a separately authorized source edit.
    SourceProjectionCandidate,
    /// Explicitly unsupported or not-applicable rewrite.
    UnsupportedOrNotApplicable,
}

impl TransformationClass {
    /// Closed list of every transformation class.
    pub const ALL: [Self; 6] = [
        Self::InternalCanonicalization,
        Self::AnalysisPreservingSimplification,
        Self::ExecutionOptimization,
        Self::FactStrengtheningWithoutIrRewrite,
        Self::SourceProjectionCandidate,
        Self::UnsupportedOrNotApplicable,
    ];

    /// Stable canonical tag.
    pub fn tag(self) -> &'static str {
        match self {
            Self::InternalCanonicalization => "internal_canonicalization",
            Self::AnalysisPreservingSimplification => "analysis_preserving_simplification",
            Self::ExecutionOptimization => "execution_optimization",
            Self::FactStrengtheningWithoutIrRewrite => "fact_strengthening_without_ir_rewrite",
            Self::SourceProjectionCandidate => "source_projection_candidate",
            Self::UnsupportedOrNotApplicable => "unsupported_or_not_applicable",
        }
    }

    /// Closed set of consumers this class may serve.
    ///
    /// [`ConsumerClass::SourceEdit`] appears for exactly one class, so an
    /// internal rewrite can never become a source edit by declaration.
    pub fn permitted_consumers(self) -> BTreeSet<ConsumerClass> {
        let permitted: &[ConsumerClass] = match self {
            Self::InternalCanonicalization => &[ConsumerClass::InternalStageRewrite],
            Self::AnalysisPreservingSimplification => &[
                ConsumerClass::InternalStageRewrite,
                ConsumerClass::Analysis,
                ConsumerClass::Diagnostic,
            ],
            Self::ExecutionOptimization => {
                &[ConsumerClass::InternalStageRewrite, ConsumerClass::BoundedExecution]
            }
            Self::FactStrengtheningWithoutIrRewrite => {
                &[ConsumerClass::FactStore, ConsumerClass::Analysis, ConsumerClass::Diagnostic]
            }
            Self::SourceProjectionCandidate => &[ConsumerClass::SourceEdit],
            Self::UnsupportedOrNotApplicable => &[ConsumerClass::NoConsumer],
        };
        permitted.iter().copied().collect()
    }

    /// Changes this class may never make, whatever a law declares.
    fn forbidden_changes(self) -> BTreeSet<ChangedProposition> {
        let forbidden: &[ChangedProposition] = match self {
            Self::FactStrengtheningWithoutIrRewrite => {
                &[ChangedProposition::IrShape, ChangedProposition::SourceText]
            }
            Self::SourceProjectionCandidate => &[],
            Self::UnsupportedOrNotApplicable => &[
                ChangedProposition::IrShape,
                ChangedProposition::FactStrength,
                ChangedProposition::ExecutionCost,
                ChangedProposition::RedundantOperationCount,
                ChangedProposition::UnreachableEdgeCount,
                ChangedProposition::SourceText,
            ],
            _ => &[ChangedProposition::SourceText],
        };
        forbidden.iter().copied().collect()
    }
}

/// Closed consumer class a transformation may serve.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ConsumerClass {
    /// Rewrite of the canonical IR inside the compiler.
    InternalStageRewrite,
    /// Analysis that reads the transformed facts.
    Analysis,
    /// Diagnostic production.
    Diagnostic,
    /// Bounded execution of the transformed subject.
    BoundedExecution,
    /// Storage of strengthened facts without an IR rewrite.
    FactStore,
    /// A separately authorized source edit.
    SourceEdit,
    /// No consumer at all.
    NoConsumer,
}

impl ConsumerClass {
    /// Closed list of every consumer class.
    pub const ALL: [Self; 7] = [
        Self::InternalStageRewrite,
        Self::Analysis,
        Self::Diagnostic,
        Self::BoundedExecution,
        Self::FactStore,
        Self::SourceEdit,
        Self::NoConsumer,
    ];

    /// Stable canonical tag.
    pub fn tag(self) -> &'static str {
        match self {
            Self::InternalStageRewrite => "internal_stage_rewrite",
            Self::Analysis => "analysis",
            Self::Diagnostic => "diagnostic",
            Self::BoundedExecution => "bounded_execution",
            Self::FactStore => "fact_store",
            Self::SourceEdit => "source_edit",
            Self::NoConsumer => "no_consumer",
        }
    }
}

/// Closed dynamic or unsupported concept.
///
/// None of these can satisfy an exact precondition; a law names the ones it
/// excludes so that the exclusion survives into the plan and the result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DynamicConcept {
    /// Operator overloading.
    Overload,
    /// Tied variables.
    Tie,
    /// Magical variables.
    Magic,
    /// Symbolic references.
    SymbolicReference,
    /// Ambient host or environment input.
    AmbientInput,
    /// Externally observable effect.
    ExternalEffect,
    /// Arbitrary call to unknown code.
    ArbitraryCall,
    /// XS boundary.
    XsBoundary,
    /// Platform-dependent behavior.
    PlatformDependent,
}

impl DynamicConcept {
    /// Closed list of every dynamic or unsupported concept.
    pub const ALL: [Self; 9] = [
        Self::Overload,
        Self::Tie,
        Self::Magic,
        Self::SymbolicReference,
        Self::AmbientInput,
        Self::ExternalEffect,
        Self::ArbitraryCall,
        Self::XsBoundary,
        Self::PlatformDependent,
    ];

    /// Stable canonical tag.
    pub fn tag(self) -> &'static str {
        match self {
            Self::Overload => "overload",
            Self::Tie => "tie",
            Self::Magic => "magic",
            Self::SymbolicReference => "symbolic_reference",
            Self::AmbientInput => "ambient_input",
            Self::ExternalEffect => "external_effect",
            Self::ArbitraryCall => "arbitrary_call",
            Self::XsBoundary => "xs_boundary",
            Self::PlatformDependent => "platform_dependent",
        }
    }
}

/// Closed proposition a transformation may be required to preserve.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PreservedProposition {
    /// Emitted warnings and their classes.
    Warnings,
    /// Raised exceptions.
    Exceptions,
    /// Observable effects.
    Effects,
    /// Evaluation order.
    EvaluationOrder,
    /// Scalar/list/void context.
    Context,
    /// Aliasing relationships.
    Aliasing,
    /// Source mapping used by diagnostics and edits.
    SourceMapping,
    /// Value, container, and subroutine identities.
    Identity,
    /// Cleanup and destruction behavior.
    Cleanup,
    /// The declared unsupported boundary.
    UnsupportedBoundary,
}

impl PreservedProposition {
    /// Closed list of every preservable proposition.
    pub const ALL: [Self; 10] = [
        Self::Warnings,
        Self::Exceptions,
        Self::Effects,
        Self::EvaluationOrder,
        Self::Context,
        Self::Aliasing,
        Self::SourceMapping,
        Self::Identity,
        Self::Cleanup,
        Self::UnsupportedBoundary,
    ];

    /// Stable canonical tag.
    pub fn tag(self) -> &'static str {
        match self {
            Self::Warnings => "warnings",
            Self::Exceptions => "exceptions",
            Self::Effects => "effects",
            Self::EvaluationOrder => "evaluation_order",
            Self::Context => "context",
            Self::Aliasing => "aliasing",
            Self::SourceMapping => "source_mapping",
            Self::Identity => "identity",
            Self::Cleanup => "cleanup",
            Self::UnsupportedBoundary => "unsupported_boundary",
        }
    }
}

/// Closed proposition a transformation may intend to change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ChangedProposition {
    /// Shape of the canonical IR.
    IrShape,
    /// Strength of a derived fact.
    FactStrength,
    /// Bounded execution cost.
    ExecutionCost,
    /// Count of proven-redundant operations.
    RedundantOperationCount,
    /// Count of proven-unreachable control edges.
    UnreachableEdgeCount,
    /// Program source text.
    SourceText,
}

impl ChangedProposition {
    /// Closed list of every changeable proposition.
    pub const ALL: [Self; 6] = [
        Self::IrShape,
        Self::FactStrength,
        Self::ExecutionCost,
        Self::RedundantOperationCount,
        Self::UnreachableEdgeCount,
        Self::SourceText,
    ];

    /// Stable canonical tag.
    pub fn tag(self) -> &'static str {
        match self {
            Self::IrShape => "ir_shape",
            Self::FactStrength => "fact_strength",
            Self::ExecutionCost => "execution_cost",
            Self::RedundantOperationCount => "redundant_operation_count",
            Self::UnreachableEdgeCount => "unreachable_edge_count",
            Self::SourceText => "source_text",
        }
    }
}

/// Closed claim ceiling a law and its plans may reach.
///
/// The four ceilings are deliberately **not** ordered. Bounded execution is
/// not "more" than analysis and diagnostics; they are different claims, so a
/// law that proves one does not license the other. Only
/// [`ClaimCeiling::InternalFactOnly`] is universally weaker, because every
/// ceiling already proves its internal facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaimCeiling {
    /// Internal facts only.
    InternalFactOnly,
    /// Analysis and diagnostic consumption.
    AnalysisAndDiagnostic,
    /// Bounded execution.
    BoundedExecution,
    /// A separately authorized source edit.
    AuthorizedSourceEdit,
}

impl ClaimCeiling {
    /// Closed list of every claim ceiling.
    pub const ALL: [Self; 4] = [
        Self::InternalFactOnly,
        Self::AnalysisAndDiagnostic,
        Self::BoundedExecution,
        Self::AuthorizedSourceEdit,
    ];

    /// Stable canonical tag.
    pub fn tag(self) -> &'static str {
        match self {
            Self::InternalFactOnly => "internal_fact_only",
            Self::AnalysisAndDiagnostic => "analysis_and_diagnostic",
            Self::BoundedExecution => "bounded_execution",
            Self::AuthorizedSourceEdit => "authorized_source_edit",
        }
    }

    /// Whether a law with this ceiling licenses a plan claiming `claimed`.
    ///
    /// A plan may always claim less by dropping to internal facts, and may
    /// claim its law's own ceiling. It may never cross to a sibling ceiling:
    /// a bounded-execution law does not license diagnostic consumption.
    pub fn permits(self, claimed: Self) -> bool {
        claimed == self || claimed == Self::InternalFactOnly
    }

    /// Consumers a plan claiming this ceiling may serve.
    ///
    /// The ceiling and the consumer set have to agree, or a plan could drop to
    /// `InternalFactOnly` while still naming diagnostic, execution or edit
    /// consumers -- claiming less and serving the same.
    /// [`ConsumerClass::NoConsumer`] claims nothing, so every ceiling admits it.
    pub fn permitted_consumers(self) -> BTreeSet<ConsumerClass> {
        let permitted: &[ConsumerClass] = match self {
            Self::InternalFactOnly => {
                &[ConsumerClass::InternalStageRewrite, ConsumerClass::FactStore]
            }
            Self::AnalysisAndDiagnostic => &[
                ConsumerClass::InternalStageRewrite,
                ConsumerClass::FactStore,
                ConsumerClass::Analysis,
                ConsumerClass::Diagnostic,
            ],
            Self::BoundedExecution => &[
                ConsumerClass::InternalStageRewrite,
                ConsumerClass::FactStore,
                ConsumerClass::BoundedExecution,
            ],
            Self::AuthorizedSourceEdit => &[ConsumerClass::SourceEdit],
        };
        permitted.iter().copied().chain([ConsumerClass::NoConsumer]).collect()
    }
}

/// Truth state of one precondition against one exact subject.
///
/// Only [`Self::ProvenExact`] discharges a precondition.  Unknown and dynamic
/// state are explicit typed values, never an absent or false precondition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreconditionTruth {
    /// Proven exactly for the plan's subject.
    ProvenExact,
    /// Not proven either way.
    Unknown,
    /// Governed by a named dynamic or unsupported concept.
    DynamicOrUnsupported(DynamicConcept),
}

impl PreconditionTruth {
    /// True only for [`Self::ProvenExact`].
    ///
    /// Unknown is not false, and a dynamic concept is not pure: neither may be
    /// optimistically read as satisfying exact legality.
    pub fn satisfies_exact_legality(self) -> bool {
        matches!(self, Self::ProvenExact)
    }

    fn write_canonical(self, out: &mut String) {
        match self {
            Self::ProvenExact => out.push_str("proven_exact"),
            Self::Unknown => out.push_str("unknown"),
            Self::DynamicOrUnsupported(concept) => {
                let _ = write!(out, "dynamic_or_unsupported({})", concept.tag());
            }
        }
    }
}

/// Independent oracle admitted for an equivalence obligation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EquivalenceOracle {
    /// Independently produced per-stage gold.
    IndependentStageGold,
    /// An original upstream case.
    OriginalUpstreamCase,
    /// A provenance-preserving minimized upstream case.
    MinimizedUpstreamCase,
    /// A version-bound structural relation.
    StructuralRelation,
    /// Bounded real-Perl behavior.
    BoundedRealPerlBehavior,
    /// A deliberate verifier mutation.
    VerifierMutation,
    /// The candidate's own transformed output.  Circular; never independent.
    TransformedCandidateOutput,
}

impl EquivalenceOracle {
    /// Closed list of every oracle, including the circular one this contract
    /// names so that it can be rejected rather than silently admitted.
    pub const ALL: [Self; 7] = [
        Self::IndependentStageGold,
        Self::OriginalUpstreamCase,
        Self::MinimizedUpstreamCase,
        Self::StructuralRelation,
        Self::BoundedRealPerlBehavior,
        Self::VerifierMutation,
        Self::TransformedCandidateOutput,
    ];

    /// Stable canonical tag.
    pub fn tag(self) -> &'static str {
        match self {
            Self::IndependentStageGold => "independent_stage_gold",
            Self::OriginalUpstreamCase => "original_upstream_case",
            Self::MinimizedUpstreamCase => "minimized_upstream_case",
            Self::StructuralRelation => "structural_relation",
            Self::BoundedRealPerlBehavior => "bounded_real_perl_behavior",
            Self::VerifierMutation => "verifier_mutation",
            Self::TransformedCandidateOutput => "transformed_candidate_output",
        }
    }

    /// False only for the candidate's own transformed output.
    pub fn is_independent(self) -> bool {
        !matches!(self, Self::TransformedCandidateOutput)
    }
}

/// Whether a law admits independent complete subplans.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PartialApplicationPolicy {
    /// Partial application is prohibited: the plan applies completely or not
    /// at all.
    Prohibited,
    /// The law explicitly defines named independent complete subplans.
    IndependentCompleteSubplans(BTreeSet<String>),
}

impl PartialApplicationPolicy {
    /// Construct a subplan policy with a non-empty named subplan set.
    pub fn independent_subplans(names: &[&str]) -> Result<Self> {
        if names.is_empty() {
            bail!("independent subplan policy must name at least one subplan");
        }
        let mut set = BTreeSet::new();
        for name in names {
            non_empty("independent subplan name", name)?;
            set.insert((*name).to_owned());
        }
        Ok(Self::IndependentCompleteSubplans(set))
    }

    /// True when a residual boundary may be declared instead of applying
    /// every selected location.
    pub fn admits_residual(&self) -> bool {
        matches!(self, Self::IndependentCompleteSubplans(_))
    }

    fn validate(&self) -> Result<()> {
        match self {
            Self::Prohibited => Ok(()),
            Self::IndependentCompleteSubplans(names) => {
                if names.is_empty() {
                    bail!("independent subplan policy must name at least one subplan");
                }
                for name in names {
                    non_empty("independent subplan name", name)?;
                }
                Ok(())
            }
        }
    }

    fn write_canonical(&self, out: &mut String) {
        match self {
            Self::Prohibited => out.push_str("prohibited"),
            Self::IndependentCompleteSubplans(names) => {
                out.push_str("independent_complete_subplans[");
                for name in names {
                    let _ = write!(out, "{name:?},");
                }
                out.push(']');
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Law components
// ---------------------------------------------------------------------------

/// `compiler_transformation_law.v1`: one reviewed semantic rewrite rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransformationLaw {
    /// Exact law identity.
    pub id: LawId,
    /// Exact law version.
    pub version: LawVersion,
    /// Closed transformation class.
    pub class: TransformationClass,
    /// Human-readable statement of the rule.
    pub statement: String,
    /// Exact stage the law consumes.
    pub input_stage: CompilerStage,
    /// Exact stage the law produces.
    pub output_stage: CompilerStage,
    /// Preconditions every plan must instantiate and discharge.
    pub required_preconditions: BTreeSet<PreconditionId>,
    /// Propositions that are load-bearing for this law.
    pub load_bearing_preservations: BTreeSet<PreservedProposition>,
    /// Changes the law permits a plan to intend.
    pub permitted_changes: BTreeSet<ChangedProposition>,
    /// Dynamic or unsupported concepts the law excludes.
    pub excluded_concepts: BTreeSet<DynamicConcept>,
    /// Whether independent complete subplans exist.
    pub partial_application: PartialApplicationPolicy,
    /// Consumers this law may serve.
    pub consumers: BTreeSet<ConsumerClass>,
    /// Highest claim this law can reach.
    pub claim_ceiling: ClaimCeiling,
}

impl TransformationLaw {
    /// Validate every closed-vocabulary and legality invariant of the law.
    pub fn validate(&self) -> Result<()> {
        non_empty("transformation law statement", &self.statement)?;
        self.partial_application.validate()?;
        if self.required_preconditions.is_empty()
            && self.class != TransformationClass::UnsupportedOrNotApplicable
        {
            bail!("law {:?} must name at least one required precondition", self.id.as_str());
        }
        if self.permitted_changes.is_empty()
            && self.class != TransformationClass::UnsupportedOrNotApplicable
        {
            bail!("law {:?} must permit at least one intended change", self.id.as_str());
        }
        let forbidden = self.class.forbidden_changes();
        for change in &self.permitted_changes {
            if forbidden.contains(change) {
                bail!(
                    "law {:?} of class {} must not permit the change {}",
                    self.id.as_str(),
                    self.class.tag(),
                    change.tag()
                );
            }
        }
        let permitted_consumers = self.class.permitted_consumers();
        for consumer in &self.consumers {
            if !permitted_consumers.contains(consumer) {
                bail!(
                    "law {:?} of class {} must not name the consumer {}",
                    self.id.as_str(),
                    self.class.tag(),
                    consumer.tag()
                );
            }
        }
        if self.consumers.is_empty() {
            bail!("law {:?} must name at least one consumer class", self.id.as_str());
        }
        if self.claim_ceiling == ClaimCeiling::AuthorizedSourceEdit
            && self.class != TransformationClass::SourceProjectionCandidate
        {
            bail!(
                "law {:?} of class {} must not reach the authorized-source-edit ceiling",
                self.id.as_str(),
                self.class.tag()
            );
        }
        if self.class == TransformationClass::FactStrengtheningWithoutIrRewrite
            && self.input_stage != self.output_stage
        {
            bail!(
                "law {:?} strengthens facts without an IR rewrite, so its input and output stage must match",
                self.id.as_str()
            );
        }
        Ok(())
    }

    /// Deterministic canonical semantic text of the law.
    pub fn canonical_semantic_text(&self) -> Result<String> {
        self.validate()?;
        let mut out = String::new();
        let _ = writeln!(out, "law {:?}", self.id.as_str());
        let _ = writeln!(out, "version {:?}", self.version.as_str());
        let _ = writeln!(out, "class {}", self.class.tag());
        let _ = writeln!(out, "statement {:?}", self.statement);
        let _ = writeln!(out, "input_stage {}", self.input_stage.tag());
        let _ = writeln!(out, "output_stage {}", self.output_stage.tag());
        out.push_str("required_preconditions[");
        for id in &self.required_preconditions {
            let _ = write!(out, "{:?},", id.as_str());
        }
        out.push_str("]\nload_bearing_preservations[");
        for proposition in &self.load_bearing_preservations {
            let _ = write!(out, "{},", proposition.tag());
        }
        out.push_str("]\npermitted_changes[");
        for change in &self.permitted_changes {
            let _ = write!(out, "{},", change.tag());
        }
        out.push_str("]\nexcluded_concepts[");
        for concept in &self.excluded_concepts {
            let _ = write!(out, "{},", concept.tag());
        }
        out.push_str("]\npartial_application ");
        self.partial_application.write_canonical(&mut out);
        out.push_str("\nconsumers[");
        for consumer in &self.consumers {
            let _ = write!(out, "{},", consumer.tag());
        }
        let _ = writeln!(out, "]\nclaim_ceiling {}", self.claim_ceiling.tag());
        bounded_canonical("law", self.id.as_str(), out)
    }

    /// Deterministic semantic fingerprint over [`Self::canonical_semantic_text`].
    pub fn semantic_fingerprint(&self) -> Result<ContractDigest> {
        fingerprint(&self.canonical_semantic_text()?)
    }

    /// Binding a plan must carry to reference this exact law revision.
    pub fn binding(&self) -> Result<LawBinding> {
        Ok(LawBinding {
            id: self.id.clone(),
            version: self.version.clone(),
            digest: self.semantic_fingerprint()?,
        })
    }
}

/// Exact reference from a plan to one law revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LawBinding {
    /// Referenced law identity.
    pub id: LawId,
    /// Referenced law version.
    pub version: LawVersion,
    /// Semantic fingerprint of the referenced law revision.
    pub digest: ContractDigest,
}

impl LawBinding {
    fn write_canonical(&self, out: &mut String) {
        let _ = writeln!(
            out,
            "law_binding id={:?} version={:?} digest={:?}",
            self.id.as_str(),
            self.version.as_str(),
            self.digest.as_str()
        );
    }
}

// ---------------------------------------------------------------------------
// Plan components
// ---------------------------------------------------------------------------

/// Exact candidate subject a plan is bound to.
///
/// Every dimension is load-bearing: a change in any of them creates another
/// plan subject rather than permitting reuse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransformationSubject {
    /// Exact candidate identity.
    pub candidate: SubjectRef,
    /// Exact source identity.
    pub source: SubjectRef,
    /// Compiler generation of the input facts.
    pub generation: Generation,
    /// Compiler operating profile.
    pub profile: SubjectRef,
    /// Exact Perl version subject.
    pub perl_version: SubjectRef,
    /// Exact platform subject.
    pub platform: SubjectRef,
    /// Exact capability subject.
    pub capability: SubjectRef,
}

impl TransformationSubject {
    /// True when every non-generation dimension matches.
    ///
    /// The generation is deliberately excluded: a generation change is
    /// staleness, which is a different terminal result from a subject
    /// mismatch.
    pub fn matches_ignoring_generation(&self, other: &Self) -> bool {
        self.candidate == other.candidate
            && self.source == other.source
            && self.profile == other.profile
            && self.perl_version == other.perl_version
            && self.platform == other.platform
            && self.capability == other.capability
    }

    fn write_canonical(&self, out: &mut String) {
        let _ = writeln!(
            out,
            "subject candidate={:?} source={:?} generation={} profile={:?} perl_version={:?} platform={:?} capability={:?}",
            self.candidate.as_str(),
            self.source.as_str(),
            self.generation.0,
            self.profile.as_str(),
            self.perl_version.as_str(),
            self.platform.as_str(),
            self.capability.as_str()
        );
    }
}

/// Exact identity of one stage's IR or fact subject.
///
/// The digest covers everything the named subject asserts at that stage --
/// its IR shape *and* the facts attached to it -- not the IR shape alone. A
/// transformation that strengthens a fact without rewriting any IR therefore
/// still produces a different output digest; only an attempt that changed
/// nothing at all reproduces the input digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageSubject {
    /// Stage that owns the subject.
    pub stage: CompilerStage,
    /// Exact IR or fact identity.
    pub ir_identity: SubjectRef,
    /// Digest of everything the subject asserts at its stage.
    pub digest: ContractDigest,
}

impl StageSubject {
    fn write_canonical(&self, label: &str, out: &mut String) {
        let _ = writeln!(
            out,
            "{label} stage={} identity={:?} digest={:?}",
            self.stage.tag(),
            self.ir_identity.as_str(),
            self.digest.as_str()
        );
    }
}

/// Relative source provenance retained alongside an operation identity.
///
/// This is provenance for diagnostics and edits, never selection identity.
/// It carries a workspace-relative path and a byte span; it never carries
/// source text or a host-absolute path, so canonical bytes stay private-safe.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SourceProvenance {
    relative_path: String,
    start_byte: u32,
    end_byte: u32,
}

impl SourceProvenance {
    /// Construct provenance from a workspace-relative path and a byte span.
    ///
    /// Rejects absolute paths, parent traversal, Windows drive prefixes, and
    /// any embedded newline (the shape a raw source excerpt would take).
    pub fn new(relative_path: &str, start_byte: u32, end_byte: u32) -> Result<Self> {
        non_empty("source provenance path", relative_path)?;
        if relative_path.starts_with('/') || relative_path.starts_with('\\') {
            bail!("source provenance path {relative_path:?} must be workspace-relative");
        }
        if relative_path.contains(':') {
            bail!("source provenance path {relative_path:?} must not carry a host drive prefix");
        }
        if relative_path.split(['/', '\\']).any(|segment| segment == "..") {
            bail!(
                "source provenance path {relative_path:?} must not traverse outside the workspace"
            );
        }
        if relative_path.contains(['\n', '\r']) {
            bail!("source provenance path {relative_path:?} must not embed source text");
        }
        if end_byte < start_byte {
            bail!("source provenance span {start_byte}..{end_byte} must be non-decreasing");
        }
        Ok(Self { relative_path: relative_path.to_owned(), start_byte, end_byte })
    }

    /// Borrow the workspace-relative path.
    pub fn relative_path(&self) -> &str {
        &self.relative_path
    }

    /// Byte span of the provenance.
    pub fn span(&self) -> (u32, u32) {
        (self.start_byte, self.end_byte)
    }

    fn write_canonical(&self, out: &mut String) {
        let _ = write!(
            out,
            "provenance({:?},{},{})",
            self.relative_path, self.start_byte, self.end_byte
        );
    }
}

/// How a plan selects one transformation location.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum LocationSelector {
    /// A stable operation identity inside the fixed canonical IR subject,
    /// optionally annotated with relative source provenance.
    CanonicalOperation {
        /// Stage that owns the operation.
        stage: CompilerStage,
        /// Stable operation identity.
        operation_id: OperationId,
        /// Optional relative source provenance.
        source_provenance: Option<SourceProvenance>,
    },
    /// Selection expressed only as a source text range.  Rejected: once the
    /// canonical IR subject is fixed, source ranges are provenance, not
    /// identity.
    SourceTextRange(SourceProvenance),
    /// Selection expressed only as a property of current transformed output.
    /// Rejected: that makes the plan's own result its selection authority.
    CurrentOutputShape(String),
}

impl LocationSelector {
    /// Stable operation identity, if this selector has one.
    pub fn operation_id(&self) -> Option<&OperationId> {
        match self {
            Self::CanonicalOperation { operation_id, .. } => Some(operation_id),
            Self::SourceTextRange(_) | Self::CurrentOutputShape(_) => None,
        }
    }

    /// Validate the selector against the plan's fixed input stage.
    pub fn validate(&self, input_stage: CompilerStage) -> Result<()> {
        match self {
            Self::CanonicalOperation { stage, operation_id, source_provenance } => {
                if *stage != input_stage {
                    bail!(
                        "location {:?} is owned by stage {} but the plan's input stage is {}",
                        operation_id.as_str(),
                        stage.tag(),
                        input_stage.tag()
                    );
                }
                if let Some(provenance) = source_provenance {
                    non_empty("source provenance path", &provenance.relative_path)?;
                }
                Ok(())
            }
            Self::SourceTextRange(provenance) => bail!(
                "location selection by source range {:?} is not a plan identity; select a canonical operation",
                provenance.relative_path
            ),
            Self::CurrentOutputShape(shape) => bail!(
                "location selection by current transformed output {shape:?} is circular; select a canonical operation"
            ),
        }
    }

    fn write_canonical(&self, out: &mut String) {
        match self {
            // Provenance is deliberately absent: the contract defines a
            // location's identity as its canonical operation, and provenance
            // as an annotation for diagnostics and edits. Hashing it would
            // make an unchanged plan's identity move when source shifts.
            Self::CanonicalOperation { stage, operation_id, source_provenance: _ } => {
                let _ =
                    write!(out, "canonical_operation({},{:?})", stage.tag(), operation_id.as_str());
            }
            Self::SourceTextRange(provenance) => {
                out.push_str("source_text_range(");
                provenance.write_canonical(out);
                out.push(')');
            }
            Self::CurrentOutputShape(shape) => {
                let _ = write!(out, "current_output_shape({shape:?})");
            }
        }
    }
}

/// One precondition a plan must discharge, with the stage that owns its
/// evidence and its current truth.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Precondition {
    /// Exact precondition identity, matching a law-required identity.
    pub id: PreconditionId,
    /// Statement of the precondition.
    pub statement: String,
    /// Stage at which the precondition is stated.
    pub stage: CompilerStage,
    /// Stage at which the discharging evidence was gathered.
    pub evidence_stage: CompilerStage,
    /// Exact reference to that evidence.
    pub evidence: SubjectRef,
    /// Current truth of the precondition for the plan's subject.
    pub truth: PreconditionTruth,
}

impl Precondition {
    fn validate(&self) -> Result<()> {
        non_empty("precondition statement", &self.statement)?;
        if self.stage != self.evidence_stage {
            bail!(
                "precondition {:?} is stated at stage {} but its evidence was gathered at stage {}; one stage cannot borrow another stage's proof",
                self.id.as_str(),
                self.stage.tag(),
                self.evidence_stage.tag()
            );
        }
        Ok(())
    }

    fn write_canonical(&self, out: &mut String) {
        let _ = write!(
            out,
            "precondition id={:?} statement={:?} stage={} evidence_stage={} evidence={:?} truth=",
            self.id.as_str(),
            self.statement,
            self.stage.tag(),
            self.evidence_stage.tag(),
            self.evidence.as_str()
        );
        self.truth.write_canonical(out);
        out.push('\n');
    }
}

/// One equivalence obligation: an independent oracle for one preserved
/// proposition on one exact subject.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct EquivalenceObligation {
    /// Independent oracle that discharges the obligation.
    pub oracle: EquivalenceOracle,
    /// Proposition the obligation covers.
    pub proposition: PreservedProposition,
    /// Exact subject the obligation is evaluated against.
    pub subject: SubjectRef,
}

impl EquivalenceObligation {
    fn validate(&self) -> Result<()> {
        if !self.oracle.is_independent() {
            bail!(
                "equivalence obligation for {} uses the candidate's own transformed output as its oracle",
                self.proposition.tag()
            );
        }
        Ok(())
    }

    fn write_canonical(&self, out: &mut String) {
        let _ = writeln!(
            out,
            "equivalence oracle={} proposition={} subject={:?}",
            self.oracle.tag(),
            self.proposition.tag(),
            self.subject.as_str()
        );
    }
}

/// Cancellation contract of one plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CancellationContract {
    /// Cancellation is observed cooperatively within the named scope.
    Cooperative(WorkScope),
    /// The plan cannot be cancelled once started.
    NotCancellable,
}

impl CancellationContract {
    fn write_canonical(&self, out: &mut String) {
        match self {
            Self::Cooperative(scope) => {
                let _ = write!(out, "cooperative({:?})", scope.as_str());
            }
            Self::NotCancellable => out.push_str("not_cancellable"),
        }
    }
}

/// Cleanup contract of one plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CleanupContract {
    /// Cleanup is required within the named scope.
    RequiredScope(WorkScope),
    /// The plan owns nothing that needs cleaning up.
    NothingToClean,
}

impl CleanupContract {
    fn write_canonical(&self, out: &mut String) {
        match self {
            Self::RequiredScope(scope) => {
                let _ = write!(out, "required_scope({:?})", scope.as_str());
            }
            Self::NothingToClean => out.push_str("nothing_to_clean"),
        }
    }
}

/// Work, resource, cancellation, and cleanup contract of one plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkContract {
    /// Non-empty scope of the useful work the plan must perform.
    pub useful_work: WorkScope,
    /// Non-empty scope of the resource bound the plan runs inside.
    pub resource_bound: WorkScope,
    /// Cancellation behavior.
    pub cancellation: CancellationContract,
    /// Cleanup behavior.
    pub cleanup: CleanupContract,
}

impl WorkContract {
    fn write_canonical(&self, out: &mut String) {
        let _ = write!(
            out,
            "work useful={:?} resource_bound={:?} cancellation=",
            self.useful_work.as_str(),
            self.resource_bound.as_str()
        );
        self.cancellation.write_canonical(out);
        out.push_str(" cleanup=");
        self.cleanup.write_canonical(out);
        out.push('\n');
    }
}

/// Relation from a source-projection candidate to its separate canonical
/// `RefactorPlan` transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefactorPlanRelation {
    /// Identity of the immutable canonical refactor plan.
    pub refactor_plan_id: SubjectRef,
    /// Reference to the authorized-plan/edit-set equality proof.
    pub edit_set_equality: SubjectRef,
    /// Reference to the independent application result.
    pub application_proof: SubjectRef,
    /// Reference to the post-edit parse/semantic/project currentness proof.
    pub post_edit_proof: SubjectRef,
}

impl RefactorPlanRelation {
    fn write_canonical(&self, out: &mut String) {
        let _ = writeln!(
            out,
            "refactor_relation plan={:?} edit_set_equality={:?} application={:?} post_edit={:?}",
            self.refactor_plan_id.as_str(),
            self.edit_set_equality.as_str(),
            self.application_proof.as_str(),
            self.post_edit_proof.as_str()
        );
    }
}

/// What one law-declared independent subplan completes, and what it produces.
///
/// A subplan is *complete*, so it owns both halves: the exact operations it
/// applies and the exact output subject that application lands on. Without the
/// output half, a partial application could apply the right operations and
/// report an unrelated subject.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubplanBinding {
    /// Exact operations this subplan applies.
    pub operations: BTreeSet<OperationId>,
    /// Exact output subject this subplan's application produces.
    pub expected_output: StageSubject,
}

/// `compiler_transformation_plan.v1`: one law instantiated against one exact
/// subject and one exact input IR subject.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransformationPlan {
    /// Exact plan identity.
    pub id: PlanId,
    /// Exact law revision this plan instantiates.
    pub law: LawBinding,
    /// Closed transformation class, mirrored from the law.
    pub class: TransformationClass,
    /// Exact candidate subject.
    pub subject: TransformationSubject,
    /// Exact input stage subject.
    pub input: StageSubject,
    /// Expected output stage subject identity.
    pub expected_output: StageSubject,
    /// Selected locations, all at the input stage.
    pub locations: Vec<LocationSelector>,
    /// Preconditions instantiated from the law.
    pub preconditions: Vec<Precondition>,
    /// Propositions this plan preserves.
    pub preserved: BTreeSet<PreservedProposition>,
    /// Propositions this plan intends to change.
    pub intended_changes: BTreeSet<ChangedProposition>,
    /// Dynamic or unsupported concepts excluded from this plan.
    pub excluded_concepts: BTreeSet<DynamicConcept>,
    /// Independent equivalence obligations.
    pub equivalence_obligations: Vec<EquivalenceObligation>,
    /// Work, resource, cancellation, and cleanup contract.
    pub work: WorkContract,
    /// Exact operations each law-declared subplan completes.
    ///
    /// A law names its independent complete subplans but cannot bind them to
    /// operations, because operation identities belong to one exact IR
    /// subject and a law is subject-independent. The plan supplies that
    /// binding, which is what makes a subplan *complete*: without it a name is
    /// a label, and any subset of the selected locations could borrow it.
    /// Empty when the law prohibits partial application.
    pub subplans: BTreeMap<String, SubplanBinding>,
    /// Partial-application policy, mirrored from the law.
    ///
    /// The policy lives on the plan rather than on an application-time
    /// observation so that the prohibition is enforced against the law the
    /// plan is bound to, not against a claim the caller makes at application
    /// time. `verify_law_conformance` pins it to the law, and it is part of
    /// the plan's canonical bytes, so changing it creates another plan.
    pub partial_application: PartialApplicationPolicy,
    /// Consumers this plan may serve.
    pub consumers: BTreeSet<ConsumerClass>,
    /// Highest claim this plan may reach.
    pub claim_ceiling: ClaimCeiling,
    /// Separate canonical refactor relation, required for source projection.
    pub refactor_relation: Option<RefactorPlanRelation>,
}

impl TransformationPlan {
    /// Validate every closed-vocabulary and legality invariant of the plan
    /// that does not require the law itself.
    pub fn validate(&self) -> Result<()> {
        self.partial_application.validate()?;
        if self.locations.is_empty() {
            bail!("plan {:?} must select at least one location", self.id.as_str());
        }
        if self.locations.len() > MAX_SELECTED_LOCATIONS {
            bail!(
                "plan {:?} selects {} locations, above the bound of {MAX_SELECTED_LOCATIONS}",
                self.id.as_str(),
                self.locations.len()
            );
        }
        for location in &self.locations {
            location.validate(self.input.stage)?;
        }
        let mut seen_operations = BTreeSet::new();
        for location in &self.locations {
            if let Some(operation) = location.operation_id()
                && !seen_operations.insert(operation.clone())
            {
                bail!(
                    "plan {:?} selects operation {:?} more than once",
                    self.id.as_str(),
                    operation.as_str()
                );
            }
        }
        if self.preconditions.is_empty() {
            bail!("plan {:?} must instantiate at least one precondition", self.id.as_str());
        }
        let mut seen_preconditions = BTreeSet::new();
        for precondition in &self.preconditions {
            precondition.validate()?;
            if !seen_preconditions.insert(precondition.id.clone()) {
                bail!(
                    "plan {:?} instantiates precondition {:?} more than once",
                    self.id.as_str(),
                    precondition.id.as_str()
                );
            }
        }
        // A law of the unsupported class permits no change at all, so its
        // plans must intend none. Requiring a change unconditionally would
        // make every such law unconformable: a non-empty set can never be a
        // subset of the empty permitted set.
        if self.class == TransformationClass::UnsupportedOrNotApplicable {
            if !self.intended_changes.is_empty() {
                bail!(
                    "plan {:?} of class {} must intend no change",
                    self.id.as_str(),
                    self.class.tag()
                );
            }
            // The class records that a rewrite is *not* applicable, so the plan
            // must carry the precondition that makes it so. Without this the
            // refusal would depend on which precondition an author happened to
            // mark dynamic, and a fully proven plan of this class could reach
            // an applied result.
            if self
                .preconditions
                .iter()
                .all(|precondition| precondition.truth.satisfies_exact_legality())
            {
                bail!(
                    "plan {:?} of class {} must carry the unproven or dynamic precondition that makes it inapplicable",
                    self.id.as_str(),
                    self.class.tag()
                );
            }
        } else if self.intended_changes.is_empty() {
            bail!("plan {:?} must intend at least one change", self.id.as_str());
        }
        for change in &self.intended_changes {
            // A proposition cannot be preserved and changed at once.
            if let Some(overlap) = preserved_counterpart(*change)
                && self.preserved.contains(&overlap)
            {
                bail!(
                    "plan {:?} both preserves {} and intends to change {}",
                    self.id.as_str(),
                    overlap.tag(),
                    change.tag()
                );
            }
        }
        let selected_ids: BTreeSet<&OperationId> = self.selected_operations();
        match &self.partial_application {
            PartialApplicationPolicy::Prohibited => {
                if !self.subplans.is_empty() {
                    bail!(
                        "plan {:?} binds subplan operations while its law prohibits partial application",
                        self.id.as_str()
                    );
                }
            }
            PartialApplicationPolicy::IndependentCompleteSubplans(names) => {
                let bound: BTreeSet<&String> = self.subplans.keys().collect();
                let declared: BTreeSet<&String> = names.iter().collect();
                if bound != declared {
                    bail!(
                        "plan {:?} must bind operations for exactly the subplans its law declares",
                        self.id.as_str()
                    );
                }
                let mut claimed: BTreeSet<&OperationId> = BTreeSet::new();
                // Declared below: the subplans must partition the selection.
                for (name, binding) in &self.subplans {
                    if binding.expected_output.stage == self.input.stage
                        && binding.expected_output.digest == self.input.digest
                    {
                        bail!(
                            "plan {:?} binds subplan {name:?} to the unchanged input subject, but a subplan applies operations",
                            self.id.as_str()
                        );
                    }
                    if binding.expected_output.stage != self.expected_output.stage {
                        bail!(
                            "plan {:?} binds subplan {name:?} to an output at stage {}, but the plan produces stage {}",
                            self.id.as_str(),
                            binding.expected_output.stage.tag(),
                            self.expected_output.stage.tag()
                        );
                    }
                    let operations = &binding.operations;
                    if operations.is_empty() {
                        bail!("plan {:?} binds subplan {name:?} to no operation", self.id.as_str());
                    }
                    for operation in operations {
                        if !selected_ids.contains(operation) {
                            bail!(
                                "plan {:?} binds subplan {name:?} to operation {:?}, which it does not select",
                                self.id.as_str(),
                                operation.as_str()
                            );
                        }
                        // Independent means disjoint: one operation cannot be
                        // completed by two subplans.
                        if !claimed.insert(operation) {
                            bail!(
                                "plan {:?} binds operation {:?} to more than one subplan",
                                self.id.as_str(),
                                operation.as_str()
                            );
                        }
                    }
                }
                // Independent *complete* subplans partition the selection: an
                // operation bound to none of them could never be applied except
                // in the full application, which the vocabulary has no way to
                // express. Requiring full coverage removes that ambiguity.
                for operation in &selected_ids {
                    if !claimed.contains(operation) {
                        bail!(
                            "plan {:?} selects operation {:?} but binds it to no subplan",
                            self.id.as_str(),
                            operation.as_str()
                        );
                    }
                }
            }
        }
        let mut seen_obligations = BTreeSet::new();
        for obligation in &self.equivalence_obligations {
            obligation.validate()?;
            if !seen_obligations.insert(obligation.clone()) {
                bail!(
                    "plan {:?} declares the {} obligation on {} more than once",
                    self.id.as_str(),
                    obligation.oracle.tag(),
                    obligation.proposition.tag()
                );
            }
        }
        if self.consumers.is_empty() {
            bail!("plan {:?} must name at least one consumer class", self.id.as_str());
        }
        let permitted_consumers = self.class.permitted_consumers();
        for consumer in &self.consumers {
            if !permitted_consumers.contains(consumer) {
                bail!(
                    "plan {:?} of class {} must not name the consumer {}",
                    self.id.as_str(),
                    self.class.tag(),
                    consumer.tag()
                );
            }
        }
        let ceiling_consumers = self.claim_ceiling.permitted_consumers();
        for consumer in &self.consumers {
            if !ceiling_consumers.contains(consumer) {
                bail!(
                    "plan {:?} claims {} but names the consumer {}, which that ceiling does not license",
                    self.id.as_str(),
                    self.claim_ceiling.tag(),
                    consumer.tag()
                );
            }
        }
        let names_source_edit = self.consumers.contains(&ConsumerClass::SourceEdit)
            || self.claim_ceiling == ClaimCeiling::AuthorizedSourceEdit
            || self.intended_changes.contains(&ChangedProposition::SourceText);
        if names_source_edit && self.refactor_relation.is_none() {
            bail!(
                "plan {:?} projects a source edit without a separate canonical RefactorPlan relation",
                self.id.as_str()
            );
        }
        if !names_source_edit && self.refactor_relation.is_some() {
            bail!(
                "plan {:?} declares a RefactorPlan relation without projecting a source edit",
                self.id.as_str()
            );
        }
        // `StageSubject::digest` covers the IR *and* the facts attached to it,
        // so only an attempt that changed nothing at all reproduces the input
        // digest. Any intended change at the same stage therefore contradicts
        // an unchanged expected output -- not only an IR-shape change.
        if self.expected_output.stage == self.input.stage
            && self.expected_output.digest == self.input.digest
            && !self.intended_changes.is_empty()
        {
            bail!(
                "plan {:?} intends a change but expects the input subject unchanged",
                self.id.as_str()
            );
        }
        Ok(())
    }

    /// Verify the plan against the exact law revision it names.
    ///
    /// The relation is one-directional: a plan may not drop anything the law
    /// requires, and may not reach past what the law permits. It *may* carry
    /// more than the law demands — extra preconditions, preserved
    /// propositions, excluded concepts and equivalence obligations — because
    /// each of those only narrows what the plan can legally do. Strengthening
    /// is lawful; weakening is what this rejects.
    pub fn verify_law_conformance(&self, law: &TransformationLaw) -> Result<()> {
        self.validate()?;
        law.validate()?;
        let binding = law.binding()?;
        if self.law != binding {
            bail!(
                "plan {:?} binds law {:?}@{:?} digest {:?} but the supplied law is {:?}@{:?} digest {:?}",
                self.id.as_str(),
                self.law.id.as_str(),
                self.law.version.as_str(),
                self.law.digest.as_str(),
                binding.id.as_str(),
                binding.version.as_str(),
                binding.digest.as_str()
            );
        }
        if self.class != law.class {
            bail!(
                "plan {:?} declares class {} but law {:?} is class {}",
                self.id.as_str(),
                self.class.tag(),
                law.id.as_str(),
                law.class.tag()
            );
        }
        if self.input.stage != law.input_stage {
            bail!(
                "plan {:?} consumes stage {} but law {:?} consumes stage {}",
                self.id.as_str(),
                self.input.stage.tag(),
                law.id.as_str(),
                law.input_stage.tag()
            );
        }
        if self.expected_output.stage != law.output_stage {
            bail!(
                "plan {:?} produces stage {} but law {:?} produces stage {}",
                self.id.as_str(),
                self.expected_output.stage.tag(),
                law.id.as_str(),
                law.output_stage.tag()
            );
        }
        let instantiated: BTreeSet<&PreconditionId> =
            self.preconditions.iter().map(|precondition| &precondition.id).collect();
        for required in &law.required_preconditions {
            if !instantiated.contains(required) {
                bail!(
                    "plan {:?} omits law-required precondition {:?}",
                    self.id.as_str(),
                    required.as_str()
                );
            }
        }
        for proposition in &law.load_bearing_preservations {
            if !self.preserved.contains(proposition) {
                bail!(
                    "plan {:?} omits the load-bearing preservation {}",
                    self.id.as_str(),
                    proposition.tag()
                );
            }
            if !self
                .equivalence_obligations
                .iter()
                .any(|obligation| obligation.proposition == *proposition)
            {
                bail!(
                    "plan {:?} preserves {} without an independent equivalence obligation",
                    self.id.as_str(),
                    proposition.tag()
                );
            }
        }
        for change in &self.intended_changes {
            if !law.permitted_changes.contains(change) {
                bail!(
                    "plan {:?} intends the change {} which law {:?} does not permit",
                    self.id.as_str(),
                    change.tag(),
                    law.id.as_str()
                );
            }
        }
        for concept in &law.excluded_concepts {
            if !self.excluded_concepts.contains(concept) {
                bail!(
                    "plan {:?} drops the law-excluded concept {}",
                    self.id.as_str(),
                    concept.tag()
                );
            }
        }
        for consumer in &self.consumers {
            if !law.consumers.contains(consumer) {
                bail!(
                    "plan {:?} names the consumer {} which law {:?} does not admit",
                    self.id.as_str(),
                    consumer.tag(),
                    law.id.as_str()
                );
            }
        }
        if self.partial_application != law.partial_application {
            bail!(
                "plan {:?} declares its own partial-application policy; law {:?} owns it",
                self.id.as_str(),
                law.id.as_str()
            );
        }
        if !law.claim_ceiling.permits(self.claim_ceiling) {
            bail!(
                "plan {:?} claims {} which the ceiling {} of law {:?} does not permit",
                self.id.as_str(),
                self.claim_ceiling.tag(),
                law.claim_ceiling.tag(),
                law.id.as_str()
            );
        }
        Ok(())
    }

    /// Stable operation identities this plan selects.
    pub fn selected_operations(&self) -> BTreeSet<&OperationId> {
        self.locations.iter().filter_map(LocationSelector::operation_id).collect()
    }

    /// Deterministic canonical semantic text of the plan.  Location and
    /// obligation order cannot change it.
    pub fn canonical_semantic_text(&self) -> Result<String> {
        self.validate()?;
        let mut out = String::new();
        let _ = writeln!(out, "plan {:?}", self.id.as_str());
        self.law.write_canonical(&mut out);
        let _ = writeln!(out, "class {}", self.class.tag());
        self.subject.write_canonical(&mut out);
        self.input.write_canonical("input", &mut out);
        self.expected_output.write_canonical("expected_output", &mut out);
        let mut locations = self.locations.clone();
        locations.sort();
        out.push_str("locations[");
        for location in &locations {
            location.write_canonical(&mut out);
            out.push(',');
        }
        out.push_str("]\n");
        let mut preconditions = self.preconditions.clone();
        preconditions.sort_by(|a, b| a.id.cmp(&b.id));
        for precondition in &preconditions {
            precondition.write_canonical(&mut out);
        }
        out.push_str("preserved[");
        for proposition in &self.preserved {
            let _ = write!(out, "{},", proposition.tag());
        }
        out.push_str("]\nintended_changes[");
        for change in &self.intended_changes {
            let _ = write!(out, "{},", change.tag());
        }
        out.push_str("]\nexcluded_concepts[");
        for concept in &self.excluded_concepts {
            let _ = write!(out, "{},", concept.tag());
        }
        out.push_str("]\n");
        let mut obligations = self.equivalence_obligations.clone();
        obligations.sort();
        for obligation in &obligations {
            obligation.write_canonical(&mut out);
        }
        self.work.write_canonical(&mut out);
        out.push_str("subplans[");
        for (name, binding) in &self.subplans {
            let _ = write!(out, "{name:?}=(");
            for operation in &binding.operations {
                let _ = write!(out, "{:?},", operation.as_str());
            }
            let _ = write!(
                out,
                "|{}|{:?}|{:?})",
                binding.expected_output.stage.tag(),
                binding.expected_output.ir_identity.as_str(),
                binding.expected_output.digest.as_str()
            );
        }
        out.push_str("]\npartial_application ");
        self.partial_application.write_canonical(&mut out);
        out.push_str("\nconsumers[");
        for consumer in &self.consumers {
            let _ = write!(out, "{},", consumer.tag());
        }
        let _ = writeln!(out, "]\nclaim_ceiling {}", self.claim_ceiling.tag());
        match &self.refactor_relation {
            Some(relation) => relation.write_canonical(&mut out),
            None => out.push_str("refactor_relation none\n"),
        }
        bounded_canonical("plan", self.id.as_str(), out)
    }

    /// Deterministic semantic fingerprint over [`Self::canonical_semantic_text`].
    pub fn semantic_fingerprint(&self) -> Result<ContractDigest> {
        fingerprint(&self.canonical_semantic_text()?)
    }

    /// Classify one application attempt into the closed result vocabulary.
    ///
    /// The precedence is deliberate and fixed: plan validity, subject
    /// identity, currentness, settlement, legality, verifier, equivalence,
    /// then work and output.  Measured elapsed time is never consulted, so a
    /// faster attempt cannot upgrade a failed legality, verifier, or
    /// equivalence outcome.
    ///
    /// Settlement precedes legality on purpose: an attempt that was cancelled,
    /// timed out, or failed cleanup did not settle, so its precondition and
    /// verifier observations are not trustworthy evidence about the subject.
    /// The cost is that such a result reports the settlement failure and not a
    /// partial mutation that may also have occurred; a consumer auditing for
    /// law violations must therefore treat a non-`Completed` settlement as
    /// "unknown legality", never as "no violation".
    ///
    /// Within the legality step the choice is order-independent: unproven
    /// preconditions are ranked by kind and then by identity, so reordering a
    /// plan's precondition vector cannot change the reported refusal.
    pub fn evaluate(&self, observation: &ApplicationObservation) -> Result<TransformationResult> {
        // `semantic_fingerprint` subsumes `validate` and additionally requires
        // the canonical text to stay within its bound. A plan with no
        // computable identity cannot produce an applied result.
        if let Err(error) = self.semantic_fingerprint() {
            return Ok(TransformationResult::InvalidPlan { reason: truncate_reason(&error) });
        }
        if !self.subject.matches_ignoring_generation(&observation.observed_subject) {
            return Ok(TransformationResult::SubjectMismatch {
                reason: "the observed candidate subject is not the plan's subject".to_owned(),
            });
        }
        if self.input.stage != observation.observed_input.stage
            || self.input.ir_identity != observation.observed_input.ir_identity
        {
            return Ok(TransformationResult::SubjectMismatch {
                reason: "the observed input is a different stage subject from the plan's input"
                    .to_owned(),
            });
        }
        if self.subject.generation != observation.observed_subject.generation
            || self.input.digest != observation.observed_input.digest
        {
            return Ok(TransformationResult::Stale {
                reason: "the observed input generation or digest is not the plan's input"
                    .to_owned(),
            });
        }
        match &observation.settlement {
            Settlement::Completed => {}
            Settlement::Cancelled(reason) => {
                return Ok(TransformationResult::Cancelled { reason: bounded_reason(reason) });
            }
            Settlement::TimedOut(reason) => {
                return Ok(TransformationResult::TimedOut { reason: bounded_reason(reason) });
            }
            Settlement::LimitExceeded(reason) => {
                return Ok(TransformationResult::LimitExceeded { reason: bounded_reason(reason) });
            }
            Settlement::InstrumentFailed(reason) => {
                return Ok(TransformationResult::InstrumentFailed {
                    reason: bounded_reason(reason),
                });
            }
            Settlement::CleanupFailed(reason) => {
                return Ok(TransformationResult::CleanupFailed { reason: bounded_reason(reason) });
            }
        }

        // Sorted by identity, then dynamic/unsupported before merely unknown:
        // the refusal a caller sees must not depend on the order preconditions
        // happen to be declared in, and a named dynamic concept is strictly
        // more informative than "not proven".
        let mut unproven: Vec<&Precondition> = self
            .preconditions
            .iter()
            .filter(|precondition| !precondition.truth.satisfies_exact_legality())
            .collect();
        unproven.sort_by_key(|precondition| {
            let dynamic_first =
                u8::from(!matches!(precondition.truth, PreconditionTruth::DynamicOrUnsupported(_)));
            (dynamic_first, precondition.id.clone())
        });
        if let Some(first) = unproven.first() {
            // A plan that mutated locations anyway is not a clean refusal:
            // reporting it as one would hide the mutation. Preconditions are
            // plan-wide and conjunctive, so a subplan policy does not license
            // applying part of the plan while one of them is unproven.
            if !observation.applied_operations.is_empty() {
                return Ok(TransformationResult::InvalidOutput {
                    reason: bounded_reason(&format!(
                        "partial application of {} location(s) after precondition {:?} was not proven",
                        observation.applied_operations.len(),
                        first.id.as_str()
                    )),
                });
            }
            return Ok(match first.truth {
                PreconditionTruth::DynamicOrUnsupported(concept) => {
                    TransformationResult::RefusedDynamicOrUnsupported {
                        precondition: first.id.clone(),
                        concept,
                    }
                }
                // `unproven` was filtered on `!satisfies_exact_legality`, so
                // `ProvenExact` cannot appear here.
                PreconditionTruth::Unknown | PreconditionTruth::ProvenExact => {
                    TransformationResult::RefusedPreconditionUnproven {
                        precondition: first.id.clone(),
                    }
                }
            });
        }

        match &observation.verifier {
            VerifierOutcome::Passed => {}
            VerifierOutcome::Failed(reason) => {
                return Ok(TransformationResult::VerifierFailed { reason: bounded_reason(reason) });
            }
            VerifierOutcome::NotRun => {
                return Ok(TransformationResult::VerifierFailed {
                    reason: "the plan's verifier did not run".to_owned(),
                });
            }
        }
        if let EquivalenceOutcome::NotProven(reason) = &observation.equivalence {
            return Ok(TransformationResult::EquivalenceNotProven {
                reason: bounded_reason(reason),
            });
        }
        for obligation in &self.equivalence_obligations {
            if !observation.discharged_obligations.contains(obligation) {
                return Ok(TransformationResult::EquivalenceNotProven {
                    reason: bounded_reason(&format!(
                        "the {} obligation on {} was not discharged",
                        obligation.oracle.tag(),
                        obligation.proposition.tag()
                    )),
                });
            }
        }

        if observation.applied_operations.is_empty() {
            // Nothing applied must mean nothing produced: an output that moved
            // while no location was applied is a contradictory observation, and
            // reporting it as zero work would say nothing happened when the
            // subject changed.
            if let Some(output) = &observation.output
                && *output != observation.observed_input
            {
                return Ok(TransformationResult::InvalidOutput {
                    reason: "the attempt applied no location but reported a changed output"
                        .to_owned(),
                });
            }
            return Ok(TransformationResult::ZeroUsefulWork {
                reason: "the attempt applied no location".to_owned(),
            });
        }
        // The receipt is denominated in applied locations, so it can never
        // report fewer units than the attempt says it changed.
        if observation.work.useful_operations < observation.applied_operations.len() as u64 {
            return Ok(TransformationResult::InvalidOutput {
                reason: bounded_reason(&format!(
                    "the attempt applied {} location(s) but reported only {} useful operation(s)",
                    observation.applied_operations.len(),
                    observation.work.useful_operations
                )),
            });
        }
        let selected: BTreeSet<OperationId> =
            self.selected_operations().into_iter().cloned().collect();
        if !observation.applied_operations.is_subset(&selected) {
            return Ok(TransformationResult::InvalidOutput {
                reason: "the attempt applied an operation the plan did not select".to_owned(),
            });
        }
        let output = match &observation.output {
            Some(output) => output,
            None => {
                return Ok(TransformationResult::InvalidOutput {
                    reason: "the attempt reported no output stage subject".to_owned(),
                });
            }
        };
        if output.stage != self.expected_output.stage {
            return Ok(TransformationResult::InvalidOutput {
                reason: bounded_reason(&format!(
                    "the attempt produced stage {} but the plan expects stage {}",
                    output.stage.tag(),
                    self.expected_output.stage.tag()
                )),
            });
        }

        if observation.applied_operations == selected {
            // A complete application must land on the exact output subject the
            // plan declared; anything else means the plan's expected output was
            // never load-bearing.
            if output != &self.expected_output {
                return Ok(TransformationResult::InvalidOutput {
                    reason: "the attempt did not produce the plan's expected output subject"
                        .to_owned(),
                });
            }
            if observation.residual.is_some() {
                return Ok(TransformationResult::InvalidOutput {
                    reason: "the attempt declared a residual boundary after applying every selected location"
                        .to_owned(),
                });
            }
            return Ok(TransformationResult::AppliedExact {
                output: output.clone(),
                work: observation.work,
            });
        }
        // A partial application is legal only under a law that declares
        // independent complete subplans, and only for a subplan it names.
        let subplans = match &self.partial_application {
            PartialApplicationPolicy::Prohibited => {
                return Ok(TransformationResult::InvalidOutput {
                    reason: "partial application is prohibited by the plan's law".to_owned(),
                });
            }
            PartialApplicationPolicy::IndependentCompleteSubplans(subplans) => subplans,
        };
        let residual = match &observation.residual {
            Some(residual) => residual,
            None => {
                return Ok(TransformationResult::InvalidOutput {
                    reason: "a partial application declared no residual boundary".to_owned(),
                });
            }
        };
        if !subplans.contains(&residual.subplan) {
            return Ok(TransformationResult::InvalidOutput {
                reason: bounded_reason(&format!(
                    "the residual names subplan {:?}, which the plan's law does not declare",
                    residual.subplan
                )),
            });
        }
        // "Complete" is the load-bearing word: the attempt must have applied
        // exactly the operations that subplan completes, not an arbitrary
        // subset wearing its name.
        let binding = match self.subplans.get(&residual.subplan) {
            Some(binding) => binding,
            None => {
                return Ok(TransformationResult::InvalidOutput {
                    reason: bounded_reason(&format!(
                        "the plan binds no subplan {:?}",
                        residual.subplan
                    )),
                });
            }
        };
        if binding.operations != observation.applied_operations {
            return Ok(TransformationResult::InvalidOutput {
                reason: bounded_reason(&format!(
                    "the attempt did not apply exactly the operations subplan {:?} completes",
                    residual.subplan
                )),
            });
        }
        // A subplan owns its output subject just as the whole plan owns its
        // own: applying the right operations and landing somewhere else is not
        // a completed subplan.
        if output != &binding.expected_output {
            return Ok(TransformationResult::InvalidOutput {
                reason: bounded_reason(&format!(
                    "the attempt did not produce the output subject subplan {:?} declares",
                    residual.subplan
                )),
            });
        }
        Ok(TransformationResult::AppliedWithDeclaredResidualBoundary {
            output: output.clone(),
            work: observation.work,
            residual: residual.clone(),
        })
    }

    /// Classify one attempt after proving the plan conforms to its exact law.
    ///
    /// [`Self::evaluate`] is plan-local: it cannot see the law, so it presumes
    /// conformance was established beforehand. This is the safe entry point for
    /// a caller that holds the law — a non-conforming plan yields
    /// [`TransformationResult::InvalidPlan`] rather than any applied state.
    pub fn evaluate_under_law(
        &self,
        law: &TransformationLaw,
        observation: &ApplicationObservation,
    ) -> Result<TransformationResult> {
        if let Err(error) = self.verify_law_conformance(law) {
            return Ok(TransformationResult::InvalidPlan { reason: truncate_reason(&error) });
        }
        self.evaluate(observation)
    }
}

fn preserved_counterpart(change: ChangedProposition) -> Option<PreservedProposition> {
    match change {
        ChangedProposition::SourceText => Some(PreservedProposition::SourceMapping),
        ChangedProposition::ExecutionCost
        | ChangedProposition::IrShape
        | ChangedProposition::FactStrength
        | ChangedProposition::RedundantOperationCount
        | ChangedProposition::UnreachableEdgeCount => None,
    }
}

// ---------------------------------------------------------------------------
// Result contract
// ---------------------------------------------------------------------------

/// Measured work of one application attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkReceipt {
    /// Useful operations actually performed, denominated in applied locations.
    ///
    /// One unit is one selected location the attempt actually changed, so this
    /// is never below the size of the applied set. A transformation that does
    /// additional internal work counts it above that floor; it may not count
    /// it instead of the floor.
    pub useful_operations: u64,
    /// Elapsed time, retained for reporting only.
    pub elapsed_micros: u64,
}

/// How one application attempt settled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Settlement {
    /// The attempt ran to completion.
    Completed,
    /// The attempt was cancelled.
    Cancelled(String),
    /// The attempt exceeded its time budget.
    TimedOut(String),
    /// The attempt exceeded a declared limit.
    LimitExceeded(String),
    /// The instrument itself failed.
    InstrumentFailed(String),
    /// Cleanup failed after the attempt.
    CleanupFailed(String),
}

/// Verifier outcome of one application attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifierOutcome {
    /// The verifier ran and passed.
    Passed,
    /// The verifier ran and failed.
    Failed(String),
    /// The verifier did not run; absence is never a pass.
    NotRun,
}

/// Equivalence outcome of one application attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EquivalenceOutcome {
    /// Equivalence was proven for every obligation.
    Proven,
    /// Equivalence was not proven, for the named reason.
    NotProven(String),
}

/// Declared residual boundary of a partially applied plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResidualBoundary {
    /// Named subplan that was applied.
    pub subplan: String,
    /// Named boundary that was not crossed.
    pub boundary: String,
}

impl ResidualBoundary {
    /// Construct a residual boundary with a non-empty subplan and boundary.
    pub fn new(subplan: &str, boundary: &str) -> Result<Self> {
        non_empty("residual subplan", subplan)?;
        non_empty("residual boundary", boundary)?;
        Ok(Self { subplan: subplan.to_owned(), boundary: boundary.to_owned() })
    }
}

/// Everything observed about one application attempt of one plan.
///
/// This is the input to [`TransformationPlan::evaluate`]; it carries no
/// verdict of its own, so the terminal result is derived, never asserted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplicationObservation {
    /// Candidate subject observed at application time.
    pub observed_subject: TransformationSubject,
    /// Input stage subject observed at application time.
    pub observed_input: StageSubject,
    /// Operations the attempt actually applied.
    pub applied_operations: BTreeSet<OperationId>,
    /// Equivalence obligations the attempt actually discharged.
    pub discharged_obligations: BTreeSet<EquivalenceObligation>,
    /// Verifier outcome.
    pub verifier: VerifierOutcome,
    /// Equivalence outcome.
    pub equivalence: EquivalenceOutcome,
    /// Measured work.
    pub work: WorkReceipt,
    /// Settlement of the attempt.
    pub settlement: Settlement,
    /// Output stage subject, when one was produced.
    pub output: Option<StageSubject>,
    /// Residual boundary, when the attempt was partial.
    pub residual: Option<ResidualBoundary>,
}

/// `compiler_transformation_result.v1`: the closed terminal vocabulary for one
/// application attempt.
///
/// Refusal, staleness, invalidity, verifier failure, unproven equivalence, and
/// instrument failure are independently representable states.  None of them is
/// an applied transformation with an empty effect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransformationResult {
    /// Every selected location was applied and every obligation discharged.
    AppliedExact {
        /// Produced output subject.
        output: StageSubject,
        /// Measured work.
        work: WorkReceipt,
    },
    /// A law-declared independent subplan was applied, with a named residual.
    AppliedWithDeclaredResidualBoundary {
        /// Produced output subject.
        output: StageSubject,
        /// Measured work.
        work: WorkReceipt,
        /// Declared residual boundary.
        residual: ResidualBoundary,
    },
    /// A precondition was not proven exactly.
    RefusedPreconditionUnproven {
        /// The first unproven precondition.
        precondition: PreconditionId,
    },
    /// A precondition is governed by a dynamic or unsupported concept.
    RefusedDynamicOrUnsupported {
        /// The governing precondition.
        precondition: PreconditionId,
        /// The named dynamic concept.
        concept: DynamicConcept,
    },
    /// The plan's input generation or digest is no longer current.
    Stale {
        /// Why the plan is stale.
        reason: String,
    },
    /// The observed candidate subject is not the plan's subject.
    SubjectMismatch {
        /// Why the subjects differ.
        reason: String,
    },
    /// The plan itself does not satisfy its own contract.
    InvalidPlan {
        /// Why the plan is invalid.
        reason: String,
    },
    /// The attempt produced an output the plan cannot accept.
    InvalidOutput {
        /// Why the output is invalid.
        reason: String,
    },
    /// The plan's verifier failed or did not run.
    VerifierFailed {
        /// Why the verifier did not pass.
        reason: String,
    },
    /// One or more equivalence obligations were not discharged.
    EquivalenceNotProven {
        /// Which obligation is missing.
        reason: String,
    },
    /// The attempt performed no useful work.
    ZeroUsefulWork {
        /// Why no useful work occurred.
        reason: String,
    },
    /// The attempt was cancelled.
    Cancelled {
        /// Cancellation reason.
        reason: String,
    },
    /// The attempt exceeded its time budget.
    TimedOut {
        /// Timeout reason.
        reason: String,
    },
    /// The attempt exceeded a declared limit.
    LimitExceeded {
        /// Limit reason.
        reason: String,
    },
    /// The instrument itself failed.
    InstrumentFailed {
        /// Instrument failure reason.
        reason: String,
    },
    /// Cleanup failed after the attempt.
    CleanupFailed {
        /// Cleanup failure reason.
        reason: String,
    },
}

impl TransformationResult {
    /// Number of distinct terminal states in the closed result vocabulary.
    pub const VARIANT_COUNT: usize = 16;

    /// Stable canonical tag.
    pub fn tag(&self) -> &'static str {
        match self {
            Self::AppliedExact { .. } => "applied_exact",
            Self::AppliedWithDeclaredResidualBoundary { .. } => {
                "applied_with_declared_residual_boundary"
            }
            Self::RefusedPreconditionUnproven { .. } => "refused_precondition_unproven",
            Self::RefusedDynamicOrUnsupported { .. } => "refused_dynamic_or_unsupported",
            Self::Stale { .. } => "stale",
            Self::SubjectMismatch { .. } => "subject_mismatch",
            Self::InvalidPlan { .. } => "invalid_plan",
            Self::InvalidOutput { .. } => "invalid_output",
            Self::VerifierFailed { .. } => "verifier_failed",
            Self::EquivalenceNotProven { .. } => "equivalence_not_proven",
            Self::ZeroUsefulWork { .. } => "zero_useful_work",
            Self::Cancelled { .. } => "cancelled",
            Self::TimedOut { .. } => "timed_out",
            Self::LimitExceeded { .. } => "limit_exceeded",
            Self::InstrumentFailed { .. } => "instrument_failed",
            Self::CleanupFailed { .. } => "cleanup_failed",
        }
    }

    /// True only for the two applied states.
    ///
    /// Every other state, refusal included, changed nothing that a consumer
    /// may read as a transformation.
    pub fn is_applied(&self) -> bool {
        matches!(self, Self::AppliedExact { .. } | Self::AppliedWithDeclaredResidualBoundary { .. })
    }

    /// True for the two typed refusals, which are terminal and complete.
    pub fn is_refusal(&self) -> bool {
        matches!(
            self,
            Self::RefusedPreconditionUnproven { .. } | Self::RefusedDynamicOrUnsupported { .. }
        )
    }
}

// ---------------------------------------------------------------------------
// Canonical helpers
// ---------------------------------------------------------------------------

fn bounded_canonical(kind: &str, id: &str, text: String) -> Result<String> {
    if text.len() > MAX_CANONICAL_TEXT_BYTES {
        bail!(
            "canonical {kind} text for {id:?} is {} bytes, above the bound of {MAX_CANONICAL_TEXT_BYTES}",
            text.len()
        );
    }
    Ok(text)
}

fn fingerprint(canonical: &str) -> Result<ContractDigest> {
    let digest = Sha256::digest(canonical.as_bytes());
    let mut hex = String::with_capacity(64);
    for byte in digest {
        let _ = write!(hex, "{byte:02x}");
    }
    ContractDigest::from_hex(&hex).context("sha256 hex output must satisfy the digest invariant")
}

/// Bound an observation-supplied reason to the module's free-text limit.
///
/// Result reasons come from the caller, so without this the bounded-text
/// property would hold for every retained field except the result contract.
fn bounded_reason(text: &str) -> String {
    // MAX_TEXT_LEN is a byte limit, so bound bytes, not characters: 512
    // multibyte characters are well over 512 bytes. Back off to the nearest
    // char boundary so the result stays valid UTF-8.
    if text.len() <= MAX_TEXT_LEN {
        return text.to_owned();
    }
    let mut end = MAX_TEXT_LEN;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

fn truncate_reason(error: &anyhow::Error) -> String {
    bounded_reason(&format!("{error:#}"))
}

// ---------------------------------------------------------------------------
// Shape fixtures
// ---------------------------------------------------------------------------

/// Minimal representable shapes for the three initial T03 transformation
/// families and the T05b source-projection candidate.
///
/// These exist so that T02 (law registry), T03 (implementations), and T04
/// (equivalence proof) can be built against this vocabulary without inventing
/// a second one.  They are shapes, not reviewed laws: registering a reviewed
/// law is T02's claim, not this contract's.
pub mod shape_fixtures {
    use super::{
        ApplicationObservation, BTreeMap, CancellationContract, ChangedProposition, ClaimCeiling,
        CleanupContract, CompilerStage, ConsumerClass, ContractDigest, DynamicConcept,
        EquivalenceObligation, EquivalenceOracle, EquivalenceOutcome, Generation, LawId,
        LawVersion, LocationSelector, OperationId, PartialApplicationPolicy, PlanId, Precondition,
        PreconditionId, PreconditionTruth, PreservedProposition, RefactorPlanRelation, Result,
        Settlement, SourceProvenance, StageSubject, SubjectRef, SubplanBinding,
        TransformationClass, TransformationLaw, TransformationPlan, TransformationSubject,
        VerifierOutcome, WorkContract, WorkReceipt, WorkScope,
    };
    use std::collections::BTreeSet;

    fn digest(seed: u8) -> Result<ContractDigest> {
        let mut hex = String::with_capacity(64);
        for index in 0..32u8 {
            hex.push_str(&format!("{:02x}", seed.wrapping_add(index)));
        }
        ContractDigest::from_hex(&hex)
    }

    /// The exact subject every fixture is bound to.
    pub fn subject() -> Result<TransformationSubject> {
        Ok(TransformationSubject {
            candidate: SubjectRef::new("candidate/shape-fixture")?,
            source: SubjectRef::new("lib/Shape.pm")?,
            generation: Generation(7),
            profile: SubjectRef::new("compiler_static_project.v1")?,
            perl_version: SubjectRef::new("perl-5.40.0")?,
            platform: SubjectRef::new("x86_64-unknown-linux-gnu")?,
            capability: SubjectRef::new("no-xs, no-ambient-input")?,
        })
    }

    fn stage_subject(stage: CompilerStage, identity: &str, seed: u8) -> Result<StageSubject> {
        Ok(StageSubject { stage, ir_identity: SubjectRef::new(identity)?, digest: digest(seed)? })
    }

    fn proven(
        id: &str,
        statement: &str,
        stage: CompilerStage,
        evidence: &str,
    ) -> Result<Precondition> {
        Ok(Precondition {
            id: PreconditionId::new(id)?,
            statement: statement.to_owned(),
            stage,
            evidence_stage: stage,
            evidence: SubjectRef::new(evidence)?,
            truth: PreconditionTruth::ProvenExact,
        })
    }

    fn work_contract(useful: &str) -> Result<WorkContract> {
        Ok(WorkContract {
            useful_work: WorkScope::new(useful)?,
            resource_bound: WorkScope::new("bounded to the selected operations of one body")?,
            cancellation: CancellationContract::Cooperative(WorkScope::new(
                "checked between selected operations",
            )?),
            cleanup: CleanupContract::NothingToClean,
        })
    }

    /// T03a shape: exact bounded value propagation and folding in HIR.
    pub fn exact_value_folding_law() -> Result<TransformationLaw> {
        Ok(TransformationLaw {
            id: LawId::new("hir.exact-value-folding")?,
            version: LawVersion::new("v1")?,
            class: TransformationClass::AnalysisPreservingSimplification,
            statement:
                "fold an operation whose operands are exact bounded values into that exact value"
                    .to_owned(),
            input_stage: CompilerStage::Hir,
            output_stage: CompilerStage::Hir,
            required_preconditions: [
                PreconditionId::new("operands-are-exact-bounded-values")?,
                PreconditionId::new("operation-is-effect-free")?,
            ]
            .into_iter()
            .collect(),
            load_bearing_preservations: [
                PreservedProposition::Warnings,
                PreservedProposition::Exceptions,
                PreservedProposition::Effects,
                PreservedProposition::EvaluationOrder,
                PreservedProposition::Context,
                PreservedProposition::SourceMapping,
            ]
            .into_iter()
            .collect(),
            permitted_changes: [
                ChangedProposition::IrShape,
                ChangedProposition::RedundantOperationCount,
            ]
            .into_iter()
            .collect(),
            excluded_concepts: [
                DynamicConcept::Overload,
                DynamicConcept::Tie,
                DynamicConcept::Magic,
                DynamicConcept::ExternalEffect,
            ]
            .into_iter()
            .collect(),
            partial_application: PartialApplicationPolicy::Prohibited,
            consumers: [ConsumerClass::InternalStageRewrite, ConsumerClass::Analysis]
                .into_iter()
                .collect(),
            claim_ceiling: ClaimCeiling::AnalysisAndDiagnostic,
        })
    }

    /// The conforming plan for [`exact_value_folding_law`].
    pub fn exact_value_folding_plan() -> Result<TransformationPlan> {
        let law = exact_value_folding_law()?;
        let obligations = vec![
            EquivalenceObligation {
                oracle: EquivalenceOracle::IndependentStageGold,
                proposition: PreservedProposition::Warnings,
                subject: SubjectRef::new("hir gold for lib/Shape.pm")?,
            },
            EquivalenceObligation {
                oracle: EquivalenceOracle::BoundedRealPerlBehavior,
                proposition: PreservedProposition::Exceptions,
                subject: SubjectRef::new("bounded real-perl run of lib/Shape.pm")?,
            },
            EquivalenceObligation {
                oracle: EquivalenceOracle::OriginalUpstreamCase,
                proposition: PreservedProposition::Effects,
                subject: SubjectRef::new("upstream case t/op/const.t")?,
            },
            EquivalenceObligation {
                oracle: EquivalenceOracle::StructuralRelation,
                proposition: PreservedProposition::EvaluationOrder,
                subject: SubjectRef::new("structural order relation for the folded body")?,
            },
            EquivalenceObligation {
                oracle: EquivalenceOracle::StructuralRelation,
                proposition: PreservedProposition::Context,
                subject: SubjectRef::new("structural context relation for the folded body")?,
            },
            EquivalenceObligation {
                oracle: EquivalenceOracle::IndependentStageGold,
                proposition: PreservedProposition::SourceMapping,
                subject: SubjectRef::new("source map gold for the folded body")?,
            },
        ];
        Ok(TransformationPlan {
            id: PlanId::new("plan.hir.exact-value-folding.shape")?,
            law: law.binding()?,
            class: law.class,
            subject: subject()?,
            input: stage_subject(CompilerStage::Hir, "hir body lib/Shape.pm#area", 0x10)?,
            expected_output: stage_subject(
                CompilerStage::Hir,
                "hir body lib/Shape.pm#area (folded)",
                0x20,
            )?,
            locations: vec![
                LocationSelector::CanonicalOperation {
                    stage: CompilerStage::Hir,
                    operation_id: OperationId::new("hir:op:0007")?,
                    source_provenance: Some(SourceProvenance::new("lib/Shape.pm", 120, 138)?),
                },
                LocationSelector::CanonicalOperation {
                    stage: CompilerStage::Hir,
                    operation_id: OperationId::new("hir:op:0011")?,
                    source_provenance: None,
                },
            ],
            preconditions: vec![
                proven(
                    "operands-are-exact-bounded-values",
                    "both operands carry exact bounded values",
                    CompilerStage::Hir,
                    "bounded value analysis for lib/Shape.pm#area",
                )?,
                proven(
                    "operation-is-effect-free",
                    "the folded operation has no proven effect",
                    CompilerStage::Hir,
                    "effect summary for lib/Shape.pm#area",
                )?,
            ],
            preserved: law.load_bearing_preservations.clone(),
            intended_changes: [
                ChangedProposition::IrShape,
                ChangedProposition::RedundantOperationCount,
            ]
            .into_iter()
            .collect(),
            excluded_concepts: law.excluded_concepts.clone(),
            equivalence_obligations: obligations,
            work: work_contract("fold each selected exact-value operation exactly once")?,
            subplans: BTreeMap::new(),
            partial_application: law.partial_application.clone(),
            consumers: [ConsumerClass::InternalStageRewrite, ConsumerClass::Analysis]
                .into_iter()
                .collect(),
            claim_ceiling: ClaimCeiling::AnalysisAndDiagnostic,
            refactor_relation: None,
        })
    }

    /// T03b shape: branch pruning from a proven truth/definedness predicate.
    pub fn branch_pruning_law() -> Result<TransformationLaw> {
        Ok(TransformationLaw {
            id: LawId::new("pir-a.proven-branch-pruning")?,
            version: LawVersion::new("v1")?,
            class: TransformationClass::AnalysisPreservingSimplification,
            statement: "prune a branch whose guard has a proven exact truth or definedness value"
                .to_owned(),
            input_stage: CompilerStage::PirA,
            output_stage: CompilerStage::PirA,
            required_preconditions: [PreconditionId::new("guard-truth-is-proven-exact")?]
                .into_iter()
                .collect(),
            load_bearing_preservations: [
                PreservedProposition::Exceptions,
                PreservedProposition::Effects,
                PreservedProposition::EvaluationOrder,
            ]
            .into_iter()
            .collect(),
            permitted_changes: [
                ChangedProposition::IrShape,
                ChangedProposition::UnreachableEdgeCount,
            ]
            .into_iter()
            .collect(),
            excluded_concepts: [
                DynamicConcept::Overload,
                DynamicConcept::Tie,
                DynamicConcept::Magic,
                DynamicConcept::SymbolicReference,
            ]
            .into_iter()
            .collect(),
            partial_application: PartialApplicationPolicy::Prohibited,
            consumers: [ConsumerClass::InternalStageRewrite, ConsumerClass::Diagnostic]
                .into_iter()
                .collect(),
            claim_ceiling: ClaimCeiling::AnalysisAndDiagnostic,
        })
    }

    /// The conforming plan for [`branch_pruning_law`].
    pub fn branch_pruning_plan() -> Result<TransformationPlan> {
        let law = branch_pruning_law()?;
        Ok(TransformationPlan {
            id: PlanId::new("plan.pir-a.proven-branch-pruning.shape")?,
            law: law.binding()?,
            class: law.class,
            subject: subject()?,
            input: stage_subject(CompilerStage::PirA, "pir-a body lib/Shape.pm#area", 0x30)?,
            expected_output: stage_subject(
                CompilerStage::PirA,
                "pir-a body lib/Shape.pm#area (pruned)",
                0x40,
            )?,
            locations: vec![LocationSelector::CanonicalOperation {
                stage: CompilerStage::PirA,
                operation_id: OperationId::new("pir:edge:0003")?,
                source_provenance: None,
            }],
            preconditions: vec![proven(
                "guard-truth-is-proven-exact",
                "the guard's truth is proven exactly and is not overloaded, tied, or magical",
                CompilerStage::PirA,
                "truth/definedness predicate for lib/Shape.pm#area",
            )?],
            preserved: law.load_bearing_preservations.clone(),
            intended_changes: [
                ChangedProposition::IrShape,
                ChangedProposition::UnreachableEdgeCount,
            ]
            .into_iter()
            .collect(),
            excluded_concepts: law.excluded_concepts.clone(),
            equivalence_obligations: vec![
                EquivalenceObligation {
                    oracle: EquivalenceOracle::MinimizedUpstreamCase,
                    proposition: PreservedProposition::Exceptions,
                    subject: SubjectRef::new("minimized upstream case for guard exceptions")?,
                },
                EquivalenceObligation {
                    oracle: EquivalenceOracle::IndependentStageGold,
                    proposition: PreservedProposition::Effects,
                    subject: SubjectRef::new("pir-a effect gold for the pruned body")?,
                },
                EquivalenceObligation {
                    oracle: EquivalenceOracle::VerifierMutation,
                    proposition: PreservedProposition::EvaluationOrder,
                    subject: SubjectRef::new("order-mutation of the pruned body")?,
                },
            ],
            work: work_contract("prune each proven-unreachable edge exactly once")?,
            subplans: BTreeMap::new(),
            partial_application: law.partial_application.clone(),
            consumers: [ConsumerClass::InternalStageRewrite, ConsumerClass::Diagnostic]
                .into_iter()
                .collect(),
            claim_ceiling: ClaimCeiling::AnalysisAndDiagnostic,
            refactor_relation: None,
        })
    }

    /// T03c shape: effect-free unreachable-control and graph simplification.
    pub fn effect_free_control_law() -> Result<TransformationLaw> {
        Ok(TransformationLaw {
            id: LawId::new("eir.effect-free-control-simplification")?,
            version: LawVersion::new("v1")?,
            class: TransformationClass::ExecutionOptimization,
            statement:
                "remove proven-unreachable effect-free blocks and edges while the verifier still accepts the graph"
                    .to_owned(),
            input_stage: CompilerStage::Eir,
            output_stage: CompilerStage::Eir,
            required_preconditions: [
                PreconditionId::new("block-is-proven-unreachable")?,
                PreconditionId::new("block-is-effect-free")?,
            ]
            .into_iter()
            .collect(),
            load_bearing_preservations: [
                PreservedProposition::Effects,
                PreservedProposition::Cleanup,
                PreservedProposition::UnsupportedBoundary,
            ]
            .into_iter()
            .collect(),
            permitted_changes: [
                ChangedProposition::IrShape,
                ChangedProposition::ExecutionCost,
                ChangedProposition::UnreachableEdgeCount,
            ]
            .into_iter()
            .collect(),
            excluded_concepts: [DynamicConcept::ExternalEffect, DynamicConcept::ArbitraryCall]
                .into_iter()
                .collect(),
            partial_application: PartialApplicationPolicy::independent_subplans(&[
                "unreachable-blocks",
                "unreachable-edges",
            ])?,
            consumers: [ConsumerClass::InternalStageRewrite, ConsumerClass::BoundedExecution]
                .into_iter()
                .collect(),
            claim_ceiling: ClaimCeiling::BoundedExecution,
        })
    }

    /// The conforming plan for [`effect_free_control_law`].
    pub fn effect_free_control_plan() -> Result<TransformationPlan> {
        let law = effect_free_control_law()?;
        Ok(TransformationPlan {
            id: PlanId::new("plan.eir.effect-free-control-simplification.shape")?,
            law: law.binding()?,
            class: law.class,
            subject: subject()?,
            input: stage_subject(CompilerStage::Eir, "eir graph lib/Shape.pm#area", 0x50)?,
            expected_output: stage_subject(
                CompilerStage::Eir,
                "eir graph lib/Shape.pm#area (simplified)",
                0x60,
            )?,
            locations: vec![
                LocationSelector::CanonicalOperation {
                    stage: CompilerStage::Eir,
                    operation_id: OperationId::new("eir:block:0002")?,
                    source_provenance: None,
                },
                LocationSelector::CanonicalOperation {
                    stage: CompilerStage::Eir,
                    operation_id: OperationId::new("eir:edge:0005")?,
                    source_provenance: None,
                },
            ],
            preconditions: vec![
                proven(
                    "block-is-proven-unreachable",
                    "the block is proven unreachable on every path",
                    CompilerStage::Eir,
                    "eir reachability for lib/Shape.pm#area",
                )?,
                proven(
                    "block-is-effect-free",
                    "the block performs no proven effect",
                    CompilerStage::Eir,
                    "eir effect summary for lib/Shape.pm#area",
                )?,
            ],
            preserved: law.load_bearing_preservations.clone(),
            intended_changes: [
                ChangedProposition::IrShape,
                ChangedProposition::ExecutionCost,
                ChangedProposition::UnreachableEdgeCount,
            ]
            .into_iter()
            .collect(),
            excluded_concepts: law.excluded_concepts.clone(),
            equivalence_obligations: vec![
                EquivalenceObligation {
                    oracle: EquivalenceOracle::IndependentStageGold,
                    proposition: PreservedProposition::Effects,
                    subject: SubjectRef::new("eir effect gold for the simplified graph")?,
                },
                EquivalenceObligation {
                    oracle: EquivalenceOracle::BoundedRealPerlBehavior,
                    proposition: PreservedProposition::Cleanup,
                    subject: SubjectRef::new("bounded real-perl cleanup observation")?,
                },
                EquivalenceObligation {
                    oracle: EquivalenceOracle::VerifierMutation,
                    proposition: PreservedProposition::UnsupportedBoundary,
                    subject: SubjectRef::new("verifier mutation of the unsupported boundary")?,
                },
            ],
            work: work_contract("remove each proven-unreachable effect-free block or edge once")?,
            subplans: BTreeMap::from([
                (
                    "unreachable-blocks".to_owned(),
                    SubplanBinding {
                        operations: BTreeSet::from([OperationId::new("eir:block:0002")?]),
                        expected_output: stage_subject(
                            CompilerStage::Eir,
                            "eir graph lib/Shape.pm#area (blocks removed)",
                            0x61,
                        )?,
                    },
                ),
                (
                    "unreachable-edges".to_owned(),
                    SubplanBinding {
                        operations: BTreeSet::from([OperationId::new("eir:edge:0005")?]),
                        expected_output: stage_subject(
                            CompilerStage::Eir,
                            "eir graph lib/Shape.pm#area (edges removed)",
                            0x62,
                        )?,
                    },
                ),
            ]),
            partial_application: law.partial_application.clone(),
            consumers: [ConsumerClass::InternalStageRewrite, ConsumerClass::BoundedExecution]
                .into_iter()
                .collect(),
            claim_ceiling: ClaimCeiling::BoundedExecution,
            refactor_relation: None,
        })
    }

    /// T05b shape: a source-projection candidate with its separate canonical
    /// refactor relation.
    pub fn source_projection_law() -> Result<TransformationLaw> {
        Ok(TransformationLaw {
            id: LawId::new("hir.defined-or-canonical-source-projection")?,
            version: LawVersion::new("v1")?,
            class: TransformationClass::SourceProjectionCandidate,
            statement:
                "project a proven defined-or canonicalization into an authorized source edit"
                    .to_owned(),
            input_stage: CompilerStage::Hir,
            output_stage: CompilerStage::Hir,
            required_preconditions: [PreconditionId::new("defined-or-shape-is-proven-exact")?]
                .into_iter()
                .collect(),
            load_bearing_preservations: [
                PreservedProposition::Warnings,
                PreservedProposition::EvaluationOrder,
                PreservedProposition::Identity,
            ]
            .into_iter()
            .collect(),
            permitted_changes: [ChangedProposition::SourceText].into_iter().collect(),
            excluded_concepts: [DynamicConcept::Overload, DynamicConcept::Magic]
                .into_iter()
                .collect(),
            partial_application: PartialApplicationPolicy::Prohibited,
            consumers: [ConsumerClass::SourceEdit].into_iter().collect(),
            claim_ceiling: ClaimCeiling::AuthorizedSourceEdit,
        })
    }

    /// The conforming plan for [`source_projection_law`].
    pub fn source_projection_plan() -> Result<TransformationPlan> {
        let law = source_projection_law()?;
        Ok(TransformationPlan {
            id: PlanId::new("plan.hir.defined-or-canonical-source-projection.shape")?,
            law: law.binding()?,
            class: law.class,
            subject: subject()?,
            input: stage_subject(CompilerStage::Hir, "hir body lib/Shape.pm#name", 0x70)?,
            expected_output: stage_subject(
                CompilerStage::Hir,
                "hir body lib/Shape.pm#name (canonical defined-or)",
                0x80,
            )?,
            locations: vec![LocationSelector::CanonicalOperation {
                stage: CompilerStage::Hir,
                operation_id: OperationId::new("hir:op:0042")?,
                source_provenance: Some(SourceProvenance::new("lib/Shape.pm", 300, 341)?),
            }],
            preconditions: vec![proven(
                "defined-or-shape-is-proven-exact",
                "the ternary defined-check is exactly the defined-or canonical form",
                CompilerStage::Hir,
                "hir shape match for lib/Shape.pm#name",
            )?],
            preserved: law.load_bearing_preservations.clone(),
            intended_changes: [ChangedProposition::SourceText].into_iter().collect(),
            excluded_concepts: law.excluded_concepts.clone(),
            equivalence_obligations: vec![
                EquivalenceObligation {
                    oracle: EquivalenceOracle::IndependentStageGold,
                    proposition: PreservedProposition::Warnings,
                    subject: SubjectRef::new("warning gold for the projected edit")?,
                },
                EquivalenceObligation {
                    oracle: EquivalenceOracle::StructuralRelation,
                    proposition: PreservedProposition::EvaluationOrder,
                    subject: SubjectRef::new("structural order relation for the projected edit")?,
                },
                EquivalenceObligation {
                    oracle: EquivalenceOracle::BoundedRealPerlBehavior,
                    proposition: PreservedProposition::Identity,
                    subject: SubjectRef::new("bounded real-perl identity observation")?,
                },
            ],
            work: work_contract("project exactly one authorized defined-or edit")?,
            subplans: BTreeMap::new(),
            partial_application: law.partial_application.clone(),
            consumers: [ConsumerClass::SourceEdit].into_iter().collect(),
            claim_ceiling: ClaimCeiling::AuthorizedSourceEdit,
            refactor_relation: Some(RefactorPlanRelation {
                refactor_plan_id: SubjectRef::new("refactor-plan/defined-or/lib-shape-name")?,
                edit_set_equality: SubjectRef::new("authorized plan equals applied edit set")?,
                application_proof: SubjectRef::new("independent application result")?,
                post_edit_proof: SubjectRef::new("post-edit parse and project currentness")?,
            }),
        })
    }

    /// An observation under which the supplied plan applies exactly.
    ///
    /// Tests mutate one field of this conforming shape at a time, so each
    /// falsifier changes exactly one thing.
    pub fn conforming_observation(
        plan: &TransformationPlan,
        law: &TransformationLaw,
    ) -> Result<ApplicationObservation> {
        let _ = law;
        Ok(ApplicationObservation {
            observed_subject: plan.subject.clone(),
            observed_input: plan.input.clone(),
            applied_operations: plan
                .selected_operations()
                .into_iter()
                .cloned()
                .collect::<BTreeSet<OperationId>>(),
            discharged_obligations: plan.equivalence_obligations.iter().cloned().collect(),
            verifier: VerifierOutcome::Passed,
            equivalence: EquivalenceOutcome::Proven,
            work: WorkReceipt {
                useful_operations: plan.locations.len() as u64,
                elapsed_micros: 900,
            },
            settlement: Settlement::Completed,
            output: Some(plan.expected_output.clone()),
            residual: None,
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::{
        ApplicationObservation, CancellationContract, ChangedProposition, ClaimCeiling,
        CleanupContract, CompilerStage, ConsumerClass, ContractDigest, DynamicConcept,
        EquivalenceObligation, EquivalenceOracle, EquivalenceOutcome, Generation, LawId,
        LawVersion, LocationSelector, MAX_SELECTED_LOCATIONS, MAX_TEXT_LEN, OperationId,
        PartialApplicationPolicy, PlanId, Precondition, PreconditionId, PreconditionTruth,
        PreservedProposition, RefactorPlanRelation, ResidualBoundary, Result, Settlement,
        SourceProvenance, SubjectRef, SubplanBinding, TransformationClass, TransformationLaw,
        TransformationPlan, TransformationResult, VerifierOutcome, WorkReceipt, WorkScope,
        shape_fixtures,
    };
    use std::collections::{BTreeMap, BTreeSet};

    fn subject_ref(value: &str) -> SubjectRef {
        match SubjectRef::new(value) {
            Ok(reference) => reference,
            Err(error) => unreachable!("subject reference builds: {error}"),
        }
    }

    fn work_scope(value: &str) -> WorkScope {
        match WorkScope::new(value) {
            Ok(scope) => scope,
            Err(error) => unreachable!("work scope builds: {error}"),
        }
    }

    fn text_id(value: &str) -> PlanId {
        match PlanId::new(value) {
            Ok(id) => id,
            Err(error) => unreachable!("plan id builds: {error}"),
        }
    }

    fn law_version(value: &str) -> LawVersion {
        match LawVersion::new(value) {
            Ok(version) => version,
            Err(error) => unreachable!("law version builds: {error}"),
        }
    }

    /// The output subject a named subplan of `plan` declares.
    fn subplan_output(plan: &TransformationPlan, name: &str) -> super::StageSubject {
        match plan.subplans.get(name) {
            Some(binding) => binding.expected_output.clone(),
            None => unreachable!("the plan binds subplan {name}"),
        }
    }

    fn operation_id(value: &str) -> OperationId {
        match OperationId::new(value) {
            Ok(id) => id,
            Err(error) => unreachable!("operation id builds: {error}"),
        }
    }

    /// The reason text a result carries, whichever variant it is.
    fn reason_of(result: &TransformationResult) -> &str {
        match result {
            TransformationResult::Stale { reason }
            | TransformationResult::SubjectMismatch { reason }
            | TransformationResult::InvalidPlan { reason }
            | TransformationResult::InvalidOutput { reason }
            | TransformationResult::VerifierFailed { reason }
            | TransformationResult::EquivalenceNotProven { reason }
            | TransformationResult::ZeroUsefulWork { reason }
            | TransformationResult::Cancelled { reason }
            | TransformationResult::TimedOut { reason }
            | TransformationResult::LimitExceeded { reason }
            | TransformationResult::InstrumentFailed { reason }
            | TransformationResult::CleanupFailed { reason } => reason,
            TransformationResult::AppliedExact { .. }
            | TransformationResult::AppliedWithDeclaredResidualBoundary { .. }
            | TransformationResult::RefusedPreconditionUnproven { .. }
            | TransformationResult::RefusedDynamicOrUnsupported { .. } => "",
        }
    }

    fn seeded_digest(seed: u8) -> ContractDigest {
        let mut hex = String::with_capacity(64);
        for index in 0..32u8 {
            hex.push_str(&format!("{:02x}", seed.wrapping_add(index)));
        }
        match ContractDigest::from_hex(&hex) {
            Ok(digest) => digest,
            Err(error) => unreachable!("seeded digest builds: {error}"),
        }
    }

    fn folding() -> (TransformationLaw, TransformationPlan) {
        match (
            shape_fixtures::exact_value_folding_law(),
            shape_fixtures::exact_value_folding_plan(),
        ) {
            (Ok(law), Ok(plan)) => (law, plan),
            (Err(error), _) | (_, Err(error)) => {
                unreachable!("exact value folding fixture builds: {error}")
            }
        }
    }

    fn observation(plan: &TransformationPlan, law: &TransformationLaw) -> ApplicationObservation {
        match shape_fixtures::conforming_observation(plan, law) {
            Ok(observation) => observation,
            Err(error) => unreachable!("conforming observation builds: {error}"),
        }
    }

    fn assert_invalid(plan: &TransformationPlan, expected: &str, context: &str) {
        let error = match plan.validate() {
            Err(error) => error,
            Ok(()) => unreachable!("{context}"),
        };
        let text = format!("{error:#}");
        assert!(text.contains(expected), "{context}: expected {expected:?}, got {text}");
    }

    // Falsifier 1: the plan is identified only by a source range or by a
    // property of current transformed output.
    #[test]
    fn falsifier_01_plan_identity_is_never_source_range_or_current_output() -> Result<()> {
        let (_, plan) = folding();

        let mut by_range = plan.clone();
        by_range.locations = vec![LocationSelector::SourceTextRange(SourceProvenance::new(
            "lib/Shape.pm",
            120,
            138,
        )?)];
        assert_invalid(
            &by_range,
            "is not a plan identity",
            "a source range is provenance, never selection identity",
        );

        let mut by_output = plan.clone();
        by_output.locations =
            vec![LocationSelector::CurrentOutputShape("the folded constant node".to_owned())];
        assert_invalid(
            &by_output,
            "is circular",
            "current transformed output cannot select the plan's own locations",
        );

        // The admissible selector keeps provenance without letting it become
        // identity: dropping the provenance leaves the plan valid.
        let mut without_provenance = plan.clone();
        without_provenance.locations[0] = LocationSelector::CanonicalOperation {
            stage: CompilerStage::Hir,
            operation_id: OperationId::new("hir:op:0007")?,
            source_provenance: None,
        };
        without_provenance.validate()?;
        Ok(())
    }

    // Falsifier 2: unknown or dynamic preconditions are read as false, empty,
    // or pure.
    #[test]
    fn falsifier_02_unknown_and_dynamic_preconditions_never_satisfy_legality() -> Result<()> {
        assert!(PreconditionTruth::ProvenExact.satisfies_exact_legality());
        assert!(!PreconditionTruth::Unknown.satisfies_exact_legality());
        for concept in DynamicConcept::ALL {
            assert!(
                !PreconditionTruth::DynamicOrUnsupported(concept).satisfies_exact_legality(),
                "{} must never discharge an exact precondition",
                concept.tag()
            );
        }

        let (law, plan) = folding();
        let mut unknown = plan.clone();
        unknown.preconditions[0].truth = PreconditionTruth::Unknown;
        let mut observed = observation(&unknown, &law);
        observed.applied_operations = BTreeSet::new();
        observed.work = WorkReceipt { useful_operations: 0, elapsed_micros: 12 };
        match unknown.evaluate(&observed)? {
            TransformationResult::RefusedPreconditionUnproven { precondition } => {
                assert_eq!(precondition.as_str(), "operands-are-exact-bounded-values");
            }
            other => unreachable!("an unknown precondition must refuse, got {}", other.tag()),
        }

        let mut tied = plan.clone();
        tied.preconditions[0].truth = PreconditionTruth::DynamicOrUnsupported(DynamicConcept::Tie);
        let mut observed = observation(&tied, &law);
        observed.applied_operations = BTreeSet::new();
        observed.work = WorkReceipt { useful_operations: 0, elapsed_micros: 12 };
        match tied.evaluate(&observed)? {
            TransformationResult::RefusedDynamicOrUnsupported { concept, .. } => {
                assert_eq!(concept, DynamicConcept::Tie);
            }
            other => unreachable!("a tied precondition must refuse, got {}", other.tag()),
        }
        Ok(())
    }

    // Falsifier 3: parser/HIR/PIR/effects/EIR proof cross-satisfies.
    #[test]
    fn falsifier_03_one_stage_cannot_borrow_another_stages_proof() -> Result<()> {
        let (law, plan) = folding();

        let mut borrowed = plan.clone();
        borrowed.preconditions[0].evidence_stage = CompilerStage::Parser;
        assert_invalid(
            &borrowed,
            "cannot borrow another stage's proof",
            "HIR legality is not discharged by parser evidence",
        );

        let mut foreign_location = plan.clone();
        foreign_location.locations[0] = LocationSelector::CanonicalOperation {
            stage: CompilerStage::Eir,
            operation_id: OperationId::new("eir:block:0002")?,
            source_provenance: None,
        };
        assert_invalid(
            &foreign_location,
            "the plan's input stage is hir",
            "an EIR operation is not selectable by a HIR plan",
        );

        let mut wrong_input_stage = plan.clone();
        wrong_input_stage.input.stage = CompilerStage::PirA;
        wrong_input_stage.expected_output.stage = CompilerStage::PirA;
        for location in &mut wrong_input_stage.locations {
            if let LocationSelector::CanonicalOperation { stage, .. } = location {
                *stage = CompilerStage::PirA;
            }
        }
        let error = match wrong_input_stage.verify_law_conformance(&law) {
            Err(error) => format!("{error:#}"),
            Ok(()) => unreachable!("a PIR-A plan cannot instantiate a HIR law"),
        };
        assert!(error.contains("consumes stage pir_a"), "got {error}");
        Ok(())
    }

    // Falsifier 4: a load-bearing preservation selected by the law is omitted
    // by the plan, or preserved without an independent obligation.
    #[test]
    fn falsifier_04_load_bearing_preservations_cannot_be_dropped() -> Result<()> {
        let (law, plan) = folding();
        plan.verify_law_conformance(&law)?;

        let mut dropped = plan.clone();
        assert!(dropped.preserved.remove(&PreservedProposition::Warnings));
        let error = match dropped.verify_law_conformance(&law) {
            Err(error) => format!("{error:#}"),
            Ok(()) => unreachable!("a dropped load-bearing preservation must fail conformance"),
        };
        assert!(error.contains("omits the load-bearing preservation warnings"), "got {error}");

        // The falsifier names six propositions; the folding law selects all
        // six, so each one is dropped independently in both directions.
        for proposition in [
            PreservedProposition::Warnings,
            PreservedProposition::Exceptions,
            PreservedProposition::Effects,
            PreservedProposition::EvaluationOrder,
            PreservedProposition::Context,
            PreservedProposition::SourceMapping,
        ] {
            assert!(
                law.load_bearing_preservations.contains(&proposition),
                "{} must be selected by the law under test",
                proposition.tag()
            );

            let mut dropped_preservation = plan.clone();
            assert!(dropped_preservation.preserved.remove(&proposition));
            let error = match dropped_preservation.verify_law_conformance(&law) {
                Err(error) => format!("{error:#}"),
                Ok(()) => unreachable!("dropping {} must fail conformance", proposition.tag()),
            };
            assert!(
                error.contains(&format!(
                    "omits the load-bearing preservation {}",
                    proposition.tag()
                )),
                "got {error}"
            );

            let mut dropped_obligation = plan.clone();
            dropped_obligation
                .equivalence_obligations
                .retain(|obligation| obligation.proposition != proposition);
            let error = match dropped_obligation.verify_law_conformance(&law) {
                Err(error) => format!("{error:#}"),
                Ok(()) => {
                    unreachable!("dropping the {} obligation must fail", proposition.tag())
                }
            };
            assert!(
                error.contains(&format!(
                    "preserves {} without an independent equivalence obligation",
                    proposition.tag()
                )),
                "got {error}"
            );
        }
        Ok(())
    }

    // Falsifier 5: a changed input generation or profile lets a plan be
    // reused.
    #[test]
    fn falsifier_05_changed_generation_or_profile_cannot_reuse_a_plan() -> Result<()> {
        let (law, plan) = folding();
        let baseline = plan.semantic_fingerprint()?;

        let mut later_generation = plan.clone();
        later_generation.subject.generation = Generation(8);
        assert_ne!(
            later_generation.semantic_fingerprint()?,
            baseline,
            "a generation change must create another plan subject"
        );

        let mut other_profile = plan.clone();
        other_profile.subject.profile = SubjectRef::new("compiler_bounded_execution.v1")?;
        assert_ne!(other_profile.semantic_fingerprint()?, baseline);

        let mut advanced = observation(&plan, &law);
        advanced.observed_subject.generation = Generation(8);
        assert_eq!(plan.evaluate(&advanced)?.tag(), "stale");

        let mut rebuilt_input = observation(&plan, &law);
        rebuilt_input.observed_input.digest = plan.expected_output.digest.clone();
        assert_eq!(plan.evaluate(&rebuilt_input)?.tag(), "stale");

        let mut other_candidate = observation(&plan, &law);
        other_candidate.observed_subject.candidate = SubjectRef::new("candidate/other")?;
        assert_eq!(plan.evaluate(&other_candidate)?.tag(), "subject_mismatch");
        Ok(())
    }

    // Falsifier 6: the attempt silently partially applies after one failed
    // precondition.
    #[test]
    fn falsifier_06_partial_application_after_a_failed_precondition_is_not_a_refusal() -> Result<()>
    {
        let (law, plan) = folding();
        assert_eq!(law.partial_application, PartialApplicationPolicy::Prohibited);
        assert_eq!(
            plan.partial_application, law.partial_application,
            "the plan mirrors the law's policy; the caller never supplies it"
        );

        let mut half_applied = plan.clone();
        half_applied.preconditions[1].truth = PreconditionTruth::Unknown;
        let mut observed = observation(&half_applied, &law);
        observed.applied_operations = [OperationId::new("hir:op:0007")?].into_iter().collect();
        observed.work = WorkReceipt { useful_operations: 1, elapsed_micros: 400 };
        match half_applied.evaluate(&observed)? {
            TransformationResult::InvalidOutput { reason } => {
                assert!(reason.contains("partial application"), "got {reason}");
                assert!(reason.contains("operation-is-effect-free"), "got {reason}");
            }
            other => unreachable!(
                "a mutation after a failed precondition must not report a clean refusal, got {}",
                other.tag()
            ),
        }

        // The same failed precondition with nothing applied is an honest,
        // complete refusal.
        let mut untouched = observation(&half_applied, &law);
        untouched.applied_operations = BTreeSet::new();
        untouched.work = WorkReceipt { useful_operations: 0, elapsed_micros: 400 };
        assert!(half_applied.evaluate(&untouched)?.is_refusal());
        Ok(())
    }

    // Falsifier 7: refusal is represented as an applied but empty
    // transformation.
    #[test]
    fn falsifier_07_refusal_is_never_an_applied_empty_transformation() -> Result<()> {
        let (law, plan) = folding();

        // An honest zero-work observation applied nothing *and* produced
        // nothing: it reports no changed output.
        let mut empty = observation(&plan, &law);
        empty.applied_operations = BTreeSet::new();
        empty.work = WorkReceipt { useful_operations: 0, elapsed_micros: 5 };
        empty.output = None;
        let result = plan.evaluate(&empty)?;
        assert_eq!(result.tag(), "zero_useful_work");
        assert!(!result.is_applied(), "zero useful work is not an applied transformation");

        // Reporting the input subject back is the same honest shape.
        let mut unchanged_output = empty.clone();
        unchanged_output.output = Some(unchanged_output.observed_input.clone());
        assert_eq!(plan.evaluate(&unchanged_output)?.tag(), "zero_useful_work");

        // But an output that moved while nothing was applied is contradictory:
        // calling it zero work would say nothing happened to a changed subject.
        let mut moved_output = empty.clone();
        moved_output.output = Some(plan.expected_output.clone());
        match plan.evaluate(&moved_output)? {
            TransformationResult::InvalidOutput { reason } => {
                assert!(reason.contains("reported a changed output"), "got {reason}");
            }
            other => unreachable!(
                "a changed output with nothing applied must not read as zero work, got {}",
                other.tag()
            ),
        }

        let mut counted_but_unapplied = observation(&plan, &law);
        counted_but_unapplied.applied_operations = BTreeSet::new();
        counted_but_unapplied.output = None;
        assert_eq!(plan.evaluate(&counted_but_unapplied)?.tag(), "zero_useful_work");

        // Applying locations while reporting no useful work is a contradictory
        // receipt, not an honest zero-work result: calling it zero-work would
        // hand a consumer a non-applied verdict for a subject that changed.
        let mut applied_but_uncounted = observation(&plan, &law);
        applied_but_uncounted.work = WorkReceipt { useful_operations: 0, elapsed_micros: 5 };
        match plan.evaluate(&applied_but_uncounted)? {
            TransformationResult::InvalidOutput { reason } => {
                assert!(reason.contains("reported only 0 useful operation"), "got {reason}");
            }
            other => unreachable!(
                "applied locations with no reported work must not read as zero work, got {}",
                other.tag()
            ),
        }

        // Refusals produced by `evaluate` from a real failing scenario --
        // not hand-constructed values -- must not read as applied, and must
        // not be reported as an applied transformation with an empty effect.
        for truth in [
            PreconditionTruth::Unknown,
            PreconditionTruth::DynamicOrUnsupported(DynamicConcept::Magic),
        ] {
            let mut refusing = plan.clone();
            refusing.preconditions[0].truth = truth;
            let mut observed = observation(&refusing, &law);
            observed.applied_operations = BTreeSet::new();
            observed.work = WorkReceipt { useful_operations: 0, elapsed_micros: 5 };
            let result = refusing.evaluate(&observed)?;
            assert!(result.is_refusal(), "{} must be a refusal", result.tag());
            assert!(!result.is_applied(), "{} must not read as applied", result.tag());
            assert_ne!(result.tag(), "zero_useful_work", "a refusal is not zero-work");
        }
        Ok(())
    }

    // Falsifier 8: a faster result overrides failed legality or equivalence.
    #[test]
    fn falsifier_08_speed_never_overrides_failed_legality_or_equivalence() -> Result<()> {
        let (law, plan) = folding();

        let mut verifier_failed = observation(&plan, &law);
        verifier_failed.verifier = VerifierOutcome::Failed("output graph rejected".to_owned());
        let slow = plan.evaluate(&verifier_failed)?;
        verifier_failed.work = WorkReceipt { useful_operations: 2, elapsed_micros: 1 };
        let fast = plan.evaluate(&verifier_failed)?;
        assert_eq!(slow.tag(), "verifier_failed");
        assert_eq!(fast.tag(), "verifier_failed");

        let mut not_run = observation(&plan, &law);
        not_run.verifier = VerifierOutcome::NotRun;
        assert_eq!(plan.evaluate(&not_run)?.tag(), "verifier_failed");

        let mut not_proven = observation(&plan, &law);
        not_proven.equivalence = EquivalenceOutcome::NotProven("warning class differs".to_owned());
        not_proven.work = WorkReceipt { useful_operations: 2, elapsed_micros: 1 };
        assert_eq!(plan.evaluate(&not_proven)?.tag(), "equivalence_not_proven");

        let mut undischarged = observation(&plan, &law);
        undischarged
            .discharged_obligations
            .retain(|obligation| obligation.proposition != PreservedProposition::SourceMapping);
        undischarged.work = WorkReceipt { useful_operations: 2, elapsed_micros: 1 };
        match plan.evaluate(&undischarged)? {
            TransformationResult::EquivalenceNotProven { reason } => {
                assert!(reason.contains("source_mapping"), "got {reason}");
            }
            other => unreachable!("an undischarged obligation must not apply, got {}", other.tag()),
        }
        Ok(())
    }

    // Falsifier 9: source edits are permitted without a separate RefactorPlan
    // relation, or an internal class reaches the source-edit consumer.
    #[test]
    fn falsifier_09_source_edits_require_a_separate_refactor_relation() -> Result<()> {
        let projection = shape_fixtures::source_projection_plan()?;
        let projection_law = shape_fixtures::source_projection_law()?;
        projection.verify_law_conformance(&projection_law)?;

        let mut unrelated = projection.clone();
        unrelated.refactor_relation = None;
        assert_invalid(
            &unrelated,
            "without a separate canonical RefactorPlan relation",
            "a source projection without a refactor relation must fail",
        );

        // No internal class may name the source-edit consumer, whatever a law
        // declares.
        for class in TransformationClass::ALL {
            let permits_source_edit =
                class.permitted_consumers().contains(&ConsumerClass::SourceEdit);
            assert_eq!(
                permits_source_edit,
                class == TransformationClass::SourceProjectionCandidate,
                "{} must not admit the source-edit consumer",
                class.tag()
            );
        }

        let (_, folding_plan) = folding();
        let mut widened = folding_plan.clone();
        widened.consumers.insert(ConsumerClass::SourceEdit);
        assert_invalid(
            &widened,
            "must not name the consumer source_edit",
            "an analysis-preserving simplification is not a source edit",
        );

        let mut widened_law = shape_fixtures::exact_value_folding_law()?;
        widened_law.claim_ceiling = ClaimCeiling::AuthorizedSourceEdit;
        let error = match widened_law.validate() {
            Err(error) => format!("{error:#}"),
            Ok(()) => unreachable!("an internal law cannot reach the source-edit ceiling"),
        };
        assert!(error.contains("authorized-source-edit ceiling"), "got {error}");
        Ok(())
    }

    // Falsifier 10: current transformed output is used as expected proof.
    #[test]
    fn falsifier_10_transformed_output_is_never_its_own_oracle() -> Result<()> {
        let (_, plan) = folding();
        for oracle in EquivalenceOracle::ALL {
            assert_eq!(
                oracle.is_independent(),
                oracle != EquivalenceOracle::TransformedCandidateOutput,
                "{} independence is misclassified",
                oracle.tag()
            );
        }

        let mut circular = plan.clone();
        circular.equivalence_obligations.push(EquivalenceObligation {
            oracle: EquivalenceOracle::TransformedCandidateOutput,
            proposition: PreservedProposition::Effects,
            subject: SubjectRef::new("the candidate's own folded output")?,
        });
        assert_invalid(
            &circular,
            "uses the candidate's own transformed output as its oracle",
            "an obligation cannot be discharged by the result it is proving",
        );
        Ok(())
    }

    // Falsifier 11: semantic bytes change under location or obligation order.
    #[test]
    fn falsifier_11_canonical_bytes_are_order_independent_and_bounded() -> Result<()> {
        let (_, plan) = folding();
        let expected = plan.semantic_fingerprint()?;

        let mut reordered = plan.clone();
        reordered.locations.reverse();
        reordered.preconditions.reverse();
        reordered.equivalence_obligations.reverse();
        assert_eq!(
            reordered.semantic_fingerprint()?,
            expected,
            "location, precondition, and obligation order must not change plan identity"
        );

        // Order-independence is not blindness: any semantic field change must
        // move the fingerprint.
        let mutations: Vec<fn(&mut TransformationPlan)> = vec![
            |plan| {
                plan.intended_changes.remove(&ChangedProposition::RedundantOperationCount);
            },
            |plan| {
                plan.excluded_concepts.insert(DynamicConcept::XsBoundary);
            },
            |plan| plan.preconditions[0].truth = PreconditionTruth::Unknown,
            |plan| {
                plan.consumers.remove(&ConsumerClass::Analysis);
            },
            |plan| {
                plan.locations[0] = LocationSelector::CanonicalOperation {
                    stage: CompilerStage::Hir,
                    operation_id: match OperationId::new("hir:op:0099") {
                        Ok(id) => id,
                        Err(error) => unreachable!("operation id builds: {error}"),
                    },
                    source_provenance: None,
                };
            },
            // Every remaining field `canonical_semantic_text` writes is
            // load-bearing too -- the acceptance list names output and work
            // identity explicitly, so neither may be invisible to the digest.
            |plan| plan.id = text_id("plan.other"),
            |plan| plan.law.version = law_version("v9"),
            |plan| plan.subject.source = subject_ref("lib/Other.pm"),
            |plan| plan.subject.candidate = subject_ref("candidate/other"),
            |plan| plan.subject.perl_version = subject_ref("perl-5.42.0"),
            |plan| plan.subject.platform = subject_ref("aarch64-apple-darwin"),
            |plan| plan.subject.capability = subject_ref("xs-permitted"),
            |plan| plan.input.ir_identity = subject_ref("another hir body"),
            |plan| plan.expected_output.digest = seeded_digest(0xaa),
            |plan| {
                plan.preserved.remove(&PreservedProposition::Context);
            },
            |plan| plan.work.useful_work = work_scope("a different useful-work scope"),
            |plan| plan.work.resource_bound = work_scope("a different resource bound"),
            |plan| {
                plan.work.cleanup = CleanupContract::RequiredScope(work_scope("clean the graph"))
            },
            |plan| plan.work.cancellation = CancellationContract::NotCancellable,
            |plan| plan.equivalence_obligations[0].subject = subject_ref("another gold subject"),
            |plan| plan.equivalence_obligations[0].oracle = EquivalenceOracle::VerifierMutation,
            |plan| plan.preconditions[0].statement = "a different statement".to_owned(),
            |plan| plan.preconditions[0].evidence = subject_ref("different evidence"),
            |plan| {
                let selected: BTreeSet<OperationId> =
                    plan.selected_operations().into_iter().cloned().collect();
                plan.partial_application =
                    match PartialApplicationPolicy::independent_subplans(&["only-one"]) {
                        Ok(policy) => policy,
                        Err(error) => unreachable!("subplan policy builds: {error}"),
                    };
                let expected_output = plan.expected_output.clone();
                plan.subplans = BTreeMap::from([(
                    "only-one".to_owned(),
                    SubplanBinding { operations: selected, expected_output },
                )]);
            },
        ];
        for (index, mutate) in mutations.iter().enumerate() {
            let mut changed = plan.clone();
            mutate(&mut changed);
            assert_ne!(
                changed.semantic_fingerprint()?,
                expected,
                "semantic mutation {index} must change plan identity"
            );
        }

        // `class` is in the digest too. It cannot be mutated in isolation on
        // this plan -- the fixture's consumers are legal only for its current
        // class -- so narrow the consumers first and vary the class from there.
        let mut narrowed = plan.clone();
        narrowed.consumers = [ConsumerClass::InternalStageRewrite].into_iter().collect();
        let narrowed_digest = narrowed.semantic_fingerprint()?;
        let mut reclassified = narrowed.clone();
        reclassified.class = TransformationClass::InternalCanonicalization;
        assert_ne!(
            reclassified.semantic_fingerprint()?,
            narrowed_digest,
            "the transformation class must be part of plan identity"
        );

        // The claim ceiling is in the digest too, and it likewise cannot be
        // varied in isolation on the unnarrowed plan: a ceiling licenses a
        // consumer set, so the two move together.
        let mut lowered = narrowed.clone();
        lowered.claim_ceiling = ClaimCeiling::InternalFactOnly;
        assert_ne!(
            lowered.semantic_fingerprint()?,
            narrowed_digest,
            "the claim ceiling must be part of plan identity"
        );

        assert!(plan.canonical_semantic_text()?.len() <= super::MAX_CANONICAL_TEXT_BYTES);
        Ok(())
    }

    // Falsifier 12: source or private state leaks into the retained contract.
    #[test]
    fn falsifier_12_provenance_is_private_safe_and_bounded() -> Result<()> {
        for rejected in [
            "/home/someone/lib/Shape.pm",
            "\\\\host\\share\\Shape.pm",
            "C:/Users/someone/Shape.pm",
            "../../etc/passwd",
            "lib/Shape.pm\nmy $secret = 1;",
        ] {
            assert!(
                SourceProvenance::new(rejected, 0, 1).is_err(),
                "{rejected:?} must not be retainable as provenance"
            );
        }
        assert!(
            SourceProvenance::new("lib/Shape.pm", 10, 4).is_err(),
            "a decreasing span is not a span"
        );
        SourceProvenance::new("lib/Shape.pm", 10, 40)?;

        let (_, plan) = folding();
        let canonical = plan.canonical_semantic_text()?;
        assert!(!canonical.contains("/home/"), "canonical bytes must not carry a host path");
        // Every retained field is capped at MAX_TEXT_LEN, so no single line can
        // be unbounded.
        for line in canonical.lines() {
            assert!(
                line.len() <= MAX_TEXT_LEN * 8,
                "canonical line is unbounded: {} bytes",
                line.len()
            );
        }
        // The whole-text bound is enforced, not merely never approached.
        // Every individual field is legal here; it is their sum that overruns,
        // which is exactly the case a per-field cap alone would miss.
        let mut wide = plan.clone();
        wide.locations = (0..256usize)
            .map(|index| {
                let padded = format!("hir:op:{index:08}:{}", "p".repeat(MAX_TEXT_LEN - 32));
                Ok(LocationSelector::CanonicalOperation {
                    stage: CompilerStage::Hir,
                    operation_id: OperationId::new(&padded)?,
                    source_provenance: None,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let error = match wide.canonical_semantic_text() {
            Err(error) => format!("{error:#}"),
            Ok(text) => unreachable!("{} bytes must exceed the canonical bound", text.len()),
        };
        assert!(error.contains("canonical plan text"), "got {error}");
        assert!(error.contains("above the bound of"), "got {error}");

        // The selected-location bound is enforced separately, by validate,
        // before any canonical text is built.
        let mut many = plan.clone();
        many.locations = (0..=MAX_SELECTED_LOCATIONS)
            .map(|index| {
                Ok(LocationSelector::CanonicalOperation {
                    stage: CompilerStage::Hir,
                    operation_id: OperationId::new(&format!("hir:op:{index:08}"))?,
                    source_provenance: None,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        assert_invalid(&many, "locations, above the bound of", "the location bound is enforced");

        let overlong = "x".repeat(MAX_TEXT_LEN + 1);
        assert!(SubjectRef::new(&overlong).is_err(), "text fields are bounded");
        assert!(OperationId::new(&overlong).is_err(), "operation ids are bounded");
        assert!(LawId::new(&overlong).is_err(), "law ids are bounded");
        Ok(())
    }

    // Acceptance: the closed vocabularies are exactly the ones the contract
    // declares, and their tags are distinct and stable.
    #[test]
    fn closed_vocabularies_are_complete_and_distinct() -> Result<()> {
        let mut tags = BTreeSet::new();
        for stage in CompilerStage::ALL {
            assert!(tags.insert(format!("stage:{}", stage.tag())));
        }
        for class in TransformationClass::ALL {
            assert!(tags.insert(format!("class:{}", class.tag())));
            assert!(!class.permitted_consumers().is_empty());
        }
        for consumer in ConsumerClass::ALL {
            assert!(tags.insert(format!("consumer:{}", consumer.tag())));
        }
        for concept in DynamicConcept::ALL {
            assert!(tags.insert(format!("concept:{}", concept.tag())));
        }
        for proposition in PreservedProposition::ALL {
            assert!(tags.insert(format!("preserved:{}", proposition.tag())));
        }
        for change in ChangedProposition::ALL {
            assert!(tags.insert(format!("changed:{}", change.tag())));
        }
        for oracle in EquivalenceOracle::ALL {
            assert!(tags.insert(format!("oracle:{}", oracle.tag())));
        }
        for ceiling in ClaimCeiling::ALL {
            assert!(tags.insert(format!("ceiling:{}", ceiling.tag())));
        }

        let (law, plan) = folding();
        let mut result_tags = BTreeSet::new();
        for result in [
            plan.evaluate(&observation(&plan, &law))?,
            TransformationResult::AppliedWithDeclaredResidualBoundary {
                output: plan.expected_output.clone(),
                work: WorkReceipt { useful_operations: 1, elapsed_micros: 1 },
                residual: ResidualBoundary::new("unreachable-edges", "blocks left untouched")?,
            },
            TransformationResult::RefusedPreconditionUnproven {
                precondition: plan.preconditions[0].id.clone(),
            },
            TransformationResult::RefusedDynamicOrUnsupported {
                precondition: plan.preconditions[0].id.clone(),
                concept: DynamicConcept::Overload,
            },
            TransformationResult::Stale { reason: "r".to_owned() },
            TransformationResult::SubjectMismatch { reason: "r".to_owned() },
            TransformationResult::InvalidPlan { reason: "r".to_owned() },
            TransformationResult::InvalidOutput { reason: "r".to_owned() },
            TransformationResult::VerifierFailed { reason: "r".to_owned() },
            TransformationResult::EquivalenceNotProven { reason: "r".to_owned() },
            TransformationResult::ZeroUsefulWork { reason: "r".to_owned() },
            TransformationResult::Cancelled { reason: "r".to_owned() },
            TransformationResult::TimedOut { reason: "r".to_owned() },
            TransformationResult::LimitExceeded { reason: "r".to_owned() },
            TransformationResult::InstrumentFailed { reason: "r".to_owned() },
            TransformationResult::CleanupFailed { reason: "r".to_owned() },
        ] {
            assert!(result_tags.insert(result.tag()), "{} is not a distinct tag", result.tag());
        }
        assert_eq!(result_tags.len(), TransformationResult::VARIANT_COUNT);
        Ok(())
    }

    // Acceptance: every non-completed settlement is its own terminal state and
    // never collapses into a refusal or an applied transformation.
    #[test]
    fn settlement_states_remain_independently_representable() -> Result<()> {
        let (law, plan) = folding();
        for (settlement, expected) in [
            (Settlement::Cancelled("client cancelled".to_owned()), "cancelled"),
            (Settlement::TimedOut("budget exhausted".to_owned()), "timed_out"),
            (Settlement::LimitExceeded("location bound".to_owned()), "limit_exceeded"),
            (Settlement::InstrumentFailed("verifier crashed".to_owned()), "instrument_failed"),
            (Settlement::CleanupFailed("temp graph retained".to_owned()), "cleanup_failed"),
        ] {
            let mut observed = observation(&plan, &law);
            observed.settlement = settlement;
            let result = plan.evaluate(&observed)?;
            assert_eq!(result.tag(), expected);
            assert!(!result.is_applied());
            assert!(!result.is_refusal());
        }
        Ok(())
    }

    // Acceptance: T02/T03/T04 can instantiate the three initial families and
    // the source projection without a second plan vocabulary.
    #[test]
    fn shape_fixtures_instantiate_every_initial_family() -> Result<()> {
        let pairs: Vec<(TransformationLaw, TransformationPlan)> = vec![
            (
                shape_fixtures::exact_value_folding_law()?,
                shape_fixtures::exact_value_folding_plan()?,
            ),
            (shape_fixtures::branch_pruning_law()?, shape_fixtures::branch_pruning_plan()?),
            (
                shape_fixtures::effect_free_control_law()?,
                shape_fixtures::effect_free_control_plan()?,
            ),
            (shape_fixtures::source_projection_law()?, shape_fixtures::source_projection_plan()?),
        ];
        // A registry keyed by exact law revision is all T02 needs; every
        // fixture is a distinct revision with a distinct plan identity.
        let mut registry = BTreeSet::new();
        let mut plan_digests = BTreeSet::new();
        for (law, plan) in &pairs {
            law.validate()?;
            plan.verify_law_conformance(law)?;
            assert!(registry.insert((
                law.id.clone(),
                law.version.clone(),
                law.semantic_fingerprint()?
            )));
            assert!(plan_digests.insert(plan.semantic_fingerprint()?));
            let observed = shape_fixtures::conforming_observation(plan, law)?;
            assert_eq!(plan.evaluate(&observed)?.tag(), "applied_exact");
        }
        assert_eq!(registry.len(), 4);
        Ok(())
    }

    // Acceptance: a law-declared independent subplan is the only route to a
    // partial application, and it must name its residual boundary.
    #[test]
    fn residual_boundaries_require_a_law_declared_subplan() -> Result<()> {
        let law = shape_fixtures::effect_free_control_law()?;
        let plan = shape_fixtures::effect_free_control_plan()?;
        assert!(law.partial_application.admits_residual());

        let mut partial = shape_fixtures::conforming_observation(&plan, &law)?;
        partial.applied_operations = [OperationId::new("eir:block:0002")?].into_iter().collect();
        partial.work = WorkReceipt { useful_operations: 1, elapsed_micros: 300 };
        partial.residual =
            Some(ResidualBoundary::new("unreachable-blocks", "edges left untouched")?);
        partial.output = Some(subplan_output(&plan, "unreachable-blocks"));
        assert_eq!(plan.evaluate(&partial)?.tag(), "applied_with_declared_residual_boundary");

        let mut undeclared = partial.clone();
        undeclared.residual = None;
        match plan.evaluate(&undeclared)? {
            TransformationResult::InvalidOutput { reason } => {
                assert!(reason.contains("declared no residual boundary"), "got {reason}");
            }
            other => unreachable!("an undeclared residual must not apply, got {}", other.tag()),
        }

        // The same partial shape under a prohibiting plan is invalid output,
        // and the policy is the plan's own — an observation cannot launder it.
        let mut prohibited_plan = plan.clone();
        prohibited_plan.partial_application = PartialApplicationPolicy::Prohibited;
        prohibited_plan.subplans = BTreeMap::new();
        match prohibited_plan.evaluate(&partial)? {
            TransformationResult::InvalidOutput { reason } => {
                assert!(reason.contains("prohibited"), "got {reason}");
            }
            other => unreachable!("a prohibited partial must not apply, got {}", other.tag()),
        }
        // And that laundering is caught before evaluation: a plan whose policy
        // disagrees with its law fails conformance.
        let error = match prohibited_plan.verify_law_conformance(&law) {
            Err(error) => format!("{error:#}"),
            Ok(()) => unreachable!("a plan cannot restate its law's partial-application policy"),
        };
        assert!(error.contains("declares its own partial-application policy"), "got {error}");
        Ok(())
    }

    // Acceptance: a fact-strengthening law cannot rewrite IR, and an
    // unsupported law cannot change anything.
    #[test]
    fn class_change_boundaries_hold_against_a_declaring_law() -> Result<()> {
        let mut fact_only = shape_fixtures::exact_value_folding_law()?;
        fact_only.class = TransformationClass::FactStrengtheningWithoutIrRewrite;
        fact_only.consumers = [ConsumerClass::FactStore].into_iter().collect();
        let error = match fact_only.validate() {
            Err(error) => format!("{error:#}"),
            Ok(()) => unreachable!("fact strengthening cannot permit an IR rewrite"),
        };
        assert!(error.contains("must not permit the change ir_shape"), "got {error}");

        let mut unsupported = shape_fixtures::branch_pruning_law()?;
        unsupported.class = TransformationClass::UnsupportedOrNotApplicable;
        unsupported.consumers = [ConsumerClass::NoConsumer].into_iter().collect();
        let error = match unsupported.validate() {
            Err(error) => format!("{error:#}"),
            Ok(()) => unreachable!("an unsupported law cannot permit any change"),
        };
        assert!(error.contains("must not permit the change"), "got {error}");

        let mut ceiling_climb = shape_fixtures::branch_pruning_plan()?;
        ceiling_climb.claim_ceiling = ClaimCeiling::BoundedExecution;
        ceiling_climb.consumers = [ConsumerClass::InternalStageRewrite].into_iter().collect();
        let law = shape_fixtures::branch_pruning_law()?;
        let error = match ceiling_climb.verify_law_conformance(&law) {
            Err(error) => format!("{error:#}"),
            Ok(()) => unreachable!("a plan cannot cross to a sibling ceiling"),
        };
        assert!(error.contains("does not permit"), "got {error}");

        // The ceilings are siblings, not a ladder: only dropping to internal
        // facts is universally licensed.
        for law_ceiling in ClaimCeiling::ALL {
            for claimed in ClaimCeiling::ALL {
                let expected = claimed == law_ceiling || claimed == ClaimCeiling::InternalFactOnly;
                assert_eq!(
                    law_ceiling.permits(claimed),
                    expected,
                    "{} must {}permit {}",
                    law_ceiling.tag(),
                    if expected { "" } else { "not " },
                    claimed.tag()
                );
            }
        }
        assert!(
            !ClaimCeiling::BoundedExecution.permits(ClaimCeiling::AnalysisAndDiagnostic),
            "a bounded-execution law does not license diagnostic consumption"
        );

        // Dropping to internal facts is accepted.
        let mut modest = shape_fixtures::branch_pruning_plan()?;
        modest.claim_ceiling = ClaimCeiling::InternalFactOnly;
        modest.consumers = [ConsumerClass::InternalStageRewrite].into_iter().collect();
        modest.verify_law_conformance(&law)?;

        // Dropping the ceiling without dropping the consumers it licensed is
        // claiming less while serving the same, and is rejected.
        let mut modest_in_name_only = shape_fixtures::branch_pruning_plan()?;
        modest_in_name_only.claim_ceiling = ClaimCeiling::InternalFactOnly;
        assert_invalid(
            &modest_in_name_only,
            "which that ceiling does not license",
            "a lower ceiling cannot retain stronger consumers",
        );
        Ok(())
    }

    // Acceptance: a plan may not bind a law revision it does not match.
    #[test]
    fn law_binding_is_exact_revision_bound() -> Result<()> {
        let (law, plan) = folding();
        plan.verify_law_conformance(&law)?;

        let mut revised = law.clone();
        revised.statement = "a revised statement of the same rule".to_owned();
        let error = match plan.verify_law_conformance(&revised) {
            Err(error) => format!("{error:#}"),
            Ok(()) => unreachable!("a revised law must not satisfy the old binding"),
        };
        assert!(error.contains("digest"), "got {error}");

        let mut renamed = law.clone();
        renamed.version = LawVersion::new("v2")?;
        assert!(plan.verify_law_conformance(&renamed).is_err());
        assert!(LawVersion::new("2").is_err(), "law versions are v-prefixed");
        Ok(())
    }

    // Acceptance: every shape invariant `TransformationPlan::validate`
    // declares actually rejects. Without this, a plan builder could break any
    // of these and the falsifier suite would still be green.
    #[test]
    fn plan_validation_rejects_every_invalid_shape() -> Result<()> {
        let (_, plan) = folding();
        plan.validate()?;

        let cases: Vec<(&str, &str, fn(&mut TransformationPlan))> = vec![
            ("must select at least one location", "no locations", |plan| plan.locations.clear()),
            ("selects operation", "a duplicate operation", |plan| {
                plan.locations.push(plan.locations[0].clone());
            }),
            ("must instantiate at least one precondition", "no preconditions", |plan| {
                plan.preconditions.clear();
            }),
            ("instantiates precondition", "a duplicate precondition", |plan| {
                plan.preconditions.push(plan.preconditions[0].clone());
            }),
            ("must intend at least one change", "no intended change", |plan| {
                plan.intended_changes.clear();
            }),
            ("must name at least one consumer class", "no consumer", |plan| {
                plan.consumers.clear();
            }),
            (
                "declares a RefactorPlan relation without projecting a source edit",
                "an unrelated refactor relation",
                |plan| {
                    plan.refactor_relation = Some(RefactorPlanRelation {
                        refactor_plan_id: subject_ref("refactor-plan/unrelated"),
                        edit_set_equality: subject_ref("equality"),
                        application_proof: subject_ref("application"),
                        post_edit_proof: subject_ref("post-edit"),
                    });
                },
            ),
            (
                "intends a change but expects the input subject unchanged",
                "an unchanged output digest",
                |plan| plan.expected_output = plan.input.clone(),
            ),
            (
                "both preserves source_mapping and intends to change source_text",
                "a preserved-and-changed proposition",
                |plan| {
                    plan.intended_changes.insert(ChangedProposition::SourceText);
                },
            ),
            ("must be at most", "an overlong precondition statement", |plan| {
                plan.preconditions[0].statement = "x".repeat(MAX_TEXT_LEN + 1);
            }),
            ("obligation on", "a duplicate equivalence obligation", |plan| {
                plan.equivalence_obligations.push(plan.equivalence_obligations[0].clone());
            }),
        ];
        for (expected, context, mutate) in cases {
            let mut broken = plan.clone();
            mutate(&mut broken);
            assert_invalid(&broken, expected, context);
        }
        Ok(())
    }

    // Acceptance: every invariant `TransformationLaw::validate` declares
    // actually rejects. T02 validates laws standalone, before any plan exists.
    #[test]
    fn law_validation_rejects_every_invalid_shape() -> Result<()> {
        let base = shape_fixtures::exact_value_folding_law()?;
        base.validate()?;

        let cases: Vec<(&str, &str, fn(&mut TransformationLaw))> = vec![
            ("must name at least one required precondition", "no precondition", |law| {
                law.required_preconditions.clear();
            }),
            ("must permit at least one intended change", "no permitted change", |law| {
                law.permitted_changes.clear();
            }),
            ("must not name the consumer", "a consumer outside the class", |law| {
                law.consumers.insert(ConsumerClass::BoundedExecution);
            }),
            ("must name at least one consumer class", "no consumer", |law| law.consumers.clear()),
            ("must not be empty", "an empty statement", |law| law.statement = "  ".to_owned()),
            (
                "strengthens facts without an IR rewrite",
                "a fact-strengthening law that crosses stages",
                |law| {
                    law.class = TransformationClass::FactStrengtheningWithoutIrRewrite;
                    law.consumers = [ConsumerClass::FactStore].into_iter().collect();
                    law.permitted_changes =
                        [ChangedProposition::FactStrength].into_iter().collect();
                    law.output_stage = CompilerStage::Eir;
                },
            ),
            ("must name at least one subplan", "an empty subplan set", |law| {
                law.partial_application =
                    PartialApplicationPolicy::IndependentCompleteSubplans(BTreeSet::new());
            }),
            ("must not permit the change", "an unsupported law that permits a change", |law| {
                law.class = TransformationClass::UnsupportedOrNotApplicable;
                law.consumers = [ConsumerClass::NoConsumer].into_iter().collect();
            }),
        ];
        for (expected, context, mutate) in cases {
            let mut broken = base.clone();
            mutate(&mut broken);
            let error = match broken.validate() {
                Err(error) => format!("{error:#}"),
                Ok(()) => unreachable!("{context} must fail law validation"),
            };
            assert!(error.contains(expected), "{context}: expected {expected:?}, got {error}");
        }
        Ok(())
    }

    // Acceptance: every law/plan divergence `verify_law_conformance` declares
    // actually rejects. This is the gate T02/T03/T04 will call.
    #[test]
    fn conformance_rejects_every_law_plan_divergence() -> Result<()> {
        let (law, plan) = folding();
        plan.verify_law_conformance(&law)?;

        let cases: Vec<(&str, &str, fn(&mut TransformationPlan))> = vec![
            ("declares class", "a class mismatch", |plan| {
                plan.class = TransformationClass::ExecutionOptimization;
                plan.consumers = [ConsumerClass::InternalStageRewrite].into_iter().collect();
            }),
            ("produces stage", "an output-stage mismatch alone", |plan| {
                plan.expected_output.stage = CompilerStage::Eir;
            }),
            ("omits law-required precondition", "a missing law precondition", |plan| {
                plan.preconditions
                    .retain(|precondition| precondition.id.as_str() != "operation-is-effect-free");
            }),
            ("which law", "an unpermitted intended change", |plan| {
                plan.intended_changes.insert(ChangedProposition::ExecutionCost);
            }),
            ("drops the law-excluded concept", "a dropped exclusion", |plan| {
                plan.excluded_concepts.remove(&DynamicConcept::Tie);
            }),
        ];
        for (expected, context, mutate) in cases {
            let mut broken = plan.clone();
            mutate(&mut broken);
            let error = match broken.verify_law_conformance(&law) {
                Err(error) => format!("{error:#}"),
                Ok(()) => unreachable!("{context} must fail conformance"),
            };
            assert!(error.contains(expected), "{context}: expected {expected:?}, got {error}");
        }

        // A plan naming a consumer its class permits but its law does not.
        let mut narrowed_law = law.clone();
        narrowed_law.consumers = [ConsumerClass::InternalStageRewrite].into_iter().collect();
        let mut rebound = plan.clone();
        rebound.law = narrowed_law.binding()?;
        let error = match rebound.verify_law_conformance(&narrowed_law) {
            Err(error) => format!("{error:#}"),
            Ok(()) => unreachable!("a consumer outside the law must fail conformance"),
        };
        assert!(error.contains("which law"), "got {error}");
        Ok(())
    }

    // Acceptance: the identity constructors reject malformed input, so a
    // digest, subplan set, or named field cannot be empty or ill-formed.
    #[test]
    fn identity_constructors_reject_malformed_input() -> Result<()> {
        for bad in [
            String::new(),
            "0123456789abcdef".to_owned(),
            "0".repeat(63),
            "0".repeat(65),
            "g".repeat(64),
            "ABCDEF0123456789".repeat(4),
        ] {
            assert!(
                ContractDigest::from_hex(&bad).is_err(),
                "{bad:?} must not be a contract digest"
            );
        }
        ContractDigest::from_hex(&"ab".repeat(32))?;

        assert!(
            PartialApplicationPolicy::independent_subplans(&[]).is_err(),
            "a subplan policy naming nothing is not a subplan policy"
        );
        assert!(PartialApplicationPolicy::independent_subplans(&["  "]).is_err());
        PartialApplicationPolicy::independent_subplans(&["one"])?;

        for blank in ["", "   ", "\t\n"] {
            assert!(SubjectRef::new(blank).is_err(), "{blank:?} must not be a subject");
            assert!(OperationId::new(blank).is_err(), "{blank:?} must not be an operation id");
            assert!(LawId::new(blank).is_err(), "{blank:?} must not be a law id");
            assert!(WorkScope::new(blank).is_err(), "{blank:?} must not be a work scope");
            assert!(PreconditionId::new(blank).is_err(), "{blank:?} must not be a precondition id");
            assert!(PlanId::new(blank).is_err(), "{blank:?} must not be a plan id");
        }
        assert!(ResidualBoundary::new("", "boundary").is_err());
        assert!(ResidualBoundary::new("subplan", "").is_err());
        Ok(())
    }

    // Acceptance: every class that names a consumer has a valid instance, so
    // "the classes remain distinct" is proven by construction and not only by
    // rejection. An unsupported law must also admit a conforming plan: a law
    // no plan can instantiate would be an unreachable branch of the contract.
    #[test]
    fn every_transformation_class_has_a_conforming_plan() -> Result<()> {
        let (_, folding_plan) = folding();

        // Internal canonicalization: an IR rewrite serving only the internal
        // stage consumer.
        let mut canonical_law = shape_fixtures::exact_value_folding_law()?;
        canonical_law.id = LawId::new("hir.assignment-place-canonicalization")?;
        canonical_law.class = TransformationClass::InternalCanonicalization;
        canonical_law.consumers = [ConsumerClass::InternalStageRewrite].into_iter().collect();
        canonical_law.claim_ceiling = ClaimCeiling::InternalFactOnly;
        canonical_law.validate()?;

        let mut canonical_plan = folding_plan.clone();
        canonical_plan.id = text_id("plan.hir.assignment-place-canonicalization");
        canonical_plan.law = canonical_law.binding()?;
        canonical_plan.class = canonical_law.class;
        canonical_plan.consumers = canonical_law.consumers.clone();
        canonical_plan.claim_ceiling = ClaimCeiling::InternalFactOnly;
        canonical_plan.verify_law_conformance(&canonical_law)?;

        // Fact strengthening: no IR rewrite, so the plan intends only a fact
        // change and its consumers never include an internal stage rewrite.
        let mut fact_law = shape_fixtures::exact_value_folding_law()?;
        fact_law.id = LawId::new("hir.bounded-value-fact-strengthening")?;
        fact_law.class = TransformationClass::FactStrengtheningWithoutIrRewrite;
        fact_law.consumers =
            [ConsumerClass::FactStore, ConsumerClass::Analysis].into_iter().collect();
        fact_law.permitted_changes = [ChangedProposition::FactStrength].into_iter().collect();
        fact_law.validate()?;

        let mut fact_plan = folding_plan.clone();
        fact_plan.id = text_id("plan.hir.bounded-value-fact-strengthening");
        fact_plan.law = fact_law.binding()?;
        fact_plan.class = fact_law.class;
        fact_plan.consumers = fact_law.consumers.clone();
        fact_plan.intended_changes = [ChangedProposition::FactStrength].into_iter().collect();
        fact_plan.verify_law_conformance(&fact_law)?;
        let observed = shape_fixtures::conforming_observation(&fact_plan, &fact_law)?;
        assert_eq!(fact_plan.evaluate(&observed)?.tag(), "applied_exact");

        // Unsupported / not-applicable: the law permits no change, so its plan
        // must intend none. A plan that intends one is rejected, and the
        // conforming plan refuses on its dynamic precondition.
        let mut unsupported_law = shape_fixtures::exact_value_folding_law()?;
        unsupported_law.id = LawId::new("hir.overloaded-operand-not-applicable")?;
        unsupported_law.class = TransformationClass::UnsupportedOrNotApplicable;
        unsupported_law.consumers = [ConsumerClass::NoConsumer].into_iter().collect();
        unsupported_law.permitted_changes = BTreeSet::new();
        unsupported_law.claim_ceiling = ClaimCeiling::InternalFactOnly;
        unsupported_law.validate()?;

        let mut unsupported_plan = folding_plan.clone();
        unsupported_plan.id = text_id("plan.hir.overloaded-operand-not-applicable");
        unsupported_plan.law = unsupported_law.binding()?;
        unsupported_plan.class = unsupported_law.class;
        unsupported_plan.consumers = unsupported_law.consumers.clone();
        unsupported_plan.claim_ceiling = ClaimCeiling::InternalFactOnly;
        unsupported_plan.intended_changes = BTreeSet::new();
        unsupported_plan.preconditions[0].truth =
            PreconditionTruth::DynamicOrUnsupported(DynamicConcept::Overload);
        unsupported_plan.verify_law_conformance(&unsupported_law)?;

        let mut intends_a_change = unsupported_plan.clone();
        intends_a_change.intended_changes = [ChangedProposition::IrShape].into_iter().collect();
        assert_invalid(
            &intends_a_change,
            "must intend no change",
            "an unsupported plan cannot intend a change",
        );

        // The refusal is a class invariant, not an accident of this fixture's
        // chosen dynamic precondition: an unsupported plan whose preconditions
        // are all proven is rejected, so it can never reach an applied result.
        let mut all_proven = unsupported_plan.clone();
        for precondition in &mut all_proven.preconditions {
            precondition.truth = PreconditionTruth::ProvenExact;
        }
        assert_invalid(
            &all_proven,
            "must carry the unproven or dynamic precondition",
            "a fully proven unsupported plan cannot exist",
        );
        let mut applied = shape_fixtures::conforming_observation(&all_proven, &unsupported_law)?;
        applied.work = WorkReceipt { useful_operations: 2, elapsed_micros: 10 };
        match all_proven.evaluate(&applied)? {
            TransformationResult::InvalidPlan { reason } => {
                assert!(reason.contains("must carry the unproven"), "got {reason}");
            }
            other => unreachable!(
                "an unsupported plan must never reach an applied result, got {}",
                other.tag()
            ),
        }

        let mut observed =
            shape_fixtures::conforming_observation(&unsupported_plan, &unsupported_law)?;
        observed.applied_operations = BTreeSet::new();
        observed.work = WorkReceipt { useful_operations: 0, elapsed_micros: 3 };
        match unsupported_plan.evaluate(&observed)? {
            TransformationResult::RefusedDynamicOrUnsupported { concept, .. } => {
                assert_eq!(concept, DynamicConcept::Overload);
            }
            other => unreachable!("an unsupported plan must refuse, got {}", other.tag()),
        }

        for class in TransformationClass::ALL {
            assert_eq!(
                class.permitted_consumers().contains(&ConsumerClass::NoConsumer),
                class == TransformationClass::UnsupportedOrNotApplicable,
                "{} consumer contract is misclassified",
                class.tag()
            );
        }
        Ok(())
    }

    // Acceptance: the refusal a caller sees does not depend on the order the
    // plan happens to declare its preconditions in, and a named dynamic
    // concept outranks a bare "not proven".
    #[test]
    fn refusal_selection_is_order_independent_and_most_informative() -> Result<()> {
        let (law, plan) = folding();
        let mut both_failing = plan.clone();
        both_failing.preconditions[0].truth = PreconditionTruth::Unknown;
        both_failing.preconditions[1].truth =
            PreconditionTruth::DynamicOrUnsupported(DynamicConcept::Tie);

        let mut observed = observation(&both_failing, &law);
        observed.applied_operations = BTreeSet::new();
        observed.work = WorkReceipt { useful_operations: 0, elapsed_micros: 3 };

        let forward = both_failing.evaluate(&observed)?;
        let mut reversed = both_failing.clone();
        reversed.preconditions.reverse();
        let backward = reversed.evaluate(&observed)?;
        assert_eq!(forward, backward, "declaration order must not change the refusal");
        match forward {
            TransformationResult::RefusedDynamicOrUnsupported { concept, .. } => {
                assert_eq!(concept, DynamicConcept::Tie, "the named concept is more informative");
            }
            other => unreachable!("a tied precondition must be reported, got {}", other.tag()),
        }

        // With two unknowns, the reported precondition is still order-stable.
        let mut two_unknown = plan.clone();
        two_unknown.preconditions[0].truth = PreconditionTruth::Unknown;
        two_unknown.preconditions[1].truth = PreconditionTruth::Unknown;
        let mut flipped = two_unknown.clone();
        flipped.preconditions.reverse();
        assert_eq!(two_unknown.evaluate(&observed)?, flipped.evaluate(&observed)?);
        Ok(())
    }

    // Acceptance: the source-projection plan's refactor relation and every
    // precondition's evidence are part of plan identity.
    #[test]
    fn refactor_relation_and_evidence_are_part_of_plan_identity() -> Result<()> {
        let projection = shape_fixtures::source_projection_plan()?;
        let expected = projection.semantic_fingerprint()?;

        let mut repointed = projection.clone();
        match &mut repointed.refactor_relation {
            Some(relation) => {
                relation.application_proof = subject_ref("a different application result");
            }
            None => unreachable!("the source projection fixture carries a refactor relation"),
        }
        assert_ne!(
            repointed.semantic_fingerprint()?,
            expected,
            "re-pointing the application proof must create another plan"
        );

        let mut other_evidence = projection.clone();
        other_evidence.preconditions[0] = Precondition {
            evidence: subject_ref("evidence from somewhere else"),
            ..other_evidence.preconditions[0].clone()
        };
        assert_ne!(other_evidence.semantic_fingerprint()?, expected);
        Ok(())
    }

    // Review finding: `evaluate` only compared the output *stage*, so a plan's
    // declared output identity and digest were decoration. An exact
    // application must land on exactly the subject the plan declared, and
    // cannot also claim a residual.
    #[test]
    fn exact_application_requires_the_declared_output_subject() -> Result<()> {
        let (law, plan) = folding();
        assert_eq!(plan.evaluate(&observation(&plan, &law))?.tag(), "applied_exact");

        let mut other_digest = observation(&plan, &law);
        other_digest.output = Some(super::StageSubject {
            digest: seeded_digest(0xc1),
            ..plan.expected_output.clone()
        });
        match plan.evaluate(&other_digest)? {
            TransformationResult::InvalidOutput { reason } => {
                assert!(reason.contains("expected output subject"), "got {reason}");
            }
            other => unreachable!("a different output digest must not apply, got {}", other.tag()),
        }

        let mut other_identity = observation(&plan, &law);
        other_identity.output = Some(super::StageSubject {
            ir_identity: subject_ref("some other hir body"),
            ..plan.expected_output.clone()
        });
        assert_eq!(plan.evaluate(&other_identity)?.tag(), "invalid_output");

        // A complete application that also declares a residual is
        // self-contradictory, not an exact success.
        let mut contradictory = observation(&plan, &law);
        contradictory.residual = Some(ResidualBoundary::new("everything", "nothing left")?);
        match plan.evaluate(&contradictory)? {
            TransformationResult::InvalidOutput { reason } => {
                assert!(reason.contains("after applying every selected location"), "got {reason}");
            }
            other => unreachable!("a contradictory residual must not apply, got {}", other.tag()),
        }
        Ok(())
    }

    // Review finding: the subplan *names* a law declares were decoration --
    // any residual was accepted whenever the policy admitted one.
    #[test]
    fn partial_application_requires_a_law_declared_subplan_name() -> Result<()> {
        let law = shape_fixtures::effect_free_control_law()?;
        let plan = shape_fixtures::effect_free_control_plan()?;

        let mut partial = shape_fixtures::conforming_observation(&plan, &law)?;
        partial.applied_operations = [operation_id("eir:block:0002")].into_iter().collect();
        partial.work = WorkReceipt { useful_operations: 1, elapsed_micros: 300 };
        partial.residual =
            Some(ResidualBoundary::new("unreachable-blocks", "edges left untouched")?);
        partial.output = Some(subplan_output(&plan, "unreachable-blocks"));
        assert_eq!(plan.evaluate(&partial)?.tag(), "applied_with_declared_residual_boundary");

        let mut undeclared = partial.clone();
        undeclared.residual =
            Some(ResidualBoundary::new("something-the-law-never-named", "boundary")?);
        match plan.evaluate(&undeclared)? {
            TransformationResult::InvalidOutput { reason } => {
                assert!(reason.contains("does not declare"), "got {reason}");
                assert!(reason.contains("something-the-law-never-named"), "got {reason}");
            }
            other => unreachable!("an undeclared subplan must not apply, got {}", other.tag()),
        }

        // And a mutation alongside an unproven precondition is invalid output
        // even under a subplan policy: preconditions are plan-wide.
        let mut refusing = plan.clone();
        refusing.preconditions[0].truth = PreconditionTruth::Unknown;
        let mut mutated = shape_fixtures::conforming_observation(&refusing, &law)?;
        mutated.applied_operations = [operation_id("eir:block:0002")].into_iter().collect();
        mutated.work = WorkReceipt { useful_operations: 1, elapsed_micros: 300 };
        mutated.residual =
            Some(ResidualBoundary::new("unreachable-blocks", "edges left untouched")?);
        match refusing.evaluate(&mutated)? {
            TransformationResult::InvalidOutput { reason } => {
                assert!(reason.contains("partial application"), "got {reason}");
            }
            other => unreachable!(
                "a subplan policy does not license mutating under an unproven precondition, got {}",
                other.tag()
            ),
        }
        Ok(())
    }

    // Review finding: currentness compared the input digest and stage but not
    // its identity, so a plan could run against a differently-named subject.
    #[test]
    fn a_different_input_identity_is_a_subject_mismatch_not_staleness() -> Result<()> {
        let (law, plan) = folding();

        let mut renamed = observation(&plan, &law);
        renamed.observed_input.ir_identity = subject_ref("a different hir body");
        match plan.evaluate(&renamed)? {
            TransformationResult::SubjectMismatch { reason } => {
                assert!(reason.contains("different stage subject"), "got {reason}");
            }
            other => unreachable!("a renamed input is a subject mismatch, got {}", other.tag()),
        }

        // The same identity with a moved digest is staleness, and the two stay
        // distinct.
        let mut moved = observation(&plan, &law);
        moved.observed_input.digest = seeded_digest(0xd2);
        assert_eq!(plan.evaluate(&moved)?.tag(), "stale");
        Ok(())
    }

    // Review finding: result reasons came from the observation unbounded, so
    // the module's bounded-text property excluded the result contract.
    #[test]
    fn observation_supplied_reasons_are_bounded() -> Result<()> {
        let (law, plan) = folding();
        let overlong = "n".repeat(MAX_TEXT_LEN * 4);

        for (settlement, expected) in [
            (Settlement::Cancelled(overlong.clone()), "cancelled"),
            (Settlement::TimedOut(overlong.clone()), "timed_out"),
            (Settlement::LimitExceeded(overlong.clone()), "limit_exceeded"),
            (Settlement::InstrumentFailed(overlong.clone()), "instrument_failed"),
            (Settlement::CleanupFailed(overlong.clone()), "cleanup_failed"),
        ] {
            let mut observed = observation(&plan, &law);
            observed.settlement = settlement;
            let result = plan.evaluate(&observed)?;
            assert_eq!(result.tag(), expected);
            assert!(reason_of(&result).len() <= MAX_TEXT_LEN, "{expected} reason is unbounded");
        }

        let mut verifier = observation(&plan, &law);
        verifier.verifier = VerifierOutcome::Failed(overlong.clone());
        assert!(reason_of(&plan.evaluate(&verifier)?).len() <= MAX_TEXT_LEN);

        let mut equivalence = observation(&plan, &law);
        equivalence.equivalence = EquivalenceOutcome::NotProven(overlong);
        assert!(reason_of(&plan.evaluate(&equivalence)?).len() <= MAX_TEXT_LEN);
        Ok(())
    }

    // Review finding: `evaluate` is plan-local and cannot see the law, so
    // legality depended on the caller having run conformance first.
    // `evaluate_under_law` removes that discipline dependency.
    #[test]
    fn evaluate_under_law_refuses_a_non_conforming_plan() -> Result<()> {
        let (law, plan) = folding();
        let observed = observation(&plan, &law);
        assert_eq!(plan.evaluate_under_law(&law, &observed)?.tag(), "applied_exact");

        // A plan that restates its law's partial-application policy is
        // structurally valid, so plan-local evaluation still applies it --
        // which is exactly the discipline gap. Under the law it is invalid.
        let mut laundered = plan.clone();
        let selected: BTreeSet<OperationId> =
            laundered.selected_operations().into_iter().cloned().collect();
        laundered.partial_application =
            PartialApplicationPolicy::independent_subplans(&["invented"])?;
        let laundered_output = laundered.expected_output.clone();
        laundered.subplans = BTreeMap::from([(
            "invented".to_owned(),
            SubplanBinding { operations: selected, expected_output: laundered_output },
        )]);
        laundered.validate()?;
        assert_eq!(laundered.evaluate(&observed)?.tag(), "applied_exact");
        match laundered.evaluate_under_law(&law, &observed)? {
            TransformationResult::InvalidPlan { reason } => {
                assert!(reason.contains("partial-application policy"), "got {reason}");
            }
            other => unreachable!("a non-conforming plan must not apply, got {}", other.tag()),
        }

        let mut wrong_class = plan.clone();
        wrong_class.class = TransformationClass::ExecutionOptimization;
        wrong_class.consumers = [ConsumerClass::InternalStageRewrite].into_iter().collect();
        assert_eq!(wrong_class.evaluate_under_law(&law, &observed)?.tag(), "invalid_plan");
        Ok(())
    }

    // Review finding: `canonical_semantic_text` hashed each location's optional
    // provenance, so moving unchanged source or annotating a location changed
    // the plan's identity -- contradicting the contract's own rule that a
    // location is identified by its canonical operation and provenance is an
    // annotation.
    #[test]
    fn provenance_is_annotation_and_never_plan_identity() -> Result<()> {
        let (_, plan) = folding();
        let expected = plan.semantic_fingerprint()?;

        let mut dropped = plan.clone();
        dropped.locations[0] = LocationSelector::CanonicalOperation {
            stage: CompilerStage::Hir,
            operation_id: operation_id("hir:op:0007"),
            source_provenance: None,
        };
        assert_eq!(
            dropped.semantic_fingerprint()?,
            expected,
            "dropping provenance must not change plan identity"
        );

        let mut moved = plan.clone();
        moved.locations[0] = LocationSelector::CanonicalOperation {
            stage: CompilerStage::Hir,
            operation_id: operation_id("hir:op:0007"),
            source_provenance: Some(SourceProvenance::new("lib/Shape.pm", 9000, 9018)?),
        };
        assert_eq!(
            moved.semantic_fingerprint()?,
            expected,
            "the same operation in moved source is the same plan"
        );

        let mut renamed_file = plan.clone();
        renamed_file.locations[0] = LocationSelector::CanonicalOperation {
            stage: CompilerStage::Hir,
            operation_id: operation_id("hir:op:0007"),
            source_provenance: Some(SourceProvenance::new("lib/Other.pm", 120, 138)?),
        };
        assert_eq!(renamed_file.semantic_fingerprint()?, expected);

        let mut added = plan.clone();
        added.locations[1] = LocationSelector::CanonicalOperation {
            stage: CompilerStage::Hir,
            operation_id: operation_id("hir:op:0011"),
            source_provenance: Some(SourceProvenance::new("lib/Shape.pm", 200, 210)?),
        };
        assert_eq!(added.semantic_fingerprint()?, expected);

        // The operation identity itself remains load-bearing.
        let mut reidentified = plan.clone();
        reidentified.locations[0] = LocationSelector::CanonicalOperation {
            stage: CompilerStage::Hir,
            operation_id: operation_id("hir:op:9999"),
            source_provenance: None,
        };
        assert_ne!(reidentified.semantic_fingerprint()?, expected);
        Ok(())
    }

    // Review finding: a subplan name was a label with no membership, so any
    // proper subset of the selected locations could borrow a declared name and
    // be reported as a complete subplan.
    #[test]
    fn a_declared_subplan_binds_the_operations_it_completes() -> Result<()> {
        let law = shape_fixtures::effect_free_control_law()?;
        let plan = shape_fixtures::effect_free_control_plan()?;
        plan.verify_law_conformance(&law)?;

        // The exact bound set applies.
        let mut exact_subplan = shape_fixtures::conforming_observation(&plan, &law)?;
        exact_subplan.applied_operations = [operation_id("eir:block:0002")].into_iter().collect();
        exact_subplan.work = WorkReceipt { useful_operations: 1, elapsed_micros: 300 };
        exact_subplan.residual =
            Some(ResidualBoundary::new("unreachable-blocks", "edges left untouched")?);
        let blocks_output = match plan.subplans.get("unreachable-blocks") {
            Some(binding) => binding.expected_output.clone(),
            None => unreachable!("the effect-free fixture binds unreachable-blocks"),
        };
        exact_subplan.output = Some(blocks_output.clone());
        assert_eq!(plan.evaluate(&exact_subplan)?.tag(), "applied_with_declared_residual_boundary");

        // A subplan owns its output subject: the right operations landing on
        // the whole plan's output, or anywhere else, is not a completed subplan.
        let mut wrong_output = exact_subplan.clone();
        wrong_output.output = Some(plan.expected_output.clone());
        match plan.evaluate(&wrong_output)? {
            TransformationResult::InvalidOutput { reason } => {
                assert!(reason.contains("output subject subplan"), "got {reason}");
                assert!(reason.contains("unreachable-blocks"), "got {reason}");
            }
            other => unreachable!("a subplan's output is load-bearing, got {}", other.tag()),
        }

        // A subplan cannot declare an output at another stage.
        let mut foreign_stage = plan.clone();
        if let Some(binding) = foreign_stage.subplans.get_mut("unreachable-blocks") {
            binding.expected_output.stage = CompilerStage::Hir;
        }
        assert_invalid(
            &foreign_stage,
            "but the plan produces stage",
            "a subplan cannot produce another stage",
        );

        // The other subplan's operations under this subplan's name do not.
        let mut wrong_subset = exact_subplan.clone();
        wrong_subset.applied_operations = [operation_id("eir:edge:0005")].into_iter().collect();
        match plan.evaluate(&wrong_subset)? {
            TransformationResult::InvalidOutput { reason } => {
                assert!(reason.contains("did not apply exactly the operations"), "got {reason}");
                assert!(reason.contains("unreachable-blocks"), "got {reason}");
            }
            other => unreachable!("a borrowed subplan name must not apply, got {}", other.tag()),
        }

        // Binding invariants: within the selection, disjoint, and only when the
        // law admits subplans at all.
        let mut unselected = plan.clone();
        unselected.subplans.insert(
            "unreachable-blocks".to_owned(),
            SubplanBinding {
                operations: [operation_id("eir:block:9999")].into_iter().collect(),
                expected_output: blocks_output.clone(),
            },
        );
        assert_invalid(&unselected, "which it does not select", "a subplan cannot bind outside");

        let mut overlapping = plan.clone();
        overlapping.subplans.insert(
            "unreachable-edges".to_owned(),
            SubplanBinding {
                operations: [operation_id("eir:block:0002")].into_iter().collect(),
                expected_output: blocks_output.clone(),
            },
        );
        assert_invalid(&overlapping, "more than one subplan", "subplans must be disjoint");

        let mut empty_binding = plan.clone();
        empty_binding.subplans.insert(
            "unreachable-edges".to_owned(),
            SubplanBinding { operations: BTreeSet::new(), expected_output: blocks_output },
        );
        assert_invalid(&empty_binding, "to no operation", "a subplan completes something");

        let mut unnamed = plan.clone();
        unnamed.subplans.remove("unreachable-edges");
        assert_invalid(&unnamed, "exactly the subplans its law declares", "bindings match names");

        let (_, folding_plan) = folding();
        let mut bound_but_prohibited = folding_plan.clone();
        bound_but_prohibited.subplans = BTreeMap::from([(
            "invented".to_owned(),
            SubplanBinding {
                operations: [operation_id("hir:op:0007")].into_iter().collect(),
                expected_output: folding_plan.expected_output.clone(),
            },
        )]);
        assert_invalid(
            &bound_but_prohibited,
            "while its law prohibits partial application",
            "a prohibiting law has no subplans to bind",
        );
        Ok(())
    }

    // Review finding: the work receipt had no defined unit, so an applied
    // attempt could report fewer useful operations than the locations it says
    // it changed.
    #[test]
    fn reported_work_covers_every_applied_location() -> Result<()> {
        let (law, plan) = folding();
        assert_eq!(plan.locations.len(), 2);

        let mut under_reported = observation(&plan, &law);
        under_reported.work = WorkReceipt { useful_operations: 1, elapsed_micros: 400 };
        match plan.evaluate(&under_reported)? {
            TransformationResult::InvalidOutput { reason } => {
                assert!(reason.contains("applied 2 location(s)"), "got {reason}");
                assert!(reason.contains("only 1 useful operation(s)"), "got {reason}");
            }
            other => unreachable!("under-reported work must not apply, got {}", other.tag()),
        }

        // The floor is the applied count; work above it is the transformation's
        // own internal effort and stays legal.
        let mut exact = observation(&plan, &law);
        exact.work = WorkReceipt { useful_operations: 2, elapsed_micros: 400 };
        assert_eq!(plan.evaluate(&exact)?.tag(), "applied_exact");

        let mut generous = observation(&plan, &law);
        generous.work = WorkReceipt { useful_operations: 40, elapsed_micros: 400 };
        assert_eq!(plan.evaluate(&generous)?.tag(), "applied_exact");
        Ok(())
    }

    // Review question: conformance permits a plan to carry more than its law
    // requires. That is deliberate -- each extra only narrows what the plan may
    // legally do -- and this pins the asymmetry so T02 can rely on it.
    #[test]
    fn a_plan_may_strengthen_its_law_but_never_weaken_it() -> Result<()> {
        let (law, plan) = folding();

        let mut strengthened = plan.clone();
        strengthened.preconditions.push(Precondition {
            id: PreconditionId::new("extra.operand-is-not-tied")?,
            statement: "the operand is proven untied beyond what the law demands".to_owned(),
            stage: CompilerStage::Hir,
            evidence_stage: CompilerStage::Hir,
            evidence: subject_ref("tie analysis for lib/Shape.pm#area"),
            truth: PreconditionTruth::ProvenExact,
        });
        strengthened.preserved.insert(PreservedProposition::Cleanup);
        strengthened.excluded_concepts.insert(DynamicConcept::XsBoundary);
        strengthened.equivalence_obligations.push(EquivalenceObligation {
            oracle: EquivalenceOracle::VerifierMutation,
            proposition: PreservedProposition::Cleanup,
            subject: subject_ref("cleanup mutation beyond the law's obligations"),
        });
        strengthened.verify_law_conformance(&law)?;

        // Weakening in the same places is exactly what conformance rejects, and
        // `conformance_rejects_every_law_plan_divergence` walks each case.
        let mut weakened = plan.clone();
        weakened.excluded_concepts.remove(&DynamicConcept::Overload);
        assert!(weakened.verify_law_conformance(&law).is_err());
        Ok(())
    }

    // Review finding: the unchanged-output guard only fired for an IR-shape
    // change, but `StageSubject::digest` covers the IR *and* its facts, so any
    // intended change contradicts an unchanged expected output.
    #[test]
    fn no_intended_change_may_expect_the_input_subject_unchanged() -> Result<()> {
        let (_, plan) = folding();

        for change in ChangedProposition::ALL {
            let mut unchanged = plan.clone();
            unchanged.intended_changes = [change].into_iter().collect();
            unchanged.expected_output = unchanged.input.clone();
            // Source-text projection needs its relation to stay valid; give it
            // one so the failure under test is the unchanged output, not that.
            if change == ChangedProposition::SourceText {
                unchanged.consumers = [ConsumerClass::SourceEdit].into_iter().collect();
                unchanged.class = TransformationClass::SourceProjectionCandidate;
                unchanged.claim_ceiling = ClaimCeiling::AuthorizedSourceEdit;
                unchanged.preserved.remove(&PreservedProposition::SourceMapping);
                unchanged.refactor_relation = Some(RefactorPlanRelation {
                    refactor_plan_id: subject_ref("refactor-plan/unchanged"),
                    edit_set_equality: subject_ref("equality"),
                    application_proof: subject_ref("application"),
                    post_edit_proof: subject_ref("post-edit"),
                });
            }
            assert_invalid(
                &unchanged,
                "intends a change but expects the input subject unchanged",
                "an unchanged output contradicts an intended change",
            );
        }

        // A plan that intends no change -- the unsupported class -- may declare
        // the input subject unchanged, because it changes nothing.
        let mut unsupported = plan.clone();
        unsupported.class = TransformationClass::UnsupportedOrNotApplicable;
        unsupported.consumers = [ConsumerClass::NoConsumer].into_iter().collect();
        unsupported.claim_ceiling = ClaimCeiling::InternalFactOnly;
        unsupported.intended_changes = BTreeSet::new();
        unsupported.expected_output = unsupported.input.clone();
        unsupported.preconditions[0].truth =
            PreconditionTruth::DynamicOrUnsupported(DynamicConcept::Overload);
        unsupported.validate()?;
        Ok(())
    }

    // Review finding: MAX_TEXT_LEN is a byte limit, but the bounding helper
    // counted characters, and several internally formatted reasons were never
    // bounded at all.
    #[test]
    fn every_result_reason_is_bounded_in_bytes() -> Result<()> {
        let (law, plan) = folding();

        // 512 multibyte characters are far more than 512 bytes.
        let multibyte = "\u{00e9}\u{4f60}\u{1f600}".repeat(MAX_TEXT_LEN);
        assert!(multibyte.len() > MAX_TEXT_LEN * 4);
        let bounded = super::bounded_reason(&multibyte);
        assert!(bounded.len() <= MAX_TEXT_LEN, "bounded to {} bytes", bounded.len());
        // Still valid UTF-8: it round-trips through the same slice.
        assert_eq!(bounded, String::from_utf8(bounded.clone().into_bytes())?);

        for settlement in [
            Settlement::Cancelled(multibyte.clone()),
            Settlement::TimedOut(multibyte.clone()),
            Settlement::LimitExceeded(multibyte.clone()),
            Settlement::InstrumentFailed(multibyte.clone()),
            Settlement::CleanupFailed(multibyte.clone()),
        ] {
            let mut observed = observation(&plan, &law);
            observed.settlement = settlement;
            let result = plan.evaluate(&observed)?;
            assert!(
                reason_of(&result).len() <= MAX_TEXT_LEN,
                "{} reason is {} bytes",
                result.tag(),
                reason_of(&result).len()
            );
        }

        // An internally formatted reason embeds caller-supplied text, so it is
        // bounded too. A maximum-length subplan name is the longest such case.
        let long_name = "\u{4f60}".repeat(MAX_TEXT_LEN / 3);
        let subplan_law = shape_fixtures::effect_free_control_law()?;
        let subplan_plan = shape_fixtures::effect_free_control_plan()?;
        let mut partial = shape_fixtures::conforming_observation(&subplan_plan, &subplan_law)?;
        partial.applied_operations = [operation_id("eir:block:0002")].into_iter().collect();
        partial.work = WorkReceipt { useful_operations: 1, elapsed_micros: 10 };
        partial.residual = Some(ResidualBoundary::new(&long_name, "boundary")?);
        let result = subplan_plan.evaluate(&partial)?;
        assert_eq!(result.tag(), "invalid_output");
        assert!(
            reason_of(&result).len() <= MAX_TEXT_LEN,
            "formatted reason is {} bytes",
            reason_of(&result).len()
        );

        // An invalid plan's reason comes from an error chain and is bounded the
        // same way.
        let mut broken = plan.clone();
        broken.preconditions[0].statement = "\u{1f600}".repeat(MAX_TEXT_LEN);
        let invalid = plan_with_invalid_statement(&broken, &law)?;
        assert_eq!(invalid.tag(), "invalid_plan");
        assert!(reason_of(&invalid).len() <= MAX_TEXT_LEN);
        Ok(())
    }

    fn plan_with_invalid_statement(
        plan: &TransformationPlan,
        law: &TransformationLaw,
    ) -> Result<TransformationResult> {
        let observed = shape_fixtures::conforming_observation(plan, law)?;
        plan.evaluate(&observed)
    }

    // Review question: disjoint subplans could leave selected operations bound
    // to none of them, so applying every subplan would still not equal the
    // whole plan. Independent *complete* subplans partition the selection.
    #[test]
    fn declared_subplans_partition_the_selection() -> Result<()> {
        let law = shape_fixtures::effect_free_control_law()?;
        let plan = shape_fixtures::effect_free_control_plan()?;
        plan.validate()?;

        // Every selected operation is bound to exactly one subplan.
        let selected: BTreeSet<OperationId> =
            plan.selected_operations().into_iter().cloned().collect();
        let mut covered: BTreeSet<OperationId> = BTreeSet::new();
        for binding in plan.subplans.values() {
            for operation in &binding.operations {
                assert!(covered.insert(operation.clone()), "subplans must stay disjoint");
            }
        }
        assert_eq!(covered, selected, "the subplans must cover the whole selection");

        // Selecting one more operation without binding it leaves a gap.
        let mut gap = plan.clone();
        gap.locations.push(LocationSelector::CanonicalOperation {
            stage: CompilerStage::Eir,
            operation_id: operation_id("eir:block:0009"),
            source_provenance: None,
        });
        assert_invalid(
            &gap,
            "binds it to no subplan",
            "an operation reserved for full application only is not expressible",
        );

        // Dropping a subplan's operation leaves the same gap from the other
        // direction, and the law's names are still all bound.
        let mut shrunk = plan.clone();
        if let Some(binding) = shrunk.subplans.get_mut("unreachable-edges") {
            binding.operations = [operation_id("eir:block:0002")].into_iter().collect();
        }
        assert_invalid(&shrunk, "more than one subplan", "shrinking must not overlap");
        Ok(())
    }

    // Review claim: "unsupported laws can be uninstantiable -- no plan can
    // instantiate a law with no required preconditions, because unsupported
    // plans require an unproven one." That is incorrect, and this shows why: a
    // plan may carry preconditions the law does not require (the strengthening
    // asymmetry), so it supplies the unproven precondition itself.
    #[test]
    fn an_unsupported_law_without_required_preconditions_is_instantiable() -> Result<()> {
        let (_, folding_plan) = folding();

        let mut law = shape_fixtures::exact_value_folding_law()?;
        law.id = LawId::new("hir.overload-not-applicable")?;
        law.class = TransformationClass::UnsupportedOrNotApplicable;
        law.consumers = [ConsumerClass::NoConsumer].into_iter().collect();
        law.permitted_changes = BTreeSet::new();
        law.required_preconditions = BTreeSet::new();
        law.claim_ceiling = ClaimCeiling::InternalFactOnly;
        law.validate()?;
        assert!(law.required_preconditions.is_empty(), "the law requires no precondition");

        let mut plan = folding_plan.clone();
        plan.id = text_id("plan.hir.overload-not-applicable");
        plan.law = law.binding()?;
        plan.class = law.class;
        plan.consumers = law.consumers.clone();
        plan.claim_ceiling = ClaimCeiling::InternalFactOnly;
        plan.intended_changes = BTreeSet::new();
        plan.expected_output = plan.input.clone();
        plan.preconditions[0].truth =
            PreconditionTruth::DynamicOrUnsupported(DynamicConcept::Overload);
        plan.verify_law_conformance(&law)?;

        let mut observed = shape_fixtures::conforming_observation(&plan, &law)?;
        observed.applied_operations = BTreeSet::new();
        observed.work = WorkReceipt { useful_operations: 0, elapsed_micros: 1 };
        match plan.evaluate(&observed)? {
            TransformationResult::RefusedDynamicOrUnsupported { concept, .. } => {
                assert_eq!(concept, DynamicConcept::Overload);
            }
            other => unreachable!("the plan instantiates and refuses, got {}", other.tag()),
        }
        Ok(())
    }

    // Review finding: a plan whose canonical text overruns its bound has no
    // computable identity, yet evaluation only checked structural validity.
    #[test]
    fn a_plan_without_a_computable_identity_never_applies() -> Result<()> {
        let (law, plan) = folding();

        let mut oversized = plan.clone();
        oversized.locations = (0..256usize)
            .map(|index| {
                let padded = format!("hir:op:{index:08}:{}", "p".repeat(MAX_TEXT_LEN - 32));
                Ok(LocationSelector::CanonicalOperation {
                    stage: CompilerStage::Hir,
                    operation_id: OperationId::new(&padded)?,
                    source_provenance: None,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        // Structurally valid, but with no computable fingerprint.
        oversized.validate()?;
        assert!(oversized.semantic_fingerprint().is_err());

        let mut observed = shape_fixtures::conforming_observation(&oversized, &law)?;
        observed.work =
            WorkReceipt { useful_operations: oversized.locations.len() as u64, elapsed_micros: 1 };
        match oversized.evaluate(&observed)? {
            TransformationResult::InvalidPlan { reason } => {
                assert!(reason.contains("above the bound of"), "got {reason}");
            }
            other => unreachable!(
                "a plan with no computable identity must not apply, got {}",
                other.tag()
            ),
        }
        Ok(())
    }

    // Review finding: a subplan applies operations, so binding it to the
    // unchanged input subject would let a partial application report success
    // for output that never moved -- the same rule the whole plan carries.
    #[test]
    fn a_subplan_cannot_produce_the_unchanged_input_subject() -> Result<()> {
        let plan = shape_fixtures::effect_free_control_plan()?;
        plan.validate()?;

        let mut unchanged = plan.clone();
        if let Some(binding) = unchanged.subplans.get_mut("unreachable-blocks") {
            binding.expected_output = unchanged.input.clone();
        }
        assert_invalid(
            &unchanged,
            "the unchanged input subject",
            "a subplan that applies operations must move its output",
        );
        Ok(())
    }
}
