//! In-memory domain model and closure laws for maintained compiler operating
//! profiles (#12186, train row COMP-PROFILE-C01, parent #12176).
//!
//! This module owns the dependency-neutral, in-memory vocabulary for maintained
//! compiler operating profiles: exact identity, row dispositions, imports,
//! subject dimensions, evidence requirements, limitations, ownership,
//! invalidation, and claim ceilings.  It is additive beside the
//! `compiler_profile.v1` capability wire contract in `compiler_profile.rs`;
//! that module keeps its own meaning and is not reinterpreted here.
//!
//! Deliberately absent (issue non-goals): no checked row inventory, no manifest
//! or file syntax, no serde derives, no CLI, no receipt adaptation, no
//! candidate evaluation, no status output, and no GitHub/workflow/LSP-DTO/
//! provider types.  The successor initial-row inventory instantiates this
//! vocabulary; it must not invent a second one.
//!
//! Closure laws expressed and validated here:
//!
//! - every required applicable row is conjunctive (satisfaction is per-row;
//!   the model deliberately defines no aggregate roll-up number);
//! - conditional, optional, unsupported and not-applicable are closed typed
//!   states, never omitted rows;
//! - an import names an exact lower profile identity/version/digest and
//!   preserves every imported row and limitation verbatim
//!   (`CompilerProfileDefinition::verify_import_closure`);
//! - the thirteen proposition families, four proof classes, and five source
//!   tiers are independent closed dimensions that cannot cross-satisfy through
//!   constructors or validation;
//! - profile identity changes when any semantic row field changes, and row
//!   order or insertion order cannot change semantic identity
//!   (`CompilerProfileDefinition::semantic_fingerprint`).

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

// ---------------------------------------------------------------------------
// Identity types
// ---------------------------------------------------------------------------

/// Exact identity of a maintained compiler operating profile.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct CompilerProfileId(String);

impl CompilerProfileId {
    /// Construct a non-empty profile identity.
    pub fn new(value: &str) -> Result<Self> {
        non_empty("compiler profile id", value)?;
        Ok(Self(value.to_owned()))
    }

    /// Borrow the identity text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Exact version of a maintained compiler operating profile.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct CompilerProfileVersion(String);

impl CompilerProfileVersion {
    /// Construct a non-empty `v`-prefixed profile version.
    pub fn new(value: &str) -> Result<Self> {
        non_empty("compiler profile version", value)?;
        if !value.starts_with('v') {
            bail!("compiler profile version {value:?} must start with 'v'");
        }
        Ok(Self(value.to_owned()))
    }

    /// Borrow the version text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Deterministic semantic fingerprint of a profile definition.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct CompilerProfileDigest(String);

impl CompilerProfileDigest {
    /// Construct a digest from exactly 64 lowercase hex characters.
    pub fn from_hex(value: &str) -> Result<Self> {
        if value.len() != 64
            || !value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            bail!("compiler profile digest must be 64 lowercase hex characters, got {value:?}");
        }
        Ok(Self(value.to_owned()))
    }

    /// Borrow the digest text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Exact identity of one profile row.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct CompilerProfileRowId(String);

impl CompilerProfileRowId {
    /// Construct a non-empty row identity.
    pub fn new(value: &str) -> Result<Self> {
        non_empty("compiler profile row id", value)?;
        Ok(Self(value.to_owned()))
    }

    /// Borrow the row identity text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Exact subject named by a subject selector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubjectRef(String);

impl SubjectRef {
    /// Construct a non-empty exact subject reference.
    pub fn new(value: &str) -> Result<Self> {
        non_empty("subject reference", value)?;
        Ok(Self(value.to_owned()))
    }

    /// Borrow the subject text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Non-empty scope of required work.  Zero-work scope cannot be constructed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkScope(String);

impl WorkScope {
    /// Construct a non-empty work scope.
    pub fn new(value: &str) -> Result<Self> {
        non_empty("work scope", value)?;
        Ok(Self(value.to_owned()))
    }

    /// Borrow the work scope text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn non_empty(field: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        bail!("{field} must not be empty");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Closed independent dimensions
// ---------------------------------------------------------------------------

/// Independent proposition family of a row.  The thirteen families are
/// pairwise independent: evidence for one family can never stand in for
/// another.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ClaimFamily {
    /// Parser/compiler-internal fact production (parse, semantic, PIR facts).
    ParserInternal,
    /// Provider consumption of compiler facts.
    Provider,
    /// Edit authorization.
    Edit,
    /// Project/world model currentness.
    ProjectWorld,
    /// Execution behavior.
    Execution,
    /// Performance or resource results.
    Performance,
    /// Exact-process behavior.
    ExactProcess,
    /// Packaged artifact behavior.
    Packaged,
    /// Installed-host behavior.
    InstalledHost,
    /// Actual-client behavior.
    ActualClient,
    /// Test-reachability propositions.
    TestReachability,
    /// Legacy-exit propositions.
    LegacyExit,
    /// Public-claim propositions.
    PublicClaim,
}

impl ClaimFamily {
    /// Closed list of every proposition family.
    pub const ALL: [Self; 13] = [
        Self::ParserInternal,
        Self::Provider,
        Self::Edit,
        Self::ProjectWorld,
        Self::Execution,
        Self::Performance,
        Self::ExactProcess,
        Self::Packaged,
        Self::InstalledHost,
        Self::ActualClient,
        Self::TestReachability,
        Self::LegacyExit,
        Self::PublicClaim,
    ];

    /// Stable canonical tag.
    pub fn tag(self) -> &'static str {
        match self {
            Self::ParserInternal => "parser_internal",
            Self::Provider => "provider",
            Self::Edit => "edit",
            Self::ProjectWorld => "project_world",
            Self::Execution => "execution",
            Self::Performance => "performance",
            Self::ExactProcess => "exact_process",
            Self::Packaged => "packaged",
            Self::InstalledHost => "installed_host",
            Self::ActualClient => "actual_client",
            Self::TestReachability => "test_reachability",
            Self::LegacyExit => "legacy_exit",
            Self::PublicClaim => "public_claim",
        }
    }
}

/// Independent proof axis.  One axis can never satisfy another: curated
/// expectation is not a real-Perl oracle, oracle agreement is not an EIR
/// mechanism, and neither is evaluated work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProofClass {
    /// Curated expectation proof.
    CuratedExpectation,
    /// Real-Perl oracle proof.
    RealPerlOracle,
    /// EIR mechanism proof.
    EirMechanism,
    /// Evaluated-work proof.
    EvaluatedWork,
}

impl ProofClass {
    /// Closed list of every proof axis.
    pub const ALL: [Self; 4] =
        [Self::CuratedExpectation, Self::RealPerlOracle, Self::EirMechanism, Self::EvaluatedWork];

    /// Stable canonical tag.
    pub fn tag(self) -> &'static str {
        match self {
            Self::CuratedExpectation => "curated_expectation",
            Self::RealPerlOracle => "real_perl_oracle",
            Self::EirMechanism => "eir_mechanism",
            Self::EvaluatedWork => "evaluated_work",
        }
    }
}

/// Exact stage at which evidence is gathered.  The five stages are distinct
/// closed states and cannot collapse into one another.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SourceTier {
    /// In-source evidence.
    Source,
    /// Exact-process evidence.
    ExactProcess,
    /// Packaged-artifact evidence.
    Packaged,
    /// Installed-host evidence.
    InstalledHost,
    /// Actual-client evidence.
    ActualClient,
}

impl SourceTier {
    /// Closed list of every source tier.
    pub const ALL: [Self; 5] =
        [Self::Source, Self::ExactProcess, Self::Packaged, Self::InstalledHost, Self::ActualClient];

    /// Stable canonical tag.
    pub fn tag(self) -> &'static str {
        match self {
            Self::Source => "source",
            Self::ExactProcess => "exact_process",
            Self::Packaged => "packaged",
            Self::InstalledHost => "installed_host",
            Self::ActualClient => "actual_client",
        }
    }
}

/// Closed row disposition.  Conditional, optional, unsupported and
/// not-applicable rows are explicit typed states with named payloads; a row
/// can never drop out of a profile by omission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowDisposition {
    /// Required applicable row; all required applicable rows are conjunctive.
    Required,
    /// Required only while the named trigger holds.
    Conditional { trigger: String },
    /// Optional row.
    Optional,
    /// Explicitly unsupported row with a named reason.
    Unsupported { reason: String },
    /// Explicitly not-applicable row with a named justification.
    NotApplicable { justification: String },
}

impl RowDisposition {
    /// Closed list size for exhaustiveness checks.
    pub const VARIANT_COUNT: usize = 5;

    /// Construct a conditional disposition with a non-empty trigger.
    pub fn conditional(trigger: &str) -> Result<Self> {
        non_empty("conditional disposition trigger", trigger)?;
        Ok(Self::Conditional { trigger: trigger.to_owned() })
    }

    /// Construct an unsupported disposition with a non-empty reason.
    pub fn unsupported(reason: &str) -> Result<Self> {
        non_empty("unsupported disposition reason", reason)?;
        Ok(Self::Unsupported { reason: reason.to_owned() })
    }

    /// Construct a not-applicable disposition with a non-empty justification.
    pub fn not_applicable(justification: &str) -> Result<Self> {
        non_empty("not-applicable disposition justification", justification)?;
        Ok(Self::NotApplicable { justification: justification.to_owned() })
    }

    /// True only for required applicable rows.
    pub fn is_required(&self) -> bool {
        matches!(self, Self::Required)
    }

    fn validate(&self) -> Result<()> {
        match self {
            Self::Required | Self::Optional => Ok(()),
            Self::Conditional { trigger } => non_empty("conditional disposition trigger", trigger),
            Self::Unsupported { reason } => non_empty("unsupported disposition reason", reason),
            Self::NotApplicable { justification } => {
                non_empty("not-applicable disposition justification", justification)
            }
        }
    }

    fn write_canonical(&self, out: &mut String) {
        match self {
            Self::Required => out.push_str("required"),
            Self::Conditional { trigger } => {
                let _ = write!(out, "conditional({trigger:?})");
            }
            Self::Optional => out.push_str("optional"),
            Self::Unsupported { reason } => {
                let _ = write!(out, "unsupported({reason:?})");
            }
            Self::NotApplicable { justification } => {
                let _ = write!(out, "not_applicable({justification:?})");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Row component types
// ---------------------------------------------------------------------------

/// Exact subject dimension of a row.  Local-lexical, project/world,
/// cross-file external behavior, bounded execution, packaged, installed-host,
/// and actual-client subjects are distinct typed selectors, not string
/// conventions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubjectSelector {
    /// A local-lexical subject.
    LocalLexical(SubjectRef),
    /// A static project/world subject.
    StaticProject(SubjectRef),
    /// A cross-file external behavior subject.
    CrossFileExternal(SubjectRef),
    /// A bounded-execution subject.
    BoundedExecution(SubjectRef),
    /// A packaged-artifact subject.
    PackagedArtifact(SubjectRef),
    /// An installed-host-environment subject.
    InstalledHostEnvironment(SubjectRef),
    /// An actual-client-surface subject.
    ActualClientSurface(SubjectRef),
}

impl SubjectSelector {
    fn write_canonical(&self, out: &mut String) {
        let (tag, subject) = match self {
            Self::LocalLexical(subject) => ("local_lexical", subject),
            Self::StaticProject(subject) => ("static_project", subject),
            Self::CrossFileExternal(subject) => ("cross_file_external", subject),
            Self::BoundedExecution(subject) => ("bounded_execution", subject),
            Self::PackagedArtifact(subject) => ("packaged_artifact", subject),
            Self::InstalledHostEnvironment(subject) => ("installed_host_environment", subject),
            Self::ActualClientSurface(subject) => ("actual_client_surface", subject),
        };
        let _ = write!(out, "{tag}({:?})", subject.as_str());
    }
}

/// Evidence a row requires: one exact proposition family, one exact source
/// tier, and a conjunctive set of independent proof axes.  Several axes may be
/// required; no axis satisfies another.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceRequirement {
    /// Independent proposition family.
    pub family: ClaimFamily,
    /// Exact evidence stage.
    pub source_tier: SourceTier,
    /// Conjunctive, non-empty set of independent proof axes.
    pub proof_axes: BTreeSet<ProofClass>,
}

impl EvidenceRequirement {
    /// Construct an evidence requirement with at least one proof axis.
    pub fn new(
        family: ClaimFamily,
        source_tier: SourceTier,
        proof_axes: BTreeSet<ProofClass>,
    ) -> Result<Self> {
        if proof_axes.is_empty() {
            bail!("evidence requirement must name at least one proof axis");
        }
        Ok(Self { family, source_tier, proof_axes })
    }

    fn validate(&self) -> Result<()> {
        if self.proof_axes.is_empty() {
            bail!("evidence requirement must name at least one proof axis");
        }
        Ok(())
    }

    fn write_canonical(&self, out: &mut String) {
        let _ = write!(out, "family={} tier={} axes=[", self.family.tag(), self.source_tier.tag());
        for axis in &self.proof_axes {
            let _ = write!(out, "{},", axis.tag());
        }
        out.push(']');
    }
}

/// Currentness rule for a row's evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurrentnessRule {
    /// Evidence is current while the named source is unchanged.
    SourceLocked,
    /// Evidence is current while the project/world model is unchanged.
    ProjectWorldCurrent,
    /// Evidence is current within the named execution bound.
    ExecutionBounded,
    /// Evidence is an observation of the current host or client.
    HostObserved,
}

impl CurrentnessRule {
    fn tag(self) -> &'static str {
        match self {
            Self::SourceLocked => "source_locked",
            Self::ProjectWorldCurrent => "project_world_current",
            Self::ExecutionBounded => "execution_bounded",
            Self::HostObserved => "host_observed",
        }
    }
}

/// Coverage rule for a row's evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoverageRule {
    /// The row covers its whole subject.
    Exhaustive,
    /// The row covers its subject inside a named boundary.
    Bounded { boundary: String },
    /// The row is explicitly partial; the remainder is named, not omitted.
    ExplicitlyPartial { remainder: String },
}

impl CoverageRule {
    fn validate(&self) -> Result<()> {
        match self {
            Self::Exhaustive => Ok(()),
            Self::Bounded { boundary } => non_empty("coverage boundary", boundary),
            Self::ExplicitlyPartial { remainder } => non_empty("coverage remainder", remainder),
        }
    }

    fn write_canonical(&self, out: &mut String) {
        match self {
            Self::Exhaustive => out.push_str("exhaustive"),
            Self::Bounded { boundary } => {
                let _ = write!(out, "bounded({boundary:?})");
            }
            Self::ExplicitlyPartial { remainder } => {
                let _ = write!(out, "explicitly_partial({remainder:?})");
            }
        }
    }
}

/// Currentness and coverage rule for a row's evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletenessRequirement {
    /// Currentness rule.
    pub currentness: CurrentnessRule,
    /// Coverage rule.
    pub coverage: CoverageRule,
}

impl CompletenessRequirement {
    fn validate(&self) -> Result<()> {
        self.coverage.validate()
    }

    fn write_canonical(&self, out: &mut String) {
        let _ = write!(out, "currentness={} coverage=", self.currentness.tag());
        self.coverage.write_canonical(out);
    }
}

/// Work a row requires.  Correctness-only, production work, oracle/cold work,
/// and performance/resource results are distinct typed states: oracle or cold
/// work can never be typed as production work, and production work always
/// names a non-zero scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkRequirement {
    /// Correctness-only row; no work result is claimed.
    Correctness,
    /// Production work with a non-zero named scope.
    Production(WorkScope),
    /// Oracle or cold-path work with a named scope; never production work.
    OracleOrCold(WorkScope),
    /// Performance or resource result with a named scope.
    PerformanceResource(WorkScope),
}

impl WorkRequirement {
    fn write_canonical(&self, out: &mut String) {
        match self {
            Self::Correctness => out.push_str("correctness"),
            Self::Production(scope) => {
                let _ = write!(out, "production({:?})", scope.as_str());
            }
            Self::OracleOrCold(scope) => {
                let _ = write!(out, "oracle_or_cold({:?})", scope.as_str());
            }
            Self::PerformanceResource(scope) => {
                let _ = write!(out, "performance_resource({:?})", scope.as_str());
            }
        }
    }
}

/// One allowed limitation, owned and bounded.  Limitations survive import
/// verbatim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AllowedLimitation {
    /// Limitation identity, unique within its collection.
    pub id: String,
    /// Construct the limitation applies to.
    pub construct: String,
    /// Why the limitation exists.
    pub reason: String,
    /// Owning reference for the limitation.
    pub owner: String,
}

impl AllowedLimitation {
    /// Construct a fully named limitation.
    pub fn new(id: &str, construct: &str, reason: &str, owner: &str) -> Result<Self> {
        let limitation = Self {
            id: id.to_owned(),
            construct: construct.to_owned(),
            reason: reason.to_owned(),
            owner: owner.to_owned(),
        };
        limitation.validate()?;
        Ok(limitation)
    }

    fn validate(&self) -> Result<()> {
        non_empty("limitation id", &self.id)?;
        non_empty("limitation construct", &self.construct)?;
        non_empty("limitation reason", &self.reason)?;
        non_empty("limitation owner", &self.owner)?;
        Ok(())
    }

    fn write_canonical(&self, out: &mut String) {
        let _ = writeln!(
            out,
            "limitation {:?} construct={:?} reason={:?} owner={:?}",
            self.id, self.construct, self.reason, self.owner
        );
    }
}

/// One independent legacy-exit obligation axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Obligation {
    /// The obligation must be proven.
    Required,
    /// The obligation does not apply to this row, stated explicitly.
    NotApplicable,
}

impl Obligation {
    fn tag(self) -> &'static str {
        match self {
            Self::Required => "required",
            Self::NotApplicable => "not_applicable",
        }
    }
}

/// Legacy-exit requirement with three independent axes.  Replacement
/// currentness, old-path absence, and recurrence proof are separate fields:
/// one axis can never satisfy another.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyExitRequirement {
    /// Replacement-currentness obligation.
    pub replacement_currentness: Obligation,
    /// Old-path-absence obligation.
    pub old_path_absence: Obligation,
    /// Recurrence-proof obligation.
    pub recurrence_proof: Obligation,
}

impl LegacyExitRequirement {
    /// A row with no legacy exit at all, stated explicitly.
    pub const NONE: Self = Self {
        replacement_currentness: Obligation::NotApplicable,
        old_path_absence: Obligation::NotApplicable,
        recurrence_proof: Obligation::NotApplicable,
    };

    fn write_canonical(&self, out: &mut String) {
        let _ = write!(
            out,
            "replacement_currentness={} old_path_absence={} recurrence_proof={}",
            self.replacement_currentness.tag(),
            self.old_path_absence.tag(),
            self.recurrence_proof.tag()
        );
    }
}

/// Maximum claim a row can support.  Ceilings are closed typed states:
/// observed evidence, accepted compatibility state, and bounded public claims
/// are distinct, and none of them is support, release, or publication
/// authorization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaimCeiling {
    /// The row is internal observed evidence only.
    ObservedEvidence,
    /// The row supports an accepted compatibility state for its subject.
    AcceptedCompatibility,
    /// The row supports a bounded public claim; still not support, release,
    /// or publication authorization.
    BoundedPublicClaim,
}

impl ClaimCeiling {
    /// Closed list of every claim ceiling.
    pub const ALL: [Self; 3] =
        [Self::ObservedEvidence, Self::AcceptedCompatibility, Self::BoundedPublicClaim];

    /// Strongest claim this ceiling can support, as inspectable data.  No
    /// variant maps to support, release, or publication authority; that
    /// authorization lives outside the profile model (#12186).
    pub fn strongest_claim(self) -> &'static str {
        match self {
            Self::ObservedEvidence => "internal observed evidence",
            Self::AcceptedCompatibility => "accepted compatibility state",
            Self::BoundedPublicClaim => "bounded public claim",
        }
    }

    /// Stable canonical tag, shared with the observation contract (#12188).
    pub fn tag(self) -> &'static str {
        match self {
            Self::ObservedEvidence => "observed_evidence",
            Self::AcceptedCompatibility => "accepted_compatibility",
            Self::BoundedPublicClaim => "bounded_public_claim",
        }
    }
}

/// Kind of input that invalidates a row's evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum InvalidationKind {
    /// The underlying source changed.
    Source,
    /// A dependency of the subject changed.
    Dependency,
    /// The project/world model changed.
    WorldModel,
    /// The host or client environment changed.
    HostEnvironment,
    /// The oracle or expectation basis changed.
    Oracle,
}

impl InvalidationKind {
    /// Stable canonical tag, shared with the observation contract (#12188).
    pub fn tag(self) -> &'static str {
        match self {
            Self::Source => "source_change",
            Self::Dependency => "dependency_change",
            Self::WorldModel => "world_model_change",
            Self::HostEnvironment => "host_environment_change",
            Self::Oracle => "oracle_change",
        }
    }
}

/// One invalidation input: what change re-opens the row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidationInput {
    /// Kind of invalidating change.
    pub kind: InvalidationKind,
    /// Exact detail of the invalidating input.
    pub detail: String,
}

impl InvalidationInput {
    /// Construct an invalidation input with non-empty detail.
    pub fn new(kind: InvalidationKind, detail: &str) -> Result<Self> {
        non_empty("invalidation detail", detail)?;
        Ok(Self { kind, detail: detail.to_owned() })
    }

    fn validate(&self) -> Result<()> {
        non_empty("invalidation detail", &self.detail)
    }

    fn write_canonical(&self, out: &mut String) {
        let _ = writeln!(out, "invalidation {} {:?}", self.kind.tag(), self.detail);
    }
}

/// Owning reference and the wake event that re-opens the row or profile.
/// Ownership here is an identifier only; workflow state of any external
/// tracker is never evidence in this model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerAndWakeEvent {
    /// Owning reference.
    pub owner: String,
    /// Event that wakes re-validation.
    pub wake_event: String,
}

impl OwnerAndWakeEvent {
    /// Construct an owner/wake pair with both fields named.
    pub fn new(owner: &str, wake_event: &str) -> Result<Self> {
        non_empty("owner", owner)?;
        non_empty("wake event", wake_event)?;
        Ok(Self { owner: owner.to_owned(), wake_event: wake_event.to_owned() })
    }

    fn validate(&self) -> Result<()> {
        non_empty("owner", &self.owner)?;
        non_empty("wake event", &self.wake_event)?;
        Ok(())
    }

    fn write_canonical(&self, out: &mut String) {
        let _ = write!(out, "owner={:?} wake_event={:?}", self.owner, self.wake_event);
    }
}

// ---------------------------------------------------------------------------
// Rows
// ---------------------------------------------------------------------------

/// One profile row.  Every row names an exact subject, evidence requirement,
/// completeness rule, work requirement, limitation policy, legacy-exit
/// requirement, claim ceiling, invalidation inputs, and owner/wake event;
/// none of these fields can be absent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompilerProfileRow {
    /// Exact row identity.
    pub id: CompilerProfileRowId,
    /// Closed typed disposition.
    pub disposition: RowDisposition,
    /// Exact subject selector.
    pub subject: SubjectSelector,
    /// Evidence requirement (family, tier, conjunctive proof axes).
    pub evidence: EvidenceRequirement,
    /// Currentness and coverage rule.
    pub completeness: CompletenessRequirement,
    /// Work requirement.
    pub work: WorkRequirement,
    /// Allowed limitations (sorted by identity in canonical form).
    pub limitations: Vec<AllowedLimitation>,
    /// Legacy-exit requirement; use [`LegacyExitRequirement::NONE`] when the
    /// row has no legacy exit.
    pub legacy_exit: LegacyExitRequirement,
    /// Claim ceiling.
    pub ceiling: ClaimCeiling,
    /// Invalidation inputs; at least one is required.
    pub invalidation: Vec<InvalidationInput>,
    /// Owner and wake event.
    pub owner: OwnerAndWakeEvent,
}

impl CompilerProfileRow {
    /// Validate the row's internal closure rules.
    pub fn validate(&self) -> Result<()> {
        self.disposition.validate().with_context(|| format!("row {:?}", self.id.as_str()))?;
        self.evidence.validate().with_context(|| format!("row {:?}", self.id.as_str()))?;
        self.completeness.validate().with_context(|| format!("row {:?}", self.id.as_str()))?;
        for limitation in &self.limitations {
            limitation.validate().with_context(|| format!("row {:?}", self.id.as_str()))?;
        }
        ensure_distinct_ids(
            self.limitations.iter().map(|limitation| limitation.id.as_str()),
            "row limitations",
        )?;
        if self.invalidation.is_empty() {
            bail!("row {:?} must name at least one invalidation input", self.id.as_str());
        }
        for input in &self.invalidation {
            input.validate().with_context(|| format!("row {:?}", self.id.as_str()))?;
        }
        self.owner.validate().with_context(|| format!("row {:?}", self.id.as_str()))?;
        if matches!(
            self.disposition,
            RowDisposition::Unsupported { .. } | RowDisposition::NotApplicable { .. }
        ) && self.ceiling != ClaimCeiling::ObservedEvidence
        {
            bail!(
                "row {:?} is unsupported or not applicable and cannot claim more than observed evidence",
                self.id.as_str()
            );
        }
        Ok(())
    }

    fn write_canonical(&self, out: &mut String) {
        let _ = writeln!(out, "row {:?}", self.id.as_str());
        out.push_str("  disposition=");
        self.disposition.write_canonical(out);
        out.push('\n');
        out.push_str("  subject=");
        self.subject.write_canonical(out);
        out.push('\n');
        out.push_str("  evidence ");
        self.evidence.write_canonical(out);
        out.push('\n');
        out.push_str("  completeness ");
        self.completeness.write_canonical(out);
        out.push('\n');
        out.push_str("  work=");
        self.work.write_canonical(out);
        out.push('\n');
        let mut limitations = self.limitations.clone();
        limitations.sort_by(|a, b| a.id.cmp(&b.id));
        for limitation in &limitations {
            out.push_str("  ");
            limitation.write_canonical(out);
        }
        out.push_str("  legacy_exit ");
        self.legacy_exit.write_canonical(out);
        out.push('\n');
        let _ = writeln!(out, "  ceiling={}", self.ceiling.tag());
        let mut invalidation = self.invalidation.clone();
        invalidation
            .sort_by(|a, b| a.kind.tag().cmp(b.kind.tag()).then_with(|| a.detail.cmp(&b.detail)));
        for input in &invalidation {
            out.push_str("  ");
            input.write_canonical(out);
        }
        out.push_str("  ");
        self.owner.write_canonical(out);
        out.push('\n');
    }
}

// ---------------------------------------------------------------------------
// Imports and profile definition
// ---------------------------------------------------------------------------

/// Exact import of a lower profile: identity, version, and semantic digest.
/// The digest pins the lower profile's full semantic content, so an import
/// can never drift into a weaker or stronger lower profile silently.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompilerProfileImport {
    /// Exact lower profile identity.
    pub profile_id: CompilerProfileId,
    /// Exact lower profile version.
    pub version: CompilerProfileVersion,
    /// Exact lower profile semantic digest.
    pub digest: CompilerProfileDigest,
}

impl CompilerProfileImport {
    /// Construct the exact import entry for a lower profile.
    pub fn for_profile(lower: &CompilerProfileDefinition) -> Result<Self> {
        Ok(Self {
            profile_id: lower.id.clone(),
            version: lower.version.clone(),
            digest: lower.semantic_fingerprint()?,
        })
    }

    fn write_canonical(&self, out: &mut String) {
        let _ = writeln!(
            out,
            "import {:?} version={:?} digest={:?}",
            self.profile_id.as_str(),
            self.version.as_str(),
            self.digest.as_str()
        );
    }
}

/// In-memory definition of one maintained compiler operating profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompilerProfileDefinition {
    /// Exact profile identity.
    pub id: CompilerProfileId,
    /// Exact profile version.
    pub version: CompilerProfileVersion,
    /// Purpose of the profile.
    pub purpose: String,
    /// Reason for this revision of the definition.
    pub change_reason: String,
    /// Profile-level owner and wake event.
    pub owner: OwnerAndWakeEvent,
    /// Exact lower-profile imports.
    pub imports: Vec<CompilerProfileImport>,
    /// All rows, including conditional, optional, unsupported, and
    /// not-applicable typed states.  Order carries no semantics.
    pub rows: Vec<CompilerProfileRow>,
    /// Profile-level allowed limitations.
    pub limitations: Vec<AllowedLimitation>,
}

impl CompilerProfileDefinition {
    /// Validate the definition's internal closure rules.
    pub fn validate(&self) -> Result<()> {
        non_empty("profile purpose", &self.purpose)?;
        non_empty("profile change reason", &self.change_reason)?;
        self.owner.validate()?;
        ensure_distinct_ids(self.rows.iter().map(|row| row.id.as_str()), "profile rows")?;
        ensure_distinct_ids(
            self.imports.iter().map(|import| import.profile_id.as_str()),
            "profile imports",
        )?;
        if self.imports.iter().any(|import| import.profile_id == self.id) {
            bail!("profile {:?} must not import itself", self.id.as_str());
        }
        ensure_distinct_ids(
            self.limitations.iter().map(|limitation| limitation.id.as_str()),
            "profile limitations",
        )?;
        for limitation in &self.limitations {
            limitation.validate()?;
        }
        for row in &self.rows {
            row.validate()?;
        }
        Ok(())
    }

    /// Identities of the unconditionally required rows.  Satisfaction of a
    /// profile is the conjunction of these rows plus every conditional row
    /// whose trigger currently holds; trigger state is runtime data this
    /// dependency-neutral model deliberately does not carry, so conditional
    /// applicability is resolved by the downstream evaluator (a separate
    /// claim), never assumed here.  The model deliberately defines no
    /// aggregate roll-up figure, so there is nothing to average.
    pub fn required_unconditional_row_ids(&self) -> BTreeSet<&str> {
        self.rows
            .iter()
            .filter(|row| row.disposition.is_required())
            .map(|row| row.id.as_str())
            .collect()
    }

    /// Identities of the conditional rows together with their triggers.  Each
    /// listed row becomes conjunctive while its named trigger holds; the
    /// downstream evaluator owns trigger evaluation.
    pub fn conditional_row_triggers(&self) -> BTreeMap<&str, &str> {
        self.rows
            .iter()
            .filter_map(|row| match &row.disposition {
                RowDisposition::Conditional { trigger } => {
                    Some((row.id.as_str(), trigger.as_str()))
                }
                _ => None,
            })
            .collect()
    }

    /// Deterministic canonical semantic text: every semantic field of every
    /// row, with all order-insensitive collections sorted.  Row order and
    /// insertion order cannot change this text.
    pub fn canonical_semantic_text(&self) -> Result<String> {
        self.validate()?;
        let mut out = String::new();
        let _ = writeln!(out, "profile {:?}", self.id.as_str());
        let _ = writeln!(out, "version {:?}", self.version.as_str());
        let _ = writeln!(out, "purpose {:?}", self.purpose);
        let _ = writeln!(out, "change_reason {:?}", self.change_reason);
        self.owner.write_canonical(&mut out);
        out.push('\n');
        let mut imports = self.imports.clone();
        imports.sort_by(|a, b| a.profile_id.cmp(&b.profile_id));
        for import in &imports {
            import.write_canonical(&mut out);
        }
        let mut limitations = self.limitations.clone();
        limitations.sort_by(|a, b| a.id.cmp(&b.id));
        for limitation in &limitations {
            limitation.write_canonical(&mut out);
        }
        let mut rows = self.rows.clone();
        rows.sort_by(|a, b| a.id.cmp(&b.id));
        for row in &rows {
            row.write_canonical(&mut out);
        }
        Ok(out)
    }

    /// Deterministic semantic fingerprint over [`Self::canonical_semantic_text`].
    pub fn semantic_fingerprint(&self) -> Result<CompilerProfileDigest> {
        let canonical = self.canonical_semantic_text()?;
        let digest = Sha256::digest(canonical.as_bytes());
        let mut hex = String::with_capacity(64);
        for byte in digest {
            let _ = write!(hex, "{byte:02x}");
        }
        CompilerProfileDigest::from_hex(&hex)
            .context("sha256 hex output must satisfy the digest invariant")
    }

    /// Verify the import closure law against one lower profile: the lower
    /// profile must be declared as an import with its exact identity, version,
    /// and current semantic digest, and every imported row and profile-level
    /// limitation must be preserved verbatim in this profile.
    pub fn verify_import_closure(&self, lower: &CompilerProfileDefinition) -> Result<()> {
        self.validate()?;
        lower.validate()?;
        let import =
            self.imports.iter().find(|import| import.profile_id == lower.id).ok_or_else(|| {
                anyhow::anyhow!(
                    "profile {:?} does not declare an import of {:?}",
                    self.id.as_str(),
                    lower.id.as_str()
                )
            })?;
        if import.version != lower.version {
            bail!(
                "import of {:?} declares version {:?} but the lower profile is {:?}",
                lower.id.as_str(),
                import.version.as_str(),
                lower.version.as_str()
            );
        }
        let lower_digest = lower.semantic_fingerprint()?;
        if import.digest != lower_digest {
            bail!(
                "import of {:?} declares digest {:?} but the lower profile fingerprint is {:?}",
                lower.id.as_str(),
                import.digest.as_str(),
                lower_digest.as_str()
            );
        }
        let own_rows: BTreeMap<&str, &CompilerProfileRow> =
            self.rows.iter().map(|row| (row.id.as_str(), row)).collect();
        for imported_row in &lower.rows {
            match own_rows.get(imported_row.id.as_str()) {
                None => bail!(
                    "imported row {:?} from {:?} is missing in {:?}",
                    imported_row.id.as_str(),
                    lower.id.as_str(),
                    self.id.as_str()
                ),
                Some(own_row) if *own_row != imported_row => bail!(
                    "imported row {:?} from {:?} is not preserved verbatim in {:?}",
                    imported_row.id.as_str(),
                    lower.id.as_str(),
                    self.id.as_str()
                ),
                Some(_) => {}
            }
        }
        let own_limitations: BTreeMap<&str, &AllowedLimitation> = self
            .limitations
            .iter()
            .map(|limitation| (limitation.id.as_str(), limitation))
            .collect();
        for imported_limitation in &lower.limitations {
            match own_limitations.get(imported_limitation.id.as_str()) {
                None => bail!(
                    "imported limitation {:?} from {:?} is missing in {:?}",
                    imported_limitation.id,
                    lower.id.as_str(),
                    self.id.as_str()
                ),
                Some(own_limitation) if *own_limitation != imported_limitation => bail!(
                    "imported limitation {:?} from {:?} is not preserved verbatim in {:?}",
                    imported_limitation.id,
                    lower.id.as_str(),
                    self.id.as_str()
                ),
                Some(_) => {}
            }
        }
        Ok(())
    }
}

fn ensure_distinct_ids<'a>(ids: impl Iterator<Item = &'a str>, name: &str) -> Result<()> {
    let ids = ids.collect::<Vec<_>>();
    if ids.iter().any(|id| id.trim().is_empty()) {
        bail!("{name} must not contain an empty id");
    }
    let unique = ids.iter().collect::<BTreeSet<_>>();
    if unique.len() != ids.len() {
        bail!("{name} must not contain duplicate ids");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Shape fixtures
// ---------------------------------------------------------------------------

/// Minimal in-memory shape fixtures for the four #12176 profile classes.
///
/// These fixtures prove representability and closure only.  They are not the
/// checked repository row inventory, not a manifest, and do not establish
/// current product state; the successor inventory owns the full exact row set
/// and canonical owner/evidence mapping.
pub mod shape_fixtures {
    use super::{
        AllowedLimitation, ClaimCeiling, ClaimFamily, CompilerProfileDefinition, CompilerProfileId,
        CompilerProfileImport, CompilerProfileRow, CompilerProfileRowId, CompilerProfileVersion,
        CompletenessRequirement, CoverageRule, CurrentnessRule, EvidenceRequirement,
        InvalidationInput, InvalidationKind, LegacyExitRequirement, Obligation, OwnerAndWakeEvent,
        ProofClass, Result, RowDisposition, SourceTier, SubjectRef, SubjectSelector,
        WorkRequirement, WorkScope,
    };

    const OWNER: &str = "#12176 compiler-profile train";
    const WAKE: &str = "row-inventory successor issue re-opened";
    const PARTIAL: &str =
        "shape fixture proves representability only; the successor inventory owns the full row set";

    fn owner() -> Result<OwnerAndWakeEvent> {
        OwnerAndWakeEvent::new(OWNER, WAKE)
    }

    fn base_row(
        id: &str,
        family: ClaimFamily,
        subject: SubjectSelector,
        tier: SourceTier,
        axes: &[ProofClass],
        work: WorkRequirement,
    ) -> Result<CompilerProfileRow> {
        Ok(CompilerProfileRow {
            id: CompilerProfileRowId::new(id)?,
            disposition: RowDisposition::Required,
            subject,
            evidence: EvidenceRequirement::new(family, tier, axes.iter().copied().collect())?,
            completeness: CompletenessRequirement {
                currentness: CurrentnessRule::SourceLocked,
                coverage: CoverageRule::ExplicitlyPartial { remainder: PARTIAL.to_owned() },
            },
            work,
            limitations: Vec::new(),
            legacy_exit: LegacyExitRequirement::NONE,
            ceiling: ClaimCeiling::ObservedEvidence,
            invalidation: vec![InvalidationInput::new(
                InvalidationKind::Source,
                "the named subject's source basis changed",
            )?],
            owner: owner()?,
        })
    }

    /// Minimal shape of `compiler_local_lexical.v1`: bounded local-lexical
    /// profile with no imports and no long-horizon prerequisites.
    pub fn compiler_local_lexical_v1() -> Result<CompilerProfileDefinition> {
        let parse_facts = base_row(
            "lexical.parse-fact-production",
            ClaimFamily::ParserInternal,
            SubjectSelector::LocalLexical(SubjectRef::new(
                "parser/semantic/PIR fact production for one buffer",
            )?),
            SourceTier::Source,
            &[ProofClass::CuratedExpectation],
            WorkRequirement::Correctness,
        )?;
        let reachability = base_row(
            "lexical.test-reachability",
            ClaimFamily::TestReachability,
            SubjectSelector::LocalLexical(SubjectRef::new(
                "local-lexical propositions reachable from focused tests",
            )?),
            SourceTier::Source,
            &[ProofClass::CuratedExpectation],
            WorkRequirement::Correctness,
        )?;
        Ok(CompilerProfileDefinition {
            id: CompilerProfileId::new("compiler_local_lexical")?,
            version: CompilerProfileVersion::new("v1")?,
            purpose: "bounded local-lexical compiler operating profile (shape fixture)".to_owned(),
            change_reason: "initial shape fixture for #12186".to_owned(),
            owner: owner()?,
            imports: Vec::new(),
            rows: vec![parse_facts, reachability],
            limitations: vec![AllowedLimitation::new(
                "lim.local-lexical-scope",
                "local lexical analysis",
                "shape fixture is bounded to one buffer; wider claims belong to higher profiles",
                OWNER,
            )?],
        })
    }

    /// Minimal shape of `compiler_static_project.v1`: imports
    /// `compiler_local_lexical.v1` and adds project/world and provider rows.
    pub fn compiler_static_project_v1() -> Result<CompilerProfileDefinition> {
        let lower = compiler_local_lexical_v1()?;
        let mut world = base_row(
            "project.world-currentness",
            ClaimFamily::ProjectWorld,
            SubjectSelector::StaticProject(SubjectRef::new(
                "project/world model currentness for the open workspace",
            )?),
            SourceTier::Source,
            &[ProofClass::CuratedExpectation, ProofClass::RealPerlOracle],
            WorkRequirement::Correctness,
        )?;
        world.completeness.currentness = CurrentnessRule::ProjectWorldCurrent;
        world.invalidation.push(InvalidationInput::new(
            InvalidationKind::WorldModel,
            "the project/world model changed under the evidence",
        )?);
        let provider = base_row(
            "project.provider-consumption",
            ClaimFamily::Provider,
            SubjectSelector::StaticProject(SubjectRef::new(
                "provider consumption of project-level compiler facts",
            )?),
            SourceTier::Source,
            &[ProofClass::CuratedExpectation],
            WorkRequirement::Correctness,
        )?;
        let mut rows = lower.rows.clone();
        rows.extend([world, provider]);
        let mut limitations = lower.limitations.clone();
        limitations.push(AllowedLimitation::new(
            "lim.static-project-bound",
            "static project analysis",
            "shape fixture covers static project facts only; execution claims belong higher",
            OWNER,
        )?);
        Ok(CompilerProfileDefinition {
            id: CompilerProfileId::new("compiler_static_project")?,
            version: CompilerProfileVersion::new("v1")?,
            purpose: "static project compiler operating profile (shape fixture)".to_owned(),
            change_reason: "initial shape fixture for #12186".to_owned(),
            owner: owner()?,
            imports: vec![CompilerProfileImport::for_profile(&lower)?],
            rows,
            limitations,
        })
    }

    /// Minimal shape of `compiler_bounded_execution.v1`: imports
    /// `compiler_static_project.v1` and adds bounded execution and
    /// performance rows proven by evaluated work.
    pub fn compiler_bounded_execution_v1() -> Result<CompilerProfileDefinition> {
        let lower = compiler_static_project_v1()?;
        let mut execution = base_row(
            "execution.bounded-work",
            ClaimFamily::Execution,
            SubjectSelector::BoundedExecution(SubjectRef::new(
                "bounded execution of evaluated compiler work",
            )?),
            SourceTier::ExactProcess,
            &[ProofClass::EvaluatedWork],
            WorkRequirement::Production(WorkScope::new(
                "non-zero bounded execution work inside the named bound",
            )?),
        )?;
        execution.completeness.currentness = CurrentnessRule::ExecutionBounded;
        let mut performance = base_row(
            "execution.performance-resource",
            ClaimFamily::Performance,
            SubjectSelector::BoundedExecution(SubjectRef::new(
                "performance/resource result inside the named bound",
            )?),
            SourceTier::ExactProcess,
            &[ProofClass::EvaluatedWork],
            WorkRequirement::PerformanceResource(WorkScope::new(
                "measured resource result inside the named bound",
            )?),
        )?;
        performance.completeness.currentness = CurrentnessRule::ExecutionBounded;
        let mut rows = lower.rows.clone();
        rows.extend([execution, performance]);
        Ok(CompilerProfileDefinition {
            id: CompilerProfileId::new("compiler_bounded_execution")?,
            version: CompilerProfileVersion::new("v1")?,
            purpose: "bounded execution compiler operating profile (shape fixture)".to_owned(),
            change_reason: "initial shape fixture for #12186".to_owned(),
            owner: owner()?,
            imports: vec![CompilerProfileImport::for_profile(&lower)?],
            rows,
            limitations: lower.limitations.clone(),
        })
    }

    /// Minimal shape of `compiler_maintained_code_intelligence.v1`: imports
    /// `compiler_bounded_execution.v1` and adds edit-authorization and
    /// bounded public-claim rows, plus an explicitly unsupported actual-client
    /// row proving that unsupported is a closed typed state, not an omission.
    pub fn compiler_maintained_code_intelligence_v1() -> Result<CompilerProfileDefinition> {
        let lower = compiler_bounded_execution_v1()?;
        let mut edit = base_row(
            "intelligence.edit-authorization",
            ClaimFamily::Edit,
            SubjectSelector::CrossFileExternal(SubjectRef::new(
                "edit authorization behavior across files",
            )?),
            SourceTier::Source,
            &[ProofClass::EirMechanism, ProofClass::EvaluatedWork],
            WorkRequirement::Production(WorkScope::new(
                "non-zero maintained edit authorization work",
            )?),
        )?;
        edit.ceiling = ClaimCeiling::AcceptedCompatibility;
        edit.legacy_exit = LegacyExitRequirement {
            replacement_currentness: Obligation::Required,
            old_path_absence: Obligation::Required,
            recurrence_proof: Obligation::Required,
        };
        let public_claim = base_row(
            "intelligence.public-claim",
            ClaimFamily::PublicClaim,
            SubjectSelector::CrossFileExternal(SubjectRef::new(
                "bounded public maintained-code-intelligence claim boundary",
            )?),
            SourceTier::Source,
            &[ProofClass::EirMechanism, ProofClass::EvaluatedWork],
            WorkRequirement::Production(WorkScope::new(
                "non-zero maintained code intelligence work",
            )?),
        );
        let mut public_claim = public_claim?;
        public_claim.ceiling = ClaimCeiling::BoundedPublicClaim;
        let mut actual_client = base_row(
            "intelligence.actual-client-surface",
            ClaimFamily::ActualClient,
            SubjectSelector::ActualClientSurface(SubjectRef::new(
                "actual client surface behavior",
            )?),
            SourceTier::ActualClient,
            &[ProofClass::EvaluatedWork],
            WorkRequirement::Correctness,
        )?;
        actual_client.disposition = RowDisposition::unsupported(
            "shape fixture proves the closed unsupported state; no client row is claimed here",
        )?;
        let mut rows = lower.rows.clone();
        rows.extend([edit, public_claim, actual_client]);
        Ok(CompilerProfileDefinition {
            id: CompilerProfileId::new("compiler_maintained_code_intelligence")?,
            version: CompilerProfileVersion::new("v1")?,
            purpose: "maintained code intelligence compiler operating profile (shape fixture)"
                .to_owned(),
            change_reason: "initial shape fixture for #12186".to_owned(),
            owner: owner()?,
            imports: vec![CompilerProfileImport::for_profile(&lower)?],
            rows,
            limitations: lower.limitations.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AllowedLimitation, ClaimCeiling, ClaimFamily, CompilerProfileDefinition, CompilerProfileId,
        CompilerProfileImport, CompilerProfileRow, CompilerProfileRowId, CompilerProfileVersion,
        CompletenessRequirement, CoverageRule, CurrentnessRule, EvidenceRequirement,
        InvalidationInput, InvalidationKind, LegacyExitRequirement, Obligation, OwnerAndWakeEvent,
        ProofClass, Result, RowDisposition, SourceTier, SubjectRef, SubjectSelector,
        WorkRequirement, WorkScope, shape_fixtures,
    };
    use std::collections::BTreeSet;

    fn baseline_row() -> CompilerProfileRow {
        match shape_fixtures::compiler_local_lexical_v1() {
            Ok(profile) => match profile.rows.into_iter().next() {
                Some(row) => row,
                None => unreachable!("local lexical fixture always has a first row"),
            },
            Err(error) => unreachable!("local lexical fixture builds: {error}"),
        }
    }

    fn baseline_profile() -> CompilerProfileDefinition {
        match shape_fixtures::compiler_maintained_code_intelligence_v1() {
            Ok(profile) => profile,
            Err(error) => unreachable!("maintained fixture builds: {error}"),
        }
    }

    fn assert_invalid(profile: &CompilerProfileDefinition, expected: &str, context: &str) {
        let error = match profile.validate() {
            Err(error) => error,
            Ok(()) => unreachable!("{context}"),
        };
        let text = format!("{error:#}");
        assert!(text.contains(expected), "{context}: expected {expected:?}, got {text}");
    }

    // The issue's falsifier 1: #12079 or one local lexical pass can stand in
    // for a stronger profile.
    #[test]
    fn falsifier_01_local_lexical_cannot_stand_in_for_stronger_profile() -> Result<()> {
        let local = shape_fixtures::compiler_local_lexical_v1()?;
        let maintained = baseline_profile();
        assert_ne!(local.semantic_fingerprint()?, maintained.semantic_fingerprint()?);
        // A local-lexical-only profile cannot close the maintained profile's
        // import: every higher row would be missing.
        let mut pretender = local.clone();
        pretender.id = CompilerProfileId::new("compiler_maintained_code_intelligence")?;
        pretender.imports = maintained.imports.clone();
        let lower = shape_fixtures::compiler_bounded_execution_v1()?;
        let error = match pretender.verify_import_closure(&lower) {
            Err(error) => error,
            Ok(()) => {
                unreachable!(
                    "a local-lexical-only profile must not close a stronger profile's import"
                )
            }
        };
        assert!(
            error.to_string().contains("is missing"),
            "closure must name missing rows: {error}"
        );
        Ok(())
    }

    // Falsifier 2: all long-horizon compiler work becomes a prerequisite to
    // the bounded local profile.
    #[test]
    fn falsifier_02_local_profile_has_no_long_horizon_prerequisites() -> Result<()> {
        let local = shape_fixtures::compiler_local_lexical_v1()?;
        assert!(local.imports.is_empty(), "the bounded local profile imports nothing");
        for row in &local.rows {
            assert!(
                matches!(
                    row.evidence.family,
                    ClaimFamily::ParserInternal | ClaimFamily::TestReachability
                ),
                "local lexical profile must not require long-horizon family {:?}",
                row.evidence.family
            );
            assert_eq!(row.evidence.source_tier, SourceTier::Source);
            assert!(matches!(row.work, WorkRequirement::Correctness));
        }
        Ok(())
    }

    // Falsifier 3: issue/PR/workflow state enters the evidence model.
    #[test]
    fn falsifier_03_workflow_state_has_no_place_in_the_evidence_model() {
        assert_eq!(ClaimFamily::ALL.len(), 13);
        assert_eq!(ProofClass::ALL.len(), 4);
        assert_eq!(SourceTier::ALL.len(), 5);
        for tag in ClaimFamily::ALL
            .iter()
            .map(|family| family.tag())
            .chain(ProofClass::ALL.iter().map(|class| class.tag()))
            .chain(SourceTier::ALL.iter().map(|tier| tier.tag()))
        {
            for token in tag.split('_') {
                for forbidden in ["issue", "pr", "workflow", "github", "merge", "review", "ticket"]
                {
                    assert!(
                        token != forbidden,
                        "evidence dimension tag {tag:?} must not encode workflow state ({forbidden})"
                    );
                }
            }
        }
    }

    // Falsifier 4: parser proof can satisfy provider/edit/installed-host proof.
    #[test]
    fn falsifier_04_claim_families_cannot_cross_satisfy() -> Result<()> {
        let parser_row = baseline_row();
        for family in [ClaimFamily::Provider, ClaimFamily::Edit, ClaimFamily::InstalledHost] {
            let mut retyped = parser_row.clone();
            retyped.evidence.family = family;
            assert_ne!(parser_row, retyped, "family {family:?} must be a distinct proposition");
        }
        let mut provider_profile = shape_fixtures::compiler_local_lexical_v1()?;
        provider_profile.rows[0].evidence.family = ClaimFamily::Provider;
        assert_ne!(
            shape_fixtures::compiler_local_lexical_v1()?.semantic_fingerprint()?,
            provider_profile.semantic_fingerprint()?,
            "re-typing a parser row as provider must change profile identity"
        );
        Ok(())
    }

    // Falsifier 5: fixture replay or oracle agreement can satisfy EIR
    // mechanism/evaluation.
    #[test]
    fn falsifier_05_oracle_agreement_cannot_satisfy_eir_mechanism() -> Result<()> {
        let oracle = EvidenceRequirement::new(
            ClaimFamily::Execution,
            SourceTier::Source,
            BTreeSet::from([ProofClass::RealPerlOracle]),
        )?;
        let eir = EvidenceRequirement::new(
            ClaimFamily::Execution,
            SourceTier::Source,
            BTreeSet::from([ProofClass::EirMechanism]),
        )?;
        assert_ne!(oracle, eir, "oracle agreement is not an EIR mechanism");
        let curated = EvidenceRequirement::new(
            ClaimFamily::Execution,
            SourceTier::Source,
            BTreeSet::from([ProofClass::CuratedExpectation]),
        )?;
        assert_ne!(curated, eir, "fixture replay is not an EIR mechanism");
        let multi = EvidenceRequirement::new(
            ClaimFamily::Execution,
            SourceTier::Source,
            BTreeSet::from([ProofClass::CuratedExpectation, ProofClass::RealPerlOracle]),
        )?;
        assert_ne!(
            multi.proof_axes,
            BTreeSet::from([ProofClass::EirMechanism, ProofClass::EvaluatedWork]),
            "no combination of weaker axes collapses into EIR/evaluation"
        );
        Ok(())
    }

    // Falsifier 6: source-locked debt can be typed as general semantic support.
    #[test]
    fn falsifier_06_source_locked_debt_cannot_be_typed_as_general_support() -> Result<()> {
        for disposition in [
            RowDisposition::unsupported("tracked debt, not supported")?,
            RowDisposition::not_applicable("not applicable to this subject")?,
        ] {
            for ceiling in [ClaimCeiling::AcceptedCompatibility, ClaimCeiling::BoundedPublicClaim] {
                let mut row = baseline_row();
                row.disposition = disposition.clone();
                row.ceiling = ceiling;
                let mut profile = shape_fixtures::compiler_local_lexical_v1()?;
                profile.rows[0] = row;
                assert_invalid(
                    &profile,
                    "cannot claim more than observed evidence",
                    "a non-applicable row must not claim an elevated ceiling",
                );
            }
        }

        let mut strengthened = shape_fixtures::compiler_local_lexical_v1()?;
        strengthened.rows[0].ceiling = ClaimCeiling::BoundedPublicClaim;
        assert_ne!(
            shape_fixtures::compiler_local_lexical_v1()?.semantic_fingerprint()?,
            strengthened.semantic_fingerprint()?,
            "strengthening a ceiling must change profile identity"
        );
        Ok(())
    }

    // Falsifier 7: source/exact-process/package/install/client stages collapse.
    #[test]
    fn falsifier_07_stages_cannot_collapse() -> Result<()> {
        assert_eq!(SourceTier::ALL.len(), 5);
        let tags: BTreeSet<&str> = SourceTier::ALL.iter().map(|tier| tier.tag()).collect();
        assert_eq!(tags.len(), 5, "source tiers must be five distinct closed stages");
        let source_row = baseline_row();
        for tier in [
            SourceTier::ExactProcess,
            SourceTier::Packaged,
            SourceTier::InstalledHost,
            SourceTier::ActualClient,
        ] {
            let mut retyped = source_row.clone();
            retyped.evidence.source_tier = tier;
            assert_ne!(source_row, retyped, "tier {tier:?} must not collapse into source");
        }
        Ok(())
    }

    // Falsifier 8: an unsupported/not-proven required row disappears by
    // omission.
    #[test]
    fn falsifier_08_required_row_cannot_disappear_by_omission() -> Result<()> {
        let maintained = baseline_profile();
        let lower = shape_fixtures::compiler_bounded_execution_v1()?;
        maintained.verify_import_closure(&lower)?;

        let mut omitted = maintained.clone();
        let dropped = omitted.rows.remove(0).id.as_str().to_owned();
        assert_ne!(
            omitted.semantic_fingerprint()?,
            maintained.semantic_fingerprint()?,
            "omitting a row must change profile identity"
        );
        let error = match omitted.verify_import_closure(&lower) {
            Err(error) => error,
            Ok(()) => unreachable!("dropping an imported row must break import closure"),
        };
        assert!(
            error.to_string().contains(&dropped) || error.to_string().contains("is missing"),
            "closure error must name the dropped row {dropped}: {error}"
        );

        // The closed alternative to omission exists and validates.
        let unsupported = RowDisposition::unsupported("explicit typed state, never omission")?;
        assert!(unsupported.validate().is_ok());
        assert_eq!(RowDisposition::VARIANT_COUNT, 5);
        Ok(())
    }

    // Falsifier 9: zero-work execution can satisfy a required work row.
    #[test]
    fn falsifier_09_zero_work_cannot_satisfy_a_work_row() {
        assert!(WorkScope::new("").is_err(), "an empty work scope is zero work and must fail");
        assert!(WorkScope::new("   ").is_err(), "whitespace-only work scope must fail");
        assert!(WorkScope::new("bounded execution of the named suite").is_ok());
    }

    // Falsifier 10: cold/oracle work can be typed as production work avoided.
    #[test]
    fn falsifier_10_oracle_cold_work_is_not_production_work() -> Result<()> {
        let scope = WorkScope::new("the same named scope")?;
        let production = WorkRequirement::Production(scope.clone());
        let cold = WorkRequirement::OracleOrCold(scope);
        assert_ne!(production, cold, "oracle/cold work is never production work");

        let mut row = baseline_row();
        row.work = production;
        let mut retyped = row.clone();
        retyped.work = cold;
        assert_ne!(row, retyped, "re-typing cold work as production work must change the row");
        Ok(())
    }

    // Falsifier 11: an imported lower profile loses rows or limitations.
    #[test]
    fn falsifier_11_import_closure_preserves_rows_and_limitations() -> Result<()> {
        let higher = shape_fixtures::compiler_static_project_v1()?;
        let lower = shape_fixtures::compiler_local_lexical_v1()?;
        higher.verify_import_closure(&lower)?;

        // Lost row.
        let mut lost_row = higher.clone();
        lost_row.rows.retain(|row| !row.id.as_str().starts_with("lexical."));
        assert!(
            lost_row.verify_import_closure(&lower).is_err(),
            "an import that loses rows must fail"
        );

        // Weakened row.
        let mut weakened = higher.clone();
        let first =
            match weakened.rows.iter_mut().find(|row| row.id.as_str().starts_with("lexical.")) {
                Some(row) => row,
                None => unreachable!("fixture has lexical rows"),
            };
        first.disposition = RowDisposition::Optional;
        assert!(
            weakened.verify_import_closure(&lower).is_err(),
            "an import that weakens an imported row must fail"
        );

        // Lost limitation.
        let mut lost_limitation = higher.clone();
        lost_limitation.limitations.retain(|limitation| limitation.id != "lim.local-lexical-scope");
        assert!(
            lost_limitation.verify_import_closure(&lower).is_err(),
            "an import that loses a limitation must fail"
        );

        // Stale digest.
        let mut stale = higher.clone();
        let import = match stale.imports.first_mut() {
            Some(import) => import,
            None => unreachable!("fixture imports the lower profile"),
        };
        import.version = CompilerProfileVersion::new("v2")?;
        assert!(stale.verify_import_closure(&lower).is_err(), "a stale version must fail closure");

        // Undeclared import.
        let mut undeclared = higher.clone();
        undeclared.imports.clear();
        assert!(
            undeclared.verify_import_closure(&lower).is_err(),
            "closure against an undeclared lower profile must fail"
        );
        Ok(())
    }

    // Falsifier 12: row ordering changes the profile fingerprint.
    #[test]
    fn falsifier_12_row_order_cannot_change_identity() -> Result<()> {
        let maintained = baseline_profile();
        let expected = maintained.semantic_fingerprint()?;

        let mut reversed = maintained.clone();
        reversed.rows.reverse();
        reversed.limitations.reverse();
        assert_eq!(
            reversed.semantic_fingerprint()?,
            expected,
            "row order must not change identity"
        );

        let mut rebuilt = maintained.clone();
        rebuilt.rows.sort_by(|a, b| b.id.cmp(&a.id));
        assert_eq!(
            rebuilt.semantic_fingerprint()?,
            expected,
            "insertion order must not change identity"
        );

        assert_eq!(
            maintained.semantic_fingerprint()?,
            maintained.semantic_fingerprint()?,
            "fingerprints must be deterministic across repeated computation"
        );
        Ok(())
    }

    // Falsifier 13: a scalar figure or aggregate percentage is introduced.
    #[test]
    fn falsifier_13_no_scalar_aggregate_figure_exists() {
        let source = include_str!("compiler_profile_contract.rs");
        let production = match source.split("#[cfg(test)]").next() {
            Some(production) => production,
            None => unreachable!("module has a production half"),
        };
        let production = production.to_lowercase();
        let tokens: BTreeSet<&str> =
            production.split(|character: char| !character.is_ascii_alphanumeric()).collect();
        for forbidden in
            ["score", "percent", "percentage", "weight", "readiness", "f32", "f64", "ratio"]
        {
            assert!(
                !tokens.contains(forbidden),
                "the production model must not introduce an aggregate figure ({forbidden})"
            );
        }
        // Satisfaction is an exact set of conjunctive rows, never a number.
        let local = match shape_fixtures::compiler_local_lexical_v1() {
            Ok(profile) => profile,
            Err(error) => unreachable!("fixture builds: {error}"),
        };
        let required: BTreeSet<&str> = local.required_unconditional_row_ids();
        assert_eq!(required.len(), 2);
    }

    // Falsifier 14: claim ceiling, legacy exit, owner or invalidation fields
    // are absent.
    #[test]
    fn falsifier_14_ceiling_exit_owner_and_invalidation_are_mandatory() -> Result<()> {
        let row = baseline_row();
        // The fields are non-optional struct members; prove they carry data.
        assert_eq!(row.ceiling, ClaimCeiling::ObservedEvidence);
        assert_eq!(row.legacy_exit, LegacyExitRequirement::NONE);
        assert!(!row.owner.owner.is_empty() && !row.owner.wake_event.is_empty());
        assert!(!row.invalidation.is_empty());

        let mut no_invalidation = shape_fixtures::compiler_local_lexical_v1()?;
        no_invalidation.rows[0].invalidation.clear();
        assert_invalid(
            &no_invalidation,
            "must name at least one invalidation input",
            "a row without invalidation inputs must fail validation",
        );

        let mut no_wake = shape_fixtures::compiler_local_lexical_v1()?;
        no_wake.rows[0].owner.wake_event.clear();
        assert_invalid(
            &no_wake,
            "wake event must not be empty",
            "a row without a wake event must fail validation",
        );

        let mut no_owner = shape_fixtures::compiler_local_lexical_v1()?;
        no_owner.rows[0].owner.owner.clear();
        assert_invalid(
            &no_owner,
            "owner must not be empty",
            "a row without an owner must fail validation",
        );
        Ok(())
    }

    // Falsifier 15: support/release authority can be inferred from a profile
    // result.
    #[test]
    fn falsifier_15_profile_evidence_confers_no_support_release_authority() -> Result<()> {
        // The ceiling vocabulary is closed and exactly these three variants.
        assert_eq!(
            ClaimCeiling::ALL,
            [
                ClaimCeiling::ObservedEvidence,
                ClaimCeiling::AcceptedCompatibility,
                ClaimCeiling::BoundedPublicClaim,
            ]
        );
        // Every ceiling's strongest claim is inspectable data, and none of
        // them is a support, release, or publication authorization.
        for ceiling in ClaimCeiling::ALL {
            for token in ceiling.strongest_claim().split_whitespace() {
                for forbidden in ["support", "release", "publication", "authorization"] {
                    assert!(
                        token != forbidden,
                        "ceiling {ceiling:?} maps toward {forbidden} authority: {}",
                        ceiling.strongest_claim()
                    );
                }
            }
        }
        let maintained = baseline_profile();
        let strongest = match maintained
            .rows
            .iter()
            .filter(|row| row.disposition.is_required())
            .map(|row| row.ceiling)
            .max_by_key(|ceiling| match ceiling {
                ClaimCeiling::ObservedEvidence => 0,
                ClaimCeiling::AcceptedCompatibility => 1,
                ClaimCeiling::BoundedPublicClaim => 2,
            }) {
            Some(ceiling) => ceiling,
            None => unreachable!("maintained fixture has required rows"),
        };
        assert_eq!(strongest, ClaimCeiling::BoundedPublicClaim);
        assert_eq!(strongest.strongest_claim(), "bounded public claim");
        Ok(())
    }

    // Closure law: unconditionally required rows are conjunctive, conditional
    // rows expose their triggers for the downstream evaluator, and the other
    // dispositions are closed typed states.
    #[test]
    fn closure_required_rows_are_conjunctive_and_dispositions_closed() -> Result<()> {
        let maintained = baseline_profile();
        let required = maintained.required_unconditional_row_ids();
        assert!(required.contains("intelligence.edit-authorization"));
        assert!(required.contains("intelligence.public-claim"));
        assert!(
            !required.contains("intelligence.actual-client-surface"),
            "an unsupported row is a closed typed state, not a required row"
        );
        for disposition in [
            RowDisposition::Required,
            RowDisposition::conditional("while the named trigger holds")?,
            RowDisposition::Optional,
            RowDisposition::unsupported("named reason")?,
            RowDisposition::not_applicable("named justification")?,
        ] {
            disposition.validate()?;
        }
        // Conditional rows are not silently dropped from the conjunction
        // surface: their triggers stay visible for the downstream evaluator.
        assert!(maintained.conditional_row_triggers().is_empty());
        let mut with_conditional = maintained.clone();
        let conditional_id = with_conditional.rows[0].id.as_str().to_owned();
        with_conditional.rows[0].disposition =
            RowDisposition::conditional("while the named trigger holds")?;
        assert_eq!(
            with_conditional.conditional_row_triggers().get(conditional_id.as_str()),
            Some(&"while the named trigger holds")
        );
        assert!(
            !with_conditional.required_unconditional_row_ids().contains(conditional_id.as_str())
        );
        Ok(())
    }

    // Closure law: profile identity changes when any semantic row field
    // changes.
    #[test]
    fn closure_identity_changes_with_any_semantic_row_field() -> Result<()> {
        let base = shape_fixtures::compiler_local_lexical_v1()?;
        let expected = base.semantic_fingerprint()?;

        let mutations: Vec<fn(&mut CompilerProfileDefinition)> = vec![
            |profile| profile.rows[0].disposition = RowDisposition::Optional,
            |profile| {
                profile.rows[0].subject =
                    SubjectSelector::LocalLexical(match SubjectRef::new("another subject") {
                        Ok(subject) => subject,
                        Err(error) => unreachable!("subject builds: {error}"),
                    });
            },
            |profile| profile.rows[0].evidence.source_tier = SourceTier::ExactProcess,
            |profile| {
                profile.rows[0].evidence.proof_axes.insert(ProofClass::RealPerlOracle);
            },
            |profile| {
                profile.rows[0].completeness.coverage = CoverageRule::Exhaustive;
            },
            |profile| {
                profile.rows[0].work =
                    WorkRequirement::Production(match WorkScope::new("non-zero production work") {
                        Ok(scope) => scope,
                        Err(error) => unreachable!("work scope builds: {error}"),
                    });
            },
            |profile| profile.rows[0].ceiling = ClaimCeiling::AcceptedCompatibility,
            |profile| {
                profile.rows[0].legacy_exit.replacement_currentness = Obligation::Required;
            },
            |profile| profile.rows[0].owner.wake_event = "another wake".to_owned(),
        ];
        for (index, mutate) in mutations.iter().enumerate() {
            let mut changed = base.clone();
            mutate(&mut changed);
            assert_ne!(
                changed.semantic_fingerprint()?,
                expected,
                "semantic mutation {index} must change profile identity"
            );
        }
        Ok(())
    }

    // Acceptance: the four initial profile shapes are representable, closed,
    // and form an exact import chain.
    #[test]
    fn shape_fixtures_are_representable_and_form_a_closed_import_chain() -> Result<()> {
        let local = shape_fixtures::compiler_local_lexical_v1()?;
        let project = shape_fixtures::compiler_static_project_v1()?;
        let execution = shape_fixtures::compiler_bounded_execution_v1()?;
        let maintained = shape_fixtures::compiler_maintained_code_intelligence_v1()?;
        for profile in [&local, &project, &execution, &maintained] {
            profile.validate()?;
        }
        project.verify_import_closure(&local)?;
        execution.verify_import_closure(&project)?;
        maintained.verify_import_closure(&execution)?;
        Ok(())
    }

    // Acceptance: the successor initial-row inventory can instantiate the
    // model without adding a second type vocabulary.
    #[test]
    fn successor_inventory_can_instantiate_without_a_second_vocabulary() -> Result<()> {
        let lower = shape_fixtures::compiler_local_lexical_v1()?;
        let row = CompilerProfileRow {
            id: CompilerProfileRowId::new("inventory.example-row")?,
            disposition: RowDisposition::conditional("while the inventory names the trigger")?,
            subject: SubjectSelector::StaticProject(SubjectRef::new("inventory-owned subject")?),
            evidence: EvidenceRequirement::new(
                ClaimFamily::ProjectWorld,
                SourceTier::Source,
                BTreeSet::from([ProofClass::CuratedExpectation, ProofClass::EirMechanism]),
            )?,
            completeness: CompletenessRequirement {
                currentness: CurrentnessRule::ProjectWorldCurrent,
                coverage: CoverageRule::Bounded { boundary: "inventory-named bound".to_owned() },
            },
            work: WorkRequirement::OracleOrCold(WorkScope::new("inventory-named cold work")?),
            limitations: vec![AllowedLimitation::new(
                "lim.inventory-example",
                "inventory construct",
                "inventory-owned reason",
                "inventory owner",
            )?],
            legacy_exit: LegacyExitRequirement {
                replacement_currentness: Obligation::Required,
                old_path_absence: Obligation::NotApplicable,
                recurrence_proof: Obligation::NotApplicable,
            },
            ceiling: ClaimCeiling::AcceptedCompatibility,
            invalidation: vec![
                InvalidationInput::new(InvalidationKind::Source, "source changed")?,
                InvalidationInput::new(InvalidationKind::Oracle, "oracle changed")?,
            ],
            owner: OwnerAndWakeEvent::new("inventory owner", "inventory wake event")?,
        };
        let mut rows = lower.rows.clone();
        rows.push(row);
        let instantiated = CompilerProfileDefinition {
            id: CompilerProfileId::new("inventory_example")?,
            version: CompilerProfileVersion::new("v1")?,
            purpose: "successor inventory instantiation proof".to_owned(),
            change_reason: "instantiated from public constructors only".to_owned(),
            owner: OwnerAndWakeEvent::new("inventory owner", "inventory wake event")?,
            imports: vec![CompilerProfileImport::for_profile(&lower)?],
            rows,
            limitations: lower.limitations.clone(),
        };
        instantiated.validate()?;
        instantiated.verify_import_closure(&lower)?;
        instantiated.semantic_fingerprint()?;
        Ok(())
    }

    // Legacy-exit axes are independent: one axis can never satisfy another.
    #[test]
    fn legacy_exit_axes_are_independent() -> Result<()> {
        let mut row = baseline_row();
        row.legacy_exit = LegacyExitRequirement {
            replacement_currentness: Obligation::Required,
            old_path_absence: Obligation::NotApplicable,
            recurrence_proof: Obligation::NotApplicable,
        };
        assert_eq!(row.legacy_exit.old_path_absence, Obligation::NotApplicable);
        assert_eq!(row.legacy_exit.recurrence_proof, Obligation::NotApplicable);

        let base = shape_fixtures::compiler_local_lexical_v1()?;
        let mut with_exit = base.clone();
        with_exit.rows[0].legacy_exit.replacement_currentness = Obligation::Required;
        assert_ne!(
            base.semantic_fingerprint()?,
            with_exit.semantic_fingerprint()?,
            "a legacy-exit axis change must change profile identity"
        );
        Ok(())
    }
}
