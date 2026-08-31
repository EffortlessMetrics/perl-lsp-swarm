//! Typed separation of upstream observation authority from direct diagnostic
//! probes (#8173).
//!
//! An [`UpstreamObservationSet`] is the frozen product of exactly one upstream
//! harness run: its expected membership, canonical observed rows, explicit
//! discrepancies, and raw process terminal disposition. A
//! [`DirectDiagnosticSet`] records bounded diagnostic probes that may
//! investigate that exact frozen observation but can never flow back into
//! upstream membership, completeness, totals, transitions, or accepted state.
//! Nothing in this module converts one authority class into the other.
//!
//! Extras census honesty: rows observed outside expected selection membership
//! never enter totals, but they also cannot vanish from every durable product
//! (#8173). Until the full membership-equality law lands (#7737/#12106),
//! extras are tolerated at freeze time, surfaced through a `tracing::warn!`
//! during settle, and persisted as the `extra_rows` census on the retained
//! direct-diagnostics receipt even when no probe runs.
//!
//! Terminal identity comes exclusively from the shared
//! [`TerminalProcessOutcome`] taxonomy (#6884/#12377); this module defines no
//! parallel terminal vocabulary.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use color_eyre::eyre::{Result, bail};
use perl_core_harness_types::{
    DiscoveredTest, HarnessMode, HarnessProfile, HarnessRunner, RunnerRecord, RunnerStatus,
};
use serde::{Deserialize, Serialize};

use crate::normalization::sha256_digest_bytes;
use crate::normalize_test_path;
use crate::transition::TerminalProcessOutcome;

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

/// Why probe results could not be trusted: stale probe-context removal
/// failed, so leftovers from a previous run are indistinguishable from fresh
/// bytes.
pub(crate) const LIMITATION_PROBE_CONTEXT_STALE_REMOVAL_FAILED: &str =
    "direct_probe_context_stale_removal_failed";

/// Why a probe row was excluded: its path did not normalize.
pub(crate) const LIMITATION_PROBE_ROW_MALFORMED_PATH: &str = "direct_probe_row_malformed_path";

/// Why a probe row was excluded: its normalized path appeared more than once.
pub(crate) const LIMITATION_PROBE_ROW_DUPLICATE: &str = "direct_probe_row_duplicate";

/// Immutable identity of the expected upstream runner subject.
///
/// This is deliberately narrower than [`UpstreamObservationId`]: equal target
/// membership does not imply equal observed evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ObservedRunnerSubjectId(String);

impl ObservedRunnerSubjectId {
    /// Stable receipt form of the subject identity.
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

/// Immutable identity of one complete frozen upstream observation.
///
/// The digest binds runner/profile/mode, ordered expected rows, canonical
/// observed records, missing and extra discrepancies, and terminal
/// disposition. A direct diagnostic set can obtain this identity only from an
/// already-settled [`UpstreamObservationSet`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UpstreamObservationId(String);

impl UpstreamObservationId {
    /// Stable receipt form of the observation identity.
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

/// Frozen upstream observation: expected membership, observed rows,
/// discrepancies, and terminal state.
///
/// Construction goes through [`UpstreamObservationSet::settle`] only, which
/// validates rows and derives the observation digest before any summary or
/// diagnostic exists. Fields are private so no mixed-authority collection or
/// caller-supplied digest can be substituted later.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UpstreamObservationSet {
    subject: ObservedRunnerSubjectId,
    observation_id: UpstreamObservationId,
    mode: HarnessMode,
    expected: Vec<ExpectedInvocationId>,
    observed: BTreeMap<InvocationId, SettledInvocation>,
    extras: Vec<SettledInvocation>,
    terminal: TerminalProcessOutcome,
    harness_status: Option<i32>,
}

impl UpstreamObservationSet {
    /// Freeze one upstream observation from exactly one context read.
    ///
    /// Missing expected rows stay missing: nothing here repairs membership.
    /// Duplicate normalized expected rows and malformed paths fail closed
    /// instead of silently collapsing into totals.
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
            let invocation = InvocationId(normalized);
            if !expected_ids.insert(invocation.clone()) {
                bail!("duplicate discovered test path for {}", invocation.as_str());
            }
            expected.push(ExpectedInvocationId { invocation });
        }

        let mut observed = BTreeMap::<InvocationId, SettledInvocation>::new();
        let mut extras = Vec::<SettledInvocation>::new();
        for record in records {
            let normalized = normalize_test_path(&record.path).ok_or_else(|| {
                color_eyre::eyre::eyre!(
                    "upstream runner record path did not normalize: {}",
                    record.path
                )
            })?;
            let invocation = InvocationId(normalized);
            if !expected_ids.contains(&invocation) {
                extras.push(SettledInvocation { invocation, record: record.clone() });
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

        // Extras never enter totals, but silence would accept a wrong
        // selection membership with no durable trace at all; warn at freeze
        // time and persist the census on the retained receipt
        // (#7737/#12106 deferral).
        if !extras.is_empty() {
            let extra_rows = extras.len();
            tracing::warn!(
                "perl-core-harness: {extra_rows} upstream row(s) fell outside expected selection membership and never enter report totals"
            );
        }

        // The legacy persisted terminal fact and its shared-taxonomy
        // admission decision are both frozen together (#6884/#12377): the
        // admission gate later reads the frozen outcome, never a fresh
        // classification.
        let terminal = TerminalProcessOutcome::from_harness_status(terminal_status, runner, mode);
        let subject = subject_id(runner, mode, profile, &expected);
        let observation_id =
            observation_id(runner, mode, profile, &expected, &observed, &extras, &terminal)?;
        Ok(Self {
            subject,
            observation_id,
            mode,
            expected,
            observed,
            extras,
            terminal,
            harness_status: terminal_status,
        })
    }

    /// Frozen expected-subject identity of this observation.
    pub(crate) fn subject(&self) -> &ObservedRunnerSubjectId {
        &self.subject
    }

    /// Digest of the complete frozen observation, not merely its subject.
    pub(crate) fn observation_id(&self) -> &UpstreamObservationId {
        &self.observation_id
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
            extra: self.extras.len(),
        }
    }

    /// Shared-taxonomy terminal outcome frozen at settle time (#6884).
    pub(crate) fn terminal(&self) -> &TerminalProcessOutcome {
        &self.terminal
    }

    /// Legacy persisted exit-status identity frozen at settle time.
    pub(crate) fn harness_status(&self) -> Option<i32> {
        self.harness_status
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

fn observation_id(
    runner: HarnessRunner,
    mode: HarnessMode,
    profile: HarnessProfile,
    expected: &[ExpectedInvocationId],
    observed: &BTreeMap<InvocationId, SettledInvocation>,
    extras: &[SettledInvocation],
    terminal: &TerminalProcessOutcome,
) -> Result<UpstreamObservationId> {
    let mut canonical = Vec::<u8>::new();
    append_canonical_field(&mut canonical, "schema", b"perl_core_harness.upstream_observation.v1");
    append_canonical_field(&mut canonical, "runner", runner.as_str().as_bytes());
    append_canonical_field(&mut canonical, "mode", mode.as_str().as_bytes());
    append_canonical_field(&mut canonical, "profile", profile.as_str().as_bytes());

    for item in expected {
        append_canonical_field(&mut canonical, "expected", item.as_str().as_bytes());
    }
    for (invocation, settled) in observed {
        append_canonical_field(&mut canonical, "observed_id", invocation.as_str().as_bytes());
        let encoded = serde_json::to_vec(settled.record())?;
        append_canonical_field(&mut canonical, "observed_record", &encoded);
    }
    for item in expected {
        if !observed.contains_key(&item.invocation) {
            append_canonical_field(&mut canonical, "missing", item.as_str().as_bytes());
        }
    }

    // Extra rows are canonicalized by normalized identity and complete record
    // bytes so input iteration order cannot change the observation digest.
    let mut canonical_extras = extras
        .iter()
        .map(|settled| {
            let encoded = serde_json::to_vec(settled.record())?;
            Ok((settled.invocation.as_str().to_string(), encoded))
        })
        .collect::<Result<Vec<_>>>()?;
    canonical_extras.sort();
    for (invocation, encoded) in canonical_extras {
        append_canonical_field(&mut canonical, "extra_id", invocation.as_bytes());
        append_canonical_field(&mut canonical, "extra_record", &encoded);
    }

    append_canonical_field(&mut canonical, "terminal", terminal.label().as_bytes());
    append_terminal_identity_fields(&mut canonical, terminal);

    Ok(UpstreamObservationId(sha256_digest_bytes(&canonical)))
}

/// Canonical variant-payload identity of the shared terminal taxonomy.
///
/// The label alone cannot distinguish, for example, a recognized nonzero
/// completion from an unproven one carrying the same status byte; the digest
/// therefore binds every behavior-bearing payload field (#6884/#12377).
fn append_terminal_identity_fields(target: &mut Vec<u8>, terminal: &TerminalProcessOutcome) {
    match terminal {
        TerminalProcessOutcome::CleanExit => {}
        TerminalProcessOutcome::RecognizedRunnerStatus { code, meaning } => {
            append_canonical_field(target, "terminal_code", code.to_string().as_bytes());
            append_canonical_field(target, "terminal_meaning", meaning.as_bytes());
        }
        TerminalProcessOutcome::NonZeroExit { code } => {
            append_canonical_field(target, "terminal_code", code.to_string().as_bytes());
        }
        TerminalProcessOutcome::Signal { signal, name } => {
            append_canonical_field(target, "terminal_signal", signal.to_string().as_bytes());
            let name = name.as_deref().unwrap_or("none");
            append_canonical_field(target, "terminal_signal_name", name.as_bytes());
        }
        TerminalProcessOutcome::TimedOut
        | TerminalProcessOutcome::Cancelled
        | TerminalProcessOutcome::SpawnFailed
        | TerminalProcessOutcome::OutputTruncated
        | TerminalProcessOutcome::InstrumentFailure
        | TerminalProcessOutcome::CleanupFailure => {}
    }
}

fn append_canonical_field(target: &mut Vec<u8>, label: &str, value: &[u8]) {
    target.extend_from_slice(label.as_bytes());
    target.push(b'=');
    target.extend_from_slice(value.len().to_string().as_bytes());
    target.push(b'\n');
    target.extend_from_slice(value);
    target.push(b'\n');
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

    /// Probe that cannot claim any result for a recorded reason, such as its
    /// only candidate rows coming from an untrusted context.
    pub(crate) fn unavailable_for_reason(
        subject_path: &str,
        process_status: Option<i32>,
        limitation: &str,
    ) -> Self {
        Self {
            subject_path: subject_path.to_string(),
            process_status,
            outcome: DiagnosticProbeOutcome::Unavailable,
            record: None,
            limitations: vec![limitation.to_string()],
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

/// Frozen parent facts copied from one settled upstream observation.
///
/// This is the only source of parent lineage for diagnostic receipts; no
/// caller-supplied string or count can replace any of it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FrozenDirectParentObservation {
    subject: ObservedRunnerSubjectId,
    observation_id: UpstreamObservationId,
    mode: String,
    terminal_label: String,
    harness_status: Option<i32>,
    membership: UpstreamMembershipCounts,
}

/// Separate diagnostic product: probes investigating a frozen discrepancy.
///
/// This type has no conversion into [`UpstreamObservationSet`], no shared
/// collection with it, and no path into report totals or accepted state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DirectDiagnosticSet {
    parent_observation: Option<FrozenDirectParentObservation>,
    probes: Vec<SettledDiagnosticProbe>,
    limitations: Vec<String>,
}

impl DirectDiagnosticSet {
    /// Plan diagnostics against one exact frozen upstream observation.
    pub(crate) fn plan(parent: &UpstreamObservationSet) -> Self {
        Self {
            parent_observation: Some(FrozenDirectParentObservation {
                subject: parent.subject().clone(),
                observation_id: parent.observation_id().clone(),
                mode: parent.mode.as_str().to_string(),
                terminal_label: parent.terminal().label().to_string(),
                harness_status: parent.harness_status(),
                membership: parent.counts(),
            }),
            probes: Vec::new(),
            limitations: vec![LIMITATION_MISSING_UPSTREAM_SELECTION_CONTEXT.to_string()],
        }
    }

    /// Record one executed probe.
    pub(crate) fn add_probe(&mut self, probe: SettledDiagnosticProbe) {
        self.probes.push(probe);
    }

    /// Record one additional set-level limitation (idempotent per reason).
    pub(crate) fn add_limitation(&mut self, limitation: String) {
        if !self.limitations.contains(&limitation) {
            self.limitations.push(limitation);
        }
    }

    /// Parent observation investigated by these probes.
    fn parent_observation(&self) -> Option<&FrozenDirectParentObservation> {
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

/// Settle executed probes against raw probe-context rows under diagnostic
/// authority only.
///
/// The upstream lane fails closed on malformed and duplicate rows; the probe
/// lane mirrors that honesty without aborting diagnostics: malformed and
/// duplicate rows become visible set-level limitations and can never back a
/// settled outcome. When stale-context removal failed, fresh bytes cannot be
/// distinguished from leftovers from a previous run, so no executed probe may
/// claim a result at all (#8173).
pub(crate) fn settle_probe_context_rows(
    diagnostics: &mut DirectDiagnosticSet,
    executed: &[(String, Option<i32>)],
    rows: Vec<RunnerRecord>,
    context_trusted: bool,
) {
    let mut by_path = BTreeMap::<String, Vec<RunnerRecord>>::new();
    for row in rows {
        match normalize_test_path(&row.path) {
            Some(path) => by_path.entry(path).or_default().push(row),
            None => {
                tracing::warn!(
                    "perl-core-harness: direct diagnostic probe row path did not normalize: {}",
                    row.path
                );
                diagnostics.add_limitation(LIMITATION_PROBE_ROW_MALFORMED_PATH.to_string());
            }
        }
    }
    for (path, path_rows) in &by_path {
        if path_rows.len() > 1 {
            tracing::warn!(
                "perl-core-harness: direct diagnostic probe context contains {} rows for {path}; none may stand for the subject",
                path_rows.len()
            );
            diagnostics.add_limitation(LIMITATION_PROBE_ROW_DUPLICATE.to_string());
        }
    }

    if !context_trusted {
        diagnostics.add_limitation(LIMITATION_PROBE_CONTEXT_STALE_REMOVAL_FAILED.to_string());
    }
    for (subject_path, process_status) in executed {
        let probe = if !context_trusted {
            SettledDiagnosticProbe::unavailable_for_reason(
                subject_path,
                *process_status,
                LIMITATION_PROBE_CONTEXT_STALE_REMOVAL_FAILED,
            )
        } else {
            match by_path.get(subject_path).map(Vec::as_slice) {
                Some([record]) => {
                    SettledDiagnosticProbe::settled(subject_path, *process_status, record.clone())
                }
                Some(_) => SettledDiagnosticProbe::unavailable_for_reason(
                    subject_path,
                    *process_status,
                    LIMITATION_PROBE_ROW_DUPLICATE,
                ),
                None => SettledDiagnosticProbe::unavailable(subject_path, *process_status),
            }
        };
        diagnostics.add_probe(probe);
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

/// Reference to the exact frozen upstream observation a diagnostic
/// investigated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DirectParentObservation {
    pub subject_id: String,
    pub observation_digest: String,
    pub terminal_disposition: String,
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

/// Build the diagnostic receipt from a settled diagnostic set.
///
/// Every load-bearing parent fact was copied from [`UpstreamObservationSet`]
/// by [`DirectDiagnosticSet::plan`]; this function accepts nothing else.
pub(crate) fn direct_diagnostics_receipt(
    diagnostics: &DirectDiagnosticSet,
) -> DirectDiagnosticReceipt {
    let parent_observation =
        diagnostics.parent_observation().map(|parent| DirectParentObservation {
            subject_id: parent.subject.as_str().to_string(),
            observation_digest: parent.observation_id.as_str().to_string(),
            terminal_disposition: parent.terminal_label.clone(),
            harness_status: parent.harness_status,
            expected_rows: parent.membership.expected,
            observed_rows: parent.membership.observed,
            missing_rows: parent.membership.missing,
            extra_rows: parent.membership.extra,
        });
    let mode =
        diagnostics.parent_observation().map(|parent| parent.mode.as_str()).unwrap_or("unknown");
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
                mode: mode.to_string(),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn discovered() -> Vec<DiscoveredTest> {
        vec![DiscoveredTest { path: "base/ok.t".into(), root: "base".into() }]
    }

    fn record(assertions: usize) -> RunnerRecord {
        RunnerRecord {
            mechanism: None,
            schema_version: "perl_core_harness.runner_record.v1".into(),
            mode: "parse".into(),
            path: "base/ok.t".into(),
            status: RunnerStatus::Pass,
            assertions_passed: assertions,
            assertions_total: assertions,
            bucket: None,
            first_diagnostic: None,
            semantic_boundaries: Vec::new(),
        }
    }

    fn observation(
        assertions: usize,
        terminal_status: Option<i32>,
    ) -> Result<UpstreamObservationSet> {
        UpstreamObservationSet::settle(
            HarnessRunner::Test,
            HarnessMode::Parse,
            HarnessProfile::Base,
            &discovered(),
            &[record(assertions)],
            terminal_status,
        )
    }

    #[test]
    fn observation_digest_changes_when_observed_payload_changes() -> Result<()> {
        let first = observation(1, Some(0))?;
        let second = observation(2, Some(0))?;

        assert_eq!(first.subject(), second.subject());
        assert_ne!(first.observation_id(), second.observation_id());

        let first_receipt = direct_diagnostics_receipt(&DirectDiagnosticSet::plan(&first));
        let second_receipt = direct_diagnostics_receipt(&DirectDiagnosticSet::plan(&second));
        assert_ne!(
            first_receipt.parent_observation.as_ref().map(|parent| &parent.observation_digest),
            second_receipt.parent_observation.as_ref().map(|parent| &parent.observation_digest)
        );
        Ok(())
    }

    #[test]
    fn observation_digest_changes_when_terminal_disposition_changes() -> Result<()> {
        let clean = observation(1, Some(0))?;
        let failed = observation(1, Some(7))?;

        assert_eq!(clean.subject(), failed.subject());
        assert_ne!(clean.observation_id(), failed.observation_id());

        // Same status byte, different shared-taxonomy admission class: in
        // execute mode the scheduler's nonzero exit is a recognized completion
        // (#3451), in parse mode it is unproven. The digest must bind that
        // distinction even though the legacy status identity is equal.
        let recognized = UpstreamObservationSet::settle(
            HarnessRunner::Test,
            HarnessMode::Execute,
            HarnessProfile::Base,
            &discovered(),
            &[record(1)],
            Some(1),
        )?;
        let unproven = UpstreamObservationSet::settle(
            HarnessRunner::Test,
            HarnessMode::Parse,
            HarnessProfile::Base,
            &discovered(),
            &[record(1)],
            Some(1),
        )?;
        assert_eq!(
            recognized.terminal().label(),
            "recognized_runner_status",
            "execute/test/1 is the #3451 recognized completion state"
        );
        assert_eq!(unproven.terminal().label(), "nonzero_exit");
        assert_ne!(recognized.observation_id(), unproven.observation_id());
        assert_ne!(
            direct_diagnostics_receipt(&DirectDiagnosticSet::plan(&recognized))
                .parent_observation
                .as_ref()
                .map(|parent| parent.terminal_disposition.clone()),
            direct_diagnostics_receipt(&DirectDiagnosticSet::plan(&unproven))
                .parent_observation
                .as_ref()
                .map(|parent| parent.terminal_disposition.clone())
        );
        Ok(())
    }

    #[test]
    fn receipt_parent_lineage_comes_only_from_the_frozen_plan() -> Result<()> {
        let parent = observation(1, Some(0))?;
        let diagnostics = DirectDiagnosticSet::plan(&parent);

        let first = direct_diagnostics_receipt(&diagnostics);
        let second = direct_diagnostics_receipt(&DirectDiagnosticSet::plan(&parent));
        assert_eq!(first, second);
        let frozen = first.parent_observation.as_ref().expect("planned diagnostics have a parent");
        assert_eq!(frozen.harness_status, Some(0));
        assert_eq!(frozen.terminal_disposition, "clean_exit");
        assert_eq!(frozen.expected_rows, 1);
        assert_eq!(frozen.observed_rows, 1);
        assert_eq!(frozen.missing_rows, 0);
        assert_eq!(frozen.extra_rows, 0);
        assert_eq!(first.probes.len(), 0);
        Ok(())
    }

    #[test]
    fn settle_fails_closed_on_duplicate_expected_rows() -> Result<()> {
        let duplicated = vec![
            DiscoveredTest { path: "base/ok.t".into(), root: "base".into() },
            DiscoveredTest { path: "base/ok.t".into(), root: "base".into() },
        ];
        let Err(err) = UpstreamObservationSet::settle(
            HarnessRunner::Test,
            HarnessMode::Parse,
            HarnessProfile::Base,
            &duplicated,
            &[record(1)],
            Some(0),
        ) else {
            bail!("duplicate normalized discovery rows must fail closed");
        };
        assert!(
            err.to_string().contains("duplicate discovered test path"),
            "discriminating duplicate-discovery error required: {err}"
        );
        Ok(())
    }

    #[test]
    fn settle_fails_closed_on_malformed_upstream_row_paths() -> Result<()> {
        let mut malformed = record(1);
        malformed.path = "not-a-normalized-test.txt".into();
        let Err(err) = UpstreamObservationSet::settle(
            HarnessRunner::Test,
            HarnessMode::Parse,
            HarnessProfile::Base,
            &discovered(),
            &[malformed],
            Some(0),
        ) else {
            bail!("an upstream row whose path does not normalize must fail closed");
        };
        assert!(
            err.to_string().contains("did not normalize"),
            "discriminating normalization error required: {err}"
        );
        Ok(())
    }
}
