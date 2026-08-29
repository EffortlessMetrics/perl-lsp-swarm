//! Normalized stack-local result fan-in (`stack_increment_result.v1`).
//!
//! One stable advisory result binds exactly one compiled subject and one
//! compiled `stack_local` route plan to ordered observations. Rows live in a
//! canonical keyed map so insertion order can never change serialized bytes,
//! parent prerequisite state stays visible next to the child increment, and
//! every green classification requires executed, identity-bound run rows.

use super::plan::STACK_LOCAL_PROFILE;
use super::subject::{refresh_currentness, subject_digest, validate_subject};
use super::{
    PROTECTED_MAIN_NOT_EVALUATED, STACK_ADVISORY_CONTEXT_NAME, STACK_INCREMENT_PRODUCER,
    STACK_INCREMENT_RESULT_SCHEMA, StackIncrementSubjectV1, validate_nonempty,
};
use crate::ci_route_plan::{CiRoutePlanV1, PlannedOutcome};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Closed refusal-code vocabulary emitted by result compilation.
pub const STACK_RESULT_ERROR_CODES: &[&str] = &[
    "malformed_result",
    "protected_main_promotion",
    "plan_unbound_to_subject",
    "plan_digest_mismatch",
    "plan_untrusted",
    "observation_identity_mismatch",
    "run_identity_missing",
    "run_evidence_contradicted",
    "unplanned_observation",
];

/// The row-error code reserved for instruments that failed before producing
/// domain evidence. Route-plan lifecycle/projection errors pass through under
/// their own #4789 vocabulary unchanged.
pub const INSTRUMENT_FAILED_ERROR_CODE: &str = "instrument_failed";

/// Typed result-compilation refusal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StackResultCompileError {
    /// Closed refusal-code identity from [`STACK_RESULT_ERROR_CODES`].
    pub code: String,
    /// Human-readable explanation naming the refused fact.
    pub message: String,
}

impl std::fmt::Display for StackResultCompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for StackResultCompileError {}

fn refuse_result(code: &str, message: impl Into<String>) -> StackResultCompileError {
    debug_assert!(STACK_RESULT_ERROR_CODES.contains(&code), "unknown refusal code {code}");
    StackResultCompileError { code: code.to_string(), message: message.into() }
}

/// Independent parent-train prerequisite visibility. A child row passing can
/// never erase these facts from the published result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParentPrerequisiteState {
    /// The parent train node reports success.
    Passed,
    /// The parent train node reports failure.
    Failed,
    /// The parent train node is still running.
    Pending,
    /// No parent prerequisite evidence was supplied to this result.
    NotReported,
}

/// Aggregated child-increment status over this result's selected rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChildIncrementStatus {
    /// Every selected run row executed and passed with bound identities.
    CurrentSuccess,
    /// At least one selected run row failed on its own exit evidence.
    CurrentFailure,
    /// Selected run rows still await observations.
    Pending,
    /// Every selected row proved its own non-engagement or quarantine, and
    /// the child delta is empty.
    ScopedNoop,
    /// An executed instrument failed before producing domain evidence.
    InstrumentFailed,
    /// Selection cannot prove sufficiency or carries typed error rows.
    NotProven,
}

/// Advisory context status exactly mirroring the issue vocabulary. A green
/// value here never satisfies protected-main merge readiness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextStatus {
    /// Child increment proved current success and the parent prerequisite
    /// reported pass.
    CurrentSuccess,
    /// The child increment has failing run rows.
    CurrentFailure,
    /// Selected run rows await observations.
    Pending,
    /// All selected rows scoped themselves out against an empty delta.
    ScopedNoop,
    /// The parent prerequisite is failed or pending: stack-local progress is
    /// blocked regardless of the child increment.
    BlockedParent,
    /// The bound subject moved since admission; the result is not current.
    Stale,
    /// An executed instrument failed before producing domain evidence.
    InstrumentFailed,
    /// Nothing sufficient was proven.
    NotProven,
}

/// Executable identity one run observation must match exactly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunIdentity {
    /// Command the plan pinned for this gate.
    pub command: String,
    /// Timeout the plan pinned for this gate; must be positive.
    pub timeout_seconds: u64,
}

/// One bounded execution observation supplied by any read-only runner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StackObservation {
    /// Gate the observation belongs to.
    pub gate_id: String,
    /// Execution identity claimed for the run; must equal the plan row's.
    pub identity: RunIdentity,
    /// Whether the run exited successfully.
    pub passed: bool,
    /// Process exit code when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    /// Digest of captured output; required whenever `passed` is true, so a
    /// metadata-only green can never satisfy a semantic run row.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_digest: Option<String>,
    /// True when the instrument itself failed without producing domain
    /// evidence; such a row can never aggregate to success.
    pub instrument_failed: bool,
}

/// Canonical result row. Variants mirror the shared route-planner outcomes
/// plus executed run observations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum StackRowResult {
    /// Executed, identity-matched, exited zero, output digest present.
    RunPassed {
        /// Digest of the captured run output.
        output_digest: String,
    },
    /// Executed and exited non-zero.
    RunFailed {
        /// Non-zero process exit code.
        exit_code: i32,
    },
    /// Planned run row without any observation yet.
    Pending,
    /// Positive selector proof established non-engagement.
    ScopedNoop {
        /// Selector-supplied reason.
        reason: String,
        /// Selector-evidence digest backing the no-op.
        selector_digest: String,
    },
    /// Lifecycle-governed quarantine carried visibly.
    Quarantined {
        /// Quarantine reason token.
        reason: String,
        /// Quarantine owner.
        owner: String,
        /// Optional owning issue reference.
        #[serde(skip_serializing_if = "Option::is_none")]
        owner_issue: Option<String>,
        /// Review horizon date (`YYYY-MM-DD`).
        review_after: String,
    },
    /// Visible typed error row; instrument failures carry code
    /// `instrument_failed`.
    Error {
        /// Closed error-code identity.
        code: String,
        /// Bounded explanation.
        message: String,
    },
}

/// Compile input joining subject, plan, plan digest, and observations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StackResultInput {
    /// Current validated subject at result time.
    pub subject: StackIncrementSubjectV1,
    /// Compiled stack-local plan bound to that subject.
    pub plan: CiRoutePlanV1,
    /// [`super::plan::stack_plan_digest`] of the same plan, recomputed by
    /// the caller and cross-checked here.
    pub plan_digest: String,
    /// Observations for run rows, in any order.
    pub observations: Vec<StackObservation>,
    /// Parent prerequisite evidence; `None` records `NotReported`.
    pub parent_state: Option<ParentPrerequisiteState>,
    /// Parent head SHA currently observed; must match the subject or the
    /// result classifies stale.
    pub observed_parent_head_sha: String,
    /// Child head SHA currently observed; must match the subject or the
    /// result classifies stale.
    pub observed_child_head_sha: String,
    /// Reproduce command surfaced verbatim in explanations.
    pub reproduce_command: String,
}

/// Compiled advisory result (`stack_increment_result.v1`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StackIncrementResultV1 {
    /// Contract identity.
    pub schema: String,
    /// Producer identity.
    pub producer: String,
    /// Stable advisory context name.
    pub context_name: String,
    /// Exact repository of both endpoints.
    pub repository: String,
    /// Parent PR number.
    pub parent_pr_number: u64,
    /// Child PR number.
    pub child_pr_number: u64,
    /// Subject digest this result binds to.
    pub subject_digest: String,
    /// Plan digest this result binds to.
    pub route_plan_digest: String,
    /// Canonical keyed rows: ordering can never change serialized bytes.
    pub rows: BTreeMap<String, StackRowResult>,
    /// Positive count of governed denominator rows copied from the bound plan.
    /// Artifact validation requires this count to equal `rows.len()`, so an
    /// empty hand-authored row map cannot masquerade as a scoped no-op.
    pub planned_gate_count: u64,
    /// Independent parent prerequisite visibility.
    pub parent_prerequisite_state: ParentPrerequisiteState,
    /// Aggregated child increment status.
    pub child_increment_status: ChildIncrementStatus,
    /// Published advisory context status.
    pub context_status: ContextStatus,
    /// Number of child-only delta path rows behind the selection.
    pub delta_path_count: u64,
    /// Reason recorded when staleness forced the context stale.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub staleness_reason: Option<String>,
    /// Permanent protected-main state of this artifact.
    pub protected_main_state: String,
    /// Reproduce command surfaced verbatim.
    pub reproduce_command: String,
}

fn find_run_command<'a>(plan: &'a CiRoutePlanV1, gate_id: &str) -> Option<(&'a str, u64)> {
    plan.rows.iter().find_map(|row| match &row.outcome {
        PlannedOutcome::Run { command, timeout_seconds, .. } if row.gate_id == gate_id => {
            Some((command.as_str(), *timeout_seconds))
        }
        _ => None,
    })
}

fn row_from_outcome(outcome: &PlannedOutcome) -> StackRowResult {
    match outcome {
        PlannedOutcome::Run { .. } => StackRowResult::Pending,
        PlannedOutcome::ScopedNoop { reason, selector_digest } => StackRowResult::ScopedNoop {
            reason: reason.clone(),
            selector_digest: selector_digest.clone(),
        },
        PlannedOutcome::Quarantined { reason, owner, owner_issue, review_after } => {
            StackRowResult::Quarantined {
                reason: reason.clone(),
                owner: owner.clone(),
                owner_issue: owner_issue.clone(),
                review_after: review_after.clone(),
            }
        }
        PlannedOutcome::Error { code, message } => {
            StackRowResult::Error { code: code.clone(), message: message.clone() }
        }
    }
}

/// Row-class counters shared by compile-time aggregation and artifact
/// revalidation so both surfaces classify published facts identically.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct RowClassCounts {
    succeeded_runs: u64,
    failed_runs: u64,
    pending_runs: u64,
    instrument_errors: u64,
    other_errors: u64,
    disengaged_rows: u64,
}

/// Aggregate the row-class counters into the child-increment status. The one
/// authority shared by compilation and artifact revalidation.
fn classify_child_increment(counts: RowClassCounts, delta_changed: bool) -> ChildIncrementStatus {
    let RowClassCounts {
        succeeded_runs,
        failed_runs,
        pending_runs,
        instrument_errors,
        other_errors,
        disengaged_rows,
    } = counts;
    if instrument_errors > 0 {
        ChildIncrementStatus::InstrumentFailed
    } else if other_errors > 0 {
        ChildIncrementStatus::NotProven
    } else if failed_runs > 0 {
        ChildIncrementStatus::CurrentFailure
    } else if pending_runs > 0 {
        ChildIncrementStatus::Pending
    } else if succeeded_runs > 0 {
        ChildIncrementStatus::CurrentSuccess
    } else {
        debug_assert!(
            !delta_changed || disengaged_rows == 0,
            "non-empty delta with only disengagement rows is refused before classification"
        );
        ChildIncrementStatus::ScopedNoop
    }
}

/// Publish the advisory context status from the independent facts. The one
/// authority shared by compilation and artifact revalidation.
fn classify_context_status(
    stale: bool,
    parent_state: ParentPrerequisiteState,
    child_status: ChildIncrementStatus,
) -> ContextStatus {
    if stale {
        ContextStatus::Stale
    } else if matches!(
        parent_state,
        ParentPrerequisiteState::Failed | ParentPrerequisiteState::Pending
    ) {
        ContextStatus::BlockedParent
    } else if child_status == ChildIncrementStatus::CurrentSuccess
        && parent_state != ParentPrerequisiteState::Passed
    {
        // A green advisory requires a reported parent pass (#13360 root
        // cause 3): missing prerequisite evidence is not proven, so a
        // passing child can never publish success on its own.
        ContextStatus::NotProven
    } else {
        match child_status {
            ChildIncrementStatus::CurrentSuccess => ContextStatus::CurrentSuccess,
            ChildIncrementStatus::CurrentFailure => ContextStatus::CurrentFailure,
            ChildIncrementStatus::Pending => ContextStatus::Pending,
            ChildIncrementStatus::ScopedNoop => ContextStatus::ScopedNoop,
            ChildIncrementStatus::InstrumentFailed => ContextStatus::InstrumentFailed,
            ChildIncrementStatus::NotProven => ContextStatus::NotProven,
        }
    }
}

/// Validate every cross-field invariant of a compiled result artifact. Pure.
///
/// This is the semantic result validator: it recomputes the aggregate
/// classification from the artifact's own rows and refuses whenever the
/// stored statuses do not reconcile, so a hand-authored artifact with
/// arbitrary digests, empty or failing rows, or a forged green status cannot
/// pass validation or drive `explain` to success (#13360 root cause 5).
///
/// # Errors
/// Returns a typed refusal for identity drift, digest-shape failures,
/// malformed rows, and any aggregate/status mismatch.
pub fn validate_result(result: &StackIncrementResultV1) -> Result<(), StackResultCompileError> {
    if result.schema != STACK_INCREMENT_RESULT_SCHEMA {
        return Err(refuse_result(
            "malformed_result",
            format!("unsupported schema {:?}", result.schema),
        ));
    }
    if result.producer != STACK_INCREMENT_PRODUCER {
        return Err(refuse_result(
            "malformed_result",
            format!("unsupported producer {:?}", result.producer),
        ));
    }
    if result.context_name != STACK_ADVISORY_CONTEXT_NAME {
        return Err(refuse_result(
            "malformed_result",
            format!("unsupported advisory context {:?}", result.context_name),
        ));
    }
    if result.protected_main_state != PROTECTED_MAIN_NOT_EVALUATED {
        return Err(refuse_result(
            "protected_main_promotion",
            format!(
                "protected-main state {:?} is not permitted on a stack-local result; the only \
                 legal value is {PROTECTED_MAIN_NOT_EVALUATED}",
                result.protected_main_state
            ),
        ));
    }
    validate_nonempty("repository", &result.repository)
        .map_err(|message| refuse_result("malformed_result", message))?;
    if result.parent_pr_number == 0 || result.child_pr_number == 0 {
        return Err(refuse_result(
            "malformed_result",
            "parent and child PR numbers must be positive",
        ));
    }
    super::validate_sha256_hex("subject digest", &result.subject_digest)
        .map_err(|message| refuse_result("malformed_result", message))?;
    super::validate_sha256_hex("route plan digest", &result.route_plan_digest)
        .map_err(|message| refuse_result("malformed_result", message))?;
    validate_nonempty("reproduce command", &result.reproduce_command)
        .map_err(|message| refuse_result("malformed_result", message))?;
    if result.planned_gate_count == 0 {
        return Err(refuse_result(
            "malformed_result",
            "bound route-plan denominator must contain at least one gate",
        ));
    }
    let planned_gate_count = usize::try_from(result.planned_gate_count).map_err(|_| {
        refuse_result(
            "malformed_result",
            format!("bound route-plan denominator is too large: {}", result.planned_gate_count),
        )
    })?;
    if result.rows.len() != planned_gate_count {
        return Err(refuse_result(
            "malformed_result",
            format!(
                "result carries {} rows for a bound denominator of {} gates",
                result.rows.len(),
                result.planned_gate_count
            ),
        ));
    }

    let mut counts = RowClassCounts::default();
    for (gate_id, row) in &result.rows {
        validate_nonempty("gate id", gate_id)
            .map_err(|message| refuse_result("malformed_result", message))?;
        match row {
            StackRowResult::RunPassed { output_digest } => {
                validate_nonempty("output digest", output_digest)
                    .map_err(|message| refuse_result("malformed_result", message))?;
                counts.succeeded_runs += 1;
            }
            StackRowResult::RunFailed { exit_code } => {
                if *exit_code == 0 {
                    return Err(refuse_result(
                        "run_evidence_contradicted",
                        format!("gate {gate_id:?} carries a run-failed row with exit code 0"),
                    ));
                }
                counts.failed_runs += 1;
            }
            StackRowResult::Pending => counts.pending_runs += 1,
            StackRowResult::ScopedNoop { reason, selector_digest } => {
                validate_nonempty("scoped-noop reason", reason)
                    .map_err(|message| refuse_result("malformed_result", message))?;
                validate_nonempty("selector digest", selector_digest)
                    .map_err(|message| refuse_result("malformed_result", message))?;
                counts.disengaged_rows += 1;
            }
            StackRowResult::Quarantined { reason, owner, owner_issue, review_after } => {
                validate_nonempty("quarantine reason", reason)
                    .map_err(|message| refuse_result("malformed_result", message))?;
                validate_nonempty("quarantine owner", owner)
                    .map_err(|message| refuse_result("malformed_result", message))?;
                validate_nonempty("review horizon", review_after)
                    .map_err(|message| refuse_result("malformed_result", message))?;
                if let Some(issue) = owner_issue {
                    validate_nonempty("owner issue", issue)
                        .map_err(|message| refuse_result("malformed_result", message))?;
                }
                counts.disengaged_rows += 1;
            }
            StackRowResult::Error { code, message } => {
                validate_nonempty("error code", code)
                    .map_err(|message| refuse_result("malformed_result", message))?;
                validate_nonempty("error message", message)
                    .map_err(|message| refuse_result("malformed_result", message))?;
                if code == INSTRUMENT_FAILED_ERROR_CODE {
                    counts.instrument_errors += 1;
                } else {
                    counts.other_errors += 1;
                }
            }
        }
    }

    // Artifact-surface mirror of the compile-time sufficiency refusal: a
    // changed delta whose rows only ever prove disengagement can never exist
    // as a valid published result.
    let delta_changed = result.delta_path_count > 0;
    if delta_changed
        && counts.succeeded_runs == 0
        && counts.failed_runs == 0
        && counts.pending_runs == 0
        && counts.instrument_errors == 0
        && counts.other_errors == 0
        && counts.disengaged_rows > 0
    {
        return Err(refuse_result(
            "malformed_result",
            "insufficient selector evidence: the recorded delta changed paths while every row \
             only proved non-engagement",
        ));
    }

    let recomputed_child = classify_child_increment(counts, delta_changed);
    if recomputed_child != result.child_increment_status {
        return Err(refuse_result(
            "malformed_result",
            format!(
                "rows aggregate to child increment status {recomputed_child:?} but the artifact \
                 claims {:?}",
                result.child_increment_status
            ),
        ));
    }
    let recomputed_context = classify_context_status(
        result.staleness_reason.is_some(),
        result.parent_prerequisite_state,
        recomputed_child,
    );
    if recomputed_context != result.context_status {
        return Err(refuse_result(
            "malformed_result",
            format!(
                "independent states classify to context status {recomputed_context:?} but the \
                 artifact claims {:?}",
                result.context_status
            ),
        ));
    }
    Ok(())
}

/// Compile, bind, and normalize one advisory stack-local result.
///
/// # Errors
/// Returns typed refusals for subject/plan binding failures, plan digest
/// drift, unplanned or identity-mismatched observations, missing run
/// identity digests, and protected-main promotion attempts.
pub fn compile_result(
    input: StackResultInput,
) -> Result<StackIncrementResultV1, StackResultCompileError> {
    validate_nonempty("reproduce command", &input.reproduce_command)
        .map_err(|message| refuse_result("malformed_result", message))?;
    // The plan is publicly deserializable and its digest is unkeyed, so a
    // matching digest alone proves nothing: the plan must first pass its own
    // semantic validator and carry the exact stack-local profile before any
    // digest bound to it can be trusted (#13360 root cause 2).
    input.plan.validate().map_err(|message| {
        refuse_result("plan_untrusted", format!("route plan refuses: {message}"))
    })?;
    if input.plan.requested_profile != STACK_LOCAL_PROFILE {
        return Err(refuse_result(
            "plan_untrusted",
            format!(
                "route plan profile {:?} is not {STACK_LOCAL_PROFILE}; only the stack-local \
                 profile can back a stack result",
                input.plan.requested_profile
            ),
        ));
    }
    let expected_digest = subject_digest(&input.subject);
    let plan_digest_recomputed = super::plan::stack_plan_digest(&input.plan)
        .map_err(|error| refuse_result("plan_digest_mismatch", error.to_string()))?;
    let planned_gate_count = u64::try_from(input.plan.denominator.len()).map_err(|_| {
        refuse_result(
            "malformed_result",
            "route-plan denominator cannot be represented in a result artifact",
        )
    })?;
    if plan_digest_recomputed != input.plan_digest {
        return Err(refuse_result(
            "plan_digest_mismatch",
            format!(
                "supplied plan digest {} does not reconcile with the compiled plan",
                input.plan_digest
            ),
        ));
    }
    if input.plan.subject.subject_digest != expected_digest {
        return Err(refuse_result(
            "plan_unbound_to_subject",
            "supplied plan does not bind to the supplied subject digest",
        ));
    }
    validate_subject(&input.subject)
        .map_err(|error| refuse_result("malformed_result", error.to_string()))?;
    let staleness_reason = refresh_currentness(
        &input.subject,
        &input.observed_parent_head_sha,
        &input.observed_child_head_sha,
    )
    .err()
    .map(|error| error.to_string());

    // Observations may attach only to planned run rows, and only with the
    // plan-pinned executable identity. Anything else refuses: metadata-only
    // greens cannot satisfy semantic rows by substitution.
    let mut observations_by_gate: BTreeMap<String, &StackObservation> = BTreeMap::new();
    for observation in &input.observations {
        let gate_has_run_row = find_run_command(&input.plan, &observation.gate_id).is_some();
        if !gate_has_run_row {
            return Err(refuse_result(
                "unplanned_observation",
                format!(
                    "gate {:?} received an observation but holds no planned run row",
                    observation.gate_id
                ),
            ));
        }
        if observations_by_gate.insert(observation.gate_id.clone(), observation).is_some() {
            return Err(refuse_result(
                "unplanned_observation",
                format!("gate {:?} received more than one observation", observation.gate_id),
            ));
        }
    }

    let mut rows: BTreeMap<String, StackRowResult> = BTreeMap::new();
    let mut succeeded_runs = 0u64;
    let mut failed_runs = 0u64;
    let mut pending_runs = 0u64;
    let mut instrument_errors = 0u64;
    let mut other_errors = 0u64;
    let mut disengaged_rows = 0u64;

    for row in &input.plan.rows {
        let planned_outcome = &row.outcome;
        let compiled_row = match planned_outcome {
            PlannedOutcome::Run { command, timeout_seconds, .. } => {
                match observations_by_gate.remove(&row.gate_id) {
                    None => {
                        pending_runs += 1;
                        StackRowResult::Pending
                    }
                    Some(observation) => {
                        if observation.identity.command != *command
                            || observation.identity.timeout_seconds != *timeout_seconds
                        {
                            return Err(refuse_result(
                                "observation_identity_mismatch",
                                format!(
                                    "gate {:?} observation claims command {:?}/{}s but the \
                                     plan pinned {:?}/{}s",
                                    row.gate_id,
                                    observation.identity.command,
                                    observation.identity.timeout_seconds,
                                    command,
                                    timeout_seconds
                                ),
                            ));
                        }
                        if observation.instrument_failed {
                            instrument_errors += 1;
                            StackRowResult::Error {
                                code: INSTRUMENT_FAILED_ERROR_CODE.to_string(),
                                message: format!(
                                    "gate {:?} instrument failed before producing domain \
                                     evidence",
                                    row.gate_id
                                ),
                            }
                        } else if observation.passed {
                            // A passing row requires zero-exit evidence
                            // (#13360 root cause 4): a `passed` claim
                            // contradicted by a non-zero exit code, or
                            // unsupported by any exit code, is contradictory
                            // runner evidence and never counts as success.
                            match observation.exit_code {
                                Some(0) => {}
                                Some(exit_code) => {
                                    return Err(refuse_result(
                                        "run_evidence_contradicted",
                                        format!(
                                            "gate {:?} claims a passing run with exit code \
                                             {exit_code}; contradictory process evidence is \
                                             not proven",
                                            row.gate_id
                                        ),
                                    ));
                                }
                                None => {
                                    return Err(refuse_result(
                                        "run_evidence_contradicted",
                                        format!(
                                            "gate {:?} claims a passing run without exit \
                                             evidence; a zero exit is required before a row \
                                             can count as passed",
                                            row.gate_id
                                        ),
                                    ));
                                }
                            }
                            let Some(output_digest) = &observation.output_digest else {
                                return Err(refuse_result(
                                    "run_identity_missing",
                                    format!(
                                        "gate {:?} claims a passing run without an output \
                                         digest; metadata-only greens cannot satisfy semantic \
                                         run rows",
                                        row.gate_id
                                    ),
                                ));
                            };
                            if output_digest.trim().is_empty() {
                                return Err(refuse_result(
                                    "run_identity_missing",
                                    format!(
                                        "gate {:?} carries an empty output digest",
                                        row.gate_id
                                    ),
                                ));
                            }
                            succeeded_runs += 1;
                            StackRowResult::RunPassed { output_digest: output_digest.clone() }
                        } else {
                            match observation.exit_code {
                                Some(exit_code) if exit_code != 0 => {
                                    failed_runs += 1;
                                    StackRowResult::RunFailed { exit_code }
                                }
                                _ => {
                                    instrument_errors += 1;
                                    StackRowResult::Error {
                                        code: INSTRUMENT_FAILED_ERROR_CODE.to_string(),
                                        message: format!(
                                            "gate {:?} reported failure without a usable exit \
                                             code",
                                            row.gate_id
                                        ),
                                    }
                                }
                            }
                        }
                    }
                }
            }
            PlannedOutcome::ScopedNoop { .. } => {
                disengaged_rows += 1;
                row_from_outcome(planned_outcome)
            }
            PlannedOutcome::Quarantined { .. } => {
                disengaged_rows += 1;
                row_from_outcome(planned_outcome)
            }
            PlannedOutcome::Error { code, .. } => {
                if code == INSTRUMENT_FAILED_ERROR_CODE {
                    instrument_errors += 1;
                } else {
                    other_errors += 1;
                }
                row_from_outcome(planned_outcome)
            }
        };
        rows.insert(row.gate_id.clone(), compiled_row);
    }

    if !observations_by_gate.is_empty() {
        let leftover = observations_by_gate.into_keys().next().unwrap_or_default();
        return Err(refuse_result(
            "unplanned_observation",
            format!("gate {leftover:?} received an observation outside every planned run row"),
        ));
    }

    // Fail-closed sufficiency: a non-empty child delta whose entire governed
    // selection only ever proves its own disengagement has no sufficient
    // positive proof, so it can never publish as scoped green.
    let delta_changed = !input.subject.delta.paths.is_empty();
    if delta_changed
        && succeeded_runs == 0
        && failed_runs == 0
        && pending_runs == 0
        && instrument_errors == 0
        && other_errors == 0
        && disengaged_rows > 0
    {
        return Err(refuse_result(
            "malformed_result",
            "insufficient selector evidence: the child delta changed paths while every \
             selected row only proved non-engagement; admit runnable gates covering the \
             delta instead of publishing an empty-plan green",
        ));
    }

    let child_increment_status = classify_child_increment(
        RowClassCounts {
            succeeded_runs,
            failed_runs,
            pending_runs,
            instrument_errors,
            other_errors,
            disengaged_rows,
        },
        delta_changed,
    );

    let parent_prerequisite_state =
        input.parent_state.unwrap_or(ParentPrerequisiteState::NotReported);
    let context_status = classify_context_status(
        staleness_reason.is_some(),
        parent_prerequisite_state,
        child_increment_status,
    );

    Ok(StackIncrementResultV1 {
        schema: STACK_INCREMENT_RESULT_SCHEMA.to_string(),
        producer: STACK_INCREMENT_PRODUCER.to_string(),
        context_name: STACK_ADVISORY_CONTEXT_NAME.to_string(),
        repository: input.subject.repository.clone(),
        parent_pr_number: input.subject.parent.pr_number,
        child_pr_number: input.subject.child.pr_number,
        subject_digest: expected_digest,
        route_plan_digest: input.plan_digest,
        rows,
        planned_gate_count,
        parent_prerequisite_state,
        child_increment_status,
        context_status,
        delta_path_count: input.subject.delta.paths.len() as u64,
        staleness_reason,
        protected_main_state: PROTECTED_MAIN_NOT_EVALUATED.to_string(),
        reproduce_command: input.reproduce_command,
    })
}

/// Render the stable human explanation required on every published advisory
/// context: subjects, delta surface, selected rows, parent prerequisite
/// state, the permanent protected-main line, and the reproduce command.
#[must_use]
pub fn render_explanation(result: &StackIncrementResultV1) -> String {
    let mut text = String::new();
    text.push_str(&format!(
        "# {}\n\nContext status: {:?}\n\n## Subjects\n\n- repository: {}\n- parent PR: #{}\n- \
         child PR: #{}\n- subject digest: `{}`\n- route plan digest: `{}`\n- bound denominator gates: {}\n\n## Delta\n\nchild-\
         only changed paths: {}\n\n## Selected proof rows\n\n",
        result.context_name,
        result.context_status,
        result.repository,
        result.parent_pr_number,
        result.child_pr_number,
        result.subject_digest,
        result.route_plan_digest,
        result.planned_gate_count,
        result.delta_path_count,
    ));
    for (gate_id, row) in &result.rows {
        let line = match row {
            StackRowResult::RunPassed { .. } => format!("- `{gate_id}`: run passed"),
            StackRowResult::RunFailed { exit_code } => {
                format!("- `{gate_id}`: run failed (exit {exit_code})")
            }
            StackRowResult::Pending => format!("- `{gate_id}`: pending"),
            StackRowResult::ScopedNoop { .. } => format!("- `{gate_id}`: scoped no-op"),
            StackRowResult::Quarantined { owner, .. } => {
                format!("- `{gate_id}`: quarantined (owner {owner})")
            }
            StackRowResult::Error { code, message } => {
                format!("- `{gate_id}`: error {code}: {message}")
            }
        };
        text.push_str(&line);
        text.push('\n');
    }
    text.push_str(&format!(
        "\n## Independent states\n\n- parent prerequisite: {:?}\n- child increment: {:?}\n\n\
         Protected-main: {}\n\nReproduce:\n\n```sh\n{}\n```\n",
        result.parent_prerequisite_state,
        result.child_increment_status,
        result.protected_main_state,
        result.reproduce_command,
    ));
    if let Some(reason) = &result.staleness_reason {
        text.push_str(&format!("\nStaleness: {reason}\n"));
    }
    text
}
