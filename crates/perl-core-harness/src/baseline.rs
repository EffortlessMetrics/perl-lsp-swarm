//! Checked-in compile-baseline authority for the upstream Perl core harness.
//!
//! A baseline is the accepted ratchet a fresh run report is compared against:
//! the legacy schema for the historical `--accept`/check flow, and the
//! comparison-series-bound v2 schema that also carries semantic-boundary
//! inventory, boundary retirements, and measured-subject identity. Both
//! digest the run report and compare it field-by-field so acceptance and
//! regression detection stay independent of report-envelope shape.

use crate::normalization::hex_lower;
use crate::series::{read_series_manifest, validate_series_manifest};
use crate::transition;
use crate::{
    default_run_report_path, normalize_test_path, project_root, read_run_report,
    reject_inadmissible_report_mechanisms,
};
use color_eyre::eyre::{Context, Result, bail};
use perl_core_harness_types::{
    BOUNDARY_RETIREMENT_SCHEMA_VERSION, BaselineComparison, BaselineViolation,
    BaselineViolationKind, BoundaryRetirement, COMPILE_BASELINE_SCHEMA_VERSION,
    COMPILE_BASELINE_V2_SCHEMA_VERSION, CompileBaseline, CompileBaselineV2, HarnessMode,
    HarnessProfile, HarnessRunner, ObservedSemanticBoundary, RUN_REPORT_SCHEMA_VERSION, RunFailure,
    RunFileResult, RunReport, RunSummary, RunnerStatus, SemanticBoundaryConfidence,
    SemanticBoundaryDisposition, SemanticBoundaryLockScope, SeriesManifest,
    validate_file_result_mechanisms,
};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

/// Configuration for `perl-core-harness baseline`.
#[derive(Debug, Clone)]
pub struct BaselineConfig {
    pub mode: HarnessMode,
    pub profile: HarnessProfile,
    pub report: Option<PathBuf>,
    pub baseline: Option<PathBuf>,
    pub accept: bool,
    pub series: Option<PathBuf>,
    pub previous_baseline: Option<PathBuf>,
    pub boundary_retirements: Option<PathBuf>,
    pub compiler_subject_identity: Option<String>,
    pub invocation_identity: Option<String>,
    pub capability_identity: Option<String>,
    pub environment_identity: Option<String>,
    pub accepted_transition_id: Option<String>,
    pub evidence_bundle: Option<String>,
}

/// Check or update a checked-in Perl core harness baseline.
pub fn baseline(config: BaselineConfig) -> Result<()> {
    let report_path = config
        .report
        .clone()
        .unwrap_or_else(|| default_run_report_path(config.mode, config.profile));
    let baseline_path = config
        .baseline
        .clone()
        .unwrap_or_else(|| default_baseline_path(config.mode, config.profile));
    let report = read_run_report(&report_path)?;
    // Terminal admission precedes any count semantics (#6884): only a clean
    // exit or a recognized runner/mode completion state proves process
    // completion, however green the file and assertion counts look. The
    // execute-mode recognition keeps the #3451 selected-base receipt flow
    // working instead of permanently misclassifying it as instrument failure.
    let terminal = transition::TerminalProcessOutcome::from_harness_status(
        report.harness_status,
        report.runner,
        report.mode,
    );
    if !terminal.is_scoreable() {
        bail!(
            "perl-core-harness baseline refuses {} with runner terminal status {:?}: process completion is not proven ({})",
            report_path.display(),
            report.harness_status,
            terminal.not_proven_reason()
        );
    }
    reject_v2_options_without_series(&config)?;

    if let Some(series_path) = config.series.as_ref() {
        let series = read_series_manifest(series_path)?;
        validate_series_manifest(&series)?;
        if config.accept {
            let previous = if let Some(path) = config.previous_baseline.as_ref() {
                Some(read_compile_baseline_v2(path)?)
            } else if baseline_path.is_file() {
                Some(read_compile_baseline_v2(&baseline_path)?)
            } else {
                None
            };
            let retirements = config
                .boundary_retirements
                .as_ref()
                .map(|path| read_boundary_retirements(path))
                .transpose()?
                .unwrap_or_default();
            let accepted = baseline_v2_from_report(
                &report,
                &series,
                &config,
                previous.as_ref(),
                &retirements,
            )?;
            write_compile_baseline_v2(&baseline_path, &accepted)?;
            tracing::info!(
                "perl-core-harness: accepted {} {} v2 baseline",
                accepted.mode,
                accepted.profile
            );
            tracing::info!("wrote {}", baseline_path.display());
            return Ok(());
        }

        let baseline = read_compile_baseline_v2(&baseline_path)?;
        let identities = required_v2_identities(&config)?;
        validate_v2_identities_against_series(&identities, &series)?;
        let retirements = config
            .boundary_retirements
            .as_ref()
            .map(|path| read_boundary_retirements(path))
            .transpose()?
            .unwrap_or_default();
        let comparison = compare_baseline_v2_with_identities(
            &baseline,
            &report,
            &series,
            Some(&identities),
            config.accepted_transition_id.as_deref(),
            &retirements,
        );
        if !comparison.is_clean() {
            bail_baseline_comparison(&comparison)?;
        }
        tracing::info!(
            "perl-core-harness: v2 baseline check passed for {} {}",
            report.mode,
            report.profile
        );
        return Ok(());
    }

    if config.accept {
        let baseline = baseline_from_report(&report)?;
        write_compile_baseline(&baseline_path, &baseline)?;
        tracing::info!(
            "perl-core-harness: accepted {} {} baseline",
            baseline.mode,
            baseline.profile
        );
        tracing::info!("wrote {}", baseline_path.display());
        return Ok(());
    }

    let baseline = read_compile_baseline(&baseline_path)?;
    let comparison = compare_baseline(&baseline, &report);
    if !comparison.is_clean() {
        let details = comparison
            .violations
            .iter()
            .map(|violation| {
                let path = violation.path.as_deref().unwrap_or("-");
                format!("{:?} {path}: {}", violation.kind, violation.message)
            })
            .collect::<Vec<_>>()
            .join("\n");
        bail!(
            "perl-core-harness baseline check failed with {} violation(s):\n{}",
            comparison.violations.len(),
            details
        );
    }

    tracing::info!(
        "perl-core-harness: baseline check passed for {} {}",
        report.mode,
        report.profile
    );
    Ok(())
}

pub(crate) fn default_baseline_path(mode: HarnessMode, profile: HarnessProfile) -> PathBuf {
    let root = project_root().unwrap_or_else(|_| PathBuf::from("."));
    root.join(".ci").join("perl-core-harness").join(format!("{profile}-{mode}-baseline.json"))
}

/// Refuse a checked-in baseline whose per-file execution-mechanism claims are
/// not admissible for its mode.
fn reject_inadmissible_baseline_mechanisms(
    mode: HarnessMode,
    file_results: &[RunFileResult],
) -> Result<()> {
    validate_file_result_mechanisms(mode, file_results)
        .map_err(|violation| color_eyre::eyre::eyre!("{violation}"))
}

pub(crate) fn read_compile_baseline(path: &Path) -> Result<CompileBaseline> {
    let raw =
        fs::read_to_string(path).with_context(|| format!("reading baseline {}", path.display()))?;
    let baseline: CompileBaseline = serde_json::from_str(&raw)
        .with_context(|| format!("decoding baseline {}", path.display()))?;
    reject_inadmissible_baseline_mechanisms(baseline.mode, &baseline.file_results)
        .with_context(|| format!("baseline {}", path.display()))?;
    Ok(baseline)
}

pub(crate) fn write_compile_baseline(path: &Path, baseline: &CompileBaseline) -> Result<()> {
    if let Some(parent) = path.parent() {
        let context = format!("creating baseline directory {}", parent.display());
        fs::create_dir_all(parent).context(context)?;
    }
    let json = serde_json::to_string_pretty(baseline).context("serializing compile baseline")?;
    fs::write(path, format!("{json}\n"))
        .with_context(|| format!("writing baseline {}", path.display()))
}

pub(crate) fn baseline_from_report(report: &RunReport) -> Result<CompileBaseline> {
    // Acceptance copies file results verbatim into the durable artifact, so an
    // inadmissible claim must be refused here and not only at report decode.
    reject_inadmissible_report_mechanisms(report)?;
    let mut baseline = CompileBaseline {
        schema_version: COMPILE_BASELINE_SCHEMA_VERSION.to_string(),
        report_schema_version: report.schema_version.clone(),
        mode: report.mode,
        profile: report.profile,
        files_total: report.summary.files_total,
        files_passed: report.summary.files_passed,
        files_failed: report.summary.files_failed,
        tap_assertions_total: report.summary.tap_assertions_total,
        tap_assertions_passed: report.summary.tap_assertions_passed,
        buckets: report.buckets.clone(),
        expected_failures: report.failures.clone(),
        file_results: report.file_results.clone(),
        semantic_boundaries: Some(report.semantic_boundaries.clone()),
    };
    sort_baseline(&mut baseline);
    let mut validation = validate_report_bucket_shape(report);
    validation.extend(validate_semantic_boundary_shape(report));
    if !validation.is_empty() {
        let details = validation
            .iter()
            .map(|violation| {
                let path = violation.path.as_deref().unwrap_or("-");
                format!("{:?} {path}: {}", violation.kind, violation.message)
            })
            .collect::<Vec<_>>()
            .join("\n");
        bail!("cannot accept baseline with invalid receipt shape:\n{details}");
    }
    Ok(baseline)
}

pub(crate) fn baseline_v2_from_report(
    report: &RunReport,
    series: &SeriesManifest,
    config: &BaselineConfig,
    previous: Option<&CompileBaselineV2>,
    retirements: &[BoundaryRetirement],
) -> Result<CompileBaselineV2> {
    reject_inadmissible_report_mechanisms(report)?;
    let identities = required_v2_identities(config)?;
    validate_report_against_series(report, series, config.mode)?;
    validate_v2_identities_against_series(&identities, series)?;
    ensure_valid_report_shape(report)?;
    let accepted_boundary_violations =
        validate_accepted_semantic_boundary_inventory(&report.semantic_boundaries);
    if !accepted_boundary_violations.is_empty() {
        bail_baseline_comparison(&BaselineComparison { violations: accepted_boundary_violations })?;
    }
    let file_membership =
        report.file_results.iter().map(|result| result.path.clone()).collect::<BTreeSet<_>>();
    let expected_membership = series.normalized_manifest.iter().cloned().collect::<BTreeSet<_>>();
    if file_membership != expected_membership {
        bail!(
            "report file membership does not exactly match comparison series {}",
            series.series_id
        );
    }
    if let Some(previous) = previous {
        if previous.series_id != series.series_id || previous.manifest_hash != series.manifest_hash
        {
            bail!("previous baseline does not belong to the current comparison series");
        }
        let transition_id = config.accepted_transition_id.as_deref().ok_or_else(|| {
            color_eyre::eyre::eyre!(
                "boundary or baseline transitions require --accepted-transition-id"
            )
        })?;
        let transition_violations = compare_boundary_transition(
            previous,
            &report.semantic_boundaries,
            retirements,
            transition_id,
            series,
            report,
        );
        if !transition_violations.is_empty() {
            bail_baseline_comparison(&BaselineComparison { violations: transition_violations })?;
        }
    } else if !retirements.is_empty() {
        bail!("boundary retirements require a previous v2 baseline");
    }

    let mut file_results = report.file_results.clone();
    let mut expected_failures = report.failures.clone();
    let mut semantic_boundaries = report.semantic_boundaries.clone();
    file_results.sort_by(|left, right| left.path.cmp(&right.path));
    expected_failures.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.bucket.cmp(&right.bucket))
            .then_with(|| left.phase.cmp(&right.phase))
    });
    semantic_boundaries.sort_by_key(semantic_boundary_key);

    Ok(CompileBaselineV2 {
        schema_version: COMPILE_BASELINE_V2_SCHEMA_VERSION.to_string(),
        report_schema_version: report.schema_version.clone(),
        series_id: series.series_id.clone(),
        manifest_hash: series.manifest_hash.clone(),
        repository_commit: series.repository_commit.clone(),
        perl_resolved_ref: series.perl_resolved_ref.clone(),
        preparation_receipt_id: series.preparation_receipt_id.clone(),
        compiler_subject_identity: identities.compiler_subject_identity,
        invocation_identity: identities.invocation_identity,
        capability_identity: identities.capability_identity,
        environment_identity: identities.environment_identity,
        source_report_digest: report_digest(report)?,
        accepted_transition_id: config.accepted_transition_id.clone(),
        evidence_bundle: config.evidence_bundle.clone(),
        mode: report.mode,
        profile: report.profile,
        runner: report.runner,
        files_total: file_results.len(),
        file_membership: file_results.iter().map(|result| result.path.clone()).collect(),
        files_passed: report.summary.files_passed,
        files_failed: report.summary.files_failed,
        tap_assertions_total: report.summary.tap_assertions_total,
        tap_assertions_passed: report.summary.tap_assertions_passed,
        buckets: report.buckets.clone(),
        expected_failures,
        file_results,
        semantic_boundaries,
        boundary_retirements: retirements.to_vec(),
    })
}

pub(crate) struct V2Identities {
    pub(crate) compiler_subject_identity: String,
    pub(crate) invocation_identity: String,
    pub(crate) capability_identity: String,
    pub(crate) environment_identity: String,
}

fn required_v2_identities(config: &BaselineConfig) -> Result<V2Identities> {
    Ok(V2Identities {
        compiler_subject_identity: required_identity(
            config.compiler_subject_identity.as_deref(),
            "compiler subject",
        )?,
        invocation_identity: required_identity(
            config.invocation_identity.as_deref(),
            "invocation",
        )?,
        capability_identity: required_identity(
            config.capability_identity.as_deref(),
            "capability",
        )?,
        environment_identity: required_identity(
            config.environment_identity.as_deref(),
            "environment",
        )?,
    })
}

fn required_identity(value: Option<&str>, name: &str) -> Result<String> {
    let value = value.filter(|value| !value.trim().is_empty()).ok_or_else(|| {
        color_eyre::eyre::eyre!("baseline v2 requires a non-empty {name} identity")
    })?;
    Ok(value.to_string())
}

fn reject_v2_options_without_series(config: &BaselineConfig) -> Result<()> {
    let has_v2_option = config.previous_baseline.is_some()
        || config.boundary_retirements.is_some()
        || config.compiler_subject_identity.is_some()
        || config.invocation_identity.is_some()
        || config.capability_identity.is_some()
        || config.environment_identity.is_some()
        || config.accepted_transition_id.is_some()
        || config.evidence_bundle.is_some();
    if config.series.is_none() && has_v2_option {
        bail!("baseline v2 options require a comparison-series manifest");
    }
    if config.accepted_transition_id.as_deref().is_some_and(|id| id.trim().is_empty()) {
        bail!("baseline v2 transition identity must not be empty");
    }
    if config.boundary_retirements.is_some() && config.accepted_transition_id.is_none() {
        bail!("boundary retirement receipts require --accepted-transition-id");
    }
    Ok(())
}

fn validate_v2_identities_against_series(
    identities: &V2Identities,
    series: &SeriesManifest,
) -> Result<()> {
    if identities.compiler_subject_identity != series.compiler_subject_identity
        || identities.invocation_identity != series.invocation_identity
        || identities.capability_identity != series.capability_identity
        || identities.environment_identity != series.environment_identity
    {
        bail!("baseline v2 identity inputs do not match the comparison series");
    }
    Ok(())
}

pub(crate) fn ensure_valid_report_shape(report: &RunReport) -> Result<()> {
    let mut validation = validate_report_bucket_shape(report);
    validation.extend(validate_semantic_boundary_shape(report));
    if !validation.is_empty() {
        let details = validation
            .iter()
            .map(|violation| {
                let path = violation.path.as_deref().unwrap_or("-");
                format!("{:?} {path}: {}", violation.kind, violation.message)
            })
            .collect::<Vec<_>>()
            .join("\n");
        bail!("cannot accept baseline with invalid receipt shape:\n{details}");
    }
    validate_result_summary_shape(
        report.summary.files_total,
        report.summary.files_passed,
        report.summary.files_failed,
        report.summary.tap_assertions_total,
        report.summary.tap_assertions_passed,
        &report.file_results,
        "run report",
    )
}

pub(crate) fn validate_result_summary_shape(
    files_total: usize,
    files_passed: usize,
    files_failed: usize,
    tap_assertions_total: usize,
    tap_assertions_passed: usize,
    file_results: &[RunFileResult],
    subject: &str,
) -> Result<()> {
    if files_passed + files_failed != files_total {
        bail!("{subject} file counts do not add up to files_total");
    }
    if tap_assertions_passed > tap_assertions_total {
        bail!("{subject} passed assertions exceed tap_assertions_total");
    }
    if file_results.len() != files_total {
        bail!("{subject} file_results length does not match files_total");
    }
    let mut paths = BTreeSet::new();
    let mut passed_files = 0;
    let mut failed_files = 0;
    let mut assertions_total = 0;
    let mut assertions_passed = 0;
    for result in file_results {
        let Some(path) = normalize_test_path(&result.path) else {
            bail!("{subject} contains an invalid test path");
        };
        if !paths.insert(path) {
            bail!("{subject} contains duplicate file results");
        }
        match result.status {
            RunnerStatus::Pass => passed_files += 1,
            RunnerStatus::Fail => failed_files += 1,
        }
        if result.assertions_passed > result.assertions_total {
            bail!("{subject} has a file with passed assertions exceeding its total");
        }
        assertions_total += result.assertions_total;
        assertions_passed += result.assertions_passed;
    }
    if passed_files != files_passed || failed_files != files_failed {
        bail!("{subject} file statuses do not match its summary counts");
    }
    if assertions_total != tap_assertions_total || assertions_passed != tap_assertions_passed {
        bail!("{subject} file assertions do not match its summary counts");
    }
    Ok(())
}

pub(crate) fn validate_report_against_series(
    report: &RunReport,
    series: &SeriesManifest,
    mode: HarnessMode,
) -> Result<()> {
    if report.commit != series.repository_commit {
        bail!("measured report commit does not match comparison series");
    }
    if report.perl_ref != series.perl_resolved_ref {
        bail!("measured report Perl ref does not match comparison series");
    }
    if report.runner != series.runner {
        bail!("measured report runner does not match comparison series");
    }
    if report.profile != series.profile {
        bail!("measured report profile does not match comparison series");
    }
    if report.mode != mode {
        bail!("measured report mode does not match requested baseline mode");
    }
    if report.schema_version != RUN_REPORT_SCHEMA_VERSION {
        bail!("measured report schema is not the supported run-report schema");
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn compare_baseline_v2(
    baseline: &CompileBaselineV2,
    report: &RunReport,
    series: &SeriesManifest,
) -> BaselineComparison {
    compare_baseline_v2_with_identities(baseline, report, series, None, None, &[])
}

pub(crate) fn compare_baseline_v2_with_identities(
    baseline: &CompileBaselineV2,
    report: &RunReport,
    series: &SeriesManifest,
    identities: Option<&V2Identities>,
    transition_id: Option<&str>,
    retirements: &[BoundaryRetirement],
) -> BaselineComparison {
    let mut violations = Vec::new();
    violations.extend(validate_persisted_boundary_retirements(baseline, Some(series)));
    violations.extend(validate_accepted_semantic_boundary_inventory(&baseline.semantic_boundaries));
    if baseline.schema_version != COMPILE_BASELINE_V2_SCHEMA_VERSION {
        violations.push(violation(
            BaselineViolationKind::SchemaMismatch,
            None,
            format!(
                "baseline schema {} does not match {}",
                baseline.schema_version, COMPILE_BASELINE_V2_SCHEMA_VERSION
            ),
        ));
    }
    if baseline.series_id != series.series_id || baseline.manifest_hash != series.manifest_hash {
        violations.push(violation(
            BaselineViolationKind::SeriesMismatch,
            None,
            "baseline does not reference the supplied comparison series",
        ));
    }
    if baseline.repository_commit != series.repository_commit
        || baseline.perl_resolved_ref != series.perl_resolved_ref
        || baseline.preparation_receipt_id != series.preparation_receipt_id
    {
        violations.push(violation(
            BaselineViolationKind::PreparationIdentityMismatch,
            None,
            "baseline preparation or source identity differs from the comparison series",
        ));
    }
    if baseline.report_schema_version != report.schema_version
        || baseline.mode != report.mode
        || baseline.profile != report.profile
        || baseline.runner != report.runner
        || baseline.repository_commit != report.commit
        || baseline.perl_resolved_ref != report.perl_ref
    {
        violations.push(violation(
            BaselineViolationKind::MeasuredSubjectMismatch,
            None,
            "current report is not the measured subject declared by the v2 baseline",
        ));
    }
    if report_digest(report).map(|digest| digest != baseline.source_report_digest).unwrap_or(true) {
        violations.push(violation(
            BaselineViolationKind::MeasuredSubjectMismatch,
            None,
            "current report digest differs from the v2 baseline subject",
        ));
    }
    if let Some(identities) = identities
        && (baseline.compiler_subject_identity != identities.compiler_subject_identity
            || baseline.invocation_identity != identities.invocation_identity
            || baseline.capability_identity != identities.capability_identity
            || baseline.environment_identity != identities.environment_identity)
    {
        violations.push(violation(
            BaselineViolationKind::MeasuredSubjectMismatch,
            None,
            "current identity inputs differ from the v2 baseline measured subject",
        ));
    }
    let current_membership =
        report.file_results.iter().map(|result| result.path.as_str()).collect::<BTreeSet<_>>();
    let series_membership =
        series.normalized_manifest.iter().map(String::as_str).collect::<BTreeSet<_>>();
    for path in series_membership.difference(&current_membership) {
        violations.push(violation(
            BaselineViolationKind::MissingExpectedFile,
            Some((*path).to_string()),
            "comparison series file is missing from the current report",
        ));
    }
    for path in current_membership.difference(&series_membership) {
        violations.push(violation(
            BaselineViolationKind::UnexpectedFile,
            Some((*path).to_string()),
            "current report contains a file outside the immutable comparison series",
        ));
    }
    if baseline.file_membership != series.normalized_manifest
        || baseline.files_total != series.normalized_manifest.len()
    {
        violations.push(violation(
            BaselineViolationKind::ManifestMismatch,
            None,
            "baseline file membership does not equal the comparison-series manifest",
        ));
    }
    let legacy = CompileBaseline {
        schema_version: COMPILE_BASELINE_SCHEMA_VERSION.to_string(),
        report_schema_version: baseline.report_schema_version.clone(),
        mode: baseline.mode,
        profile: baseline.profile,
        files_total: baseline.files_total,
        files_passed: baseline.files_passed,
        files_failed: baseline.files_failed,
        tap_assertions_total: baseline.tap_assertions_total,
        tap_assertions_passed: baseline.tap_assertions_passed,
        buckets: baseline.buckets.clone(),
        expected_failures: baseline.expected_failures.clone(),
        file_results: baseline.file_results.clone(),
        semantic_boundaries: Some(baseline.semantic_boundaries.clone()),
    };
    violations.extend(
        compare_baseline(&legacy, report)
            .violations
            .into_iter()
            .filter(|violation| violation.kind != BaselineViolationKind::SemanticBoundary),
    );
    violations.extend(compare_boundary_transition(
        baseline,
        &report.semantic_boundaries,
        retirements,
        transition_id.unwrap_or(""),
        series,
        report,
    ));
    BaselineComparison { violations }
}

pub(crate) fn compare_boundary_transition(
    previous: &CompileBaselineV2,
    current: &[ObservedSemanticBoundary],
    retirements: &[BoundaryRetirement],
    transition_id: &str,
    series: &SeriesManifest,
    report: &RunReport,
) -> Vec<BaselineViolation> {
    let mut sorted_current = current.to_vec();
    sorted_current.sort_by_key(semantic_boundary_key);
    let previous_by_key = previous
        .semantic_boundaries
        .iter()
        .map(|boundary| (semantic_boundary_key(boundary), boundary))
        .collect::<BTreeMap<_, _>>();
    let current_by_key = sorted_current
        .iter()
        .map(|boundary| (semantic_boundary_key(boundary), boundary))
        .collect::<BTreeMap<_, _>>();
    let retirement_keys = retirements
        .iter()
        .map(|retirement| SemanticBoundaryKey {
            path: retirement.path.clone(),
            id: retirement.id.clone(),
            source_start: retirement.source_start,
            source_end: retirement.source_end,
        })
        .collect::<BTreeSet<_>>();
    let current_report_digest = report_digest(report);
    let mut violations = Vec::new();
    for key in previous_by_key.keys() {
        if !current_by_key.contains_key(key) && !retirement_keys.contains(key) {
            violations.push(violation(
                BaselineViolationKind::BoundaryRemovedWithoutRetirement,
                Some(key.path.clone()),
                "accepted semantic boundary disappeared without a retirement receipt",
            ));
        }
    }
    if previous.semantic_boundaries != sorted_current && transition_id.trim().is_empty() {
        violations.push(violation(
            BaselineViolationKind::SemanticBoundary,
            None,
            "semantic-boundary changes require a reviewed transition identity",
        ));
    }
    for retirement in retirements {
        let retirement_key = SemanticBoundaryKey {
            path: retirement.path.clone(),
            id: retirement.id.clone(),
            source_start: retirement.source_start,
            source_end: retirement.source_end,
        };
        let retirement_digest_matches = current_report_digest
            .as_ref()
            .is_ok_and(|digest| digest == &retirement.source_report_digest);
        if retirement.schema_version != BOUNDARY_RETIREMENT_SCHEMA_VERSION
            || retirement.transition_id != transition_id
            || retirement.replacement_issue.trim().is_empty()
            || retirement.evidence_bundle.trim().is_empty()
            || retirement.series_id != series.series_id
            || retirement.manifest_hash != series.manifest_hash
            || retirement.measurement_sha != report.commit
            || !retirement_digest_matches
        {
            let message = if current_report_digest.is_err() {
                "cannot validate retirement: failed to compute current report digest"
            } else {
                "boundary retirement receipt is incomplete, stale, or uses the wrong measured subject"
            };
            violations.push(violation(
                BaselineViolationKind::BoundaryRetirementReceiptMismatch,
                Some(retirement.path.clone()),
                message,
            ));
        }
        if !previous_by_key.contains_key(&retirement_key) {
            violations.push(violation(
                BaselineViolationKind::BoundaryRetirementReferencesUnknownBoundary,
                Some(retirement.path.clone()),
                "retirement receipt references a boundary absent from the previous baseline",
            ));
        }
        if current_by_key.contains_key(&retirement_key) {
            violations.push(violation(
                BaselineViolationKind::BoundaryRetirementReceiptMismatch,
                Some(retirement.path.clone()),
                "retirement receipt references a boundary still present in the current report",
            ));
        }
    }
    violations
}

fn validate_persisted_boundary_retirements(
    baseline: &CompileBaselineV2,
    series: Option<&SeriesManifest>,
) -> Vec<BaselineViolation> {
    let mut violations = Vec::new();
    for retirement in &baseline.boundary_retirements {
        let transition_matches = baseline
            .accepted_transition_id
            .as_deref()
            .is_some_and(|transition_id| transition_id == retirement.transition_id);
        let series_matches = series.is_none_or(|series| {
            retirement.series_id == series.series_id
                && retirement.manifest_hash == series.manifest_hash
        });
        if retirement.schema_version != BOUNDARY_RETIREMENT_SCHEMA_VERSION
            || retirement.path.trim().is_empty()
            || retirement.id.trim().is_empty()
            || retirement.source_start >= retirement.source_end
            || retirement.series_id != baseline.series_id
            || retirement.manifest_hash != baseline.manifest_hash
            || retirement.measurement_sha != baseline.repository_commit
            || retirement.source_report_digest != baseline.source_report_digest
            || !transition_matches
            || retirement.replacement_issue.trim().is_empty()
            || retirement.evidence_bundle.trim().is_empty()
            || !series_matches
        {
            violations.push(violation(
                BaselineViolationKind::BoundaryRetirementReceiptMismatch,
                Some(retirement.path.clone()),
                "persisted boundary retirement does not match the baseline and comparison-series identity",
            ));
        }
    }
    if !baseline.boundary_retirements.is_empty() && baseline.accepted_transition_id.is_none() {
        violations.push(violation(
            BaselineViolationKind::BoundaryRetirementReceiptMismatch,
            None,
            "persisted boundary retirements require the baseline accepted transition identity",
        ));
    }
    violations
}

#[derive(serde::Serialize)]
struct StableRunReportDigest<'a> {
    schema_version: &'a str,
    commit: &'a str,
    perl_ref: &'a str,
    runner: HarnessRunner,
    mode: HarnessMode,
    profile: HarnessProfile,
    harness_status: Option<i32>,
    summary: &'a RunSummary,
    buckets: &'a BTreeMap<String, usize>,
    file_results: &'a [RunFileResult],
    failures: &'a [RunFailure],
    semantic_boundaries: &'a [ObservedSemanticBoundary],
}

pub(crate) fn report_digest(report: &RunReport) -> Result<String> {
    let mut stable = report.clone();
    stable.file_results.sort_by(|left, right| left.path.cmp(&right.path));
    stable.failures.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.bucket.cmp(&right.bucket))
            .then_with(|| left.phase.cmp(&right.phase))
    });
    stable.semantic_boundaries.sort_by_key(semantic_boundary_key);
    // Temporary paths, host Perl, and wall-clock timestamps are deliberately absent.
    // Keep this field-by-field representation explicit so receipt identity does not
    // depend on the full RunReport envelope or its disposable execution metadata.
    let digest_input = StableRunReportDigest {
        schema_version: &stable.schema_version,
        commit: &stable.commit,
        perl_ref: &stable.perl_ref,
        runner: stable.runner,
        mode: stable.mode,
        profile: stable.profile,
        harness_status: stable.harness_status,
        summary: &stable.summary,
        buckets: &stable.buckets,
        file_results: &stable.file_results,
        failures: &stable.failures,
        semantic_boundaries: &stable.semantic_boundaries,
    };
    let bytes = serde_json::to_vec(&digest_input)
        .context("serializing stable field-by-field report digest")?;
    Ok(hex_lower(&Sha256::digest(bytes)))
}

pub(crate) fn read_compile_baseline_v2(path: &Path) -> Result<CompileBaselineV2> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("reading v2 baseline {}", path.display()))?;
    let value: serde_json::Value = serde_json::from_str(&raw)
        .with_context(|| format!("decoding baseline envelope {}", path.display()))?;
    parse_compile_baseline_v2(value, &path.display().to_string())
}

pub(crate) fn parse_compile_baseline_v2(
    value: serde_json::Value,
    label: &str,
) -> Result<CompileBaselineV2> {
    let schema =
        value.get("schema_version").and_then(serde_json::Value::as_str).unwrap_or("missing");
    if schema == COMPILE_BASELINE_SCHEMA_VERSION {
        bail!("historical compile baseline v1 is readable but non-authoritative; migrate it to v2");
    }
    if schema != COMPILE_BASELINE_V2_SCHEMA_VERSION {
        bail!("unsupported compile baseline schema: {schema}");
    }
    if !value.as_object().is_some_and(|object| object.contains_key("semantic_boundaries")) {
        bail!(
            "{:?}: migrated v2 baseline must declare semantic_boundaries, including an empty list",
            BaselineViolationKind::MissingBoundaryInventory
        );
    }
    let baseline: CompileBaselineV2 =
        serde_json::from_value(value).with_context(|| format!("decoding v2 baseline {label}"))?;
    reject_inadmissible_baseline_mechanisms(baseline.mode, &baseline.file_results)
        .with_context(|| format!("v2 baseline {label}"))?;
    let mut violations = validate_persisted_boundary_retirements(&baseline, None);
    violations.extend(validate_accepted_semantic_boundary_inventory(&baseline.semantic_boundaries));
    if !violations.is_empty() {
        bail_baseline_comparison(&BaselineComparison { violations })?;
    }
    Ok(baseline)
}

pub(crate) fn write_compile_baseline_v2(path: &Path, baseline: &CompileBaselineV2) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating baseline directory {}", parent.display()))?;
    }
    let json = serde_json::to_string_pretty(baseline).context("serializing compile baseline v2")?;
    fs::write(path, format!("{json}\n"))
        .with_context(|| format!("writing v2 baseline {}", path.display()))
}

fn read_boundary_retirements(path: &Path) -> Result<Vec<BoundaryRetirement>> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("reading boundary retirements {}", path.display()))?;
    serde_json::from_str(&raw)
        .with_context(|| format!("decoding boundary retirements {}", path.display()))
}

fn bail_baseline_comparison(comparison: &BaselineComparison) -> Result<()> {
    let details = comparison
        .violations
        .iter()
        .map(|violation| {
            let path = violation.path.as_deref().unwrap_or("-");
            format!("{:?} {path}: {}", violation.kind, violation.message)
        })
        .collect::<Vec<_>>()
        .join("\n");
    bail!(
        "perl-core-harness baseline transition/check failed with {} violation(s):\n{}",
        comparison.violations.len(),
        details
    )
}

fn sort_baseline(baseline: &mut CompileBaseline) {
    baseline.file_results.sort_by(|left, right| left.path.cmp(&right.path));
    baseline.expected_failures.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.bucket.cmp(&right.bucket))
            .then_with(|| left.phase.cmp(&right.phase))
    });
    if let Some(boundaries) = &mut baseline.semantic_boundaries {
        boundaries.sort_by_key(semantic_boundary_key);
    }
}

pub(crate) fn compare_baseline(
    baseline: &CompileBaseline,
    report: &RunReport,
) -> BaselineComparison {
    let mut violations = Vec::new();

    if baseline.schema_version != COMPILE_BASELINE_SCHEMA_VERSION {
        violations.push(violation(
            BaselineViolationKind::SchemaMismatch,
            None,
            format!(
                "baseline schema {} does not match {}",
                baseline.schema_version, COMPILE_BASELINE_SCHEMA_VERSION
            ),
        ));
    }
    if baseline.report_schema_version != RUN_REPORT_SCHEMA_VERSION {
        violations.push(violation(
            BaselineViolationKind::SchemaMismatch,
            None,
            format!(
                "baseline report schema {} does not match {}",
                baseline.report_schema_version, RUN_REPORT_SCHEMA_VERSION
            ),
        ));
    }
    if report.schema_version != baseline.report_schema_version {
        violations.push(violation(
            BaselineViolationKind::SchemaMismatch,
            None,
            format!(
                "report schema {} does not match baseline report schema {}",
                report.schema_version, baseline.report_schema_version
            ),
        ));
    }
    if baseline.mode != report.mode {
        violations.push(violation(
            BaselineViolationKind::ModeMismatch,
            None,
            format!("baseline mode {} does not match report mode {}", baseline.mode, report.mode),
        ));
    }
    if baseline.profile != report.profile {
        violations.push(violation(
            BaselineViolationKind::ProfileMismatch,
            None,
            format!(
                "baseline profile {} does not match report profile {}",
                baseline.profile, report.profile
            ),
        ));
    }

    violations.extend(validate_report_bucket_shape(report));
    violations.extend(validate_semantic_boundary_shape(report));
    violations.extend(compare_file_results(baseline, report));
    violations.extend(compare_failure_buckets(baseline, report));
    violations.extend(compare_summary_assertions(baseline, report));
    violations.extend(compare_semantic_boundaries(baseline, report));

    BaselineComparison { violations }
}

fn compare_semantic_boundaries(
    baseline: &CompileBaseline,
    report: &RunReport,
) -> Vec<BaselineViolation> {
    let Some(accepted_boundaries) = &baseline.semantic_boundaries else {
        return Vec::new();
    };
    let mut baseline_by_key = BTreeMap::new();
    let mut current_by_key = BTreeMap::new();
    let mut violations = Vec::new();

    for boundary in accepted_boundaries {
        let key = semantic_boundary_key(boundary);
        if baseline_by_key.insert(key.clone(), boundary).is_some() {
            violations.push(violation(
                BaselineViolationKind::SemanticBoundary,
                Some(boundary.path.clone()),
                format!("baseline contains duplicate semantic boundary key: {}", boundary.id),
            ));
        }
    }
    for boundary in &report.semantic_boundaries {
        let key = semantic_boundary_key(boundary);
        if current_by_key.insert(key, boundary).is_some() {
            violations.push(violation(
                BaselineViolationKind::SemanticBoundary,
                Some(boundary.path.clone()),
                format!("current report contains duplicate semantic boundary key: {}", boundary.id),
            ));
        }
    }

    for (key, current) in &current_by_key {
        let Some(accepted) = baseline_by_key.get(key) else {
            violations.push(violation(
                BaselineViolationKind::SemanticBoundary,
                Some(current.path.clone()),
                format!(
                    "current semantic boundary is not accepted by the baseline: {}",
                    current.id
                ),
            ));
            continue;
        };
        if accepted != current {
            violations.push(violation(
                BaselineViolationKind::SemanticBoundary,
                Some(current.path.clone()),
                format!(
                    "current semantic boundary changed from the accepted baseline: {}",
                    current.id
                ),
            ));
        }
    }

    violations
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) struct SemanticBoundaryKey {
    path: String,
    id: String,
    source_start: usize,
    source_end: usize,
}

pub(crate) fn semantic_boundary_key(boundary: &ObservedSemanticBoundary) -> SemanticBoundaryKey {
    SemanticBoundaryKey {
        path: boundary.path.clone(),
        id: boundary.id.clone(),
        source_start: boundary.source_span.start,
        source_end: boundary.source_span.end,
    }
}

fn compare_file_results(baseline: &CompileBaseline, report: &RunReport) -> Vec<BaselineViolation> {
    let baseline_results = baseline
        .file_results
        .iter()
        .map(|result| (result.path.as_str(), result))
        .collect::<BTreeMap<_, _>>();
    let report_results = report
        .file_results
        .iter()
        .map(|result| (result.path.as_str(), result))
        .collect::<BTreeMap<_, _>>();
    let expected_failure_paths = baseline
        .expected_failures
        .iter()
        .map(|failure| failure.path.as_str())
        .collect::<BTreeSet<_>>();
    let report_failure_paths =
        report.failures.iter().map(|failure| failure.path.as_str()).collect::<BTreeSet<_>>();
    let mut violations = Vec::new();

    for (path, baseline_result) in &baseline_results {
        let Some(report_result) = report_results.get(path) else {
            violations.push(violation(
                BaselineViolationKind::MissingExpectedFile,
                Some((*path).to_string()),
                "baseline file is missing from current report",
            ));
            continue;
        };

        if baseline_result.status == RunnerStatus::Pass
            && report_result.status == RunnerStatus::Fail
        {
            violations.push(violation(
                BaselineViolationKind::PreviouslyPassingFileFailed,
                Some((*path).to_string()),
                "file passed in baseline but fails in current report",
            ));
        }
        if report_result.assertions_passed < baseline_result.assertions_passed {
            violations.push(violation(
                BaselineViolationKind::AssertionRegression,
                Some((*path).to_string()),
                format!(
                    "assertions passed regressed from {} to {}",
                    baseline_result.assertions_passed, report_result.assertions_passed
                ),
            ));
        }
        if report_result.assertions_total < baseline_result.assertions_total {
            violations.push(violation(
                BaselineViolationKind::AssertionRegression,
                Some((*path).to_string()),
                format!(
                    "assertions total regressed from {} to {}",
                    baseline_result.assertions_total, report_result.assertions_total
                ),
            ));
        }
    }

    for report_failure in &report_failure_paths {
        if !baseline_results.contains_key(report_failure)
            || !expected_failure_paths.contains(report_failure)
        {
            violations.push(violation(
                BaselineViolationKind::UnexpectedNewFailure,
                Some((*report_failure).to_string()),
                "current report contains a failure not accepted by the baseline",
            ));
        }
    }

    violations
}

fn compare_failure_buckets(
    baseline: &CompileBaseline,
    report: &RunReport,
) -> Vec<BaselineViolation> {
    let mut violations = Vec::new();
    for (bucket, current_count) in &report.buckets {
        let baseline_count = baseline.buckets.get(bucket).copied().unwrap_or(0);
        if *current_count > baseline_count {
            violations.push(violation(
                BaselineViolationKind::BucketCountIncreased,
                None,
                format!("bucket {bucket} increased from {baseline_count} to {current_count}"),
            ));
        }
    }
    violations
}

fn compare_summary_assertions(
    baseline: &CompileBaseline,
    report: &RunReport,
) -> Vec<BaselineViolation> {
    let mut violations = Vec::new();
    if report.summary.tap_assertions_passed < baseline.tap_assertions_passed {
        violations.push(violation(
            BaselineViolationKind::AssertionRegression,
            None,
            format!(
                "passed assertions regressed from {} to {}",
                baseline.tap_assertions_passed, report.summary.tap_assertions_passed
            ),
        ));
    }
    if report.summary.tap_assertions_total < baseline.tap_assertions_total {
        violations.push(violation(
            BaselineViolationKind::AssertionRegression,
            None,
            format!(
                "total assertions regressed from {} to {}",
                baseline.tap_assertions_total, report.summary.tap_assertions_total
            ),
        ));
    }
    violations
}

pub(crate) fn validate_report_bucket_shape(report: &RunReport) -> Vec<BaselineViolation> {
    let mut violations = Vec::new();
    let failure_paths =
        report.failures.iter().map(|failure| failure.path.as_str()).collect::<BTreeSet<_>>();
    for failure in &report.failures {
        if failure.bucket.trim().is_empty() {
            violations.push(violation(
                BaselineViolationKind::UnbucketedFailure,
                Some(failure.path.clone()),
                "failure has an empty bucket",
            ));
        } else if failure.bucket == "unknown" {
            violations.push(violation(
                BaselineViolationKind::UnknownBucket,
                Some(failure.path.clone()),
                "failure is bucketed as unknown",
            ));
        }
    }
    for result in &report.file_results {
        if result.status == RunnerStatus::Fail && !failure_paths.contains(result.path.as_str()) {
            violations.push(violation(
                BaselineViolationKind::UnbucketedFailure,
                Some(result.path.clone()),
                "failing file has no failure bucket record",
            ));
        }
    }
    violations
}

fn violation(
    kind: BaselineViolationKind,
    path: Option<String>,
    message: impl Into<String>,
) -> BaselineViolation {
    BaselineViolation { kind, path, message: message.into() }
}

pub(crate) fn validate_semantic_boundary_shape(report: &RunReport) -> Vec<BaselineViolation> {
    validate_semantic_boundary_inventory(&report.semantic_boundaries)
}

pub(crate) fn validate_semantic_boundary_inventory(
    boundaries: &[ObservedSemanticBoundary],
) -> Vec<BaselineViolation> {
    let mut violations = Vec::new();
    let mut keys = BTreeSet::new();
    for boundary in boundaries {
        let path = Some(boundary.path.clone());
        let mut add = |message: &str| {
            violations.push(violation(
                BaselineViolationKind::SemanticBoundary,
                path.clone(),
                message,
            ));
        };

        if !keys.insert(semantic_boundary_key(boundary)) {
            add("semantic boundary inventory contains a duplicate key");
        }

        if boundary.path.trim().is_empty() {
            add("semantic boundary has an empty path");
        }
        if boundary.id.trim().is_empty() {
            add("semantic boundary has an empty stable id");
        }
        if boundary.reason.trim().is_empty() {
            add("semantic boundary has an empty reason");
        }
        if boundary.source_kind.trim().is_empty() {
            add("semantic boundary has an empty source kind");
        }
        if boundary.owner_workstream.trim().is_empty() {
            add("semantic boundary has no owning workstream");
        }
        if boundary.supporting_test.trim().is_empty() {
            add("semantic boundary has no supporting test");
        }
        if boundary.source_span.start > boundary.source_span.end {
            add("semantic boundary source span is reversed");
        }

        match boundary.disposition {
            SemanticBoundaryDisposition::SourceLockedCompatibility => {
                if boundary.lock_scope != SemanticBoundaryLockScope::PathAndSource {
                    add("source-locked compatibility boundary must use a path_and_source lock");
                }
                if boundary.confidence != SemanticBoundaryConfidence::Exact {
                    add("source-locked compatibility boundary must have exact confidence");
                }
                if boundary.blocks_compilation {
                    add("source-locked compatibility boundary must not block compilation");
                }
            }
            SemanticBoundaryDisposition::Unknown => {
                add("unknown semantic boundary disposition is not admissible");
                if boundary.confidence != SemanticBoundaryConfidence::Unresolved {
                    add("unknown semantic boundary must have unresolved confidence");
                }
                if !boundary.blocks_compilation {
                    add("unknown semantic boundary must block compilation");
                }
            }
            SemanticBoundaryDisposition::Unsupported => {
                if boundary.confidence != SemanticBoundaryConfidence::Unresolved {
                    add("unsupported semantic boundary must have unresolved confidence");
                }
                if !boundary.blocks_compilation {
                    add("unsupported semantic boundary must block compilation");
                }
            }
            SemanticBoundaryDisposition::ImplementedStatic
            | SemanticBoundaryDisposition::StaticallyClassified
            | SemanticBoundaryDisposition::OrdinaryRuntime
            | SemanticBoundaryDisposition::DeferredRuntime
            | SemanticBoundaryDisposition::DeferredLifecycle => {
                if boundary.blocks_compilation {
                    add("non-blocking semantic boundary disposition cannot block compilation");
                }
            }
            SemanticBoundaryDisposition::GovernedCompileTimeDynamic => {}
        }
    }
    violations
}

pub(crate) fn validate_accepted_semantic_boundary_inventory(
    boundaries: &[ObservedSemanticBoundary],
) -> Vec<BaselineViolation> {
    let mut violations = validate_semantic_boundary_inventory(boundaries);
    for boundary in boundaries {
        let path = Some(boundary.path.clone());
        if matches!(
            boundary.disposition,
            SemanticBoundaryDisposition::Unknown | SemanticBoundaryDisposition::Unsupported
        ) {
            violations.push(violation(
                BaselineViolationKind::SemanticBoundary,
                path.clone(),
                "accepted baseline cannot contain unknown or unsupported semantic boundaries",
            ));
        }
        if boundary.blocks_compilation {
            violations.push(violation(
                BaselineViolationKind::SemanticBoundary,
                path,
                "accepted baseline cannot contain a compile-blocking semantic boundary",
            ));
        }
    }
    violations
}
