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
    #[serde(default)]
    pub target_name: String,
    pub feature: Option<String>,
    #[serde(default)]
    pub required_features: Vec<String>,
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
    #[serde(default)]
    pub observation_complete: bool,
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
    pub covered_paths: BTreeSet<String>,
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::path::Path;

    #[test]
    fn public_projection_types_round_trip_identity_and_status_names() {
        let request = InventoryRequest {
            root: Path::new("."),
            registry_path: None,
            lint_ledger_path: None,
            lint_catalog_dir: None,
            clippy_observation: None,
            owner_state: None,
            repository_commit: Some("test".to_string()),
        };
        assert!(request.repository_commit.as_deref() == Some("test"));
        let observation = ClippyObservation {
            targets: vec![ClippyTargetObservation {
                package: "demo".to_string(),
                target: "lib".to_string(),
                status: ClippyTargetStatus::Ok,
            }],
        };
        assert_eq!(observation.targets[0].status, ClippyTargetStatus::Ok);
        let _ = ClippyTargetStatus::Aborted;
        let _ = ClippyTargetStatus::Missing;
        let owners = OwnerState { closed_or_missing: ["#13397".to_string()].into_iter().collect() };
        assert!(owners.closed_or_missing.contains("#13397"));
        assert_eq!(TargetKind::UnitTest.as_str(), "unit_test");
        assert_eq!(TargetKind::IntegrationTest.as_str(), "integration_test");
        assert_eq!(TargetKind::Example.as_str(), "example");
        assert_eq!(TargetKind::Bench.as_str(), "bench");
        assert_eq!(TargetKind::Build.as_str(), "build");
        assert_eq!(TargetKind::Unknown.as_str(), "unknown");
        assert_eq!(DebtStatus::DirectDebt.as_str(), "direct_debt");
        assert_eq!(DebtStatus::IntentionalExactException.as_str(), "intentional_exact_exception");
        assert_eq!(DebtStatus::SelectedForConversion.as_str(), "selected_for_conversion");
        assert_eq!(DebtStatus::ConvertedAbsent.as_str(), "converted_absent");
        assert_eq!(DebtStatus::StaleRegistry.as_str(), "stale_registry");
        assert_eq!(DebtStatus::StaleSourceDeclaration.as_str(), "stale_source_declaration");
        assert_eq!(DebtStatus::StaleOwner.as_str(), "stale_owner");
        assert_eq!(DebtStatus::Unowned.as_str(), "unowned");
        assert_eq!(DebtStatus::InstrumentNotProven.as_str(), "instrument_not_proven");
        assert!(DebtStatus::ConvertedAbsent.is_green());
        assert!(!DebtStatus::DirectDebt.is_green());
        let _ = InstrumentStatus::Ok;
        let instrument = Instrument {
            kind: "panic_registry".to_string(),
            subject: "ci/panic_test_identities.json".to_string(),
            status: InstrumentStatus::NotProven,
            detail: "missing".to_string(),
        };
        let digest = SourceDigest {
            path: "policy/clippy-lints.toml".to_string(),
            sha256: "abc".to_string(),
        };
        let package = PackageRecord {
            name: "demo".to_string(),
            manifest: "crates/demo/Cargo.toml".to_string(),
            features: vec!["need-me".to_string()],
        };
        let file = FileRecord {
            package: "demo".to_string(),
            target_kind: TargetKind::IntegrationTest,
            path: "crates/demo/tests/known.rs".to_string(),
            target_name: "known".to_string(),
            feature: None,
            required_features: Vec::new(),
            platform: None,
        };
        let entry = Entrypoint {
            package: "demo".to_string(),
            target_kind: TargetKind::IntegrationTest,
            path: "crates/demo/tests/known.rs".to_string(),
            name: "known_panic".to_string(),
            feature: None,
            platform: None,
        };
        let population =
            Population { packages: vec![package], files: vec![file], entrypoints: vec![entry] };
        let row = DebtRow {
            kind: "site".to_string(),
            package: "demo".to_string(),
            target_kind: TargetKind::IntegrationTest,
            path: "crates/demo/tests/known.rs".to_string(),
            entrypoint: "known_panic".to_string(),
            site_family: "panic!".to_string(),
            source_identity: "panic!(\"known\")".to_string(),
            selector_identity: "invocation:abc:occurrence:1".to_string(),
            declaration_identity: String::new(),
            declaration_scope: String::new(),
            registry_relation: "none".to_string(),
            owner: "#13397".to_string(),
            status: DebtStatus::DirectDebt,
            proof_requirement: "source-scan".to_string(),
            limitations: Vec::new(),
        };
        let counts = DerivedCounts {
            files: 1,
            entrypoints: 1,
            rows: 1,
            unowned: 0,
            stale_registry: 0,
            instrument_not_proven: 1,
            observation_complete: false,
            by_family: vec![("panic!".to_string(), 1)],
            by_status: vec![("direct_debt".to_string(), 1)],
        };
        let inventory = Inventory {
            schema: SCHEMA.to_string(),
            producer: PRODUCER.to_string(),
            repository_commit: "test".to_string(),
            digests: vec![digest],
            instruments: vec![instrument],
            population,
            rows: vec![row],
            counts,
            limitations: vec!["macro_test".to_string()],
        };
        assert!(inventory.rows[0].identity_key().contains("panic!"));
        let vocabulary = Vocabulary {
            lints: BTreeSet::from(["clippy::unwrap_used".to_string()]),
            method_families: BTreeSet::from(["unwrap"]),
            macro_families: BTreeSet::from(["panic!"]),
            instruments: Vec::new(),
        };
        let topology =
            Topology { packages: Vec::new(), files: Vec::new(), instruments: Vec::new() };
        let discovered = Discovered {
            entrypoints: Vec::new(),
            sites: vec![RawSite {
                package: "demo".to_string(),
                target_kind: TargetKind::IntegrationTest,
                path: "crates/demo/tests/known.rs".to_string(),
                entrypoint: "known_panic".to_string(),
                family: "panic!".to_string(),
                snippet: "panic!(\"known\")".to_string(),
                line: 4,
                column: 8,
                feature: None,
                platform: None,
                covering_declaration: None,
                covering_scope: None,
                covering_owner: None,
            }],
            declarations: vec![RawDeclaration {
                package: "demo".to_string(),
                target_kind: TargetKind::IntegrationTest,
                path: "crates/demo/tests/known.rs".to_string(),
                entrypoint: "known_panic".to_string(),
                lint: "clippy::unwrap_used".to_string(),
                form: "allow".to_string(),
                scope: "fn".to_string(),
                owner: String::new(),
                snippet: "allow(clippy::unwrap_used)".to_string(),
                line: 1,
            }],
            instruments: Vec::new(),
            covered_paths: BTreeSet::new(),
        };
        let key = RegistryKey {
            path: "crates/demo/tests/known.rs".to_string(),
            enclosing_test_or_function: "known_panic".to_string(),
            macro_family: "panic!".to_string(),
            normalized_snippet: "panic!(\"known\")".to_string(),
            selector_identity: "invocation:abc:occurrence:1".to_string(),
        };
        let record = RegistryRecord {
            key: key.clone(),
            accepted_reason: "owner".to_string(),
            state: RegistryState::Active,
        };
        assert_eq!(record.state, RegistryState::Active);
        assert_eq!(vocabulary.lints.len(), 1);
        assert!(topology.files.is_empty());
        assert_eq!(discovered.sites.len(), 1);
        let _ = request.root;
    }
}
