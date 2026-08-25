//! Independent proposition dimensions for compiler profiles.
//!
//! Every distinction required by #12186 is a distinct enum or newtype here so
//! that constructors and validators can reject cross-satisfaction instead of
//! letting values blur:
//!
//! - observed upstream result vs accepted compatibility state vs general
//!   semantic support ([`UpstreamObservation`], [`CompatibilityAcceptance`],
//!   [`SemanticSupportLevel`]);
//! - parser/semantic/PIR fact production vs provider consumption vs edit
//!   authorization ([`ClaimFamily`]);
//! - project/world currentness vs cross-file external behavior
//!   (distinct [`ClaimFamily`] variants);
//! - curated expectation vs real-Perl oracle vs EIR mechanism vs evaluated
//!   work ([`EvidenceClass`]);
//! - source vs exact-process vs packaged vs installed-host vs actual-client
//!   stage ([`ExecutionStage`]);
//! - correctness vs production work vs oracle/cold work vs performance or
//!   resource result ([`ClaimFamily`] plus [`WorkContext`]/[`WorkRequirement`]);
//! - replacement currentness vs old-path absence vs recurrence proof lives in
//!   [`super::requirements::LegacyExitDimension`].

use std::collections::BTreeSet;

use super::CompilerProfileError;
use super::fingerprint::CanonWriter;
use super::fingerprint::CanonicalEncode;

/// The independent semantic proposition family of a row or proof axis.
///
/// Families are deliberately non-comparable: evidence produced for one family
/// never satisfies an axis of another family (see
/// [`class_supports_family`] and per-axis validation).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, PartialOrd, Ord)]
pub enum ClaimFamily {
    /// Parser, semantic analyzer, and PIR internal fact production.
    CompilerInternalFacts,
    /// Provider consumption of compiler facts.
    ProviderConsumption,
    /// Edit authorization behavior on top of consumed facts.
    EditAuthorization,
    /// Workspace/project currentness of the world model.
    ProjectWorldCurrentness,
    /// Cross-file external behavior agreement.
    CrossFileExternalBehavior,
    /// Bounded execution of compiler/provider work.
    ExecutionBoundedness,
    /// Performance or resource results.
    PerformanceResource,
    /// The EIR evaluation mechanism itself.
    EirMechanism,
    /// Legacy path exit.
    LegacyExit,
    /// Test reachability of a surface.
    TestReachability,
}

impl CanonicalEncode for ClaimFamily {
    fn encode(&self, writer: &mut CanonWriter) {
        let tag = match self {
            Self::CompilerInternalFacts => "fam_compiler_internal_facts",
            Self::ProviderConsumption => "fam_provider_consumption",
            Self::EditAuthorization => "fam_edit_authorization",
            Self::ProjectWorldCurrentness => "fam_project_world_currentness",
            Self::CrossFileExternalBehavior => "fam_cross_file_external_behavior",
            Self::ExecutionBoundedness => "fam_execution_boundedness",
            Self::PerformanceResource => "fam_performance_resource",
            Self::EirMechanism => "fam_eir_mechanism",
            Self::LegacyExit => "fam_legacy_exit",
            Self::TestReachability => "fam_test_reachability",
        };
        writer.tag(tag);
    }
}

/// The stage at which a proposition is observed. Ordered from weakest to
/// strongest reachability; a floor check may accept stronger stages, but the
/// stages never collapse into one another because every proof axis names its
/// own exact `(family, stage)` pair.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, PartialOrd, Ord)]
pub enum ExecutionStage {
    /// Source-tree only observation.
    SourceTree,
    /// The exact process under test was observed.
    ExactProcess,
    /// A packaged artifact was exercised.
    PackagedArtifact,
    /// An installed host was exercised.
    InstalledHost,
    /// An actual client drove the behavior end-to-end.
    ActualClient,
}

impl ExecutionStage {
    /// Whether `self` reaches at least the `floor` stage.
    pub fn at_least(self, floor: Self) -> bool {
        self >= floor
    }
}

impl CanonicalEncode for ExecutionStage {
    fn encode(&self, writer: &mut CanonWriter) {
        let tag = match self {
            Self::SourceTree => "stg_source_tree",
            Self::ExactProcess => "stg_exact_process",
            Self::PackagedArtifact => "stg_packaged_artifact",
            Self::InstalledHost => "stg_installed_host",
            Self::ActualClient => "stg_actual_client",
        };
        writer.tag(tag);
    }
}

/// The class of evidence offered for a proof axis.
///
/// The four-way separation demanded by #12186 is encoded by membership in this
/// enum: [`EvidenceClass::CuratedFixtureReplay`] (curated expectation),
/// [`EvidenceClass::RealPerlOracleAgreement`] (real-Perl oracle),
/// [`EvidenceClass::EirMechanismEvaluation`] (EIR mechanism/evaluated work),
/// plus the production/consumption observation classes.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, PartialOrd, Ord)]
pub enum EvidenceClass {
    /// A local lexical pass; the weakest compiler-internal signal.
    LocalLexicalPass,
    /// Parser/semantic/PIR fact production was demonstrated.
    ParserFactProduction,
    /// Replay of a curated fixture expectation.
    CuratedFixtureReplay,
    /// Agreement with real Perl behavior.
    RealPerlOracleAgreement,
    /// Evaluation of the EIR mechanism on evaluated work.
    EirMechanismEvaluation,
    /// Observed provider consumption of compiler facts.
    ProviderBehaviorProbe,
    /// Observed edit authorization decisions.
    EditAuthorizationProbe,
    /// Observed workspace/world currentness behavior.
    WorkspaceWorldObservation,
    /// Observed cross-file external behavior.
    CrossFileBehaviorObservation,
    /// Verification through a packaged/installed/client host.
    InstalledHostVerification,
}

impl CanonicalEncode for EvidenceClass {
    fn encode(&self, writer: &mut CanonWriter) {
        let tag = match self {
            Self::LocalLexicalPass => "evc_local_lexical_pass",
            Self::ParserFactProduction => "evc_parser_fact_production",
            Self::CuratedFixtureReplay => "evc_curated_fixture_replay",
            Self::RealPerlOracleAgreement => "evc_real_perl_oracle_agreement",
            Self::EirMechanismEvaluation => "evc_eir_mechanism_evaluation",
            Self::ProviderBehaviorProbe => "evc_provider_behavior_probe",
            Self::EditAuthorizationProbe => "evc_edit_authorization_probe",
            Self::WorkspaceWorldObservation => "evc_workspace_world_observation",
            Self::CrossFileBehaviorObservation => "evc_cross_file_behavior_observation",
            Self::InstalledHostVerification => "evc_installed_host_verification",
        };
        writer.tag(tag);
    }
}

/// Whether a piece of evidence of `class` may ever back an axis of `family`.
///
/// This matrix is the constructor- and validator-level guard against
/// cross-family satisfaction: parser-family proof cannot satisfy provider,
/// edit, or installed-host axes; fixture replay and oracle agreement cannot
/// satisfy the EIR-mechanism family; only evaluated EIR work can.
pub const fn class_supports_family(class: EvidenceClass, family: ClaimFamily) -> bool {
    matches!(
        (class, family),
        (EvidenceClass::LocalLexicalPass, ClaimFamily::CompilerInternalFacts)
            | (EvidenceClass::ParserFactProduction, ClaimFamily::CompilerInternalFacts)
            | (
                EvidenceClass::CuratedFixtureReplay,
                ClaimFamily::CompilerInternalFacts
                    | ClaimFamily::CrossFileExternalBehavior
                    | ClaimFamily::TestReachability
                    | ClaimFamily::ExecutionBoundedness
            )
            | (
                EvidenceClass::RealPerlOracleAgreement,
                ClaimFamily::CompilerInternalFacts
                    | ClaimFamily::CrossFileExternalBehavior
                    | ClaimFamily::ExecutionBoundedness
            )
            | (EvidenceClass::EirMechanismEvaluation, ClaimFamily::EirMechanism)
            | (EvidenceClass::ProviderBehaviorProbe, ClaimFamily::ProviderConsumption)
            | (EvidenceClass::EditAuthorizationProbe, ClaimFamily::EditAuthorization)
            | (EvidenceClass::WorkspaceWorldObservation, ClaimFamily::ProjectWorldCurrentness)
            | (EvidenceClass::CrossFileBehaviorObservation, ClaimFamily::CrossFileExternalBehavior)
            | (
                EvidenceClass::InstalledHostVerification,
                ClaimFamily::ProviderConsumption
                    | ClaimFamily::EditAuthorization
                    | ClaimFamily::ProjectWorldCurrentness
                    | ClaimFamily::CrossFileExternalBehavior
                    | ClaimFamily::ExecutionBoundedness
            )
    )
}

/// Provenance authority tier of an evidence source, ordered from furthest to
/// closest to maintained repository truth. A spec's tier floor accepts equal or
/// higher-ranked tiers only.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, PartialOrd, Ord)]
pub enum SourceTier {
    /// Unmodified upstream material.
    UpstreamUnmodified,
    /// Vendored copies carried in the tree.
    Vendored,
    /// Locally patched material.
    LocalPatch,
    /// Material owned by this repository.
    RepositoryOwned,
}

impl CanonicalEncode for SourceTier {
    fn encode(&self, writer: &mut CanonWriter) {
        let tag = match self {
            Self::UpstreamUnmodified => "tier_upstream_unmodified",
            Self::Vendored => "tier_vendored",
            Self::LocalPatch => "tier_local_patch",
            Self::RepositoryOwned => "tier_repository_owned",
        };
        writer.tag(tag);
    }
}

/// The area of product surface a subject selector addresses.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, PartialOrd, Ord)]
pub enum SubjectArea {
    /// Concrete syntax constructs.
    SyntaxConstruct,
    /// Diagnostic rules surfaced to users.
    DiagnosticRule,
    /// Provider actions (hover, completion, rename, ...).
    ProviderAction,
    /// Workspace-level scenarios.
    WorkspaceScenario,
    /// Runtime/dynamic boundaries.
    RuntimeBoundary,
    /// Packaging units.
    PackagingUnit,
    /// Documentation surfaces.
    DocumentationSurface,
}

impl CanonicalEncode for SubjectArea {
    fn encode(&self, writer: &mut CanonWriter) {
        let tag = match self {
            Self::SyntaxConstruct => "area_syntax_construct",
            Self::DiagnosticRule => "area_diagnostic_rule",
            Self::ProviderAction => "area_provider_action",
            Self::WorkspaceScenario => "area_workspace_scenario",
            Self::RuntimeBoundary => "area_runtime_boundary",
            Self::PackagingUnit => "area_packaging_unit",
            Self::DocumentationSurface => "area_documentation_surface",
        };
        writer.tag(tag);
    }
}

/// Exact subject selector of a row: an area plus a stable selector token.
/// Rows never address subjects by prose alone.
#[derive(Clone, Debug, Eq, Hash, PartialEq, PartialOrd, Ord)]
pub struct SubjectSelector {
    /// The addressed product area.
    pub area: SubjectArea,
    /// Stable selector token inside that area.
    pub selector: String,
}

impl SubjectSelector {
    /// Validated constructor; the selector must be a stable token.
    pub fn new(area: SubjectArea, selector: &str) -> Result<Self, CompilerProfileError> {
        if !super::is_stable_token(selector) {
            return Err(CompilerProfileError::Structure {
                message: format!("subject selector {selector:?} must match [a-z0-9._-]"),
            });
        }
        Ok(Self { area, selector: selector.to_string() })
    }
}

impl CanonicalEncode for SubjectSelector {
    fn encode(&self, writer: &mut CanonWriter) {
        self.area.encode(writer);
        writer.str_field("sel", &self.selector);
    }
}

/// One concrete proof obligation: an exact claim family at an exact stage.
/// Two axes are equal only when both members match, which is what keeps
/// source/exact-process/packaged/installed-host/client propositions
/// independent.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, PartialOrd, Ord)]
pub struct ProofAxis {
    /// Semantic proposition family.
    pub family: ClaimFamily,
    /// Stage at which the proposition must hold.
    pub stage: ExecutionStage,
}

impl ProofAxis {
    /// Constructor for a `(family, stage)` axis.
    pub const fn new(family: ClaimFamily, stage: ExecutionStage) -> Self {
        Self { family, stage }
    }
}

impl CanonicalEncode for ProofAxis {
    fn encode(&self, writer: &mut CanonWriter) {
        self.family.encode(writer);
        self.stage.encode(writer);
    }
}

/// What kind of work was actually performed to produce evidence.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, PartialOrd, Ord)]
pub enum WorkContext {
    /// Work executed on the production path.
    ProductionPath,
    /// Cold-start work outside the production path.
    ColdStart,
    /// Work executed inside an oracle harness.
    OracleHarness,
}

impl CanonicalEncode for WorkContext {
    fn encode(&self, writer: &mut CanonWriter) {
        let tag = match self {
            Self::ProductionPath => "ctx_production_path",
            Self::ColdStart => "ctx_cold_start",
            Self::OracleHarness => "ctx_oracle_harness",
        };
        writer.tag(tag);
    }
}

/// Work actually performed for one evidence record.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, PartialOrd, Ord)]
pub struct WorkPerformed {
    /// Context the work ran in.
    pub context: WorkContext,
    /// Measured work units (never negative).
    pub units: u32,
}

impl WorkPerformed {
    /// Constructor for performed work in an explicit context.
    pub const fn new(context: WorkContext, units: u32) -> Self {
        Self { context, units }
    }

    /// Zero-unit production-path execution (valid only where no work is
    /// required; validators reject it against any positive minimum).
    pub const fn zero_execution() -> Self {
        Self { context: WorkContext::ProductionPath, units: 0 }
    }
}

impl CanonicalEncode for WorkPerformed {
    fn encode(&self, writer: &mut CanonWriter) {
        self.context.encode(writer);
        writer.u64_field("units", u64::from(self.units));
    }
}

/// The work a proof axis demands before evidence may satisfy it.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, PartialOrd, Ord)]
pub struct WorkRequirement {
    /// Context the required work must have run in.
    pub required_context: WorkContext,
    /// Minimum work units; zero means work is not load-bearing here.
    pub minimum_units: u32,
}

impl WorkRequirement {
    /// No work obligation.
    pub const fn none() -> Self {
        Self { required_context: WorkContext::ProductionPath, minimum_units: 0 }
    }

    /// Require at least `minimum_units` units of work in `context`.
    pub const fn at_least(context: WorkContext, minimum_units: u32) -> Self {
        Self { required_context: context, minimum_units }
    }
}

impl CanonicalEncode for WorkRequirement {
    fn encode(&self, writer: &mut CanonWriter) {
        self.required_context.encode(writer);
        writer.u64_field("min_units", u64::from(self.minimum_units));
    }
}

/// What was actually observed upstream — kept strictly separate from what has
/// been accepted and from how far semantics are supported locally.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, PartialOrd, Ord)]
pub enum UpstreamObservation {
    /// Nothing upstream has been observed yet.
    Unobserved,
    /// Upstream imported without modification.
    ImportedCleanly,
    /// Upstream imported with documented local patches.
    AppliedWithPatches,
    /// Local tree diverged from upstream.
    DivergedFromUpstream,
}

impl CanonicalEncode for UpstreamObservation {
    fn encode(&self, writer: &mut CanonWriter) {
        let tag = match self {
            Self::Unobserved => "obs_unobserved",
            Self::ImportedCleanly => "obs_imported_cleanly",
            Self::AppliedWithPatches => "obs_applied_with_patches",
            Self::DivergedFromUpstream => "obs_diverged_from_upstream",
        };
        writer.tag(tag);
    }
}

/// The compatibility decision recorded for upstream material — distinct from
/// the observation above and from semantic support below.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, PartialOrd, Ord)]
pub enum CompatibilityAcceptance {
    /// Accepted unchanged.
    AcceptedUnchanged,
    /// Accepted with documented deviations.
    AcceptedWithDocumentedDeviations,
    /// Not accepted yet.
    NotYetAccepted,
    /// Rejected for this profile.
    RejectedForThisProfile,
}

impl CanonicalEncode for CompatibilityAcceptance {
    fn encode(&self, writer: &mut CanonWriter) {
        let tag = match self {
            Self::AcceptedUnchanged => "acc_accepted_unchanged",
            Self::AcceptedWithDocumentedDeviations => "acc_accepted_with_deviations",
            Self::NotYetAccepted => "acc_not_yet_accepted",
            Self::RejectedForThisProfile => "acc_rejected_for_this_profile",
        };
        writer.tag(tag);
    }
}

/// How far semantics are supported — the general-support end of this scale may
/// never be claimed from source-locked debt alone (validators enforce the
/// evidence-stage consequence).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, PartialOrd, Ord)]
pub enum SemanticSupportLevel {
    /// Not supported by this profile.
    Unsupported,
    /// Internal facts only; nothing consumes them yet.
    InternalFactsOnly,
    /// Providers consume the facts.
    FactsConsumedByProviders,
    /// General semantic support across consumers and edits.
    GeneralSemanticSupport,
}

impl CanonicalEncode for SemanticSupportLevel {
    fn encode(&self, writer: &mut CanonWriter) {
        let tag = match self {
            Self::Unsupported => "sup_unsupported",
            Self::InternalFactsOnly => "sup_internal_facts_only",
            Self::FactsConsumedByProviders => "sup_facts_consumed_by_providers",
            Self::GeneralSemanticSupport => "sup_general_semantic_support",
        };
        writer.tag(tag);
    }
}

/// The typed support triple carried by every row. The three members are
/// independent dimensions; constructors reject ambiguous strengthenings such
/// as general semantic support over unobserved upstream material.
#[derive(Clone, Debug, Eq, Hash, PartialEq, PartialOrd, Ord)]
pub struct SupportClaim {
    /// What was observed upstream.
    pub observed_upstream: UpstreamObservation,
    /// What compatibility state has been accepted.
    pub accepted_compatibility: CompatibilityAcceptance,
    /// How far semantics are supported.
    pub semantic_support: SemanticSupportLevel,
}

impl SupportClaim {
    /// Validated constructor rejecting ambiguous strengthening combinations.
    pub fn new(
        observed_upstream: UpstreamObservation,
        accepted_compatibility: CompatibilityAcceptance,
        semantic_support: SemanticSupportLevel,
    ) -> Result<Self, CompilerProfileError> {
        let claim = Self { observed_upstream, accepted_compatibility, semantic_support };
        claim.check()?;
        Ok(claim)
    }

    /// Re-checks an existing claim; used when rows are mutated so that
    /// ambiguous strengthening cannot survive into a validated profile.
    /// Error payloads carry an empty row id until a row binds itself.
    pub(crate) fn check(&self) -> Result<(), CompilerProfileError> {
        if self.semantic_support == SemanticSupportLevel::GeneralSemanticSupport {
            let accepted_ok = matches!(
                self.accepted_compatibility,
                CompatibilityAcceptance::AcceptedUnchanged
                    | CompatibilityAcceptance::AcceptedWithDocumentedDeviations
            );
            if !accepted_ok || self.observed_upstream == UpstreamObservation::Unobserved {
                return Err(CompilerProfileError::SupportOverstatement {
                    row: String::new(),
                    detail: "general semantic support requires an accepted compatibility \
                             state and an observed upstream result"
                        .to_string(),
                });
            }
        }
        if self.semantic_support == SemanticSupportLevel::Unsupported
            && (self.observed_upstream != UpstreamObservation::Unobserved
                || self.accepted_compatibility != CompatibilityAcceptance::NotYetAccepted)
        {
            return Err(CompilerProfileError::DispositionConflict {
                row: String::new(),
                detail: "an unsupported semantic claim cannot carry observed upstream \
                         material or an acceptance decision"
                    .to_string(),
            });
        }
        Ok(())
    }
}

impl CanonicalEncode for SupportClaim {
    fn encode(&self, writer: &mut CanonWriter) {
        self.observed_upstream.encode(writer);
        self.accepted_compatibility.encode(writer);
        self.semantic_support.encode(writer);
    }
}

/// Collects the canonical encoding helpers shared by dimension types.
pub(crate) fn encode_set<T: CanonicalEncode>(values: &BTreeSet<T>, writer: &mut CanonWriter) {
    for value in values {
        value.encode(writer);
    }
}
