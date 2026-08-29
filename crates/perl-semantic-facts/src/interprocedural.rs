//! Dependency-neutral versioned contracts for interprocedural composition
//! (#12672, programme #12671).
//!
//! Four contracts:
//!
//! - [`CallApplicationSubject`] (`call_application_subject.v1`) — one exact
//!   caller/callee/call/input/context subject;
//! - [`CallableSemanticSummaryRef`] (`callable_semantic_summary_ref.v1`) — a
//!   reference envelope identifying one callable-local semantic subject and
//!   the canonical facts it references, with composition policy, result
//!   facets, currentness, work, refusal and claim ceilings;
//! - [`CallableSemanticSummary`] (`callable_semantic_summary.v1`) — the
//!   immutable per-callable packet (#12674, I02): canonical fact references
//!   joined by identity for direct intraprocedural behavior, a facet
//!   completeness ledger, and outbound calls recorded as unresolved
//!   transitive dependencies;
//! - [`InterproceduralFactResult`] (`interprocedural_fact_result.v1`) — the
//!   composition result envelope, whose outcome vocabulary keeps Composed,
//!   Refused, Conservative, Stale, Invalid, ResourceExhausted and
//!   InstrumentError distinct while preserving unaffected facts.
//!
//! These contracts reference canonical facts by identity ([`FactId`],
//! [`BoundaryLink`]) rather than copying or redefining fact vocabulary. They
//! extract no facts, traverse no call graph, perform no composition, and
//! change no provider behavior — types, validation, canonical serialization,
//! and synthetic fixtures only.

use serde::{Deserialize, Serialize};

use crate::{
    BoundaryKind, BoundaryLink, EntityId, FactId, FileId, ScopeId, SemanticConfidence,
    SemanticProvenance, SemanticReasonCode, SourceAnchor, SourceGeneration,
};

/// `call_application_subject.v1` schema version.
pub const CALL_APPLICATION_SUBJECT_SCHEMA_VERSION: u32 = 1;
/// `callable_semantic_summary_ref.v1` schema version.
pub const CALLABLE_SEMANTIC_SUMMARY_REF_SCHEMA_VERSION: u32 = 1;
/// `interprocedural_fact_result.v1` schema version.
pub const INTERPROCEDURAL_FACT_RESULT_SCHEMA_VERSION: u32 = 1;

/// What the call targets, at the precision the evidence supports.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum CallTarget {
    /// A resolved callable entity.
    Exact(EntityId),
    /// The call leaves the exact boundary (dynamic dispatch, symbolic
    /// reference, unsupported effect) — the target must carry its boundary.
    DynamicBoundary(BoundaryLink),
}

/// One ordered call input, at the precision the evidence supports.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum CallInput {
    /// An exact value-producing fact.
    ExactValue(FactId),
    /// The input crosses an exactness boundary.
    DynamicBoundary(BoundaryLink),
    /// The input position exists but no evidence identifies it. Omitted is
    /// never silently dropped: it is an explicit facet of the subject.
    Omitted,
}

/// Receiver shape of a call.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ReceiverKind {
    /// Plain subroutine call.
    Function,
    /// Static/class method (`Class->method`).
    StaticMethod,
    /// Instance method (`$obj->method`).
    InstanceMethod,
}

/// Lexical context the call is evaluated in.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct CallContext {
    /// Receiver shape of the call.
    pub receiver: ReceiverKind,
    /// Lexical scope enclosing the call site, when established.
    pub lexical_scope: Option<ScopeId>,
}

/// Content identity of a callable body — an identity reference, never the
/// content itself.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum BodyIdentity {
    /// A stable content identity (e.g. a content hash) for the caller and
    /// callee bodies the subject reasons over. Two subjects with different
    /// body identities are different subjects — a changed body must never
    /// validate as the same call.
    Exact(String),
    /// Body identity not established — explicit, never a silent default.
    Unknown,
}

/// Toolchain/profile subject the call is interpreted under. Features are
/// sorted and deduplicated at construction for deterministic bytes.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileIdentity {
    /// Perl version identity, when established.
    pub perl_version: Option<String>,
    /// Feature flags in effect (canonical order).
    pub features: Vec<String>,
    /// Platform identity, when established.
    pub platform: Option<String>,
    /// Capability configuration identity, when established.
    pub capability: Option<String>,
}

impl ProfileIdentity {
    /// Construct with canonical feature ordering applied.
    #[must_use]
    pub fn new(
        perl_version: Option<String>,
        mut features: Vec<String>,
        platform: Option<String>,
        capability: Option<String>,
    ) -> Self {
        features.sort();
        features.dedup();
        Self { perl_version, features, platform, capability }
    }
}

/// Source/document/root/project/profile identity of the call site.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceIdentity {
    /// Document the call site lives in.
    pub document: FileId,
    /// Workspace root identity, when established.
    pub workspace_root: Option<String>,
    /// Accepted project/workspace generation the subject is valid under,
    /// when established.
    pub project_generation: Option<SourceGeneration>,
    /// Toolchain/profile the call is interpreted under.
    pub profile: ProfileIdentity,
}

/// Whether the call executes at compile time or runtime. The phases are
/// contract-distinct and must never collapse.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum CallPhase {
    /// Compile-time execution (BEGIN/CHECK/UNITCHECK).
    CompileTime,
    /// Runtime execution.
    Runtime,
    /// Phase not established.
    Unknown,
}

/// Parameter/place substitution relation for one call input.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ParameterSubstitution {
    /// Callee parameter position (ordered, unique per subject).
    pub parameter: u32,
    /// The input place substituted into it.
    pub place: CallInput,
}

/// Identity of the summary/application policy the subject composes under.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ApplicationPolicyIdentity {
    /// Direct composition without summaries.
    Direct,
    /// Composition through callable summaries.
    SummaryBacked,
    /// A named consumer policy.
    ConsumerNamed(String),
}

/// Bounded depth and component identity for recursive composition.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ComponentIdentity {
    /// Maximum composition depth (must be positive).
    pub max_depth: u32,
    /// Strongly-connected component identity, when established.
    pub component_id: Option<u64>,
}

/// One exact caller/callee/call/input/context subject
/// (`call_application_subject.v1`). Every identity facet is explicit: an
/// incomplete subject is a validation violation, never an implicit default.
/// Name/package/anchor alone do not identify the subject — body, source/
/// profile, phase, edge, substitution, policy, and component identities are
/// all part of what "this exact call" means (#12672 operator review).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallApplicationSubject {
    /// Schema version ([`CALL_APPLICATION_SUBJECT_SCHEMA_VERSION`]).
    pub schema_version: u32,
    /// The exact call-site fact this subject is anchored to.
    pub call_fact_id: FactId,
    /// The calling callable entity.
    pub caller: EntityId,
    /// What is being called, at the precision the evidence supports.
    pub callee: CallTarget,
    /// Source anchor of the call site.
    pub anchor: SourceAnchor,
    /// Source generation the subject is current for.
    pub source_generation: SourceGeneration,
    /// Package the call site belongs to, when established.
    pub package: Option<String>,
    /// Ordered call inputs. Order is contract-significant and preserved
    /// verbatim by canonical serialization.
    pub inputs: Vec<CallInput>,
    /// Lexical/receiver context of the call.
    pub context: CallContext,
    /// Content identity of the caller/callee bodies.
    pub body: BodyIdentity,
    /// Document/root/project/profile identity of the call site.
    pub source: SourceIdentity,
    /// Compile-time vs runtime phase of the call.
    pub call_phase: CallPhase,
    /// Accepted workspace/CompilerWorld generation the subject composes
    /// under, when established.
    pub world_generation: Option<SourceGeneration>,
    /// Canonical call-edge identity.
    pub call_edge_id: crate::EdgeId,
    /// Ordered parameter/place substitutions. Parameters are unique and
    /// canonically ordered at construction.
    pub substitutions: Vec<ParameterSubstitution>,
    /// Summary/application policy identity.
    pub policy_identity: ApplicationPolicyIdentity,
    /// Bounded depth and component identity for recursive composition.
    pub component: ComponentIdentity,
}

impl CallApplicationSubject {
    /// Validate the subject's own invariants (fail-closed).
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut violations = Vec::new();
        if self.schema_version != CALL_APPLICATION_SUBJECT_SCHEMA_VERSION {
            violations.push(format!(
                "schema_version {} is not call_application_subject.v1 ({})",
                self.schema_version, CALL_APPLICATION_SUBJECT_SCHEMA_VERSION
            ));
        }
        if self.anchor.end_byte < self.anchor.start_byte {
            violations.push("call-site anchor end precedes its start".to_string());
        }
        // A file identity of FileId(u64::MAX) is the sentinel used by
        // synthetic fixtures for "no file"; a real subject must name a file.
        if self.anchor.file_id == NO_FILE {
            violations.push("call-site anchor must name a real file identity".to_string());
        }
        if self.source.document != self.anchor.file_id {
            violations.push(
                "source document and call-site anchor file must agree (one call, one document)"
                    .to_string(),
            );
        }
        if let BodyIdentity::Exact(hash) = &self.body
            && hash.is_empty()
        {
            violations.push("an Exact body identity must not be empty".to_string());
        }
        if self.source.profile.features.windows(2).any(|pair| pair[0] >= pair[1]) {
            violations.push(
                "profile features must be strictly sorted and deduplicated (canonical ordering)"
                    .to_string(),
            );
        }
        if self.component.max_depth == 0 {
            violations.push("component max_depth must be positive".to_string());
        }
        for (index, substitution) in self.substitutions.iter().enumerate() {
            if substitution.parameter as usize >= self.inputs.len() {
                violations.push(format!(
                    "substitution parameter {} has no matching input position ({} inputs)",
                    substitution.parameter,
                    self.inputs.len()
                ));
            }
            if index > 0 && self.substitutions[index - 1].parameter >= substitution.parameter {
                violations.push(
                    "substitution parameters must be unique and strictly increasing (canonical \
                     ordering)"
                        .to_string(),
                );
            }
        }
        if violations.is_empty() { Ok(()) } else { Err(violations) }
    }
}

/// Sentinel file identity that must never survive validation in a contract
/// subject — "no file" is not a place a call can live.
pub const NO_FILE: FileId = FileId(u64::MAX);

/// How a summary may be composed.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum CompositionPolicy {
    /// Direct call facts only — no callee summaries consumed.
    DirectOnly,
    /// Callee summaries consumed along an acyclic call relation.
    Acyclic,
    /// Recursive components solved with a bounded monotone closure; the work
    /// budget is part of the contract.
    RecursiveBounded,
    /// The consumer's own policy decides; the summary offers facts but no
    /// composition guarantee. Distinct from Acyclic so consumer policy can
    /// never be read back as summary policy.
    ConsumerPolicy,
}

/// Which result facets a summary carries. Facets are independent: presence
/// of one never strengthens another (cross-facet strengthening is a
/// validation violation checked against the claim ceiling).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ResultFacets {
    /// Return/result-value facts.
    pub result: bool,
    /// Side-effect facts.
    pub effect: bool,
    /// Escape/aliasing facts.
    pub escape: bool,
    /// Control-flow facts.
    pub control: bool,
}

impl ResultFacets {
    /// Construct a facet set. Facets are independent: presence of one never
    /// strengthens another.
    #[must_use]
    pub const fn new(result: bool, effect: bool, escape: bool, control: bool) -> Self {
        Self { result, effect, escape, control }
    }
}

/// How far the summary's claims may be read.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ClaimCeiling {
    /// Claims hold exactly as referenced.
    Exact,
    /// Claims are conservative approximations.
    Provisional,
    /// The summary makes no claim at all (identity/reference only).
    NoClaim,
}

/// What a summary does at its certainty boundary.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum RefusalCeiling {
    /// Refuse composition with an explicit reason.
    Refuse,
    /// Compose conservatively, marking every affected facet provisional.
    Conservative,
}

/// Currentness of a summary against the source it describes.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum SummaryCurrentness {
    /// Current for the named generation.
    Fresh(SourceGeneration),
    /// Known older than the current generation — reuse is stale and must be
    /// reported as such, never silently as current.
    Stale,
    /// Currentness not established.
    Unknown,
}

/// Bounded work contract: the budget offered and the units accounted.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct WorkBudget {
    /// Maximum work units the composition may consume.
    pub max_units: u32,
}

impl WorkBudget {
    /// Construct a work budget offering `max_units` units.
    #[must_use]
    pub const fn new(max_units: u32) -> Self {
        Self { max_units }
    }
}

/// Privacy classification of contract payloads.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum PrivacyClass {
    /// Contains no private host paths, environment values, or user data —
    /// safe to publish in receipts.
    PrivateSafe,
    /// May contain private material — must not be published.
    Private,
}

/// A reference envelope identifying one callable-local semantic subject and
/// the canonical facts it references (`callable_semantic_summary_ref.v1`).
/// References are by identity only; nothing here copies fact content.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallableSemanticSummaryRef {
    /// Schema version ([`CALLABLE_SEMANTIC_SUMMARY_REF_SCHEMA_VERSION]).
    pub schema_version: u32,
    /// The callable entity this summary describes.
    pub callable: EntityId,
    /// Source generation the summary was assembled for.
    pub source_generation: SourceGeneration,
    /// Canonical facts the summary references, sorted and deduplicated at
    /// construction so canonical bytes are deterministic.
    pub referenced_facts: Vec<FactId>,
    /// Boundaries limiting the referenced facts, sorted at construction.
    pub referenced_boundaries: Vec<BoundaryLink>,
    /// How the summary may be composed.
    pub composition_policy: CompositionPolicy,
    /// Which result facets the summary carries.
    pub facets: ResultFacets,
    /// Currentness against the described source.
    pub currentness: SummaryCurrentness,
    /// Bounded work the composition may consume.
    pub work: WorkBudget,
    /// Boundary behavior at the certainty edge.
    pub refusal_ceiling: RefusalCeiling,
    /// How far the summary's claims may be read.
    pub claim_ceiling: ClaimCeiling,
    /// Privacy classification of the payload.
    pub privacy: PrivacyClass,
}

impl CallableSemanticSummaryRef {
    /// Construct with canonical ordering applied (sorted, deduplicated
    /// references) so two assemblies of the same references produce
    /// identical canonical bytes.
    #[must_use]
    #[allow(clippy::too_many_arguments)] // the constructor mirrors the contract fields
    pub fn new(
        callable: EntityId,
        source_generation: SourceGeneration,
        mut referenced_facts: Vec<FactId>,
        mut referenced_boundaries: Vec<BoundaryLink>,
        composition_policy: CompositionPolicy,
        facets: ResultFacets,
        currentness: SummaryCurrentness,
        work: WorkBudget,
        refusal_ceiling: RefusalCeiling,
        claim_ceiling: ClaimCeiling,
        privacy: PrivacyClass,
    ) -> Self {
        referenced_facts.sort();
        referenced_facts.dedup();
        referenced_boundaries.sort();
        referenced_boundaries.dedup();
        Self {
            schema_version: CALLABLE_SEMANTIC_SUMMARY_REF_SCHEMA_VERSION,
            callable,
            source_generation,
            referenced_facts,
            referenced_boundaries,
            composition_policy,
            facets,
            currentness,
            work,
            refusal_ceiling,
            claim_ceiling,
            privacy,
        }
    }

    /// Whether the payload may be published in receipts (privacy ceiling).
    #[must_use]
    pub fn is_publishable(&self) -> bool {
        self.privacy == PrivacyClass::PrivateSafe
    }

    /// Validate the summary's own invariants (fail-closed).
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut violations = Vec::new();
        if self.schema_version != CALLABLE_SEMANTIC_SUMMARY_REF_SCHEMA_VERSION {
            violations.push(format!(
                "schema_version {} is not callable_semantic_summary_ref.v1 ({})",
                self.schema_version, CALLABLE_SEMANTIC_SUMMARY_REF_SCHEMA_VERSION
            ));
        }
        if self.referenced_facts.windows(2).any(|pair| pair[0] >= pair[1]) {
            violations.push(
                "referenced_facts must be strictly sorted and deduplicated (canonical ordering)"
                    .to_string(),
            );
        }
        if self.referenced_boundaries.windows(2).any(|pair| pair[0] >= pair[1]) {
            violations.push(
                "referenced_boundaries must be strictly sorted and deduplicated (canonical ordering)"
                    .to_string(),
            );
        }
        // Cross-facet strengthening: a NoClaim summary may not carry facets —
        // facets without claims are a strengthening smuggle.
        if self.claim_ceiling == ClaimCeiling::NoClaim
            && (self.facets.result
                || self.facets.effect
                || self.facets.escape
                || self.facets.control)
        {
            violations.push(
                "a NoClaim summary must not carry result facets (cross-facet strengthening)"
                    .to_string(),
            );
        }
        // Stale reuse: a stale summary must not present Exact claims.
        if matches!(self.currentness, SummaryCurrentness::Stale)
            && self.claim_ceiling == ClaimCeiling::Exact
        {
            violations.push(
                "a stale summary must not present Exact claims (historical-as-current)".to_string(),
            );
        }
        // Fresh currentness must name the summary's own known generation —
        // Fresh(Unknown) or Fresh(other) hands consumers two contradictory
        // freshness identities (#12672 review).
        if let SummaryCurrentness::Fresh(fresh_generation) = &self.currentness {
            match fresh_generation {
                SourceGeneration::Known(fresh_value) => {
                    if let SourceGeneration::Known(summary_value) = &self.source_generation
                        && fresh_value != summary_value
                    {
                        violations.push(format!(
                            "Fresh({fresh_value}) disagrees with the summary's source_generation \
                             {summary_value} (one freshness identity)"
                        ));
                    }
                }
                SourceGeneration::Unknown => violations.push(
                    "Fresh currentness must name a known generation, not Unknown".to_string(),
                ),
            }
        }
        if self.work.max_units == 0 {
            violations.push("work budget must offer at least one unit".to_string());
        }
        if violations.is_empty() { Ok(()) } else { Err(violations) }
    }
}

/// Terminal outcome of one interprocedural composition. Every state is
/// explicit: refusal is never an empty result, staleness is never silent,
/// and an instrument failure is never a semantic verdict.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InterproceduralOutcome {
    /// Composition completed with facts.
    Composed,
    /// Composition refused at the certainty boundary, with the reason.
    Refused { reason: SemanticReasonCode },
    /// Composition completed conservatively; every affected facet is
    /// provisional, with the reason recorded.
    Conservative { reason: SemanticReasonCode },
    /// The summary is known older than the subject's generation.
    Stale { reason: SemanticReasonCode },
    /// The subject or summary failed validation.
    Invalid { reason: String },
    /// The work budget was exhausted before completion.
    ResourceExhausted { units_consumed: u32 },
    /// The instrument itself failed — no semantic verdict exists.
    InstrumentError { reason: String },
}

/// The composition result envelope (`interprocedural_fact_result.v1`).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterproceduralFactResult {
    /// Schema version ([`INTERPROCEDURAL_FACT_RESULT_SCHEMA_VERSION]).
    pub schema_version: u32,
    /// The exact call subject the result answers for.
    pub subject: CallApplicationSubject,
    /// The summary reference consumed, when the outcome used one.
    pub summary_ref: Option<CallableSemanticSummaryRef>,
    /// Terminal outcome.
    pub outcome: InterproceduralOutcome,
    /// Composed facts. Empty is admissible only with an outcome that names
    /// why — never as an implicit "nothing found".
    pub facts: Vec<crate::SemanticFactEnvelope>,
    /// Boundaries affecting the result, sorted at construction.
    pub boundaries: Vec<BoundaryLink>,
    /// Work units consumed by the composition.
    pub units_consumed: u32,
    /// Generation the result itself is current for.
    pub source_generation: SourceGeneration,
    /// Confidence of the result as a whole (never stronger than the weakest
    /// consumed evidence).
    pub confidence: SemanticConfidence,
    /// Provenance of the composition path.
    pub provenance: SemanticProvenance,
    /// Reason code of the result.
    pub reason_code: SemanticReasonCode,
}

impl InterproceduralFactResult {
    /// Validate the result's own invariants (fail-closed).
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut violations = Vec::new();
        if self.schema_version != INTERPROCEDURAL_FACT_RESULT_SCHEMA_VERSION {
            violations.push(format!(
                "schema_version {} is not interprocedural_fact_result.v1 ({})",
                self.schema_version, INTERPROCEDURAL_FACT_RESULT_SCHEMA_VERSION
            ));
        }
        if let Err(subject_violations) = self.subject.validate() {
            violations.extend(subject_violations.into_iter().map(|v| format!("subject: {v}")));
        }
        // Cross-generation binding: the result answers exactly the subject's
        // generation — a result for G2 must never validate while answering a
        // G1 subject, and a Fresh summary's generation must agree with the
        // subject's (#12672 operator review).
        match (&self.source_generation, &self.subject.source_generation) {
            (SourceGeneration::Known(result_gen), SourceGeneration::Known(subject_gen)) => {
                if result_gen != subject_gen {
                    violations.push(format!(
                        "result generation {result_gen} differs from the subject's generation \
                         {subject_gen} (cross-generation reuse)"
                    ));
                }
            }
            _ => violations
                .push("result and subject generations must both be known and equal".to_string()),
        }
        if let Some(summary) = &self.summary_ref
            && let SummaryCurrentness::Fresh(SourceGeneration::Known(fresh_gen)) =
                &summary.currentness
            && let SourceGeneration::Known(subject_gen) = &self.subject.source_generation
            && fresh_gen != subject_gen
        {
            violations.push(format!(
                "summary is Fresh({fresh_gen}) but the subject is generation {subject_gen} \
                 (cross-generation reuse)"
            ));
        }
        match &self.outcome {
            InterproceduralOutcome::Composed => {
                if self.facts.is_empty() {
                    violations.push(
                        "Composed with no facts is missing-as-empty: the outcome must name why \
                         (Refused, Conservative, Stale, Invalid, ResourceExhausted)"
                            .to_string(),
                    );
                }
                // Historical-as-current: a result may not be newer-claimed
                // than stale summary evidence.
                if let Some(summary) = &self.summary_ref
                    && matches!(summary.currentness, SummaryCurrentness::Stale)
                {
                    violations.push(
                        "Composed from a stale summary must be Stale, not Composed \
                         (historical-as-current)"
                            .to_string(),
                    );
                }
                // No strengthening of consumed evidence: every composed fact
                // must classify Exact or Degraded under the canonical
                // envelope classifier — a Refused or Stale fact stays at its
                // own status (#12672 review).
                for fact in &self.facts {
                    match fact.status() {
                        crate::SemanticFactStatus::Exact | crate::SemanticFactStatus::Degraded => {}
                        other => violations.push(format!(
                            "Composed must not promote a {other:?} fact (stale or refused \
                             evidence stays at its own status)"
                        )),
                    }
                }
                // Confidence ceiling: the result's confidence is never
                // stronger than the weakest consumed known evidence
                // (Confidence is declared High < Medium < Low in Ord).
                if let SemanticConfidence::Known(result_confidence) = self.confidence
                    && let Some(weakest) = self
                        .facts
                        .iter()
                        .filter_map(|fact| match fact.confidence {
                            SemanticConfidence::Known(confidence) => Some(confidence),
                            SemanticConfidence::Unknown => None,
                        })
                        .max()
                    && result_confidence < weakest
                {
                    violations.push(format!(
                        "result confidence {result_confidence:?} is stronger than the weakest \
                         consumed evidence {weakest:?} (confidence ceiling)"
                    ));
                }
            }
            InterproceduralOutcome::Refused { .. } | InterproceduralOutcome::Invalid { .. }
                if !self.facts.is_empty() =>
            {
                violations.push(
                    "a refused or invalid outcome must not carry facts (refusal-as-empty is \
                     required, not optional)"
                        .to_string(),
                );
            }
            InterproceduralOutcome::ResourceExhausted { units_consumed }
                if *units_consumed != self.units_consumed =>
            {
                // One authoritative unit count: the outcome's own accounting
                // must agree with the top-level field before any ceiling is
                // checked against it (#12672 review).
                violations.push(format!(
                    "ResourceExhausted.units_consumed {units_consumed} disagrees with the \
                     top-level units_consumed {} (one authoritative count)",
                    self.units_consumed
                ));
            }
            _ => {}
        }
        // Identity binding: an exact-target call may consume only a summary
        // of the exact callee; a dynamic-target call carries no summary at
        // all — its boundary owns the outcome (#12672 review).
        match &self.subject.callee {
            CallTarget::Exact(callee) => {
                if let Some(summary) = &self.summary_ref
                    && summary.callable != *callee
                {
                    violations.push(format!(
                        "summary_ref describes {:#?} but the exact callee is {:#?} (summary must \
                         bind to the exact callee)",
                        summary.callable, callee
                    ));
                }
            }
            CallTarget::DynamicBoundary(_) => {
                if self.summary_ref.is_some() {
                    violations.push(
                        "a dynamic-target call must not carry a summary — the boundary owns the \
                         outcome"
                            .to_string(),
                    );
                }
            }
        }
        if let Some(summary) = &self.summary_ref {
            let summary_violations = summary.validate();
            if let Err(summary_violations) = summary_violations {
                violations
                    .extend(summary_violations.into_iter().map(|v| format!("summary_ref: {v}")));
            }
            if self.units_consumed > summary.work.max_units {
                violations.push(format!(
                    "units_consumed {} exceeds the summary's work budget {} (resource ceiling)",
                    self.units_consumed, summary.work.max_units
                ));
            }
        }
        if self.boundaries.windows(2).any(|pair| pair[0] >= pair[1]) {
            violations.push(
                "boundaries must be strictly sorted and deduplicated (canonical ordering)"
                    .to_string(),
            );
        }
        if violations.is_empty() { Ok(()) } else { Err(violations) }
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// callable_semantic_summary.v1 (#12674, I02)
//
// The immutable per-callable packet contract. A packet joins EXISTING canonical
// compiler facts (HIR items, per-body PIR operations, envelope identities) by
// identity into one validated record per admitted callable. It composes no
// callee facts, resolves no calls, builds no call graph, and traverses no
// project: outbound calls are recorded as unresolved transitive dependencies
// that name exactly the facets they block.
// ──────────────────────────────────────────────────────────────────────────────

/// `callable_semantic_summary.v1` schema version.
pub const CALLABLE_SEMANTIC_SUMMARY_SCHEMA_VERSION: u32 = 1;

/// The facet vocabulary of a callable-local summary. Facets are independent:
/// one complete facet never strengthens another, and completeness is always
/// facet-specific. Declaration order is the canonical ordering.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum SummaryFacetKind {
    /// Return/result-value behavior.
    Result,
    /// Parameter binding behavior.
    ParameterBinding,
    /// Lexical/package places touched by the callable.
    Place,
    /// Side effects (assignments, modifications, stash access).
    Effect,
    /// Aliasing and escape behavior.
    AliasEscape,
    /// Diagnostics raised for the callable.
    Diagnostic,
    /// Exception/throw behavior.
    Exception,
    /// Control-flow behavior.
    Control,
    /// Compile-time effect behavior (phase blocks).
    CompileEffect,
    /// Dynamic/compatibility boundaries limiting the summary.
    Boundary,
    /// Outbound call dependencies.
    OutboundCall,
}

impl SummaryFacetKind {
    /// Every facet kind in canonical order. A valid summary carries exactly
    /// one [`FacetCompleteness`] entry per kind, in this order.
    pub const ALL: [SummaryFacetKind; 11] = [
        SummaryFacetKind::Result,
        SummaryFacetKind::ParameterBinding,
        SummaryFacetKind::Place,
        SummaryFacetKind::Effect,
        SummaryFacetKind::AliasEscape,
        SummaryFacetKind::Diagnostic,
        SummaryFacetKind::Exception,
        SummaryFacetKind::Control,
        SummaryFacetKind::CompileEffect,
        SummaryFacetKind::Boundary,
        SummaryFacetKind::OutboundCall,
    ];
}

/// Completeness status of one facet. `Complete` is lawful only when nothing
/// the facet depends on is missing, unsupported, or blocked by an unresolved
/// outbound call; `NotProven` declares that the substrate cannot prove the
/// facet at all (never a silent exact empty set).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum SummaryFacetStatus {
    /// The facet is fully evidenced for this callable.
    Complete,
    /// The facet is partially evidenced; the gap is declared in the counts.
    Limited,
    /// The substrate cannot prove this facet (unsupported or inapplicable);
    /// the declaration is explicit in the counts.
    NotProven,
}

/// Honest per-facet accounting. Counts declare what was planned, selected,
/// terminal, unsupported, and missing so a gap is never silently read as an
/// exact empty set.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct FacetCompleteness {
    /// The facet this entry accounts.
    pub facet: SummaryFacetKind,
    /// Completeness status of the facet.
    pub status: SummaryFacetStatus,
    /// Evidence units the assembly planned to cover.
    pub planned: u32,
    /// Evidence units actually selected into the packet.
    pub selected: u32,
    /// Terminal evidence units (e.g. explicit exits, recorded boundaries).
    pub terminal: u32,
    /// Units the substrate declared unsupported or inapplicable.
    pub unsupported: u32,
    /// Units known to be missing (unmodeled constructs, instrument gaps).
    pub missing: u32,
    /// Unresolved outbound dependencies that block this facet. For the
    /// `OutboundCall` facet itself the dependencies are the facet's content
    /// (counted in `selected`), so this stays zero; for every other facet a
    /// nonzero count blocks `Complete`.
    pub outbound_dependencies: u32,
}

impl FacetCompleteness {
    /// Construct one facet ledger entry.
    #[must_use]
    #[allow(clippy::too_many_arguments)] // the constructor mirrors the contract fields
    pub const fn new(
        facet: SummaryFacetKind,
        status: SummaryFacetStatus,
        planned: u32,
        selected: u32,
        terminal: u32,
        unsupported: u32,
        missing: u32,
        outbound_dependencies: u32,
    ) -> Self {
        Self {
            facet,
            status,
            planned,
            selected,
            terminal,
            unsupported,
            missing,
            outbound_dependencies,
        }
    }
}

/// Identity reference to one canonical compiler fact. References are by
/// identity only; nothing here copies fact content or re-derives it from
/// text, names, or paths.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum CallableFactRef {
    /// A canonical promoted envelope fact.
    Envelope(FactId),
    /// A per-body PIR operation: HIR body index plus PIR node id. PIR node
    /// ids are per-lowering identities — deterministic for the same source
    /// and lowering, meaningful only together with the body index and the
    /// lowering that produced them.
    PirOp {
        /// Index of the HIR body in the file's body arena.
        body: u32,
        /// PIR node id within that body's lowering.
        op: u64,
    },
    /// A dynamic/compatibility boundary reference.
    Boundary(BoundaryLink),
    /// A canonical HIR item identity (`HirId` index). Used for canonical HIR
    /// facts the per-body PIR lowering does not yet model — outbound calls
    /// and dynamic-boundary items — so the packet never aliases two
    /// different PIR id spaces into one `PirOp` identity.
    HirItem(u64),
}

/// What an outbound call targets, at the precision the evidence supports.
/// Names are display/identity passthroughs exactly as recorded by HIR/PIR —
/// never re-resolved.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum OutboundCallee {
    /// A statically written callee name, package-qualified when the source
    /// qualified it. Passthrough, not re-resolution.
    Named(String),
    /// The callee crosses a dynamic boundary (coderef, symbolic, dynamic
    /// receiver or method name); the boundary is carried, never dropped.
    Dynamic(BoundaryLink),
    /// The callee shape is not established.
    Unknown,
}

/// Resolution state of an outbound call dependency. I02 performs no
/// composition: every recorded dependency is unresolved-transitive, and the
/// facets it blocks stay limited until a later composition issue (I03/I04)
/// resolves it.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum CallResolution {
    /// Recorded as an unresolved transitive dependency; no callee facts were
    /// composed and no purity/emptiness/non-throwing is inferred.
    UnresolvedTransitive,
}

/// One outbound call recorded as an unresolved transitive dependency. An
/// unresolved call is never treated as pure, empty, or non-throwing:
/// `blocked_facets` names exactly the facets it blocks, and is never empty.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutboundCallDependency {
    /// Identity of the call fact.
    pub call: CallableFactRef,
    /// Source anchor of the call site, when source-backed.
    pub anchor: Option<SourceAnchor>,
    /// What the call targets, at the precision the evidence supports.
    pub callee: OutboundCallee,
    /// Facets this unresolved call blocks, in canonical order, deduplicated
    /// at construction. Always non-empty.
    pub blocked_facets: Vec<SummaryFacetKind>,
    /// Resolution state (always [`CallResolution::UnresolvedTransitive`] in
    /// this schema version).
    pub resolution: CallResolution,
}

impl OutboundCallDependency {
    /// Construct with canonical blocked-facet ordering applied (sorted,
    /// deduplicated) so two assemblies of the same call produce identical
    /// canonical bytes. An empty `blocked_facets` is a validation violation,
    /// never a silent "pure" call.
    #[must_use]
    pub fn new(
        call: CallableFactRef,
        anchor: Option<SourceAnchor>,
        callee: OutboundCallee,
        mut blocked_facets: Vec<SummaryFacetKind>,
        resolution: CallResolution,
    ) -> Self {
        blocked_facets.sort();
        blocked_facets.dedup();
        Self { call, anchor, callee, blocked_facets, resolution }
    }
}

/// Kind of result exit from a callable.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ResultExitKind {
    /// `return EXPR` — an explicit value return.
    ExplicitReturn,
    /// Falling off the end of the body — Perl's implicit last-expression or
    /// void result. Every callable body has exactly one, recorded last.
    ImplicitFallthrough,
    /// Bare `return;` — a value-less return, recorded only when the HIR/PIR
    /// evidence actually distinguishes it from `return EXPR`.
    BareReturn,
}

/// One result exit of the callable, in source (lowered) order.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResultExitRef {
    /// Exit kind.
    pub kind: ResultExitKind,
    /// The exit's source fact, when one exists (the implicit fallthrough exit
    /// has no op of its own and carries `None`).
    pub source: Option<CallableFactRef>,
    /// Source anchor of the exit, when source-backed.
    pub anchor: Option<SourceAnchor>,
}

impl ResultExitRef {
    /// Construct one result exit reference.
    #[must_use]
    pub const fn new(
        kind: ResultExitKind,
        source: Option<CallableFactRef>,
        anchor: Option<SourceAnchor>,
    ) -> Self {
        Self { kind, source, anchor }
    }
}

/// Lexical role of a place reference.
///
/// This mirrors the INTERIM lexical-role vocabulary owned by perl-parser-core
/// (`pir::extractor::LexicalRole`, `pir::lexical_contribution::OccurrenceRole`,
/// authority issue #2660). It is a passthrough for identity joins, NOT a new
/// authority: roles are mapped one-to-one from the PIR operations that name
/// them and gain no new semantics here.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum PlaceRole {
    /// Binding introduction site.
    Declaration,
    /// Value read.
    Read,
    /// Plain value write.
    Write,
    /// Compound read-modify-write (`+=`, `++`, ...).
    Modify,
}

/// One binding/place reference of the callable, in source (lowered) order.
/// Places are identified by their canonical fact identity, never grouped by
/// spelling or range alone.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BindingPlaceRef {
    /// Sigil + name as recorded by the substrate. Display/identity
    /// passthrough; identity is the `source` reference, not this string.
    pub name: String,
    /// Lexical role of the reference.
    pub role: PlaceRole,
    /// Identity of the place fact.
    pub source: CallableFactRef,
    /// Source anchor, when source-backed.
    pub anchor: Option<SourceAnchor>,
}

impl BindingPlaceRef {
    /// Construct one binding/place reference.
    #[must_use]
    pub const fn new(
        name: String,
        role: PlaceRole,
        source: CallableFactRef,
        anchor: Option<SourceAnchor>,
    ) -> Self {
        Self { name, role, source, anchor }
    }
}

/// Effect category of one observed side effect.
///
/// Mirrors the PIR operation categories one-to-one (passthrough, no new
/// effect vocabulary): `Assign`, `Modify`, `StashRead`, `StashWrite`,
/// `StashModify`.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum EffectKind {
    /// An assignment expression.
    Assign,
    /// Compound read-modify-write on a lexical place.
    Modify,
    /// Package/stash symbol read.
    StashRead,
    /// Package/stash symbol write.
    StashWrite,
    /// Compound read-modify-write on a package/stash symbol.
    StashModify,
}

/// One effect of the callable, in source (lowered) order.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectRef {
    /// Effect category.
    pub kind: EffectKind,
    /// Identity of the effect fact.
    pub source: CallableFactRef,
    /// Source anchor, when source-backed.
    pub anchor: Option<SourceAnchor>,
}

impl EffectRef {
    /// Construct one effect reference.
    #[must_use]
    pub const fn new(
        kind: EffectKind,
        source: CallableFactRef,
        anchor: Option<SourceAnchor>,
    ) -> Self {
        Self { kind, source, anchor }
    }
}

/// One provenance edge from the packet to an observed boundary SITE, in
/// source order.
///
/// Distinct from the envelope's `referenced_boundaries`: that set dedups by
/// semantic boundary identity (correct — equivalent facts share identity),
/// while `boundary_sites` retains every provenance edge (the issue's
/// identity/normalization law). Two `eval` sites with the same boundary kind
/// dedup to one envelope link but keep two site records with their own
/// anchors.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoundarySiteRef {
    /// Boundary category observed at this site.
    pub kind: BoundaryKind,
    /// Identity of the boundary site fact.
    pub source: CallableFactRef,
    /// Source anchor of the site, when source-backed.
    pub anchor: Option<SourceAnchor>,
}

impl BoundarySiteRef {
    /// Construct one boundary site reference.
    #[must_use]
    pub const fn new(
        kind: BoundaryKind,
        source: CallableFactRef,
        anchor: Option<SourceAnchor>,
    ) -> Self {
        Self { kind, source, anchor }
    }
}

/// Bounded work accounting for one summary. The work law: zero useful
/// visited operations can never satisfy a required summary.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SummaryWorkLedger {
    /// Callables the assembly planned to summarize (per-packet: 1).
    pub planned_callables: u32,
    /// Callables actually visited (per-packet: 1).
    pub visited_callables: u32,
    /// Evidence units offered to the walk (body expressions + statements +
    /// attributed flat items). The lowering may model one expression as
    /// several operations, so `visited_ops` can honestly exceed
    /// `planned_ops`; the two counts are independent accountings, not a
    /// ceiling relation.
    pub planned_ops: u32,
    /// Operations actually visited. Must be positive: an empty walk never
    /// satisfies a required summary.
    pub visited_ops: u32,
    /// Budget units consumed by the assembly.
    pub units_consumed: u32,
    /// Canonical JSON byte length of this packet at assembly time.
    pub bytes_retained: u64,
}

impl SummaryWorkLedger {
    /// Construct one work ledger.
    #[must_use]
    pub const fn new(
        planned_callables: u32,
        visited_callables: u32,
        planned_ops: u32,
        visited_ops: u32,
        units_consumed: u32,
        bytes_retained: u64,
    ) -> Self {
        Self {
            planned_callables,
            visited_callables,
            planned_ops,
            visited_ops,
            units_consumed,
            bytes_retained,
        }
    }
}

/// One immutable callable-local semantic summary packet
/// (`callable_semantic_summary.v1`). The packet joins existing canonical
/// compiler facts by identity for direct intraprocedural behavior only;
/// outbound calls are recorded as unresolved transitive dependencies.
///
/// Invariants enforced by [`CallableSemanticSummary::validate`] (fail-closed):
///
/// - exactly one facet ledger entry per [`SummaryFacetKind`], canonical order;
/// - a facet with `missing > 0` or `outbound_dependencies > 0` is never
///   `Complete`, and no facet named in any dependency's `blocked_facets` is
///   `Complete` (an unresolved call is never pure/empty/non-throwing);
/// - every dependency carries a non-empty, canonically ordered
///   `blocked_facets`;
/// - `work.visited_ops > 0` (the work law);
/// - a stale summary never carries `Exact` claims, and `Exact` claims require
///   every facet `Complete` (completeness is facet-specific);
/// - source/evaluation order is preserved verbatim for exits, bindings,
///   effects, and outbound calls; canonical (sorted) order applies to
///   identity sets only.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallableSemanticSummary {
    /// Schema version ([`CALLABLE_SEMANTIC_SUMMARY_SCHEMA_VERSION`]).
    pub schema_version: u32,
    /// The callable entity this summary describes.
    pub callable: EntityId,
    /// Callable name as recorded, display only — never identity.
    pub callable_name: Option<String>,
    /// Content identity of the callable body.
    pub body: BodyIdentity,
    /// Source generation the summary was assembled for.
    pub source_generation: SourceGeneration,
    /// Source anchor of the callable declaration.
    pub anchor: SourceAnchor,
    /// The I01 reference envelope: composition policy, result facets,
    /// currentness, work budget, refusal/claim ceilings, privacy, and the
    /// canonical referenced fact/boundary identity sets.
    pub summary_ref: CallableSemanticSummaryRef,
    /// Facet completeness ledger — exactly one entry per
    /// [`SummaryFacetKind`], in canonical order.
    pub facets: Vec<FacetCompleteness>,
    /// Result exits in source (lowered) order. Exactly one
    /// [`ResultExitKind::ImplicitFallthrough`], recorded last.
    pub result_exits: Vec<ResultExitRef>,
    /// Binding/place references in source (lowered) order.
    pub bindings: Vec<BindingPlaceRef>,
    /// Effects in source (lowered) order.
    pub effects: Vec<EffectRef>,
    /// Outbound call dependencies in source (lowered) order.
    pub outbound_calls: Vec<OutboundCallDependency>,
    /// Every observed boundary site in source order — the per-site
    /// provenance record. The envelope's `referenced_boundaries` dedups by
    /// semantic boundary identity; this list keeps each edge.
    pub boundary_sites: Vec<BoundarySiteRef>,
    /// Work accounting for this summary.
    pub work: SummaryWorkLedger,
}

impl CallableSemanticSummary {
    /// Construct one packet. Source-ordered lists (`result_exits`,
    /// `bindings`, `effects`, `outbound_calls`, `boundary_sites`) are
    /// preserved verbatim;
    /// canonical ordering applies to identity sets (enforced by
    /// [`CallableSemanticSummaryRef::new`] and
    /// [`OutboundCallDependency::new`]) and is checked fail-closed by
    /// [`CallableSemanticSummary::validate`].
    #[must_use]
    #[allow(clippy::too_many_arguments)] // the constructor mirrors the contract fields
    pub fn new(
        callable: EntityId,
        callable_name: Option<String>,
        body: BodyIdentity,
        source_generation: SourceGeneration,
        anchor: SourceAnchor,
        summary_ref: CallableSemanticSummaryRef,
        facets: Vec<FacetCompleteness>,
        result_exits: Vec<ResultExitRef>,
        bindings: Vec<BindingPlaceRef>,
        effects: Vec<EffectRef>,
        outbound_calls: Vec<OutboundCallDependency>,
        boundary_sites: Vec<BoundarySiteRef>,
        work: SummaryWorkLedger,
    ) -> Self {
        Self {
            schema_version: CALLABLE_SEMANTIC_SUMMARY_SCHEMA_VERSION,
            callable,
            callable_name,
            body,
            source_generation,
            anchor,
            summary_ref,
            facets,
            result_exits,
            bindings,
            effects,
            outbound_calls,
            boundary_sites,
            work,
        }
    }

    /// Canonical JSON bytes of this packet. Canonical serialization is
    /// deterministic: two packets that are structurally equal serialize to
    /// byte-identical output. Fail-closed: a serialization failure is an
    /// error, never a partial byte string.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }

    /// Validate the packet's own invariants (fail-closed).
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut violations = Vec::new();
        if self.schema_version != CALLABLE_SEMANTIC_SUMMARY_SCHEMA_VERSION {
            violations.push(format!(
                "schema_version {} is not callable_semantic_summary.v1 ({})",
                self.schema_version, CALLABLE_SEMANTIC_SUMMARY_SCHEMA_VERSION
            ));
        }
        if self.anchor.end_byte < self.anchor.start_byte {
            violations.push("callable anchor end precedes its start".to_string());
        }
        if self.anchor.file_id == NO_FILE {
            violations.push("callable anchor must name a real file identity".to_string());
        }
        if let BodyIdentity::Exact(hash) = &self.body
            && hash.is_empty()
        {
            violations.push("an Exact body identity must not be empty".to_string());
        }
        if let Some(name) = &self.callable_name
            && name.is_empty()
        {
            violations.push("a present callable_name must not be empty".to_string());
        }
        // The embedded envelope must be valid and describe the same callable
        // and generation (one subject, one identity).
        if let Err(ref_violations) = self.summary_ref.validate() {
            violations.extend(ref_violations.into_iter().map(|v| format!("summary_ref: {v}")));
        }
        if self.summary_ref.callable != self.callable {
            violations.push(
                "summary_ref describes a different callable than the packet (one subject, one \
                 identity)"
                    .to_string(),
            );
        }
        if self.summary_ref.source_generation != self.source_generation {
            violations.push(
                "summary_ref generation disagrees with the packet generation (one freshness \
                 identity)"
                    .to_string(),
            );
        }
        // Stale reuse, re-checked at the packet join: a stale summary must
        // not present Exact claims.
        if matches!(self.summary_ref.currentness, SummaryCurrentness::Stale)
            && self.summary_ref.claim_ceiling == ClaimCeiling::Exact
        {
            violations.push(
                "a stale summary must not present Exact claims (historical-as-current)".to_string(),
            );
        }
        // Facet ledger: exactly one entry per kind, canonical order.
        if self.facets.len() != SummaryFacetKind::ALL.len() {
            violations.push(format!(
                "facet ledger must carry exactly {} entries (one per facet kind), found {}",
                SummaryFacetKind::ALL.len(),
                self.facets.len()
            ));
        }
        for (index, entry) in self.facets.iter().enumerate() {
            if let Some(expected) = SummaryFacetKind::ALL.get(index)
                && entry.facet != *expected
            {
                violations.push(
                    "facet ledger entries must be exactly one per kind in canonical order"
                        .to_string(),
                );
                break;
            }
        }
        // Completeness is facet-specific and honest: missing, unsupported,
        // or blocking evidence always limits the facet it touches.
        for entry in &self.facets {
            if entry.status == SummaryFacetStatus::Complete
                && (entry.missing > 0 || entry.unsupported > 0 || entry.outbound_dependencies > 0)
            {
                violations.push(format!(
                    "facet {:?} is Complete with missing={} unsupported={} outbound_dependencies={} \
                     (missing/unsupported/blocked evidence can never be Complete)",
                    entry.facet, entry.missing, entry.unsupported, entry.outbound_dependencies
                ));
            }
        }
        // Boundary provenance: the Boundary facet's ledger count must agree
        // with the packet's site record — never a deduped or dropped count.
        if let Some(entry) =
            self.facets.iter().find(|entry| entry.facet == SummaryFacetKind::Boundary)
            && entry.selected as usize != self.boundary_sites.len()
        {
            violations.push(format!(
                "Boundary facet selected {} disagrees with {} recorded boundary sites \
                 (site/ledger mismatch)",
                entry.selected,
                self.boundary_sites.len()
            ));
        }
        // Outbound dependencies: never pure/empty/non-throwing. Every
        // dependency names a non-empty canonically ordered blocked set, and
        // every facet it names is not Complete in the ledger.
        for dependency in &self.outbound_calls {
            if dependency.blocked_facets.is_empty() {
                violations.push(
                    "an unresolved outbound call must name the facets it blocks (never \
                     pure/empty/non-throwing)"
                        .to_string(),
                );
            }
            if dependency.blocked_facets.windows(2).any(|pair| pair[0] >= pair[1]) {
                violations.push(
                    "blocked_facets must be strictly sorted and deduplicated (canonical ordering)"
                        .to_string(),
                );
            }
            if let OutboundCallee::Named(name) = &dependency.callee
                && name.is_empty()
            {
                violations.push("a Named outbound callee must not be empty".to_string());
            }
            for blocked in &dependency.blocked_facets {
                if let Some(entry) = self.facets.iter().find(|entry| entry.facet == *blocked)
                    && entry.status == SummaryFacetStatus::Complete
                {
                    violations.push(format!(
                        "facet {blocked:?} is Complete but an unresolved outbound call blocks it \
                         (cross-facet completeness join)"
                    ));
                }
            }
        }
        // Result exits: every callable has exactly one implicit fallthrough
        // exit, recorded last; source order is otherwise preserved verbatim.
        let fallthroughs = self
            .result_exits
            .iter()
            .enumerate()
            .filter(|(_, exit)| exit.kind == ResultExitKind::ImplicitFallthrough)
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if fallthroughs.len() != 1
            || fallthroughs.first().copied() != self.result_exits.len().checked_sub(1)
        {
            violations.push(
                "exactly one ImplicitFallthrough exit must be recorded, last in source order"
                    .to_string(),
            );
        }
        // The work law: zero useful visited operations can never satisfy a
        // required summary.
        if self.work.visited_ops == 0 {
            violations.push(
                "visited_ops is zero: an empty walk never satisfies a required summary (work law)"
                    .to_string(),
            );
        }
        if self.work.visited_callables == 0
            || self.work.visited_callables > self.work.planned_callables
        {
            violations.push(
                "visited_callables must be at least one and never exceed planned_callables"
                    .to_string(),
            );
        }
        // Claim-ceiling join: Exact claims require every facet Complete —
        // one limited facet limits the whole claim ceiling, never the
        // reverse.
        if self.summary_ref.claim_ceiling == ClaimCeiling::Exact
            && self.facets.iter().any(|entry| entry.status != SummaryFacetStatus::Complete)
        {
            violations.push(
                "Exact claims require every facet Complete (completeness is facet-specific)"
                    .to_string(),
            );
        }
        if violations.is_empty() { Ok(()) } else { Err(violations) }
    }
}

#[cfg(test)]
mod tests;
