//! Typed normalized result for one planned gate run (`routed_gate_result.v1`,
//! #9156).
//!
//! One accepted `ci_route_plan.v1` (#10179) `run` row produces exactly one
//! versioned, validated result record. This module owns the typed payload,
//! the closed outcome vocabulary, the builder that projects an execution
//! observation into it, domain-separated fingerprinting over the same
//! canonical byte encoding family as #10179, and durable single-writer
//! publication.
//!
//! Authorities consumed (never re-derived here):
//!
//! - `ci_route_plan.v1` (#10179): exact plan/row identity — subject,
//!   fingerprint, digests, denominator, profile expansion, and the planned
//!   run row (gate id, native tier, policy role, lifecycle, applicability,
//!   command, timeout) are copied verbatim from the validated plan;
//! - `gate_disposition.v1` / `ci_route_profile.v1` (#10176/#10178) reach
//!   this payload only through the compiled plan, never directly;
//! - the gate runner (one-gate invocation, receipts, logs) supplies
//!   [`RunObservation`] facts; nothing in this module re-runs a command or
//!   reinterprets its output.
//!
//! The result reports what happened when the row was attempted. It never
//! decides selector applicability, lifecycle disposition, or live GitHub
//! enforcement: `not_applicable` is a plan-side `scoped_noop`, not a C1
//! execution outcome. The three verdict planes stay independent:
//!
//! ```text
//! product_result    did the gate command establish a product verdict?
//! instrument_result were prerequisites/process/receipt instruments valid?
//! reporting_result  were logs/receipts/publication reported correctly?
//! ```

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::ci_route_plan::{
    Applicability, CI_ROUTE_PLAN_SCHEMA, CiRoutePlanV1, LifecycleDisposition, PlannedOutcome,
    PolicyRole, SelectorRole, canonical_json, deserialize_option_reject_null,
};

/// Domain contract identity of this payload.
pub const ROUTED_GATE_RESULT_SCHEMA: &str = "routed_gate_result.v1";
pub const ROUTED_GATE_RESULT_PRODUCER: &str = "xtask::routed_result";

/// Domain separation prefix for the fingerprint preimage, mirroring the
/// frozen #10179 spelling style:
/// `SHA-256("routed_gate_result.v1\0" || canonical_semantic_bytes)`.
pub const FINGERPRINT_DOMAIN: &[u8] = b"routed_gate_result.v1\0";

// ---------------------------------------------------------------------------
// Closed outcome vocabulary
// ---------------------------------------------------------------------------

/// Closed terminal outcome of one plane. Every variant keeps the fact it
/// names visible; mis-attribution classes the issue lists cannot be encoded
/// at all ([`build_routed_result`] refuses them).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalOutcome {
    Success,
    Failure,
    Timeout,
    Cancelled,
    BlockedNotProven,
    InstrumentFailure,
    Missing,
    Stale,
    NotProven,
}

impl TerminalOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            TerminalOutcome::Success => "success",
            TerminalOutcome::Failure => "failure",
            TerminalOutcome::Timeout => "timeout",
            TerminalOutcome::Cancelled => "cancelled",
            TerminalOutcome::BlockedNotProven => "blocked_not_proven",
            TerminalOutcome::InstrumentFailure => "instrument_failure",
            TerminalOutcome::Missing => "missing",
            TerminalOutcome::Stale => "stale",
            TerminalOutcome::NotProven => "not_proven",
        }
    }

    /// A product verdict exists only when the product command reached and
    /// settled on one of these outcomes; everything else carries no claim.
    pub fn is_product_verdict(self) -> bool {
        matches!(
            self,
            TerminalOutcome::Success | TerminalOutcome::Failure | TerminalOutcome::Timeout
        )
    }
}

/// One independent verdict plane with bounded detail. Detail states what was
/// observed, not an interpretation that could overwrite another plane.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlaneOutcome {
    pub outcome: TerminalOutcome,
    #[serde(default)]
    pub detail: String,
}

/// Prerequisite states preserved independently from later product children
/// (#8063/#9577/#10160 class separation).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrerequisiteState {
    Ready,
    Failed,
    Missing,
    Stale,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrerequisiteEvidence {
    pub state: PrerequisiteState,
    /// Named artifacts whose absence blocked the command (`perllsp`,
    /// fixtures, toolchain pieces), when the state is not ready.
    #[serde(default)]
    pub missing_artifacts: Vec<String>,
    /// Dependency-gate identities observed before this row and their
    /// recorded dispositions. Ordered by construction (map).
    #[serde(default)]
    pub dependency_gates: BTreeMap<String, String>,
}

/// Child process terminal observation, verbatim from the runner. A signal /
/// timeout kill stays visibly distinct from a clean exit here.
///
/// `in_process` marks a gate the runner executed in its own process
/// (internal task dispatch): there was no child process, so `exit_code`
/// stays `None` and process-terminal facts (signal/timeout/cancel) cannot
/// coexist with it — the validator refuses that shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChildObservation {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_option_reject_null"
    )]
    pub exit_code: Option<i32>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_option_reject_null"
    )]
    pub signal: Option<String>,
    pub timed_out: bool,
    pub cancelled: bool,
    #[serde(default)]
    pub in_process: bool,
}

/// Timing observation in UNIX milliseconds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservationTiming {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_option_reject_null"
    )]
    pub started_at_unix_ms: Option<i64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_option_reject_null"
    )]
    pub ended_at_unix_ms: Option<i64>,
    pub duration_ms: u64,
}

/// Hosted CI identity binding. Required whenever the row ran on hosted CI;
/// absent offline, where no workflow identity may be invented.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostedIdentity {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_option_reject_null"
    )]
    pub workflow: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_option_reject_null"
    )]
    pub job: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_option_reject_null"
    )]
    pub run_id: Option<String>,
    pub run_attempt: u64,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_option_reject_null"
    )]
    pub matrix: Option<String>,
}

/// Bounded reference to one log/receipt/artifact produced by the run.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactRef {
    pub role: String,
    pub path: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_option_reject_null"
    )]
    pub sha256: Option<String>,
}

/// The runner-observed facts feeding [`build_routed_result`]. Nothing here
/// encodes a verdict: verdicts are projected by the builder from these facts
/// plus the validated plan row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum RoutedReaderGateStatus {
    Pass,
    Fail,
    Timeout,
    CancelledAfterStart,
    SpawnErrorBeforeStart,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunObservation {
    pub runner_status: RoutedReaderGateStatus,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_option_reject_null"
    )]
    pub hosted: Option<HostedIdentity>,
    /// Pre-command prerequisite evidence. `None` means the runner could not
    /// observe prerequisite state at all (never-started commands), which the
    /// builder records as `Missing`, never as an assumed-ready fact.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_option_reject_null"
    )]
    pub prerequisites: Option<PrerequisiteEvidence>,
    pub command_started: bool,
    pub child: ChildObservation,
    pub timing: ObservationTiming,
    pub artifacts: Vec<ArtifactRef>,
    /// Named receipt/reporting identities that were expected for a completed
    /// command but are absent or unreadable. Non-empty forces the reporting
    /// plane below success; it never alters the product plane.
    #[serde(default)]
    pub receipt_shortfall: Vec<String>,
}

// ---------------------------------------------------------------------------
// Published payload
// ---------------------------------------------------------------------------

/// Exact subject reference, mirrored from the consumed plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResultSubjectRef {
    pub kind: String,
    pub head_sha: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_option_reject_null"
    )]
    pub base_sha: Option<String>,
    pub subject_digest: String,
}

/// The consumed plan's authority identity, carried so downstream consumers
/// (#9157/#9159/#9161) can verify provenance without recompiling plans.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanAuthorityIdentity {
    pub schema: String,
    pub semantic_fingerprint: String,
    pub requested_profile: String,
    pub included_native_tiers: Vec<String>,
    pub expansion_fingerprint: String,
    pub policy_digest: String,
    pub disposition_digest: String,
    pub workflow_digest: String,
    pub selector_digest: String,
    pub denominator: Vec<String>,
}

/// The planned `run` row identity expected by the plan, bound to this result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlannedRowIdentity {
    pub gate_id: String,
    pub native_tier: String,
    pub policy_role: PolicyRole,
    pub lifecycle: LifecycleDisposition,
    pub requested_profile: String,
    pub command: String,
    pub timeout_seconds: u64,
    pub applicability_expected: Applicability,
}

/// One normalized result for one planned gate run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoutedGateResultV1 {
    pub schema: String,
    pub producer: String,
    pub subject: ResultSubjectRef,
    pub route_plan_fingerprint: String,
    pub plan_authority: PlanAuthorityIdentity,
    pub row: PlannedRowIdentity,
    pub prerequisites: PrerequisiteEvidence,
    pub command_started: bool,
    pub child: ChildObservation,
    pub timing: ObservationTiming,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_option_reject_null"
    )]
    pub hosted: Option<HostedIdentity>,
    pub product: PlaneOutcome,
    pub instrument: PlaneOutcome,
    pub reporting: PlaneOutcome,
    pub artifacts: Vec<ArtifactRef>,
    pub focused_reproduce_command: String,
    /// Domain-separated SHA-256 of the canonical semantic projection:
    /// `SHA-256("routed_gate_result.v1\0" || bytes)`; recomputed and compared
    /// at validation, never part of its own preimage.
    #[serde(deserialize_with = "reject_null_fingerprint")]
    pub result_fingerprint: String,
}

/// Explicit null for the fingerprint (or any non-optional field) is not a
/// canonical spelling; fail closed at the deserializer.
fn reject_null_fingerprint<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<String>::deserialize(deserializer)?;
    match value {
        Some(text) => Ok(text),
        None => Err(serde::de::Error::custom("explicit null is not a canonical spelling")),
    }
}

impl RoutedGateResultV1 {
    /// Fingerprint-bearing projection. Set-like collections are normalized
    /// (ascending unique); ordered fields keep structural order because that
    /// order is semantic, not presentation. The fingerprint field itself is
    /// excluded from its own preimage by construction.
    fn semantic_projection(&self) -> ResultSemanticProjection<'_> {
        let mut artifacts = self.artifacts.clone();
        artifacts.sort();
        artifacts.dedup();
        ResultSemanticProjection {
            schema: &self.schema,
            producer: &self.producer,
            subject: &self.subject,
            route_plan_fingerprint: &self.route_plan_fingerprint,
            plan_authority: &self.plan_authority,
            row: &self.row,
            prerequisites: &self.prerequisites,
            command_started: self.command_started,
            child: &self.child,
            timing: &self.timing,
            hosted: &self.hosted,
            product: &self.product,
            instrument: &self.instrument,
            reporting: &self.reporting,
            artifacts,
            focused_reproduce_command: &self.focused_reproduce_command,
        }
    }

    /// Canonical semantic bytes — the exact fingerprint preimage (before
    /// domain separation).
    pub fn canonical_semantic_bytes(&self) -> Result<Vec<u8>, String> {
        let value = serde_json::to_value(self.semantic_projection())
            .map_err(|error| format!("semantic projection failed: {error}"))?;
        canonical_json(&value)
    }

    /// Domain-separated SHA-256 fingerprint of the canonical semantic bytes.
    pub fn semantic_fingerprint_of(&self) -> Result<String, String> {
        let mut hasher = Sha256::new();
        hasher.update(FINGERPRINT_DOMAIN);
        hasher.update(self.canonical_semantic_bytes()?);
        Ok(hex(&hasher.finalize()))
    }

    /// Canonical encoded bytes of the complete published payload. Validates
    /// first: no invalid result is ever encoded.
    pub fn canonical_json(&self) -> Result<Vec<u8>, String> {
        self.validate()?;
        let value = serde_json::to_value(RoutedResultCanonicalPayload {
            semantic: self.semantic_projection(),
            result_fingerprint: &self.result_fingerprint,
        })
        .map_err(|error| format!("payload projection failed: {error}"))?;
        canonical_json(&value)
    }

    /// Validate every cross-field invariant, including that
    /// `result_fingerprint` equals the recomputed digest of the canonical
    /// semantic projection.
    pub fn validate(&self) -> Result<(), String> {
        validate_result(self)
    }
}

/// Fingerprint-bearing semantic fields (artifact order normalized).
#[derive(Debug, Clone, Serialize)]
struct ResultSemanticProjection<'a> {
    schema: &'a str,
    producer: &'a str,
    subject: &'a ResultSubjectRef,
    route_plan_fingerprint: &'a str,
    plan_authority: &'a PlanAuthorityIdentity,
    row: &'a PlannedRowIdentity,
    prerequisites: &'a PrerequisiteEvidence,
    command_started: bool,
    child: &'a ChildObservation,
    timing: &'a ObservationTiming,
    #[serde(skip_serializing_if = "Option::is_none")]
    hosted: &'a Option<HostedIdentity>,
    product: &'a PlaneOutcome,
    instrument: &'a PlaneOutcome,
    reporting: &'a PlaneOutcome,
    /// Set-like: normalized ascending unique here so source order and
    /// duplicate input lines collapse deterministically.
    artifacts: Vec<ArtifactRef>,
    focused_reproduce_command: &'a str,
}

/// Complete published payload: the semantic projection embedded verbatim plus
/// exactly the digest field, mirroring the #10179 construction so the
/// published field list cannot drift from the fingerprint preimage.
#[derive(Debug, Clone, Serialize)]
struct RoutedResultCanonicalPayload<'a> {
    #[serde(flatten)]
    semantic: ResultSemanticProjection<'a>,
    result_fingerprint: &'a str,
}

// ---------------------------------------------------------------------------
// Builder: one observation + one validated plan row -> one normalized result
// ---------------------------------------------------------------------------

/// Build one normalized result for the planned `run` row of `gate_id`.
///
/// Consumes and validates the plan first (#10179 identity is authority).
/// Refuses every dishonest shape: a dependency-blocked or never-started
/// command cannot carry a product verdict, a prerequisite-blocked child
/// cannot become a product failure, and contradictory observations fail
/// closed instead of normalizing into a plausible-looking success.
pub fn build_routed_result(
    plan: &CiRoutePlanV1,
    gate_id: &str,
    observation: RunObservation,
) -> Result<RoutedGateResultV1, String> {
    plan.validate().map_err(|error| format!("route plan rejected before execution: {error}"))?;
    let row = plan
        .rows
        .iter()
        .find(|row| row.gate_id == gate_id)
        .ok_or_else(|| format!("gate {gate_id:?} is not part of the governed denominator"))?;
    let PlannedOutcomeRunSnapshot { command, timeout_seconds } = match &row.outcome {
        crate::ci_route_plan::PlannedOutcome::Run { command, timeout_seconds, .. } => {
            PlannedOutcomeRunSnapshot {
                command: command.clone(),
                timeout_seconds: *timeout_seconds,
            }
        }
        _ => {
            return Err(format!(
                "gate {gate_id:?} has no planned run row (scoped-noop/quarantined/error rows do not execute)"
            ));
        }
    };
    if row.applicability != Applicability::Applicable {
        return Err(format!(
            "gate {gate_id:?} is not positively applicable on this subject; execution results require applicable planned rows"
        ));
    }

    let prerequisites = observation.prerequisites.clone().unwrap_or_else(|| PrerequisiteEvidence {
        state: PrerequisiteState::Missing,
        missing_artifacts: vec!["prerequisite evidence unavailable".to_string()],
        dependency_gates: BTreeMap::new(),
    });

    // Honesty guards: not-ready prerequisites must mean never-started.
    // In-process execution is post-start activity: the work ran inside the
    // runner process, so a never-started claim cannot carry it (review
    // thread 3873885140).
    let child_touched = observation.child.exit_code.is_some()
        || observation.child.signal.is_some()
        || observation.child.timed_out
        || observation.child.cancelled
        || observation.child.in_process;
    if prerequisites.state != PrerequisiteState::Ready
        && (observation.command_started || child_touched)
    {
        return Err(format!(
            "prerequisites are {:?} but the command claims post-prerequisite activity; \
             dependency-blocked work must be recorded never-started, not executed",
            prerequisites.state
        ));
    }

    // Contradiction guards between the runner status and the raw observations.
    match &observation.runner_status {
        RoutedReaderGateStatus::Fail
            if observation.child.timed_out || observation.child.cancelled =>
        {
            return Err("runner reports failure with timeout or cancellation facts".to_string());
        }
        RoutedReaderGateStatus::Timeout if !observation.child.timed_out => {
            return Err("runner reports timeout but the child shows no timeout flag".to_string());
        }
        RoutedReaderGateStatus::CancelledAfterStart if !observation.child.cancelled => {
            return Err(
                "runner reports cancellation but the child shows no cancellation flag".to_string()
            );
        }
        RoutedReaderGateStatus::Timeout if observation.child.cancelled => {
            return Err("runner reports timeout with cancellation facts".to_string());
        }
        RoutedReaderGateStatus::CancelledAfterStart if observation.child.timed_out => {
            return Err("runner reports cancellation with timeout facts".to_string());
        }
        RoutedReaderGateStatus::SpawnErrorBeforeStart
            if observation.command_started || child_touched =>
        {
            return Err(
                "runner reports spawn-before-start but the child shows post-start activity"
                    .to_string(),
            );
        }
        _ => {}
    }

    check_timing(&observation.timing).map_err(|error| format!("timing: {error}"))?;

    // --- instrument plane -------------------------------------------------
    let (instrument_outcome, instrument_detail) = match prerequisites.state {
        PrerequisiteState::Ready if observation.command_started => {
            (TerminalOutcome::Success, "prerequisites ready; process started".to_string())
        }
        PrerequisiteState::Ready => (
            TerminalOutcome::InstrumentFailure,
            "command never started despite ready prerequisites".to_string(),
        ),
        PrerequisiteState::Missing => {
            (TerminalOutcome::Missing, format_missing_prerequisites(&prerequisites))
        }
        PrerequisiteState::Failed => {
            (TerminalOutcome::InstrumentFailure, format_dependency_failures(&prerequisites))
        }
        PrerequisiteState::Stale => {
            (TerminalOutcome::Stale, "prerequisite evidence stale".to_string())
        }
    };

    // --- product plane ----------------------------------------------------
    let product_outcome = if prerequisites.state != PrerequisiteState::Ready {
        TerminalOutcome::BlockedNotProven
    } else {
        match &observation.runner_status {
            RoutedReaderGateStatus::SpawnErrorBeforeStart => TerminalOutcome::BlockedNotProven,
            RoutedReaderGateStatus::Timeout => TerminalOutcome::Timeout,
            RoutedReaderGateStatus::CancelledAfterStart => TerminalOutcome::Cancelled,
            RoutedReaderGateStatus::Pass => {
                // A clean pass is either a child process exiting zero or an
                // in-process gate with no process facts at all (review
                // thread 3871822391: internal xtask dispatch never carries
                // an exit code); anything else refuses to mint success.
                let clean_process = observation.child.exit_code == Some(0)
                    && observation.child.signal.is_none()
                    && !observation.child.in_process;
                let clean_in_process = observation.child.in_process
                    && observation.child.exit_code.is_none()
                    && observation.child.signal.is_none();
                if (clean_process || clean_in_process)
                    && !observation.child.timed_out
                    && !observation.child.cancelled
                {
                    TerminalOutcome::Success
                } else {
                    return Err(
                        "runner reports pass but the child did not exit cleanly; refusing to mint success"
                            .to_string(),
                    );
                }
            }
            // A failing gate is a product failure regardless of exit spelling
            // (including zero-exit free-form failures); the raw child fact
            // stays attached either way.
            RoutedReaderGateStatus::Fail => TerminalOutcome::Failure,
        }
    };

    // --- reporting plane --------------------------------------------------
    // The result artifact's own publication is this invocation's return
    // contract (publish durably or fail loudly), not a field in the bytes —
    // the fingerprint cannot cover its own write. The reporting plane
    // records the run's receipt/reporting instruments observed before
    // sealing (review threads 3871822398/3871822416).
    let reporting = if !observation.command_started {
        PlaneOutcome {
            outcome: TerminalOutcome::NotProven,
            detail: "command never started; no run reporting was produced".to_string(),
        }
    } else if !observation.receipt_shortfall.is_empty() {
        PlaneOutcome {
            outcome: TerminalOutcome::Missing,
            detail: format!(
                "missing receipt evidence: {}",
                observation.receipt_shortfall.join("; ")
            ),
        }
    } else if let Some(log) = observation.artifacts.iter().find(|artifact| artifact.role == "log") {
        PlaneOutcome {
            outcome: TerminalOutcome::Success,
            detail: format!("gate log bound: {}", log.path),
        }
    } else {
        PlaneOutcome {
            outcome: TerminalOutcome::NotProven,
            detail: "no bounded receipt identity supplied with the observation".to_string(),
        }
    };

    let mut result = RoutedGateResultV1 {
        schema: ROUTED_GATE_RESULT_SCHEMA.to_string(),
        producer: ROUTED_GATE_RESULT_PRODUCER.to_string(),
        subject: ResultSubjectRef {
            kind: plan.subject.kind.clone(),
            head_sha: plan.subject.head_sha.clone(),
            base_sha: plan.subject.base_sha.clone(),
            subject_digest: plan.subject.subject_digest.clone(),
        },
        route_plan_fingerprint: plan.semantic_fingerprint.clone(),
        plan_authority: PlanAuthorityIdentity {
            schema: CI_ROUTE_PLAN_SCHEMA.to_string(),
            semantic_fingerprint: plan.semantic_fingerprint.clone(),
            requested_profile: plan.requested_profile.clone(),
            included_native_tiers: plan.included_native_tiers.clone(),
            expansion_fingerprint: plan.expansion_fingerprint.clone(),
            policy_digest: plan.policy_digest.clone(),
            disposition_digest: plan.disposition_digest.clone(),
            workflow_digest: plan.workflow_digest.clone(),
            selector_digest: plan.selection.selector_digest.clone(),
            denominator: plan.denominator.clone(),
        },
        row: PlannedRowIdentity {
            gate_id: row.gate_id.clone(),
            native_tier: row.native_tier.clone(),
            policy_role: row.policy_role,
            lifecycle: row.lifecycle,
            requested_profile: plan.requested_profile.clone(),
            command,
            timeout_seconds,
            applicability_expected: row.applicability,
        },
        prerequisites,
        command_started: observation.command_started,
        child: observation.child.clone(),
        timing: observation.timing.clone(),
        hosted: observation.hosted.clone(),
        product: PlaneOutcome { outcome: product_outcome, detail: product_detail(product_outcome) },
        instrument: PlaneOutcome { outcome: instrument_outcome, detail: instrument_detail },
        reporting,
        artifacts: observation.artifacts.clone(),
        focused_reproduce_command: build_reproduce_command(
            &row.native_tier,
            gate_id,
            &plan.selection.base,
            row.selector_role,
            &row.outcome,
        ),
        result_fingerprint: String::new(),
    };
    result.result_fingerprint = result.semantic_fingerprint_of()?;
    result.validate()?;
    Ok(result)
}

struct PlannedOutcomeRunSnapshot {
    command: String,
    timeout_seconds: u64,
}

fn product_detail(outcome: TerminalOutcome) -> String {
    match outcome {
        TerminalOutcome::Success => "gate command reached and established a passing verdict".into(),
        TerminalOutcome::Failure => "gate command ran and established a failing verdict".into(),
        TerminalOutcome::Timeout => "gate command exceeded its planned timeout".into(),
        TerminalOutcome::Cancelled => "gate command was cancelled after start".into(),
        TerminalOutcome::BlockedNotProven => {
            "no product verdict: the command did not start or complete under valid instruments"
                .into()
        }
        other => format!("product plane recorded {}", other.as_str()),
    }
}

fn format_missing_prerequisites(prerequisites: &PrerequisiteEvidence) -> String {
    if prerequisites.missing_artifacts.is_empty() {
        "prerequisite artifacts missing; none named by evidence".to_string()
    } else {
        format!("missing prerequisite artifacts: {}", prerequisites.missing_artifacts.join(", "))
    }
}

fn format_dependency_failures(prerequisites: &PrerequisiteEvidence) -> String {
    if prerequisites.dependency_gates.is_empty() {
        "prerequisite gates failed; dependency dispositions not recorded".to_string()
    } else {
        let joined: Vec<String> = prerequisites
            .dependency_gates
            .iter()
            .map(|(gate, reason)| format!("{gate}: {reason}"))
            .collect();
        format!("failed dependencies: {}", joined.join("; "))
    }
}

fn check_timing(timing: &ObservationTiming) -> Result<(), String> {
    match (timing.started_at_unix_ms, timing.ended_at_unix_ms) {
        (Some(start), Some(end)) => {
            if end < start {
                return Err("ended before started".to_string());
            }
            let delta = u64::try_from(end - start).map_err(|_| "duration overflow".to_string())?;
            if delta != timing.duration_ms {
                return Err(format!(
                    "duration_ms {} does not equal ended-started ({delta})",
                    timing.duration_ms
                ));
            }
            Ok(())
        }
        (None, None) => {
            if timing.duration_ms != 0 {
                return Err("unstarted observation claims nonzero duration".to_string());
            }
            Ok(())
        }
        (Some(_), None) | (None, Some(_)) => {
            Err("half-observed timing: start and end must both be present or both absent".into())
        }
    }
}

/// Invocable focused reproduction of the row's gate execution (review
/// thread 3871822422). The plan spells native tiers in snake_case while the
/// `gates` CLI spells them kebab-case, so the command is emitted in the CLI
/// spelling; `cargo xtask gates --tier <tier> --gate <gate>` re-runs exactly
/// this row's gate.
/// The command a reader can actually run to reproduce this row.
///
/// The generic `--tier/--base/--gate` spelling is only invocable for rows
/// whose command does not depend on resolved package scope. `plan_gates`
/// short-circuits to `static_gate_plan` as soon as a `--gate` filter is
/// present, so the scope selector never runs and a scoped gate's
/// `{package_args}` placeholder is left unresolved -- which the runner then
/// refuses outright rather than running a gate that would silently pass with
/// zero packages. Advertising that spelling for a scoped row would hand the
/// reader a command that always fails.
///
/// For those rows the honest reproduction is the plan's own fully expanded
/// command: it is exactly what this row executed, it needs no scope
/// re-derivation, and it cannot go stale relative to the result it is
/// attached to.
fn build_reproduce_command(
    native_tier: &str,
    gate_id: &str,
    base: &str,
    selector_role: SelectorRole,
    outcome: &PlannedOutcome,
) -> String {
    if let (true, PlannedOutcome::Run { command, .. }) =
        (scope_dependent_selector(selector_role), outcome)
    {
        return command.trim().to_string();
    }
    let tier = native_tier.replace('_', "-");
    let staged = if tier == "commit" { " --staged" } else { "" };
    format!("cargo xtask gates --tier {tier} --base {base} --gate {gate_id}{staged}")
}

/// Selector roles whose expanded command depends on the resolved package
/// scope, and which therefore cannot be reproduced through a `--gate` filter.
fn scope_dependent_selector(role: SelectorRole) -> bool {
    match role {
        SelectorRole::RustScoped | SelectorRole::RustPackageScoped => true,
        SelectorRole::AlwaysOn
        | SelectorRole::RustFallback
        | SelectorRole::Static
        | SelectorRole::Unspecified => false,
    }
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

fn validate_result(result: &RoutedGateResultV1) -> Result<(), String> {
    if result.schema != ROUTED_GATE_RESULT_SCHEMA {
        return Err(format!("unsupported result schema {:?}", result.schema));
    }
    if result.producer != ROUTED_GATE_RESULT_PRODUCER {
        return Err(format!("unsupported producer {:?}", result.producer));
    }
    if result.route_plan_fingerprint != result.plan_authority.semantic_fingerprint {
        return Err("route_plan_fingerprint disagrees with plan_authority identity".to_string());
    }
    // A consumer validates a result without the plan beside it, so the copied
    // provenance has to be coherent on its own. Re-sealing the fingerprint
    // preserves a contradiction rather than catching it, which would let an
    // incoherent result read as validated provenance.
    if result.plan_authority.schema != CI_ROUTE_PLAN_SCHEMA {
        return Err(format!(
            "plan_authority claims schema {:?}, not {CI_ROUTE_PLAN_SCHEMA}",
            result.plan_authority.schema
        ));
    }
    if result.row.requested_profile != result.plan_authority.requested_profile {
        return Err(format!(
            "row claims profile {:?} but the plan authority is {:?}",
            result.row.requested_profile, result.plan_authority.requested_profile
        ));
    }
    let denominator_hits = result
        .plan_authority
        .denominator
        .iter()
        .filter(|gate| *gate == &result.row.gate_id)
        .count();
    if denominator_hits != 1 {
        return Err(format!(
            "row gate {:?} appears {denominator_hits} times in the governed denominator; \
             exactly one occurrence is required",
            result.row.gate_id
        ));
    }
    if !result.plan_authority.included_native_tiers.contains(&result.row.native_tier) {
        return Err(format!(
            "row native tier {:?} is outside the plan's included tiers {:?}",
            result.row.native_tier, result.plan_authority.included_native_tiers
        ));
    }
    for sha in [&result.subject.head_sha, &result.subject.subject_digest] {
        if sha.is_empty() {
            return Err("empty subject identity".to_string());
        }
    }
    if let Some(sha) = &result.subject.base_sha
        && sha.is_empty()
    {
        return Err("empty base_sha".to_string());
    }
    if result.row.gate_id.is_empty() || result.row.command.is_empty() {
        return Err("planned row identity incomplete".to_string());
    }
    if result.row.applicability_expected != Applicability::Applicable {
        return Err("execution results require an applicable planned row".to_string());
    }
    if result.row.timeout_seconds == 0 {
        return Err("planned run rows carry a nonzero timeout policy".to_string());
    }
    for artifact in &result.artifacts {
        if artifact.role.is_empty() || artifact.path.is_empty() {
            return Err("artifact reference lacks role or path".to_string());
        }
        if let Some(sha256) = &artifact.sha256
            && !is_hex_sha256(sha256)
        {
            return Err(format!("artifact {} sha256 is not lowercase hex sha256", artifact.path));
        }
    }
    validate_hosted_identity(result.hosted.as_ref())?;
    check_timing(&result.timing)?;
    validate_plane_honesty(result)?;
    let recomputed = result.semantic_fingerprint_of()?;
    if recomputed != result.result_fingerprint {
        return Err("result_fingerprint does not match the recomputed canonical preimage; \
             the record is stale or tampered"
            .to_string());
    }
    Ok(())
}

fn validate_hosted_identity(hosted: Option<&HostedIdentity>) -> Result<(), String> {
    if let Some(hosted) = hosted {
        if hosted.run_attempt == 0 {
            return Err("hosted run attempt 0 is not a real attempt identity".to_string());
        }
        if hosted.run_id.is_none() {
            return Err("hosted identity without a run id binds nothing".to_string());
        }
        // A hosted result must bind the claimed identity set; a partial
        // record (run id without workflow/job) contradicts the
        // workflow/job/run/attempt binding contract (review thread
        // 3872200290). `matrix` stays optional: a non-matrix job genuinely
        // has no matrix context, and absence is the honest spelling.
        if hosted.workflow.is_none() || hosted.job.is_none() {
            return Err(
                "hosted identity must bind workflow, job, run id, and run attempt together; \
                 a partial ambient GitHub environment cannot be recorded as hosted evidence"
                    .to_string(),
            );
        }
    }
    Ok(())
}

/// The planes must tell the story the raw observations support. These are the
/// encoded negative controls: cross-plane flattening and invented verdicts do
/// not validate.
fn validate_plane_honesty(result: &RoutedGateResultV1) -> Result<(), String> {
    let child_touched = result.child.exit_code.is_some()
        || result.child.signal.is_some()
        || result.child.timed_out
        || result.child.cancelled
        || result.child.in_process;

    if result.prerequisites.state != PrerequisiteState::Ready
        && (result.command_started || child_touched)
    {
        return Err(
            "not-ready prerequisites coexist with claimed post-prerequisite activity".to_string()
        );
    }
    if result.prerequisites.state != PrerequisiteState::Ready
        && result.product.outcome != TerminalOutcome::BlockedNotProven
    {
        return Err(
            "a blocked/unproven prerequisite set cannot yield any product outcome other than blocked_not_proven"
                .to_string(),
        );
    }

    if !result.command_started && child_touched {
        return Err("never-started command carries terminal child facts".to_string());
    }
    if !result.command_started && result.product.outcome.is_product_verdict() {
        return Err("never-started command carries a product verdict".to_string());
    }

    let expected_instrument = match (result.prerequisites.state, result.command_started) {
        (PrerequisiteState::Ready, true) => TerminalOutcome::Success,
        (PrerequisiteState::Ready, false) => TerminalOutcome::InstrumentFailure,
        (PrerequisiteState::Missing, _) => TerminalOutcome::Missing,
        (PrerequisiteState::Failed, _) => TerminalOutcome::InstrumentFailure,
        (PrerequisiteState::Stale, _) => TerminalOutcome::Stale,
    };
    if result.instrument.outcome != expected_instrument {
        return Err(format!(
            "instrument outcome {} disagrees with prerequisite/command state (expected {})",
            result.instrument.outcome.as_str(),
            expected_instrument.as_str()
        ));
    }

    match result.product.outcome {
        TerminalOutcome::Success => {
            let clean_process = result.child.exit_code == Some(0)
                && result.child.signal.is_none()
                && !result.child.timed_out
                && !result.child.cancelled;
            let clean_in_process = result.child.in_process
                && result.child.exit_code.is_none()
                && result.child.signal.is_none()
                && !result.child.timed_out
                && !result.child.cancelled;
            if !(clean_process || clean_in_process) {
                return Err(
                    "success requires a clean zero exit (or an in-process execution with no \
                     process facts) and no signal"
                        .to_string(),
                );
            }
        }
        TerminalOutcome::Timeout => {
            if !result.child.timed_out || result.child.cancelled {
                return Err("timeout product outcome without a timeout flag".to_string());
            }
        }
        TerminalOutcome::Cancelled => {
            if !result.child.cancelled || result.child.timed_out {
                return Err("cancelled product outcome without a cancellation flag".to_string());
            }
        }
        TerminalOutcome::BlockedNotProven => {
            if result.command_started {
                return Err(
                    "blocked_not_proven while claiming the command started is a contradiction"
                        .to_string(),
                );
            }
        }
        TerminalOutcome::Failure => {
            if result.child.timed_out || result.child.cancelled {
                return Err("failure cannot carry timeout or cancellation facts".to_string());
            }
        }
        other => {
            // Missing/stale/not-proven/instrument-only products still must
            // not ride on a settled child that claims clean success.
            let clean_process = result.child.exit_code == Some(0) && result.child.signal.is_none();
            let clean_in_process = result.child.in_process
                && result.child.exit_code.is_none()
                && result.child.signal.is_none();
            if clean_process || clean_in_process {
                return Err(format!(
                    "non-verdict product outcome {} cannot coexist with a clean exit",
                    other.as_str()
                ));
            }
        }
    }
    // An in-process gate has no child process: process-terminal facts cannot
    // coexist with it (review thread 3871822391).
    if result.child.in_process
        && (result.child.exit_code.is_some()
            || result.child.signal.is_some()
            || result.child.timed_out
            || result.child.cancelled)
    {
        return Err("in-process execution cannot carry child-process terminal facts".to_string());
    }
    Ok(())
}

fn is_hex_sha256(value: &str) -> bool {
    value.len() == 64
        && value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

// ---------------------------------------------------------------------------
// Durable publication
// ---------------------------------------------------------------------------

/// Publish one result through the shared single-writer durable substrate
/// ([`crate::durable_publish`], review thread 3872200294): unique
/// create-new temporary file, complete write + flush + sync, temp read-back
/// verification, atomic rename, POSIX directory sync, final read-back. The
/// final name is content-addressed (`<gate>-<fingerprint-prefix>.json`), so
/// repeat publication of identical bytes is idempotent while anything else
/// lands under a different name instead of clobbering history.
///
/// Any substrate refusal is a typed error; a failing invocation never
/// leaves a valid-looking current result under the requested name.
pub fn publish_routed_receipt(
    directory: &Path,
    result: &RoutedGateResultV1,
) -> Result<PathBuf, String> {
    let bytes = result.canonical_json()?;
    let stem = sanitize_component(&result.row.gate_id);
    let short_fingerprint = result.result_fingerprint.get(..16).unwrap_or("").to_string();
    if short_fingerprint.len() < 16 {
        return Err("result fingerprint too short to publish".to_string());
    }
    let destination = directory.join(format!("{stem}-{short_fingerprint}.json"));
    crate::durable_publish::publish_atomically(&destination, &bytes)
        .map_err(|error| error.to_string())?;
    Ok(destination)
}

fn sanitize_component(value: &str) -> String {
    value
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect()
}

/// Lowercase hex encoding (mirrors the #10179 encoder's fallback spelling).
fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}
