//! Typed contracts for upstream Perl test-target topology.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const TARGET_SELECTION_SCHEMA_VERSION: &str = "perl_core_harness.target_selection.v1";
pub const TARGET_MATRIX_SCHEMA_VERSION: &str = "perl_core_harness.target_matrix.v1";
pub const TARGET_MATRIX_INDEX_SCHEMA_VERSION: &str =
    "perl_core_harness.target_matrix_index.v1";
pub const TARGET_MATRIX_PART_SCHEMA_VERSION: &str =
    "perl_core_harness.target_matrix_part.v1";
pub const TARGET_TOPOLOGY_DRIFT_SCHEMA_VERSION: &str =
    "perl_core_harness.target_topology_drift.v1";

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetKind {
    PhysicalSeries,
    SelectorVariant,
    EnvironmentVariant,
    PreparationOnly,
    GeneratedComposite,
    InstrumentationOnly,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetAuthorityKind {
    Test,
    Harness,
    Make,
    Explicit,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetAuthority {
    pub kind: TargetAuthorityKind,
    pub entrypoint: String,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManifestPopulation {
    RootLib,
    CoreRootLib,
    Dist,
    Ext,
    Cpan,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TargetSelector {
    RecursiveRoot { path: String },
    NonRecursiveGlob { pattern: String },
    ExactFile { path: String },
    ExternalGlob { pattern: String },
    ManifestPopulation { component: ManifestPopulation },
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetScriptForm {
    DotT,
    TestPl,
    GeneratedPerl,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetPerlRuntime {
    Miniperl,
    FullPerl,
    Inherited,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetTerminalPolicy {
    NotApplicable,
    Choose,
    Tty,
    NoTty,
    Inherited,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompositeOverlapPolicy {
    RejectOverlap,
    DeduplicateByLogicalSource,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetPreparation {
    pub make_target: Option<String>,
    pub perl_runtime: TargetPerlRuntime,
    pub required_products: Vec<String>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetExclusion {
    pub subject: String,
    pub reason_code: String,
    pub claim_impact: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetSelectionContract {
    pub schema_version: String,
    pub target_id: String,
    pub upstream_name: String,
    pub aliases: Vec<String>,
    pub display_name: String,
    pub perl_version_row: String,
    pub target_kind: TargetKind,
    pub authority: TargetAuthority,
    pub selection_authority: Option<TargetAuthority>,
    pub selectors: Vec<TargetSelector>,
    pub script_forms: Vec<TargetScriptForm>,
    pub preparation: TargetPreparation,
    pub variant_of: Option<String>,
    pub composite_members: Vec<String>,
    pub composite_overlap_policy: Option<CompositeOverlapPolicy>,
    pub runner_switches: Vec<String>,
    pub variant_parameters: BTreeMap<String, String>,
    pub environment: BTreeMap<String, String>,
    pub terminal_policy: TargetTerminalPolicy,
    pub capability_predicates: Vec<String>,
    pub exclusions: Vec<TargetExclusion>,
    pub replaces_target_id: Option<String>,
    pub change_reason: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetDisposition {
    Implemented,
    Planned,
    GeneratedComposite,
    PreparationOnly,
    InstrumentationOnly,
    PlatformUnavailable,
    PolicyExcluded,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetMatrixEntry {
    pub contract: TargetSelectionContract,
    pub disposition: TargetDisposition,
    pub owner_issue: Option<u64>,
    pub claim_boundary: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetMatrixIndex {
    pub schema_version: String,
    pub perl_version_row: String,
    pub perl_requested_ref: String,
    pub perl_resolved_ref: String,
    pub topology_sources: BTreeMap<String, String>,
    pub target_files: Vec<String>,
    pub claim_boundary: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetMatrixPart {
    pub schema_version: String,
    pub targets: Vec<TargetMatrixEntry>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpstreamTargetMatrix {
    pub schema_version: String,
    pub perl_version_row: String,
    pub perl_requested_ref: String,
    pub perl_resolved_ref: String,
    pub topology_sources: BTreeMap<String, String>,
    pub targets: Vec<TargetMatrixEntry>,
    pub claim_boundary: String,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetTopologyDriftStatus {
    Compared,
    NotProven,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetTopologyDrift {
    pub schema_version: String,
    pub status: TargetTopologyDriftStatus,
    pub pinned_matrix_fingerprint: String,
    pub observed_matrix_fingerprint: Option<String>,
    pub observed_perl_ref: String,
    pub observed_perl_resolved_ref: String,
    pub observed_topology_sources: BTreeMap<String, String>,
    pub added_target_ids: Vec<String>,
    pub removed_target_ids: Vec<String>,
    pub changed_target_ids: Vec<String>,
    pub not_proven_reason: Option<String>,
    pub claim_boundary: String,
}
