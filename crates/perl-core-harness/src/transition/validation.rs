//! Canonical validation for accepted compiler ratchets, current run reports,
//! and the exact context that makes them comparable.
//!
//! Transition classification is allowed only after both evidence subjects and
//! their comparison context satisfy these structural, identity, completion,
//! membership, failure, and assertion invariants.

use super::model::AcceptedBaseline;
use perl_core_harness_types::{
    BOUNDARY_RETIREMENT_SCHEMA_VERSION, COMPILE_BASELINE_SCHEMA_VERSION,
    COMPILE_BASELINE_V2_SCHEMA_VERSION, CompileBaselineV2, HarnessMode, HarnessProfile,
    HarnessRunner, ObservedSemanticBoundary, RUN_REPORT_SCHEMA_VERSION, RunFailure,
    RunFileResult, RunReport, RunnerStatus,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Exact comparison-context schema required for definitive V2 transitions.
pub const COMPILER_COMPARISON_CONTEXT_SCHEMA_VERSION: &str =
    "perl_core_harness.comparison_context.v1";

/// Evidence side that failed canonical validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceSubject {
    /// Retained accepted baseline or ratchet.
    AcceptedBaseline,
    /// Fresh current run report.
    CurrentRun,
    /// External exact-series/invocation context joining the two subjects.
    ComparisonContext,
}

impl fmt::Display for EvidenceSubject {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::AcceptedBaseline => "accepted baseline",
            Self::CurrentRun => "current run",
            Self::ComparisonContext => "comparison context",
        })
    }
}

/// Stable class for one evidence-validation failure.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceValidationKind {
    /// The evidence schema is not supported by this classifier.
    UnsupportedSchema,
    /// A required identity is absent or malformed.
    MissingIdentity,
    /// A path is empty, absolute, escaping, or non-normalized.
    InvalidPath,
    /// A path or semantic identity appears more than once.
    DuplicateIdentity,
    /// Immutable membership and detailed results disagree.
    MembershipMismatch,
    /// Aggregate file totals disagree with detailed rows.
    FileCountMismatch,
    /// Aggregate pass/fail counts disagree with detailed statuses.
    StatusCountMismatch,
    /// A detailed row reports more passed assertions than total assertions.
    AssertionBounds,
    /// Aggregate TAP assertion counts disagree with detailed rows.
    AssertionTotalMismatch,
    /// The runner did not reach a complete successful terminal state.
    IncompleteRun,
    /// Failure or bucket evidence is internally inconsistent.
    FailureInventory,
    /// Semantic-boundary evidence is malformed or duplicated.
    SemanticBoundary,
    /// Boundary-retirement evidence is malformed or duplicated.
    BoundaryRetirement,
    /// The accepted/current/context subjects do not identify one comparable series.
    IncomparableSubject,
    /// Context does not digest or identify the exact current report.
    ContextMismatch,
}

/// Typed evidence-validation error retained before transition classification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceValidationError {
    /// Evidence side that failed.
    pub subject: EvidenceSubject,
    /// Stable failure class.
    pub kind: EvidenceValidationKind,
    /// Optional normalized path identifying the affected row.
    pub path: Option<String>,
    /// Actionable bounded explanation.
    pub message: String,
}

impl fmt::Display for EvidenceValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(path) = &self.path {
            write!(formatter, "{} invalid at {path}: {}", self.subject, self.message)
        } else {
            write!(formatter, "{} invalid: {}", self.subject, self.message)
        }
    }
}

impl std::error::Error for EvidenceValidationError {}

/// Exact external identity required to compare a current V2 observation with
/// one accepted V2 ratchet.
///
/// The context is non-authorizing by itself. Its producer must derive these
/// fields from the authoritative comparison-series, invocation, preparation,
/// capability, environment, and current-report evidence. Validation binds the
/// accepted baseline and current report to these declared subjects.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompilerComparisonContext {
    /// Context schema.
    pub schema_version: String,
    /// Accepted series identity.
    pub series_id: String,
    /// Accepted immutable manifest hash.
    pub manifest_hash: String,
    /// Accepted measurement commit named by the ratchet.
    pub accepted_repository_commit: String,
    /// Accepted compiler subject named by the ratchet.
    pub accepted_compiler_subject_identity: String,
    /// Accepted source-report digest named by the ratchet.
    pub accepted_source_report_digest: String,
    /// Exact current repository commit measured by the report.
    pub current_repository_commit: String,
    /// Exact current compiler subject identity supplied by the evidence producer.
    pub current_compiler_subject_identity: String,
    /// SHA-256 digest of canonical current `RunReport` JSON bytes.
    pub current_source_report_digest: String,
    /// Resolved upstream Perl identity.
    pub perl_resolved_ref: String,
    /// Preparation receipt identity.
    pub preparation_receipt_id: String,
    /// Invocation identity shared by the accepted and current observation.
    pub invocation_identity: String,
    /// Capability identity shared by the accepted and current observation.
    pub capability_identity: String,
    /// Environment identity shared by the accepted and current observation.
    pub environment_identity: String,
    /// Measured mode.
    pub mode: HarnessMode,
    /// Measured profile.
    pub profile: HarnessProfile,
    /// Upstream runner.
    pub runner: HarnessRunner,
    /// Exact ordered immutable denominator.
    pub file_membership: Vec<String>,
}

/// Accepted baseline proven structurally valid for transition classification.
#[derive(Debug, Clone, Copy)]
pub struct ValidatedAcceptedBaseline<'a> {
    inner: &'a AcceptedBaseline,
}

impl<'a> ValidatedAcceptedBaseline<'a> {
    /// Borrow the validated accepted baseline.
    #[must_use]
    pub const fn as_inner(self) -> &'a AcceptedBaseline {
        self.inner
    }
}

/// Current report proven complete, structurally valid, and context-bound.
#[derive(Debug, Clone, Copy)]
pub struct ValidatedRunReport<'a> {
    inner: &'a RunReport,
}

impl<'a> ValidatedRunReport<'a> {
    /// Borrow the validated current report.
    #[must_use]
    pub const fn as_inner(self) -> &'a RunReport {
        self.inner
    }
}

/// Fully validated comparable V2 evidence pair.
#[derive(Debug, Clone, Copy)]
pub struct ValidatedComparison<'a> {
    accepted: ValidatedAcceptedBaseline<'a>,
    current: ValidatedRunReport<'a>,
}

impl<'a> ValidatedComparison<'a> {
    /// Borrow the validated accepted baseline.
    #[must_use]
    pub const fn accepted(self) -> &'a AcceptedBaseline {
        self.accepted.as_inner()
    }

    /// Borrow the validated current report.
    #[must_use]
    pub const fn current(self) -> &'a RunReport {
        self.current.as_inner()
    }
}

/// Canonical SHA-256 digest for one current report.
pub fn run_report_digest(current: &RunReport) -> Result<String, EvidenceValidationError> {
    let encoded = serde_json::to_vec(current).map_err(|error| {
        validation_error(
            EvidenceSubject::CurrentRun,
            EvidenceValidationKind::ContextMismatch,
            None,
            format!("serializing current report for digest: {error}"),
        )
    })?;
    let digest = Sha256::digest(encoded);
    Ok(format!("sha256:{digest:x}"))
}

/// Validate a retained accepted baseline before any transition is classified.
pub fn validate_accepted_baseline(
    accepted: &AcceptedBaseline,
) -> Result<ValidatedAcceptedBaseline<'_>, EvidenceValidationError> {
    match accepted {
        AcceptedBaseline::V1(value) => {
            if value.schema_version != COMPILE_BASELINE_SCHEMA_VERSION
                || value.report_schema_version != RUN_REPORT_SCHEMA_VERSION
            {
                return Err(validation_error(
                    EvidenceSubject::AcceptedBaseline,
                    EvidenceValidationKind::UnsupportedSchema,
                    None,
                    "unsupported accepted V1 baseline or report schema",
                ));
            }
            validate_rows(
                EvidenceSubject::AcceptedBaseline,
                &value.file_results,
                value.files_total,
                value.files_passed,
                value.files_failed,
                value.tap_assertions_total,
                value.tap_assertions_passed,
            )?;
            validate_failure_inventory(
                EvidenceSubject::AcceptedBaseline,
                &value.file_results,
                &value.expected_failures,
                &value.buckets,
            )?;
            if let Some(boundaries) = &value.semantic_boundaries {
                validate_semantic_boundaries(
                    EvidenceSubject::AcceptedBaseline,
                    &value.file_results,
                    boundaries,
                    true,
                )?;
            }
        }
        AcceptedBaseline::V2(value) => validate_v2_baseline(value)?,
    }

    Ok(ValidatedAcceptedBaseline { inner: accepted })
}

/// Validate a current run report and bind it to exact comparison context.
pub fn validate_run_report<'a>(
    current: &'a RunReport,
    context: &CompilerComparisonContext,
) -> Result<ValidatedRunReport<'a>, EvidenceValidationError> {
    validate_context(context)?;
    validate_run_report_structure(current)?;

    let observed_paths = current
        .file_results
        .iter()
        .map(|result| result.path.as_str())
        .collect::<Vec<_>>();
    let context_paths = context
        .file_membership
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    if current.commit != context.current_repository_commit
        || current.perl_ref != context.perl_resolved_ref
        || current.mode != context.mode
        || current.profile != context.profile
        || current.runner != context.runner
        || observed_paths != context_paths
    {
        return Err(validation_error(
            EvidenceSubject::ComparisonContext,
            EvidenceValidationKind::ContextMismatch,
            None,
            "current report does not match the declared commit, Perl, mode, profile, runner, or file membership",
        ));
    }
    let actual_digest = run_report_digest(current)?;
    if actual_digest != context.current_source_report_digest {
        return Err(validation_error(
            EvidenceSubject::ComparisonContext,
            EvidenceValidationKind::ContextMismatch,
            None,
            "current report digest does not match the declared comparison context",
        ));
    }

    Ok(ValidatedRunReport { inner: current })
}

/// Validate both evidence subjects and prove they belong to one exact V2
/// comparison context.
pub fn validate_comparison<'a>(
    accepted: &'a AcceptedBaseline,
    current: &'a RunReport,
    context: &CompilerComparisonContext,
) -> Result<ValidatedComparison<'a>, EvidenceValidationError> {
    let accepted = validate_accepted_baseline(accepted)?;
    let current = validate_run_report(current, context)?;
    let AcceptedBaseline::V2(value) = accepted.as_inner() else {
        return Err(validation_error(
            EvidenceSubject::AcceptedBaseline,
            EvidenceValidationKind::IncomparableSubject,
            None,
            "V1 baselines lack immutable series, runner, Perl, invocation, capability, environment, preparation, and subject identities; definitive transitions require V2",
        ));
    };

    let stable_identity_matches = value.series_id == context.series_id
        && value.manifest_hash == context.manifest_hash
        && value.repository_commit == context.accepted_repository_commit
        && value.compiler_subject_identity == context.accepted_compiler_subject_identity
        && value.source_report_digest == context.accepted_source_report_digest
        && value.perl_resolved_ref == context.perl_resolved_ref
        && value.preparation_receipt_id == context.preparation_receipt_id
        && value.invocation_identity == context.invocation_identity
        && value.capability_identity == context.capability_identity
        && value.environment_identity == context.environment_identity
        && value.mode == context.mode
        && value.profile == context.profile
        && value.runner == context.runner
        && value.file_membership == context.file_membership;
    if !stable_identity_matches {
        return Err(validation_error(
            EvidenceSubject::ComparisonContext,
            EvidenceValidationKind::IncomparableSubject,
            None,
            "accepted V2 ratchet does not match the declared series, manifest, accepted subject, Perl, preparation, invocation, capability, environment, mode, profile, runner, or membership",
        ));
    }

    Ok(ValidatedComparison { accepted, current })
}

/// Structural current-report validation used by the context-free compatibility
/// entry point before it returns `NotProven` for missing comparison context.
pub(crate) fn validate_run_report_structure(
    current: &RunReport,
) -> Result<(), EvidenceValidationError> {
    if current.schema_version != RUN_REPORT_SCHEMA_VERSION {
        return Err(validation_error(
            EvidenceSubject::CurrentRun,
            EvidenceValidationKind::UnsupportedSchema,
            None,
            "report schema is not the supported run-report version",
        ));
    }
    if current.harness_status != Some(0) {
        return Err(validation_error(
            EvidenceSubject::CurrentRun,
            EvidenceValidationKind::IncompleteRun,
            None,
            format!(
                "current harness_status {:?} is not a complete successful run",
                current.harness_status
            ),
        ));
    }
    require_sha(EvidenceSubject::CurrentRun, "commit", &current.commit)?;
    for (field, value) in [
        ("perl_ref", current.perl_ref.as_str()),
        ("prepared_tree", current.prepared_tree.as_str()),
        ("run_tree", current.run_tree.as_str()),
        ("host_perl", current.host_perl.as_str()),
    ] {
        require_nonempty(EvidenceSubject::CurrentRun, field, value)?;
    }

    validate_rows(
        EvidenceSubject::CurrentRun,
        &current.file_results,
        current.summary.files_total,
        current.summary.files_passed,
        current.summary.files_failed,
        current.summary.tap_assertions_total,
        current.summary.tap_assertions_passed,
    )?;
    validate_failure_inventory(
        EvidenceSubject::CurrentRun,
        &current.file_results,
        &current.failures,
        &current.buckets,
    )?;
    validate_semantic_boundaries(
        EvidenceSubject::CurrentRun,
        &current.file_results,
        &current.semantic_boundaries,
        false,
    )?;
    Ok(())
}

fn validate_context(context: &CompilerComparisonContext) -> Result<(), EvidenceValidationError> {
    let subject = EvidenceSubject::ComparisonContext;
    if context.schema_version != COMPILER_COMPARISON_CONTEXT_SCHEMA_VERSION {
        return Err(validation_error(
            subject,
            EvidenceValidationKind::UnsupportedSchema,
            None,
            "unsupported compiler comparison-context schema",
        ));
    }
    require_sha(
        subject,
        "accepted_repository_commit",
        &context.accepted_repository_commit,
    )?;
    require_sha(
        subject,
        "current_repository_commit",
        &context.current_repository_commit,
    )?;
    for (field, identity) in [
        ("series_id", context.series_id.as_str()),
        ("manifest_hash", context.manifest_hash.as_str()),
        (
            "accepted_compiler_subject_identity",
            context.accepted_compiler_subject_identity.as_str(),
        ),
        (
            "accepted_source_report_digest",
            context.accepted_source_report_digest.as_str(),
        ),
        (
            "current_compiler_subject_identity",
            context.current_compiler_subject_identity.as_str(),
        ),
        (
            "current_source_report_digest",
            context.current_source_report_digest.as_str(),
        ),
        ("perl_resolved_ref", context.perl_resolved_ref.as_str()),
        (
            "preparation_receipt_id",
            context.preparation_receipt_id.as_str(),
        ),
        ("invocation_identity", context.invocation_identity.as_str()),
        ("capability_identity", context.capability_identity.as_str()),
        ("environment_identity", context.environment_identity.as_str()),
    ] {
        require_nonempty(subject, field, identity)?;
    }
    if !context.current_source_report_digest.starts_with("sha256:") {
        return Err(validation_error(
            subject,
            EvidenceValidationKind::ContextMismatch,
            None,
            "current_source_report_digest must use sha256:<hex> identity",
        ));
    }
    if context.file_membership.is_empty() {
        return Err(validation_error(
            subject,
            EvidenceValidationKind::MembershipMismatch,
            None,
            "comparison context contains no file membership",
        ));
    }
    if let Some(path) = first_duplicate_str(context.file_membership.iter().map(String::as_str)) {
        return Err(validation_error(
            subject,
            EvidenceValidationKind::DuplicateIdentity,
            Some(path.to_string()),
            "comparison context repeats a file-membership path",
        ));
    }
    for path in &context.file_membership {
        validate_path(subject, path)?;
    }
    Ok(())
}

fn validate_v2_baseline(value: &CompileBaselineV2) -> Result<(), EvidenceValidationError> {
    let subject = EvidenceSubject::AcceptedBaseline;
    if value.schema_version != COMPILE_BASELINE_V2_SCHEMA_VERSION
        || value.report_schema_version != RUN_REPORT_SCHEMA_VERSION
    {
        return Err(validation_error(
            subject,
            EvidenceValidationKind::UnsupportedSchema,
            None,
            "unsupported accepted V2 baseline or report schema",
        ));
    }

    require_sha(subject, "repository_commit", &value.repository_commit)?;
    for (field, identity) in [
        ("series_id", value.series_id.as_str()),
        ("manifest_hash", value.manifest_hash.as_str()),
        ("perl_resolved_ref", value.perl_resolved_ref.as_str()),
        (
            "preparation_receipt_id",
            value.preparation_receipt_id.as_str(),
        ),
        (
            "compiler_subject_identity",
            value.compiler_subject_identity.as_str(),
        ),
        ("invocation_identity", value.invocation_identity.as_str()),
        ("capability_identity", value.capability_identity.as_str()),
        ("environment_identity", value.environment_identity.as_str()),
        ("source_report_digest", value.source_report_digest.as_str()),
    ] {
        require_nonempty(subject, field, identity)?;
    }

    if let Some(path) = first_duplicate_str(value.file_membership.iter().map(String::as_str)) {
        return Err(validation_error(
            subject,
            EvidenceValidationKind::DuplicateIdentity,
            Some(path.to_string()),
            format!("accepted V2 file_membership repeats path {path}"),
        ));
    }
    for path in &value.file_membership {
        validate_path(subject, path)?;
    }

    validate_rows(
        subject,
        &value.file_results,
        value.files_total,
        value.files_passed,
        value.files_failed,
        value.tap_assertions_total,
        value.tap_assertions_passed,
    )?;

    if !value
        .file_membership
        .iter()
        .map(String::as_str)
        .eq(value.file_results.iter().map(|result| result.path.as_str()))
    {
        return Err(validation_error(
            subject,
            EvidenceValidationKind::MembershipMismatch,
            None,
            "accepted V2 file_results do not match immutable file_membership",
        ));
    }

    validate_failure_inventory(
        subject,
        &value.file_results,
        &value.expected_failures,
        &value.buckets,
    )?;
    validate_semantic_boundaries(
        subject,
        &value.file_results,
        &value.semantic_boundaries,
        true,
    )?;
    validate_boundary_retirements(value)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn validate_rows(
    subject: EvidenceSubject,
    results: &[RunFileResult],
    files_total: usize,
    files_passed: usize,
    files_failed: usize,
    assertions_total: usize,
    assertions_passed: usize,
) -> Result<(), EvidenceValidationError> {
    if results.is_empty() {
        return Err(validation_error(
            subject,
            EvidenceValidationKind::FileCountMismatch,
            None,
            "definitive evidence contains no file-result rows",
        ));
    }
    if let Some(path) = first_duplicate_str(results.iter().map(|result| result.path.as_str())) {
        let message = match subject {
            EvidenceSubject::AcceptedBaseline => {
                format!("accepted observation repeats file-result path {path}")
            }
            EvidenceSubject::CurrentRun => {
                format!("current observation repeats file-result path {path}")
            }
            EvidenceSubject::ComparisonContext => {
                format!("comparison context repeats file-result path {path}")
            }
        };
        return Err(validation_error(
            subject,
            EvidenceValidationKind::DuplicateIdentity,
            Some(path.to_string()),
            message,
        ));
    }

    let mut observed_passed = 0usize;
    let mut observed_failed = 0usize;
    let mut observed_assertions_total = 0usize;
    let mut observed_assertions_passed = 0usize;
    for result in results {
        validate_path(subject, &result.path)?;
        if result.assertions_passed > result.assertions_total {
            return Err(validation_error(
                subject,
                EvidenceValidationKind::AssertionBounds,
                Some(result.path.clone()),
                format!(
                    "assertions_passed {} exceeds assertions_total {}",
                    result.assertions_passed, result.assertions_total
                ),
            ));
        }
        match result.status {
            RunnerStatus::Pass => observed_passed += 1,
            RunnerStatus::Fail => observed_failed += 1,
        }
        observed_assertions_total = observed_assertions_total
            .checked_add(result.assertions_total)
            .ok_or_else(|| {
                validation_error(
                    subject,
                    EvidenceValidationKind::AssertionTotalMismatch,
                    Some(result.path.clone()),
                    "detailed assertion total overflowed usize",
                )
            })?;
        observed_assertions_passed = observed_assertions_passed
            .checked_add(result.assertions_passed)
            .ok_or_else(|| {
                validation_error(
                    subject,
                    EvidenceValidationKind::AssertionTotalMismatch,
                    Some(result.path.clone()),
                    "detailed passed-assertion total overflowed usize",
                )
            })?;
    }

    if files_total != results.len() {
        return Err(validation_error(
            subject,
            EvidenceValidationKind::FileCountMismatch,
            None,
            format!(
                "files_total {files_total} differs from {} detailed rows",
                results.len()
            ),
        ));
    }
    if files_passed != observed_passed
        || files_failed != observed_failed
        || files_passed.checked_add(files_failed) != Some(files_total)
    {
        return Err(validation_error(
            subject,
            EvidenceValidationKind::StatusCountMismatch,
            None,
            format!(
                "aggregate files passed/failed {files_passed}/{files_failed} differ from detailed statuses {observed_passed}/{observed_failed}"
            ),
        ));
    }
    if assertions_total != observed_assertions_total
        || assertions_passed != observed_assertions_passed
        || assertions_passed > assertions_total
    {
        return Err(validation_error(
            subject,
            EvidenceValidationKind::AssertionTotalMismatch,
            None,
            format!(
                "aggregate TAP assertions {assertions_passed}/{assertions_total} differ from detailed assertions {observed_assertions_passed}/{observed_assertions_total}"
            ),
        ));
    }
    Ok(())
}

fn validate_failure_inventory(
    subject: EvidenceSubject,
    results: &[RunFileResult],
    failures: &[RunFailure],
    buckets: &BTreeMap<String, usize>,
) -> Result<(), EvidenceValidationError> {
    let failed_paths = results
        .iter()
        .filter(|result| result.status == RunnerStatus::Fail)
        .map(|result| result.path.as_str())
        .collect::<BTreeSet<_>>();
    if failures.len() != failed_paths.len() {
        return Err(validation_error(
            subject,
            EvidenceValidationKind::FailureInventory,
            None,
            format!(
                "{} failed file result(s) require exactly one owned failure row each, but {} failure row(s) were supplied",
                failed_paths.len(),
                failures.len()
            ),
        ));
    }

    let mut failure_paths = BTreeSet::new();
    let mut observed_buckets = BTreeMap::<String, usize>::new();
    for failure in failures {
        validate_path(subject, &failure.path)?;
        for (field, value) in [
            ("failure.phase", failure.phase.as_str()),
            ("failure.bucket", failure.bucket.as_str()),
            ("failure.first_diagnostic", failure.first_diagnostic.as_str()),
            ("failure.workstream", failure.workstream.as_str()),
        ] {
            require_nonempty(subject, field, value)?;
        }
        if !failure_paths.insert(failure.path.as_str()) {
            return Err(validation_error(
                subject,
                EvidenceValidationKind::DuplicateIdentity,
                Some(failure.path.clone()),
                "more than one failure row identifies the same failed file",
            ));
        }
        if !failed_paths.contains(failure.path.as_str()) {
            return Err(validation_error(
                subject,
                EvidenceValidationKind::FailureInventory,
                Some(failure.path.clone()),
                "failure row does not identify a detailed failed file result",
            ));
        }
        let mut impact = BTreeSet::new();
        if failure
            .lsp_impact
            .iter()
            .any(|value| value.trim().is_empty() || !impact.insert(value.as_str()))
        {
            return Err(validation_error(
                subject,
                EvidenceValidationKind::FailureInventory,
                Some(failure.path.clone()),
                "lsp_impact entries must be nonempty and unique",
            ));
        }
        *observed_buckets.entry(failure.bucket.clone()).or_default() += 1;
    }
    if failure_paths != failed_paths {
        let missing = failed_paths
            .difference(&failure_paths)
            .copied()
            .collect::<Vec<_>>()
            .join(", ");
        return Err(validation_error(
            subject,
            EvidenceValidationKind::FailureInventory,
            None,
            format!("failed file result(s) lack owned failure rows: {missing}"),
        ));
    }
    if &observed_buckets != buckets {
        return Err(validation_error(
            subject,
            EvidenceValidationKind::FailureInventory,
            None,
            format!(
                "aggregate failure buckets {buckets:?} differ from exact failure-row cardinalities {observed_buckets:?}"
            ),
        ));
    }
    Ok(())
}

fn validate_semantic_boundaries(
    subject: EvidenceSubject,
    results: &[RunFileResult],
    boundaries: &[ObservedSemanticBoundary],
    accepted: bool,
) -> Result<(), EvidenceValidationError> {
    let result_paths = results
        .iter()
        .map(|result| result.path.as_str())
        .collect::<BTreeSet<_>>();
    let mut identities = BTreeSet::new();
    for boundary in boundaries {
        validate_path(subject, &boundary.path)?;
        if !result_paths.contains(boundary.path.as_str()) {
            return Err(validation_error(
                subject,
                EvidenceValidationKind::SemanticBoundary,
                Some(boundary.path.clone()),
                "semantic boundary does not identify a detailed file-result path",
            ));
        }
        for (field, value) in [
            ("boundary.id", boundary.id.as_str()),
            ("boundary.reason", boundary.reason.as_str()),
            ("boundary.source_kind", boundary.source_kind.as_str()),
            (
                "boundary.owner_workstream",
                boundary.owner_workstream.as_str(),
            ),
            (
                "boundary.supporting_test",
                boundary.supporting_test.as_str(),
            ),
        ] {
            require_nonempty(subject, field, value)?;
        }
        if boundary.source_span.start >= boundary.source_span.end {
            return Err(validation_error(
                subject,
                EvidenceValidationKind::SemanticBoundary,
                Some(boundary.path.clone()),
                "semantic-boundary source span is empty or reversed",
            ));
        }
        let identity = (
            boundary.path.as_str(),
            boundary.id.as_str(),
            boundary.source_span.start,
            boundary.source_span.end,
        );
        if !identities.insert(identity) {
            return Err(validation_error(
                subject,
                EvidenceValidationKind::DuplicateIdentity,
                Some(boundary.path.clone()),
                "duplicate semantic-boundary identity",
            ));
        }
        if accepted && boundary.blocks_compilation {
            return Err(validation_error(
                subject,
                EvidenceValidationKind::SemanticBoundary,
                Some(boundary.path.clone()),
                "accepted semantic boundary cannot block compilation",
            ));
        }
    }
    Ok(())
}

fn validate_boundary_retirements(
    value: &CompileBaselineV2,
) -> Result<(), EvidenceValidationError> {
    let subject = EvidenceSubject::AcceptedBaseline;
    let mut identities = BTreeSet::new();
    for retirement in &value.boundary_retirements {
        if retirement.schema_version != BOUNDARY_RETIREMENT_SCHEMA_VERSION {
            return Err(validation_error(
                subject,
                EvidenceValidationKind::BoundaryRetirement,
                Some(retirement.path.clone()),
                "unsupported boundary-retirement schema",
            ));
        }
        validate_path(subject, &retirement.path)?;
        if retirement.source_start >= retirement.source_end {
            return Err(validation_error(
                subject,
                EvidenceValidationKind::BoundaryRetirement,
                Some(retirement.path.clone()),
                "boundary-retirement source span is empty or reversed",
            ));
        }
        for (field, identity) in [
            ("retirement.id", retirement.id.as_str()),
            ("retirement.series_id", retirement.series_id.as_str()),
            ("retirement.manifest_hash", retirement.manifest_hash.as_str()),
            (
                "retirement.source_report_digest",
                retirement.source_report_digest.as_str(),
            ),
            (
                "retirement.transition_id",
                retirement.transition_id.as_str(),
            ),
            (
                "retirement.replacement_issue",
                retirement.replacement_issue.as_str(),
            ),
            (
                "retirement.evidence_bundle",
                retirement.evidence_bundle.as_str(),
            ),
        ] {
            require_nonempty(subject, field, identity)?;
        }
        require_sha(
            subject,
            "retirement.measurement_sha",
            &retirement.measurement_sha,
        )?;
        let identity = (
            retirement.path.as_str(),
            retirement.id.as_str(),
            retirement.source_start,
            retirement.source_end,
        );
        if !identities.insert(identity) {
            return Err(validation_error(
                subject,
                EvidenceValidationKind::DuplicateIdentity,
                Some(retirement.path.clone()),
                "duplicate boundary-retirement identity",
            ));
        }
    }
    Ok(())
}

fn validate_path(
    subject: EvidenceSubject,
    path: &str,
) -> Result<(), EvidenceValidationError> {
    let invalid = path.trim().is_empty()
        || path.starts_with('/')
        || path.starts_with('\\')
        || path.contains('\\')
        || path
            .split('/')
            .any(|component| component.is_empty() || component == "." || component == "..")
        || path
            .split('/')
            .next()
            .is_some_and(|component| component.ends_with(':'));
    if invalid {
        return Err(validation_error(
            subject,
            EvidenceValidationKind::InvalidPath,
            Some(path.to_string()),
            "path must be a normalized repository-relative identity",
        ));
    }
    Ok(())
}

fn require_nonempty(
    subject: EvidenceSubject,
    field: &str,
    value: &str,
) -> Result<(), EvidenceValidationError> {
    if value.trim().is_empty() {
        return Err(validation_error(
            subject,
            EvidenceValidationKind::MissingIdentity,
            None,
            format!("required identity {field} is empty"),
        ));
    }
    Ok(())
}

fn require_sha(
    subject: EvidenceSubject,
    field: &str,
    value: &str,
) -> Result<(), EvidenceValidationError> {
    if value.len() != 40 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(validation_error(
            subject,
            EvidenceValidationKind::MissingIdentity,
            None,
            format!("required identity {field} is not a 40-hex commit SHA"),
        ));
    }
    Ok(())
}

fn first_duplicate_str<'a>(values: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    let mut seen = BTreeSet::new();
    values.into_iter().find(|value| !seen.insert(*value))
}

fn validation_error(
    subject: EvidenceSubject,
    kind: EvidenceValidationKind,
    path: Option<String>,
    message: impl Into<String>,
) -> EvidenceValidationError {
    EvidenceValidationError {
        subject,
        kind,
        path,
        message: message.into(),
    }
}
