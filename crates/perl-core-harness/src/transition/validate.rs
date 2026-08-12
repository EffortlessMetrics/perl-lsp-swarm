//! Canonical structural validation for transition evidence.
//!
//! Raw deserialized [`RunReport`] / V2 baseline values are not classification
//! subjects until these invariants hold. Incomplete harness runs
//! (`harness_status != Some(0)`) stay outside [`ValidatedRunReport`] and are
//! handled as `NotProven` by the classifier.

use crate::transition::model::AcceptedBaseline;
use perl_core_harness_types::{
    COMPILE_BASELINE_V2_SCHEMA_VERSION, CompileBaselineV2, RUN_REPORT_SCHEMA_VERSION,
    RunFileResult, RunReport, RunnerStatus,
};
use std::collections::BTreeSet;

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
/// Requires a complete successful harness execution (`harness_status == Some(0)`)
/// plus path uniqueness, per-file assertion bounds, and summary/file-result
/// reconciliation.
pub fn validate_run_report(
    report: &RunReport,
) -> Result<ValidatedRunReport, EvidenceValidationError> {
    if report.schema_version != RUN_REPORT_SCHEMA_VERSION {
        return Err(EvidenceValidationError::new(format!(
            "unsupported run-report schema {}",
            report.schema_version
        )));
    }
    if report.harness_status != Some(0) {
        return Err(EvidenceValidationError::new(format!(
            "current harness_status {:?} is not a complete successful run",
            report.harness_status
        )));
    }
    if let Some(path) = first_duplicate_path(&report.file_results) {
        return Err(EvidenceValidationError::new(format!(
            "current observation repeats file-result path {path}"
        )));
    }
    validate_file_result_assertions(&report.file_results, "current")?;
    validate_summary_against_file_results(report)?;
    Ok(ValidatedRunReport { inner: report.clone() })
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
    if let Some(path) = first_duplicate_str(baseline.file_membership.iter().map(String::as_str)) {
        return Err(EvidenceValidationError::new(format!(
            "accepted V2 file_membership repeats path {path}"
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
    let files_passed =
        baseline.file_results.iter().filter(|result| result.status == RunnerStatus::Pass).count();
    let files_failed = files_total.saturating_sub(files_passed);
    let tap_assertions_total = checked_assertion_sum(&baseline.file_results, "accepted", |result| {
        result.assertions_total
    })?;
    let tap_assertions_passed = checked_assertion_sum(&baseline.file_results, "accepted", |result| {
        result.assertions_passed
    })?;
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
    Ok(ValidatedCompileBaselineV2 { inner: baseline.clone() })
}

/// Validate whichever accepted baseline shape the classifier currently accepts.
pub fn validate_accepted_baseline(
    accepted: &AcceptedBaseline,
) -> Result<(), EvidenceValidationError> {
    match accepted {
        AcceptedBaseline::V2(value) => validate_compile_baseline_v2(value).map(|_| ()),
        AcceptedBaseline::V1(value) => {
            if let Some(path) = first_duplicate_path(&value.file_results) {
                return Err(EvidenceValidationError::new(format!(
                    "accepted observation repeats file-result path {path}"
                )));
            }
            validate_file_result_assertions(&value.file_results, "accepted")?;
            let files_total = value.file_results.len();
            let files_passed = value
                .file_results
                .iter()
                .filter(|result| result.status == RunnerStatus::Pass)
                .count();
            let files_failed = files_total.saturating_sub(files_passed);
            let tap_assertions_total = checked_assertion_sum(&value.file_results, "accepted", |result| {
                result.assertions_total
            })?;
            let tap_assertions_passed = checked_assertion_sum(&value.file_results, "accepted", |result| {
                result.assertions_passed
            })?;
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
            Ok(())
        }
    }
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
    let files_passed =
        report.file_results.iter().filter(|result| result.status == RunnerStatus::Pass).count();
    let files_failed = files_total.saturating_sub(files_passed);
    let tap_assertions_total = checked_assertion_sum(&report.file_results, "current", |result| {
        result.assertions_total
    })?;
    let tap_assertions_passed = checked_assertion_sum(&report.file_results, "current", |result| {
        result.assertions_passed
    })?;
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
    value: impl Fn(&RunFileResult) -> usize,
) -> Result<usize, EvidenceValidationError> {
    results.iter().try_fold(0usize, |total, result| {
        total.checked_add(value(result)).ok_or_else(|| {
            EvidenceValidationError::new(format!(
                "{side} aggregate assertion count overflows usize"
            ))
        })
    })
}

fn first_duplicate_path(results: &[RunFileResult]) -> Option<&str> {
    first_duplicate_str(results.iter().map(|result| result.path.as_str()))
}

fn first_duplicate_str<'a>(paths: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    let mut seen = BTreeSet::new();
    paths.into_iter().find(|path| !seen.insert(*path))
}
