//! Closed wire schema for the reachability fixture manifest
//! (`analysis_reachability_fixture_manifest.v1`, #10998).
//!
//! One deterministic canonical manifest names the exact local-flow,
//! workspace-fact, graph, operation, profile-budget, policy, diagnostic,
//! transport/currentness, compatibility and proof-closeout population used by
//! later proof PRs (#11004/#11006/#11012/#11018). Every type is
//! `deny_unknown_fields`: an unrecognized field is a manifest validation
//! error, never silently-ignored metadata.
//!
//! The nine-row expectation chain stays structurally separate:
//! facts → graph → structural liveness → operation/work/terminal outcome →
//! policy eligibility → diagnostic item/result → transport response →
//! currentness transition → compatibility disposition are distinct typed
//! slots on [`RowExpectations`], never collapsible into one `dead/live`,
//! `pass/fail`, diagnostic-count or receipt bit.
//!
//! This schema is declaration-only vocabulary. It owns fixture identity,
//! metadata, validation, coverage accounting and generated views; it does not
//! implement analysis, execute semantic or exact-process proof, select product
//! behavior, repair failures, change compatibility or promote a claim.

use std::collections::BTreeMap;

/// Manifest schema identifier pinned by every document and self-fixture.
pub const SCHEMA_ID: &str = "analysis_reachability_fixture_manifest.v1";
/// Canonical manifest name constant.
pub const MANIFEST_NAME: &str = "analysis-reachability-fixture-denominator";
/// Current schema generation.
pub const SCHEMA_VERSION: u64 = 1;
/// Digest algorithm applied to referenced source fixtures. Bytes are read as
/// checked out, CRLF line endings are normalized to LF, then SHA-256 is taken
/// so insertion order and checkout root/crlf settings cannot change identity.
pub const DIGEST_ALGORITHM: &str = "sha256-lf-normalized";
/// Repository-relative canonical manifest location.
pub const MANIFEST_RELATIVE_PATH: &str = "fixtures/analysis_reachability_denominator/manifest.json";
/// Repository-relative generated coverage view (never hand-edited). Named to
/// avoid the repository's `COVERAGE_*.md` ignore pattern.
pub const VIEW_RELATIVE_PATH: &str =
    "fixtures/analysis_reachability_denominator/denominator-coverage-view.md";

/// Closed set of denominator families (issue #10998 "Denominator families").
pub const FAMILIES: &[&str] = &[
    "A_local_flow",
    "W_workspace_facts",
    "G_graph_structure",
    "R_operation_terminal",
    "B_profile_budget",
    "P_closed_world_policy",
    "D_diagnostic_identity",
    "T_transport_currentness",
    "C_compatibility_recurrence",
    "X_proof_closeout",
];

/// Closed set of proof-owner issues that may own a row's proposition.
pub const PROOF_OWNER_ISSUES: &[u64] = &[11004, 11006, 11012, 11018];

/// Top-level manifest document.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// Must equal [`SCHEMA_ID`].
    pub schema: String,
    /// Must equal [`SCHEMA_VERSION`].
    pub schema_version: u64,
    /// Must equal [`MANIFEST_NAME`].
    pub manifest: String,
    /// Controlling issue for this denominator (#10998).
    pub owner_issue: u64,
    /// Declaration-only manifests never claim execution authority.
    pub status: String,
    /// Claim boundary phrases validated by the checker.
    pub claim_boundary: String,
    /// Must equal [`DIGEST_ALGORITHM`].
    pub digest_algorithm: String,
    /// Closed repo-relative roots referenced fixture paths may live under.
    pub allowed_fixture_roots: Vec<String>,
    /// Reviewed authority issue references backing this denominator.
    pub authorities: Vec<String>,
    /// Contract issue identities referenced by typed expectation slots.
    pub contracts: Contracts,
    /// Default proof owner per family; rows may override within the closed set.
    pub proof_owners: BTreeMap<String, u64>,
    /// Declared denominator per family with required coverage and deferrals.
    pub denominator: Vec<FamilyDenominator>,
    /// Row population the document declares. The validator cross-checks this
    /// against the actual `rows` length so silent deletions become violations
    /// instead of a shrinking-but-green denominator.
    pub declared_row_count: u64,
    /// Denominator rows, unique by `row_id` (order in the document is not
    /// meaningful; validation and views sort canonically).
    pub rows: Vec<Row>,
}

/// Contract issue identities referenced by typed expectation slots.
#[derive(Debug, Clone, Default, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct Contracts {
    /// #11553 operation stage/work-dimension contract.
    pub operation_stage_and_work: u64,
    /// #11590 product profile/budget contract.
    pub product_profile_budget: u64,
    /// #8101 closed-world policy contract.
    pub closed_world_policy: u64,
    /// #10941 diagnostic result class/identity inputs.
    pub diagnostic_result_identity: u64,
    /// #8142 diagnostic item/composition/remediation contract.
    pub diagnostic_item_composition: u64,
    /// #10953 push transport surface.
    pub push_transport: u64,
    /// #10957 pull/currentness surface.
    pub pull_currentness: u64,
    /// #9777 compatibility projection contract.
    pub compatibility_projection: u64,
}

/// Declared denominator for one family.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct FamilyDenominator {
    pub family: String,
    /// Coverage classes this family must keep instantiated rows for.
    #[serde(default)]
    pub required_coverage: Vec<String>,
    /// Coverage classes deliberately not yet instantiated, routed to an owner.
    #[serde(default)]
    pub deferred_coverage: Vec<DeferredCoverage>,
}

/// A declared-but-not-yet-instantiated coverage slot. Deferrals stay visible;
/// they can never silently become clean coverage.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct DeferredCoverage {
    pub coverage: String,
    pub owner_issue: u64,
    pub reason: String,
}

/// One denominator row: one fixture subject carrying separated expectations.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct Row {
    /// Stable unique row identifier (`[a-z0-9][a-z0-9.-]*`).
    pub row_id: String,
    /// Referenced fixture identity: bytes stay in their owned location.
    pub fixture: FixtureIdentity,
    /// Source/project/workspace/root/profile/configuration/environment
    /// subject identities this row speaks about.
    pub subjects: Vec<String>,
    /// Exact logical-source roles covered by the fixture reference.
    pub source_roles: Vec<String>,
    /// Stable train node/component/family/claim-profile identities.
    pub train: TrainIdentity,
    /// Required parser/semantic/reference/root/framework prerequisites.
    #[serde(default)]
    pub prerequisites: Vec<String>,
    /// Positive/opposite/near-neighbour control linkage by row id.
    pub controls: RowControls,
    /// The nine-row chain of separated expectation objects.
    pub expectations: RowExpectations,
    /// Declared terminal outcome of this row's proposition.
    pub terminal: TerminalOutcome,
    /// Named result identity/currentness/completeness/operation authority for
    /// exact results; must be absent on non-success terminals.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_identity: Option<ResultIdentity>,
    /// Independent pinned authority (e.g. a gold `expected_module.json`) this
    /// row's proposition restates; drift checking against these bytes is owned
    /// by the named consumer proof.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authority_reference: Option<AuthorityReference>,
    /// Support limitation class and exit owner.
    pub limitation: Limitation,
    /// Oracle type, independence class and proof ceiling.
    pub oracle: Oracle,
    /// Stable race/barrier orchestration metadata (never product state).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub race_barrier: Option<RaceBarrier>,
    /// Instrument availability status; missing instruments stay `not_proven`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instrument: Option<InstrumentStatus>,
    /// Proof-owner override; defaults to the family's `proof_owners` entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_issue: Option<u64>,
}

/// Exact fixture identity: path plus content digest, never copied bytes.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct FixtureIdentity {
    /// Stable fixture identifier; identical ids must pin identical bytes.
    pub id: String,
    /// Repo-relative slash path inside one declared allowed root.
    pub path: String,
    /// Recorded [`DIGEST_ALGORITHM`] digest of the referenced bytes.
    pub digest_sha256_lf: String,
    /// Role of these bytes in this row: `positive`, `control_opposite`,
    /// `control_near_neighbour`, `falsifier` or `shared_canonical`.
    pub role: FixtureRole,
}

/// Closed fixture-role vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FixtureRole {
    Positive,
    ControlOpposite,
    ControlNearNeighbour,
    Falsifier,
    SharedCanonical,
}

impl FixtureRole {
    /// True when the row promotes an expected proposition from these bytes.
    pub fn is_promoted(self) -> bool {
        matches!(self, Self::Positive)
    }
}

/// Stable train identities binding the row to the stable control plane.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct TrainIdentity {
    pub node: String,
    pub component: String,
    pub family: String,
    pub claim_profile: String,
}

/// Positive/opposite/near-neighbour linkage to other rows.
#[derive(Debug, Clone, Default, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct RowControls {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opposite: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub near_neighbour: Option<String>,
}

/// Separated expectation objects: the nine-row chain stays distinct.
#[derive(Debug, Clone, Default, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct RowExpectations {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_flow_facts: Option<FactsExpectation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_facts: Option<FactsExpectation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub graph_structure: Option<GraphExpectation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structural_liveness: Option<LivenessExpectation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation: Option<OperationExpectation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_budget: Option<ProfileExpectation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy: Option<PolicyExpectation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostic: Option<DiagnosticExpectation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transport: Option<TransportExpectation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub currentness: Option<CurrentnessExpectation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compatibility: Option<CompatibilityExpectation>,
}

impl RowExpectations {
    /// Number of populated expectation slots.
    pub fn populated(&self) -> usize {
        [
            self.local_flow_facts.is_some(),
            self.workspace_facts.is_some(),
            self.graph_structure.is_some(),
            self.structural_liveness.is_some(),
            self.operation.is_some(),
            self.profile_budget.is_some(),
            self.policy.is_some(),
            self.diagnostic.is_some(),
            self.transport.is_some(),
            self.currentness.is_some(),
            self.compatibility.is_some(),
        ]
        .into_iter()
        .filter(|present| *present)
        .count()
    }
}

/// Test-support constructors for unit tests and negative-control documents.
#[cfg(test)]
impl RowExpectations {
    /// Facts plus currentness baseline used by canonical rows.
    pub fn facts_with_currentness(
        proposition: &str,
        fact_class: FactClass,
        currentness: CurrentnessExpectation,
    ) -> Self {
        Self {
            local_flow_facts: Some(FactsExpectation {
                proposition: proposition.to_string(),
                fact_class,
            }),
            currentness: Some(currentness),
            ..Self::default()
        }
    }

    /// Single-slot constructors used by negative-control documents.
    pub fn facts_only(proposition: &str, fact_class: FactClass) -> Self {
        Self {
            local_flow_facts: Some(FactsExpectation {
                proposition: proposition.to_string(),
                fact_class,
            }),
            ..Self::default()
        }
    }

    pub fn currentness_only(expectation: CurrentnessExpectation) -> Self {
        Self { currentness: Some(expectation), ..Self::default() }
    }

    pub fn operation_only(expectation: OperationExpectation) -> Self {
        Self { operation: Some(expectation), ..Self::default() }
    }

    pub fn profile_only(expectation: ProfileExpectation) -> Self {
        Self { profile_budget: Some(expectation), ..Self::default() }
    }

    pub fn transport_only(expectation: TransportExpectation) -> Self {
        Self { transport: Some(expectation), ..Self::default() }
    }
}

/// Expected canonical AST/local-flow or workspace fact propositions.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct FactsExpectation {
    pub proposition: String,
    /// Exact value/edge truth versus unknown/partial/recovered boundaries.
    pub fact_class: FactClass,
}

/// Closed fact-class vocabulary: same-spelling, recovered and missing facts
/// can never inherit exact flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FactClass {
    ExactValueOrEdge,
    UnknownOrPartialValue,
    RecoveredMalformedBoundary,
    AbsentFactNonEdge,
}

/// Expected admitted graph-family ledger and condensed shape.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct GraphExpectation {
    pub proposition: String,
    pub shape: GraphShape,
}

/// Closed graph-shape vocabulary (#10998 family G).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphShape {
    IsolatedNode,
    Chain,
    SelfLoop,
    ReachableScc,
    DeadScc,
    CondensedDag,
    CompleteFamilyAdmission,
    MissingFamilyNotAdmitted,
    PartialStaleOrSchemaMismatchedInput,
}

/// Expected production/test closure and structural classification evidence.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct LivenessExpectation {
    pub proposition: String,
    pub classification: LivenessClassification,
}

/// Closed structural-liveness vocabulary; interface-retained and blocker
/// evidence stay orthogonal to closure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LivenessClassification {
    ProductionLive,
    TestOnlyLive,
    RootUnreachable,
    InterfaceRetainedOrthogonal,
    BlockerEvidenceOnly,
}

/// Expected #11553 operation kind/stage/checkpoints/work dimensions and
/// terminal outcome.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct OperationExpectation {
    pub proposition: String,
    /// Material #11553 operation stage this row predeclares.
    pub stage: OperationStage,
    /// Deterministic work dimensions the operation must account for.
    #[serde(default)]
    pub work_dimensions: Vec<String>,
    /// Declared checkpoint positions for cancellation semantics.
    #[serde(default)]
    pub checkpoints: Vec<String>,
    /// Terminal outcome declared for the operation attempt.
    pub terminal_outcome: TerminalOutcome,
}

/// Closed material #11553 operation stages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationStage {
    GraphAdmission,
    SccDiscoveryCondensation,
    ProductionClosure,
    TestClosure,
    ClassificationQuerySourcePartition,
    BoundedExplanation,
    PolicyProjection,
    DiagnosticComposition,
    TransportProjectionChunking,
    ResultReuseFinalPublication,
    SemanticProof,
    ExactProcessProof,
}

impl OperationStage {
    /// Every material #11553 stage in vocabulary order; drives the
    /// stage-completeness coverage pass.
    pub const ALL: &[OperationStage] = &[
        Self::GraphAdmission,
        Self::SccDiscoveryCondensation,
        Self::ProductionClosure,
        Self::TestClosure,
        Self::ClassificationQuerySourcePartition,
        Self::BoundedExplanation,
        Self::PolicyProjection,
        Self::DiagnosticComposition,
        Self::TransportProjectionChunking,
        Self::ResultReuseFinalPublication,
        Self::SemanticProof,
        Self::ExactProcessProof,
    ];

    /// Wire (snake_case) spelling used in documents and `stage:` coverage
    /// tokens.
    pub fn wire_name(self) -> &'static str {
        match self {
            Self::GraphAdmission => "graph_admission",
            Self::SccDiscoveryCondensation => "scc_discovery_condensation",
            Self::ProductionClosure => "production_closure",
            Self::TestClosure => "test_closure",
            Self::ClassificationQuerySourcePartition => "classification_query_source_partition",
            Self::BoundedExplanation => "bounded_explanation",
            Self::PolicyProjection => "policy_projection",
            Self::DiagnosticComposition => "diagnostic_composition",
            Self::TransportProjectionChunking => "transport_projection_chunking",
            Self::ResultReuseFinalPublication => "result_reuse_final_publication",
            Self::SemanticProof => "semantic_proof",
            Self::ExactProcessProof => "exact_process_proof",
        }
    }

    /// Stages whose operations must account for deterministic work dimensions.
    pub fn requires_work_dimensions(self) -> bool {
        matches!(
            self,
            Self::GraphAdmission
                | Self::SccDiscoveryCondensation
                | Self::ProductionClosure
                | Self::TestClosure
                | Self::ClassificationQuerySourcePartition
                | Self::TransportProjectionChunking
                | Self::ResultReuseFinalPublication
        )
    }
}

/// Expected #11590 product profile disposition. The manifest records profile
/// evidence and claim ceiling; it never selects values itself.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileExpectation {
    pub proposition: String,
    pub profile: ProfileName,
    /// Required work dimensions all dispositioned within the profile.
    pub required_work_dimensions: Vec<String>,
    /// Whether the row advertises partial workspace support; only false is
    /// admissible until the safe stream commit proof lands.
    pub partial_support_advertised: bool,
    /// Ordinary representative versus adversarial envelope class.
    pub envelope_class: EnvelopeClass,
}

/// Closed #11590 profile vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileName {
    InteractivePrivate,
    WorkspaceFull,
    ScheduledSemanticProof,
    CompatibilityProjection,
    WorkspacePartial,
}

/// Closed envelope-class vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvelopeClass {
    WithinAdmittedEnvelope,
    HitsFiniteLimit,
}

/// Expected #8101 policy eligibility under a closed-world mode.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyExpectation {
    pub proposition: String,
    pub mode: PolicyMode,
    pub eligibility: PolicyEligibility,
    pub reason: String,
}

/// Closed #8101 policy-mode vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyMode {
    Off,
    PrivateClosedWorld,
    ExplicitClosedWorld,
}

/// Closed policy-eligibility vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyEligibility {
    EligibleIsolatedPrivateCallable,
    EligibleWithComponentEvidence,
    IneligibleProductionLive,
    IneligibleTestOnlyInitialCohort,
    InterfaceOpenWorldRetained,
    BlockedDynamicFramework,
    NotEligiblePartialStaleUnsupportedTerminal,
    EligibleBoundedViewEvidenceRetained,
    NeverEligibleIncompleteSemantics,
    InvalidConfigurationFailClosed,
}

/// Expected neutral diagnostic item/result composition (#10941/#8142).
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticExpectation {
    pub proposition: String,
    /// Canonical diagnostic item identity (e.g. PL406) or receipt class.
    pub item_identity: String,
    pub composition: DiagnosticComposition,
    /// Suppression identity when the item carries one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suppression_identity: Option<String>,
}

/// Closed diagnostic-composition vocabulary. No diagnostic/tag row authorizes
/// deletion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticComposition {
    CompleteLocalOnly,
    CompleteLocalPlusWorkspace,
    WorkspaceDeferredNotReady,
    PartialDegraded,
    TerminalOutcomeReceipt,
    ProviderSetSchemaCatalogChange,
    SameBytesDifferentInstance,
}

/// Expected external transport response rows (#10953).
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct TransportExpectation {
    pub proposition: String,
    pub route: TransportRoute,
    /// Client-visible expectation for chunked, cancelled or superseded
    /// transports; required for those routes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_visible_expectation: Option<String>,
}

/// Closed transport-route vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TransportRoute {
    PublishDiagnostics,
    TextDocumentDiagnostic,
    WorkspaceDiagnosticFull,
    WorkspaceDiagnosticPartialChunks,
}

impl TransportRoute {
    /// Routes whose rows must carry a client-visible expectation.
    pub fn requires_client_visible_expectation(self) -> bool {
        matches!(self, Self::WorkspaceDiagnosticPartialChunks)
    }
}

/// Expected currentness transitions and cache/result-ID dispositions
/// (#10957).
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct CurrentnessExpectation {
    pub proposition: String,
    pub transition: CurrentnessTransition,
    /// Held contributor/target generation identity when applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation: Option<String>,
}

/// Closed currentness-transition vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CurrentnessTransition {
    CallerOnlyLiveToUnreachable,
    CallerOnlyUnreachableToLive,
    IrrelevantEditCompleteEquivalent,
    ConfigProfileProviderChangeSameBytes,
    CloseReopenNewDocumentInstance,
    StaleContributorGenerationMovement,
    FailedRecomputationNeverUnchanged,
    FallbackClearsStaleWorkspaceTier,
    MidPushBatchSupersessionAndRepair,
    PartialStreamSupersessionClientDiscard,
    ResultIdNonEligibleAfterTerminalAttempt,
}

/// Expected compatibility/projection/recurrence disposition (#9777).
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompatibilityExpectation {
    pub proposition: String,
    pub disposition: CompatibilityDisposition,
}

/// Closed compatibility-disposition vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CompatibilityDisposition {
    PublicItemDispositioned,
    LocalAdapterCanonicalProjection,
    WorkspaceAdapterCanonicalProjection,
    LegacyDtoReportLossinessRecorded,
    ZeroOldProviderSelection,
    ZeroBareNameConsumerSurface,
    RawScannerAuthorityAbsent,
    RecurrenceTransferDestination,
    FailureCannotBecomeCompatibilitySuccess,
}

/// Closed terminal-outcome vocabulary shared by rows and operations. Success
/// and failure classes remain distinct typed values, never a collapsed bit.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Deserialize, serde::Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum TerminalOutcome {
    CompleteNonempty,
    CompleteLegitimateEmpty,
    SemanticPartialTypedCeiling,
    NotReady,
    Stale,
    DynamicOrUnsupported,
    CancelledBeforeStart,
    CancelledAtCheckpoint,
    DeadlineExceeded,
    ResourceLimitAtBoundary,
    CheckedNearOverflow,
    SupersededBeforePublication,
    RepeatedOperationReset,
    BoundedViewComplete,
    IncompleteSemanticNeverBoundedComplete,
    InstrumentFailure,
    ProductFailure,
}

impl TerminalOutcome {
    /// Every terminal outcome in vocabulary order; drives the
    /// terminal-completeness coverage pass.
    pub const ALL: &[TerminalOutcome] = &[
        Self::CompleteNonempty,
        Self::CompleteLegitimateEmpty,
        Self::SemanticPartialTypedCeiling,
        Self::NotReady,
        Self::Stale,
        Self::DynamicOrUnsupported,
        Self::CancelledBeforeStart,
        Self::CancelledAtCheckpoint,
        Self::DeadlineExceeded,
        Self::ResourceLimitAtBoundary,
        Self::CheckedNearOverflow,
        Self::SupersededBeforePublication,
        Self::RepeatedOperationReset,
        Self::BoundedViewComplete,
        Self::IncompleteSemanticNeverBoundedComplete,
        Self::InstrumentFailure,
        Self::ProductFailure,
    ];

    /// Wire (snake_case) spelling used in documents and `terminal:` coverage
    /// tokens.
    pub fn wire_name(self) -> &'static str {
        match self {
            Self::CompleteNonempty => "complete_nonempty",
            Self::CompleteLegitimateEmpty => "complete_legitimate_empty",
            Self::SemanticPartialTypedCeiling => "semantic_partial_typed_ceiling",
            Self::NotReady => "not_ready",
            Self::Stale => "stale",
            Self::DynamicOrUnsupported => "dynamic_or_unsupported",
            Self::CancelledBeforeStart => "cancelled_before_start",
            Self::CancelledAtCheckpoint => "cancelled_at_checkpoint",
            Self::DeadlineExceeded => "deadline_exceeded",
            Self::ResourceLimitAtBoundary => "resource_limit_at_boundary",
            Self::CheckedNearOverflow => "checked_near_overflow",
            Self::SupersededBeforePublication => "superseded_before_publication",
            Self::RepeatedOperationReset => "repeated_operation_reset",
            Self::BoundedViewComplete => "bounded_view_complete",
            Self::IncompleteSemanticNeverBoundedComplete => {
                "incomplete_semantic_never_bounded_complete"
            }
            Self::InstrumentFailure => "instrument_failure",
            Self::ProductFailure => "product_failure",
        }
    }

    /// Terminals naming an exact result with named identity authority.
    pub fn is_exact_result(self) -> bool {
        matches!(
            self,
            Self::CompleteNonempty | Self::CompleteLegitimateEmpty | Self::BoundedViewComplete
        )
    }

    /// Terminals representing non-success attempts.
    pub fn is_non_success(self) -> bool {
        !matches!(self, Self::CompleteNonempty | Self::CompleteLegitimateEmpty)
    }

    /// Terminal outcomes that require recorded instrumentation evidence.
    pub fn requires_instrument_receipt(self) -> bool {
        matches!(
            self,
            Self::CancelledBeforeStart
                | Self::CancelledAtCheckpoint
                | Self::DeadlineExceeded
                | Self::ResourceLimitAtBoundary
                | Self::InstrumentFailure
                | Self::ProductFailure
        )
    }
}

/// Named result identity authority for exact results.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResultIdentity {
    /// Stable result identity string (result id / cache key class).
    pub identity: String,
    /// Completeness authority the identity claims.
    pub completeness: CompletenessClaim,
}

/// Closed completeness-claim vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CompletenessClaim {
    SemanticComplete,
    BoundedViewComplete,
}

/// A pinned independent authority backing one row's proposition. The manifest
/// names the authority bytes; their drift checking stays owned by the
/// consumer proof named in `note`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorityReference {
    /// Repo-relative slash path of the pinned authority file inside one
    /// declared allowed root.
    pub path: String,
    /// Who re-checks these bytes and when (consumer issue).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Support-limitation class and exit owner for unsupported/partial/open-world
/// and terminal boundaries.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct Limitation {
    pub support_class: SupportClass,
    /// Owning issue for unsupported/partial/open-world exits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_owner_issue: Option<u64>,
}

/// Closed support-class vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SupportClass {
    Supported,
    Partial,
    UnsupportedOpenWorld,
    UnsupportedTerminal,
}

impl SupportClass {
    /// Classes that must route their exit to a named owner issue.
    pub fn requires_exit_owner(self) -> bool {
        !matches!(self, Self::Supported)
    }
}

/// Oracle type, independence class and proof ceiling for the row.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct Oracle {
    pub oracle_type: OracleType,
    pub independence_class: IndependenceClass,
    pub proof_ceiling: ProofCeiling,
}

/// Closed oracle-type vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OracleType {
    IndependentExpectedAuthority,
    BoundedRealPerl,
    BoundedProtocolClient,
    ObservedOutputRetainedOnly,
    DeclarationOnly,
}

impl OracleType {
    /// Observed snapshots may be retained but never serve as the expected
    /// oracle of a promoted row.
    pub fn is_implementation_derived(self) -> bool {
        matches!(self, Self::ObservedOutputRetainedOnly)
    }
}

/// Closed independence-class vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub enum IndependenceClass {
    #[serde(rename = "independent")]
    Independent,
    #[serde(rename = "observed_only")]
    ObservedOnly,
    #[serde(rename = "none")]
    None_,
}

/// Closed proof-ceiling vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProofCeiling {
    StaticDeclaration,
    InternalSemantic,
    ExactProcessExternal,
    ExternalCloseout,
}

/// Stable barrier/event position metadata for race rows. This is test
/// orchestration metadata, never product state.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct RaceBarrier {
    pub kind: RaceBarrierKind,
    /// Stable barrier/event position name (no wall-clock sleeps).
    pub position: String,
}

/// Closed race-barrier vocabulary (#10998 deterministic race identities).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RaceBarrierKind {
    HeldTargetContributorGeneration,
    MidGraphAdmissionSccClosureQuery,
    BeforeFinalSemanticPublication,
    BetweenSourceProjections,
    MidPushBatch,
    MidWorkspacePartialStream,
    BeforeCacheResultIdCommit,
    CloseReopenRootRemovalConfigProfileMovement,
}

impl RaceBarrierKind {
    /// Barriers whose rows must declare the exact client-visible/currentness
    /// expectation for the interrupted transport.
    pub fn requires_client_visible_expectation(self) -> bool {
        matches!(self, Self::MidPushBatch | Self::MidWorkspacePartialStream)
    }
}

/// Instrument availability status. Missing instruments become explicit
/// `not_proven` coverage, never inferred zeros.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct InstrumentStatus {
    pub status: InstrumentStatusKind,
    /// Required literal `not_proven` when status is missing.
    pub disposition: String,
}

/// Closed instrument-status vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InstrumentStatusKind {
    Present,
    Missing,
}
