//! Typed critic rule-proof manifest model (`critic_rule_proof.v1`).

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const SCHEMA_VERSION: &str = "critic_rule_proof.v1";
pub const MANIFEST_NAME: &str = "critic-rule-proof";
pub const MANIFEST_PATH: &str = "fixtures/critic-rule-proof/manifest.json";
pub const SCHEMA_PATH: &str = "schemas/critic_rule_proof.v1.schema.json";
pub const STATUS_PATH: &str = "docs/project/status/critic_rule_proof.md";
pub const FIXTURE_ROOT: &str = "fixtures/critic-rule-proof";
pub const ISSUE: u32 = 6973;

/// Resolve a declared fixture path beneath the repository root and require it
/// to stay inside the fixture root. Absolute paths, non-normal components
/// (`.`, `..`, roots, prefixes), and symlinked escapes are rejected so a
/// manifest cannot digest or execute files outside
/// `fixtures/critic-rule-proof`.
pub fn resolve_fixture_path(root: &Path, fixture: &str) -> Result<PathBuf, String> {
    if fixture.is_empty() {
        return Err("fixture path is empty".to_string());
    }
    if fixture.contains('\\') {
        return Err(format!("fixture `{fixture}` must use `/` separators inside the fixture root"));
    }
    let relative = Path::new(fixture);
    if relative.is_absolute() {
        return Err(format!("fixture `{fixture}` must be a relative path inside the fixture root"));
    }
    for component in relative.components() {
        if !matches!(component, std::path::Component::Normal(_)) {
            return Err(format!("fixture `{fixture}` contains a non-normal path component"));
        }
    }
    let fixture_root = root.join(FIXTURE_ROOT);
    let canonical_root = fixture_root
        .canonicalize()
        .map_err(|error| format!("fixture root `{FIXTURE_ROOT}`: cannot resolve: {error}"))?;
    let canonical = fixture_root
        .join(relative)
        .canonicalize()
        .map_err(|error| format!("fixture `{fixture}`: cannot resolve: {error}"))?;
    if !canonical.starts_with(&canonical_root) {
        return Err(format!("fixture `{fixture}` resolves outside the fixture root"));
    }
    Ok(canonical)
}

/// Closed evidence-class vocabulary for one rule-proof case.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceClass {
    PositiveFinding,
    NearMissNegative,
    ProjectShapedFalsePositive,
    FileLevelSuppression,
    CanonicalIdentity,
    SourceRangeAndSeverity,
    RemediationClass,
    AutomaticFixRoundTrip,
    Boundary,
}

impl EvidenceClass {
    /// Stable snake_case spelling used in schema, status, and errors.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PositiveFinding => "positive_finding",
            Self::NearMissNegative => "near_miss_negative",
            Self::ProjectShapedFalsePositive => "project_shaped_false_positive",
            Self::FileLevelSuppression => "file_level_suppression",
            Self::CanonicalIdentity => "canonical_identity",
            Self::SourceRangeAndSeverity => "source_range_and_severity",
            Self::RemediationClass => "remediation_class",
            Self::AutomaticFixRoundTrip => "automatic_fix_round_trip",
            Self::Boundary => "boundary",
        }
    }

    /// Every evidence class the schema admits, in status-table order.
    #[must_use]
    pub const fn all() -> &'static [Self] {
        &[
            Self::PositiveFinding,
            Self::NearMissNegative,
            Self::ProjectShapedFalsePositive,
            Self::FileLevelSuppression,
            Self::CanonicalIdentity,
            Self::SourceRangeAndSeverity,
            Self::RemediationClass,
            Self::AutomaticFixRoundTrip,
            Self::Boundary,
        ]
    }

    /// Classes every pilot rule must exhibit. Automatic round-trip is required
    /// only when `declared_remediation` is `automatic_candidate`.
    #[must_use]
    pub const fn required_for_every_pilot_rule(self) -> bool {
        !matches!(self, Self::AutomaticFixRoundTrip)
    }

    /// Finding-oriented classes that cannot be inherited from a clean fixture.
    #[must_use]
    pub const fn requires_governed_expected_finding(self) -> bool {
        matches!(
            self,
            Self::PositiveFinding
                | Self::CanonicalIdentity
                | Self::SourceRangeAndSeverity
                | Self::RemediationClass
                | Self::AutomaticFixRoundTrip
        )
    }

    /// Negative classes that must name the governed rule as a non-finding.
    #[must_use]
    pub const fn requires_governed_non_finding(self) -> bool {
        matches!(self, Self::NearMissNegative | Self::ProjectShapedFalsePositive)
    }

    /// Parse-error fixtures skip live critic; only boundary evidence is honest.
    #[must_use]
    pub const fn allowed_on_parse_error(self) -> bool {
        matches!(self, Self::Boundary)
    }
}

/// Native critic profile named by a case or rule row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProofProfile {
    Recommended,
    Strict,
}

impl ProofProfile {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Recommended => "recommended",
            Self::Strict => "strict",
        }
    }
}

/// Whether the fixture is expected to parse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParseExpectation {
    Ok,
    Error,
}

/// Static remediation eligibility named by the proof contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProofRemediation {
    None,
    Manual,
    PreviewCandidate,
    AutomaticCandidate,
}

impl ProofRemediation {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Manual => "manual",
            Self::PreviewCandidate => "preview_candidate",
            Self::AutomaticCandidate => "automatic_candidate",
        }
    }

    #[must_use]
    pub const fn automatic_round_trip_applicable(self) -> bool {
        matches!(self, Self::AutomaticCandidate)
    }
}

/// Native critic severity spelling used by the proof contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProofSeverity {
    Gentle,
    Stern,
    Harsh,
    Cruel,
    Brutal,
}

impl ProofSeverity {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Gentle => "gentle",
            Self::Stern => "stern",
            Self::Harsh => "harsh",
            Self::Cruel => "cruel",
            Self::Brutal => "brutal",
        }
    }
}

/// Apply policy for an automatic-fix round trip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FixApply {
    Automatic,
    Forbidden,
}

/// One compatibility alias row copied from the identity registry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AliasRecord {
    pub origin: String,
    pub code: String,
    pub shape: String,
}

/// Fixture identity and digest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FixtureRecord {
    pub digest: String,
}

/// One governed native rule in the pilot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleRecord {
    pub rule_id: String,
    pub canonical_id: String,
    pub profile: ProofProfile,
    pub declared_remediation: ProofRemediation,
    pub identity_aliases: Vec<AliasRecord>,
}

/// One expected finding location and identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedFinding {
    pub rule_id: String,
    pub start_byte: usize,
    pub end_byte: usize,
    pub excerpt: String,
    pub severity: ProofSeverity,
    pub remediation_eligibility: ProofRemediation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix_title: Option<String>,
}

/// Apply/reparse/re-diagnose contract for automatic edits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FixRoundTrip {
    pub apply: FixApply,
    pub expect_reparse: ParseExpectation,
    pub expect_target_removed: bool,
    pub expect_no_new_governed: bool,
    pub expected_edits: Vec<ExpectedEdit>,
}

/// Exact edit identity required by an automatic-fix proof.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedEdit {
    pub start_byte: usize,
    pub end_byte: usize,
    pub new_text: String,
}

/// One proof case bound to a fixture and proposition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaseRecord {
    pub case_id: String,
    pub rule_id: String,
    pub evidence_classes: Vec<EvidenceClass>,
    pub fixture: String,
    pub profile: ProofProfile,
    pub include: Vec<String>,
    pub parse_expectation: ParseExpectation,
    pub expected_findings: Vec<ExpectedFinding>,
    pub expected_non_findings: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix_round_trip: Option<FixRoundTrip>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suppression_selector: Option<String>,
    pub proposition: String,
}

/// Versioned rule-proof manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleProofManifest {
    #[serde(rename = "$schema", default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    pub schema_version: String,
    pub manifest: String,
    pub issue: u32,
    pub owner: String,
    pub status: String,
    pub updated: String,
    pub claim_boundary: String,
    pub evidence_classes: Vec<EvidenceClass>,
    pub fixtures: std::collections::BTreeMap<String, FixtureRecord>,
    pub rules: Vec<RuleRecord>,
    pub cases: Vec<CaseRecord>,
}

impl RuleProofManifest {
    #[must_use]
    pub fn rule(&self, rule_id: &str) -> Option<&RuleRecord> {
        self.rules.iter().find(|rule| rule.rule_id == rule_id)
    }
}
