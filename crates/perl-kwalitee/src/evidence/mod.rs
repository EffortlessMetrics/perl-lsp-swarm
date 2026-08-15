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

/// Receipt freshness predicate, shared by every receipt-backed evidence reader.
///
/// A receipt is stale when it carries a non-empty commit SHA that does not match
/// the current expected commit. An empty or `"unknown"` expected commit means the
/// current commit could not be resolved, so no downgrade is applied (the receipt
/// is trusted as-is). An empty receipt commit means the generator did not stamp
/// one, which is also trusted rather than downgraded — generators that omit the
/// stamp should be fixed separately.
///
/// Used by [`readiness`], [`quality_gate`], and [`nightly`] so the same rule is
/// applied uniformly and a fourth reader cannot silently omit it.
pub(crate) fn is_stale(receipt_commit: &str, expected_commit: &str) -> bool {
    !expected_commit.is_empty()
        && expected_commit != "unknown"
        && !receipt_commit.is_empty()
        && receipt_commit != expected_commit
}
