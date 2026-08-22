//! Typed representations for the close-proof issue contract and close packet
//! schemas (`issue_contract.v1`, `issue_close_proof.v1`).
//!
//! Enumeration membership and the proof-level ordering follow the issue text of
//! #10380 exactly; semantic completion evaluation remains owned by CP03 (#10382).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub const ISSUE_CONTRACT_SCHEMA_V1: &str = "issue_contract.v1";
pub const CLOSE_PACKET_SCHEMA_V1: &str = "issue_close_proof.v1";

/// What class of proposition an issue owns.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueKind {
    Leaf,
    PhaseLeaf,
    MultiPhase,
    Scaffold,
    Cohort,
    Migration,
    Controller,
    Installed,
    Policy,
    Activation,
}

/// The claim/proof strength an issue requires or a disposition establishes.
///
/// The satisfaction order is the enumeration order from #10380:
/// `representation < mechanism < connected_route < authorized_behavior <
/// cohort < controller_fan_in < installed < public`. A lower level never
/// satisfies a higher required level.
#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProofLevel {
    Representation,
    Mechanism,
    ConnectedRoute,
    AuthorizedBehavior,
    Cohort,
    ControllerFanIn,
    Installed,
    Public,
}

impl ProofLevel {
    pub const fn rank(self) -> u8 {
        match self {
            Self::Representation => 0,
            Self::Mechanism => 1,
            Self::ConnectedRoute => 2,
            Self::AuthorizedBehavior => 3,
            Self::Cohort => 4,
            Self::ControllerFanIn => 5,
            Self::Installed => 6,
            Self::Public => 7,
        }
    }

    /// Whether evidence established at `self` can satisfy a row requiring
    /// `required`. A proof level cannot satisfy a stronger required level.
    pub const fn satisfies(self, required: Self) -> bool {
        self.rank() >= required.rank()
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Representation => "representation",
            Self::Mechanism => "mechanism",
            Self::ConnectedRoute => "connected_route",
            Self::AuthorizedBehavior => "authorized_behavior",
            Self::Cohort => "cohort",
            Self::ControllerFanIn => "controller_fan_in",
            Self::Installed => "installed",
            Self::Public => "public",
        }
    }
}

/// The close relation a packet requests between its PR and its issue.
#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloseMode {
    Completed,
    PhaseCompleteIssueRemainsOpen,
    ControllerComplete,
    Superseded,
    TrueDuplicate,
    NotPlanned,
}

/// Repository-scoped issue identity.
#[derive(Clone, Debug, Eq, PartialEq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IssueRef {
    pub repository: String,
    pub number: u64,
}

/// Stable identity of an accepted ruling that constrains the denominator.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RulingIdentity {
    pub identity: String,
    pub digest: String,
}

/// Current-movement identity of a contract. Packets bind against this; any
/// movement invalidates previously generated packets.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContractIdentity {
    pub issue_body_digest: String,
    pub denominator_digest: String,
    pub accepted_ruling: Option<RulingIdentity>,
}

/// Whether and under which conditions denominator rows may be transferred to
/// another open owner.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransferPolicy {
    pub permitted: bool,
    #[serde(default)]
    pub conditions: Vec<String>,
}

/// One stable denominator row. Row IDs own the denominator, never checkbox
/// position or wording.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DenominatorRow {
    pub row_id: String,
    pub statement: String,
    pub required_proof_level: ProofLevel,
}

/// A negative control guarding one denominator row.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NegativeControlRow {
    pub control_id: String,
    pub guards_row_id: String,
    pub description: String,
}

/// Opaque reference to domain-owner evidence. The close-proof schema records
/// the reference; it never reinterprets domain semantics.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceRef {
    pub producer: String,
    pub subject: String,
    pub content_digest: String,
    pub reference: String,
}

/// Transfer of one denominator row to another open owner.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "disposition", rename_all = "snake_case")]
pub enum RowDispositionValue {
    ProvenCurrentMain {
        evidence: EvidenceRef,
    },
    NotApplicableByReviewedRuling {
        ruling_ref: String,
    },
    TransferredToOpenOwner {
        proposition: String,
        destination_repository: String,
        destination_issue: u64,
        destination_contract_identity: String,
        rationale: String,
    },
    RemovedSurfaceWithProof {
        proof: EvidenceRef,
    },
    NotProven {
        reason: String,
    },
    Contradicted {
        reason: String,
    },
    Stale {
        reason: String,
    },
}

impl RowDispositionValue {
    /// Whether the disposition can satisfy a `completed` denominator. Missing,
    /// stale, contradicted, unknown, or `not_proven` rows cannot.
    pub fn satisfies_completion(self) -> bool {
        matches!(
            self,
            Self::ProvenCurrentMain { .. }
                | Self::NotApplicableByReviewedRuling { .. }
                | Self::TransferredToOpenOwner { .. }
                | Self::RemovedSurfaceWithProof { .. }
        )
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::ProvenCurrentMain { .. } => "proven_current_main",
            Self::NotApplicableByReviewedRuling { .. } => "not_applicable_by_reviewed_ruling",
            Self::TransferredToOpenOwner { .. } => "transferred_to_open_owner",
            Self::RemovedSurfaceWithProof { .. } => "removed_surface_with_proof",
            Self::NotProven { .. } => "not_proven",
            Self::Contradicted { .. } => "contradicted",
            Self::Stale { .. } => "stale",
        }
    }
}

/// Disposition of one negative control at packet generation time.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ControlOutcome {
    Verified,
    Failed { reason: String },
    NotProven { reason: String },
}

/// Disposition of one mandatory child issue.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ChildState {
    ClosedByPacket {
        packet_subject: String,
    },
    StillOpen,
    TransferredToOpenOwner {
        proposition: String,
        destination_repository: String,
        destination_issue: u64,
        destination_contract_identity: String,
        rationale: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChildDispositionRecord {
    pub child: IssueRef,
    pub state: ChildState,
}

/// A bounded claim statement tying prose back to stable row IDs.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimStatement {
    pub statement: String,
    #[serde(default)]
    pub covers_rows: Vec<String>,
}

/// The binding a packet records against the contract identity it was
/// generated from.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PacketBinding {
    pub contract_issue_body_digest: String,
    pub contract_denominator_digest: String,
    pub accepted_ruling_digest: Option<String>,
}

/// Independent outcome surfaces. A PR-scope pass and an issue-close failure may
/// be recorded simultaneously; no cross-constraint forces them to agree.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrScopeOutcome {
    Pass,
    Fail,
    NotProven,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueCloseOutcome {
    Valid,
    Invalid,
    StaleContract,
    NotProven,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CloseVerdict {
    pub pr_scope: PrScopeOutcome,
    pub issue_close: IssueCloseOutcome,
    pub reasons: Vec<String>,
}

/// Duplicate target for a `true_duplicate` close request.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DuplicateRef {
    pub repository: String,
    pub number: u64,
}

/// Versioned close packet (`issue_close_proof.v1`).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClosePacket {
    pub schema_version: String,
    pub repository: String,
    pub issue_number: u64,
    pub requested_close_mode: CloseMode,
    pub contract_binding: PacketBinding,
    #[serde(default)]
    pub candidate_pr: Option<u64>,
    #[serde(default)]
    pub landed_subjects: Vec<String>,
    #[serde(default)]
    pub landing_content_proof: Vec<EvidenceRef>,
    #[serde(default)]
    pub established_claims: Vec<ClaimStatement>,
    #[serde(default)]
    pub explicitly_not_established_claims: Vec<ClaimStatement>,
    #[serde(default)]
    pub row_dispositions: BTreeMap<String, RowDispositionValue>,
    #[serde(default)]
    pub negative_control_dispositions: BTreeMap<String, ControlOutcome>,
    #[serde(default)]
    pub child_dispositions: Vec<ChildDispositionRecord>,
    #[serde(default)]
    pub duplicate_of: Option<DuplicateRef>,
    pub verdict: CloseVerdict,
}
