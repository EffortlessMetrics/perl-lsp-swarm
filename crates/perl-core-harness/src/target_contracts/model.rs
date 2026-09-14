//! Typed contracts for upstream Perl test-target topology.

use serde::ser::SerializeStruct;
use serde::{Deserialize, Serialize, Serializer};
use std::collections::BTreeMap;

pub const TARGET_SELECTION_SCHEMA_VERSION: &str = "perl_core_harness.target_selection.v1";
pub const TARGET_MATRIX_SCHEMA_VERSION: &str = "perl_core_harness.target_matrix.v1";
pub const TARGET_MATRIX_INDEX_SCHEMA_VERSION: &str = "perl_core_harness.target_matrix_index.v1";
pub const TARGET_MATRIX_PART_SCHEMA_VERSION: &str = "perl_core_harness.target_matrix_part.v1";
pub const TARGET_TOPOLOGY_DRIFT_SCHEMA_VERSION: &str = "perl_core_harness.target_topology_drift.v1";

/// Capability predicate a target declares when it needs a controlling terminal.
///
/// Upstream reaches a terminal two different ways — `t/TEST` selects `runtests
/// tty`, while `minitest` redirects stdin from the configured terminal device —
/// but both require the same host capability, so both declare this predicate.
pub const CONTROLLING_TERMINAL_CAPABILITY: &str = "controlling_terminal";

/// The only environment mechanism that selects the no-terminal path.
///
/// `runtests` sets it in the `tty = N` branch, and `minitest_notty` sets it
/// directly; either way it is what makes a run skip terminal-dependent tests.
pub const RUNTESTS_NO_TTY_ENV: &str = "PERL_SKIP_TTY_TEST";

/// A TAP::Harness *display* variable — deliberately not a terminal mechanism.
///
/// Despite the name, `HARNESS_NOTTY` only makes TAP::Harness format output as
/// though stdout were not a console. It does not resolve the terminal: upstream
/// `test_harness_notty` runs `HARNESS_NOTTY=1 TESTFILE=harness runtests choose`,
/// so its terminal is auto-detected exactly like plain `test_harness`. Treating
/// it as equivalent to [`RUNTESTS_NO_TTY_ENV`] would let a row claim the
/// no-terminal path while still redirecting stdin from `/dev/tty`, which is why
/// [`TargetSelectionContract::validate`] refuses that conflation.
pub const HARNESS_DISPLAY_NO_TTY_ENV: &str = "HARNESS_NOTTY";

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

/// How a target resolves the controlling terminal it runs under.
///
/// The policy is not free-form annotation: [`TargetSelectionContract::validate`]
/// requires it to agree with the row's declared capability predicates and its
/// no-terminal mechanism. Only [`RUNTESTS_NO_TTY_ENV`] resolves the terminal;
/// [`HARNESS_DISPLAY_NO_TTY_ENV`] changes output formatting and never makes a
/// row [`TargetTerminalPolicy::NoTty`].
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetTerminalPolicy {
    /// The target has no terminal-dependent behavior to resolve.
    NotApplicable,
    /// The runner auto-detects a terminal (`runtests choose`).
    Choose,
    /// The target forces a terminal and cannot run without one.
    Tty,
    /// The target forces the no-terminal path through an explicit mechanism.
    NoTty,
    /// The target keeps whatever policy its base target resolved.
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

#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
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

impl Serialize for TargetSelectionContract {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // Canonical serialization always contains every contract field. The
        // topology digest uses a separate projection so presentation fields do
        // not alter topology identity without weakening serde round trips.
        let mut state = serializer.serialize_struct("TargetSelectionContract", 23)?;
        state.serialize_field("schema_version", &self.schema_version)?;
        state.serialize_field("target_id", &self.target_id)?;
        state.serialize_field("upstream_name", &self.upstream_name)?;
        state.serialize_field("aliases", &self.aliases)?;
        state.serialize_field("display_name", &self.display_name)?;
        state.serialize_field("perl_version_row", &self.perl_version_row)?;
        state.serialize_field("target_kind", &self.target_kind)?;
        state.serialize_field("authority", &self.authority)?;
        state.serialize_field("selection_authority", &self.selection_authority)?;
        state.serialize_field("selectors", &self.selectors)?;
        state.serialize_field("script_forms", &self.script_forms)?;
        state.serialize_field("preparation", &self.preparation)?;
        state.serialize_field("variant_of", &self.variant_of)?;
        state.serialize_field("composite_members", &self.composite_members)?;
        state.serialize_field("composite_overlap_policy", &self.composite_overlap_policy)?;
        state.serialize_field("runner_switches", &self.runner_switches)?;
        state.serialize_field("variant_parameters", &self.variant_parameters)?;
        state.serialize_field("environment", &self.environment)?;
        state.serialize_field("terminal_policy", &self.terminal_policy)?;
        state.serialize_field("capability_predicates", &self.capability_predicates)?;
        state.serialize_field("exclusions", &self.exclusions)?;
        state.serialize_field("replaces_target_id", &self.replaces_target_id)?;
        state.serialize_field("change_reason", &self.change_reason)?;
        state.end()
    }
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
