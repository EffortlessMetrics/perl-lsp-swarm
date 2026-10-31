//! Exact identity projection for the quarantined legacy parser metamorphic population.
//!
//! The legacy trailing-whitespace hash is investigation evidence only. This
//! module freezes the population that feeds that observation while the typed
//! metamorphic oracle replaces it. It deliberately preserves the historical
//! applicability rule without treating that rule as Perl-region authority.

use std::collections::BTreeSet;
use std::error::Error;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::str;

use perl_lsp_rs_core::hashing::sha256_hex;
use serde::{Deserialize, Serialize};

/// Schema version for the legacy population projection.
pub const LEGACY_POPULATION_SCHEMA_VERSION: u32 = 1;

/// Profile identity for the quarantined trailing-whitespace observation.
pub const LEGACY_WHITESPACE_PROFILE: &str = "trailing_horizontal_whitespace.legacy.v1";

/// The one aggregate metric the retained whitespace population may declare.
///
/// `build_legacy_whitespace_population` always names this metric, so a
/// population declaring any other quarantined row as its aggregate would bind
/// unrelated observations to the whitespace population's profile and counts.
pub const LEGACY_WHITESPACE_AGGREGATE_METRIC: &str = "whitespace_invariance_rate";

/// Current authored parser-accuracy manifest.
pub const DEFAULT_PARSER_ACCURACY_MANIFEST: &str =
    "crates/perl-corpus/fixtures/parser_accuracy/manifest.json";

const SUPPORTED_MANIFEST_SCHEMA_VERSION: u32 = 1;

/// Legacy applicability retained for migration accounting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LegacyApplicability {
    /// The historical whole-source transform was attempted.
    Applied,
    /// The historical heuristic omitted the fixture or found no transform point.
    Unclassified,
}

/// Why a legacy population row remains investigation evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LegacyReason {
    /// A raw projection hash cannot establish semantic invariance.
    LegacyHashOracleUntrusted,
    /// The historical whole-file heuristic does not establish inapplicability.
    LegacyApplicabilityUnclassified,
}

/// One exact fixture subject supplied to the population projector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyFixtureInput {
    fixture_id: String,
    source_path: String,
    source_bytes: Vec<u8>,
}

impl LegacyFixtureInput {
    /// Construct one fixture subject from exact source bytes.
    #[must_use]
    pub fn new(fixture_id: String, source_path: String, source_bytes: Vec<u8>) -> Self {
        Self { fixture_id, source_path, source_bytes }
    }
}

/// One retained row in the legacy investigation population.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegacyPopulationRow {
    /// Projection schema version.
    pub schema_version: u32,
    /// Stable, non-ordinal case identity.
    pub case_id: String,
    /// Parser-accuracy fixture identity.
    pub fixture_id: String,
    /// Portable repository-relative source path.
    pub source_path: String,
    /// SHA-256 digest of the exact source bytes.
    pub source_content_digest: String,
    /// Versioned transformation profile.
    pub transformation_profile: String,
    /// Historical applicability disposition.
    pub legacy_applicability: LegacyApplicability,
    /// Investigation-only reason.
    pub reason: LegacyReason,
}

/// Deterministic summary of one exact legacy population.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegacyPopulationSummary {
    /// Projection schema version.
    pub schema_version: u32,
    /// Parser-accuracy manifest schema consumed by this projection.
    pub manifest_schema_version: u32,
    /// Versioned transformation profile.
    pub transformation_profile: String,
    /// Digest of canonical ordered rows.
    pub population_identity: String,
    /// Number of retained fixture rows.
    pub total_case_count: usize,
    /// Number historically admitted by the legacy heuristic.
    pub applied_case_count: usize,
    /// Number retained without a trusted applicability conclusion.
    pub unclassified_case_count: usize,
}

/// Exact, ordered legacy population projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyPopulation {
    manifest_schema_version: u32,
    rows: Vec<LegacyPopulationRow>,
}

impl LegacyPopulation {
    /// Return canonical rows sorted by stable case identity.
    #[must_use]
    pub fn rows(&self) -> &[LegacyPopulationRow] {
        &self.rows
    }

    /// Count rows historically admitted by the legacy heuristic.
    #[must_use]
    pub fn applied_count(&self) -> usize {
        self.rows
            .iter()
            .filter(|row| row.legacy_applicability == LegacyApplicability::Applied)
            .count()
    }

    /// Count rows whose legacy applicability remains unclassified.
    #[must_use]
    pub fn unclassified_count(&self) -> usize {
        self.rows.len().saturating_sub(self.applied_count())
    }

    /// Serialize the complete population as canonical newline-delimited JSON.
    pub fn canonical_ndjson(&self) -> Result<String, LegacyPopulationError> {
        let mut output = String::new();
        for row in &self.rows {
            let encoded = serde_json::to_string(row).map_err(LegacyPopulationError::Serialize)?;
            output.push_str(&encoded);
            output.push('\n');
        }
        Ok(output)
    }

    /// Compute the algorithm-tagged digest of canonical ordered rows.
    pub fn population_identity(&self) -> Result<String, LegacyPopulationError> {
        Ok(sha256_hex(self.canonical_ndjson()?.as_bytes()))
    }

    /// Build a deterministic denominator summary from retained rows.
    pub fn summary(&self) -> Result<LegacyPopulationSummary, LegacyPopulationError> {
        Ok(LegacyPopulationSummary {
            schema_version: LEGACY_POPULATION_SCHEMA_VERSION,
            manifest_schema_version: self.manifest_schema_version,
            transformation_profile: LEGACY_WHITESPACE_PROFILE.to_owned(),
            population_identity: self.population_identity()?,
            total_case_count: self.rows.len(),
            applied_case_count: self.applied_count(),
            unclassified_case_count: self.unclassified_count(),
        })
    }

    /// Serialize the deterministic summary with one terminal newline.
    pub fn canonical_summary_json(&self) -> Result<String, LegacyPopulationError> {
        let mut output = serde_json::to_string_pretty(&self.summary()?)
            .map_err(LegacyPopulationError::Serialize)?;
        output.push('\n');
        Ok(output)
    }
}

/// Every legacy metamorphic observation the contract holds as investigation-only.
///
/// Only the whitespace row is bound to a projected population, so an artifact
/// that declared a *partial* `quarantined_metrics` set would let the other two
/// reappear as `measured` and be counted as trusted accuracy. Both validators
/// require the declaration to cover this set, so under-declaring is refused
/// rather than obeyed.
///
/// This is not the `is_legacy_untrusted_metric` name classifier this contract
/// retired. That downgraded *any* row whose name resembled a legacy metric;
/// this only checks that the artifact's own declaration is complete. A metric
/// named `whitespace_invariance_rate_v2` is still ordinary trusted accuracy.
pub const LEGACY_QUARANTINED_METRICS: [&str; 3] =
    ["whitespace_invariance_rate", "comment_invariance_rate", "newline_style_invariance_rate"];

/// The single runtime authority for the canonical population-identity format.
///
/// `perl_lsp_rs_core::hashing::sha256_hex` — the only producer of these
/// identities — emits `sha256:` followed by 64 **lowercase** hex characters,
/// and `.ci/schemas/parser-accuracy.schema.json` pins the same
/// `^sha256:[0-9a-f]{64}$`. A validator written as `is_ascii_hexdigit` would
/// also admit uppercase `A-F`, so an artifact the schema rejects would render
/// as current status. Both the generator's `validate_legacy_population_evidence`
/// and the status reader's `trust_disposition_is_fail_closed` call this, so the
/// runtime has one definition rather than one per consumer.
#[must_use]
pub fn is_canonical_population_identity(value: &str) -> bool {
    let Some(digest) = value.strip_prefix("sha256:") else {
        return false;
    };
    digest.len() == 64 && digest.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

/// Fail-closed population construction errors.
#[derive(Debug)]
pub enum LegacyPopulationError {
    /// A manifest or source file could not be read.
    Read {
        /// Path whose bytes were requested.
        path: PathBuf,
        /// Underlying filesystem error.
        source: std::io::Error,
    },
    /// The manifest could not be decoded.
    DecodeManifest {
        /// Manifest path.
        path: PathBuf,
        /// Underlying JSON error.
        source: serde_json::Error,
    },
    /// The manifest schema is not the one this compatibility projection knows.
    UnsupportedManifestSchema {
        /// Observed schema version.
        observed: u32,
    },
    /// No fixtures were supplied.
    EmptyPopulation,
    /// A fixture identity was empty or contained control characters.
    InvalidFixtureId {
        /// Rejected identity.
        fixture_id: String,
    },
    /// Two manifest rows reused one fixture identity.
    DuplicateFixtureId {
        /// Duplicated identity.
        fixture_id: String,
    },
    /// Two projected rows reused one case identity.
    DuplicateCaseId {
        /// Duplicated identity.
        case_id: String,
    },
    /// A source path was not canonical repository-relative syntax.
    InvalidSourcePath {
        /// Owning fixture identity.
        fixture_id: String,
        /// Rejected path.
        source_path: String,
        /// Stable rejection reason.
        reason: &'static str,
    },
    /// The legacy text transform cannot consume non-UTF-8 source bytes.
    SourceNotUtf8 {
        /// Owning fixture identity.
        fixture_id: String,
        /// Source path.
        source_path: String,
        /// Underlying UTF-8 error.
        source: str::Utf8Error,
    },
    /// Canonical JSON encoding failed.
    Serialize(serde_json::Error),
}

impl fmt::Display for LegacyPopulationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { path, source } => {
                write!(formatter, "failed to read {}: {source}", path.display())
            }
            Self::DecodeManifest { path, source } => {
                write!(formatter, "failed to decode {}: {source}", path.display())
            }
            Self::UnsupportedManifestSchema { observed } => write!(
                formatter,
                "unsupported parser-accuracy manifest schema {observed}; expected {SUPPORTED_MANIFEST_SCHEMA_VERSION}"
            ),
            Self::EmptyPopulation => {
                write!(formatter, "legacy population must retain at least one fixture")
            }
            Self::InvalidFixtureId { fixture_id } => {
                write!(formatter, "invalid parser-accuracy fixture id {fixture_id:?}")
            }
            Self::DuplicateFixtureId { fixture_id } => {
                write!(formatter, "duplicate parser-accuracy fixture id {fixture_id:?}")
            }
            Self::DuplicateCaseId { case_id } => {
                write!(formatter, "duplicate legacy population case id {case_id:?}")
            }
            Self::InvalidSourcePath { fixture_id, source_path, reason } => write!(
                formatter,
                "invalid source path {source_path:?} for fixture {fixture_id:?}: {reason}"
            ),
            Self::SourceNotUtf8 { fixture_id, source_path, source } => write!(
                formatter,
                "legacy source {source_path:?} for fixture {fixture_id:?} is not UTF-8: {source}"
            ),
            Self::Serialize(source) => {
                write!(formatter, "failed to serialize legacy population: {source}")
            }
        }
    }
}

impl Error for LegacyPopulationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Read { source, .. } => Some(source),
            Self::DecodeManifest { source, .. } => Some(source),
            Self::SourceNotUtf8 { source, .. } => Some(source),
            Self::Serialize(source) => Some(source),
            Self::UnsupportedManifestSchema { .. }
            | Self::EmptyPopulation
            | Self::InvalidFixtureId { .. }
            | Self::DuplicateFixtureId { .. }
            | Self::DuplicateCaseId { .. }
            | Self::InvalidSourcePath { .. } => None,
        }
    }
}

#[derive(Debug, Deserialize)]
struct Manifest {
    schema_version: u32,
    fixtures: Vec<ManifestFixture>,
}

#[derive(Debug, Deserialize)]
struct ManifestFixture {
    id: String,
    source_path: String,
}

/// Load the exact current manifest population and source bytes from a checkout.
pub fn load_legacy_whitespace_population(
    project_root: &Path,
) -> Result<LegacyPopulation, LegacyPopulationError> {
    let manifest_path = project_root.join(DEFAULT_PARSER_ACCURACY_MANIFEST);
    let manifest_bytes = fs::read(&manifest_path)
        .map_err(|source| LegacyPopulationError::Read { path: manifest_path.clone(), source })?;
    let manifest: Manifest = serde_json::from_slice(&manifest_bytes)
        .map_err(|source| LegacyPopulationError::DecodeManifest { path: manifest_path, source })?;

    if manifest.schema_version != SUPPORTED_MANIFEST_SCHEMA_VERSION {
        return Err(LegacyPopulationError::UnsupportedManifestSchema {
            observed: manifest.schema_version,
        });
    }

    let mut fixtures = Vec::with_capacity(manifest.fixtures.len());
    for fixture in manifest.fixtures {
        validate_fixture_id(&fixture.id)?;
        validate_source_path(&fixture.id, &fixture.source_path)?;

        let source_path = project_root.join(&fixture.source_path);
        let source_bytes = fs::read(&source_path)
            .map_err(|source| LegacyPopulationError::Read { path: source_path, source })?;
        fixtures.push(LegacyFixtureInput::new(fixture.id, fixture.source_path, source_bytes));
    }

    build_legacy_whitespace_population(manifest.schema_version, fixtures)
}

/// Project exact fixture subjects through the retained legacy applicability rule.
///
/// This seam is public so policy tests and the legacy scorer can consume one
/// implementation. It does not establish that the transformation is safe.
pub fn build_legacy_whitespace_population(
    manifest_schema_version: u32,
    fixtures: Vec<LegacyFixtureInput>,
) -> Result<LegacyPopulation, LegacyPopulationError> {
    if manifest_schema_version != SUPPORTED_MANIFEST_SCHEMA_VERSION {
        return Err(LegacyPopulationError::UnsupportedManifestSchema {
            observed: manifest_schema_version,
        });
    }
    if fixtures.is_empty() {
        return Err(LegacyPopulationError::EmptyPopulation);
    }

    let mut fixture_ids = BTreeSet::new();
    let mut case_ids = BTreeSet::new();
    let mut rows = Vec::with_capacity(fixtures.len());

    for fixture in fixtures {
        validate_fixture_id(&fixture.fixture_id)?;
        validate_source_path(&fixture.fixture_id, &fixture.source_path)?;
        if !fixture_ids.insert(fixture.fixture_id.clone()) {
            return Err(LegacyPopulationError::DuplicateFixtureId {
                fixture_id: fixture.fixture_id,
            });
        }

        let source = str::from_utf8(&fixture.source_bytes).map_err(|source| {
            LegacyPopulationError::SourceNotUtf8 {
                fixture_id: fixture.fixture_id.clone(),
                source_path: fixture.source_path.clone(),
                source,
            }
        })?;
        let applied = legacy_whitespace_case_applies(source);
        let case_id = format!("{LEGACY_WHITESPACE_PROFILE}::{}", fixture.fixture_id);
        if !case_ids.insert(case_id.clone()) {
            return Err(LegacyPopulationError::DuplicateCaseId { case_id });
        }

        rows.push(LegacyPopulationRow {
            schema_version: LEGACY_POPULATION_SCHEMA_VERSION,
            case_id,
            fixture_id: fixture.fixture_id,
            source_path: fixture.source_path,
            source_content_digest: sha256_hex(&fixture.source_bytes),
            transformation_profile: LEGACY_WHITESPACE_PROFILE.to_owned(),
            legacy_applicability: if applied {
                LegacyApplicability::Applied
            } else {
                LegacyApplicability::Unclassified
            },
            reason: if applied {
                LegacyReason::LegacyHashOracleUntrusted
            } else {
                LegacyReason::LegacyApplicabilityUnclassified
            },
        });
    }

    rows.sort_by(|left, right| left.case_id.cmp(&right.case_id));

    Ok(LegacyPopulation { manifest_schema_version, rows })
}

/// The single retained legacy applicability definition.
///
/// The legacy parser-accuracy scorer consumes this predicate so the sampled
/// whitespace denominator cannot drift from the pinned population identity.
#[must_use]
pub fn legacy_whitespace_case_applies(source: &str) -> bool {
    if source.contains("<<") || source.contains("__DATA__") || source.contains("__END__") {
        return false;
    }

    source.split_inclusive('\n').any(|segment| {
        let without_lf = segment.strip_suffix('\n').unwrap_or(segment);
        let body = without_lf.strip_suffix('\r').unwrap_or(without_lf);
        !body.trim().is_empty()
    })
}

fn validate_fixture_id(fixture_id: &str) -> Result<(), LegacyPopulationError> {
    if fixture_id.is_empty() || fixture_id.chars().any(char::is_control) {
        return Err(LegacyPopulationError::InvalidFixtureId { fixture_id: fixture_id.to_owned() });
    }
    Ok(())
}

fn validate_source_path(fixture_id: &str, source_path: &str) -> Result<(), LegacyPopulationError> {
    let invalid = |reason| LegacyPopulationError::InvalidSourcePath {
        fixture_id: fixture_id.to_owned(),
        source_path: source_path.to_owned(),
        reason,
    };

    if source_path.is_empty() {
        return Err(invalid("empty"));
    }
    if source_path.starts_with('/') {
        return Err(invalid("absolute"));
    }
    if source_path.contains('\\') {
        return Err(invalid("backslash_separator"));
    }
    if source_path.ends_with('/') {
        return Err(invalid("trailing_separator"));
    }

    let mut components = source_path.split('/');
    let first = components.next().unwrap_or(source_path);
    // Reject every platform prefix, including drive-relative ones such as
    // `C:fixture.pl`, whose joining could escape the repository root.
    if first.contains(':') {
        return Err(invalid("prefixed"));
    }
    for component in std::iter::once(first).chain(components) {
        if component.is_empty() {
            return Err(invalid("empty_component"));
        }
        if component == "." {
            return Err(invalid("current_component"));
        }
        if component == ".." {
            return Err(invalid("parent_component"));
        }
        if component.chars().any(char::is_control) {
            return Err(invalid("control_character"));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    // Assertions in this unit-test module favor `expect_err`/`panic!` with
    // descriptive messages; the workspace-wide deny is a production-code rule.
    #![allow(clippy::expect_used, clippy::panic)]

    use super::*;

    #[test]
    fn validate_source_path_rejects_every_documented_reason() {
        let cases: [(&str, &str, &str); 10] = [
            ("empty", "", "empty"),
            ("absolute", "/abs/fixture.pl", "absolute"),
            ("backslash_separator", "fixtures\\fixture.pl", "backslash_separator"),
            ("trailing_separator", "fixtures/", "trailing_separator"),
            ("windows_drive_absolute", "C:/fixtures/fixture.pl", "prefixed"),
            ("windows_drive_relative", "C:fixture.pl", "prefixed"),
            ("empty_component", "fixtures//fixture.pl", "empty_component"),
            ("current_component", "./fixtures/fixture.pl", "current_component"),
            ("parent_component", "fixtures/../fixture.pl", "parent_component"),
            ("control_character", "fixtures/\u{7}fixture.pl", "control_character"),
        ];

        for (fixture_id, source_path, expected_reason) in cases {
            let error = validate_source_path(fixture_id, source_path)
                .expect_err("hostile source path must be rejected");
            match error {
                LegacyPopulationError::InvalidSourcePath { reason, .. } => {
                    assert_eq!(reason, expected_reason, "case {fixture_id}");
                }
                other => {
                    panic!("case {fixture_id}: unexpected error {other:?}");
                }
            }
        }
    }

    #[test]
    fn validate_source_path_accepts_portable_relative_paths() {
        assert!(validate_source_path("fixture", "fixtures/fixture.pl").is_ok());
        assert!(validate_source_path("fixture", "a/b/c/fixture.pl").is_ok());
    }
}
