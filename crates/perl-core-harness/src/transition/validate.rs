//! Canonical structural validation for transition evidence.
//!
//! Raw deserialized [`RunReport`] / V2 baseline values are not classification
//! subjects until these invariants hold. Terminal admission runs first: only
//! observations whose typed terminal process outcome is scoreable (#6884) —
//! a clean exit or a recognized runner/mode status — reach structural and
//! count validation. Everything else stays outside [`ValidatedRunReport`]
//! and is handled as `NotProven` by the classifier.

use crate::transition::model::AcceptedBaseline;
use crate::transition::terminal::TerminalProcessOutcome;
use perl_core_harness_types::{
    COMPILE_BASELINE_V2_SCHEMA_VERSION, CompileBaselineV2, HarnessMode, ObservedSemanticBoundary,
    RUN_REPORT_SCHEMA_VERSION, RunFailure, RunFileResult, RunReport, RunnerStatus,
    validate_file_result_mechanisms,
};
use std::collections::{BTreeMap, BTreeSet};

/// Typed reason a raw observation cannot become validated transition evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceValidationError {
    /// Human-readable validation failure.
    pub reason: String,
}

impl EvidenceValidationError {
    fn new(reason: impl Into<String>) -> Self {
        Self { reason: reason.into() }
    }
}

/// Run report that passed canonical structural validation for classification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedRunReport {
    inner: RunReport,
}

impl ValidatedRunReport {
    /// Borrow the validated report.
    pub fn inner(&self) -> &RunReport {
        &self.inner
    }
}

/// Accepted V2 baseline that passed canonical structural validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedCompileBaselineV2 {
    inner: CompileBaselineV2,
}

impl ValidatedCompileBaselineV2 {
    /// Borrow the validated baseline.
    pub fn inner(&self) -> &CompileBaselineV2 {
        &self.inner
    }
}

/// Validate a current run report for definitive transition classification.
///
/// Requires a terminally scoreable observation (#6884 typed admission: clean
/// exit or recognized runner/mode status) plus path uniqueness, per-file
/// assertion bounds, summary/file-result reconciliation, and honest
/// semantic-boundary identity. Terminal validity precedes every count check.
pub fn validate_run_report(
    report: &RunReport,
) -> Result<ValidatedRunReport, EvidenceValidationError> {
    if report.schema_version != RUN_REPORT_SCHEMA_VERSION {
        return Err(EvidenceValidationError::new(format!(
            "unsupported run-report schema {}",
            report.schema_version
        )));
    }
    let terminal = TerminalProcessOutcome::from_harness_status(
        report.harness_status,
        report.runner,
        report.mode,
    );
    if !terminal.is_scoreable() {
        return Err(EvidenceValidationError::new(format!(
            "current harness_status {:?} fails terminal admission ({}): {}",
            report.harness_status,
            terminal.label(),
            terminal.not_proven_reason()
        )));
    }
    if let Some(path) = first_whitespace_contaminated_path(&report.file_results) {
        return Err(EvidenceValidationError::new(format!(
            "current file-result path {path:?} has leading or trailing whitespace"
        )));
    }
    if let Some(path) = first_duplicate_path(&report.file_results) {
        return Err(EvidenceValidationError::new(format!(
            "current observation repeats file-result path {path}"
        )));
    }
    validate_mechanism_claims(report.mode, &report.file_results, "current")?;
    validate_file_result_assertions(&report.file_results, "current")?;
    validate_summary_against_file_results(report)?;
    validate_failure_inventory(
        &report.failures,
        &report.file_results,
        report.mode.as_str(),
        "current",
    )?;
    validate_semantic_boundary_identities(
        &report.semantic_boundaries,
        &report.file_results,
        "current",
    )?;
    Ok(ValidatedRunReport { inner: report.clone() })
}

/// Reject transition evidence whose per-file execution-mechanism claims are not
/// admissible for the mode that produced them.
///
/// This module is the canonical structural validator for transition evidence,
/// so the mechanism contract has to hold here too. Without it, `classify` and
/// `check` would accept an observation or an accepted baseline claiming a rail
/// no evidence backs — the same forgery the receipt, report, and baseline
/// readers already refuse (#14363).
fn validate_mechanism_claims(
    mode: HarnessMode,
    file_results: &[RunFileResult],
    subject: &str,
) -> Result<(), EvidenceValidationError> {
    validate_file_result_mechanisms(mode, file_results).map_err(|(path, violation)| {
        EvidenceValidationError::new(format!("{subject} file result {path}: {violation}"))
    })
}

/// Validate an accepted V2 baseline's structural count and membership invariants.
pub fn validate_compile_baseline_v2(
    baseline: &CompileBaselineV2,
) -> Result<ValidatedCompileBaselineV2, EvidenceValidationError> {
    if baseline.schema_version != COMPILE_BASELINE_V2_SCHEMA_VERSION {
        return Err(EvidenceValidationError::new(format!(
            "unsupported accepted V2 schema {}",
            baseline.schema_version
        )));
    }
    if baseline.report_schema_version != RUN_REPORT_SCHEMA_VERSION {
        return Err(EvidenceValidationError::new(
            "accepted V2 report schema is not the supported run-report version",
        ));
    }
    if let Some(path) = first_whitespace_contaminated_str(
        baseline.file_membership.iter().map(String::as_str),
        "accepted V2 file_membership",
    ) {
        return Err(EvidenceValidationError::new(path));
    }
    if let Some(path) = first_duplicate_str(baseline.file_membership.iter().map(String::as_str)) {
        return Err(EvidenceValidationError::new(format!(
            "accepted V2 file_membership repeats path {path}"
        )));
    }
    validate_mechanism_claims(baseline.mode, &baseline.file_results, "accepted V2")?;
    if let Some(path) = first_whitespace_contaminated_path(&baseline.file_results) {
        return Err(EvidenceValidationError::new(format!(
            "accepted file-result path {path:?} has leading or trailing whitespace"
        )));
    }
    if let Some(path) = first_duplicate_path(&baseline.file_results) {
        return Err(EvidenceValidationError::new(format!(
            "accepted observation repeats file-result path {path}"
        )));
    }
    let membership = baseline.file_membership.iter().map(String::as_str).collect::<BTreeSet<_>>();
    let result_paths =
        baseline.file_results.iter().map(|result| result.path.as_str()).collect::<BTreeSet<_>>();
    if membership != result_paths {
        return Err(EvidenceValidationError::new(
            "accepted V2 file_results do not match immutable file_membership",
        ));
    }
    validate_file_result_assertions(&baseline.file_results, "accepted")?;
    let files_total = baseline.file_results.len();
    let files_passed = count_passed_files(&baseline.file_results);
    let files_failed = files_total.saturating_sub(files_passed);
    let tap_assertions_total =
        checked_assertion_sum(&baseline.file_results, "accepted", assertion_total)?;
    let tap_assertions_passed =
        checked_assertion_sum(&baseline.file_results, "accepted", assertion_passed)?;
    if baseline.files_total != files_total
        || baseline.files_passed != files_passed
        || baseline.files_failed != files_failed
        || baseline.tap_assertions_total != tap_assertions_total
        || baseline.tap_assertions_passed != tap_assertions_passed
        || baseline.tap_assertions_passed > baseline.tap_assertions_total
    {
        return Err(EvidenceValidationError::new(
            "accepted V2 aggregate file/TAP totals do not reconcile with detailed file_results",
        ));
    }
    validate_failure_inventory(
        &baseline.expected_failures,
        &baseline.file_results,
        baseline.mode.as_str(),
        "accepted",
    )?;
    validate_semantic_boundary_identities(
        &baseline.semantic_boundaries,
        &baseline.file_results,
        "accepted",
    )?;
    Ok(ValidatedCompileBaselineV2 { inner: baseline.clone() })
}

/// Validate whichever accepted baseline shape the classifier currently accepts.
pub fn validate_accepted_baseline(
    accepted: &AcceptedBaseline,
) -> Result<(), EvidenceValidationError> {
    match accepted {
        AcceptedBaseline::V2(value) => validate_compile_baseline_v2(value).map(|_| ()),
        AcceptedBaseline::V1(value) => {
            if let Some(path) = first_whitespace_contaminated_path(&value.file_results) {
                return Err(EvidenceValidationError::new(format!(
                    "accepted file-result path {path:?} has leading or trailing whitespace"
                )));
            }
            if let Some(path) = first_duplicate_path(&value.file_results) {
                return Err(EvidenceValidationError::new(format!(
                    "accepted observation repeats file-result path {path}"
                )));
            }
            validate_file_result_assertions(&value.file_results, "accepted")?;
            let files_total = value.file_results.len();
            let files_passed = count_passed_files(&value.file_results);
            let files_failed = files_total.saturating_sub(files_passed);
            let tap_assertions_total =
                checked_assertion_sum(&value.file_results, "accepted", assertion_total)?;
            let tap_assertions_passed =
                checked_assertion_sum(&value.file_results, "accepted", assertion_passed)?;
            if value.files_total != files_total
                || value.files_passed != files_passed
                || value.files_failed != files_failed
                || value.tap_assertions_total != tap_assertions_total
                || value.tap_assertions_passed != tap_assertions_passed
                || value.tap_assertions_passed > value.tap_assertions_total
            {
                return Err(EvidenceValidationError::new(
                    "accepted V1 aggregate file/TAP totals do not reconcile with detailed file_results",
                ));
            }
            validate_failure_inventory(
                &value.expected_failures,
                &value.file_results,
                value.mode.as_str(),
                "accepted",
            )?;
            if let Some(boundaries) = &value.semantic_boundaries {
                validate_semantic_boundary_identities(boundaries, &value.file_results, "accepted")?;
            }
            Ok(())
        }
    }
}

fn validate_failure_inventory(
    failures: &[RunFailure],
    file_results: &[RunFileResult],
    mode: &str,
    side: &str,
) -> Result<(), EvidenceValidationError> {
    let mut failure_paths = BTreeSet::new();
    for failure in failures {
        if failure.path.trim().is_empty() {
            return Err(EvidenceValidationError::new(format!(
                "{side} failure record has an empty path"
            )));
        }
        if failure.path != failure.path.trim() {
            return Err(EvidenceValidationError::new(format!(
                "{side} failure path {:?} has leading or trailing whitespace",
                failure.path
            )));
        }
        if failure.bucket.trim().is_empty() {
            return Err(EvidenceValidationError::new(format!(
                "{side} failure path {} has an empty bucket",
                failure.path
            )));
        }
        if failure.phase.trim().is_empty() {
            return Err(EvidenceValidationError::new(format!(
                "{side} failure path {} has an empty phase",
                failure.path
            )));
        }
        if failure.phase != mode {
            return Err(EvidenceValidationError::new(format!(
                "{side} failure path {} has phase {:?} that does not match harness mode {:?}",
                failure.path, failure.phase, mode
            )));
        }
        if !failure_paths.insert(failure.path.as_str()) {
            return Err(EvidenceValidationError::new(format!(
                "{side} failure inventory repeats path {}",
                failure.path
            )));
        }
        match file_results.iter().find(|result| result.path == failure.path) {
            Some(result) if result.status != RunnerStatus::Fail => {
                return Err(EvidenceValidationError::new(format!(
                    "{side} failure path {} does not identify a failing file",
                    failure.path
                )));
            }
            Some(_) => {}
            None => {
                return Err(EvidenceValidationError::new(format!(
                    "{side} failure path {} has no file-result record",
                    failure.path
                )));
            }
        }
    }
    for result in file_results {
        if result.status == RunnerStatus::Fail && !failure_paths.contains(result.path.as_str()) {
            return Err(EvidenceValidationError::new(format!(
                "{side} failing file {} has no failure record",
                result.path
            )));
        }
    }
    Ok(())
}

fn validate_semantic_boundary_identities(
    boundaries: &[ObservedSemanticBoundary],
    file_results: &[RunFileResult],
    side: &str,
) -> Result<(), EvidenceValidationError> {
    let known_paths: BTreeSet<&str> = file_results.iter().map(|r| r.path.as_str()).collect();
    // Boundary identity is `(path, id, source_span)`, matching the canonical
    // `SemanticBoundaryKey` used by the baseline comparison in `crate::lib`. One file
    // may legitimately emit the same stable id at several distinct source spans (for
    // example, two `runtime_symbolic_reference` sites), so only an exact repeat of the
    // full key is a genuine duplicate.
    let mut seen_boundary_keys: BTreeMap<&str, BTreeSet<(&str, usize, usize)>> = BTreeMap::new();
    for boundary in boundaries {
        if boundary.id.trim().is_empty() {
            return Err(EvidenceValidationError::new(format!(
                "{side} semantic boundary path {} has an empty stable id",
                boundary.path
            )));
        }
        if boundary.path.trim().is_empty() {
            return Err(EvidenceValidationError::new(format!(
                "{side} semantic boundary has an empty path for id {}",
                boundary.id
            )));
        }
        if !known_paths.contains(boundary.path.as_str()) {
            return Err(EvidenceValidationError::new(format!(
                "{side} semantic boundary id {} references path {} which is not in file_results",
                boundary.id, boundary.path
            )));
        }
        let keys_for_path = seen_boundary_keys.entry(boundary.path.as_str()).or_default();
        if !keys_for_path.insert((
            boundary.id.as_str(),
            boundary.source_span.start,
            boundary.source_span.end,
        )) {
            return Err(EvidenceValidationError::new(format!(
                "{side} semantic boundary path {} repeats boundary id {} at source span {}..{}",
                boundary.path, boundary.id, boundary.source_span.start, boundary.source_span.end
            )));
        }
    }
    Ok(())
}

fn validate_file_result_assertions(
    results: &[RunFileResult],
    side: &str,
) -> Result<(), EvidenceValidationError> {
    for result in results {
        if result.assertions_passed > result.assertions_total {
            return Err(EvidenceValidationError::new(format!(
                "{side} file-result path {} has assertions_passed {} greater than assertions_total {}",
                result.path, result.assertions_passed, result.assertions_total
            )));
        }
    }
    Ok(())
}

fn validate_summary_against_file_results(
    report: &RunReport,
) -> Result<(), EvidenceValidationError> {
    let files_total = report.file_results.len();
    let files_passed = count_passed_files(&report.file_results);
    let files_failed = files_total.saturating_sub(files_passed);
    let tap_assertions_total =
        checked_assertion_sum(&report.file_results, "current", assertion_total)?;
    let tap_assertions_passed =
        checked_assertion_sum(&report.file_results, "current", assertion_passed)?;
    if report.summary.files_total != files_total
        || report.summary.files_passed != files_passed
        || report.summary.files_failed != files_failed
        || report.summary.tap_assertions_total != tap_assertions_total
        || report.summary.tap_assertions_passed != tap_assertions_passed
        || report.summary.tap_assertions_passed > report.summary.tap_assertions_total
    {
        return Err(EvidenceValidationError::new(
            "current summary file/TAP totals do not reconcile with detailed file_results",
        ));
    }
    Ok(())
}

fn checked_assertion_sum(
    results: &[RunFileResult],
    side: &str,
    value: fn(&RunFileResult) -> usize,
) -> Result<usize, EvidenceValidationError> {
    let mut total = 0usize;
    for result in results {
        total = total.checked_add(value(result)).ok_or_else(|| {
            EvidenceValidationError::new(format!(
                "{side} aggregate assertion count overflows usize"
            ))
        })?;
    }
    Ok(total)
}

fn assertion_total(result: &RunFileResult) -> usize {
    result.assertions_total
}

fn assertion_passed(result: &RunFileResult) -> usize {
    result.assertions_passed
}

fn count_passed_files(results: &[RunFileResult]) -> usize {
    let mut passed = 0usize;
    for result in results {
        if result.status == RunnerStatus::Pass {
            passed += 1;
        }
    }
    passed
}

fn first_duplicate_path(results: &[RunFileResult]) -> Option<&str> {
    first_duplicate_str(results.iter().map(|result| result.path.as_str()))
}

fn first_duplicate_str<'a>(paths: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    let mut seen = BTreeSet::new();
    paths.into_iter().find(|path| !seen.insert(*path))
}

/// Returns the first file-result path that has leading or trailing whitespace,
/// preventing silent path identity mismatches caused by extraneous whitespace.
fn first_whitespace_contaminated_path(results: &[RunFileResult]) -> Option<&str> {
    results.iter().find_map(|result| {
        if result.path != result.path.trim() { Some(result.path.as_str()) } else { None }
    })
}

/// Returns an error message if any path in `paths` has leading or trailing whitespace.
/// The `label` is used to identify the context in the error message.
fn first_whitespace_contaminated_str<'a>(
    paths: impl IntoIterator<Item = &'a str>,
    label: &str,
) -> Option<String> {
    for path in paths {
        if path != path.trim() {
            return Some(format!("{label} path {path:?} has leading or trailing whitespace"));
        }
    }
    None
}

#[cfg(test)]
mod ripr_inventory_call_observers {
    use super::*;
    use perl_core_harness_types::{
        ExecutionMechanism, HarnessMode, HarnessProfile, HarnessRunner, ObservedSemanticBoundary,
        RunFailure, RunFileResult, RunReport, RunSummary, RunnerStatus, SemanticBoundaryConfidence,
        SemanticBoundaryDisposition, SemanticBoundaryLockScope, SemanticBoundarySourceSpan,
    };
    use std::collections::BTreeMap;

    /// An execute-mode observation shaped like the selected-base receipt.
    fn clean_execute_report() -> RunReport {
        let mut report = clean_report();
        report.mode = HarnessMode::Execute;
        report.harness_status = Some(1);
        for result in &mut report.file_results {
            result.mechanism = Some(ExecutionMechanism::FixtureReplay);
        }
        report
    }

    #[test]
    fn validate_run_report_rejects_a_relabelled_execution_mechanism() {
        // `classify` and `check` reach this validator, so an observation
        // claiming a rail no evidence backs must not become comparable.
        for mechanism in [ExecutionMechanism::EirExecution, ExecutionMechanism::RealPerlOracle] {
            let mut report = clean_execute_report();
            for result in &mut report.file_results {
                result.mechanism = Some(mechanism);
            }

            let err = validate_run_report(&report).expect_err("relabelled mechanism");

            assert!(
                err.reason.contains("no current rail can supply"),
                "unexpected reason for {mechanism}: {}",
                err.reason
            );
        }
    }

    #[test]
    fn validate_run_report_rejects_an_execution_observation_without_a_mechanism() {
        let mut report = clean_execute_report();
        for result in &mut report.file_results {
            result.mechanism = None;
        }

        let err = validate_run_report(&report).expect_err("missing mechanism");

        assert!(
            err.reason.contains("does not declare an execution mechanism"),
            "unexpected reason: {}",
            err.reason
        );
    }

    #[test]
    fn validate_run_report_rejects_a_compile_observation_claiming_execution_evidence() {
        let mut report = clean_report();
        for result in &mut report.file_results {
            result.mechanism = Some(ExecutionMechanism::FixtureReplay);
        }

        let err = validate_run_report(&report).expect_err("mechanism outside execution");

        assert!(
            err.reason.contains("only execution receipts may carry"),
            "unexpected reason: {}",
            err.reason
        );
    }

    #[test]
    fn validate_run_report_accepts_an_honestly_classified_execution_observation() {
        // Opposite-direction control: the contract must not block real evidence.
        validate_run_report(&clean_execute_report())
            .expect("an honestly classified execute observation stays comparable");
    }

    #[test]
    fn validate_run_report_rejects_missing_failure_record() {
        let mut report = clean_report();
        report.file_results[0].status = RunnerStatus::Fail;
        report.file_results[0].assertions_passed = 0;
        report.summary.files_passed = 0;
        report.summary.files_failed = 1;
        report.summary.tap_assertions_passed = 0;
        report.failures.clear();
        let err = validate_run_report(&report).expect_err("missing failure record");
        assert_eq!(
            err,
            EvidenceValidationError::new("current failing file base/0.t has no failure record")
        );
    }

    #[test]
    fn validate_run_report_rejects_empty_failure_bucket() {
        let mut report = clean_report();
        report.file_results[0].status = RunnerStatus::Fail;
        report.file_results[0].assertions_passed = 0;
        report.summary.files_passed = 0;
        report.summary.files_failed = 1;
        report.summary.tap_assertions_passed = 0;
        report.failures = vec![failure("base/0.t", "")];
        let err = validate_run_report(&report).expect_err("empty bucket");
        assert_eq!(
            err,
            EvidenceValidationError::new("current failure path base/0.t has an empty bucket")
        );
    }

    #[test]
    fn validate_run_report_rejects_empty_boundary_id() {
        let mut report = clean_report();
        let mut boundary = boundary();
        boundary.id.clear();
        report.semantic_boundaries.push(boundary);
        let err = validate_run_report(&report).expect_err("empty boundary id");
        assert_eq!(
            err,
            EvidenceValidationError::new(
                "current semantic boundary path base/0.t has an empty stable id"
            )
        );
    }

    #[test]
    fn validate_run_report_rejects_orphan_boundary_path() {
        let mut report = clean_report();
        let mut boundary = boundary();
        boundary.path = "orphan/missing.t".into(); // not in file_results
        report.semantic_boundaries.push(boundary);
        let result = validate_run_report(&report);
        assert!(result.is_err(), "expected Err for orphan boundary path but got Ok");
        if let Err(err) = result {
            assert!(
                err.reason.contains("not in file_results"),
                "unexpected reason: {}",
                err.reason
            );
            assert!(
                err.reason.contains("orphan/missing.t"),
                "path missing from reason: {}",
                err.reason
            );
        }
    }

    #[test]
    fn validate_run_report_rejects_duplicate_boundary_id_for_same_path() {
        let mut report = clean_report();
        report.semantic_boundaries.push(boundary());
        report.semantic_boundaries.push(boundary()); // same path + id
        let result = validate_run_report(&report);
        assert!(result.is_err(), "expected Err for duplicate boundary id but got Ok");
        if let Err(err) = result {
            assert!(
                err.reason.contains("repeats boundary id"),
                "unexpected reason: {}",
                err.reason
            );
        }
    }

    /// Boundary identity is `(path, id, source_span)` — the same canonical key the
    /// baseline comparison uses. One file emitting the same dynamic-boundary category
    /// at two distinct source sites is legitimate evidence, not a duplicate.
    #[test]
    fn validate_run_report_allows_same_id_at_distinct_spans_on_one_path() {
        let mut report = clean_report();
        let mut first = boundary();
        first.source_span = SemanticBoundarySourceSpan { start: 0, end: 1 };
        let mut second = boundary();
        second.source_span = SemanticBoundarySourceSpan { start: 8, end: 12 };
        report.semantic_boundaries.push(first);
        report.semantic_boundaries.push(second);
        assert!(
            validate_run_report(&report).is_ok(),
            "same boundary id at distinct source spans on one path must be accepted"
        );
    }

    #[test]
    fn validate_run_report_allows_same_id_on_different_paths() {
        let mut report = RunReport {
            summary: RunSummary {
                files_total: 2,
                files_passed: 2,
                files_failed: 0,
                tap_assertions_total: 2,
                tap_assertions_passed: 2,
            },
            file_results: vec![
                RunFileResult {
                    mechanism: None,
                    path: "base/0.t".into(),
                    status: RunnerStatus::Pass,
                    assertions_passed: 1,
                    assertions_total: 1,
                },
                RunFileResult {
                    mechanism: None,
                    path: "base/1.t".into(),
                    status: RunnerStatus::Pass,
                    assertions_passed: 1,
                    assertions_total: 1,
                },
            ],
            ..clean_report()
        };
        let mut b1 = boundary();
        b1.path = "base/0.t".into();
        let mut b2 = boundary();
        b2.path = "base/1.t".into(); // same id, different path → allowed
        report.semantic_boundaries.push(b1);
        report.semantic_boundaries.push(b2);
        assert!(validate_run_report(&report).is_ok());
    }

    #[test]
    fn validate_run_report_rejects_whitespace_contaminated_file_result_path() {
        let mut report = clean_report();
        report.file_results[0].path = " base/0.t".into(); // leading space
        let result = validate_run_report(&report);
        assert!(result.is_err(), "expected Err for whitespace-contaminated path but got Ok");
        if let Err(err) = result {
            assert!(
                err.reason.contains("leading or trailing whitespace"),
                "unexpected reason: {}",
                err.reason
            );
        }
    }

    #[test]
    fn validate_run_report_rejects_whitespace_contaminated_failure_path() {
        let mut report = clean_report();
        report.file_results[0].status = RunnerStatus::Fail;
        report.file_results[0].assertions_passed = 0;
        report.summary.files_passed = 0;
        report.summary.files_failed = 1;
        report.summary.tap_assertions_passed = 0;
        // Whitespace-contaminated path in the failure record, but clean in file_results.
        let mut f = failure("base/0.t", "parse_recovery");
        f.path = "base/0.t ".into(); // trailing space
        report.failures = vec![f];
        let result = validate_run_report(&report);
        assert!(result.is_err(), "expected Err for whitespace in failure path but got Ok");
        if let Err(err) = result {
            assert!(
                err.reason.contains("leading or trailing whitespace"),
                "unexpected reason: {}",
                err.reason
            );
        }
    }

    #[test]
    fn validate_run_report_rejects_phase_mismatch() {
        let mut report = clean_report();
        report.file_results[0].status = RunnerStatus::Fail;
        report.file_results[0].assertions_passed = 0;
        report.summary.files_passed = 0;
        report.summary.files_failed = 1;
        report.summary.tap_assertions_passed = 0;
        let mut f = failure("base/0.t", "parse_recovery");
        f.phase = "parse".into(); // mode is compile, phase must match
        report.failures = vec![f];
        let result = validate_run_report(&report);
        assert!(result.is_err(), "expected Err for phase/mode mismatch but got Ok");
        if let Err(err) = result {
            assert!(
                err.reason.contains("does not match harness mode"),
                "unexpected reason: {}",
                err.reason
            );
        }
    }

    #[test]
    fn validate_run_report_rejects_empty_failure_phase() {
        let mut report = clean_report();
        report.file_results[0].status = RunnerStatus::Fail;
        report.file_results[0].assertions_passed = 0;
        report.summary.files_passed = 0;
        report.summary.files_failed = 1;
        report.summary.tap_assertions_passed = 0;
        let mut f = failure("base/0.t", "parse_recovery");
        f.phase = "".into();
        report.failures = vec![f];
        let result = validate_run_report(&report);
        assert!(result.is_err(), "expected Err for empty phase but got Ok");
        if let Err(err) = result {
            assert!(err.reason.contains("empty phase"), "unexpected reason: {}", err.reason);
        }
    }

    #[test]
    fn validate_run_report_accepts_reconciled_failure_inventory() {
        let mut report = clean_report();
        report.file_results[0].status = RunnerStatus::Fail;
        report.file_results[0].assertions_passed = 0;
        report.summary.files_passed = 0;
        report.summary.files_failed = 1;
        report.summary.tap_assertions_passed = 0;
        report.failures = vec![failure("base/0.t", "parse_recovery")];
        assert!(validate_run_report(&report).is_ok());
    }

    /// First #6884 falsifier (historical shape): a runner terminal of 255 with
    /// all-green file/assertion counts must stay outside validation; counts
    /// can never override terminal invalidity.
    #[test]
    fn status_255_with_all_pass_counts_fails_terminal_admission() {
        let mut report = clean_report();
        report.harness_status = Some(255);
        let err = validate_run_report(&report).expect_err("255 all-pass must not validate");
        assert!(err.reason.contains("harness_status"), "unexpected reason: {}", err.reason);
        assert!(err.reason.contains("nonzero_exit"), "unexpected reason: {}", err.reason);
        assert!(err.reason.contains("255"), "reason must carry the observed code");
        assert!(err.reason.contains("counts cannot override"), "unexpected reason: {}", err.reason);
    }

    #[test]
    fn missing_terminal_identity_names_instrument_failure() {
        let mut report = clean_report();
        report.harness_status = None;
        let err = validate_run_report(&report).expect_err("missing status must not validate");
        assert!(err.reason.contains("instrument_failure"), "unexpected reason: {}", err.reason);
    }

    #[test]
    fn terminal_admission_precedes_count_reconciliation() {
        // Even a structurally broken summary must be reported as a terminal
        // admission failure first when the process outcome is invalid.
        let mut report = clean_report();
        report.harness_status = None;
        report.summary.files_passed = 9;
        report.summary.files_total = 1;
        let err = validate_run_report(&report).expect_err("invalid terminal must fail first");
        assert!(err.reason.contains("instrument_failure"), "unexpected reason: {}", err.reason);
    }

    /// Opposite-direction control: execute-mode reports carrying the upstream
    /// scheduler's recognized nonzero completion (#3451) must pass validation
    /// instead of being permanently misclassified by zero-only defensive code.
    #[test]
    fn recognized_execute_nonzero_status_is_terminally_admissible() {
        // `clean_execute_report` is `clean_report` in execute mode with the
        // scheduler's recognized nonzero status and an honest mechanism, so the
        // subject stays terminal admissibility rather than classification.
        assert!(validate_run_report(&clean_execute_report()).is_ok());
    }

    #[test]
    fn validate_run_report_accepts_valid_semantic_boundary() {
        let mut report = clean_report();
        report.semantic_boundaries.push(boundary());
        assert!(validate_run_report(&report).is_ok());
    }

    #[test]
    fn aggregate_helpers_observe_each_report_dimension() {
        let mut results = clean_report().file_results;
        results[0].assertions_total = 3;
        results[0].assertions_passed = 2;

        assert_eq!(checked_assertion_sum(&results, "current", assertion_total), Ok(3));
        assert_eq!(checked_assertion_sum(&results, "current", assertion_passed), Ok(2));
        assert_eq!(count_passed_files(&results), 1);
        results[0].status = RunnerStatus::Fail;
        assert_eq!(count_passed_files(&results), 0);
    }

    fn clean_report() -> RunReport {
        RunReport {
            schema_version: RUN_REPORT_SCHEMA_VERSION.into(),
            commit: "c".into(),
            timestamp: "t".into(),
            perl_ref: "p".into(),
            prepared_tree: "prep".into(),
            run_tree: "run".into(),
            host_perl: "host".into(),
            runner: HarnessRunner::Test,
            mode: HarnessMode::Compile,
            profile: HarnessProfile::Base,
            harness_status: Some(0),
            summary: RunSummary {
                files_total: 1,
                files_passed: 1,
                files_failed: 0,
                tap_assertions_total: 1,
                tap_assertions_passed: 1,
            },
            buckets: BTreeMap::new(),
            file_results: vec![RunFileResult {
                mechanism: None,
                path: "base/0.t".into(),
                status: RunnerStatus::Pass,
                assertions_passed: 1,
                assertions_total: 1,
            }],
            failures: Vec::new(),
            semantic_boundaries: Vec::new(),
        }
    }

    fn failure(path: &str, bucket: &str) -> RunFailure {
        RunFailure {
            path: path.into(),
            phase: "compile".into(),
            bucket: bucket.into(),
            first_diagnostic: "sample".into(),
            workstream: "parser".into(),
            lsp_impact: vec!["diagnostics".into()],
        }
    }

    fn boundary() -> ObservedSemanticBoundary {
        ObservedSemanticBoundary {
            path: "base/0.t".into(),
            id: "boundary".into(),
            disposition: SemanticBoundaryDisposition::Unsupported,
            reason: "sample".into(),
            source_span: SemanticBoundarySourceSpan { start: 0, end: 1 },
            source_kind: "expression".into(),
            confidence: SemanticBoundaryConfidence::Unresolved,
            blocks_compilation: true,
            blocks_downstream_static_facts: true,
            lock_scope: SemanticBoundaryLockScope::None,
            owner_workstream: "parser".into(),
            supporting_test: "tests/sample.rs".into(),
        }
    }
}
