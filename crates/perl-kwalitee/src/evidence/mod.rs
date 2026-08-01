//! Evidence adapters: the pure functions that turn repository state (manifests,
//! first-mile surfaces, receipt files) into an indicator [`Outcome`].
//!
//! Every adapter is filesystem-only and deterministic — no process spawning, no
//! network. Heavier checks that genuinely need to run a subprocess (release
//! archive validation, the runCritic parity test, `update-status --check`) are
//! supplied to the evaluator from outside as
//! [`ExternalResult`](crate::ExternalResult)s; the crate never shells out.

pub(crate) mod cargo_manifest;
pub(crate) mod dap;
pub(crate) mod nightly;
pub(crate) mod product_surface;
pub(crate) mod quality_gate;
pub(crate) mod readiness;

use crate::indicator::{EvidenceRef, IndicatorStatus};

/// The raw result of evaluating one indicator, before it is combined with its
/// catalog metadata (id/area/title/weight/mandatory).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Outcome {
    pub status: IndicatorStatus,
    pub evidence: Vec<EvidenceRef>,
    pub remediation: Option<String>,
}

/// The shared receipt-freshness rule.
///
/// Every receipt the crate reads records the commit it was generated at. A
/// receipt stamped with a different commit than the evaluation's HEAD is not
/// proof about the current tree, so a `Pass` is downgraded to `Warn` and a note
/// naming both commits is appended to `evidence`. Non-pass statuses are left
/// alone — stale evidence never *improves* an outcome.
///
/// Freshness is only asserted when both sides name a real commit. An empty
/// string on either side means the field was absent, and `"unknown"` is the
/// xtask wrapper's placeholder for an unresolvable git HEAD; in those cases
/// there is nothing to compare and the status passes through untouched.
///
/// `field` names the receipt field the commit came from (`"commit"`, `"head"`)
/// so the note points at the actual key to inspect.
pub(crate) fn apply_freshness(
    status: IndicatorStatus,
    receipt_commit: &str,
    expected_commit: &str,
    field: &str,
    evidence: &mut Vec<EvidenceRef>,
) -> IndicatorStatus {
    let stale = !expected_commit.is_empty()
        && expected_commit != "unknown"
        && !receipt_commit.is_empty()
        && receipt_commit != expected_commit;
    if !stale {
        return status;
    }

    evidence.push(EvidenceRef::new(
        "note",
        format!("stale receipt: {field} {receipt_commit} != HEAD {expected_commit}"),
    ));
    if status == IndicatorStatus::Pass { IndicatorStatus::Warn } else { status }
}

impl Outcome {
    pub fn pass(evidence: Vec<EvidenceRef>) -> Self {
        Outcome { status: IndicatorStatus::Pass, evidence, remediation: None }
    }

    pub fn fail(evidence: Vec<EvidenceRef>, remediation: impl Into<String>) -> Self {
        Outcome { status: IndicatorStatus::Fail, evidence, remediation: Some(remediation.into()) }
    }

    pub fn warn(evidence: Vec<EvidenceRef>, remediation: impl Into<String>) -> Self {
        Outcome { status: IndicatorStatus::Warn, evidence, remediation: Some(remediation.into()) }
    }

    pub fn unverified(evidence: Vec<EvidenceRef>, remediation: impl Into<String>) -> Self {
        Outcome {
            status: IndicatorStatus::Unverified,
            evidence,
            remediation: Some(remediation.into()),
        }
    }
}
