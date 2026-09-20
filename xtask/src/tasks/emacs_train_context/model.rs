//! Typed models for the `emacs_node_context.v1` exact-tree context engine
//! (CTXENG `#11756`, populating the E04 context plane `#11718`).
//!
//! Two input documents are consumed strictly as data:
//!
//! - the stable `emacs_train.v1` train manifest (E01 `#10918`), typed below
//!   with `deny_unknown_fields` so a malformed or silently extended manifest
//!   fails closed as a schema defect instead of being normalized;
//! - the `emacs_train_context_mappings.v1` population document shipped with
//!   this engine, also `deny_unknown_fields`.
//!
//! The `emacs_train_revision.v1` ledger (E01R `#11770`) is consumed as raw
//! data (digest-bound, typed accessors) per its own consumption rule: it is
//! never re-derived, cloned, or rewritten here.

use serde::{Deserialize, Serialize};

pub const MANIFEST_SCHEMA_NAME: &str = "emacs_train.v1";
pub const MAPPING_SCHEMA_NAME: &str = "emacs_train_context_mappings.v1";
pub const PACKET_SCHEMA_NAME: &str = "emacs_node_context.v1";
pub const SCHEMA_VERSION: u64 = 1;

/// Component roles a mapping entry may claim. Roles, not paths, decide what a
/// mapped file is allowed to be: only `production` may implement a node seam.
pub const ROLES: [&str; 12] = [
    "production",
    "test_falsifier",
    "test_control",
    "fixture",
    "schema",
    "spec",
    "population_data",
    "generated_output",
    "doc",
    "policy",
    "script",
    "instruction",
];

/// File kinds a mapping entry may declare. Kinds are declared by population
/// data and verified against the exact tree; the law table in `resolve.rs`
/// binds roles to kinds so a helper/schema/fixture/generated file can never
/// satisfy a production role.
pub const KINDS: [&str; 12] = [
    "rust_source",
    "rust_test",
    "elisp_source",
    "elisp_test",
    "script",
    "schema_json",
    "spec_bundle",
    "doc",
    "policy_toml",
    "json_data",
    "fixture",
    "generated",
];

// ---------------------------------------------------------------------------
// Stable train manifest (E01 #10918), consumed as data.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrainManifest {
    pub schema: String,
    pub schema_version: u64,
    pub programme: Programme,
    pub authority_planes: Vec<AuthorityPlane>,
    pub train_role_vocabulary: Vec<RoleVocabulary>,
    pub evidence_semantics: EvidenceSemantics,
    pub external_authorities: Vec<ExternalAuthority>,
    pub open_decisions_routed_elsewhere: Vec<OpenDecision>,
    pub existing_candidate_adoption: ExistingCandidateAdoption,
    pub nodes: Vec<TrainNode>,
    #[serde(default)]
    pub supersessions: Vec<Supersession>,
    pub revision_governance: RevisionGovernance,
    pub limitations: Vec<String>,
}

impl TrainManifest {
    pub fn node(&self, node_id: &str) -> Option<&TrainNode> {
        self.nodes.iter().find(|n| n.node_id == node_id)
    }

    pub fn node_by_issue(&self, issue: u64) -> Option<&TrainNode> {
        self.nodes.iter().find(|n| n.issue == issue)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Programme {
    pub parent_programme_issue: u64,
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
    pub metadata_only_rule: String,
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
    pub owner: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExistingCandidateAdoption {
    pub node: String,
    pub candidate_pull: u64,
    pub confirm_with: String,
    pub rule: String,
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
pub struct RevisionGovernance {
    pub owner_node: String,
    pub owner_issue: u64,
    pub invalidates: String,
    pub never: String,
    pub metadata_only: String,
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
    pub spec: NodeSpec,
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
pub struct NodeSpec {
    pub disposition: String,
    pub owner: String,
    pub stale_policy: String,
    pub spec_authority: String,
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

// ---------------------------------------------------------------------------
// Population mapping document (`emacs_train_context_mappings.v1`).
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MappingDocument {
    pub schema: String,
    pub schema_version: u64,
    pub consumed_manifest: ConsumedDocument,
    pub consumed_ledger: ConsumedDocument,
    pub population_ownership: PopulationOwnership,
    pub bounds: MappingBounds,
    pub nodes: Vec<NodeMapping>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConsumedDocument {
    pub bundle: String,
    pub schema: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PopulationOwnership {
    pub engine: u64,
    pub substrate_population: u64,
    pub projection_population: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MappingBounds {
    pub max_components_per_node: usize,
    pub max_tests_per_node: usize,
    pub max_read_set: usize,
    pub max_write_set: usize,
    pub max_not_authority: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeMapping {
    pub node_id: String,
    pub status: String,
    #[serde(default)]
    pub components: Vec<ComponentMapping>,
    #[serde(default)]
    pub tests: Vec<TestMapping>,
    #[serde(default)]
    pub generated: Vec<GeneratedMapping>,
    #[serde(default)]
    pub read_set: Vec<String>,
    #[serde(default)]
    pub write_set: Vec<String>,
    #[serde(default)]
    pub not_authority: Vec<NotAuthority>,
    #[serde(default)]
    pub specs: Vec<String>,
    #[serde(default)]
    pub write_set_note: Option<String>,
    #[serde(default)]
    pub no_production_component_reason: Option<String>,
    #[serde(default)]
    pub blocker: Option<MappingBlocker>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComponentMapping {
    pub component_id: String,
    pub role: String,
    pub kind: String,
    pub path: String,
    #[serde(default)]
    pub symbol: Option<String>,
    #[serde(default)]
    pub symbol_kind: Option<String>,
    #[serde(default)]
    pub client_family: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TestMapping {
    pub path: String,
    #[serde(default)]
    pub selector: Option<String>,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneratedMapping {
    pub input: String,
    pub output: String,
    pub generator: String,
    pub stale_check: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotAuthority {
    pub path_or_symbol: String,
    pub reason: String,
    pub owner: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MappingBlocker {
    pub reason: String,
    pub owner_issue: u64,
    pub action: String,
}

// ---------------------------------------------------------------------------
// Emitted context packet (`emacs_node_context.v1`).
// ---------------------------------------------------------------------------

/// One resolved per-node context packet. Every field is derived: semantic
/// fields recompute from the stable manifest, navigation fields recompute from
/// the population mapping validated against the exact current tree. No cached
/// packet is ever read; staleness is impossible by construction and reuse
/// across trees is detectable through `binding`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeContextPacket {
    pub schema: String,
    pub schema_version: u64,
    pub status: String,
    pub binding: ContextBinding,
    pub node: PacketNode,
    pub proposition: PacketProposition,
    pub spec: PacketSpec,
    pub graph: PacketGraph,
    pub revision_currency: PacketRevisionCurrency,
    pub instructions: Vec<PacketInstruction>,
    pub checked_specs: Vec<PacketCheckedSpec>,
    pub components: Vec<PacketComponent>,
    pub tests: Vec<PacketTest>,
    pub generated: Vec<PacketGenerated>,
    pub read_set: Vec<String>,
    pub write_set: Vec<String>,
    pub not_authority: Vec<PacketNotAuthority>,
    pub gaps: Vec<PacketGap>,
    pub bounds: PacketBounds,
    pub privacy: PacketPrivacy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextBinding {
    pub git_commit: String,
    pub git_tree: String,
    pub manifest_digest: String,
    pub ledger_digest: String,
    pub architecture_digests: Vec<PacketDigest>,
    pub mapping_digest: String,
    pub input_digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PacketDigest {
    pub path: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PacketNode {
    pub node_id: String,
    pub issue: u64,
    pub title: String,
    pub title_fingerprint: String,
    pub aliases: Vec<String>,
    pub train_role: String,
    pub lane: String,
    pub chain_home: String,
    pub chain_controller: String,
    pub buildable: bool,
    pub conflict_key: String,
    pub parallel_group: String,
    pub stack_relation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PacketProposition {
    pub one_pr_outcome: String,
    pub claim_ceiling: String,
    pub authority_before: String,
    pub authority_after: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PacketSpec {
    pub disposition: String,
    pub owner: String,
    pub stale_policy: String,
    pub spec_authority: String,
    pub first_falsifier: String,
    pub control_opposite: String,
    pub control_stale: String,
    pub proof_focused: String,
    pub proof_routed: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PacketGraph {
    pub dependencies: Vec<PacketDependency>,
    pub successors: Vec<String>,
    pub consumed_authorities: Vec<String>,
    pub allowed_components: Vec<String>,
    pub forbidden_adjacent_owners: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PacketDependency {
    pub target: String,
    pub class: String,
    pub provenance: String,
    pub resolved_issue: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PacketRevisionCurrency {
    pub ledger_schema: String,
    pub ledger_digest: String,
    pub latest_entry_id: Option<String>,
    pub latest_sequence: Option<u64>,
    pub latest_revision_kind: Option<String>,
    pub latest_semantic_class: Option<String>,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PacketInstruction {
    pub path: String,
    pub sha256: String,
    pub scope: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PacketCheckedSpec {
    pub bundle: String,
    pub files: Vec<PacketDigest>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PacketComponent {
    pub component_id: String,
    pub role: String,
    pub kind: String,
    pub path: String,
    pub symbol: Option<String>,
    pub symbol_kind: Option<String>,
    pub client_family: Option<String>,
    pub sha256: String,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PacketTest {
    pub path: String,
    pub selector: Option<String>,
    pub kind: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PacketGenerated {
    pub input: String,
    pub output: String,
    pub generator: String,
    pub stale_check: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PacketNotAuthority {
    pub path_or_symbol: String,
    pub reason: String,
    pub owner: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PacketGap {
    pub subject: String,
    pub reason: String,
    pub owner_issue: u64,
    pub action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PacketBounds {
    pub max_components_per_node: usize,
    pub max_tests_per_node: usize,
    pub max_read_set: usize,
    pub max_write_set: usize,
    pub max_not_authority: usize,
    pub omitted: Vec<PacketOmission>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PacketOmission {
    pub list: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PacketPrivacy {
    pub repository_relative_paths_only: bool,
    pub no_source_text_embedded: bool,
    pub no_absolute_paths: bool,
    pub no_logs_or_credentials: bool,
    pub note: String,
}
