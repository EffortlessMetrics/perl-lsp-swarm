//! Production disposition and borderline-repeat confirmation for #16783.
//!
//! The shadow lane never has a second measurement in hand, so a dispatcher
//! checkbox cannot confirm a 0.5%–1.0% win. Confirmation is a verified prior
//! `release_artifact_size.v1` receipt of the same target, SHA, lock, flags,
//! and policy that already passed every adopt gate except the repeat, from a
//! distinct GitHub Actions run. A receipt from the other macOS triple, a
//! different checkout, a failed first measurement, or the same run cannot
//! promote.
//!
//! Production linker flags are a separate binding: `release.yml` may apply
//! safe-ICF rustflags only for a governed target whose recorded disposition
//! is `adopt`. Unmeasured (`not_proven`) and no-win targets keep the platform
//! linker. That binding is a contract over the live workflow text, not a
//! generator that writes `release.yml`.

use color_eyre::eyre::{Context, Result, eyre};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use super::model::{DecisionPolicy, SizeDelta};
use super::policy::{GOVERNED_TARGETS, SAFE_ICF_RUSTFLAGS, SCHEMA_VERSION, recorded_disposition};

/// Subset of a previous measurement needed to confirm a borderline repeat.
/// Extra fields on a full `release_artifact_size.v1` receipt are ignored.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct PriorReceipt {
    pub schema_version: String,
    pub recommendation: String,
    pub subject: PriorSubject,
    pub policy: DecisionPolicy,
    pub comparison: PriorComparison,
    pub baseline: PriorVariant,
    pub candidate: PriorVariant,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct PriorSubject {
    pub git_sha: String,
    pub target: String,
    pub host: String,
    pub tree_clean: bool,
    pub cargo_lock_sha256: String,
    pub baseline_rustflags: String,
    pub candidate_rustflags: String,
    #[serde(default)]
    pub environment: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct PriorComparison {
    pub combined: SizeDelta,
    pub material_reduction: bool,
    pub repeat_requirement_satisfied: bool,
    pub structural_parity: bool,
    pub target_architecture_match: bool,
    pub baseline_archive_identity: bool,
    pub candidate_archive_identity: bool,
    pub baseline_smokes_pass: bool,
    pub candidate_smokes_pass: bool,
    pub source_identity_bound: bool,
    pub component_growth_within_policy: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct PriorVariant {
    pub lsp_smoke: PriorSmoke,
    pub dap_smoke: PriorSmoke,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct PriorSmoke {
    pub binary_matches: bool,
}

/// Why a prior receipt cannot confirm the current measurement's repeat.
#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) enum RepeatDenial {
    SchemaMismatch { observed: String },
    TargetMismatch { prior: String, current: String },
    SourceMismatch,
    LockMismatch,
    FlagsMismatch,
    PolicyMismatch,
    RunIdentityMissing,
    SameMeasurementRun,
    PriorWasNotAWaitingBorderline { recommendation: String },
}

impl RepeatDenial {
    pub(crate) fn as_limitation(&self) -> String {
        match self {
            Self::SchemaMismatch { observed } => format!(
                "prior receipt schema `{observed}` cannot confirm a {SCHEMA_VERSION} measurement"
            ),
            Self::TargetMismatch { prior, current } => format!(
                "prior receipt target `{prior}` cannot confirm `{current}`; each governed triple is independent"
            ),
            Self::SourceMismatch => {
                "prior receipt is not bound to the same full source SHA as this measurement"
                    .to_string()
            }
            Self::LockMismatch => {
                "prior receipt Cargo.lock digest does not match this measurement".to_string()
            }
            Self::FlagsMismatch => {
                "prior receipt candidate rustflags do not match this measurement".to_string()
            }
            Self::PolicyMismatch => {
                "prior receipt decision policy does not match this measurement".to_string()
            }
            Self::RunIdentityMissing => {
                "repeat confirmation requires distinct GITHUB_RUN_ID values on both receipts; \
                 a local replay of the same artifacts is not a second measurement"
                    .to_string()
            }
            Self::SameMeasurementRun => {
                "prior receipt GITHUB_RUN_ID matches this measurement; one run cannot confirm itself"
                    .to_string()
            }
            Self::PriorWasNotAWaitingBorderline { recommendation } => format!(
                "prior receipt recommendation `{recommendation}` is not a waiting borderline \
                 (valid evidence that failed only the repeat requirement)"
            ),
        }
    }
}

pub(crate) fn load_prior_receipt(path: &Path) -> Result<PriorReceipt> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("reading prior receipt {}", path.display()))?;
    let receipt: PriorReceipt = serde_json::from_str(&raw)
        .map_err(|error| eyre!("prior receipt is not a usable {SCHEMA_VERSION} subset: {error}"))?;
    Ok(receipt)
}

/// A prior receipt confirms a borderline repeat only when it is a second
/// GitHub Actions run of the same subject that already passed every adopt
/// gate except the repeat requirement.
pub(crate) fn confirm_repeat(
    current_target: &str,
    current_git_sha: &str,
    current_cargo_lock_sha256: &str,
    current_candidate_rustflags: &str,
    current_policy: &DecisionPolicy,
    current_environment: &BTreeMap<String, String>,
    prior: &PriorReceipt,
) -> Result<(), RepeatDenial> {
    if prior.schema_version != SCHEMA_VERSION {
        return Err(RepeatDenial::SchemaMismatch { observed: prior.schema_version.clone() });
    }
    if prior.subject.target != current_target {
        return Err(RepeatDenial::TargetMismatch {
            prior: prior.subject.target.clone(),
            current: current_target.to_string(),
        });
    }
    if !same_full_git_sha(current_git_sha, &prior.subject.git_sha) {
        return Err(RepeatDenial::SourceMismatch);
    }
    if current_cargo_lock_sha256 == "unknown"
        || prior.subject.cargo_lock_sha256 == "unknown"
        || current_cargo_lock_sha256 != prior.subject.cargo_lock_sha256
    {
        return Err(RepeatDenial::LockMismatch);
    }
    if current_candidate_rustflags.trim() != prior.subject.candidate_rustflags.trim() {
        return Err(RepeatDenial::FlagsMismatch);
    }
    if !policy_matches(current_policy, &prior.policy) {
        return Err(RepeatDenial::PolicyMismatch);
    }
    match (github_run_id(current_environment), github_run_id(&prior.subject.environment)) {
        (None, _) | (_, None) => return Err(RepeatDenial::RunIdentityMissing),
        (Some(current_run), Some(prior_run)) if current_run == prior_run => {
            return Err(RepeatDenial::SameMeasurementRun);
        }
        (Some(_), Some(_)) => {}
    }
    if !prior_is_waiting_borderline(prior, current_policy) {
        return Err(RepeatDenial::PriorWasNotAWaitingBorderline {
            recommendation: prior.recommendation.clone(),
        });
    }
    Ok(())
}

fn prior_is_waiting_borderline(prior: &PriorReceipt, policy: &DecisionPolicy) -> bool {
    prior.recommendation == "not_proven"
        && prior.subject.tree_clean
        && prior.subject.host == prior.subject.target
        && prior.subject.baseline_rustflags.trim().is_empty()
        && prior.subject.candidate_rustflags.trim() == SAFE_ICF_RUSTFLAGS
        && prior.comparison.material_reduction
        && !prior.comparison.repeat_requirement_satisfied
        && prior.comparison.combined.reduction_basis_points
            < policy.repeat_required_below_basis_points
        && prior.comparison.structural_parity
        && prior.comparison.target_architecture_match
        && prior.comparison.baseline_archive_identity
        && prior.comparison.candidate_archive_identity
        && prior.comparison.baseline_smokes_pass
        && prior.comparison.candidate_smokes_pass
        && prior.comparison.source_identity_bound
        && prior.comparison.component_growth_within_policy
        && prior.baseline.lsp_smoke.binary_matches
        && prior.baseline.dap_smoke.binary_matches
        && prior.candidate.lsp_smoke.binary_matches
        && prior.candidate.dap_smoke.binary_matches
}

fn github_run_id(environment: &BTreeMap<String, String>) -> Option<&str> {
    environment
        .get("GITHUB_RUN_ID")
        .map(String::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn policy_matches(left: &DecisionPolicy, right: &DecisionPolicy) -> bool {
    left.minimum_reduction_basis_points == right.minimum_reduction_basis_points
        && left.minimum_reduction_bytes == right.minimum_reduction_bytes
        && left.maximum_component_growth_basis_points == right.maximum_component_growth_basis_points
        && left.maximum_component_growth_bytes == right.maximum_component_growth_bytes
        && left.repeat_required_below_basis_points == right.repeat_required_below_basis_points
}

fn same_full_git_sha(left: &str, right: &str) -> bool {
    is_full_git_sha(left)
        && is_full_git_sha(right)
        && normalize_git_sha(left) == normalize_git_sha(right)
}

fn is_full_git_sha(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.len() == 40 && trimmed.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn normalize_git_sha(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

/// Safe-ICF rustflags the production release workflow may apply for this
/// target given a controller recommendation. Any recommendation other than
/// `adopt`, and any ungoverned triple, yields no flags.
///
/// The measurement example does not apply production flags; the disposition
/// contract proof is the consumer. `release.yml` is not generated from this
/// helper — the contract test reads the live workflow.
#[allow(dead_code)]
pub(crate) fn production_safe_icf_rustflags(
    target: &str,
    recommendation: &str,
) -> Option<&'static str> {
    if GOVERNED_TARGETS.contains(&target) && recommendation == "adopt" {
        Some(SAFE_ICF_RUSTFLAGS)
    } else {
        None
    }
}

/// Production flags authorized by the recorded #16783 disposition for `target`.
/// An Intel `adopt` cannot authorize the arm64 row: lookup is exact-target.
#[allow(dead_code)]
pub(crate) fn recorded_production_rustflags(target: &str) -> Option<&'static str> {
    let recorded = recorded_disposition(target)?;
    production_safe_icf_rustflags(recorded.target, recorded.recommendation)
}
