//! Shared catalog, observation, and fixture-contract types.

use serde::{Deserialize, Serialize};

use super::error::CatalogError;

/// Frozen catalog kind.
pub const CATALOG_KIND: &str = "distribution_kwalitee.catalog";
/// Frozen catalog schema version.
pub const CATALOG_SCHEMA_VERSION: u32 = 1;
/// Frozen catalog version label.
pub const CATALOG_VERSION: &str = "v1";
/// Frozen fixture-contract kind.
pub const FIXTURE_KIND: &str = "distribution_kwalitee.fixtures";
/// Frozen fixture-contract schema version.
pub const FIXTURE_SCHEMA_VERSION: u32 = 1;

/// Catalog class from issue #7170.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricClass {
    /// Offline CPANTS-compatible core; the only class in `compatible_core_score`.
    CpantsOfflineCore,
    /// Offline extra; reported, never scored.
    CpantsOfflineExtra,
    /// Offline experimental; reported, never scored.
    CpantsOfflineExperimental,
    /// Site-related metric with a narrower local claim; never scored.
    CpantsSiteAnalogue,
    /// Native ProjectModel extension; never scored as CPANTS core.
    NativeExtension,
    /// Familiar metric that is not locally knowable or is explicitly deferred.
    UnsupportedOrDeferred,
}

impl MetricClass {
    /// Wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CpantsOfflineCore => "cpants_offline_core",
            Self::CpantsOfflineExtra => "cpants_offline_extra",
            Self::CpantsOfflineExperimental => "cpants_offline_experimental",
            Self::CpantsSiteAnalogue => "cpants_site_analogue",
            Self::NativeExtension => "native_extension",
            Self::UnsupportedOrDeferred => "unsupported_or_deferred",
        }
    }

    /// Whether this class may participate in the compatible core score.
    pub fn may_participate_in_core_score(self) -> bool {
        matches!(self, Self::CpantsOfflineCore)
    }
}

/// How a native metric relates to a CPANTS indicator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompatibilityRelationship {
    /// Same locally knowable proposition as CPANTS.
    Direct,
    /// Same family with a documented native adaptation.
    Adapted,
    /// SiteKwalitee-related, implemented from bounded local facts.
    SiteAnalogue,
    /// Native extension, not presented as CPANTS parity.
    NativeExtension,
    /// Recorded but not implemented in catalog v1.
    Deferred,
}

impl CompatibilityRelationship {
    /// Wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::Adapted => "adapted",
            Self::SiteAnalogue => "site_analogue",
            Self::NativeExtension => "native_extension",
            Self::Deferred => "deferred",
        }
    }
}

/// When a metric applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Applicability {
    /// Applies to every staged distribution input.
    AllDistributions,
    /// Applies only to archive inputs.
    ArchiveInput,
}

impl Applicability {
    /// Wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AllDistributions => "all_distributions",
            Self::ArchiveInput => "archive_input",
        }
    }

    /// Whether the metric is in scope for this input role.
    pub fn applies_to(self, input_role: InputRole) -> bool {
        match self {
            Self::AllDistributions => {
                matches!(input_role, InputRole::StagedDirectory | InputRole::Archive)
            }
            Self::ArchiveInput => matches!(input_role, InputRole::Archive),
        }
    }
}

/// Frozen scoring rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScoringRule {
    /// `passed applicable offline-core / applicable offline-core`.
    UnweightedApplicableOfflineCore,
}

/// Input role for a fixture or evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputRole {
    /// Unpacked staged distribution directory.
    StagedDirectory,
    /// Distribution archive.
    Archive,
    /// Authoring tree that is not the staged artifact.
    AuthoringTree,
}

impl InputRole {
    /// Wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::StagedDirectory => "staged_directory",
            Self::Archive => "archive",
            Self::AuthoringTree => "authoring_tree",
        }
    }
}

/// Observed result for one catalog metric. This is not an indicator implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationStatus {
    /// Metric proposition holds.
    Pass,
    /// Metric proposition fails.
    Fail,
    /// Out of scope for this input.
    NotApplicable,
    /// Required facts were not produced.
    Unverified,
    /// Input is invalid; no ordinary score may be derived.
    InvalidInput,
    /// Dynamic/unknown boundary; treated as unverified for core scoring.
    Limitation,
}

/// One independently authored observation used to exercise the scoring contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricObservation {
    /// Catalog metric ID.
    pub id: String,
    /// Observed status.
    pub status: ObservationStatus,
}

impl MetricObservation {
    /// Convenience constructor.
    pub fn new(id: impl Into<String>, status: ObservationStatus) -> Self {
        Self { id: id.into(), status }
    }
}

/// Compatible-core score derivation. Invalid input has no ratio.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompatibleCoreScore {
    /// Input cannot be scored.
    InvalidInput {
        /// Why no ordinary score exists.
        reason: String,
    },
    /// Required core evidence is missing; strict staged evaluation is incomplete.
    Incomplete {
        /// Passed applicable offline-core metrics.
        passed: u32,
        /// Applicable offline-core metrics, including unverified.
        applicable: u32,
        /// Unverified (or limitation) applicable offline-core metrics.
        unverified: u32,
    },
    /// Every applicable offline-core metric has a pass/fail decision.
    Complete {
        /// Passed applicable offline-core metrics.
        passed: u32,
        /// Applicable offline-core metrics.
        applicable: u32,
    },
}

impl CompatibleCoreScore {
    /// Explicit `passed / applicable` pair when a score exists.
    pub fn ratio(&self) -> Option<(u32, u32)> {
        match self {
            Self::InvalidInput { .. } => None,
            Self::Incomplete { passed, applicable, .. } | Self::Complete { passed, applicable } => {
                Some((*passed, *applicable))
            }
        }
    }

    /// Whether strict staged evaluation may proceed.
    pub fn strict_complete(&self) -> bool {
        matches!(self, Self::Complete { .. })
    }
}

/// One frozen catalog metric.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogMetric {
    /// Stable native ID, e.g. `cpants.has_manifest`.
    pub id: String,
    /// Familiar CPANTS alias, e.g. `has_manifest`.
    pub alias: String,
    /// One-line title.
    pub title: String,
    /// Catalog class.
    pub class: MetricClass,
    /// Whether the row participates in `compatible_core_score`.
    pub participates_in_core_score: bool,
    /// Relationship to CPANTS.
    pub relationship: CompatibilityRelationship,
    /// Upstream module that defined the familiar metric.
    pub source_module: String,
    /// Pinned upstream version label.
    pub source_version: String,
    /// Source behavior URL or equivalent reference.
    pub behavior_ref: String,
    /// Required fact classes.
    pub required_facts: Vec<String>,
    /// Applicability by input role.
    pub applicability: Applicability,
    /// Pass proposition.
    pub pass_semantics: String,
    /// Fail proposition.
    pub fail_semantics: String,
    /// Not-applicable proposition.
    pub not_applicable_semantics: String,
    /// Unverified proposition.
    pub unverified_semantics: String,
    /// Metrics this row depends on.
    pub depends_on: Vec<String>,
    /// Who owns remediation text.
    pub remediation_owner: String,
    /// Implementation-owner issue.
    pub implementation_owner: u64,
    /// Fixture IDs covering this metric.
    pub fixture_ids: Vec<String>,
    /// Documented native/CPANTS differences.
    pub known_differences: Vec<String>,
    /// Known limitations.
    pub limitations: Vec<String>,
}

impl CatalogMetric {
    /// Reject class/score/relationship contradictions for one row.
    pub fn validate_score_class(&self) -> Result<(), CatalogError> {
        if self.participates_in_core_score != self.class.may_participate_in_core_score() {
            return Err(CatalogError::ScoreClassContradiction {
                id: self.id.clone(),
                reason: format!(
                    "class `{}` cannot set participates_in_core_score={}",
                    self.class.as_str(),
                    self.participates_in_core_score
                ),
            });
        }
        match (self.class, self.relationship) {
            (MetricClass::CpantsSiteAnalogue, CompatibilityRelationship::SiteAnalogue)
            | (MetricClass::NativeExtension, CompatibilityRelationship::NativeExtension)
            | (MetricClass::UnsupportedOrDeferred, CompatibilityRelationship::Deferred)
            | (
                MetricClass::CpantsOfflineCore
                | MetricClass::CpantsOfflineExtra
                | MetricClass::CpantsOfflineExperimental,
                CompatibilityRelationship::Direct | CompatibilityRelationship::Adapted,
            ) => Ok(()),
            (class, relationship) => Err(CatalogError::ScoreClassContradiction {
                id: self.id.clone(),
                reason: format!(
                    "class `{}` is incompatible with relationship `{}`",
                    class.as_str(),
                    relationship.as_str()
                ),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::panic)]

    use super::*;

    fn metric(
        class: MetricClass,
        relationship: CompatibilityRelationship,
        participates: bool,
    ) -> CatalogMetric {
        CatalogMetric {
            id: "cpants.probe".into(),
            alias: "probe".into(),
            title: "probe".into(),
            class,
            participates_in_core_score: participates,
            relationship,
            source_module: "probe".into(),
            source_version: "1.03".into(),
            behavior_ref: "probe".into(),
            required_facts: vec!["probe".into()],
            applicability: Applicability::AllDistributions,
            pass_semantics: "pass".into(),
            fail_semantics: "fail".into(),
            not_applicable_semantics: "na".into(),
            unverified_semantics: "unverified".into(),
            depends_on: vec![],
            remediation_owner: "distribution_author".into(),
            implementation_owner: 7170,
            fixture_ids: vec!["Acme-CatalogFreeze".into()],
            known_differences: vec![],
            limitations: vec![],
        }
    }

    #[test]
    fn only_offline_core_may_participate_in_core_score() {
        assert!(MetricClass::CpantsOfflineCore.may_participate_in_core_score());
        for class in [
            MetricClass::CpantsOfflineExtra,
            MetricClass::CpantsOfflineExperimental,
            MetricClass::CpantsSiteAnalogue,
            MetricClass::NativeExtension,
            MetricClass::UnsupportedOrDeferred,
        ] {
            assert!(!class.may_participate_in_core_score());
        }
    }

    #[test]
    fn allowed_class_relationship_pairs_pass() {
        let allowed = [
            (MetricClass::CpantsOfflineCore, CompatibilityRelationship::Direct, true),
            (MetricClass::CpantsOfflineCore, CompatibilityRelationship::Adapted, true),
            (MetricClass::CpantsOfflineExtra, CompatibilityRelationship::Direct, false),
            (MetricClass::CpantsOfflineExtra, CompatibilityRelationship::Adapted, false),
            (MetricClass::CpantsOfflineExperimental, CompatibilityRelationship::Direct, false),
            (MetricClass::CpantsOfflineExperimental, CompatibilityRelationship::Adapted, false),
            (MetricClass::CpantsSiteAnalogue, CompatibilityRelationship::SiteAnalogue, false),
            (MetricClass::NativeExtension, CompatibilityRelationship::NativeExtension, false),
            (MetricClass::UnsupportedOrDeferred, CompatibilityRelationship::Deferred, false),
        ];
        for (class, relationship, participates) in allowed {
            metric(class, relationship, participates).validate_score_class().unwrap_or_else(
                |error| panic!("allowed pair {class:?}/{relationship:?} failed: {error}"),
            );
        }
    }

    #[test]
    fn non_core_classes_cannot_participate_in_core_score() {
        let cases = [
            (MetricClass::CpantsOfflineExtra, CompatibilityRelationship::Direct),
            (MetricClass::CpantsOfflineExperimental, CompatibilityRelationship::Adapted),
            (MetricClass::CpantsSiteAnalogue, CompatibilityRelationship::SiteAnalogue),
            (MetricClass::NativeExtension, CompatibilityRelationship::NativeExtension),
            (MetricClass::UnsupportedOrDeferred, CompatibilityRelationship::Deferred),
        ];
        for (class, relationship) in cases {
            let error = metric(class, relationship, true)
                .validate_score_class()
                .expect_err("non-core must not participate");
            match error {
                CatalogError::ScoreClassContradiction { id, reason } => {
                    assert_eq!(id, "cpants.probe");
                    assert!(
                        reason.contains("cannot set participates_in_core_score=true"),
                        "{reason}"
                    );
                    assert!(reason.contains(class.as_str()), "{reason}");
                }
                other => panic!("unexpected error for {class:?}: {other:?}"),
            }
        }
    }

    #[test]
    fn core_class_cannot_drop_core_participation() {
        let error =
            metric(MetricClass::CpantsOfflineCore, CompatibilityRelationship::Direct, false)
                .validate_score_class()
                .expect_err("core must participate");
        match error {
            CatalogError::ScoreClassContradiction { id, reason } => {
                assert_eq!(id, "cpants.probe");
                assert!(reason.contains("cannot set participates_in_core_score=false"), "{reason}");
                assert!(reason.contains("cpants_offline_core"), "{reason}");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn mismatched_class_and_relationship_fail() {
        let cases = [
            (MetricClass::CpantsOfflineCore, CompatibilityRelationship::SiteAnalogue, true),
            (MetricClass::CpantsOfflineCore, CompatibilityRelationship::NativeExtension, true),
            (MetricClass::CpantsOfflineCore, CompatibilityRelationship::Deferred, true),
            (MetricClass::CpantsOfflineExtra, CompatibilityRelationship::SiteAnalogue, false),
            (MetricClass::CpantsSiteAnalogue, CompatibilityRelationship::Direct, false),
            (MetricClass::NativeExtension, CompatibilityRelationship::Adapted, false),
            (MetricClass::UnsupportedOrDeferred, CompatibilityRelationship::Direct, false),
        ];
        for (class, relationship, participates) in cases {
            let error = metric(class, relationship, participates)
                .validate_score_class()
                .expect_err("mismatch");
            match error {
                CatalogError::ScoreClassContradiction { id, reason } => {
                    assert_eq!(id, "cpants.probe");
                    assert!(reason.contains("incompatible with relationship"), "{reason}");
                    assert!(reason.contains(class.as_str()), "{reason}");
                    assert!(reason.contains(relationship.as_str()), "{reason}");
                }
                other => panic!("unexpected error for {class:?}/{relationship:?}: {other:?}"),
            }
        }
    }
}

/// Frozen catalog v1 envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DistributionKwaliteeCatalog {
    /// Schema version.
    pub schema_version: u32,
    /// Catalog kind.
    pub kind: String,
    /// Catalog version label.
    pub catalog_version: String,
    /// Lifecycle status.
    pub status: String,
    /// Scoring rule.
    pub scoring_rule: ScoringRule,
    /// Pinned Module::CPANTS::Analyse version.
    pub cpants_analyse_version: String,
    /// Pinned SiteKwalitee source identity.
    pub cpants_site_kwalitee_ref: String,
    /// Production runtime claim.
    pub production_runtime: String,
    /// Oracle role claim.
    pub oracle_role: String,
    /// Frozen metric rows.
    pub metric: Vec<CatalogMetric>,
}

/// Whether fixture bytes are present in this PR.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentStatus {
    /// Files are committed under `fixtures/distribution/<id>/`.
    Committed,
    /// Identity is frozen; tree lands in a follow-up corpus PR.
    Reserved,
}

/// How expected results are expressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpectationRule {
    /// Every applicable offline-core metric is expected to pass.
    AllApplicableOfflineCorePass,
    /// Exactly the named primary metric fails, plus permitted cascades.
    SingleDefect,
    /// Input is invalid; no ordinary score.
    InvalidInput,
}

/// CPANTS comparability of a fixture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CpantsComparability {
    /// Directly comparable to pinned CPANTS output.
    Direct,
    /// Comparable only after documented adaptation.
    Adapted,
    /// Not comparable (security/invalid input, site-only, etc.).
    NotComparable,
}

/// Fixture kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FixtureKind {
    /// Minimal valid distribution.
    MinimalValidDistribution,
    /// Realistic modern valid distribution.
    RealisticValidDistribution,
    /// Single intended defect.
    SingleDefect,
    /// Archive security / invalid-input control.
    ArchiveSecurityFailure,
}

/// One frozen fixture identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DistributionKwaliteeFixture {
    /// Stable fixture ID.
    pub id: String,
    /// Fixture kind.
    pub kind: FixtureKind,
    /// Input role this identity exercises.
    pub input_role: InputRole,
    /// Whether bytes are committed.
    pub content_status: ContentStatus,
    /// Expected-result rule.
    pub expectation_rule: ExpectationRule,
    /// Primary failing metric IDs, when any.
    pub primary_fail: Vec<String>,
    /// Additional metric IDs allowed to fail or go unverified.
    pub permitted_cascades: Vec<String>,
    /// Owning issue.
    pub owning_issue: u64,
    /// CPANTS comparability.
    pub cpants_comparability: CpantsComparability,
    /// Intended proposition.
    pub intended_proposition: String,
    /// Relative files when [`ContentStatus::Committed`].
    pub committed_files: Vec<String>,
}

/// Frozen fixture-identity contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DistributionKwaliteeFixtureContract {
    /// Schema version.
    pub schema_version: u32,
    /// Fixture-contract kind.
    pub kind: String,
    /// Bound catalog kind.
    pub catalog_kind: String,
    /// Bound catalog version.
    pub catalog_version: String,
    /// Lifecycle status.
    pub status: String,
    /// Oracle policy.
    pub oracle_policy: String,
    /// Frozen fixture rows.
    pub fixture: Vec<DistributionKwaliteeFixture>,
}
