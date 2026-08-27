//! Dependency-neutral versioned contracts for interprocedural composition
//! (#12672, programme #12671).
//!
//! Three contracts:
//!
//! - [`CallApplicationSubject`] (`call_application_subject.v1`) — one exact
//!   caller/callee/call/input/context subject;
//! - [`CallableSemanticSummaryRef`] (`callable_semantic_summary_ref.v1`) — a
//!   reference envelope identifying one callable-local semantic subject and
//!   the canonical facts it references, with composition policy, result
//!   facets, currentness, work, refusal and claim ceilings;
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
    BoundaryLink, EntityId, FactId, FileId, ScopeId, SemanticConfidence, SemanticProvenance,
    SemanticReasonCode, SourceAnchor, SourceGeneration,
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

#[cfg(test)]
mod tests;
