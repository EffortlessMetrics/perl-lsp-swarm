//! Typed vocabulary for the maintained compiler-profile model (#12186).
//!
//! The enumerations and newtypes below are the stable in-memory vocabulary the
//! successor initial-row inventory instantiates; they intentionally carry no
//! GitHub, workflow, LSP DTO, provider implementation, receipt payload, or live
//! candidate state. Distinctions the closure law needs are load-bearing here:
//!
//! - observed upstream result / accepted compatibility state / general semantic
//!   support stay distinct [`ProofClass`] variants, so source-locked debt is
//!   never typed as general semantic support;
//! - parser/semantic/PIR fact production, provider consumption, and edit
//!   authorization are separate classes, so parser proof cannot satisfy
//!   provider, edit, or installed-host proof;
//! - project/world currentness and cross-file external behavior are separate;
//! - curated expectation, real-Perl oracle, EIR mechanism, and evaluated work
//!   are separate, so fixture replay or oracle agreement cannot satisfy EIR
//!   mechanism/evaluation;
//! - [`SourceTier`] keeps source, exact-process, packaged, installed-host, and
//!   actual-client stages distinct;
//! - [`WorkClass`] keeps correctness, production work, oracle/cold work, and
//!   performance/resource results distinct, with non-zero floors so zero-work
//!   execution cannot satisfy a required work row;
//! - [`ClaimFamily`] separates profile evidence from support, release, and
//!   publication authorization.

use std::num::NonZeroU64;

use serde::{Deserialize, Serialize};

use super::{CompilerProfileContractError, is_digest_hex, is_stable_token};

// ---------------------------------------------------------------------------
// Identity newtypes
// ---------------------------------------------------------------------------

/// Maintained profile identity, for example `compiler_local_lexical`. Two
/// profile references are the same maintained thing when their ids, versions,
/// and definition fingerprints agree.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct CompilerProfileId(String);

impl CompilerProfileId {
    pub fn new(value: &str) -> Result<Self, CompilerProfileContractError> {
        if !is_stable_token(value) {
            return Err(CompilerProfileContractError::Schema {
                field: "profile_id".to_string(),
                message: format!("`{value}` is not a stable profile token"),
            });
        }
        Ok(Self(value.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Version of a maintained profile, `v<digits>`.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct CompilerProfileVersion(String);

impl CompilerProfileVersion {
    pub fn new(value: &str) -> Result<Self, CompilerProfileContractError> {
        let digits = value.strip_prefix('v').unwrap_or_default();
        if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(CompilerProfileContractError::Schema {
                field: "profile_version".to_string(),
                message: format!("`{value}` is not `v<digits>`"),
            });
        }
        Ok(Self(value.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Stable identity of one row inside a profile. Row ids own the profile
/// denominator, never ordering or wording.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct CompilerProfileRowId(String);

impl CompilerProfileRowId {
    pub fn new(value: &str) -> Result<Self, CompilerProfileContractError> {
        if !is_stable_token(value) {
            return Err(CompilerProfileContractError::Schema {
                field: "row_id".to_string(),
                message: format!("`{value}` is not a stable row token"),
            });
        }
        Ok(Self(value.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Canonical owner surface for a row or limitation: a stable token naming the
/// owning product/evidence surface. GitHub identifiers are deliberately not
/// representable here; the successor inventory owns the owner map.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct OwnerToken(String);

impl OwnerToken {
    pub fn new(value: &str) -> Result<Self, CompilerProfileContractError> {
        if !is_stable_token(value) {
            return Err(CompilerProfileContractError::Schema {
                field: "owner".to_string(),
                message: format!("`{value}` is not a stable owner token"),
            });
        }
        Ok(Self(value.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// SHA-256 hex digest binding an exact profile definition.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct ProfileDigest(String);

impl ProfileDigest {
    pub fn from_hex(value: &str) -> Result<Self, CompilerProfileContractError> {
        if !is_digest_hex(value) {
            return Err(CompilerProfileContractError::Identity {
                message: "profile digest is not 64 lowercase hex characters".to_string(),
            });
        }
        Ok(Self(value.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

// ---------------------------------------------------------------------------
// Independent axes
// ---------------------------------------------------------------------------

/// What event re-opens a row's obligation or a limitation's review.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WakeEvent {
    UpstreamSeriesMovement,
    WorldSnapshotMovement,
    InterfaceTransition,
    DependencyGraphChange,
    ClientOrToolchainUpgrade,
    SubjectRecurrence,
    ReviewRulingChange,
}

/// What input invalidates a row's evidence and requires regeneration. This is
/// the invalidation closure vocabulary, distinct from [`WakeEvent`]: an
/// invalidation input destroys evidence freshness, a wake event re-opens the
/// owning obligation.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvalidationInput {
    SubjectChange,
    EditTouch,
    InterfaceTransition,
    DependencyGraphChange,
    WorldSnapshotMovement,
    UpstreamSeriesMovement,
    ToolchainOrClientUpgrade,
    RecurrenceObserved,
    ReviewRulingChange,
}

/// The stage at which evidence was produced or a proposition holds. The five
/// stages never collapse: source-stage proof never satisfies exact-process,
/// packaged, installed-host, or actual-client requirements.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceTier {
    Source,
    ExactProcess,
    Packaged,
    InstalledHost,
    ActualClient,
}

impl SourceTier {
    pub const ALL: [SourceTier; 5] = [
        SourceTier::Source,
        SourceTier::ExactProcess,
        SourceTier::Packaged,
        SourceTier::InstalledHost,
        SourceTier::ActualClient,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            SourceTier::Source => "source",
            SourceTier::ExactProcess => "exact_process",
            SourceTier::Packaged => "packaged",
            SourceTier::InstalledHost => "installed_host",
            SourceTier::ActualClient => "actual_client",
        }
    }
}

/// The class of proposition a row's evidence must establish. Classes are
/// independent axes: satisfying one class never satisfies another, and a row
/// may require several classes conjunctively.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProofClass {
    // Observed upstream result vs accepted compatibility state vs general
    // semantic support.
    ObservedUpstreamResult,
    AcceptedCompatibilityState,
    GeneralSemanticSupport,
    // Parser/semantic/PIR fact production vs provider consumption vs edit
    // authorization.
    ParserFactProduction,
    SemanticFactProduction,
    PirFactProduction,
    ProviderConsumption,
    EditAuthorization,
    // Project/world currentness vs cross-file external behavior.
    ProjectWorldCurrentness,
    CrossFileExternalBehavior,
    // Curated expectation vs real-Perl oracle vs EIR mechanism vs evaluated
    // work.
    CuratedExpectation,
    RealPerlOracle,
    EirMechanism,
    EvaluatedWork,
    // Debt retirement distinctions.
    ReplacementCurrentness,
    OldPathAbsence,
    RecurrenceProof,
    // Exact external product/test reachability.
    TestReachability,
    // Performance/resource results.
    PerformanceResourceResult,
}

impl ProofClass {
    pub const ALL: [ProofClass; 19] = [
        ProofClass::ObservedUpstreamResult,
        ProofClass::AcceptedCompatibilityState,
        ProofClass::GeneralSemanticSupport,
        ProofClass::ParserFactProduction,
        ProofClass::SemanticFactProduction,
        ProofClass::PirFactProduction,
        ProofClass::ProviderConsumption,
        ProofClass::EditAuthorization,
        ProofClass::ProjectWorldCurrentness,
        ProofClass::CrossFileExternalBehavior,
        ProofClass::CuratedExpectation,
        ProofClass::RealPerlOracle,
        ProofClass::EirMechanism,
        ProofClass::EvaluatedWork,
        ProofClass::ReplacementCurrentness,
        ProofClass::OldPathAbsence,
        ProofClass::RecurrenceProof,
        ProofClass::TestReachability,
        ProofClass::PerformanceResourceResult,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            ProofClass::ObservedUpstreamResult => "observed_upstream_result",
            ProofClass::AcceptedCompatibilityState => "accepted_compatibility_state",
            ProofClass::GeneralSemanticSupport => "general_semantic_support",
            ProofClass::ParserFactProduction => "parser_fact_production",
            ProofClass::SemanticFactProduction => "semantic_fact_production",
            ProofClass::PirFactProduction => "pir_fact_production",
            ProofClass::ProviderConsumption => "provider_consumption",
            ProofClass::EditAuthorization => "edit_authorization",
            ProofClass::ProjectWorldCurrentness => "project_world_currentness",
            ProofClass::CrossFileExternalBehavior => "cross_file_external_behavior",
            ProofClass::CuratedExpectation => "curated_expectation",
            ProofClass::RealPerlOracle => "real_perl_oracle",
            ProofClass::EirMechanism => "eir_mechanism",
            ProofClass::EvaluatedWork => "evaluated_work",
            ProofClass::ReplacementCurrentness => "replacement_currentness",
            ProofClass::OldPathAbsence => "old_path_absence",
            ProofClass::RecurrenceProof => "recurrence_proof",
            ProofClass::TestReachability => "test_reachability",
            ProofClass::PerformanceResourceResult => "performance_resource_result",
        }
    }
}

/// The exact subject a row talks about. Typed variants replace string
/// conventions; the successor inventory instantiates them without a second
/// vocabulary.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(tag = "subject", rename_all = "snake_case")]
pub enum SubjectSelector {
    SelectedUpstreamSeries { series: String },
    AcceptedDebtLedger,
    CompilerPipelineFacts,
    SameFileInitializedLexicals,
    SameFileCompleteOrRefuseRename,
    ExactEditorProductSurface { product: String },
    ProjectWorldSnapshot,
    CompileTimeDependencyGraph,
    CrossFileNavigation,
    CrossFileRefactor,
    CuratedGoldExpectations,
    RealPerlOracleRows,
    EirAdmittedEffects,
    UnsupportedDynamicBoundaries,
    MaintainedTargetDenominator,
    ReleaseProcessJourney,
    PerformanceResourceEnvelope,
}

/// Kind of work a row obligates. Correctness bounds, production work,
/// oracle/cold work, and performance/resource results never satisfy each
/// other, and floors are non-zero.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkClass {
    Correctness,
    Production,
    OracleCold,
    PerformanceResource,
}

// ---------------------------------------------------------------------------
// Claim plane
// ---------------------------------------------------------------------------

/// The claim plane a statement lives on. Profile evidence never authorizes
/// support, release, or publication; those live in their own authorization
/// surfaces (`xtask::publication_drift`).
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimFamily {
    ProfileEvidence,
    SupportAuthorization,
    ReleaseAuthorization,
    PublicationAuthorization,
}

impl ClaimFamily {
    pub const ALL: [ClaimFamily; 4] = [
        ClaimFamily::ProfileEvidence,
        ClaimFamily::SupportAuthorization,
        ClaimFamily::ReleaseAuthorization,
        ClaimFamily::PublicationAuthorization,
    ];

    pub const fn rank(self) -> u8 {
        match self {
            ClaimFamily::ProfileEvidence => 0,
            ClaimFamily::SupportAuthorization => 1,
            ClaimFamily::ReleaseAuthorization => 2,
            ClaimFamily::PublicationAuthorization => 3,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            ClaimFamily::ProfileEvidence => "profile_evidence",
            ClaimFamily::SupportAuthorization => "support_authorization",
            ClaimFamily::ReleaseAuthorization => "release_authorization",
            ClaimFamily::PublicationAuthorization => "publication_authorization",
        }
    }
}

/// Upper bound on which claim families a row's result may be cited for.
/// Validation pins every row to profile evidence only, so support, release,
/// and publication authority cannot be inferred from a profile result.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct ClaimCeiling(ClaimFamily);

impl ClaimCeiling {
    pub const fn new(family: ClaimFamily) -> Self {
        Self(family)
    }

    pub const fn profile_evidence() -> Self {
        Self(ClaimFamily::ProfileEvidence)
    }

    pub fn family(self) -> ClaimFamily {
        self.0
    }

    /// Whether evidence under this ceiling may be cited for `family`.
    pub const fn permits(self, family: ClaimFamily) -> bool {
        family.rank() <= self.0.rank()
    }
}

// ---------------------------------------------------------------------------
// Requirements and observations
// ---------------------------------------------------------------------------

/// One observation: an evidence class established at a source tier. This is
/// row-local axis typing for the closure law, not candidate evaluation.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct EvidenceObservation {
    pub class: ProofClass,
    pub tier: SourceTier,
}

/// The conjunctive evidence axes a row demands: every required class must be
/// matched by an observation of exactly that class at one of the required
/// tiers. No class satisfies another; tiers are a separate axis.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceRequirement {
    pub required_classes: std::collections::BTreeSet<ProofClass>,
    pub required_tiers: std::collections::BTreeSet<SourceTier>,
}

impl EvidenceRequirement {
    pub fn new(
        required_classes: std::collections::BTreeSet<ProofClass>,
        required_tiers: std::collections::BTreeSet<SourceTier>,
    ) -> Result<Self, CompilerProfileContractError> {
        if required_classes.is_empty() {
            return Err(CompilerProfileContractError::Schema {
                field: "evidence.required_classes".to_string(),
                message: "a row must name at least one proof class".to_string(),
            });
        }
        if required_tiers.is_empty() {
            return Err(CompilerProfileContractError::Schema {
                field: "evidence.required_tiers".to_string(),
                message: "a row must name at least one source tier".to_string(),
            });
        }
        Ok(Self { required_classes, required_tiers })
    }

    /// Row-local closure predicate: each required class is covered by an
    /// observation of exactly that class at a required tier. This never
    /// performs profile evaluation or status generation (#12177 owns those).
    pub fn is_satisfied_by(&self, observations: &[EvidenceObservation]) -> bool {
        self.required_classes.iter().all(|class| {
            observations.iter().any(|observation| {
                observation.class == *class && self.required_tiers.contains(&observation.tier)
            })
        })
    }
}

/// How much of the subject scope a row's evidence must describe to be
/// complete, and which currentness binding it carries.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompletenessRequirement {
    pub rule: CompletenessRule,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(tag = "rule", rename_all = "snake_case")]
pub enum CompletenessRule {
    /// Evidence must describe the current subject state; stale evidence never
    /// completes the row.
    CurrentSubjectState,
    /// Every subject in scope must be covered; a subset never completes.
    ExhaustiveCoverage,
    /// A reviewed representative sample completes the row.
    RepresentativeSample { sample_id: String },
    /// Exactly the enumerated denominator completes the row.
    ExactDenominator { denominator_id: String },
}

/// A work obligation with a non-zero floor: zero-work execution can never
/// satisfy it, and work classes never satisfy each other.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkRequirement {
    pub class: WorkClass,
    pub minimum_units: NonZeroU64,
}

impl WorkRequirement {
    pub fn new(class: WorkClass, minimum_units: u64) -> Result<Self, CompilerProfileContractError> {
        let minimum_units =
            NonZeroU64::new(minimum_units).ok_or(CompilerProfileContractError::Schema {
                field: "work.minimum_units".to_string(),
                message: "a required work row must demand at least one unit".to_string(),
            })?;
        Ok(Self { class, minimum_units })
    }

    /// Row-local closure predicate for work records: same class, at least the
    /// demanded units.
    pub fn is_satisfied_by(&self, observation: WorkObservation) -> bool {
        observation.class == self.class && observation.units >= self.minimum_units.get()
    }
}

/// One record of performed work: a class and a unit count.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct WorkObservation {
    pub class: WorkClass,
    pub units: u64,
}

// ---------------------------------------------------------------------------
// Limitations, exits, ownership
// ---------------------------------------------------------------------------

/// An allowed limitation: a typed statement of what a profile explicitly does
/// not claim, owned and woken like a row.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AllowedLimitation {
    pub boundary: String,
    pub owner: OwnerAndWakeEvent,
}

/// How a row is bounded by the profile's allowed limitations.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(tag = "policy", rename_all = "snake_case")]
pub enum LimitationPolicy {
    /// The row is bounded by the named limitation ids (which must exist in
    /// the profile).
    BoundedBy { limitation_ids: std::collections::BTreeSet<String> },
    /// The row holds unconditionally within the profile scope.
    Unbounded,
}

/// Legacy exit obligation for rows that replace a legacy path: proof classes
/// restricted to old-path absence and recurrence proof.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyExitRequirement {
    pub legacy_path: String,
    pub required_proof: std::collections::BTreeSet<ProofClass>,
}

/// Owner surface plus the wake event that re-opens the obligation.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnerAndWakeEvent {
    pub owner: OwnerToken,
    pub wake_event: WakeEvent,
}

// ---------------------------------------------------------------------------
// Dispositions and rows
// ---------------------------------------------------------------------------

/// The exhaustive closed states of a profile row. Conditional, optional,
/// unsupported, and not-applicable are typed states that stay present in the
/// profile — omission is never a disposition.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(tag = "disposition", rename_all = "snake_case")]
pub enum RowDisposition {
    Required,
    Conditional { condition: String },
    Optional,
    Unsupported { reason: String },
    NotApplicable { ruling: String },
}

impl RowDisposition {
    pub const ALL: [RowDisposition; 5] = [
        RowDisposition::Required,
        RowDisposition::Conditional { condition: String::new() },
        RowDisposition::Optional,
        RowDisposition::Unsupported { reason: String::new() },
        RowDisposition::NotApplicable { ruling: String::new() },
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            RowDisposition::Required => "required",
            RowDisposition::Conditional { .. } => "conditional",
            RowDisposition::Optional => "optional",
            RowDisposition::Unsupported { .. } => "unsupported",
            RowDisposition::NotApplicable { .. } => "not_applicable",
        }
    }
}

/// One row of a maintained profile: an exact subject, its disposition, the
/// conjunctive evidence/completeness/work obligations, limitation policy,
/// legacy exit, ownership, invalidation closure, and claim ceiling.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompilerProfileRow {
    pub row_id: CompilerProfileRowId,
    pub statement: String,
    pub disposition: RowDisposition,
    pub subject: SubjectSelector,
    pub evidence: EvidenceRequirement,
    pub completeness: CompletenessRequirement,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work: Option<WorkRequirement>,
    pub limitation_policy: LimitationPolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy_exit: Option<LegacyExitRequirement>,
    pub owner: OwnerAndWakeEvent,
    pub invalidation: std::collections::BTreeSet<InvalidationInput>,
    pub claim_ceiling: ClaimCeiling,
}

/// An import: the exact lower profile identity, version, and definition
/// digest. Closure validation resolves it against the lower definition and
/// checks verbatim preservation of its rows and limitations.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompilerProfileImport {
    pub profile_id: CompilerProfileId,
    pub version: CompilerProfileVersion,
    pub digest: ProfileDigest,
}

// ---------------------------------------------------------------------------
// Definition
// ---------------------------------------------------------------------------

/// A maintained compiler operating profile: identity, version, purpose,
/// imports, rows, and limitations. In-memory only: loading, manifest syntax,
/// evaluation, and status generation live in successor PRs (#12187+).
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompilerProfileDefinition {
    pub profile_id: CompilerProfileId,
    pub version: CompilerProfileVersion,
    pub purpose: String,
    pub imports: std::collections::BTreeSet<CompilerProfileImport>,
    pub rows: std::collections::BTreeMap<CompilerProfileRowId, CompilerProfileRow>,
    pub limitations: std::collections::BTreeMap<String, AllowedLimitation>,
}
