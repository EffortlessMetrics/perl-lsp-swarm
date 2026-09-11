//! Deterministic projections of one decision (#11633).
//!
//! Human text, guidance sentences, and JSON all derive from the single
//! [`WriterPreflightDecision`] object; there is no second rendering path
//! that could disagree with it (#11633 falsifier 13). Reason tokens render
//! identically to their serde form, so a human transcript and a JSON
//! receipt name the same semantics.

use crate::writer_preflight::decision::{WriterPreflightDecision, WriterPreflightReason};

/// Static, typed guidance for one reason. This is the only place free-form
/// prose exists, and it is keyed by the closed vocabulary — never carried
/// in observations or decisions.
pub fn explain(reason: WriterPreflightReason) -> &'static str {
    match reason {
        WriterPreflightReason::CanonicalCheckoutMutation => {
            "the canonical checkout must not be mutated in place; run the transition in a linked worktree"
        }
        WriterPreflightReason::ProtectedOrDetachedMutation => {
            "HEAD is on a protected branch or detached; check out the exact candidate branch first"
        }
        WriterPreflightReason::WrongOrUnknownRepository => {
            "this checkout is not the requested repository/remote identity"
        }
        WriterPreflightReason::WrongOrUnknownCandidate => {
            "the requested candidate does not match observed candidate identity (missing, moved, or already existing)"
        }
        WriterPreflightReason::BaseOrRemoteNotProven => {
            "the base commit could not be proven current against the expected base"
        }
        WriterPreflightReason::BranchWorktreeMismatch => {
            "branch/worktree registration does not match the requested subject"
        }
        WriterPreflightReason::ReservedLocalRefCollision => {
            "a reserved local ref shadows the requested branch name"
        }
        WriterPreflightReason::SameCandidateCollision => {
            "another active writer already owns this candidate; reuse or resume instead of duplicating"
        }
        WriterPreflightReason::UnresolvedIndexOrMerge => {
            "resolve the unresolved index/merge-conflict state before mutating"
        }
        WriterPreflightReason::UniqueStateAtRisk => {
            "unique unpushed/uncommitted work would be stranded or overwritten; secure it first"
        }
        WriterPreflightReason::AmbientExecutionOverride => {
            "ambient persistent Cargo overrides are set; clear them or route through the executor policy (#9548)"
        }
        WriterPreflightReason::ExecutorConfigurationMismatch => {
            "executor-owned process-local Cargo configuration does not match the declared executor policy"
        }
        WriterPreflightReason::CriticalCapacityBlock => {
            "free capacity is below the selected heavy-build requirement"
        }
        WriterPreflightReason::ProviderUnavailableOrStale => {
            "required evidence is unavailable, unsupported, failed, or stale; refresh the provider before deciding"
        }
        WriterPreflightReason::SafeReadOnlySubject => {
            "read-only subject verified against current evidence"
        }
        WriterPreflightReason::AdvisoryBehindOnly => {
            "candidate is behind its upstream without local divergence; context only, not a denial"
        }
        WriterPreflightReason::AdvisorySharedStashPresent => {
            "a shared stash exists; cleanup paths must never drop it. Context only, not a denial"
        }
        WriterPreflightReason::AdvisoryUnrelatedHostLoad => {
            "unrelated host load observed; context only, not a denial"
        }
    }
}

/// Renders the deterministic human transcript of one decision: outcome,
/// subject digest, and one line per reason (`<token>: <guidance>`), sorted
/// in the decision's canonical reason order.
pub fn render_human(decision: &WriterPreflightDecision) -> String {
    let mut lines = Vec::with_capacity(decision.reasons.len() + 2);
    lines.push(format!(
        "writer-preflight v{}: {} (subject {})",
        decision.schema_version,
        decision.outcome.as_str(),
        decision.subject_digest
    ));
    for reason in &decision.reasons {
        lines.push(format!("  {}: {}", reason.as_str(), explain(*reason)));
    }
    let mut text = lines.join("\n");
    text.push('\n');
    text
}
