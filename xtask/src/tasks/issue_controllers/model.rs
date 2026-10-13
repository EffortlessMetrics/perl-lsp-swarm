//! Typed model of the stable `issue_controller_train.v1` manifest.
//!
//! Every level uses `deny_unknown_fields` so a malformed or extended manifest
//! fails closed as a schema/serialization defect instead of being normalized
//! into something the validator silently accepts. Non-integer JSON numbers are
//! rejected by the integer field types; the digest walk additionally refuses
//! floating-point values anywhere in the document.

use serde::{Deserialize, Serialize};

pub const SCHEMA_NAME: &str = "issue_controller_train.v1";
pub const SCHEMA_VERSION: u64 = 1;
pub const HOME_PROGRAMME: &str = "issue-controllers";
pub const WRITER_NAMESPACE: &str = "issue_controllers.";

/// Dependency classes of the typed edge vocabulary (#10858).
pub const DEP_CLASSES: [&str; 4] = ["hard", "evidence", "optional", "external"];

/// Spec dispositions of the checked projection contract.
pub const DISPOSITIONS: [&str; 8] = [
    "SPEC_COMPILED",
    "EXISTING_CONTRACT_SUFFICIENT",
    "ISSUE_PLAN_SUFFICIENT",
    "CONTROLLER_NO_CODING_SPEC",
    "FAN_IN_OR_CERTIFICATION_SPEC",
    "EXTERNAL_OR_MANUAL_NO_CODING_SPEC",
    "RETURN_TO_ISSUE",
    "NOT_PROVEN",
];

/// The nine authority planes in canonical order (S00 `#11763`).
pub const AUTHORITY_PLANES: [&str; 9] = [
    "stable train contract",
    "semantic train revision",
    "current-tree implementation state",
    "offline readiness/frontier",
    "exact-tree context",
    "live collaboration/candidate state",
    "exact-head proof/review closeout",
    "live GitHub metadata state",
    "behavior/proof/support/external truth",
];

/// The fifteen train-execution roles in canonical order. These are train
/// roles, kept distinct from any issue-plane role vocabulary.
pub const TRAIN_ROLES: [&str; 15] = [
    "controller",
    "specification",
    "stable_contract",
    "validator",
    "current_tree_probe",
    "offline_frontier",
    "context_projection",
    "live_observer",
    "packet_adapter",
    "implementation",
    "proof",
    "fan_in",
    "integration",
    "external_gate",
    "dogfood",
];

/// The frozen node/issue map of the stable graph (T01 `#11764`).
pub const EXPECTED_NODES: [(&str, u64); 26] = [
    ("C01", 11682),
    ("C02", 11683),
    ("C03", 11684),
    ("C04", 11685),
    ("C05", 11686),
    ("C06", 11687),
    ("CTRL", 11681),
    ("D01", 11781),
    ("D02", 11782),
    ("I01", 11777),
    ("I02", 11778),
    ("P01", 11779),
    ("P02", 11783),
    ("R05B", 11785),
    ("S00", 11763),
    ("T01", 11764),
    ("T02", 11765),
    ("T02R", 11767),
    ("T02S", 11774),
    ("T03", 11769),
    ("T04", 11771),
    ("T05", 11772),
    ("T06", 11773),
    ("T07", 11775),
    ("T08", 11776),
    ("T08C", 11784),
];

/// Graph-law edges frozen with their exact classes (T01 `#11764`).
/// `(source, target, class)` — `target` depends on `source`.
pub const LAW_EDGES: &[(&str, &str, &str)] = &[
    ("S00", "T01", "hard"),
    ("T01", "T02", "hard"),
    ("T02", "T02R", "hard"),
    ("T02R", "T03", "hard"),
    ("T02R", "T02S", "hard"),
    ("T02S", "T04", "evidence"),
    ("T03", "T04", "hard"),
    ("T04", "T05", "hard"),
    ("T04", "T06", "hard"),
    ("T04", "T08C", "hard"),
    ("T05", "T07", "hard"),
    ("T06", "T07", "hard"),
    ("T07", "T08", "hard"),
    ("T08C", "T08", "hard"),
    ("C01", "C02", "hard"),
    ("C01", "C03", "hard"),
    ("C02", "C04", "hard"),
    ("C03", "C04", "hard"),
    ("C04", "C05", "hard"),
    ("T02R", "C05", "hard"),
    ("C05", "C06", "hard"),
    ("C05", "R05B", "hard"),
    ("C04", "I01", "hard"),
    ("T07", "I01", "hard"),
    ("T08", "I01", "hard"),
    ("I01", "I02", "hard"),
    ("C01", "P01", "hard"),
    ("C02", "P01", "hard"),
    ("C03", "P01", "hard"),
    ("C04", "P01", "hard"),
    ("C05", "P01", "hard"),
    ("C06", "P01", "hard"),
    ("I01", "P01", "hard"),
    ("I02", "P01", "hard"),
    ("T08", "P01", "hard"),
    ("P01", "D01", "hard"),
    ("D01", "D02", "hard"),
    ("D02", "P02", "hard"),
    ("C01", "P02", "hard"),
    ("C02", "P02", "hard"),
    ("C03", "P02", "hard"),
    ("C04", "P02", "hard"),
    ("C05", "P02", "hard"),
    ("C06", "P02", "hard"),
    ("I01", "P02", "hard"),
    ("I02", "P02", "hard"),
    ("P01", "P02", "hard"),
    ("D01", "P02", "hard"),
    ("T08", "P02", "hard"),
];

/// Required external authorities that the registry must carry.
pub const REQUIRED_AUTHORITIES: [&str; 11] = [
    "#10858",
    "#10872",
    "#10881",
    "#10554",
    "#11114",
    "#3983",
    "#3949",
    "#4177",
    "#3982",
    "#3957",
    "#EXPLICIT-AUTHORIZATION",
];

/// Open decisions routed to their owning nodes, in canonical order.
pub const OPEN_DECISION_OWNERS: [(&str, &str, u64); 5] = [
    ("OD1", "C02", 11683),
    ("OD2", "R05B", 11785),
    ("OD3", "C03", 11684),
    ("OD4", "I01", 11777),
    ("OD5", "T08C", 11784),
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema: String,
    pub schema_version: u64,
    pub programme: Programme,
    pub authority_planes: Vec<AuthorityPlane>,
    pub train_role_vocabulary: Vec<RoleVocabulary>,
    pub evidence_semantics: EvidenceSemantics,
    pub external_authorities: Vec<ExternalAuthority>,
    pub open_decisions_routed_elsewhere: Vec<OpenDecision>,
    pub nodes: Vec<TrainNode>,
    #[serde(default)]
    pub supersessions: Vec<Supersession>,
    pub revision_governance: RevisionGovernance,
    pub limitations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Programme {
    pub controller_issue: u64,
    pub home_programme: String,
    pub durable_architecture_issue: u64,
    pub durable_architecture_bundle: String,
    pub method_authority: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorityPlane {
    pub plane: String,
    pub owns: String,
    pub never_substitutes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoleVocabulary {
    pub role: String,
    pub owns: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceSemantics {
    pub not_proven_law: String,
    pub optional_visibility: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalAuthority {
    pub id: String,
    pub subject: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenDecision {
    pub id: String,
    pub subject: String,
    pub owning_node: String,
    pub owning_issue: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RevisionGovernance {
    pub owner_node: String,
    pub owner_issue: u64,
    pub invalidates: String,
    pub never: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Supersession {
    pub superseded_node: String,
    pub successor_issue: u64,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrainNode {
    pub node_id: String,
    pub issue: u64,
    pub title: String,
    pub title_fingerprint: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub train_role: String,
    pub lane: String,
    pub chain: Chain,
    pub one_pr_outcome: String,
    pub authority_before: String,
    pub authority_after: String,
    pub buildable: bool,
    #[serde(default)]
    pub dependencies: Vec<Dependency>,
    pub claim_ceiling: String,
    pub writer: Writer,
    pub consumed_authorities: Vec<String>,
    pub allowed_components: Vec<String>,
    pub forbidden_adjacent_owners: Vec<String>,
    pub spec: Spec,
    pub first_falsifier: String,
    pub controls: Controls,
    pub proof: Proof,
    pub review_forward: ReviewForward,
    pub obligations: Obligations,
    pub exits: Exits,
    pub rollback: Rollback,
    #[serde(default)]
    pub successors: Vec<String>,
    pub identity_fields: Vec<String>,
    #[serde(default)]
    pub limitations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Chain {
    pub home: String,
    pub controller: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dependency {
    pub target: String,
    pub class: String,
    pub provenance: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Writer {
    pub conflict_key: String,
    pub parallel_group: String,
    pub stack_relation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec {
    pub disposition: String,
    pub owner: String,
    pub stale_policy: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Controls {
    pub positive: String,
    pub opposite: String,
    pub stale: String,
    pub wrong_subject: String,
    pub fault: String,
    pub mutation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Proof {
    pub focused: String,
    pub routed: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewForward {
    pub questions: Vec<String>,
    pub lenses: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Obligations {
    pub schema: String,
    pub generated: String,
    pub docs: String,
    pub changelog: String,
    pub receipt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Exits {
    pub old_path: String,
    pub compatibility: String,
    pub supersession: String,
    pub transfer: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rollback {
    pub rollback: String,
    pub return_to_issue: String,
    pub not_proven: String,
    pub stop: String,
}
