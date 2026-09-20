//! Versioned schema only: no interpreter, timeline, catalog or provider authority.
//!
//! SourceIdentityEnvelope identifies a logical file; SemanticSubjectGeneration
//! identifies an accepted document instance and parser/profile configuration.
//! Binding preserves both, checks shared generation labels, and requires every
//! scope/port to use that exact semantic subject. Compiler generation is an
//! additional explicit binding. SemanticProvenance and LifecyclePhase retain
//! their shared meanings. Facet outcomes are local, never a confidence score.
//!
//! Public wire records are untrusted drafts. Only `admit`/bounded `read` produce
//! validated states/transitions. SHA-256 hashes canonical, domain-tagged payloads,
//! excluding their own digest. Sets are sorted and unique; transition order is
//! retained. Limits are operational defaults, not Perl language restrictions.

mod validation;
mod wire;
use perl_semantic_facts::semantic_identity::{
    SemanticScopeIdentity, SemanticSourceAnchor, SemanticSourceOrderIdentity,
    SemanticSubjectGeneration,
};
use perl_semantic_facts::{LifecyclePhase, SemanticProvenance};
use perl_source_identity::{ContentDigest, SourceIdentityEnvelope};
use serde::{Deserialize, Serialize};
use std::io::Read;
pub use wire::{MAX_BUNDLE_BYTES, MAX_DEPTH, MAX_SNAPSHOT_BYTES};

/// Maximum transitions in one source bundle.
pub const MAX_TRANSITIONS: usize = 65_536;
/// Maximum provenance or boundary references per record.
pub const MAX_REFERENCES: usize = 256;
/// Maximum custom names per feature or warning facet.
pub const MAX_CUSTOM_NAMES: usize = 1_024;
/// Maximum UTF-8 bytes in a custom name or schema-local identifier.
pub const MAX_NAME_BYTES: usize = 256;
/// Maximum lexical builtin imports.
pub const MAX_BUILTINS: usize = 4_096;
/// Maximum entries in one transition delta.
pub const MAX_DELTA_ENTRIES: usize = 4_096;

/// Admission failure; no failure yields a truncated exact value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchemaError {
    /// Invalid or contradictory semantic input.
    Invalid(String),
    /// A declared operational bound was exceeded.
    Limited(&'static str),
    /// IO or serialization instrument failed.
    Instrument(String),
    /// A requested exact projection is unavailable.
    Unavailable,
}
impl std::fmt::Display for SchemaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for SchemaError {}

/// Independent strict categories; features cannot satisfy these bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StrictCategories {
    /// Strict variables.
    pub vars: bool,
    /// Strict barewords.
    pub subs: bool,
    /// Strict symbolic references.
    pub refs: bool,
}
impl StrictCategories {
    /// Full strict requires all three categories.
    pub fn full_strict(self) -> bool {
        self.vars && self.subs && self.refs
    }
}

/// Local fact disposition; payload presence never upgrades it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// Exact value for the bound subject.
    Exact,
    /// Value depends on an unresolved condition.
    Conditional,
    /// Producer intentionally provides limited coverage.
    Limited,
    /// Producer cannot model this fact.
    Unsupported,
    /// Value belongs to an older subject.
    Stale,
    /// Required input is absent.
    Unavailable,
    /// Resource or instrument failure prevented production.
    ResourceInstrument,
}

/// Provenance reference, retaining the shared provenance classification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Origin {
    /// Stable producer/effect reference.
    pub id: String,
    /// Shared provenance grade.
    pub provenance: SemanticProvenance,
    /// Logical source anchor.
    pub anchor: SemanticSourceAnchor,
}

/// Qualified facet draft. Exact requires a value and exact-grade origins.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Facet<T> {
    /// Local disposition.
    pub outcome: Outcome,
    /// Known value, if any; even nonempty values retain their disposition.
    pub value: Option<T>,
    /// Required explanation for non-exact outcomes.
    pub reason: Option<String>,
    /// Origin references for this facet's changes.
    pub origins: Vec<Origin>,
}
impl<T> Facet<T> {
    /// Borrow an exact value without interpreting unknown as empty.
    pub fn exact(&self) -> Result<&T, SchemaError> {
        if self.outcome != Outcome::Exact {
            return Err(SchemaError::Unavailable);
        }
        self.value.as_ref().ok_or(SchemaError::Unavailable)
    }
}

/// Explicit correspondence, preserving file and document-instance identities.
///
/// The producer declares that both source-generation slots refer to the same
/// accepted opaque cursor domain. Labels compare byte-for-byte; they are never
/// parsed or numerically normalized. Matching labels alone do not identify
/// documents or sessions. The complete source and document-instance subjects
/// remain load-bearing. This schema validates the declaration, not its producer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    /// Canonical source file and revision identity.
    pub source: SourceIdentityEnvelope,
    /// Accepted document-instance, parser and semantic-profile identity.
    pub semantic: SemanticSubjectGeneration,
    /// Compiler configuration/generation identity, absent from semantic subject.
    pub compiler_generation: String,
}

/// Closed authority roles prevent cross-facet substitution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PortRole {
    /// Warning-policy contract.
    WarningPolicy,
    /// Language-profile contract.
    LanguageProfile,
    /// Structured directive/effect contract.
    DirectiveEffect,
    /// Fact-impact boundary contract.
    Boundary,
}

/// An unimplemented authority port, bound to a subject and payload digest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorityPort {
    /// Destination authority role; opaque payloads do not supply this role.
    pub role: PortRole,
    /// Port schema version (currently 1).
    pub schema_version: u32,
    /// Stable contract identifier; not a host path.
    pub contract: String,
    /// Semantic subject to which the payload belongs.
    pub subject: SemanticSubjectGeneration,
    /// Compiler generation to which the payload belongs.
    pub compiler_generation: String,
    /// Content identity of the referenced semantic payload.
    pub payload: ContentDigest,
}

/// Fact classes affected by a boundary or delta.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FactClass {
    /// Strict categories.
    Strict,
    /// Warning policy.
    Warnings,
    /// Declared version and requirements.
    Versions,
    /// Language features.
    Features,
    /// Lexical builtin imports.
    Builtins,
    /// Source encoding.
    Encoding,
    /// Locale policy.
    Locale,
    /// Language profile.
    Profile,
}

/// Boundary reference qualified independently from unrelated fact classes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Boundary {
    /// Stable boundary identifier.
    pub id: String,
    /// Affected fact classes; nonempty set.
    pub affects: Vec<FactClass>,
    /// Qualified boundary authority port.
    pub authority: Facet<AuthorityPort>,
}

/// Named feature/builtin identity with its change origin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NamedFact {
    /// Retained known or custom name.
    pub name: String,
    /// Whether the producer recognizes this name.
    pub known: bool,
    /// Origin of the named fact.
    pub origin: Origin,
}

/// Encoding state independent from feature policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EncodingState {
    /// UTF-8 source pragma state.
    pub utf8: bool,
    /// Explicit encoding name; absence means no explicit encoding.
    pub encoding: Option<String>,
}

/// Locale state independent from encoding and Unicode features.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocaleState {
    /// Whether locale semantics apply.
    pub enabled: bool,
    /// Named locale categories.
    pub categories: Vec<String>,
}

/// Explicit absence is distinct from an unavailable qualified version facet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "spelling", rename_all = "snake_case", deny_unknown_fields)]
pub enum VersionDeclaration {
    /// No source version declaration.
    Absent,
    /// Retained declaration spelling; no version-table interpretation.
    Declared(String),
}

/// Versioned untrusted state draft; admission validates every nested value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StateDraft {
    /// Current schema version, 1.
    pub schema_version: u32,
    /// Explicit source/semantic/compiler correspondence.
    pub binding: Binding,
    /// Lexical scope identity.
    pub scope: SemanticScopeIdentity,
    /// Package context, if present.
    pub package: Option<SemanticSourceOrderIdentity>,
    /// Shared phase vocabulary.
    pub phase: LifecyclePhase,
    /// Qualified language-profile authority.
    pub profile: Facet<AuthorityPort>,
    /// Declared version spelling or explicit absence.
    pub version: Facet<VersionDeclaration>,
    /// Ordered-independent version requirement identities, not evaluated versions.
    pub requirements: Facet<Vec<String>>,
    /// Independent strict variable category.
    pub strict_vars: Facet<bool>,
    /// Independent strict subroutine category.
    pub strict_subs: Facet<bool>,
    /// Independent strict reference category.
    pub strict_refs: Facet<bool>,
    /// Qualified warning-policy port, never an inferred empty catalog.
    pub warnings: Facet<AuthorityPort>,
    /// Retained custom warning names, without catalog interpretation.
    pub warning_names: Vec<String>,
    /// Effective feature identities and origins.
    pub features: Facet<Vec<NamedFact>>,
    /// Lexical builtin imports and origins.
    pub builtins: Facet<Vec<NamedFact>>,
    /// Source encoding state.
    pub encoding: Facet<EncodingState>,
    /// Locale state.
    pub locale: Facet<LocaleState>,
    /// Fact-local boundaries.
    pub boundaries: Vec<Boundary>,
}

/// Admitted canonical state. There is deliberately no Deserialize implementation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompileEnvironmentState(StateDraft);
impl CompileEnvironmentState {
    /// Validate and canonicalize a caller-owned draft.
    pub fn admit(mut draft: StateDraft) -> Result<Self, SchemaError> {
        validation::state(&mut draft)?;
        wire::canonical("compile_environment_state.v1", &draft, MAX_SNAPSHOT_BYTES)?;
        Ok(Self(draft))
    }
    /// Read bounded untrusted JSON, including whitespace in the byte budget.
    pub fn read(reader: impl Read) -> Result<Self, SchemaError> {
        Self::admit(wire::read(reader, MAX_SNAPSHOT_BYTES)?)
    }
    /// Borrow the validated canonical draft.
    pub fn draft(&self) -> &StateDraft {
        &self.0
    }
    /// Deterministic schema wire bytes.
    pub fn to_json(&self) -> Result<Vec<u8>, SchemaError> {
        wire::encode(&self.0, MAX_SNAPSHOT_BYTES)
    }
    /// SHA-256 identity over canonical domain-tagged semantic bytes.
    pub fn digest(&self) -> Result<ContentDigest, SchemaError> {
        Ok(ContentDigest::of_bytes(&wire::canonical(
            "compile_environment_state.v1",
            &self.0,
            MAX_SNAPSHOT_BYTES,
        )?))
    }
    /// Exact strict bits, independent of feature names including signatures.
    pub fn strict(&self) -> Result<StrictCategories, SchemaError> {
        Ok(StrictCategories {
            vars: *self.0.strict_vars.exact()?,
            subs: *self.0.strict_subs.exact()?,
            refs: *self.0.strict_refs.exact()?,
        })
    }
    /// One-way strict-only projection; never fills unknown warning/version facets.
    pub fn project_strict(&self, legacy: &mut crate::PragmaState) -> Result<(), SchemaError> {
        let value = self.strict()?;
        legacy.strict_vars = value.vars;
        legacy.strict_subs = value.subs;
        legacy.strict_refs = value.refs;
        legacy.signatures_strict = false;
        Ok(())
    }
}

/// Typed delta; values remain qualified and no application semantics are implied.
///
/// The v1 wire uses one external variant key, e.g. `{"strict_vars": { ... }}`.
/// This preserves ignored-field tracking through nested shared records regardless
/// of payload key order. The unpublished adjacent `facet`/`value` draft is rejected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Delta {
    /// Strict vars replacement, independently qualified.
    StrictVars(Facet<bool>),
    /// Strict subs replacement, independently qualified.
    StrictSubs(Facet<bool>),
    /// Strict refs replacement, independently qualified.
    StrictRefs(Facet<bool>),
    /// Warning authority replacement.
    Warnings(Facet<AuthorityPort>),
    /// Declared version replacement.
    Version(Facet<VersionDeclaration>),
    /// Version requirements replacement.
    Requirements(Facet<Vec<String>>),
    /// Feature set replacement.
    Features(Facet<Vec<NamedFact>>),
    /// Builtin set replacement.
    Builtins(Facet<Vec<NamedFact>>),
    /// Encoding replacement.
    Encoding(Facet<EncodingState>),
    /// Locale replacement.
    Locale(Facet<LocaleState>),
    /// Profile authority replacement.
    Profile(Facet<AuthorityPort>),
}
/// Origin of a transition, without evaluating it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransitionKind {
    /// Source directive.
    Directive,
    /// Restore the identified prior state on scope exit.
    ScopeRestore,
    /// Seed from a language profile.
    ProfileSeed,
    /// Record a semantic boundary.
    Boundary,
}
/// Versioned transition draft.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransitionDraft {
    /// Schema version, 1.
    pub schema_version: u32,
    /// Stable transition identity within the source subject.
    pub id: String,
    /// Qualified structured-effect authority.
    pub effect: Facet<AuthorityPort>,
    /// Bound source, parser, profile and compiler generations.
    pub binding: Binding,
    /// Owning lexical scope.
    pub scope: SemanticScopeIdentity,
    /// Package context.
    pub package: Option<SemanticSourceOrderIdentity>,
    /// Compile/runtime phase.
    pub phase: LifecyclePhase,
    /// Logical source order, including same-offset disambiguation.
    pub order: SemanticSourceOrderIdentity,
    /// Byte anchor in the bound source revision.
    pub byte_anchor: u64,
    /// Transition kind.
    pub kind: TransitionKind,
    /// Identity of the prior canonical state.
    pub before: ContentDigest,
    /// Identity of the resulting/restored canonical state.
    pub after: ContentDigest,
    /// Typed qualified changes, in producer order.
    pub delta: Vec<Delta>,
    /// Affected fact classes.
    pub affects: Vec<FactClass>,
    /// Provenance references.
    pub origins: Vec<Origin>,
    /// Fact-local boundaries.
    pub boundaries: Vec<Boundary>,
}
/// Admitted transition with stable semantic digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompileEnvironmentTransition(TransitionDraft);
impl CompileEnvironmentTransition {
    /// Validate a draft without applying it.
    pub fn admit(mut draft: TransitionDraft) -> Result<Self, SchemaError> {
        validation::transition(&mut draft)?;
        wire::canonical("compile_environment_transition.v1", &draft, MAX_SNAPSHOT_BYTES)?;
        Ok(Self(draft))
    }
    /// Borrow the canonical transition.
    pub fn draft(&self) -> &TransitionDraft {
        &self.0
    }
    /// Domain-separated semantic digest.
    pub fn digest(&self) -> Result<ContentDigest, SchemaError> {
        Ok(ContentDigest::of_bytes(&wire::canonical(
            "compile_environment_transition.v1",
            &self.0,
            MAX_SNAPSHOT_BYTES,
        )?))
    }
}
/// Admitted source-ordered transition bundle, bounded before JSON allocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransitionBundle(Vec<CompileEnvironmentTransition>);
impl TransitionBundle {
    /// Validate source-order continuity without sorting transitions.
    ///
    /// The total order is `(byte_anchor, order.context_ordinal())` in the single
    /// bound source. At one byte anchor, producers assign increasing ordinals
    /// across contexts. Context digests identify contexts, never sort them;
    /// different digests cannot disambiguate a tied numeric coordinate.
    pub fn admit(drafts: Vec<TransitionDraft>) -> Result<Self, SchemaError> {
        validation::count(drafts.len(), MAX_TRANSITIONS, "transitions")?;
        let mut transitions: Vec<CompileEnvironmentTransition> = Vec::new();
        let mut ids = std::collections::BTreeSet::new();
        for draft in drafts {
            let next = CompileEnvironmentTransition::admit(draft)?;
            if !ids.insert(next.0.id.clone()) {
                return Err(SchemaError::Invalid("duplicate transition id".into()));
            }
            if let Some(previous) = transitions.last()
                && (previous.0.binding != next.0.binding
                    || previous.0.after != next.0.before
                    || (previous.0.byte_anchor, previous.0.order.context_ordinal())
                        >= (next.0.byte_anchor, next.0.order.context_ordinal()))
            {
                return Err(SchemaError::Invalid(
                    "transition subject/order/state chain mismatch".into(),
                ));
            }
            transitions.push(next);
        }
        let result = Self(transitions);
        result.to_json()?;
        Ok(result)
    }
    /// Decode with aggregate wire cap, including whitespace.
    pub fn read(reader: impl Read) -> Result<Self, SchemaError> {
        Self::admit(wire::read(reader, MAX_BUNDLE_BYTES)?)
    }
    /// Retained source order.
    pub fn transitions(&self) -> &[CompileEnvironmentTransition] {
        &self.0
    }
    /// Deterministic bounded bundle wire bytes.
    pub fn to_json(&self) -> Result<Vec<u8>, SchemaError> {
        wire::encode(&self.0.iter().map(|item| &item.0).collect::<Vec<_>>(), MAX_BUNDLE_BYTES)
    }
}
