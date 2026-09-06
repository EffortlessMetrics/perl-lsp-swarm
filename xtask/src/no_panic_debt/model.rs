use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::Path;

/// Schema identity for this exact-tree projection.
pub const SCHEMA: &str = "test_panic_family_debt.v1";
pub const PRODUCER: &str = "cargo xtask no-panic debt inventory";

/// Inputs for one exact-tree observation. Missing optional paths use repository defaults.
pub struct InventoryRequest<'a> {
    pub root: &'a Path,
    pub registry_path: Option<&'a Path>,
    pub lint_ledger_path: Option<&'a Path>,
    pub lint_catalog_dir: Option<&'a Path>,
    pub clippy_observation: Option<&'a ClippyObservation>,
    pub owner_state: Option<&'a OwnerState>,
    pub repository_commit: Option<String>,
}

/// Optional Clippy instrument observation. Aborted/missing targets are `not_proven`.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClippyObservation {
    pub targets: Vec<ClippyTargetObservation>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClippyTargetObservation {
    pub package: String,
    pub target: String,
    pub status: ClippyTargetStatus,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClippyTargetStatus {
    Ok,
    Aborted,
    Missing,
}

/// Optional owner-currentness input. Ordinary generation does not call GitHub.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OwnerState {
    pub closed_or_missing: BTreeSet<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetKind {
    UnitTest,
    IntegrationTest,
    Example,
    Bench,
    Build,
    Unknown,
}

impl TargetKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UnitTest => "unit_test",
            Self::IntegrationTest => "integration_test",
            Self::Example => "example",
            Self::Bench => "bench",
            Self::Build => "build",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DebtStatus {
    DirectDebt,
    IntentionalExactException,
    SelectedForConversion,
    ConvertedAbsent,
    StaleRegistry,
    StaleSourceDeclaration,
    StaleOwner,
    Unowned,
    InstrumentNotProven,
}

impl DebtStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DirectDebt => "direct_debt",
            Self::IntentionalExactException => "intentional_exact_exception",
            Self::SelectedForConversion => "selected_for_conversion",
            Self::ConvertedAbsent => "converted_absent",
            Self::StaleRegistry => "stale_registry",
            Self::StaleSourceDeclaration => "stale_source_declaration",
            Self::StaleOwner => "stale_owner",
            Self::Unowned => "unowned",
            Self::InstrumentNotProven => "instrument_not_proven",
        }
    }

    pub fn is_green(self) -> bool {
        matches!(self, Self::IntentionalExactException | Self::ConvertedAbsent)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InstrumentStatus {
    Ok,
    NotProven,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Instrument {
    pub kind: String,
    pub subject: String,
    pub status: InstrumentStatus,
    pub detail: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SourceDigest {
    pub path: String,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PackageRecord {
    pub name: String,
    pub manifest: String,
    pub features: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FileRecord {
    pub package: String,
    pub target_kind: TargetKind,
    pub path: String,
    pub feature: Option<String>,
    pub platform: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Entrypoint {
    pub package: String,
    pub target_kind: TargetKind,
    pub path: String,
    pub name: String,
    pub feature: Option<String>,
    pub platform: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Population {
    pub packages: Vec<PackageRecord>,
    pub files: Vec<FileRecord>,
    pub entrypoints: Vec<Entrypoint>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DebtRow {
    pub kind: String,
    pub package: String,
    pub target_kind: TargetKind,
    pub path: String,
    pub entrypoint: String,
    pub site_family: String,
    pub source_identity: String,
    pub selector_identity: String,
    pub declaration_identity: String,
    pub declaration_scope: String,
    pub registry_relation: String,
    pub owner: String,
    pub status: DebtStatus,
    pub proof_requirement: String,
    pub limitations: Vec<String>,
}

impl DebtRow {
    pub(crate) fn identity_key(&self) -> String {
        format!(
            "{}|{}|{}|{}|{}",
            self.kind, self.path, self.entrypoint, self.site_family, self.selector_identity
        )
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DerivedCounts {
    pub files: usize,
    pub entrypoints: usize,
    pub rows: usize,
    pub unowned: usize,
    pub stale_registry: usize,
    pub instrument_not_proven: usize,
    pub by_family: Vec<(String, usize)>,
    pub by_status: Vec<(String, usize)>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Inventory {
    pub schema: String,
    pub producer: String,
    pub repository_commit: String,
    pub digests: Vec<SourceDigest>,
    pub instruments: Vec<Instrument>,
    pub population: Population,
    pub rows: Vec<DebtRow>,
    pub counts: DerivedCounts,
    pub limitations: Vec<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct Vocabulary {
    pub lints: BTreeSet<String>,
    pub method_families: BTreeSet<&'static str>,
    pub macro_families: BTreeSet<&'static str>,
    pub instruments: Vec<Instrument>,
}

#[derive(Clone, Debug)]
pub(crate) struct Topology {
    pub packages: Vec<PackageRecord>,
    pub files: Vec<FileRecord>,
    pub instruments: Vec<Instrument>,
}

#[derive(Clone, Debug)]
pub(crate) struct Discovered {
    pub entrypoints: Vec<Entrypoint>,
    pub sites: Vec<RawSite>,
    pub declarations: Vec<RawDeclaration>,
    pub instruments: Vec<Instrument>,
}

#[derive(Clone, Debug)]
pub(crate) struct RawSite {
    pub package: String,
    pub target_kind: TargetKind,
    pub path: String,
    pub entrypoint: String,
    pub family: String,
    pub snippet: String,
    pub line: usize,
    pub column: usize,
    pub feature: Option<String>,
    pub platform: Option<String>,
    pub covering_declaration: Option<String>,
    pub covering_scope: Option<String>,
    pub covering_owner: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct RawDeclaration {
    pub package: String,
    pub target_kind: TargetKind,
    pub path: String,
    pub entrypoint: String,
    pub lint: String,
    pub form: String,
    pub scope: String,
    pub owner: String,
    pub snippet: String,
    pub line: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) struct RegistryKey {
    pub path: String,
    pub enclosing_test_or_function: String,
    pub macro_family: String,
    pub normalized_snippet: String,
    pub selector_identity: String,
}

#[derive(Clone, Debug)]
pub(crate) struct RegistryRecord {
    pub key: RegistryKey,
    pub accepted_reason: String,
    pub state: RegistryState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RegistryState {
    Active,
    Retired,
}
