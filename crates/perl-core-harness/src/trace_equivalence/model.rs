//! Receipt, verdict, plane, and work types for the ordinary/instrumented
//! `t/TEST` equivalence contract (`upstream_trace_equivalence.v1`, #12286).
//!
//! Representation only: no upstream execution, tracing, or filesystem
//! interaction. The comparator consumes already-validated #12281 ordinary
//! discovery, #12285 instrumented parent discovery, #12284 trace, and
//! instrumentation-work receipts and emits one exact claim-ceiling relation.

use crate::observed_subject::model::OrdinaryInstrumentedEquivalenceIdentity;
use serde::{Deserialize, Serialize};

/// Schema identity of the equivalence receipt.
pub const UPSTREAM_TRACE_EQUIVALENCE_SCHEMA_VERSION: &str =
    "perl_core_harness.upstream_trace_equivalence.v1";

/// Fixed claim boundary carried by every equivalence receipt.
pub const TRACE_EQUIVALENCE_CLAIM_BOUNDARY: &str = "instrumented invocation observation is non-perturbing evidence for ordinary t/TEST only for the exact bound target/Perl/runner/patch/schema subjects and only for the named comparison planes; compiler semantic, TAP, and runtime equivalence remain unproven";

/// Propositions a passing receipt never transfers, whatever the verdict.
pub const NON_TRANSFERABLE_PROPOSITIONS: &[&str] = &[
    "another Perl revision",
    "another target or variant",
    "another trace patch, capture point, or schema",
    "another runner (including t/harness)",
    "compiler semantic or runtime equivalence",
    "future runner drift without rerun",
];

/// The one proposition a passing receipt transfers.
pub const TRANSFERABLE_PROPOSITION: &str = "instrumented invocation observation is non-perturbing evidence for ordinary t/TEST for the exact bound subjects and declared planes";

/// Claim-ceiling verdict of one paired comparison.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EquivalenceVerdict {
    /// Every declared plane matched on exact bound subjects.
    EquivalentForDeclaredPlanes,
    /// At least one declared plane measurably differed.
    MeasuredDifference,
    /// The two sides do not bind the same exact subject pair.
    NotComparable,
    /// Consumer-side freshness judgment; never written by the comparator.
    Stale,
    /// Required evidence is missing, so no relation is proven.
    NotProven,
    /// The instrument failed around the runner; no runner fact is claimed.
    InstrumentFailure,
}

impl EquivalenceVerdict {
    /// True only when the receipt grants the bounded transfer relation.
    pub fn grants_transfer(self) -> bool {
        matches!(self, Self::EquivalentForDeclaredPlanes)
    }
}

/// One independently visible comparison plane.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComparisonPlane {
    /// Raw discovery stdout bytes after trace-channel isolation.
    DiscoveryBytes,
    /// Frame-aware canonical membership in original order.
    MembershipOrder,
    /// Terminal process disposition of both runners.
    TerminalProcess,
    /// Disposable-copy, trace-file, and process cleanup evidence.
    Cleanup,
    /// Instrumented invocation rows against accepted parent membership.
    InvocationRows,
}

/// Typed outcome of one plane comparison.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlaneComparison {
    /// Plane this record judges.
    pub plane: ComparisonPlane,
    /// SHA-256 of the ordinary-side plane identity.
    pub ordinary_digest: String,
    /// SHA-256 of the instrumented-side plane identity.
    pub instrumented_digest: String,
    /// True when the two plane identities are exactly equal.
    pub matched: bool,
    /// Typed measured differences; empty exactly when `matched`.
    pub differences: Vec<String>,
}

/// Exact subjects the relation binds on both sides.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EquivalenceSubjects {
    /// Ordinary (unpatched) discovery receipt payload digest.
    pub ordinary_receipt_digest: String,
    /// Instrumented parent discovery receipt payload digest.
    pub instrumented_parent_receipt_digest: String,
    /// Effective-invocation trace receipt payload digest.
    pub trace_receipt_digest: String,
    /// Instrumentation-work receipt payload digest.
    pub work_receipt_digest: String,
    /// Transfer identity consumable by the #12287 join.
    pub equivalence_identity: OrdinaryInstrumentedEquivalenceIdentity,
}

/// Full payload of one equivalence receipt.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TraceEquivalencePayload {
    /// Exact bound subjects of the pair.
    pub subjects: EquivalenceSubjects,
    /// Target contract identity both sides executed.
    pub target_id: String,
    /// Resolved upstream Perl source reference both sides executed.
    pub perl_ref: String,
    /// Commit of the measuring repository.
    pub repository_commit: String,
    /// Claim-ceiling verdict.
    pub verdict: EquivalenceVerdict,
    /// Per-plane comparisons in declared order.
    pub planes: Vec<PlaneComparison>,
    /// Pair-level measured differences or non-comparability reasons.
    pub differences: Vec<String>,
    /// Propositions this receipt transfers (verdict-bound).
    pub transferable: Vec<String>,
    /// Propositions this receipt never transfers.
    pub non_transferable: Vec<String>,
    /// Mandatory limitations retained verbatim.
    pub limitations: Vec<String>,
    /// Fixed claim boundary retained verbatim.
    pub claim_boundary: String,
}

/// Immutable versioned equivalence receipt.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpstreamTraceEquivalenceV1 {
    /// Schema identity; always [`UPSTREAM_TRACE_EQUIVALENCE_SCHEMA_VERSION`].
    pub schema_version: String,
    /// SHA-256 over the canonical serialization of `payload`.
    pub payload_digest: String,
    /// Full evidence payload.
    pub payload: TraceEquivalencePayload,
}

/// Inputs to one comparison. Runtime-only: never serialized.
#[derive(Clone, Debug)]
pub struct TraceEquivalenceInput {
    /// Pinned target matrix the paired receipts are validated against.
    pub matrix: crate::model::UpstreamTargetMatrix,
    /// Ordinary discovery receipt; `None` is the missing-pair falsifier.
    pub ordinary: Option<crate::observed_discovery::model::UpstreamDiscoveryReceiptV1>,
    /// Instrumented parent discovery receipt (#12285).
    pub instrumented_parent: crate::observed_discovery::model::UpstreamDiscoveryReceiptV1,
    /// Effective-invocation trace receipt (#12284).
    pub trace: crate::invocation_trace::model::EffectiveInvocationTraceReceiptV1,
    /// Instrumentation-work receipt (#12285).
    pub work: crate::invocation_trace::instrument::InstrumentationWorkReceiptV1,
}
