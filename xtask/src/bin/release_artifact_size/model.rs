use color_eyre::eyre::{Result, bail};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Recommendation {
    Adopt,
    DoNotAdopt,
    Reject,
    NotProven,
}

impl Recommendation {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Adopt => "adopt",
            Self::DoNotAdopt => "do_not_adopt",
            Self::Reject => "reject",
            Self::NotProven => "not_proven",
        }
    }

    pub(crate) fn status(self) -> &'static str {
        match self {
            Self::Adopt | Self::DoNotAdopt => "pass",
            Self::Reject => "fail",
            Self::NotProven => "not_proven",
        }
    }

    pub(crate) fn is_blocking(self) -> bool {
        matches!(self, Self::Reject | Self::NotProven)
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SmokeStatus {
    Pass,
    Fail,
    Invalid,
    Missing,
}

#[derive(Debug, Serialize)]
pub(crate) struct Receipt {
    pub(crate) check: &'static str,
    pub(crate) schema_version: &'static str,
    pub(crate) status: &'static str,
    pub(crate) recommendation: Recommendation,
    pub(crate) claim_boundary: &'static str,
    pub(crate) subject: SubjectIdentity,
    pub(crate) policy: DecisionPolicy,
    pub(crate) baseline: VariantEvidence,
    pub(crate) candidate: VariantEvidence,
    pub(crate) comparison: ComparisonEvidence,
    pub(crate) limitations: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct SubjectIdentity {
    pub(crate) repository: &'static str,
    pub(crate) git_sha: String,
    pub(crate) tree_clean: bool,
    pub(crate) target: String,
    pub(crate) host: String,
    pub(crate) workspace_version: String,
    pub(crate) cargo_lock_sha256: String,
    pub(crate) rustc: String,
    pub(crate) cargo: String,
    pub(crate) rust_lld: Option<ToolIdentity>,
    pub(crate) profile: &'static str,
    pub(crate) baseline_rustflags: String,
    pub(crate) candidate_rustflags: String,
    pub(crate) environment: BTreeMap<String, String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ToolIdentity {
    pub(crate) version: String,
    pub(crate) sha256: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct DecisionPolicy {
    pub(crate) minimum_reduction_basis_points: i64,
    pub(crate) minimum_reduction_bytes: i64,
    pub(crate) maximum_component_growth_basis_points: i64,
    pub(crate) maximum_component_growth_bytes: i64,
}

impl DecisionPolicy {
    pub(crate) fn validate(&self) -> Result<()> {
        if self.minimum_reduction_basis_points < 0
            || self.minimum_reduction_bytes < 0
            || self.maximum_component_growth_basis_points < 0
            || self.maximum_component_growth_bytes < 0
        {
            bail!("size comparison policy values must be non-negative");
        }
        Ok(())
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct VariantEvidence {
    pub(crate) directory: String,
    pub(crate) binaries: BTreeMap<String, FileArtifact>,
    pub(crate) archive: Option<ArchiveEvidence>,
    pub(crate) lsp_smoke: SmokeEvidence,
    pub(crate) dap_smoke: SmokeEvidence,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct FileArtifact {
    pub(crate) path: String,
    pub(crate) bytes: u64,
    pub(crate) sha256: String,
    pub(crate) file_description: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ArchiveEvidence {
    pub(crate) artifact: FileArtifact,
    pub(crate) embedded_binaries: BTreeMap<String, EmbeddedArtifact>,
    pub(crate) matches_directory: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct EmbeddedArtifact {
    pub(crate) bytes: u64,
    pub(crate) sha256: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct SmokeEvidence {
    pub(crate) path: String,
    pub(crate) status: SmokeStatus,
    pub(crate) observed_status: Option<String>,
    pub(crate) binary: Option<String>,
    pub(crate) binary_matches: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct ComparisonEvidence {
    pub(crate) binaries: BTreeMap<String, SizeDelta>,
    pub(crate) combined: SizeDelta,
    pub(crate) archive: SizeDelta,
    pub(crate) structural_parity: bool,
    pub(crate) target_architecture_match: bool,
    pub(crate) baseline_archive_identity: bool,
    pub(crate) candidate_archive_identity: bool,
    pub(crate) baseline_smokes_pass: bool,
    pub(crate) candidate_smokes_pass: bool,
    pub(crate) material_reduction: bool,
    pub(crate) component_growth_within_policy: bool,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct SizeDelta {
    pub(crate) baseline_bytes: u64,
    pub(crate) candidate_bytes: u64,
    pub(crate) reduction_bytes: i64,
    pub(crate) reduction_basis_points: i64,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct DecisionFacts {
    pub(crate) baseline_smokes_pass: bool,
    pub(crate) candidate_smoke_failed: bool,
    pub(crate) candidate_smokes_pass: bool,
    pub(crate) structural_parity: bool,
    pub(crate) target_architecture_match: bool,
    pub(crate) baseline_archive_identity: bool,
    pub(crate) candidate_archive_identity: bool,
    pub(crate) baseline_smoke_identity: bool,
    pub(crate) candidate_smoke_identity: bool,
    pub(crate) complete_artifacts: bool,
    pub(crate) subject_complete: bool,
    pub(crate) apple_target: bool,
    pub(crate) baseline_flags_clean: bool,
    pub(crate) candidate_flags_exact: bool,
    pub(crate) material_reduction: bool,
    pub(crate) component_growth_within_policy: bool,
}

pub(crate) fn decide(facts: &DecisionFacts) -> Recommendation {
    if !facts.baseline_smokes_pass {
        return Recommendation::NotProven;
    }
    if facts.candidate_smoke_failed {
        return Recommendation::Reject;
    }
    if !facts.candidate_smokes_pass {
        return Recommendation::NotProven;
    }
    if !facts.target_architecture_match {
        return Recommendation::NotProven;
    }
    if !facts.structural_parity {
        return Recommendation::Reject;
    }
    if facts.baseline_smoke_identity && !facts.candidate_smoke_identity {
        return Recommendation::Reject;
    }
    if facts.baseline_archive_identity && !facts.candidate_archive_identity {
        return Recommendation::Reject;
    }
    if !facts.complete_artifacts
        || !facts.baseline_archive_identity
        || !facts.candidate_archive_identity
        || !facts.baseline_smoke_identity
        || !facts.candidate_smoke_identity
        || !facts.subject_complete
        || !facts.apple_target
        || !facts.baseline_flags_clean
        || !facts.candidate_flags_exact
    {
        return Recommendation::NotProven;
    }
    if facts.material_reduction && facts.component_growth_within_policy {
        Recommendation::Adopt
    } else {
        Recommendation::DoNotAdopt
    }
}

pub(crate) fn smokes_pass(variant: &VariantEvidence) -> bool {
    variant.lsp_smoke.status == SmokeStatus::Pass
        && variant.dap_smoke.status == SmokeStatus::Pass
}

pub(crate) fn component_growth_exceeds(
    delta: &SizeDelta,
    policy: &DecisionPolicy,
) -> bool {
    if delta.reduction_bytes >= 0 {
        return false;
    }
    let growth_bytes = delta.reduction_bytes.saturating_abs();
    let growth_basis_points = delta.reduction_basis_points.saturating_abs();
    growth_bytes > policy.maximum_component_growth_bytes
        || growth_basis_points > policy.maximum_component_growth_basis_points
}

pub(crate) fn size_delta(baseline_bytes: u64, candidate_bytes: u64) -> SizeDelta {
    let baseline_i128 = i128::from(baseline_bytes);
    let candidate_i128 = i128::from(candidate_bytes);
    let reduction_i128 = baseline_i128 - candidate_i128;
    let basis_points_i128 = if baseline_i128 == 0 {
        0
    } else {
        reduction_i128.saturating_mul(10_000) / baseline_i128
    };
    SizeDelta {
        baseline_bytes,
        candidate_bytes,
        reduction_bytes: clamp_i128_to_i64(reduction_i128),
        reduction_basis_points: clamp_i128_to_i64(basis_points_i128),
    }
}

fn clamp_i128_to_i64(value: i128) -> i64 {
    if value > i128::from(i64::MAX) {
        i64::MAX
    } else if value < i128::from(i64::MIN) {
        i64::MIN
    } else {
        value as i64
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DecisionFacts, DecisionPolicy, Recommendation, SizeDelta, component_growth_exceeds, decide,
        size_delta,
    };

    fn policy() -> DecisionPolicy {
        DecisionPolicy {
            minimum_reduction_basis_points: 50,
            minimum_reduction_bytes: 131_072,
            maximum_component_growth_basis_points: 25,
            maximum_component_growth_bytes: 32_768,
        }
    }

    fn ready_facts() -> DecisionFacts {
        DecisionFacts {
            baseline_smokes_pass: true,
            candidate_smoke_failed: false,
            candidate_smokes_pass: true,
            structural_parity: true,
            target_architecture_match: true,
            baseline_archive_identity: true,
            candidate_archive_identity: true,
            baseline_smoke_identity: true,
            candidate_smoke_identity: true,
            complete_artifacts: true,
            subject_complete: true,
            apple_target: true,
            baseline_flags_clean: true,
            candidate_flags_exact: true,
            material_reduction: true,
            component_growth_within_policy: true,
        }
    }

    #[test]
    fn size_delta_reports_reduction_in_basis_points() {
        let delta = size_delta(1_000_000, 990_000);
        assert_eq!(delta.reduction_bytes, 10_000);
        assert_eq!(delta.reduction_basis_points, 100);
    }

    #[test]
    fn size_delta_reports_growth_as_negative_reduction() {
        let delta = size_delta(1_000_000, 1_010_000);
        assert_eq!(delta.reduction_bytes, -10_000);
        assert_eq!(delta.reduction_basis_points, -100);
    }

    #[test]
    fn component_growth_policy_uses_byte_or_basis_point_ceiling() {
        let small_growth = SizeDelta {
            baseline_bytes: 10_000_000,
            candidate_bytes: 10_020_000,
            reduction_bytes: -20_000,
            reduction_basis_points: -20,
        };
        let basis_point_growth = SizeDelta {
            baseline_bytes: 1_000_000,
            candidate_bytes: 1_003_000,
            reduction_bytes: -3_000,
            reduction_basis_points: -30,
        };
        assert!(!component_growth_exceeds(&small_growth, &policy()));
        assert!(component_growth_exceeds(&basis_point_growth, &policy()));
    }

    #[test]
    fn decision_adopts_only_a_material_clean_win() {
        assert_eq!(decide(&ready_facts()), Recommendation::Adopt);
    }

    #[test]
    fn decision_returns_no_adopt_for_a_valid_small_win() {
        let mut facts = ready_facts();
        facts.material_reduction = false;
        assert_eq!(decide(&facts), Recommendation::DoNotAdopt);
    }

    #[test]
    fn decision_rejects_a_candidate_smoke_failure() {
        let mut facts = ready_facts();
        facts.candidate_smoke_failed = true;
        facts.candidate_smokes_pass = false;
        assert_eq!(decide(&facts), Recommendation::Reject);
    }

    #[test]
    fn decision_is_not_proven_when_subject_identity_is_incomplete() {
        let mut facts = ready_facts();
        facts.subject_complete = false;
        assert_eq!(decide(&facts), Recommendation::NotProven);
    }

    #[test]
    fn decision_rejects_candidate_archive_substitution() {
        let mut facts = ready_facts();
        facts.candidate_archive_identity = false;
        assert_eq!(decide(&facts), Recommendation::Reject);
    }

    #[test]
    fn decision_is_not_proven_when_baseline_archive_identity_is_missing() {
        let mut facts = ready_facts();
        facts.baseline_archive_identity = false;
        assert_eq!(decide(&facts), Recommendation::NotProven);
    }

    #[test]
    fn decision_rejects_candidate_smoke_substitution() {
        let mut facts = ready_facts();
        facts.candidate_smoke_identity = false;
        assert_eq!(decide(&facts), Recommendation::Reject);
    }
}
