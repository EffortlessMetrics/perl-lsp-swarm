//! Canonical structural validation for transition evidence.
//!
//! Raw deserialized [`RunReport`] / V2 baseline values are not classification
//! subjects until these invariants hold. Incomplete harness runs
//! (`harness_status != Some(0)`) stay outside [`ValidatedRunReport`] and are
//! handled as `NotProven` by the classifier.

use crate::transition::model::AcceptedBaseline;
use perl_core_harness_types::{
    COMPILE_BASELINE_V2_SCHEMA_VERSION, CompileBaselineV2, ObservedSemanticBoundary,
    RUN_REPORT_SCHEMA_VERSION, RunFailure, RunFileResult, RunReport, RunnerStatus,
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
/// plus path uniqueness, per-file assertion bounds, summary/file-result
/// reconciliation, and failure / semantic-boundary identity cardinality.
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
    validate_failure_inventory(&report.failures, &report.file_results, "current")?;
    validate_semantic_boundary_identities(&report.semantic_boundaries, "current")?;
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
    let tap_assertions_total =
        checked_assertion_sum(&baseline.file_results, "accepted", |result| {
            result.assertions_total
        })?;
    let tap_assertions_passed =
        checked_assertion_sum(&baseline.file_results, "accepted", |result| {
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
    validate_failure_inventory(&baseline.expected_failures, &baseline.file_results, "accepted")?;
    validate_semantic_boundary_identities(&baseline.semantic_boundaries, "accepted")?;
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
            let tap_assertions_total =
                checked_assertion_sum(&value.file_results, "accepted", |result| {
                    result.assertions_total
                })?;
            let tap_assertions_passed =
                checked_assertion_sum(&value.file_results, "accepted", |result| {
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
            validate_failure_inventory(&value.expected_failures, &value.file_results, "accepted")?;
            if let Some(boundaries) = &value.semantic_boundaries {
                validate_semantic_boundary_identities(boundaries, "accepted")?;
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
    let tap_assertions_total =
        checked_assertion_sum(&report.file_results, "current", |result| result.assertions_total)?;
    let tap_assertions_passed =
        checked_assertion_sum(&report.file_results, "current", |result| result.assertions_passed)?;
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

fn validate_failure_inventory(
    failures: &[RunFailure],
    file_results: &[RunFileResult],
    side: &str,
) -> Result<(), EvidenceValidationError> {
    let mut keys = BTreeSet::new();
    let mut failure_paths = BTreeSet::new();
    for failure in failures {
        if failure.path.trim().is_empty() {
            return Err(EvidenceValidationError::new(format!(
                "{side} failure inventory contains an empty path"
            )));
        }
        if failure.phase.trim().is_empty() {
            return Err(EvidenceValidationError::new(format!(
                "{side} failure path {} has an empty phase",
                failure.path
            )));
        }
        if failure.bucket.trim().is_empty() {
            return Err(EvidenceValidationError::new(format!(
                "{side} failure path {} has an empty bucket",
                failure.path
            )));
        }
        if failure.workstream.trim().is_empty() {
            return Err(EvidenceValidationError::new(format!(
                "{side} failure path {} has an empty workstream",
                failure.path
            )));
        }
        let key = (failure.path.as_str(), failure.phase.as_str(), failure.bucket.as_str());
        if !keys.insert(key) {
            return Err(EvidenceValidationError::new(format!(
                "{side} failure inventory repeats identity {}/{}/{}",
                failure.path, failure.phase, failure.bucket
            )));
        }
        failure_paths.insert(failure.path.as_str());
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
    side: &str,
) -> Result<(), EvidenceValidationError> {
    let mut keys = BTreeSet::new();
    for boundary in boundaries {
        if boundary.path.trim().is_empty() {
            return Err(EvidenceValidationError::new(format!(
                "{side} semantic boundary has an empty path"
            )));
        }
        if boundary.id.trim().is_empty() {
            return Err(EvidenceValidationError::new(format!(
                "{side} semantic boundary path {} has an empty stable id",
                boundary.path
            )));
        }
        if boundary.reason.trim().is_empty() {
            return Err(EvidenceValidationError::new(format!(
                "{side} semantic boundary {} has an empty reason",
                boundary.id
            )));
        }
        if boundary.source_kind.trim().is_empty() {
            return Err(EvidenceValidationError::new(format!(
                "{side} semantic boundary {} has an empty source kind",
                boundary.id
            )));
        }
        if boundary.owner_workstream.trim().is_empty() {
            return Err(EvidenceValidationError::new(format!(
                "{side} semantic boundary {} has no owning workstream",
                boundary.id
            )));
        }
        if boundary.supporting_test.trim().is_empty() {
            return Err(EvidenceValidationError::new(format!(
                "{side} semantic boundary {} has no supporting test",
                boundary.id
            )));
        }
        if boundary.source_span.start > boundary.source_span.end {
            return Err(EvidenceValidationError::new(format!(
                "{side} semantic boundary {} has a reversed source span",
                boundary.id
            )));
        }
        let key = (
            boundary.path.as_str(),
            boundary.id.as_str(),
            boundary.source_span.start,
            boundary.source_span.end,
        );
        if !keys.insert(key) {
            return Err(EvidenceValidationError::new(format!(
                "{side} semantic boundary inventory contains a duplicate key"
            )));
        }
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

#[cfg(test)]
mod ripr_inventory_observers {
    use super::*;
    use perl_core_harness_types::{
        COMPILE_BASELINE_V2_SCHEMA_VERSION, HarnessMode, HarnessProfile, HarnessRunner,
        ObservedSemanticBoundary, RUN_REPORT_SCHEMA_VERSION, RunFailure, RunFileResult, RunSummary,
        RunnerStatus, SemanticBoundaryConfidence, SemanticBoundaryDisposition,
        SemanticBoundaryLockScope, SemanticBoundarySourceSpan,
    };
    use std::collections::BTreeMap;

    /// RIPR-named observer for failure-inventory and boundary-identity validators.
    #[test]
    fn inventory_validator_call_presence_observer() {
        let ok_report = sample_report(1, 1);
        assert!(validate_run_report(&ok_report).is_ok());

        let mut missing = sample_report(1, 0);
        missing.failures.clear();
        let missing_err = match validate_run_report(&missing) {
            Err(error) => error,
            Ok(_) => panic!("missing failure must reject"),
        };
        assert!(missing_err.reason.contains("has no failure record"));

        let mut empty_bucket = sample_report(1, 0);
        empty_bucket.failures[0].bucket.clear();
        let bucket_err = match validate_run_report(&empty_bucket) {
            Err(error) => error,
            Ok(_) => panic!("empty bucket must reject"),
        };
        assert!(bucket_err.reason.contains("empty bucket"));

        let mut empty_phase = sample_report(1, 0);
        empty_phase.failures[0].phase.clear();
        let phase_err = match validate_run_report(&empty_phase) {
            Err(error) => error,
            Ok(_) => panic!("empty phase must reject"),
        };
        assert!(phase_err.reason.contains("empty phase"));

        let mut empty_path = sample_report(1, 0);
        empty_path.failures[0].path.clear();
        let path_err = match validate_run_report(&empty_path) {
            Err(error) => error,
            Ok(_) => panic!("empty path must reject"),
        };
        assert!(path_err.reason.contains("empty path"));

        let mut empty_workstream = sample_report(1, 0);
        empty_workstream.failures[0].workstream.clear();
        let work_err = match validate_run_report(&empty_workstream) {
            Err(error) => error,
            Ok(_) => panic!("empty workstream must reject"),
        };
        assert!(work_err.reason.contains("empty workstream"));

        let mut duplicate = sample_report(1, 0);
        duplicate.failures.push(duplicate.failures[0].clone());
        let dup_err = match validate_run_report(&duplicate) {
            Err(error) => error,
            Ok(_) => panic!("duplicate failure must reject"),
        };
        assert!(dup_err.reason.contains("repeats identity"));

        let mut boundary = sample_report(1, 1);
        boundary.semantic_boundaries.push(sample_boundary());
        assert!(validate_run_report(&boundary).is_ok());

        let mut empty_id = boundary.clone();
        empty_id.semantic_boundaries[0].id.clear();
        let id_err = match validate_run_report(&empty_id) {
            Err(error) => error,
            Ok(_) => panic!("empty id must reject"),
        };
        assert!(id_err.reason.contains("empty stable id"));

        let mut empty_reason = boundary.clone();
        empty_reason.semantic_boundaries[0].reason.clear();
        let reason_err = match validate_run_report(&empty_reason) {
            Err(error) => error,
            Ok(_) => panic!("empty reason must reject"),
        };
        assert!(reason_err.reason.contains("empty reason"));

        let mut empty_kind = boundary.clone();
        empty_kind.semantic_boundaries[0].source_kind.clear();
        let kind_err = match validate_run_report(&empty_kind) {
            Err(error) => error,
            Ok(_) => panic!("empty source kind must reject"),
        };
        assert!(kind_err.reason.contains("empty source kind"));

        let mut empty_owner = boundary.clone();
        empty_owner.semantic_boundaries[0].owner_workstream.clear();
        let owner_err = match validate_run_report(&empty_owner) {
            Err(error) => error,
            Ok(_) => panic!("empty owner must reject"),
        };
        assert!(owner_err.reason.contains("no owning workstream"));

        let mut empty_support = boundary.clone();
        empty_support.semantic_boundaries[0].supporting_test.clear();
        let support_err = match validate_run_report(&empty_support) {
            Err(error) => error,
            Ok(_) => panic!("empty support must reject"),
        };
        assert!(support_err.reason.contains("no supporting test"));

        let mut reversed = boundary.clone();
        reversed.semantic_boundaries[0].source_span.start = 4;
        reversed.semantic_boundaries[0].source_span.end = 1;
        let reversed_err = match validate_run_report(&reversed) {
            Err(error) => error,
            Ok(_) => panic!("reversed span must reject"),
        };
        assert!(reversed_err.reason.contains("reversed source span"));

        let mut duplicate_boundary = boundary.clone();
        duplicate_boundary
            .semantic_boundaries
            .push(duplicate_boundary.semantic_boundaries[0].clone());
        let dup_boundary_err = match validate_run_report(&duplicate_boundary) {
            Err(error) => error,
            Ok(_) => panic!("duplicate boundary must reject"),
        };
        assert!(dup_boundary_err.reason.contains("duplicate key"));

        let mut empty_boundary_path = boundary.clone();
        empty_boundary_path.semantic_boundaries[0].path.clear();
        let boundary_path_err = match validate_run_report(&empty_boundary_path) {
            Err(error) => error,
            Ok(_) => panic!("empty boundary path must reject"),
        };
        assert!(boundary_path_err.reason.contains("empty path"));

        let accepted = sample_v2(1, 0);
        assert!(validate_compile_baseline_v2(&accepted).is_ok());
        assert!(validate_accepted_baseline(&AcceptedBaseline::V2(Box::new(accepted))).is_ok());
    }

    fn sample_failure(path: &str, bucket: &str) -> RunFailure {
        RunFailure {
            path: path.into(),
            phase: "compile".into(),
            bucket: bucket.into(),
            first_diagnostic: "sample".into(),
            workstream: "parser".into(),
            lsp_impact: vec!["diagnostics".into()],
        }
    }

    fn sample_boundary() -> ObservedSemanticBoundary {
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

    fn sample_results(total: usize, passed: usize) -> Vec<RunFileResult> {
        (0..total)
            .map(|index| {
                let status = if index < passed {
                    RunnerStatus::Pass
                } else {
                    RunnerStatus::Fail
                };
                RunFileResult {
                    path: format!("base/{index}.t"),
                    status,
                    assertions_passed: usize::from(status == RunnerStatus::Pass),
                    assertions_total: 1,
                }
            })
            .collect()
    }

    fn sample_report(total: usize, passed: usize) -> RunReport {
        let file_results = sample_results(total, passed);
        let failures = file_results
            .iter()
            .filter(|result| result.status == RunnerStatus::Fail)
            .map(|result| sample_failure(&result.path, "parse_recovery"))
            .collect();
        RunReport {
            schema_version: RUN_REPORT_SCHEMA_VERSION.into(),
            commit: "a".repeat(40),
            timestamp: "2026-08-11T00:00:00Z".into(),
            perl_ref: "perl".into(),
            prepared_tree: "<prepared>".into(),
            run_tree: "<run>".into(),
            host_perl: "perl".into(),
            runner: HarnessRunner::Test,
            mode: HarnessMode::Compile,
            profile: HarnessProfile::Base,
            harness_status: Some(0),
            summary: RunSummary {
                files_total: total,
                files_passed: passed,
                files_failed: total - passed,
                tap_assertions_total: total,
                tap_assertions_passed: passed,
            },
            buckets: BTreeMap::new(),
            file_results,
            failures,
            semantic_boundaries: Vec::new(),
        }
    }

    fn sample_v2(total: usize, passed: usize) -> CompileBaselineV2 {
        let file_results = sample_results(total, passed);
        let expected_failures = file_results
            .iter()
            .filter(|result| result.status == RunnerStatus::Fail)
            .map(|result| sample_failure(&result.path, "parse_recovery"))
            .collect();
        CompileBaselineV2 {
            schema_version: COMPILE_BASELINE_V2_SCHEMA_VERSION.into(),
            report_schema_version: RUN_REPORT_SCHEMA_VERSION.into(),
            series_id: "series".into(),
            manifest_hash: "manifest".into(),
            repository_commit: "a".repeat(40),
            perl_resolved_ref: "perl".into(),
            preparation_receipt_id: "prepare".into(),
            compiler_subject_identity: "compiler".into(),
            invocation_identity: "invocation".into(),
            capability_identity: "capability".into(),
            environment_identity: "environment".into(),
            source_report_digest: "digest".into(),
            accepted_transition_id: Some("transition".into()),
            evidence_bundle: Some("bundle".into()),
            mode: HarnessMode::Compile,
            profile: HarnessProfile::Base,
            runner: HarnessRunner::Test,
            file_membership: file_results.iter().map(|result| result.path.clone()).collect(),
            files_total: total,
            files_passed: passed,
            files_failed: total - passed,
            tap_assertions_total: total,
            tap_assertions_passed: passed,
            buckets: BTreeMap::new(),
            expected_failures,
            file_results,
            semantic_boundaries: Vec::new(),
            boundary_retirements: Vec::new(),
        }
    }
}
