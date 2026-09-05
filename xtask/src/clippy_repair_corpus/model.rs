//! Serde model for the repair-falsifier corpus. Unknown fields fail closed so
//! provider/model-specific projections cannot drift case semantics silently
//! (#11649 falsifier 11).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub(crate) const SCHEMA_VERSION: u32 = 1;
pub(crate) const CORPUS_NAME: &str = "ClippyRepairFalsifierCorpusV1";

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) enum CorpusFamily {
    #[serde(rename = "verifier_denominator_weakening")]
    VerifierDenominatorWeakening,
    #[serde(rename = "count_identity_debt_substitution")]
    CountIdentityDebtSubstitution,
    #[serde(rename = "lost_work_error_obligation_theater")]
    LostWorkErrorObligationTheater,
    #[serde(rename = "boundary_data_semantics_theater")]
    BoundaryDataSemanticsTheater,
    #[serde(rename = "structural_ownership_theater")]
    StructuralOwnershipTheater,
    #[serde(rename = "real_regression_10600_family")]
    RealRegression10600Family,
    #[serde(rename = "documentation_cargo_test_proof_theater")]
    DocumentationCargoTestProofTheater,
}

impl CorpusFamily {
    /// The stable case-ID letter prefix that must agree with the family.
    pub(crate) fn id_prefix(self) -> char {
        match self {
            Self::VerifierDenominatorWeakening => 'A',
            Self::CountIdentityDebtSubstitution => 'B',
            Self::LostWorkErrorObligationTheater => 'C',
            Self::BoundaryDataSemanticsTheater => 'D',
            Self::StructuralOwnershipTheater => 'E',
            Self::RealRegression10600Family => 'F',
            Self::DocumentationCargoTestProofTheater => 'G',
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum PacketClass {
    Qualification,
    Foundation,
    Migration,
    Activation,
    Review,
}

/// Closed reason-code vocabulary. One code per frozen failure mechanism; new
/// codes require a reviewed corpus delta naming the owning issue.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) enum ReasonCode {
    #[serde(rename = "alias_only_complexity_hiding")]
    AliasOnlyComplexityHiding,
    #[serde(rename = "api_shape_compliance_change")]
    ApiShapeComplianceChange,
    #[serde(rename = "ascii_only_unicode_proof")]
    AsciiOnlyUnicodeProof,
    #[serde(rename = "atomic_mutex_substitution_unproved")]
    AtomicMutexSubstitutionUnproved,
    #[serde(rename = "await_structure_change_unproofed")]
    AwaitStructureChangeUnproofed,
    #[serde(rename = "baseline_absorption_refresh")]
    BaselineAbsorptionRefresh,
    #[serde(rename = "broad_suppression_carveout")]
    BroadSuppressionCarveout,
    #[serde(rename = "cargo_surface_compliance_change")]
    CargoSurfaceComplianceChange,
    #[serde(rename = "command_level_broad_allow")]
    CommandLevelBroadAllow,
    #[serde(rename = "consumed_identity_resurrection")]
    ConsumedIdentityResurrection,
    #[serde(rename = "dead_helper_deletion_lib_only_evidence")]
    DeadHelperDeletionLibOnlyEvidence,
    #[serde(rename = "feature_gated_import_deletion")]
    FeatureGatedImportDeletion,
    #[serde(rename = "generator_stale_edit")]
    GeneratorStaleEdit,
    #[serde(rename = "get_unwrap_indexing_theater")]
    GetUnwrapIndexingTheater,
    #[serde(rename = "group_lint_substitution")]
    GroupLintSubstitution,
    #[serde(rename = "incompatible_dependency_upgrade")]
    IncompatibleDependencyUpgrade,
    #[serde(rename = "invariant_free_accessor")]
    InvariantFreeAccessor,
    #[serde(rename = "invented_guarantee_documentation")]
    InventedGuaranteeDocumentation,
    #[serde(rename = "log_only_error_consumption")]
    LogOnlyErrorConsumption,
    #[serde(rename = "machine_applicable_authority_crossing")]
    MachineApplicableAuthorityCrossing,
    #[serde(rename = "malformed_auto_mutation")]
    MalformedAutoMutation,
    #[serde(rename = "module_carveout_debt_absorption")]
    ModuleCarveoutDebtAbsorption,
    #[serde(rename = "must_use_discard_theater")]
    MustUseDiscardTheater,
    #[serde(rename = "non_proof_rendered_success")]
    NonProofRenderedSuccess,
    #[serde(rename = "numeric_semantics_change")]
    NumericSemanticsChange,
    #[serde(rename = "open_world_cleanup_theater")]
    OpenWorldCleanupTheater,
    #[serde(rename = "ownership_satisfying_clone")]
    OwnershipSatisfyingClone,
    #[serde(rename = "panic_assertion_substitution")]
    PanicAssertionSubstitution,
    #[serde(rename = "parameter_bag_no_owner")]
    ParameterBagNoOwner,
    #[serde(rename = "platform_evidence_substitution")]
    PlatformEvidenceSubstitution,
    #[serde(rename = "private_evidence_substitution")]
    PrivateEvidenceSubstitution,
    #[serde(rename = "protocol_boundary_weakening")]
    ProtocolBoundaryWeakening,
    #[serde(rename = "range_semantics_default_substitution")]
    RangeSemanticsDefaultSubstitution,
    #[serde(rename = "re_export_deletion_by_suggestion")]
    ReExportDeletionBySuggestion,
    #[serde(rename = "renamed_but_semantically_ignored")]
    RenamedButSemanticallyIgnored,
    #[serde(rename = "required_target_omission")]
    RequiredTargetOmission,
    #[serde(rename = "restating_documentation")]
    RestatingDocumentation,
    #[serde(rename = "result_erasure_ok_discard")]
    ResultErasureOkDiscard,
    #[serde(rename = "same_count_substitution")]
    SameCountSubstitution,
    #[serde(rename = "stale_receipt_acceptance")]
    StaleReceiptAcceptance,
    #[serde(rename = "subject_profile_reduction")]
    SubjectProfileReduction,
    #[serde(rename = "suppression_copy_expansion")]
    SuppressionCopyExpansion,
    #[serde(rename = "suppression_displacement")]
    SuppressionDisplacement,
    #[serde(rename = "test_cfg_attr_general_carveout")]
    TestCfgAttrGeneralCarveout,
    #[serde(rename = "trampoline_splitting")]
    TrampolineSplitting,
    #[serde(rename = "unbounded_auto_fix")]
    UnboundedAutoFix,
    #[serde(rename = "unchecked_byte_slicing")]
    UncheckedByteSlicing,
    #[serde(rename = "uncontracted_deliberate_discard")]
    UncontractedDeliberateDiscard,
    #[serde(rename = "unsafe_boundary_widening")]
    UnsafeBoundaryWidening,
    #[serde(rename = "weakened_test_proof")]
    WeakenedTestProof,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum SupportedResultClass {
    Reject,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SubjectIdentity {
    pub(crate) scope: String,
    pub(crate) target_kinds: Vec<String>,
    pub(crate) profile: String,
    pub(crate) platforms: Vec<String>,
    pub(crate) features: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GovernedLintRef {
    /// Toolchain-qualified lint path, e.g. `clippy::unwrap_used`.
    pub(crate) lint: String,
    pub(crate) level_source: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DishonestMutation {
    pub(crate) kind: String,
    pub(crate) language: String,
    /// Substrings that must appear in the mutation bytes; the discrimination proof.
    pub(crate) trigger_evidence: Vec<String>,
    pub(crate) content: String,
    pub(crate) sha256: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum AuthorityKind {
    CargoLints,
    FileContract,
    GateCommand,
    ReceiptContract,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum AuthorityStatus {
    Bound { authority_kind: AuthorityKind, reference: String },
    PendingOwner { owner_issue: u32, unresolved_reason: String },
}

impl AuthorityStatus {
    pub(crate) fn is_bound(&self) -> bool {
        matches!(self, Self::Bound { .. })
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExpectedResult {
    pub(crate) result_class: SupportedResultClass,
    pub(crate) reason_code: ReasonCode,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PositiveCounterpart {
    /// Substrings that must appear in the honest alternative and must not
    /// appear in the dishonest mutation (control leak check).
    pub(crate) sanctioned_evidence: Vec<String>,
    pub(crate) note: String,
    pub(crate) content: String,
    pub(crate) sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CaseApplicability {
    pub(crate) packet_classes: Vec<PacketClass>,
    pub(crate) domains: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CaseDeltaProvenance {
    pub(crate) change_reason: String,
    pub(crate) owning_issue: u32,
    pub(crate) historical_interpretability: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RepairFalsifierCase {
    pub(crate) schema_version: u32,
    pub(crate) case_id: String,
    pub(crate) family: CorpusFamily,
    pub(crate) purpose: String,
    pub(crate) incident_reference: Option<String>,
    pub(crate) subject: SubjectIdentity,
    pub(crate) governed_lint: Option<GovernedLintRef>,
    pub(crate) dishonest_mutation: DishonestMutation,
    pub(crate) rejecting_authority: AuthorityStatus,
    pub(crate) expected_result: ExpectedResult,
    pub(crate) positive_counterpart: PositiveCounterpart,
    pub(crate) applicability: CaseApplicability,
    pub(crate) delta_provenance: CaseDeltaProvenance,
    pub(crate) claim_boundary: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ManifestCaseEntry {
    pub(crate) case_id: String,
    pub(crate) family: CorpusFamily,
    pub(crate) file: String,
    pub(crate) packet_ready: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CorpusManifest {
    pub(crate) schema_version: u32,
    pub(crate) corpus: String,
    pub(crate) producer_toolchain: String,
    pub(crate) case_count: usize,
    pub(crate) cases: Vec<ManifestCaseEntry>,
}

/// Fully loaded corpus used by validation and tests: case_id -> parsed case.
pub(crate) type LoadedCases = BTreeMap<String, RepairFalsifierCase>;
