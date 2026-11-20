//! Typed per-invocation evidence planes and settled receipt contract (#8191).

use perl_core_harness_types::{
    HarnessMode, RUNNER_RECORD_SCHEMA_VERSION, RunnerRecord, RunnerStatus,
    result_axes::EvidenceValidity,
};
use serde::{Deserialize, Serialize};

use crate::transition::TerminalProcessOutcome;

/// Wire schema for one settled invocation receipt.
pub const INVOCATION_RECEIPT_SCHEMA_VERSION: &str = "perl_core_harness.invocation.v1";

/// Canonical TAP parser identity recorded on receipts.
pub const TAP_PARSER_IDENTITY: &str = "perl_test_facts.parse_tap.v1";

/// Which authority produced the semantic runner record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvocationAuthority {
    /// Upstream `t/TEST` scheduler observation.
    UpstreamTest,
    /// Upstream TAP::Harness-backed observation.
    UpstreamHarness,
    /// Bounded direct diagnostic probe; never admissible as upstream product evidence.
    DirectProbe,
}

/// Immutable identity binding for one invocation within a run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InvocationBinding {
    pub run_id: String,
    pub invocation_id: String,
    pub expected_path: String,
    pub mode: HarnessMode,
    pub plan_payload_digest: String,
    pub source_logical_id: String,
    pub source_raw_digest: String,
    pub source_effective_digest: String,
    pub runner_artifact_identity: String,
}

/// Evidence supplied for reconciliation of one invocation.
pub struct ReconcileInvocationInput<'a> {
    pub binding: InvocationBinding,
    pub authority: InvocationAuthority,
    pub terminal: TerminalProcessOutcome,
    pub stdout_truncated: bool,
    pub stdout: &'a str,
    pub semantic_records: &'a [RunnerRecord],
}

/// Summarized process plane on a settled receipt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InvocationProcessSummary {
    pub terminal_outcome: String,
    pub scoreable: bool,
    pub stdout_truncated: bool,
}

/// Summarized TAP plane on a settled receipt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InvocationTapSummary {
    pub raw_digest: String,
    pub parser_identity: String,
    pub plan_start: Option<u32>,
    pub plan_end: Option<u32>,
    pub assertions_passed: usize,
    pub assertions_failed: usize,
    pub skipped: usize,
    pub bailed_out: bool,
    pub structurally_complete: bool,
    /// True when the TAP plan is an explicit skip-all declaration (`1..0 # SKIP …`).
    pub plan_skip_all: bool,
}

/// Summarized semantic runner-record plane on a settled receipt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InvocationSemanticSummary {
    pub payload_digest: String,
    pub mode: String,
    pub path: String,
    pub reported_status: RunnerStatus,
    pub assertions_passed: usize,
    pub assertions_total: usize,
}

/// Domain outcome distinct from evidence validity (#8567 / #8228 alignment).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvocationDomainOutcome {
    CompletedCleanly,
    CompletedWithFailures,
    RecognizedSkip,
    UnsupportedOrBounded,
    NotAssessed,
}

/// Derived reconciliation result; self-reported pass is never authoritative alone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InvocationDerived {
    pub evidence_result: EvidenceValidity,
    pub domain_outcome: InvocationDomainOutcome,
    pub reasons: Vec<String>,
}

/// One canonical settled invocation receipt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettledInvocationReceipt {
    pub schema_version: String,
    pub run_id: String,
    pub invocation_id: String,
    pub authority: InvocationAuthority,
    pub binding: InvocationBinding,
    pub process: InvocationProcessSummary,
    pub tap: InvocationTapSummary,
    pub semantic_record: Option<InvocationSemanticSummary>,
    pub derived: InvocationDerived,
}

impl SettledInvocationReceipt {
    /// Schema constant carried on every receipt.
    #[must_use]
    pub fn schema_version() -> &'static str {
        INVOCATION_RECEIPT_SCHEMA_VERSION
    }
}

/// Why a runner record failed structural validation during reconciliation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SemanticRecordRejection {
    WrongSchema { observed: String },
    WrongMode { observed: String, expected: String },
    WrongPath { observed: String, expected: String },
    ImpossibleAssertions { passed: usize, total: usize },
    MechanismViolation { detail: String },
    ConflictingStatuses,
    DuplicateRecords,
    MissingRecord,
}

impl SemanticRecordRejection {
    pub(crate) fn reason(self) -> String {
        match self {
            Self::WrongSchema { observed } => {
                format!("semantic record schema {observed} is not {RUNNER_RECORD_SCHEMA_VERSION}")
            }
            Self::WrongMode { observed, expected } => {
                format!("semantic record mode {observed} does not match expected {expected}")
            }
            Self::WrongPath { .. } => {
                "semantic record path does not match invocation binding".to_string()
            }
            Self::ImpossibleAssertions { passed, total } => {
                format!("semantic record assertions_passed {passed} exceeds total {total}")
            }
            Self::MechanismViolation { detail } => detail,
            Self::ConflictingStatuses => {
                "conflicting semantic records for one invocation".to_string()
            }
            Self::DuplicateRecords => "duplicate semantic records for one invocation".to_string(),
            Self::MissingRecord => "missing semantic runner record".to_string(),
        }
    }
}
