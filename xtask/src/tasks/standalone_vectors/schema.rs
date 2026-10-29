//! Closed wire schema for the standalone install semantic conformance corpus
//! (`standalone_install_vectors.v1`, #11550 child 1 of #10737).
//!
//! Every type is `deny_unknown_fields`: an unrecognized field is a corpus
//! validation error, never silently-ignored metadata. All collections are
//! order-stable (`Vec` with declared order, maps are `BTreeMap`) so identical
//! inputs serialize byte-identically.
//!
//! This schema is proof-only vocabulary. It encodes the #10243 transaction
//! contract shapes (intent, resolved subject, stage IDs, receipt results) as
//! fixtures; it deliberately copies no live target matrix, release table, or
//! package-channel state (#11550 negative control).

use std::collections::BTreeMap;

/// Corpus schema identifier pinned by the manifest and every vector.
pub const CORPUS_SCHEMA_ID: &str = "standalone_install_vectors.v1";

/// Semantic packet schema identifier emitted by the oracle.
pub const PACKET_SCHEMA_ID: &str = "standalone_semantic_packet.v1";

/// Stage receipt schema identifier assumed when a scripted port does not
/// declare one. A port may declare an unknown schema to exercise the
/// fail-closed unknown-schema rule.
pub const RECEIPT_SCHEMA_ID: &str = "stage_receipt.v1";

/// Top-level corpus manifest: the stable, sorted index of vectors.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct CorpusManifest {
    /// Must equal [`CORPUS_SCHEMA_ID`].
    pub schema: String,
    /// Contract generation shared by every vector in this corpus.
    pub contract_generation: u32,
    /// Vector references, strictly sorted and unique by `vector_id`.
    pub vectors: Vec<VectorRef>,
}

/// One vector reference in the manifest.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct VectorRef {
    pub vector_id: String,
    /// Path relative to the corpus directory.
    pub path: String,
}

/// One semantic conformance vector.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct Vector {
    pub vector_id: String,
    /// Must equal the manifest `contract_generation`.
    pub contract_generation: u32,
    /// Scenario family label (review aid; not matched against live state).
    pub family: String,
    pub platform_classification: PlatformClassification,
    /// Immutable install intent fixture (#10243 two-phase subject model).
    pub intent: IntentFixture,
    /// The single resolved subject produced by the bounded resolver fixture.
    pub resolved_subject: ResolvedSubjectFixture,
    /// Optional explicit fallback subject; required when the intent allows
    /// archive-to-source fallback.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_subject: Option<ResolvedSubjectFixture>,
    /// Declared retry plan: after a terminal failure at `after_stage`, a new
    /// attempt with a fresh identity re-runs the applicable graph.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry: Option<RetryPlan>,
    /// Applicable stage graph with predecessor edges and scripted ports.
    pub stage_graph: Vec<StageSpec>,
    /// Scripted deterministic stage-port responses keyed by script id.
    pub port_scripts: BTreeMap<String, PortScript>,
    /// Headline expected consequences the oracle-derived packet must match.
    pub expected: ExpectedConsequences,
    /// Redaction assertion: tokens that appear in port payloads and must
    /// never appear in the durable semantic packet.
    pub redaction: RedactionAssertion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlatformClassification {
    /// Platform-neutral transaction semantics compared across adapters.
    PlatformNeutral,
    /// References platform-specific behavior only as typed port outcomes;
    /// carries no hosted-platform claim.
    PlatformSpecificReferenceOnly,
}

/// Immutable standalone install intent fixture.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct IntentFixture {
    pub operation_id: String,
    pub attempt_id: String,
    pub route: Route,
    pub mode: Mode,
    pub selector: Selector,
    pub target: TargetFixture,
    pub requested_product_unit: ProductUnit,
    pub fallback_policy: FallbackPolicy,
    pub path_policy: PathPolicy,
    /// Trusted configuration generation digest (fixture identity, not live
    /// configuration).
    pub config_digest: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Route {
    FirstPartyPosix,
    FirstPartyPowershell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    ReleaseArchive,
    ExactRegistrySource,
    ExplicitLocalDevelopment,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Selector {
    LatestRequested,
    Exact { tag: String },
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct TargetFixture {
    pub platform: String,
    pub arch: String,
    pub libc: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductUnit {
    ServerOnly,
    ServerDapPair,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FallbackPolicy {
    Forbidden,
    ArchiveToSourceAllowed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PathPolicy {
    Persist,
    SessionOnly,
}

/// Resolved standalone install subject fixture (#10243). The oracle computes
/// the immutable subject digest from this fixture's canonical serialization;
/// the digest is never stored in the vector.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedSubjectFixture {
    pub subject_id: String,
    /// Must equal the intent mode (resolver cannot change mode).
    pub mode: Mode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_ref: Option<ReleaseRefFixture>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topology: Option<TopologyFixture>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archive: Option<ArchiveSubjectFixture>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registry_subject: Option<RegistrySubjectFixture>,
    pub product_unit: ProductUnit,
    /// Executable roles that must be positively observed for this subject.
    pub required_executables: Vec<String>,
    /// Destination role name (durable packets carry roles, never raw paths).
    pub destination_role: String,
    /// Whether the subject's integrity policy requires independent provenance.
    pub provenance_required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseRefFixture {
    /// Synthetic repository identity; live repository slugs are rejected by
    /// the corpus live-truth scan.
    pub repo: String,
    pub tag: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct TopologyFixture {
    pub topology_id: String,
    /// Frozen topology digest fixture.
    pub digest: String,
    /// Synthetic/checkable topology row identity.
    pub row: String,
    /// Maturity gates the claim ceiling: a historical topology stays a
    /// bounded historical product unit forever.
    pub maturity: TopologyMaturity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TopologyMaturity {
    Current,
    Historical,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArchiveSubjectFixture {
    pub name: String,
    pub format: String,
    pub integrity_policy_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct RegistrySubjectFixture {
    pub registry_id: String,
    pub package: String,
    pub version: String,
    pub toolchain_policy_id: String,
}

/// One stage node with its predecessor edges and scripted port binding.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct StageSpec {
    pub stage_id: StageId,
    pub applicability: Applicability,
    pub predecessors: Vec<StageId>,
    /// Key into `port_scripts`.
    pub port_script: String,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Deserialize, serde::Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum StageId {
    ResolveSubject,
    Transport,
    ChecksumIntegrity,
    Provenance,
    ArchiveManifestAndStaging,
    ExecutableObservation,
    SourceBuild,
    Promotion,
    PathPersistence,
    FreshProcessObservation,
    InstalledTransition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Applicability {
    /// Mandatory when reached; a missing or mislabeled result fails closed.
    Required,
    /// Skipped only when the (mode, stage) authorization map positively
    /// authorizes it; otherwise the corpus itself is invalid.
    NotApplicable,
}

/// Scripted responses consumed in order by one stage's port. An empty call
/// list models a port that produced nothing: a mandatory stage bound to it
/// fails closed with `missing_evidence` (never normalized away).
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct PortScript {
    pub calls: Vec<PortCall>,
}

/// One deterministic stage-port response.
///
/// Ports are data, not executables. They record every observable a real port
/// would return and can deliberately return wrong identities, stale
/// completions, instrument states, and private payloads. Ports never
/// normalize or repair adapter output (#11550 fixture-port protocol).
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct PortCall {
    pub result: PortResult,
    /// Receipt schema the port claims; defaults to [`RECEIPT_SCHEMA_ID`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_schema: Option<String>,
    /// Deliberate corruption: the returned receipt binds a corrupted subject
    /// digest instead of the resolved subject.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub corrupt_subject_digest: bool,
    /// Deliberate corruption: the returned receipt cites predecessor digests
    /// that are not in this attempt's validated chain.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub corrupt_predecessor_digests: bool,
    /// Archive-pair executable observation results by role.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executables: Option<BTreeMap<String, ExecutableObservation>>,
    /// Transport returned an artifact tagged newer than the resolved
    /// subject (latest drifted after resolution).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub newer_latest_tag: Option<String>,
    /// Bounded artifact identities contributed by this call.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<String>,
    /// Bounded evidence identities contributed by this call.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<String>,
    /// Side effects observed by this call, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<EffectRecord>,
    /// Marks this completion as a delayed arrival from another attempt; a
    /// conformant composer records it without advancing the newer attempt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stale_from_attempt: Option<String>,
    /// Private process-local payloads the port saw (temp paths, tokens).
    /// These must never enter the durable packet.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub private_notes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PortResult {
    Success,
    Failure,
    Cancelled,
    Timeout,
    NotProven,
    NotApplicable,
    InstrumentUnavailable,
    InstrumentDegraded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutableObservation {
    Ok,
    Invalid,
    Absent,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct EffectRecord {
    /// Highest side-effect ceiling this effect reaches.
    pub level: CeilingLevel,
    /// Bounded effect kind (e.g. `staged_archive`, `promoted`,
    /// `path_persisted`, `rollback`).
    pub kind: String,
}

/// Ordered side-effect ceilings a standalone install can reach.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Deserialize, serde::Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum CeilingLevel {
    None,
    ResolveOnly,
    TransportArtifacts,
    Staged,
    PromotionReached,
    PathPersisted,
    InstalledClaim,
}

/// Headline consequences asserted against the oracle-derived packet.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedConsequences {
    pub terminal_result: TerminalResult,
    pub terminal_stage: StageId,
    pub reason_family: ReasonFamily,
    pub action_class: ActionClass,
    pub side_effect_ceiling: CeilingLevel,
    pub claim_ceiling: ClaimCeiling,
    pub pair_claims_satisfied: bool,
    /// Number of branch records in the packet (2 when fallback ran).
    pub branch_count: usize,
    /// Number of attempt records retained in history (never erased).
    pub attempt_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalResult {
    Succeeded,
    Failed,
    Cancelled,
}

/// Bounded reason vocabulary owned by this contract (derived from the #10243
/// stage/result semantics). Not open-ended strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasonFamily {
    None,
    TransportFailed,
    IntegrityFailed,
    ProvenanceFailed,
    ArchiveInvalid,
    ObservationFailed,
    PairIncomplete,
    HealthCheckFailed,
    MissingEvidence,
    UnauthorizedNotApplicable,
    SubjectMismatch,
    PredecessorMismatch,
    UnknownSchema,
    InstrumentFailure,
    NotProven,
    Timeout,
    Cancelled,
}

/// Bounded next-action vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionClass {
    None,
    AbortInstall,
    VerifyEnvironmentThenRetry,
    CreateFallbackBranch,
    RetryNewAttempt,
}

/// What the terminal outcome authorizes claiming.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimCeiling {
    None,
    /// Promoted state exists but is not known-good (health gate failed or
    /// cancellation after promotion).
    CurrentStatePromotion,
    InstalledReleaseClaim,
    /// Explicitly historical topology: bounded historical evidence forever.
    HistoricalEvidenceOnly,
    /// Local development is non-authoritative and satisfies no install claim.
    LocalDevelopmentOnly,
}

/// Redaction assertion binding port-private tokens to packet exclusion.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct RedactionAssertion {
    /// Tokens that MUST appear in at least one port `private_notes` entry
    /// (a vacuous assertion is a corpus error) and MUST NOT appear in the
    /// durable packet.
    pub forbidden_tokens: Vec<String>,
}

/// Retry plan: after a terminal failure at `after_stage`, rerun the
/// applicable graph under a fresh attempt identity.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct RetryPlan {
    pub after_stage: StageId,
    pub attempt_id: String,
}

impl PortCall {
    /// The receipt schema this call claims.
    pub fn claimed_receipt_schema(&self) -> &str {
        self.receipt_schema.as_deref().unwrap_or(RECEIPT_SCHEMA_ID)
    }
}
