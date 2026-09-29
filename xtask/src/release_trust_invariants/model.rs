//! Typed `release_trust_invariants.v1` model (#9392).

use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: &str = "release_trust_invariants.v1";
pub const REGISTRY_NAME: &str = "release-trust-invariants";
pub const REGISTRY_PATH: &str = "policy/release-trust-invariants.v1.json";
pub const SCHEMA_PATH: &str = "schemas/release_trust_invariants.v1.schema.json";
pub const STATUS_PATH: &str = "docs/project/status/release_trust_invariants.md";
pub const ISSUE: u32 = 9392;
pub const PARENT_ISSUE: u32 = 8507;

/// Closed producer-kind vocabulary. Execution of these producers is out of scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProducerKind {
    ProviderDecisionReceipt,
    ProviderEditReceipt,
    PublicBetaExperienceFanIn,
    FirstTenMinutesReceipt,
    InstallTransitionReceipt,
    InstalledPublicBetaReceipt,
    ProcessLifecycleReceipt,
    FullDocumentSyncReceipt,
    ReleaseIntegrityCloseout,
    NoPublishSideEffectsReceipt,
    ManifestReachabilityReceipt,
    PublicClaimCatalogReceipt,
}

impl ProducerKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ProviderDecisionReceipt => "provider_decision_receipt",
            Self::ProviderEditReceipt => "provider_edit_receipt",
            Self::PublicBetaExperienceFanIn => "public_beta_experience_fan_in",
            Self::FirstTenMinutesReceipt => "first_ten_minutes_receipt",
            Self::InstallTransitionReceipt => "install_transition_receipt",
            Self::InstalledPublicBetaReceipt => "installed_public_beta_receipt",
            Self::ProcessLifecycleReceipt => "process_lifecycle_receipt",
            Self::FullDocumentSyncReceipt => "full_document_sync_receipt",
            Self::ReleaseIntegrityCloseout => "release_integrity_closeout",
            Self::NoPublishSideEffectsReceipt => "no_publish_side_effects_receipt",
            Self::ManifestReachabilityReceipt => "manifest_reachability_receipt",
            Self::PublicClaimCatalogReceipt => "public_claim_catalog_receipt",
        }
    }
}

/// Closed release-claim family vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseClaim {
    ProviderExactness,
    ProviderEditSafety,
    AggregationHonesty,
    StartupHonesty,
    InstallIntegrity,
    ArtifactIdentity,
    LifecycleCleanup,
    TextSynchronization,
    ReleaseIntegrity,
    PublicationAuthority,
    InstalledReachability,
    PublicClaimTruth,
}

impl ReleaseClaim {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ProviderExactness => "provider_exactness",
            Self::ProviderEditSafety => "provider_edit_safety",
            Self::AggregationHonesty => "aggregation_honesty",
            Self::StartupHonesty => "startup_honesty",
            Self::InstallIntegrity => "install_integrity",
            Self::ArtifactIdentity => "artifact_identity",
            Self::LifecycleCleanup => "lifecycle_cleanup",
            Self::TextSynchronization => "text_synchronization",
            Self::ReleaseIntegrity => "release_integrity",
            Self::PublicationAuthority => "publication_authority",
            Self::InstalledReachability => "installed_reachability",
            Self::PublicClaimTruth => "public_claim_truth",
        }
    }
}

/// Whether a subject identity field is required for the row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentityRequirement {
    Required,
    NotApplicable,
}

impl IdentityRequirement {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Required => "required",
            Self::NotApplicable => "not_applicable",
        }
    }
}

/// Terminal input semantics for one observation class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalSemantics {
    Count,
    Blocks,
}

impl TerminalSemantics {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Count => "count",
            Self::Blocks => "blocks",
        }
    }
}

/// Currentness of a named owner or producer authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorityStatus {
    Current,
    Superseded,
}

impl AuthorityStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::Superseded => "superseded",
        }
    }
}

/// Closed applicability platform vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    LinuxX64,
    WindowsX64,
    MacosBounded,
    TopologySelected,
}

impl Platform {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LinuxX64 => "linux_x64",
            Self::WindowsX64 => "windows_x64",
            Self::MacosBounded => "macos_bounded",
            Self::TopologySelected => "topology_selected",
        }
    }
}

/// Closed applicability profile vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Profile {
    PublicBeta,
    InstalledJourney,
    CandidatePacket,
    PublicationRehearsal,
    FirstTenMinutes,
}

impl Profile {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PublicBeta => "public_beta",
            Self::InstalledJourney => "installed_journey",
            Self::CandidatePacket => "candidate_packet",
            Self::PublicationRehearsal => "publication_rehearsal",
            Self::FirstTenMinutes => "first_ten_minutes",
        }
    }
}

/// One GitHub issue that may own a producer or invariant row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnerAuthority {
    pub issue: u32,
    pub status: AuthorityStatus,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub successor: Option<u32>,
}

/// One named producer kind and its currentness.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProducerAuthority {
    pub producer_kind: ProducerKind,
    pub status: AuthorityStatus,
    pub owner_issue: u32,
    pub command_or_workflow: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub successor: Option<ProducerKind>,
}

/// Named falsifier ID. Execution is owned by a later issue; this registry only names them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NegativeControl {
    pub id: String,
    pub owner_issue: u32,
    pub description: String,
}

/// Required subject identities for one invariant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubjectIdentity {
    pub source_sha: IdentityRequirement,
    pub candidate_digest: IdentityRequirement,
    pub artifact_hashes: IdentityRequirement,
}

/// Denominator authority and completeness for one invariant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Denominator {
    pub authority: String,
    pub completeness_rule: String,
}

/// Where the invariant applies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Applicability {
    pub platforms: Vec<Platform>,
    pub profiles: Vec<Profile>,
    pub supported_envelope_ref: u32,
}

/// Terminal input semantics. Missing/stale/skipped evidence is `not_proven` and blocks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerminalInputStates {
    pub success: TerminalSemantics,
    pub failure: TerminalSemantics,
    pub not_proven: TerminalSemantics,
}

/// One mandatory trust-invariant row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InvariantRow {
    pub invariant_id: String,
    pub release_claim: ReleaseClaim,
    pub owner_issue: u32,
    pub producer_kind: ProducerKind,
    pub subject_identity: SubjectIdentity,
    pub denominator: Denominator,
    pub applicability: Applicability,
    pub terminal_input_states: TerminalInputStates,
    pub negative_control_ids: Vec<String>,
    pub release_consumers: Vec<u32>,
    pub claim_boundary: String,
}

/// Controller-named mandatory ID set that the registry must cover.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControllerRequirement {
    pub controller_issue: u32,
    pub mandatory_invariant_ids: Vec<String>,
}

/// Versioned registry document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrustInvariantRegistry {
    #[serde(rename = "$schema", default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    pub schema_version: String,
    pub registry: String,
    pub issue: u32,
    pub parent_issue: u32,
    pub owner: String,
    pub updated: String,
    pub claim_boundary: String,
    pub owner_authorities: Vec<OwnerAuthority>,
    pub producer_authorities: Vec<ProducerAuthority>,
    pub negative_control_catalog: Vec<NegativeControl>,
    pub controller_requirements: Vec<ControllerRequirement>,
    pub invariants: Vec<InvariantRow>,
}
