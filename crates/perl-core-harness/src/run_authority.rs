//! Typed separation of upstream observation authority from direct diagnostic
//! probes (#8173).
//!
//! An [`UpstreamObservationSet`] is the frozen product of exactly one upstream
//! harness run: its expected membership, its observed rows, and its raw
//! process terminal disposition. A [`DirectDiagnosticSet`] records bounded
//! diagnostic probes that may investigate a frozen upstream discrepancy but
//! can never flow back into upstream membership, completeness, totals,
//! transitions, or accepted state. Nothing in this module converts one class
//! into the other.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use color_eyre::eyre::{Result, bail};
use perl_core_harness_types::{
    DiscoveredTest, HarnessMode, HarnessProfile, HarnessRunner, RunnerRecord, RunnerStatus,
};
use serde::{Deserialize, Serialize};

use crate::normalization::sha256_digest_bytes;
use crate::normalize_test_path;

/// Schema of the separately retained direct diagnostic receipt.
pub(crate) const DIRECT_DIAGNOSTICS_SCHEMA_VERSION: &str =
    "perl_core_harness.direct_diagnostics.v1";

/// Authority label recorded on every direct probe row.
pub(crate) const DIRECT_PROBE_AUTHORITY: &str = "direct_probe";

/// Why a direct probe cannot stand in for the upstream selection context.
pub(crate) const LIMITATION_MISSING_UPSTREAM_SELECTION_CONTEXT: &str =
    "direct_fallback_missing_upstream_selection_context";

/// Why a direct probe produced no usable record at all.
pub(crate) const LIMITATION_PROBE_UNAVAILABLE: &str = "direct_probe_produced_no_runner_record";

/// Immutable identity of one frozen upstream observation subject.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ObservedRunnerSubjectId(String);

impl ObservedRunnerSubjectId {
    /// Stable receipt form of the subject identity.
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

/// One expected upstream invocation identified by its normalized test path.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ExpectedInvocationId {
    invocation: InvocationId,
}

impl ExpectedInvocationId {
    /// Normalized upstream test path of the expected invocation.
    pub(crate) fn as_str(&self) -> &str {
        self.invocation.as_str()
    }
}

/// Identity of one settled invocation, keyed by normalized test path.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct InvocationId(String);

impl InvocationId {
    fn as_str(&self) -> &str {
        &self.0
    }
}

/// Raw upstream process terminal capture.
///
/// Classification stays with #6884; this records only what the process did so
/// the authoritative report and receipts stay honest about terminality.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UpstreamTerminalDisposition {
    /// The upstream process exited with status zero.
    Success,
    /// The upstream process exited with a nonzero status code.
    Failure(i32),
    /// The upstream process terminal state could not be captured.
    Unknown,
}

impl UpstreamTerminalDisposition {
    pub(crate) fn from_status_code(status: Option<i32>) -> Self {
        match status {
            Some(0) => Self::Success,
            Some(code) => Self::Failure(code),
            None => Self::Unknown,
        }
    }

    /// Numeric status code when one was captured.
    pub(crate) fn status_code(self) -> Option<i32> {
        match self {
            Self::Success => Some(0),
            Self::Failure(code) => Some(code),
            Self::Unknown => None,
        }
    }
}

/// One validated upstream row bound to its invocation identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SettledInvocation {
    invocation: InvocationId,
    record: RunnerRecord,
}

impl SettledInvocation {
    /// Verbatim raw runner record retained under upstream authority.
    pub(crate) fn record(&self) -> &RunnerRecord {
        &self.record
    }
}

/// Membership census recorded independently in receipts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct UpstreamMembershipCounts {
    pub expected: usize,
    pub observed: usize,
    pub missing: usize,
    pub extra: usize,
}

/// Frozen upstream observation: expected membership, observed rows, terminal.
///
/// Construction goes through [`UpstreamObservationSet::settle`] only, which
/// validates rows before any summary, digest, or diagnostic exists. Fields are
/// private so no mixed-authority collection can be substituted later.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UpstreamObservationSet {
    subject: ObservedRunnerSubjectId,
    expected: Vec<ExpectedInvocationId>,
    observed: BTreeMap<InvocationId, SettledInvocation>,
    extra_rows: usize,
    terminal: UpstreamTerminalDisposition,
}

impl UpstreamObservationSet {
    /// Freeze one upstream observation from exactly one context read.
    ///
    /// Missing expected rows stay missing: nothing here repairs membership.
    /// Duplicate normalized rows and malformed paths fail closed instead of
    /// silently collapsing into totals.
    pub(crate) fn settle(
        runner: HarnessRunner,
        mode: HarnessMode,
        profile: HarnessProfile,
        discovered: &[DiscoveredTest],
        records: &[RunnerRecord],
        terminal_status: Option<i32>,
    ) -> Result<Self> {
        let mut expected = Vec::<ExpectedInvocationId>::with_capacity(discovered.len());
        let mut expected_ids = BTreeSet::<InvocationId>::new();
        for test in discovered {
            let normalized = normalize_test_path(&test.path).ok_or_else(|| {
                color_eyre::eyre::eyre!("discovered test path did not normalize: {}", test.path)
            })?;
            if expected_ids.insert(InvocationId(normalized.clone())) {
                expected.push(ExpectedInvocationId { invocation: InvocationId(normalized) });
            }
        }

        let mut observed = BTreeMap::<InvocationId, SettledInvocation>::new();
        let mut extra_rows = 0usize;
        for record in records {
            let normalized = normalize_test_path(&record.path).ok_or_else(|| {
                color_eyre::eyre::eyre!(
                    "upstream runner record path did not normalize: {}",
                    record.path
                )
            })?;
            let invocation = InvocationId(normalized);
            if !expected_ids.contains(&invocation) {
                extra_rows += 1;
                continue;
            }
            if observed.contains_key(&invocation) {
                bail!("duplicate upstream runner record for {}", invocation.as_str());
            }
            observed.insert(
                invocation.clone(),
                SettledInvocation { invocation, record: record.clone() },
            );
        }

        let subject = subject_id(runner, mode, profile, &expected);
        Ok(Self {
            subject,
            expected,
            observed,
            extra_rows,
            terminal: UpstreamTerminalDisposition::from_status_code(terminal_status),
        })
    }

    /// Frozen subject identity of this observation.
    pub(crate) fn subject(&self) -> &ObservedRunnerSubjectId {
        &self.subject
    }

    /// Expected invocations in discovery order.
    pub(crate) fn expected(&self) -> &[ExpectedInvocationId] {
        &self.expected
    }

    /// Settled upstream row for an expected invocation, when one was observed.
    pub(crate) fn observed_record(&self, expected: &ExpectedInvocationId) -> Option<&RunnerRecord> {
        self.observed.get(&expected.invocation).map(SettledInvocation::record)
    }

    /// Expected invocations without an upstream observation.
    pub(crate) fn missing(&self) -> Vec<&ExpectedInvocationId> {
        self.expected.iter().filter(|item| !self.observed.contains_key(&item.invocation)).collect()
    }

    /// Independent membership census for receipts.
    pub(crate) fn counts(&self) -> UpstreamMembershipCounts {
        UpstreamMembershipCounts {
            expected: self.expected.len(),
            observed: self.observed.len(),
            missing: self.missing().len(),
            extra: self.extra_rows,
        }
    }

    /// Raw upstream process terminal disposition captured at freeze time.
    pub(crate) fn terminal(&self) -> UpstreamTerminalDisposition {
        self.terminal
    }
}

fn subject_id(
    runner: HarnessRunner,
    mode: HarnessMode,
    profile: HarnessProfile,
    expected: &[ExpectedInvocationId],
) -> ObservedRunnerSubjectId {
    let mut canonical = format!(
        "perl_core_harness.upstream_subject.v1\n{}\n{}\n{}\n",
        runner.as_str(),
        mode.as_str(),
        profile.as_str()
    );
    for item in expected {
        canonical.push_str(item.as_str());
        canonical.push('\n');
    }
    ObservedRunnerSubjectId(sha256_digest_bytes(canonical.as_bytes()))
}

/// Outcome of one direct diagnostic probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DiagnosticProbeOutcome {
    /// The probe reproduced a passing result for the file.
    ReproducedPass,
    /// The probe reproduced a failing result for the file.
    ReproducedFail,
    /// The probe could not settle any result for the file.
    Unavailable,
}

/// One settled direct probe with exact lineage to its parent discrepancy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SettledDiagnosticProbe {
    pub(crate) subject_path: String,
    pub(crate) process_status: Option<i32>,
    pub(crate) outcome: DiagnosticProbeOutcome,
    /// Verbatim raw probe row retained under diagnostic authority only.
    pub(crate) record: Option<RunnerRecord>,
    pub(crate) limitations: Vec<String>,
}

impl SettledDiagnosticProbe {
    /// Probe that executed but produced no usable runner record.
    pub(crate) fn unavailable(subject_path: &str, process_status: Option<i32>) -> Self {
        Self {
            subject_path: subject_path.to_string(),
            process_status,
            outcome: DiagnosticProbeOutcome::Unavailable,
            record: None,
            limitations: vec![LIMITATION_PROBE_UNAVAILABLE.to_string()],
        }
    }

    /// Probe whose raw row settled a result for its subject path.
    pub(crate) fn settled(
        subject_path: &str,
        process_status: Option<i32>,
        record: RunnerRecord,
    ) -> Self {
        let outcome = match record.status {
            RunnerStatus::Pass => DiagnosticProbeOutcome::ReproducedPass,
            RunnerStatus::Fail => DiagnosticProbeOutcome::ReproducedFail,
        };
        Self {
            subject_path: subject_path.to_string(),
            process_status,
            outcome,
            record: Some(record),
            limitations: Vec::new(),
        }
    }
}

/// Separate diagnostic product: probes investigating a frozen discrepancy.
///
/// This type has no conversion into [`UpstreamObservationSet`], no shared
/// collection with it, and no path into report totals or accepted state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DirectDiagnosticSet {
    parent_observation: Option<ObservedRunnerSubjectId>,
    probes: Vec<SettledDiagnosticProbe>,
    limitations: Vec<String>,
}

impl DirectDiagnosticSet {
    /// Plan diagnostics against one frozen upstream observation.
    pub(crate) fn plan(parent: &UpstreamObservationSet) -> Self {
        Self {
            parent_observation: Some(parent.subject().clone()),
            probes: Vec::new(),
            limitations: vec![LIMITATION_MISSING_UPSTREAM_SELECTION_CONTEXT.to_string()],
        }
    }

    /// Record one executed probe.
    pub(crate) fn add_probe(&mut self, probe: SettledDiagnosticProbe) {
        self.probes.push(probe);
    }

    /// Parent observation investigated by these probes.
    pub(crate) fn parent_observation(&self) -> Option<&ObservedRunnerSubjectId> {
        self.parent_observation.as_ref()
    }

    /// Executed probes in execution order.
    pub(crate) fn probes(&self) -> &[SettledDiagnosticProbe] {
        &self.probes
    }

    /// Declared limitations carried by the whole diagnostic set.
    pub(crate) fn limitations(&self) -> &[String] {
        &self.limitations
    }
}

/// Explicit non-claims recorded independently in every receipt (#8173).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DirectDeclaredNonClaims {
    pub rows_considered_by_upstream_completeness: usize,
    pub rows_considered_by_report_totals: usize,
    pub rows_considered_by_transition_or_current_authority: usize,
}

impl DirectDeclaredNonClaims {
    fn none() -> Self {
        Self {
            rows_considered_by_upstream_completeness: 0,
            rows_considered_by_report_totals: 0,
            rows_considered_by_transition_or_current_authority: 0,
        }
    }
}

/// Reference to the frozen upstream observation a diagnostic investigated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DirectParentObservation {
    pub subject_id: String,
    pub upstream_context_digest: String,
    pub harness_status: Option<i32>,
    pub expected_rows: usize,
    pub observed_rows: usize,
    pub missing_rows: usize,
    pub extra_rows: usize,
}

/// One receipt row per settled direct probe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DirectProbeReceiptRow {
    pub authority: String,
    pub path: String,
    pub mode: String,
    pub process_status: Option<i32>,
    pub outcome: DiagnosticProbeOutcome,
    pub status: Option<RunnerStatus>,
    pub assertions_passed: Option<usize>,
    pub assertions_total: Option<usize>,
    pub bucket: Option<String>,
    pub first_diagnostic: Option<String>,
    pub limitations: Vec<String>,
}

/// Separately retained diagnostic receipt.
///
/// This schema is deliberately not a `RunReport`: baseline, bundle, and
/// current-authority ingestion cannot accept it as upstream evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DirectDiagnosticReceipt {
    pub schema_version: String,
    pub parent_observation: Option<DirectParentObservation>,
    pub probes: Vec<DirectProbeReceiptRow>,
    pub limitations: Vec<String>,
    pub declared_non_claims: DirectDeclaredNonClaims,
}

/// Build the diagnostic receipt from a settled diagnostic set plus the frozen
/// upstream facts it investigated.
pub(crate) fn direct_diagnostics_receipt(
    diagnostics: &DirectDiagnosticSet,
    mode: HarnessMode,
    upstream_context_digest: &str,
    harness_status: Option<i32>,
    membership: UpstreamMembershipCounts,
) -> DirectDiagnosticReceipt {
    let parent_observation =
        diagnostics.parent_observation().map(|subject| DirectParentObservation {
            subject_id: subject.as_str().to_string(),
            upstream_context_digest: upstream_context_digest.to_string(),
            harness_status,
            expected_rows: membership.expected,
            observed_rows: membership.observed,
            missing_rows: membership.missing,
            extra_rows: membership.extra,
        });
    let probes = diagnostics
        .probes()
        .iter()
        .map(|probe| {
            let (status, assertions_passed, assertions_total, bucket, first_diagnostic) =
                match probe.record.as_ref() {
                    Some(record) => (
                        Some(record.status),
                        Some(record.assertions_passed),
                        Some(record.assertions_total),
                        record.bucket.clone(),
                        record.first_diagnostic.clone(),
                    ),
                    None => (None, None, None, None, None),
                };
            DirectProbeReceiptRow {
                authority: DIRECT_PROBE_AUTHORITY.to_string(),
                path: probe.subject_path.clone(),
                mode: mode.as_str().to_string(),
                process_status: probe.process_status,
                outcome: probe.outcome,
                status,
                assertions_passed,
                assertions_total,
                bucket,
                first_diagnostic,
                limitations: probe.limitations.clone(),
            }
        })
        .collect();
    DirectDiagnosticReceipt {
        schema_version: DIRECT_DIAGNOSTICS_SCHEMA_VERSION.to_string(),
        parent_observation,
        probes,
        limitations: diagnostics.limitations().to_vec(),
        declared_non_claims: DirectDeclaredNonClaims::none(),
    }
}

/// Receipt path derived from the authoritative report path.
///
/// The suffix keeps the two products physically distinct files.
pub(crate) fn direct_diagnostics_receipt_path(report_path: &Path) -> PathBuf {
    let file_name = report_path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "run-report.json".to_string());
    report_path.with_file_name(format!("{file_name}.direct-diagnostics.json"))
}
