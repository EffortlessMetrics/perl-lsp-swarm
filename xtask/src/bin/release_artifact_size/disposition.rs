//! Production disposition and borderline-repeat confirmation for #16783.
//!
//! The shadow lane never has a second measurement in hand, so a dispatcher
//! checkbox cannot confirm a 0.5%–1.0% win. Confirmation is a verified prior
//! `release_artifact_size.v1` receipt of the same target, SHA, lock, flags,
//! and policy. A receipt from the other macOS triple, a different checkout,
//! or a non-borderline first run cannot promote.
//!
//! Production linker flags are a separate binding: `release.yml` may apply
//! safe-ICF rustflags only for a governed target whose recorded disposition
//! is `adopt`. Unmeasured (`not_proven`) and no-win targets keep the platform
//! linker.

use color_eyre::eyre::{Context, Result, eyre};
use serde::Deserialize;
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
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct PriorSubject {
    pub git_sha: String,
    pub target: String,
    pub cargo_lock_sha256: String,
    pub candidate_rustflags: String,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct PriorComparison {
    pub combined: SizeDelta,
    pub material_reduction: bool,
    pub repeat_requirement_satisfied: bool,
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
            Self::PriorWasNotAWaitingBorderline { recommendation } => format!(
                "prior receipt recommendation `{recommendation}` is not a waiting borderline \
                 (`not_proven` with material reduction and unsatisfied repeat)"
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

/// A prior receipt confirms a borderline repeat only when it is the same
/// measurement subject waiting on that repeat — not another target, SHA, or
/// already-decided result, and not a checkbox.
pub(crate) fn confirm_repeat(
    current_target: &str,
    current_git_sha: &str,
    current_cargo_lock_sha256: &str,
    current_candidate_rustflags: &str,
    current_policy: &DecisionPolicy,
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
    if prior.recommendation != "not_proven"
        || !prior.comparison.material_reduction
        || prior.comparison.repeat_requirement_satisfied
        || prior.comparison.combined.reduction_basis_points
            >= current_policy.repeat_required_below_basis_points
    {
        return Err(RepeatDenial::PriorWasNotAWaitingBorderline {
            recommendation: prior.recommendation.clone(),
        });
    }
    Ok(())
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
/// contract proof is the consumer.
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
