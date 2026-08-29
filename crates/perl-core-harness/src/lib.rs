Warning: truncated output (original token count: 117980)
Total output lines: 11574

#![warn(missing_docs)]
#![cfg_attr(clippy, allow(missing_docs))]

//! Upstream Perl core harness integration scaffold.
//!
//! The scaffold can discover tests from a prepared Perl source tree and run the
//! staged profile through a `t/perl` compatibility wrapper in parse and compile
//! modes. Execute mode is limited to explicit selected base tests.

pub mod artifacts;
/// Exact, repository-only Perl::Critic oracle subjects and bounded reuse.
pub mod critic_oracle;
#[path = "target_contracts/contract.rs"]
pub mod contract;
#[path = "target_contracts/io.rs"]
pub mod io;
#[path = "target_contracts/matrix.rs"]
pub mod matrix;
#[path = "target_contracts/model.rs"]
pub mod model;
/// Typed contracts for the upstream Perl target topology.
pub mod target_contracts {
    pub use super::{contract, io, matrix, model};
}

#[cfg(test)]
#[path = "target_contracts/tests.rs"]
mod target_contract_tests;

mod normalization;
pub mod public_evidence;
mod run_authority;
mod series;
pub mod transition;

// The runner-plan authority modules are shared verbatim with the
// `perl-core-harness-runner-plan` binary units; the observed-discovery receipt
// surface reuses them so there is exactly one source-frame normalizer and one
// target-selection vocabulary. Items unused by this unit remain compiled for
// the other inclusion sites.
#[allow(dead_code)]
#[path = "runner_plan/build.rs"]
pub(crate) mod build;
#[allow(dead_code)]
// The shared normalizer's own test module predates this inclusion site and
// uses workspace-denied helpers; its tests remain exercised by their original
// bin/test units.
#[cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
#[path = "runner_plan/normalize.rs"]
pub(crate) mod normalize;
#[allow(dead_code)]
#[path = "runner_plan/model.rs"]
pub(crate) mod runner_model;

/// Strict immutable observed upstream-discovery receipts
/// (`upstream_runner_discovery.v1`, #12281): byte-exact raw envelopes, typed
/// terminal state, frame-aware decoded rows, membership dispositions, work
/// accounting, and deterministic digests over canonical payloads.
pub mod observed_discovery {
    /// Strict constructors, payload digests, freshness, and matrix adapter.
    #[path = "build.rs"]
    pub mod build;
    /// Exact supervised `t/TEST` capture route producing strict receipts
    /// (#12283): selector argv from target-contract authority, one bounded
    /// supervised process, byte-exact envelopes, and #12281 receipt assembly.
    #[path = "capture.rs"]
    pub mod capture;
    /// Strict byte-level stream decoder and observation-state derivation.
    #[path = "decode.rs"]
    pub mod decode;
    /// Receipt, envelope, row, disposition, subject, and work types.
    #[path = "model.rs"]
    pub mod model;
    /// Fail-closed validation reconstructing rows from retained raw bytes.
    #[path = "validate.rs"]
    pub mod validate;

    #[cfg(test)]
    #[path = "tests.rs"]
    mod tests;

    pub use build::{
        build_observed_discovery_receipt, check_observed_discovery_against,
        discovery_payload_digest, receipt_freshness,
    };
    pub use capture::{ObserveDiscoveryConfig, observe_discovery, observe_discovery_command};
    pub use decode::derive_observation_state;
    // The runner-plan vocabulary is already part of this module's public
    // payload types; re-export the two enums external consumers need to build
    // or inspect receipts without reaching into the crate-private module.
    pub use crate::runner_model::{DiscoveryFrame, RunnerKind};
    pub use model::{
        DiscoveryObservationState, DiscoveryPayload, DiscoverySubjectIdentity, EnvironmentIdentity,
        EvidenceClass, InvocationObservation, LineFraming, MemberDisposition,
        ObservedDiscoveryInput, ObservedDiscoveryRow, ProcessCompletion, RawStreamEnvelope,
        ReceiptFreshness, RunnerArtifactIdentity, TerminalObservation,
        UPSTREAM_DISCOVERY_SCHEMA_VERSION, UpstreamDiscoveryReceiptV1,
    };
    pub use validate::{validate_observed_discovery_receipt, validate_receipt_subject_binding};
}

/// Strict effective-invocation trace contract
/// (`upstream_effective_invocation_trace.v1`, #12284): one bounded JSONL
/// frame stream with typed per-field observation states, parent
/// discovery-receipt re-binding, proven work accounting, deterministic
/// digests, and the pure #8492 canonical-plan projection adapter.
/// Representation only: no upstream instrumentation, process execution, or
/// filesystem interaction.
pub mod invocation_trace {
    /// Pure checked adapter to canonical plan projections.
    #[path = "adapter.rs"]
    pub mod adapter;
    /// Strict constructors, payload digests, freshness, and parent adapter.
    #[path = "build.rs"]
    pub mod build;
    /// Strict byte-level frame decoder and row-state derivation.
    #[path = "decode.rs"]
    pub mod decode;
    /// Receipt, frame, field-state, row, subject, and work types.
    #[path = "model.rs"]
    pub mod model;
    /// Fail-closed validation reconstructing frames from retained raw bytes.
    #[path = "validate.rs"]
    pub mod validate;

    #[cfg(test)]
    #[path = "test_support.rs"]
    pub(crate) mod test_support;

    #[cfg(test)]
    #[path = "tests.rs"]
    mod tests;

    pub use adapter::{
        ExpectedFieldComparison, ExpectedFieldResult, ExpectedInvocationBinding,
        ExpectedInvocationValues, ProjectionOutcome, ProjectionRejection, compare_expected,
        project_effective_invocation,
    };
    pub use build::{
        build_invocation_trace_receipt, check_invocation_trace_against, trace_payload_digest,
        trace_receipt_freshness,
    };
    pub use decode::derive_row_state;
    pub use model::{
        CanonicalInvocationProjection, CapturePoint, EffectiveInvocationField,
        EffectiveInvocationFields, EffectiveInvocationRow, EffectiveInvocationTraceReceiptV1,
        FieldKey, FieldStateCounts, InvocationAuthority, InvocationObservationState,
        ObservedInvocationTraceInput, ProjectionRecord, ProjectionRejectionKind, RowSubjectBinding,
        ScriptRole, TaintMode, TestInitClass, TraceHeader, TracePayload, TraceRowDisposition,
        TraceStreamEnvelope, TraceStreamOutcome, TraceSubjectIdentity, TraceTerminal, TraceWork,
        UPSTREAM_INVOCATION_TRACE_SCHEMA_VERSION, Utf8Switch,
    };
    pub use validate::{validate_invocation_trace_receipt, validate_trace_receipt_subject_binding};
}

/// Strict pure fan-in join proving one complete observed runner subject
/// (`observed_runner_subject.v1`, #12287): the observed `t/TEST` membership
/// (#12281/#12283), its independently reconstructed plan (#7737), and the
/// effective-invocation observation set (#12284/#12285) joined one-to-one
/// under the exact #12286 transfer relation and #12158 producer identity.
/// Representation only: no upstream execution, tracing, compiler invocation,
/// production selection, or accepted-state transition.
pub mod observed_subject {
    /// Strict constructors, digests, freshness, and the join arithmetic.
    #[path = "build.rs"]
    pub mod build;
    /// Receipt, binding, row, disposition, diagnostic, state, and work types.
    #[path = "model.rs"]
    pub mod model;
    /// Fail-closed structural validation re-proving receipt-traveled laws.
    #[path = "validate.rs"]
    pub mod validate;

    #[cfg(test)]
    #[path = "tests.rs"]
    mod tests;

    pub use build::{
        build_observed_runner_subject, check_observed_runner_subject, observed_subject_freshness,
        observed_subject_payload_digest,
    };
    pub use model::{
        JoinWork, OBSERVED_RUNNER_SUBJECT_SCHEMA_VERSION, OBSERVED_SUBJECT_CLAIM_BOUNDARY,
        ObservedRunnerSubjectInput, ObservedRunnerSubjectPayload, ObservedRunnerSubjectRow,
        ObservedRunnerSubjectV1, ObservedSubjectBindings, ObservedSubjectState,
        OrdinaryInstrumentedEquivalenceIdentity, ProducerSubjectIdentity, SubjectDiagnostic,
        SubjectJoinDisposition,
    };
    pub use validate::validate_observed_runner_subject_shape;
}

use chrono::Utc;
use color_eyre::eyre::{Context, Result, bail};
use perl_core_harness_types::{
    BOUNDARY_RETIREMENT_SCHEMA_VERSION, BaselineComparison, BaselineViolation,
    BaselineViolationKind, BoundaryRetirement, COMPILE_BASELINE_SCHEMA_VERSION,
    COMPILE_BASELINE_V2_SCHEMA_VERSION, COMPILER_COMPATIBILITY_SCHEMA_VERSION,
    CURRENT_AUTHORITY_INDEX_SCHEMA_VERSION, CompatibilityAcceptedRatchet,
    CompatibilityClusterState, CompatibilityDebtState, CompatibilityObservation,
    CompatibilityRailAvailability, CompatibilityRailState, CompatibilityRunState,
    CompatibilitySeriesIdentity, CompatibilityTransition, CompatibilityTransitionCandidate,
    CompileBaseline, CompileBaselineV2, CompilerCompatibilitySeries, CompilerCompatibilityState,
    CurrentAuthorityEntry, CurrentAuthorityIndex, CurrentAuthorityStatus, DISCOVERY_SCHEMA_VERSION,
    DiscoveredTest, DiscoveryReport, FAILURE_CLUSTER_HISTORY_SCHEMA_VERSION,
    FAILURE_CLUSTER_SCHEMA_VERSION, FailureCluster, FailureClusterHistory,
    FailureClusterHistoryEntry, FailureClusterHistoryPresence, FailureClusterHistoryStatus,
    FailureClusterIdentityQuality, FailureClusterReport, FailureClusterSignature,
    FailureDebtCandidate, GAP_MAP_SCHEMA_VERSION, GapMap, LANDED_LINEAGE_SCHEMA_VERSION,
    LandedLineage, ObservedSemanticBoundary, PREPARE_SCHEMA_VERSION, PrepareReceipt, PrepareStatus,
    RUN_REPORT_SCHEMA_VERSION, RunFailure, RunFileResult, RunReport, RunSummary, RunnerRecord,
    RunnerStatus, SEMANTIC_BOUNDARY_REGISTRY_SCHEMA_VERSION, SMOKE_SCHEMA_VERSION,
    SemanticBoundaryConfidence, SemanticBoundaryDisposition, SemanticBoundaryLockScope,
    SemanticBoundaryRegistry, SemanticBoundaryRegistryEntry, SemanticBoundaryRegistryState,
    SemanticBoundaryReplacementStrategy, SeriesManifest, SmokeFailureKind, SmokeReport,
    SmokeStatus, SmokeStructuralFailure, lsp_impact_for_bucket, workstream_for_bucket,
};
pub use perl_core_harness_types::{HarnessMode, HarnessProfile, HarnessRunner};
use run_authority::{
    DirectDiagnosticReceipt, DirectDiagnosticSet, SettledDiagnosticProbe, UpstreamObservationSet,
    direct_diagnostics_receipt, direct_diagnostics_receipt_path, settle_probe_context_rows,
};
pub use series::{SeriesManifestConfig, series_manifest};

use normalization::{hex_lower, sha256_digest_bytes};
use public_evidence::PublicStringClass;
use serde::{Deserialize, de::DeserializeOwned};
use series::{read_series_manifest, validate_series_manifest};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self as std_io, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

const PERL_SOURCE_URL: &str = "https://github.com/Perl/perl5";
const EXECUTE_BASE_ALLOWLIST: &[&str] =
    &["base/if.t", "base/cond.t", "base/num.t", "base/pat.t", "base/translate.t", "base/while.t"];
static RUN_COPY_COUNTER: AtomicU64 = AtomicU64::new(0);

pub(crate) fn project_root() -> Result<PathBuf> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .and_then(|crates_dir| crates_dir.parent())
        .map(Path::to_path_buf)
        .ok_or_else(|| color_eyre::eyre::eyre!("perl-core-harness should live under crates/"))
}

fn profile_runner_args(
    profile: HarnessProfile,
    t_dir: &Path,
    runner: HarnessRunner,
) -> Result<Vec<String>> {
    match runner {
        HarnessRunner::Test => explicit_test_runner_args(t_dir, profile.roots()),
        HarnessRunner::Harness => {
            Ok(profile.roots().iter().map(|root| format!("{root}/*.t")).collect())
        }
    }
}

fn explicit_test_runner_args(t_dir: &Path, roots: &[&str]) -> Result<Vec<String>> {
    let mut args = Vec::new();
    for root in roots {
        collect_test_files(t_dir, &t_dir.join(root), &mut args)?;
    }
    args.sort();
    args.dedup();
    Ok(args)
}

fn collect_test_files(t_dir: &Path, dir: &Path, args: &mut Vec<String>) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    let entries = fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))?;
    for entry in entries {
        let entry = entry.with_context(|| format!("reading entry in {}", dir.display()))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .with_context(|| format!("reading file type for {}", path.display()))?;
        if file_type.is_dir() {
            collect_test_files(t_dir, &path, args)?;
            continue;
        }
        if !file_type.is_file() || path.extension().and_then(|ext| ext.to_str()) != Some("t") {
            continue;
        }
        let relative = path
            .strip_prefix(t_dir)
            .with_context(|| format!("normalizing test path {}", path.display()))?;
        args.push(relative.display().to_string().replace('\\', "/"));
    }
    Ok(())
}

fn normalize_selected_tests(profile: HarnessProfile, tests: &[String]) -> Result<Vec<String>> {
    let allowed_roots = profile.roots().iter().copied().collect::<BTreeSet<_>>();
    let mut normalized = Vec::new();
    for test in tests {
        let path = normalize_test_path(test)
            .ok_or_else(|| color_eyre::eyre::eyre!("invalid Perl core test path: {test}"))?;
        if path.contains("..") || path.starts_with('/') || !path.ends_with(".t") {
            bail!("invalid Perl core test path: {test}");
        }
        let Some((root, _rest)) = path.split_once('/') else {
            bail!("selected Perl core test must include a profile root: {test}");
        };
        if !allowed_roots.contains(root) {
            bail!(
                "selected Perl core test {path} is outside profile {} roots {:?}",
                profile,
                profile.roots()
            );
        }
        normalized.push(path);
    }
    normalized.sort();
    normalized.dedup();
    Ok(normalized)
}

fn validate_execute_selection(mode: HarnessMode, selected_tests: &[String]) -> Result<()> {
    if mode != HarnessMode::Execute {
        return Ok(());
    }
    if selected_tests.is_empty() {
        bail!(
            "perl-core-harness run --mode execute requires one or more explicit --test selections from {}",
            EXECUTE_BASE_ALLOWLIST.join(", ")
        );
    }
    if let Some(test) =
        selected_tests.iter().find(|test| !EXECUTE_BASE_ALLOWLIST.contains(&test.as_str()))
    {
        bail!(
            "perl-core-harness run --mode execute supports only selected base tests {}; rejected {test}",
            EXECUTE_BASE_ALLOWLIST.join(", ")
        );
    }
    Ok(())
}

fn filter_discovered_tests(
    tests: Vec<DiscoveredTest>,
    selected_tests: &[String],
) -> Result<Vec<DiscoveredTest>> {
    if selected_tests.is_empty() {
        return Ok(tests);
    }

    let selected = selected_tests.iter().cloned().collect::<BTreeSet<_>>();
    let filtered =
        tests.into_iter().filter(|test| selected.contains(&test.path)).collect::<Vec<_>>();
    let found = filtered.iter().map(|test| test.path.clone()).collect::<BTreeSet<_>>();
    if let Some(missing) = selected.iter().find(|path| !found.contains(*path)) {
        bail!("selected Perl core test {missing} was not discovered by upstream harness");
    }
    Ok(filtered)
}

/// Configuration for `perl-core-harness discover`.
#[derive(Debug, Clone)]
pub struct DiscoverConfig {
    pub perl_tree: PathBuf,
    pub host_perl: PathBuf,
    pub runner: HarnessRunner,
    pub profile: HarnessProfile,
    pub output: Option<PathBuf>,
}

/// Configuration for `perl-core-harness prepare`.
#[derive(Debug, Clone)]
pub struct PrepareConfig {
    pub perl_ref: String,
    pub output_dir: Option<PathBuf>,
}

/// Configuration for `perl-core-harness run`.
#[derive(Debug, Clone)]
pub struct RunConfig {
    pub perl_tree: PathBuf,
    pub host_perl: PathBuf,
    pub runner: HarnessRunner,
    pub mode: HarnessMode,
    pub profile: HarnessProfile,
    pub tests: Vec<String>,
    pub output: Option<PathBuf>,
    pub runner_binary: Option<PathBuf>,
    /// Whether missing upstream rows may be investigated by bounded direct
    /// diagnostic probes after the upstream report is frozen (#8173).
    ///
    /// Diagnostics are retained under a separate receipt and can never change
    /// the upstream result, totals, or verdict.
    pub diagnostic_probes: bool,
}

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

/// Configuration for `perl-core-harness boundaries`.
#[derive(Debug, Clone)]
pub struct BoundaryRegistryConfig {
    pub registry: PathBuf,
    pub baselines: Vec<PathBuf>,
    pub bundles: Vec<PathBuf>,
    pub output: Option<PathBuf>,
    pub check: bool,
    pub report: bool,
    pub historical: bool,
}

/// Configuration for `perl-core-harness triage`.
#[derive(Debug, Clone)]
pub struct TriageConfig {
    pub bundle: PathBuf,
    pub output: PathBuf,
    pub history: Option<PathBuf>,
    pub write_history: bool,
    pub check_history: bool,
}

/// Input receipts for one independently identified compatibility series.
#[derive(Debug, Clone)]
pub struct CompatibilitySeriesInput {
    pub series_manifest: PathBuf,
    pub parse_report: PathBuf,
    pub compile_report: PathBuf,
    pub compile_baseline: PathBuf,
    /// Accepted ratchet to compare with the current observation.
    pub accepted_baseline: Option<PathBuf>,
    pub evidence_bundle: PathBuf,
    pub boundary_registry: Option<PathBuf>,
    pub cluster_history: Option<PathBuf>,
    pub execute_report: Option<PathBuf>,
    /// Optional #5234 current-authority admission proof.
    pub current_authority: Option<CurrentAuthorityConfig>,
}

/// Configuration for loading typed compiler compatibility state.
#[derive(Debug, Clone)]
pub struct CompatibilityLoadConfig {
    pub inputs: Vec<CompatibilitySeriesInput>,
    pub repository_commit: String,
}

/// Configuration for `perl-core-harness smoke`.
#[derive(Debug, Clone)]
pub struct SmokeConfig {
    pub perl_tree: PathBuf,
    pub host_perl: PathBuf,
    pub runner: HarnessRunner,
    pub profile: HarnessProfile,
    pub modes: Vec<HarnessMode>,
    pub output_dir: Option<PathBuf>,
    pub runner_binary: Option<PathBuf>,
    pub perl_ref: Option<String>,
}

/// Discover test files from a prepared Perl tree and write a JSON manifest.
pub fn discover(config: DiscoverConfig) -> Result<()> {
    let perl_tree = canonicalize_existing_dir(&config.perl_tree, "prepared Perl tree")?;
    let t_dir = perl_tree.join("t");
    let script = validate_runner_script(&t_dir, config.runner)?;
    let output_path = config.output.unwrap_or_else(|| default_discovery_path(config.profile));

    let output = invoke_dumptests(
        &config.host_perl,
        &t_dir,
        &script,
        &profile_runner_args(config.profile, &t_dir, config.runner)?,
    )
    .with_context(|| {
        format!("discovering Perl core tests via {} {}", config.runner, config.profile)
    })?;

    let tests = parse_dumptests_output(&output.stdout)?;
    let report = DiscoveryReport {
        schema_version: DISCOVERY_SCHEMA_VERSION.to_string(),
        commit: current_commit(),
        timestamp: Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        perl_ref: perl_tree_ref(&perl_tree),
        prepared_tree: perl_tree.display().to_string(),
        host_perl: config.host_perl.display().to_string(),
        runner: config.runner,
        profile: config.profile,
        tests,
    };

    write_discovery_report(&output_path, &report)?;
    tracing::info!(
        "perl-core-harness: discovered {} tests for profile {} via {}",
        report.tests.len(),
        report.profile,
        report.runner
    );
    tracing::info!("wrote {}", output_path.display());
    Ok(())
}

/// Validate the semantic-boundary registry against accepted v2 baselines and
/// optional durable evidence-bundle indexes.
pub fn boundaries(config: BoundaryRegistryConfig) -> Result<()> {
    let raw = fs::read_to_string(&config.registry)
        .with_context(|| format!("reading boundary registry {}", config.registry.display()))?;
    let registry: SemanticBoundaryRegistry = serde_json::from_str(&raw)
        .with_context(|| format!("decoding boundary registry {}", config.registry.display()))?;

    let mut violations = validate_boundary_registry_shape(&registry);
    let mut baseline_data = Vec::new();
    for path in &config.baselines {
        match read_compile_baseline_v2(path) {
            Ok(baseline) => baseline_data.push((path.clone(), baseline)),
            Err(error) => violations.push(format!("{}: {error}", path.display())),
        }
    }

    let mut bundle_data = Vec::new();
    for path in &config.bundles {
        match read_boundary_bundle(path) {
            Ok(bundle) => bundle_data.push(bundle),
            Err(error) => violations.push(format!("{}: {error}", path.display())),
        }
    }

    for (path, baseline) in &baseline_data {
        violations.extend(validate_registry_against_baseline(
            &registry,
            baseline,
            config.historical,
        ));
        for bundle in bundle_data.iter().filter(|bundle| {
            bundle.index.series_id == baseline.series_id && bundle.index.profile == baseline.profile
        }) {
            violations.extend(validate_bundle_against_baseline(bundle, baseline));
        }
        if !config.bundles.is_empty()
            && !bundle_data.iter().any(|bundle| {
                bundle.index.series_id == baseline.series_id
                    && bundle.index.profile == baseline.profile
            })
        {
            violations.push(format!(
                "{}: no evidence bundle was supplied for series {} profile {}",
                path.display(),
                baseline.series_id,
                baseline.profile
            ));
        }
    }
    for bundle in &bundle_data {
        if !baseline_data.iter().any(|(_, baseline)| {
            baseline.series_id == bundle.index.series_id && baseline.profile == bundle.index.profile
        }) {
            violations.push(format!(
                "bundle {} has no matching accepted baseline authority",
                bundle.index.bundle_id
            ));
        }
    }

    let report = boundary_registry_report(
        &registry,
        &baseline_data,
        &bundle_data,
        config.historical,
        violations,
    );
    let json =
        serde_json::to_string_pretty(&report).context("serializing boundary registry report")?;
    if let Some(path) = &config.output {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("creating boundary registry report directory {}", parent.display())
            })?;
        }
        fs::write(path, format!("{json}\n"))
            .with_context(|| format!("writing boundary registry report {}", path.display()))?;
    } else if config.report {
        std_io::stdout()
            .write_all(format!("{json}\n").as_bytes())
            .context("writing boundary registry report")?;
    }

    if !report.valid {
        bail!(
            "semantic-boundary registry validation failed with {} violation(s):\n{}",
            report.violations.len(),
            report.violations.join("\n")
        );
    }
    Ok(())
}

/// Cluster typed failures and separate semantic-boundary debt from product failures.
pub fn triage(config: TriageConfig) -> Result<()> {
    let bundle = read_boundary_bundle(&config.bundle)?;
    let compile_path = bundle_artifact_path(&bundle, "compile_report")?;
    let raw = fs::read_to_string(&compile_path)
        .with_context(|| format!("reading compile report {}", compile_path.display()))?;
    let report: RunReport = serde_json::from_str(&raw)
        .with_context(|| format!("decoding compile report {}", compile_path.display()))?;
    validate_bundle_report_identity(&bundle, &report)?;
    ensure_valid_report_shape(&report)?;
    let cluster_report = build_failure_cluster_report(&bundle, &report)?;
    fs::create_dir_all(&config.output)
        .with_context(|| format!("creating triage output directory {}", config.output.display()))?;
    let json =
        serde_json::to_string_pretty(&cluster_report).context("serializing failure clusters")?;
    fs::write(config.output.join("failure-clusters.json"), format!("{json}\n"))
        .context("writing failure-clusters.json")?;
    fs::write(
        config.output.join("failure-clusters.md"),
        render_failure_cluster_markdown(&cluster_report),
    )
    .context("writing failure-clusters.md")?;

    if config.write_history || config.check_history {
        let history_path = config.history.as_ref().ok_or_else(|| {
            color_eyre::eyre::eyre!("history path is required for history checks")
        })?;
        let history = read_cluster_history(history_path, config.write_history)?;
        let shape_violations = validate_cluster_history_shape(&history);
        if !shape_violations.is_empty() {
            bail!("cluster history is invalid:\n{}", shape_violations.join("\n"));
        }
        if config.check_history {
            let violations = validate_history_against_report(&history, &cluster_report);
            if !violations.is_empty() {
                bail!("cluster history check failed:\n{}", violations.join("\n"));
            }
        } else {
            let updated = merge_cluster_history(history, &cluster_report)?;
            let updated_json =
                serde_json::to_string_pretty(&updated).context("serializing cluster history")?;
            if let Some(parent) = history_path.parent() {
                fs::create_dir_all(parent).with_context(|| {
                    format!("creating cluster history directory {}", parent.display())
                })?;
            }
            fs::write(history_path, format!("{updated_json}\n"))
                .with_context(|| format!("writing cluster history {}", history_path.display()))?;
            fs::write(config.output.join("cluster-history.json"), format!("{updated_json}\n"))
                .context("writing cluster-history.json")?;
            fs::write(
                config.output.join("cluster-history.md"),
                render_cluster_history_markdown(&updated),
            )
            .context("writing cluster-history.md")?;
        }
    }
    Ok(())
}

fn read_cluster_history(path: &Path, allow_missing: bool) -> Result<FailureClusterHistory> {
    if !path.is_file() {
        if allow_missing {
            return Ok(FailureClusterHistory {
                schema_version: FAILURE_CLUSTER_HISTORY_SCHEMA_VERSION.into(),
                entries: Vec::new(),
            });
        }
        bail!("cluster history {} is missing", path.display());
    }
    let raw = fs::read_to_string(path)
        .with_context(|| format!("reading cluster history {}", path.display()))?;
    serde_json::from_str(&raw)
        .with_context(|| format!("decoding cluster history {}", path.display()))
}

/// Inputs for validating post-merge evidence lineage and current authority.
#[derive(Debug, Clone)]
pub struct CurrentAuthorityConfig {
    /// Deterministic current-authority index.
    pub index: PathBuf,
    /// Landed-lineage records referenced by the index.
    pub lineages: Vec<PathBuf>,
    /// Repository tree containing the published evidence artifacts.
    pub repository_root: PathBuf,
    /// Exact Git commit containing the published authority records. Each
    /// lineage record's `landed_sha` identifies the measured code commit.
    pub landed_sha: String,
}

fn authority_status_rank(status: CurrentAuthorityStatus) -> u8 {
    match status {
        CurrentAuthorityStatus::Current => 0,
        CurrentAuthorityStatus::Historical => 1,
        CurrentAuthorityStatus::Superseded => 2,
    }
}

fn authority_entry_key(entry: &CurrentAuthorityEntry) -> (&str, u8, &str) {
    (
        entry.series_id.as_str(),
        authority_status_rank(entry.status),
        entry.observation_bundle_id.as_str(),
    )
}

/// Validate the immutable identity chain for a current-authority index.
pub fn validate_current_authority(config: CurrentAuthorityConfig) -> Result<CurrentAuthorityIndex> {
    if config.lineages.is_empty() {
        bail!("current-authority validation requires at least one lineage record");
    }
    validate_git_sha(&config.landed_sha, "expected landed SHA")?;
    validate_git_commit(&config.repository_root, &config.landed_sha)?;
    let index_path = repository_relative_path(&config.repository_root, &config.index)?;
    let index: CurrentAuthorityIndex = read_json_bytes(
        &git_blob_at(&config.repository_root, &config.landed_sha, &index_path)?,
        "current-authority index",
    )?;
    if index.schema_version != CURRENT_AUTHORITY_INDEX_SCHEMA_VERSION {
        bail!("unsupported current-authority index schema {}", index.schema_version);
    }
    if index.entries.is_empty() {
        bail!("current-authority index contains no series");
    }
    let mut declared_current_series = BTreeSet::new();
    for entry in &index.entries {
        if matches!(entry.status, CurrentAuthorityStatus::Current)
            && !declared_current_series.insert(entry.series_id.clone())
        {
            bail!("duplicate current authority for series {}", entry.series_id);
        }
    }
    if !index
        .entries
        .windows(2)
        .all(|pair| authority_entry_key(&pair[0]) < authority_entry_key(&pair[1]))
    {
        bail!(
            "current-authority index entries must be sorted by series, status, and observation bundle"
        );
    }

    let mut lineages = Vec::new();
    let mut lineage_paths = BTreeSet::new();
    for path in &config.lineages {
        let relative_path = repository_relative_path(&config.repository_root, path)?;
        let lineage = read_json_bytes(
            &git_blob_at(&config.repository_root, &config.landed_sha, &relative_path)?,
            "landed lineage",
        )?;
        validate_landed_lineage_shape(&lineage)?;
        validate_git_ancestor(&config.repository_root, &lineage.landed_sha, &config.landed_sha)?;
        validate_git_ancestor(
            &config.repository_root,
            &lineage.publication_sha,
            &config.landed_sha,
        )?;
        validate_publication_scope(&config.repository_root, &lineage)?;
        if !lineage_paths.insert(relative_path.clone()) {
            bail!("duplicate landed lineage path");
        }
        lineages.push((relative_path, lineage));
    }
    validate_supersession_graph(&lineages)?;

    let indexed_series =
        index.entries.iter().map(|entry| entry.series_id.clone()).collect::<BTreeSet<_>>();
    let mut current_series = BTreeSet::new();
    for entry in &index.entries {
        if entry.series_id.trim().is_empty()
            || entry.manifest_hash.trim().is_empty()
            || entry.observation_bundle_id.trim().is_empty()
            || entry.observation_bundle_digest.trim().is_empty()
            || entry.claim_boundary.trim().is_empty()
        {
            bail!("current-authority entry has incomplete identity");
        }
        validate_public_path(&entry.observation_bundle_path, "observation bundle path")?;
        validate_public_path(&entry.landed_lineage_path, "landed lineage path")?;
        if let Some(path) = &entry.accepted_baseline_path {
            validate_public_path(path, "accepted baseline path")?;
        }
        let (lineage_path, lineage) = lineages
            .iter()
            .find(|(path, lineage)| {
                lineage.series_id == entry.series_id && path == &entry.landed_lineage_path
            })
            .ok_or_else(|| {
                color_eyre::eyre::eyre!(
                    "current-authority entry {} has no matching lineage",
                    entry.series_id
                )
            })?;
        if entry.profile != lineage.profile
            || entry.manifest_hash != lineage.manifest_hash
            || entry.observation_bundle_id != lineage.evidence_bundle_id
            || entry.observation_bundle_digest != lineage.evidence_bundle_digest
            || entry.observation_transition != lineage.observation_transition
            || entry.accepted_transition_id != lineage.accepted_transition_id
            || entry.accepted_baseline_digest != lineage.accepted_baseline_digest
            || entry.accepted_baseline_evidence_bundle != lineage.accepted_baseline_evidence_bundle
        {
            bail!("current-authority entry {} disagrees with its lineage", entry.series_id);
        }
        if matches!(entry.status, CurrentAuthorityStatus::Current) {
            current_series.insert(entry.series_id.clone());
        }
        if matches!(entry.status, CurrentAuthorityStatus::Current)
            && matches!(lineage.observation_transition, CompatibilityTransition::Historical)
        {
            bail!("historical lineage {} cannot be current authority", entry.series_id);
        }
        if matches!(lineage.observation_transition, CompatibilityTransition::Regression)
            && (entry.accepted_baseline_path.is_none() || entry.accepted_transition_id.is_none())
        {
            bail!(
                "regression {} must retain an explicit accepted baseline and transition",
                entry.series_id
            );
        }
        if !lineage.authoritative_artifacts.contains_key(&entry.observation_bundle_path) {
            bail!(
                "observation bundle {} is absent from authoritative artifacts",
                entry.observation_bundle_path
            );
        }
        validate_bundle_identity(
            &config.repository_root,
            &entry.observation_bundle_path,
            entry,
            lineage,
        )?;
        validate_accepted_baseline(&config.repository_root, entry, lineage)?;
        validate_artifact_digests(
            &config.repository_root,
            &index_path,
            &lineage_paths,
            lineage_path,
            lineage,
        )?;
    }
    if current_series.is_empty() {
        bail!("current-authority index has no current series");
    }
    if current_series != indexed_series {
        bail!("every indexed series must have exactly one current authority");
    }
    Ok(index)
}

fn read_json_bytes<T: DeserializeOwned>(bytes: &[u8], label: &str) -> Result<T> {
    serde_json::from_slice(bytes).with_context(|| format!("decoding {label}"))
}

fn validate_landed_lineage_shape(lineage: &LandedLineage) -> Result<()> {
    if lineage.schema_version != LANDED_LINEAGE_SCHEMA_VERSION {
        bail!("unsupported landed-lineage schema {}", lineage.schema_version);
    }
    for (label, value) in [
        ("series ID", &lineage.series_id),
        ("manifest hash", &lineage.manifest_hash),
        ("bundle ID", &lineage.evidence_bundle_id),
        ("bundle digest", &lineage.evidence_bundle_digest),
        ("measurement SHA", &lineage.measurement_sha),
        ("publication SHA", &lineage.publication_sha),
        ("landed SHA", &lineage.landed_sha),
        ("publication base SHA", &lineage.publication_base_sha),
        ("recorder schema version", &lineage.recorder_schema_version),
        ("creation reason", &lineage.created_reason),
    ] {
        if value.trim().is_empty() {
            bail!("landed lineage has empty {label}");
        }
    }
    for (label, value) in [
        ("measurement SHA", &lineage.measurement_sha),
        ("publication SHA", &lineage.publication_sha),
        ("landed SHA", &lineage.landed_sha),
        ("publication base SHA", &lineage.publication_base_sha),
    ] {
        validate_git_sha(value, label)?;
    }
    validate_digest(&lineage.evidence_bundle_digest, "bundle digest")?;
    if lineage.authoritative_artifacts.is_empty() {
        bail!("landed lineage has no authoritative artifacts");
    }
    validate_publication_paths(&lineage.publication_paths)?;
    for (path, digest) in &lineage.authoritative_artifacts {
        validate_public_path(path, "authoritative artifact path")?;
        validate_digest(digest, "authoritative artifact digest")?;
        if lineage.publication_paths.iter().all(|published| published != path) {
            bail!("authoritative artifact {path} is absent from publication paths");
        }
    }
    let artifact_paths = lineage
        .authoritative_artifacts
        .keys()
        .map(|path| path.replace('\\', "/"))
        .collect::<BTreeSet<_>>();
    let publication_paths = lineage
        .publication_paths
        .iter()
        .map(|path| path.replace('\\', "/"))
        .collect::<BTreeSet<_>>();
    if artifact_paths != publication_paths {
        bail!("publication paths and authoritative artifact digests must match exactly");
    }
    Ok(())
}

fn validate_bundle_identity(
    root: &Path,
    bundle_path: &str,
    entry: &CurrentAuthorityEntry,
    lineage: &LandedLineage,
) -> Result<()> {
    let bytes = git_blob_at(root, &lineage.landed_sha, bundle_path)?;
    let actual_digest = sha256_digest_bytes(&bytes);
    if actual_digest != lineage.evidence_bundle_digest
        || actual_digest != entry.observation_bundle_digest
    {
        bail!("observation bundle digest does not match landed lineage");
    }
    let index: EvidenceBundleIndex = read_json_bytes(&bytes, "observation bundle")?;
    if index.schema_version != "perl_core_harness.evidence_bundle.v1"
        || index.bundle_id != lineage.evidence_bundle_id
        || index.series_id != lineage.series_id
        || index.series_id != entry.series_id
        || index.manifest_hash != lineage.manifest_hash
        || index.manifest_hash != entry.manifest_hash
        || index.profile != lineage.profile
        || index.lineage.measurement_sha != lineage.measurement_sha
        || index.lifecycle != "published"
        || index.completeness.status != "complete"
        || !index.completeness.normalized_authority
    {
        bail!("observation bundle is not a complete normalized authority for its series");
    }
    let artifact =
        index.artifacts.iter().find(|artifact| artifact.kind == "semantic_boundaries").ok_or_else(
            || color_eyre::eyre::eyre!("observation bundle has no semantic-boundaries artifact"),
        )?;
    let artifact_path = Path::new(bundle_path)
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(&artifact.logical_path);
    let artifact_path = artifact_path.to_string_lossy().replace('\\', "/");
    validate_public_path(&artifact_path, "evidence bundle artifact")?;
    if !lineage.authoritative_artifacts.contains_key(&artifact_path) {
        bail!("semantic-boundaries artifact is absent from authoritative artifacts");
    }
    let mut boundaries: Vec<ObservedSemanticBoundary> = read_json_bytes(
        &git_blob_at(root, &lineage.landed_sha, &artifact_path)?,
        "semantic-boundaries artifact",
    )?;
    boundaries.sort_by_key(semantic_boundary_key);
    if boundaries
        .windows(2)
        .any(|pair| semantic_boundary_key(&pair[0]) == semantic_boundary_key(&pair[1]))
    {
        bail!("semantic-boundaries artifact contains a duplicate boundary key");
    }
    Ok(())
}

fn validate_accepted_baseline(
    root: &Path,
    entry: &CurrentAuthorityEntry,
    lineage: &LandedLineage,
) -> Result<()> {
    let Some(path) = &entry.accepted_baseline_path else {
        if entry.accepted_baseline_digest.is_some()
            || entry.accepted_baseline_evidence_bundle.is_some()
        {
            bail!("accepted baseline metadata exists without an accepted baseline path");
        }
        return Ok(());
    };
    let bytes = git_blob_at(root, &lineage.landed_sha, path)?;
    let actual_digest = sha256_digest_bytes(&bytes);
    if entry.accepted_baseline_digest.as_deref() != Some(actual_digest.as_str())
        || lineage.accepted_baseline_digest.as_deref() != Some(actual_digest.as_str())
    {
        bail!("accepted baseline {} digest is not bound to lineage", path);
    }
    if lineage.authoritative_artifacts.get(path) != Some(&actual_digest) {
        bail!("accepted baseline {} is absent or mismatched in authoritative artifacts", path);
    }
    let baseline: CompileBaselineV2 = parse_compile_baseline_v2(
        serde_json::from_slice(&bytes)
            .with_context(|| format!("decoding accepted baseline envelope {path}"))?,
        path,
    )?;
    if baseline.series_id != entry.series_id
        || baseline.manifest_hash != entry.manifest_hash
        || baseline.accepted_transition_id != entry.accepted_transition_id
        || baseline.evidence_bundle.as_ref() != entry.accepted_baseline_evidence_bundle.as_ref()
    {
        bail!("accepted baseline {} disagrees with current authority", path);
    }
    Ok(())
}

fn validate_artifact_digests(
    root: &Path,
    index_path: &str,
    lineage_paths: &BTreeSet<String>,
    lineage_path: &str,
    lineage: &LandedLineage,
) -> Result<()> {
    for path in lineage.authoritative_artifacts.keys() {
        if path == index_path || path == lineage_path || lineage_paths.contains(path) {
            bail!("authority artifacts cannot self-reference lineage or index");
        }
        let expected = lineage.authoritative_artifacts.get(path).ok_or_else(|| {
            color_eyre::eyre::eyre!("missing digest for authoritative artifact {path}")
        })?;
        let actual = sha256_digest_bytes(&git_blob_at(root, &lineage.landed_sha, path)?);
        if actual != *expected {
            bail!("authoritative artifact {path} differs at landed SHA");
        }
    }
    Ok(())
}

fn validate_supersession_graph(lineages: &[(String, LandedLineage)]) -> Result<()> {
    let by_path =
        lineages.iter().map(|(path, lineage)| (path.as_str(), lineage)).collect::<BTreeMap<_, _>>();
    for (path, lineage) in &by_path {
        let Some(supersedes) = lineage.supersedes.as_deref() else {
            continue;
        };
        validate_public_path(supersedes, "superseded lineage path")?;
        if supersedes == *path {
            bail!("lineage {path} cannot supersede itself");
        }
        let superseded = by_path.get(supersedes).ok_or_else(|| {
            color_eyre::eyre::eyre!("lineage {path} supersedes missing lineage {supersedes}")
        })?;
        if superseded.series_id != lineage.series_id {
            bail!("lineage {path} supersedes a different series {}", superseded.series_id);
        }
    }
    for start in by_path.keys() {
        let mut seen = BTreeSet::new();
        let mut current = *start;
        while let Some(lineage) = by_path.get(current) {
            if !seen.insert(current) {
                bail!("supersession graph contains a cycle at lineage {current}");
            }
            let Some(next) = lineage.supersedes.as_deref() else {
                break;
            };
            current = next;
        }
    }
    Ok(())
}

fn validate_publication_scope(root: &Path, lineage: &LandedLineage) -> Result<()> {
    validate_git_commit(root, &lineage.publication_sha)?;
    validate_git_commit(root, &lineage.publication_base_sha)?;
    validate_git_ancestor(root, &lineage.publication_base_sha, &lineage.publication_sha)?;
    let output = Command::new("git")
        .arg("--no-replace-objects")
        .arg("-c")
        .arg("core.quotePath=false")
        .arg("-C")
        .arg(root)
        .args([
            "diff",
            "--no-renames",
            "--name-only",
            "-z",
            "--diff-filter=ACDMRTUXB",
            &lineage.publication_base_sha,
            &lineage.publication_sha,
            "--",
        ])
        .output()
        .context("computing publication commit diff")?;
    if !output.status.success() {
        bail!(
            "could not compute publication diff {}..{}: {}",
            lineage.publication_base_sha,
            lineage.publication_sha,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let mut actual = String::from_utf8(output.stdout)
        .context("publication diff contained invalid UTF-8")?
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(|path| path.replace('\\', "/"))
        .collect::<Vec<_>>();
    actual.sort();
    actual.dedup();
    let mut declared =
        lineage.publication_paths.iter().map(|path| path.replace('\\', "/")).collect::<Vec<_>>();
    declared.sort();
    declared.dedup();
    if actual != declared {
        bail!(
            "publication paths do not match Git diff: declared {:?}, actual {:?}",
            declared,
            actual
        );
    }
    Ok(())
}

fn validate_publication_paths(paths: &[String]) -> Result<()> {
    if paths.is_empty() {
        bail!("publication lineage has no changed paths");
    }
    for path in paths {
        validate_public_path(path, "publication path")?;
        let normalized = path.replace('\\', "/");
        let approved = normalized.starts_with(".ci/perl-core-harness/")
            || normalized.starts_with("evidence/")
            || normalized.starts_with("reports/")
            || normalized.starts_with("docs/project/status/")
            || normalized.starts_with("docs/project/compatibility/")
            || normalized.starts_with("plans/");
        if !approved {
            bail!("publication path {path} is outside the evidence-only allowlist");
        }
    }
    Ok(())
}

/// One nibble of a canonically serialized hexadecimal identity (#7725):
/// lower-case ASCII digits and `a`-`f` only, so every load-bearing
/// content-addressed receipt carries exactly one spelling per digest.
pub(crate) fn is_lower_case_hex_byte(byte: u8) -> bool {
    byte.is_ascii_digit() || matches!(byte, b'a'..=b'f')
}

/// A 64-character SHA-256 identity in its one canonical serialized form.
pub(crate) fn is_canonical_sha256_hex(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(is_lower_case_hex_byte)
}

fn validate_git_sha(value: &str, label: &str) -> Result<()> {
    if !(value.len() == 40 || value.len() == 64) || !value.bytes().all(is_lower_case_hex_byte) {
        bail!("{label} must be a 40- or 64-character hexadecimal SHA ([0-9a-f] lower-case)");
    }
    Ok(())
}

fn validate_git_commit(root: &Path, sha: &str) -> Result<()> {
    let object = format!("{sha}^{{commit}}");
    let output = Command::new("git")
        .arg("--no-replace-objects")
        .arg("-C")
        .arg(root)
        .args(["cat-file", "-e", &object])
        .output()
        .with_context(|| format!("checking landed commit {sha}"))?;
    if !output.status.success() {
        bail!("landed SHA {sha} is not a reachable commit in {}", root.display());
    }
    Ok(())
}

fn validate_git_ancestor(root: &Path, ancestor: &str, descendant: &str) -> Result<()> {
    let output = Command::new("git")
        .arg("--no-replace-objects")
        .arg("-C")
        .arg(root)
        .args(["merge-base", "--is-ancestor", ancestor, descendant])
        .output()
        .with_context(|| format!("checking Git ancestry {ancestor}..{descendant}"))?;
    if !output.status.success() {
        bail!("Git commit {ancestor} is not an ancestor of {descendant}");
    }
    Ok(())
}

fn git_blob_at(root: &Path, commit: &str, path: &str) -> Result<Vec<u8>> {
    validate_public_path(path, "Git artifact path")?;
    let object = format!("{commit}:{}", path.replace('\\', "/"));
    let output = Command::new("git")
        .arg("--no-replace-objects")
        .arg("-C")
        .arg(root)
        .args(["show", &object])
        .output()
        .with_context(|| format!("reading Git artifact {object}"))?;
    if !output.status.success() {
        bail!(
            "landed SHA {commit} does not contain Git artifact {path}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(output.stdout)
}

fn validate_digest(value: &str, label: &str) -> Result<()> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        bail!("{label} must use the sha256:<hex> format");
    };
    if !is_canonical_sha256_hex(hex) {
        bail!("{label} must contain 64 hexadecimal characters ([0-9a-f] lower-case)");
    }
    Ok(())
}

fn repository_relative_path(root: &Path, path: &Path) -> Result<String> {
    let raw_path = path.to_string_lossy().replace('\\', "/");
    if raw_path.split('/').any(|component| component == "..") {
        bail!("repository-relative path contains a traversal component");
    }

    let root = normalize_windows_extended_path(
        &fs::canonicalize(root)
            .map_err(|_| color_eyre::eyre::eyre!("repository root could not be resolved"))?,
    );
    let candidate_input = if path.is_absolute() { path.to_path_buf() } else { root.join(path) };
    if path_has_link_component(&candidate_input) {
        bail!("repository path contains a link or reparse point");
    }
    let candidate =
        normalize_windows_extended_path(&canonicalize_existing_prefix(&candidate_input)?);
    let relative = candidate
        .strip_prefix(&root)
        .map_err(|_| color_eyre::eyre::eyre!("repository path is outside the repository"))?;
    let relative = relative.to_string_lossy().replace('\\', "/");
    validate_public_path(&relative, "repository-relative path")?;
    Ok(relative)
}

fn path_has_link_component(path: &Path) -> bool {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        let Ok(metadata) = fs::symlink_metadata(&current) else {
            continue;
        };
        if metadata.file_type().is_symlink() {
            return true;
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;

            const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
            if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                return true;
            }
        }
    }
    false
}

fn canonicalize_existing_prefix(path: &Path) -> Result<PathBuf> {
    let mut existing = path.to_path_buf();
    let mut missing = Vec::new();
    while !existing.exists() {
        let Some(component) = existing.file_name() else {
            bail!("repository path could not be resolved");
        };
        missing.push(component.to_os_string());
        if !existing.pop() {
            bail!("repository path could not be resolved");
        }
    }

    let mut canonical = fs::canonicalize(&existing)
        .map_err(|_| color_eyre::eyre::eyre!("repository path could not be resolved"))?;
    for component in missing.iter().rev() {
        canonical.push(component);
    }
    Ok(canonical)
}

fn normalize_windows_extended_path(path: &Path) -> PathBuf {
    let value = path.to_string_lossy();
    if let Some(stripped) = value.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{stripped}"));
    }
    PathBuf::from(value.strip_prefix(r"\\?\").unwrap_or(&value))
}

/// Load one or more independently identified compatibility series from typed
/// harness receipts. This is the input contract for generated compatibility
/// views; it does not render or mutate any status document.
pub fn load_compatibility_state(
    config: CompatibilityLoadConfig,
) -> Result<CompilerCompatibilityState> {
    if config.inputs.is_empty() {
        bail!("compiler compatibility requires at least one series input");
    }
    if config.repository_commit.trim().is_empty() {
        bail!("compiler compatibility requires a repository commit");
    }
    let mut series = config
        .inputs
        .iter()
        .map(|input| load_compatibility_series(input, &config.repository_commit))
        .collect::<Result<Vec<_>>>()?;
    series.sort_by(|left, right| left.identity.series_id.cmp(&right.identity.series_id));
    for pair in series.windows(2) {
        if pair[0].identity.series_id == pair[1].identity.series_id {
            bail!(
                "compiler compatibility contains duplicate series {}",
                pair[0].identity.series_id
            );
        }
    }
    Ok(CompilerCompatibilityState {
        schema_version: COMPILER_COMPATIBILITY_SCHEMA_VERSION.into(),
        repository_commit: config.repository_commit,
        series,
    })
}

const PARSE_BASELINE_SCHEMA_VERSION: &str = "not_available";

fn load_compatibility_series(
    input: &CompatibilitySeriesInput,
    repository_commit: &str,
) -> Result<CompilerCompatibilitySeries> {
    let authority = input
        .current_authority
        .as_ref()
        .map(|config| validate_current_authority(config.clone()))
        .transpose()?;
    let series = read_series_manifest(&input.series_manifest)?;
    validate_series_manifest(&series)?;
    if series.repository_commit != repository_commit {
        bail!("series {} has a different repository subject", series.series_id);
    }
    let bundle = read_boundary_bundle(&input.evidence_bundle)?;
    if bundle.index.series_id != series.series_id
        || bundle.index.manifest_hash != series.manifest_hash
        || bundle.index.repository_commit != series.repository_commit
        || bundle.index.profile != series.profile
        || bundle.index.runner != series.runner
        || bundle.index.perl_resolved_ref != series.perl_resolved_ref
        || bundle.index.lineage.measurement_sha != series.repository_commit
    {
        bail!("evidence bundle identity does not match series {}", series.series_id);
    }
    let declared_compile = bundle_artifact_path(&bundle, "compile_report")?;
    if fs::canonicalize(&declared_compile).ok() != fs::canonicalize(&input.compile_report).ok() {
        bail!("compile report input is not the bundle-declared compile report");
    }
    let parse_report = read_run_report(&input.parse_report)?;
    let compile_report = read_run_report(&input.compile_report)?;
    validate_report_for_compatibility(&parse_report, &series, HarnessMode::Parse)?;
    validate_report_for_compatibility(&compile_report, &series, HarnessMode::Compile)?;
    let compile_membership = report_membership(&compile_report)?;
    if compile_membership != series.normalized_manifest.iter().cloned().collect() {
        bail!("compile report membership differs from series {}", series.series_id);
    }
    let compile_baseline = read_compile_baseline_v2(&input.compile_baseline)?;
    let baseline_comparison = compare_baseline_v2_with_identities(
        &compile_baseline,
        &compile_report,
        &series,
        Some(&V2Identities {
            compiler_subject_identity: series.compiler_subject_identity.clone(),
            invocation_identity: series.invocation_identity.clone(),
            capability_identity: series.capability_identity.clone(),
            environment_identity: series.environment_identity.clone(),
        }),
        None,
        &[],
    );
    if !baseline_comparison.violations.is_empty() {
        bail!(
            "compile baseline is not an authoritative subject for series {}:\n{}",
            series.series_id,
            baseline_comparison
                .violations
                .iter()
                .map(|violation| violation.message.clone())
                .collect::<Vec<_>>()
                .join("\n")
        );
    }
    let Some(accepted_path) = input.accepted_baseline.as_ref() else {
        bail!(
            "compatibility series {} must identify an accepted baseline separately from its current observation",
            series.series_id
        );
    };
    let accepted_baseline = read_compile_baseline_v2(accepted_path)?;
    validate_accepted_ratchet_identity(&accepted_baseline, &series)?;
    let (transition, transition_reason, requires_acceptance) =
        classify_compatibility_transition(&accepted_baseline, &compile_report);
    if let Some(index) = &authority {
        let current = index
            .entries
            .iter()
            .filter(|entry| {
                entry.series_id == series.series_id
                    && entry.status == CurrentAuthorityStatus::Current
            })
            .collect::<Vec<_>>();
        if current.len() != 1 {
            bail!(
                "current-authority admission for {} must contain exactly one current entry",
                series.series_id
            );
        }
        let entry = current[0];
        if entry.manifest_hash != series.manifest_hash
            || entry.observation_bundle_id != bundle.index.bundle_id
        {
            bail!("current-authority entry disagrees with series {}", series.series_id);
        }
        if entry.observation_transition != transition {
            bail!(
                "current-authority transition {:?} does not match measured transition {:?} for {}",
                entry.observation_transition,
                transition,
                series.series_id
            );
        }
        validate_authority_artifact_bindings(
            &input.evidence_bundle,
            accepted_path,
            &entry.observation_bundle_path,
            &entry.observation_bundle_digest,
            entry.accepted_baseline_path.as_deref(),
            entry.accepted_baseline_digest.as_deref(),
            input.current_authority.as_ref().map(|config| config.repository_root.as_path()),
        )?;
    }
    let mut parse_bundle = bundle.clone();
    parse_bundle.semantic_boundaries = parse_report.semantic_boundaries.clone();
    let parse_clusters = build_failure_cluster_report(&parse_bundle, &parse_report)?;
    let compile_clusters = build_failure_cluster_report(&bundle, &compile_report)?;

    let (history_rail, cluster_state) =
        load_cluster_history_state(input.cluster_history.as_deref(), &compile_clusters, &series)?;
    let registry_rail =
        load_registry_state(input.boundary_registry.as_deref(), &compile_baseline, &bundle)?;
    let debt = build_compatibility_debt_state(
        &compile_baseline,
        registry_rail.clone(),
        history_rail.clone(),
    );
    let execution = match &input.execute_report {
        Some(path) => load_execution_rail(path, &series, &bundle.index.bundle_id)?,
        None => unavailable_rail("selected execution receipt was not supplied"),
    };
    let observation = CompatibilityObservation {
        observation_bundle_id: bundle.index.bundle_id.clone(),
        measurement_sha: bundle.index.lineage.measurement_sha.clone(),
        parse: compatibility_run_state(
            &parse_report,
            PARSE_BASELINE_SCHEMA_VERSION,
            &bundle.index.bundle_id,
            parse_clusters.clusters.len(),
        ),
        compile: compatibility_run_state(
            &compile_report,
            &compile_baseline.schema_version,
            &bundle.index.bundle_id,
            compile_clusters.clusters.len(),
        ),
        debt: debt.clone(),
        clusters: cluster_state.clone(),
        execution: execution.clone(),
        curated_gold: unavailable_rail("curated semantic-gold receipt was not supplied"),
        differential_oracle: unavailable_rail("differential-oracle receipt was not supplied"),
        eir: unavailable_rail("EIR evaluation receipt was not supplied"),
        claim_boundary: "compile-harness and typed receipt state only; general semantics and runtime correctness are not implied".into(),
    };
    let accepted_ratchet = CompatibilityAcceptedRatchet {
        baseline_schema_version: accepted_baseline.schema_version.clone(),
        baseline_digest: sha256_di…87980 tokens truncated…perl_tree_requiring_t_perl_for_dumptests(root: &Path) -> TestResult<PathBuf> {
        let perl_tree = root.join("prepared-perl-requires-t-perl");
        let t_dir = perl_tree.join("t");
        fs::create_dir_all(t_dir.join("base"))?;
        fs::write(t_dir.join("base").join("ok.t"), "1;\n")?;
        let script = r#"#!/bin/sh
set -eu
if [ "${1:-}" = "--dumptests" ]; then
  if [ ! -f ./perl ]; then
    echo 'You need to run "make test_prep" first to set things up.' >&2
    exit 2
  fi
  echo "base/ok.t"
  exit 0
fi
./perl base/ok.t
"#;
        fs::write(t_dir.join("TEST"), script)?;
        Ok(perl_tree)
    }

    #[cfg(unix)]
    fn write_fake_perl_tree(root: &Path) -> TestResult<PathBuf> {
        write_fake_perl_tree_with_run_body(
            root,
            r#"./perl base/ok.t
"#,
        )
    }

    #[cfg(unix)]
    fn write_fake_perl_tree_with_two_base_tests(root: &Path) -> TestResult<PathBuf> {
        let perl_tree = root.join("prepared-perl-two-base-tests");
        let t_dir = perl_tree.join("t");
        fs::create_dir_all(t_dir.join("base"))?;
        fs::write(t_dir.join("base").join("ok.t"), "1;\n")?;
        fs::write(t_dir.join("base").join("lex.t"), "1;\n")?;
        let script = r#"#!/bin/sh
set -eu
if [ "${1:-}" = "--dumptests" ]; then
  echo "base/ok.t"
  echo "base/lex.t"
  exit 0
fi
./perl base/ok.t
./perl base/lex.t
"#;
        fs::write(t_dir.join("TEST"), script)?;
        Ok(perl_tree)
    }

    #[cfg(unix)]
    fn write_fake_perl_tree_with_base_if_test(root: &Path) -> TestResult<PathBuf> {
        write_fake_perl_tree_with_base_if_test_and_body(root, "./perl base/if.t\n")
    }

    #[cfg(unix)]
    fn write_fake_perl_tree_with_base_if_test_and_exit(
        root: &Path,
        status: i32,
    ) -> TestResult<PathBuf> {
        write_fake_perl_tree_with_base_if_test_and_body(
            root,
            &format!("./perl base/if.t\nexit {status}\n"),
        )
    }

    #[cfg(unix)]
    fn write_fake_perl_tree_with_base_if_test_and_body(
        root: &Path,
        run_body: &str,
    ) -> TestResult<PathBuf> {
        let perl_tree = root.join("prepared-perl-base-if");
        let t_dir = perl_tree.join("t");
        fs::create_dir_all(t_dir.join("base"))?;
        fs::write(t_dir.join("base").join("if.t"), "1;\n")?;
        let script = format!(
            r#"#!/bin/sh
set -eu
if [ "${{1:-}}" = "--dumptests" ]; then
  echo "base/if.t"
  exit 0
fi
{run_body}"#
        );
        fs::write(t_dir.join("TEST"), script)?;
        Ok(perl_tree)
    }

    #[cfg(unix)]
    fn write_fake_perl_tree_with_base_execute_subset(root: &Path) -> TestResult<PathBuf> {
        let perl_tree = root.join("prepared-perl-base-execute-subset");
        let t_dir = perl_tree.join("t");
        fs::create_dir_all(t_dir.join("base"))?;
        fs::write(t_dir.join("base").join("if.t"), "1;\n")?;
        fs::write(t_dir.join("base").join("cond.t"), "1;\n")?;
        fs::write(t_dir.join("base").join("num.t"), "1;\n")?;
        fs::write(t_dir.join("base").join("pat.t"), "1;\n")?;
        fs::write(t_dir.join("base").join("translate.t"), "1;\n")?;
        fs::write(t_dir.join("base").join("while.t"), "1;\n")?;
        let script = r#"#!/bin/sh
set -eu
if [ "${1:-}" = "--dumptests" ]; then
  echo "base/cond.t"
  echo "base/if.t"
  echo "base/num.t"
  echo "base/pat.t"
  echo "base/translate.t"
  echo "base/while.t"
  exit 0
fi
./perl base/cond.t
./perl base/if.t
./perl base/num.t
./perl base/pat.t
./perl base/translate.t
./perl base/while.t
"#;
        fs::write(t_dir.join("TEST"), script)?;
        Ok(perl_tree)
    }

    #[cfg(unix)]
    fn write_fake_perl_tree_with_two_comp_tests(root: &Path) -> TestResult<PathBuf> {
        let perl_tree = root.join("prepared-perl-two-comp-tests");
        let t_dir = perl_tree.join("t");
        fs::create_dir_all(t_dir.join("comp"))?;
        fs::write(t_dir.join("comp").join("require.t"), "1;\n")?;
        fs::write(t_dir.join("comp").join("use.t"), "1;\n")?;
        let script = r#"#!/bin/sh
set -eu
if [ "${1:-}" = "--dumptests" ]; then
  echo "comp/require.t"
  echo "comp/use.t"
  exit 0
fi
./perl comp/require.t
./perl comp/use.t
"#;
        fs::write(t_dir.join("TEST"), script)?;
        Ok(perl_tree)
    }

    #[cfg(unix)]
    fn write_fake_perl_tree_with_two_run_tests(root: &Path) -> TestResult<PathBuf> {
        let perl_tree = root.join("prepared-perl-two-run-tests");
        let t_dir = perl_tree.join("t");
        fs::create_dir_all(t_dir.join("run"))?;
        fs::write(t_dir.join("run").join("import.t"), "1;\n")?;
        fs::write(t_dir.join("run").join("switches.t"), "1;\n")?;
        let script = r#"#!/bin/sh
set -eu
if [ "${1:-}" = "--dumptests" ]; then
  echo "run/import.t"
  echo "run/switches.t"
  exit 0
fi
./perl run/import.t
./perl run/switches.t
"#;
        fs::write(t_dir.join("TEST"), script)?;
        Ok(perl_tree)
    }

    #[cfg(unix)]
    fn write_fake_perl_tree_with_run_body(root: &Path, run_body: &str) -> TestResult<PathBuf> {
        let perl_tree = root.join("prepared-perl");
        let t_dir = perl_tree.join("t");
        fs::create_dir_all(t_dir.join("base"))?;
        fs::write(t_dir.join("base").join("ok.t"), "1;\n")?;
        let stale_context_dir = perl_tree.join("target");
        fs::create_dir_all(&stale_context_dir)?;
        fs::write(
            stale_context_dir.join("perl-lsp-runner-records.jsonl"),
            r#"{"schema_version":"perl_core_harness.runner_record.v1","mode":"parse","path":"stale.t","status":"fail","assertions_passed":0,"assertions_total":1,"bucket":"parse_recovery","first_diagnostic":"stale"}"#,
        )?;
        let script = format!(
            r#"#!/bin/sh
set -eu
if [ "${{1:-}}" = "--dumptests" ]; then
  echo "base/ok.t"
  exit 0
fi
{run_body}"#
        );
        fs::write(t_dir.join("TEST"), script)?;
        Ok(perl_tree)
    }

    #[cfg(unix)]
    fn write_fake_runner(root: &Path, status: RunnerStatus) -> TestResult<PathBuf> {
        write_fake_runner_with_bucket(root, status, Some("parse_recovery"))
    }

    #[cfg(unix)]
    fn write_fake_execute_runner(root: &Path) -> TestResult<PathBuf> {
        let runner = root.join("fake-runner-execute-pass.sh");
        let body = r#"#!/bin/sh
set -eu
script="${1:-unknown.t}"
mode="${PERL_LSP_HARNESS_MODE:-execute}"
mkdir -p "$(dirname "$PERL_LSP_HARNESS_CONTEXT")"
case "$script" in
  *base/cond.t)
    printf '1..4\n'
    printf 'ok 1 - operator eq\n'
    printf 'ok 2 - operator ne\n'
    printf 'ok 3 - operator ==\n'
    printf 'ok 4 - operator !=\n'
    printf '{"schema_version":"perl_core_harness.runner_record.v1","mode":"%s","path":"%s","status":"pass","assertions_passed":4,"assertions_total":4,"bucket":null,"first_diagnostic":null}\n' "$mode" "$script" >> "$PERL_LSP_HARNESS_CONTEXT"
    ;;
  *base/while.t)
    printf '1..4\n'
    printf 'ok 1\n'
    printf 'ok 2\n'
    printf 'ok 3\n'
    printf 'ok 4\n'
    printf '{"schema_version":"perl_core_harness.runner_record.v1","mode":"%s","path":"%s","status":"pass","assertions_passed":4,"assertions_total":4,"bucket":null,"first_diagnostic":null}\n' "$mode" "$script" >> "$PERL_LSP_HARNESS_CONTEXT"
    ;;
  *base/num.t)
    printf '1..56\n'
    i=1
    while [ "$i" -le 56 ]; do
      printf 'ok %s\n' "$i"
      i=$((i + 1))
    done
    printf '{"schema_version":"perl_core_harness.runner_record.v1","mode":"%s","path":"%s","status":"pass","assertions_passed":56,"assertions_total":56,"bucket":null,"first_diagnostic":null}\n' "$mode" "$script" >> "$PERL_LSP_HARNESS_CONTEXT"
    ;;
  *base/pat.t)
    printf '1..2\n'
    printf 'ok 1 - match regex\n'
    printf 'ok 2 - match regex\n'
    printf '{"schema_version":"perl_core_harness.runner_record.v1","mode":"%s","path":"%s","status":"pass","assertions_passed":2,"assertions_total":2,"bucket":null,"first_diagnostic":null}\n' "$mode" "$script" >> "$PERL_LSP_HARNESS_CONTEXT"
    ;;
  *base/translate.t)
    printf '1..257\n'
    i=0
    assertion=1
    while [ "$i" -le 255 ]; do
      printf 'ok %s - native_to_unicode %s\n' "$assertion" "$i"
      i=$((i + 1))
      assertion=$((assertion + 1))
    done
    printf 'ok 257 - native_to_unicode of large number\n'
    printf '{"schema_version":"perl_core_harness.runner_record.v1","mode":"%s","path":"%s","status":"pass","assertions_passed":257,"assertions_total":257,"bucket":null,"first_diagnostic":null}\n' "$mode" "$script" >> "$PERL_LSP_HARNESS_CONTEXT"
    ;;
  *)
    printf '1..2\n'
    printf 'ok 1 - if eq\n'
    printf 'ok 2 - if ne\n'
    printf '{"schema_version":"perl_core_harness.runner_record.v1","mode":"%s","path":"%s","status":"pass","assertions_passed":2,"assertions_total":2,"bucket":null,"first_diagnostic":null}\n' "$mode" "$script" >> "$PERL_LSP_HARNESS_CONTEXT"
    ;;
esac
"#;
        fs::write(&runner, body)?;
        set_executable(&runner)?;
        Ok(runner)
    }

    #[cfg(unix)]
    fn write_fake_runner_with_bucket(
        root: &Path,
        status: RunnerStatus,
        bucket: Option<&str>,
    ) -> TestResult<PathBuf> {
        let runner = match status {
            RunnerStatus::Pass => root.join("fake-runner-pass.sh"),
            RunnerStatus::Fail => root.join("fake-runner-fail.sh"),
        };
        let body = match status {
            RunnerStatus::Pass => {
                r#"#!/bin/sh
set -eu
script="${1:-unknown.t}"
mode="${PERL_LSP_HARNESS_MODE:-parse}"
mkdir -p "$(dirname "$PERL_LSP_HARNESS_CONTEXT")"
printf '1..1\n'
printf 'ok 1 - %s %s\n' "$mode" "$script"
printf '{"schema_version":"perl_core_harness.runner_record.v1","mode":"%s","path":"%s","status":"pass","assertions_passed":1,"assertions_total":1,"bucket":null,"first_diagnostic":null}\n' "$mode" "$script" >> "$PERL_LSP_HARNESS_CONTEXT"
"#
                .to_string()
            }
            RunnerStatus::Fail => {
                let bucket_comment = bucket.unwrap_or("unknown");
                let bucket_json = match bucket {
                    Some(bucket) => format!(r#""{bucket}""#),
                    None => "null".to_string(),
                };
                format!(
                    r#"#!/bin/sh
set -eu
script="${{1:-unknown.t}}"
mode="${{PERL_LSP_HARNESS_MODE:-parse}}"
mkdir -p "$(dirname "$PERL_LSP_HARNESS_CONTEXT")"
printf '1..1\n'
printf 'not ok 1 - %s %s\n' "$mode" "$script"
printf '# bucket: {bucket_comment}\n'
printf '# first diagnostic: expected expression\n'
printf '{{"schema_version":"perl_core_harness.runner_record.v1","mode":"%s","path":"%s","status":"fail","assertions_passed":0,"assertions_total":1,"bucket":{bucket_json},"first_diagnostic":"expected expression"}}\n' "$mode" "$script" >> "$PERL_LSP_HARNESS_CONTEXT"
exit 1
"#
                )
            }
        };
        fs::write(&runner, body)?;
        set_executable(&runner)?;
        Ok(runner)
    }
    fn current_authority_fixture() -> TestResult<(tempfile::TempDir, CurrentAuthorityConfig)> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().to_path_buf();
        fs::create_dir_all(root.join(".ci/perl-core-harness"))?;
        let bundle_relative = ".ci/perl-core-harness/base-bundle.json";
        let boundary_relative = ".ci/perl-core-harness/base-boundaries.json";
        let report_relative = ".ci/perl-core-harness/base-report.json";
        let baseline_relative = ".ci/perl-core-harness/base-baseline.json";
        let lineage_relative = ".ci/perl-core-harness/base-lineage.json";
        let index_relative = ".ci/perl-core-harness/current-authority.json";
        let measurement_sha = "a".repeat(40);
        fs::write(root.join(boundary_relative), b"[]\n")?;
        fs::write(
            root.join(bundle_relative),
            serde_json::to_vec_pretty(&EvidenceBundleIndex {
                schema_version: "perl_core_harness.evidence_bundle.v1".into(),
                bundle_id: "bundle-base".into(),
                series_id: "selected-base-perl-5.42.2".into(),
                manifest_hash: "manifest-base".into(),
                repository_commit: "repo-base".into(),
                profile: HarnessProfile::Base,
                runner: HarnessRunner::Test,
                perl_resolved_ref: "perl-base".into(),
                lineage: EvidenceBundleLineage {
                    measurement_sha: measurement_sha.clone(),
                    publication_sha: None,
                    landed_sha: None,
                },
                artifacts: vec![EvidenceBundleArtifact {
                    kind: "semantic_boundaries".into(),
                    logical_path: "base-boundaries.json".into(),
                }],
                completeness: EvidenceBundleCompleteness {
                    status: "complete".into(),
                    normalized_authority: true,
                },
                lifecycle: "published".into(),
            })?,
        )?;
        let bundle_digest = sha256_digest_bytes(&fs::read(root.join(bundle_relative))?);
        fs::write(root.join(report_relative), b"normalized report\n")?;
        fs::write(
            root.join(baseline_relative),
            serde_json::to_vec_pretty(&serde_json::json!({
                "schema_version": "perl_core_harness.compile_baseline.v2",
                "report_schema_version": RUN_REPORT_SCHEMA_VERSION,
                "series_id": "selected-base-perl-5.42.2",
                "manifest_hash": "manifest-base",
                "repository_commit": "repo-base",
                "perl_resolved_ref": "perl-base",
                "preparation_receipt_id": "prepare-base",
                "compiler_subject_identity": "compiler-base",
                "invocation_identity": "invocation-base",
                "capability_identity": "capability-base",
                "environment_identity": "environment-base",
                "source_report_digest": sha256_digest_bytes(&fs::read(root.join(report_relative))?),
                "accepted_transition_id": null,
                "evidence_bundle": "accepted-bundle-base",
                "mode": "compile",
                "profile": "base",
                "runner": "test",
                "file_membership": [],
                "files_total": 0,
                "files_passed": 0,
                "files_failed": 0,
                "tap_assertions_total": 0,
                "tap_assertions_passed": 0,
                "buckets": {},
                "expected_failures": [],
                "file_results": [],
                "semantic_boundaries": [],
                "boundary_retirements": []
            }))?,
        )?;
        let run_git = |args: &[&str]| -> TestResult<String> {
            let output = Command::new("git").arg("-C").arg(&root).args(args).output()?;
            if !output.status.success() {
                bail!(
                    "fixture git command {:?} failed: {}",
                    args,
                    String::from_utf8_lossy(&output.stderr).trim()
                );
            }
            Ok(String::from_utf8(output.stdout)?.trim().to_string())
        };
        run_git(&["init", "--quiet"])?;
        run_git(&["config", "user.email", "compiler-harness@example.invalid"])?;
        run_git(&["config", "user.name", "Compiler Harness Fixture"])?;
        fs::write(root.join("fixture-base.txt"), b"base\n")?;
        run_git(&["add", "fixture-base.txt"])?;
        run_git(&["commit", "--quiet", "-m", "fixture base"])?;
        let base_sha = run_git(&["rev-parse", "HEAD"])?;
        run_git(&["add", ".ci/perl-core-harness"])?;
        run_git(&["commit", "--quiet", "-m", "fixture evidence"])?;
        let landed_sha = run_git(&["rev-parse", "HEAD"])?;
        let mut authoritative_artifacts = BTreeMap::new();
        for path in [bundle_relative, boundary_relative, report_relative, baseline_relative] {
            authoritative_artifacts
                .insert(path.to_string(), sha256_digest_bytes(&fs::read(root.join(path))?));
        }
        let lineage = LandedLineage {
            schema_version: LANDED_LINEAGE_SCHEMA_VERSION.into(),
            series_id: "selected-base-perl-5.42.2".into(),
            profile: HarnessProfile::Base,
            manifest_hash: "manifest-base".into(),
            evidence_bundle_id: "bundle-base".into(),
            evidence_bundle_digest: bundle_digest,
            measurement_sha,
            publication_sha: landed_sha.clone(),
            landed_sha: landed_sha.clone(),
            publication_base_sha: base_sha,
            authoritative_artifacts,
            publication_paths: vec![
                bundle_relative.into(),
                boundary_relative.into(),
                report_relative.into(),
                baseline_relative.into(),
            ],
            accepted_transition_id: None,
            accepted_baseline_digest: Some(sha256_digest_bytes(&fs::read(
                root.join(baseline_relative),
            )?)),
            accepted_baseline_evidence_bundle: Some("accepted-bundle-base".into()),
            observation_transition: CompatibilityTransition::NoChange,
            recorder_schema_version: LANDED_LINEAGE_SCHEMA_VERSION.into(),
            created_reason: "post-merge lineage binding".into(),
            supersedes: None,
        };
        fs::write(root.join(lineage_relative), serde_json::to_vec_pretty(&lineage)?)?;
        let index = CurrentAuthorityIndex {
            schema_version: CURRENT_AUTHORITY_INDEX_SCHEMA_VERSION.into(),
            entries: vec![CurrentAuthorityEntry {
                series_id: lineage.series_id.clone(),
                profile: lineage.profile,
                manifest_hash: lineage.manifest_hash.clone(),
                observation_bundle_path: bundle_relative.into(),
                observation_bundle_id: lineage.evidence_bundle_id.clone(),
                observation_bundle_digest: lineage.evidence_bundle_digest.clone(),
                observation_transition: lineage.observation_transition,
                accepted_baseline_path: Some(baseline_relative.into()),
                accepted_baseline_digest: lineage.accepted_baseline_digest.clone(),
                accepted_baseline_evidence_bundle: lineage
                    .accepted_baseline_evidence_bundle
                    .clone(),
                accepted_transition_id: None,
                landed_lineage_path: lineage_relative.into(),
                status: CurrentAuthorityStatus::Current,
                claim_boundary: "parse_compile_acceptance".into(),
                unavailable_rails: vec!["execution".into(), "curated_gold".into()],
            }],
        };
        fs::write(root.join(index_relative), serde_json::to_vec_pretty(&index)?)?;
        run_git(&["add", ".ci/perl-core-harness"])?;
        run_git(&["commit", "--quiet", "-m", "fixture authority records"])?;
        let authority_sha = run_git(&["rev-parse", "HEAD"])?;
        Ok((
            temp,
            CurrentAuthorityConfig {
                index: root.join(index_relative),
                lineages: vec![root.join(lineage_relative)],
                repository_root: root.to_path_buf(),
                landed_sha: authority_sha,
            },
        ))
    }

    fn commit_fixture_authority(root: &Path) -> TestResult<String> {
        let run_git = |args: &[&str]| -> TestResult<String> {
            let output = Command::new("git").arg("-C").arg(root).args(args).output()?;
            if !output.status.success() {
                bail!(
                    "fixture git command {:?} failed: {}",
                    args,
                    String::from_utf8_lossy(&output.stderr).trim()
                );
            }
            Ok(String::from_utf8(output.stdout)?.trim().to_string())
        };
        run_git(&["add", ".ci/perl-core-harness"])?;
        run_git(&["commit", "--quiet", "-m", "fixture authority update"])?;
        run_git(&["rev-parse", "HEAD"])
    }

    #[test]
    fn current_authority_validates_landed_lineage_and_artifact_digests() -> TestResult {
        let (_temp, config) = current_authority_fixture()?;
        validate_current_authority(config)?;
        Ok(())
    }

    #[test]
    fn current_authority_rejects_tampered_artifact_digest() -> TestResult {
        let (_temp, mut config) = current_authority_fixture()?;
        let lineage_path = config
            .lineages
            .first()
            .cloned()
            .ok_or_else(|| color_eyre::eyre::eyre!("fixture has no lineage"))?;
        let mut lineage: LandedLineage =
            read_json_bytes(&fs::read(&lineage_path)?, "landed lineage")?;
        let artifact = lineage
            .authoritative_artifacts
            .get_mut(".ci/perl-core-harness/base-report.json")
            .ok_or_else(|| color_eyre::eyre::eyre!("fixture has no report digest"))?;
        *artifact = format!("sha256:{}", "c".repeat(64));
        fs::write(&lineage_path, serde_json::to_vec_pretty(&lineage)?)?;
        config.landed_sha = commit_fixture_authority(&config.repository_root)?;
        let error = validate_current_authority(config)
            .err()
            .ok_or_else(|| color_eyre::eyre::eyre!("tampered artifact digest was accepted"))?;
        if !error.to_string().contains("differs at landed SHA") {
            bail!("tampered artifact digest produced an unclear error: {error}");
        }
        Ok(())
    }

    #[test]
    fn current_authority_rejects_duplicate_current_series() -> TestResult {
        let (_temp, mut config) = current_authority_fixture()?;
        let index: CurrentAuthorityIndex =
            read_json_bytes(&fs::read(&config.index)?, "current-authority index")?;
        let duplicate = index
            .entries
            .first()
            .cloned()
            .ok_or_else(|| color_eyre::eyre::eyre!("fixture has no current entry"))?;
        let mut changed = index;
        changed.entries.push(duplicate);
        fs::write(&config.index, serde_json::to_vec_pretty(&changed)?)?;
        config.landed_sha = commit_fixture_authority(&config.repository_root)?;
        let error = validate_current_authority(config)
            .err()
            .ok_or_else(|| color_eyre::eyre::eyre!("duplicate current series was accepted"))?;
        if !error.to_string().contains("duplicate current authority") {
            bail!("duplicate current series produced an unclear error: {error}");
        }
        Ok(())
    }

    #[test]
    fn current_authority_rejects_wrong_landed_sha() -> TestResult {
        let (_temp, mut config) = current_authority_fixture()?;
        config.landed_sha = "e".repeat(40);
        let error = validate_current_authority(config)
            .err()
            .ok_or_else(|| color_eyre::eyre::eyre!("wrong landed SHA was accepted"))?;
        if !error.to_string().contains("landed SHA") {
            bail!("wrong landed SHA produced an unclear error: {error}");
        }
        Ok(())
    }

    #[test]
    fn current_authority_uses_landed_git_blobs_not_worktree() -> TestResult {
        let (_temp, config) = current_authority_fixture()?;
        fs::write(
            config.repository_root.join(".ci/perl-core-harness/base-report.json"),
            b"changed report\n",
        )?;
        fs::write(
            config.repository_root.join(".ci/perl-core-harness/current-authority.json"),
            b"{}\n",
        )?;
        fs::write(config.repository_root.join(".ci/perl-core-harness/base-lineage.json"), b"{}\n")?;
        validate_current_authority(config)?;
        Ok(())
    }

    #[test]
    fn current_authority_allows_historical_observations_for_one_series() -> TestResult {
        let (_temp, mut config) = current_authority_fixture()?;
        let index: CurrentAuthorityIndex =
            read_json_bytes(&fs::read(&config.index)?, "current-authority index")?;
        let current = index
            .entries
            .first()
            .cloned()
            .ok_or_else(|| color_eyre::eyre::eyre!("fixture has no current entry"))?;
        let current_lineage_path = config
            .lineages
            .first()
            .cloned()
            .ok_or_else(|| color_eyre::eyre::eyre!("fixture has no lineage"))?;
        let mut historical_lineage: LandedLineage =
            read_json_bytes(&fs::read(&current_lineage_path)?, "landed lineage")?;
        historical_lineage.observation_transition = CompatibilityTransition::Historical;
        historical_lineage.supersedes = Some(".ci/perl-core-harness/base-lineage.json".into());
        let historical_lineage_path =
            config.repository_root.join(".ci/perl-core-harness/base-historical-lineage.json");
        fs::write(&historical_lineage_path, serde_json::to_vec_pretty(&historical_lineage)?)?;
        let mut historical = current.clone();
        historical.observation_transition = CompatibilityTransition::Historical;
        historical.landed_lineage_path =
            ".ci/perl-core-harness/base-historical-lineage.json".into();
        historical.status = CurrentAuthorityStatus::Historical;
        let index = CurrentAuthorityIndex {
            schema_version: index.schema_version,
            entries: vec![current, historical],
        };
        fs::write(&config.index, serde_json::to_vec_pretty(&index)?)?;
        config.lineages.push(historical_lineage_path);
        let run_git = |args: &[&str]| -> TestResult<String> {
            let output =
                Command::new("git").arg("-C").arg(&config.repository_root).args(args).output()?;
            if !output.status.success() {
                bail!(
                    "fixture git command {:?} failed: {}",
                    args,
                    String::from_utf8_lossy(&output.stderr).trim()
                );
            }
            Ok(String::from_utf8(output.stdout)?.trim().to_string())
        };
        run_git(&["add", ".ci/perl-core-harness"])?;
        run_git(&["commit", "--quiet", "-m", "fixture historical authority"])?;
        config.landed_sha = run_git(&["rev-parse", "HEAD"])?;
        validate_current_authority(config)?;
        Ok(())
    }

    #[test]
    fn supersession_graph_rejects_cycles() -> TestResult {
        let (_temp, config) = current_authority_fixture()?;
        let current_path = config
            .lineages
            .first()
            .ok_or_else(|| color_eyre::eyre::eyre!("fixture has no lineage"))?;
        let current: LandedLineage = read_json_bytes(&fs::read(current_path)?, "landed lineage")?;
        let mut first = current.clone();
        first.supersedes = Some("b".into());
        let mut second = current;
        second.supersedes = Some("a".into());
        let error = validate_supersession_graph(&[("a".into(), first), ("b".into(), second)])
            .err()
            .ok_or_else(|| color_eyre::eyre::eyre!("supersession cycle was accepted"))?;
        if !error.to_string().contains("cycle") {
            bail!("supersession cycle produced an unclear error: {error}");
        }
        Ok(())
    }

    #[test]
    fn current_authority_rejects_measured_code_publication_path() -> TestResult {
        let error = validate_publication_paths(&["crates/perl-parser/src/lib.rs".into()])
            .err()
            .ok_or_else(|| color_eyre::eyre::eyre!("measured code path was accepted"))?;
        if !error.to_string().contains("evidence-only allowlist") {
            bail!("measured code path produced an unclear error: {error}");
        }
        Ok(())
    }

    fn write_json_receipt<T: serde::Serialize>(path: &Path, value: &T) -> TestResult {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(value)?;
        fs::write(path, format!("{json}\n"))?;
        Ok(())
    }

    /// Materialize a published evidence bundle on disk whose compile receipt and
    /// semantic-boundary artifact carry the bundle's subject identity, so
    /// [`triage`] reads the same authority the CLI does.
    fn write_triage_bundle(root: &Path, report: &RunReport) -> TestResult<PathBuf> {
        let bundle_dir = root.join("bundles").join("bundle-1");
        let mut index = sample_boundary_bundle().index;
        index.artifacts = vec![
            EvidenceBundleArtifact {
                kind: "semantic_boundaries".into(),
                logical_path: "semantic-boundaries.json".into(),
            },
            EvidenceBundleArtifact {
                kind: "compile_report".into(),
                logical_path: "compile-report.json".into(),
            },
        ];
        let index_path = bundle_dir.join("index.json");
        write_json_receipt(&index_path, &index)?;
        write_json_receipt(
            &bundle_dir.join("semantic-boundaries.json"),
            &report.semantic_boundaries,
        )?;
        write_json_receipt(&bundle_dir.join("compile-report.json"), report)?;
        Ok(index_path)
    }

    /// A structurally valid compile receipt in which both manifest files fail
    /// with the same bucket, so triage must produce exactly one cluster.
    fn two_file_parse_failure_report() -> RunReport {
        let mut report = sample_compile_report();
        mark_file_failed(&mut report, "base/lex.t", "parse_recovery");
        mark_file_failed(&mut report, "base/ok.t", "parse_recovery");
        report.buckets.insert("parse_recovery".into(), 2);
        report
    }

    fn triage_config(bundle: PathBuf, output: PathBuf) -> TriageConfig {
        TriageConfig { bundle, output, history: None, write_history: false, check_history: false }
    }

    #[test]
    fn triage_writes_cluster_and_history_receipts_for_a_fresh_bundle() -> TestResult {
        let temp = tempfile::tempdir()?;
        let report = two_file_parse_failure_report();
        let bundle = write_triage_bundle(temp.path(), &report)?;
        let output = temp.path().join("triage");
        let history = temp.path().join("history").join("cluster-history.json");

        triage(TriageConfig {
            history: Some(history.clone()),
            write_history: true,
            ..triage_config(bundle, output.clone())
        })?;

        let clusters: FailureClusterReport =
            serde_json::from_str(&fs::read_to_string(output.join("failure-clusters.json"))?)?;
        assert_eq!(clusters.clusters.len(), 1);
        let cluster = &clusters.clusters[0];
        assert_eq!(cluster.occurrence_count, 2);
        assert_eq!(cluster.affected_files, vec!["base/lex.t", "base/ok.t"]);

        // The Markdown receipt must carry the same identity as the JSON receipt,
        // not a re-derived or truncated summary.
        let cluster_markdown = fs::read_to_string(output.join("failure-clusters.md"))?;
        assert!(cluster_markdown.starts_with("# Compiler failure clusters\n"));
        assert!(cluster_markdown.contains("- Bundle: `bundle-1`\n"));
        assert!(cluster_markdown.contains(&format!("### `{}`\n", cluster.cluster_id)));
        assert!(cluster_markdown.contains(&format!("- Bucket: `{}`\n", cluster.signature.bucket)));
        assert!(cluster_markdown.contains("- Occurrences: 2\n"));
        assert!(cluster_markdown.contains("- Files: base/lex.t, base/ok.t\n"));
        assert!(cluster_markdown.contains("## Semantic-boundary debt candidates\n"));

        let persisted: FailureClusterHistory =
            serde_json::from_str(&fs::read_to_string(&history)?)?;
        assert_eq!(persisted.entries.len(), 1);
        assert_eq!(persisted.entries[0].cluster_id, cluster.cluster_id);
        assert_eq!(persisted.entries[0].occurrence_count, 2);

        // The history mirror inside the triage output must match the durable file.
        let mirrored = fs::read_to_string(output.join("cluster-history.json"))?;
        assert_eq!(mirrored, fs::read_to_string(&history)?);

        let history_markdown = fs::read_to_string(output.join("cluster-history.md"))?;
        assert!(history_markdown.starts_with("# Compiler failure cluster history\n"));
        assert!(history_markdown.contains("- Entries: 1\n"));
        assert!(history_markdown.contains("## Status counts\n\n- unassigned: 1\n"));
        assert!(history_markdown.contains(&format!(
            "- {}: 2 occurrence(s), status unassigned, owner unassigned\n",
            cluster.cluster_id
        )));
        Ok(())
    }

    #[test]
    fn triage_reports_a_clean_compile_receipt_without_inventing_clusters() -> TestResult {
        let temp = tempfile::tempdir()?;
        let report = sample_compile_report();
        let bundle = write_triage_bundle(temp.path(), &report)?;
        let output = temp.path().join("triage");

        triage(triage_config(bundle, output.clone()))?;

        let clusters: FailureClusterReport =
            serde_json::from_str(&fs::read_to_string(output.join("failure-clusters.json"))?)?;
        assert!(clusters.clusters.is_empty());
        let markdown = fs::read_to_string(output.join("failure-clusters.md"))?;
        assert!(markdown.contains("No product failure clusters were observed.\n"));
        // Without --write-history / --check-history no history receipt is produced.
        assert!(!output.join("cluster-history.json").exists());
        assert!(!output.join("cluster-history.md").exists());
        Ok(())
    }

    fn sample_history_entry(cluster_id: &str) -> FailureClusterHistoryEntry {
        FailureClusterHistoryEntry {
            cluster_id: cluster_id.to_string(),
            signature_schema_version: FAILURE_CLUSTER_SCHEMA_VERSION.into(),
            identity_quality: FailureClusterIdentityQuality::Provisional,
            series_id: "series-1".into(),
            manifest_hash: "manifest-1".into(),
            first_seen_series_id: "series-1".into(),
            first_seen_manifest_hash: "manifest-1".into(),
            last_seen_series_id: "series-1".into(),
            last_seen_manifest_hash: "manifest-1".into(),
            first_seen_bundle: "bundle-1".into(),
            last_seen_bundle: "bundle-1".into(),
            current_affected_files: vec!["base/ok.t".into()],
            historical_affected_files: vec!["base/ok.t".into()],
            current_fact_classes: vec!["parse_recovery".into()],
            fact_classes: vec!["parse_recovery".into()],
            current_lsp_surfaces: vec!["diagnostics".into()],
            lsp_surfaces: vec!["diagnostics".into()],
            occurrence_count: 1,
            current_stage: Some("compile".into()),
            current_authority_bundle: Some("bundle-1".into()),
            observed_in_current_bundle: true,
            absence_since_bundle: None,
            presence: FailureClusterHistoryPresence::Observed,
            impacted_layer: "parser".into(),
            owner_issue: None,
            status: FailureClusterHistoryStatus::Unassigned,
            direct_reproduction: "bundle=bundle-1 series=series-1".into(),
            proposed_transition: "compiler_semantics".into(),
            stop_condition: "cluster no longer reproduces".into(),
            accepted_debt_refs: Vec::new(),
            resolution_pr: None,
            resolution_bundle: None,
            transitions: Vec::new(),
        }
    }

    #[test]
    fn triage_history_ordering_is_stable_and_bounded_to_ten_entries() -> TestResult {
        // `render_cluster_history_markdown` promotes the highest-occurrence
        // clusters, breaking ties by cluster id, and lists at most ten. Build a
        // history whose insertion order contradicts both rules.
        let mut entries = Vec::new();
        for index in 0..12 {
            let mut entry = sample_history_entry(&format!("failure-{index:02}"));
            entry.occurrence_count = index + 1;
            entries.push(entry);
        }
        let mut tied = sample_history_entry("failure-00-tied");
        tied.occurrence_count = 12;
        entries.insert(0, tied);
        let history = FailureClusterHistory {
            schema_version: FAILURE_CLUSTER_HISTORY_SCHEMA_VERSION.into(),
            entries,
        };

        let markdown = render_cluster_history_markdown(&history);
        let leverage = markdown
            .split("## High leverage\n\n")
            .nth(1)
            .ok_or_else(|| color_eyre::eyre::eyre!("missing high-leverage section"))?
            .split("\n## Clusters")
            .next()
            .unwrap_or_default()
            .lines()
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>();

        assert_eq!(leverage.len(), 10);
        // 12 occurrences is a tie between `failure-00-tied` and `failure-11`;
        // the lexicographically smaller cluster id wins.
        assert!(leverage[0].starts_with("- failure-00-tied: 12 occurrence(s)"));
        assert!(leverage[1].starts_with("- failure-11: 12 occurrence(s)"));
        assert!(leverage[2].starts_with("- failure-10: 11 occurrence(s)"));
        // The bounded section must not leak the low-occurrence tail.
        assert!(!leverage.iter().any(|line| line.starts_with("- failure-00:")));
        // The full cluster section still records every entry.
        assert!(markdown.contains("- Entries: 13\n"));
        assert!(markdown.contains("### failure-00\n"));
        Ok(())
    }

    #[test]
    fn cluster_history_markdown_renders_absent_transitions_and_missing_owners() -> TestResult {
        let mut entry = sample_history_entry("failure-absent");
        entry.owner_issue = None;
        entry.current_stage = None;
        entry.transitions.push(FailureClusterHistoryTransition {
            transition_id: "transition-1".into(),
            from_cluster_id: "failure-absent".into(),
            to_cluster_id: None,
            to_presence: FailureClusterHistoryPresence::Resolved,
            from_stage: "compile_effect".into(),
            to_stage: "absent".into(),
            before_series_id: "series-1".into(),
            before_manifest_hash: "manifest-1".into(),
            before_bundle_id: "bundle-1".into(),
            after_series_id: "series-2".into(),
            after_manifest_hash: "manifest-2".into(),
            after_bundle_id: "bundle-2".into(),
            proof_plan: "replace symbolic reference lowering".into(),
            stop_condition: "cluster no longer reproduces".into(),
            implementation_pr: Some("#5300".into()),
        });
        let history = FailureClusterHistory {
            schema_version: FAILURE_CLUSTER_HISTORY_SCHEMA_VERSION.into(),
            entries: vec![entry],
        };

        let markdown = render_cluster_history_markdown(&history);

        assert!(markdown.contains("- Owner: unassigned\n"));
        assert!(markdown.contains("- Stage: absent\n"));
        assert!(markdown.contains(
            "- Transition transition-1: failure-absent -> <absence> (compile_effect -> absent)\n"
        ));
        Ok(())
    }

    #[test]
    fn triage_check_history_requires_an_existing_history_receipt() -> TestResult {
        let temp = tempfile::tempdir()?;
        let report = two_file_parse_failure_report();
        let bundle = write_triage_bundle(temp.path(), &report)?;
        let missing = temp.path().join("history").join("cluster-history.json");

        let Err(error) = triage(TriageConfig {
            history: Some(missing.clone()),
            check_history: true,
            ..triage_config(bundle, temp.path().join("triage"))
        }) else {
            bail!("--check-history must fail closed when no history receipt exists");
        };
        // The absent receipt must be named directly. Silently substituting an
        // empty history would also fail, but for the wrong reason — it would
        // report a cluster-history mismatch instead of a missing receipt.
        let message = error.to_string();
        assert!(
            message.contains(&format!("cluster history {} is missing", missing.display())),
            "unclear error: {error}"
        );
        assert!(!message.contains("cluster history check failed"), "unclear error: {error}");
        Ok(())
    }

    #[test]
    fn triage_check_history_rejects_a_stale_history_receipt() -> TestResult {
        let temp = tempfile::tempdir()?;
        let report = two_file_parse_failure_report();
        let bundle = write_triage_bundle(temp.path(), &report)?;
        let output = temp.path().join("triage");
        let history = temp.path().join("history").join("cluster-history.json");

        triage(TriageConfig {
            history: Some(history.clone()),
            write_history: true,
            ..triage_config(bundle, output.clone())
        })?;

        // Re-run the same bundle against a receipt whose failures moved to a
        // different bucket: the persisted cluster identity no longer reproduces.
        let mut moved = sample_compile_report();
        mark_file_failed(&mut moved, "base/lex.t", "hir_lowering");
        mark_file_failed(&mut moved, "base/ok.t", "hir_lowering");
        moved.buckets.insert("hir_lowering".into(), 2);
        let moved_bundle = write_triage_bundle(temp.path(), &moved)?;

        let Err(error) = triage(TriageConfig {
            history: Some(history),
            check_history: true,
            ..triage_config(moved_bundle, output)
        }) else {
            bail!("--check-history must reject a history that no longer reproduces");
        };
        assert!(
            error.to_string().contains("cluster history check failed"),
            "unclear error: {error}"
        );
        Ok(())
    }

    #[test]
    fn triage_rejects_a_compile_receipt_from_a_different_subject() -> TestResult {
        let temp = tempfile::tempdir()?;
        let mut report = two_file_parse_failure_report();
        report.commit = "different-measurement-sha".into();
        let bundle = write_triage_bundle(temp.path(), &report)?;

        let Err(error) = triage(triage_config(bundle, temp.path().join("triage"))) else {
            bail!("triage must reject a compile receipt from another subject");
        };
        assert!(
            error.to_string().contains("compile report identity does not match evidence bundle"),
            "unclear error: {error}"
        );
        Ok(())
    }

    fn write_boundary_registry(path: &Path, entry: SemanticBoundaryRegistryEntry) -> TestResult {
        write_boundary_registry_entries(path, vec![entry])
    }

    fn write_boundary_registry_entries(
        path: &Path,
        entries: Vec<SemanticBoundaryRegistryEntry>,
    ) -> TestResult {
        write_json_receipt(
            path,
            &SemanticBoundaryRegistry {
                schema_version: SEMANTIC_BOUNDARY_REGISTRY_SCHEMA_VERSION.into(),
                entries,
            },
        )
    }

    /// Write an accepted V2 baseline for `report` and return its path.
    fn write_accepted_baseline(path: &Path, report: &RunReport) -> TestResult {
        let baseline = baseline_v2_from_report(
            report,
            &sample_registry_series(),
            &sample_baseline_v2_config(),
            None,
            &[],
        )?;
        write_json_receipt(path, &baseline)
    }

    fn boundary_config(registry: PathBuf, output: PathBuf) -> BoundaryRegistryConfig {
        BoundaryRegistryConfig {
            registry,
            baselines: Vec::new(),
            bundles: Vec::new(),
            output: Some(output),
            check: false,
            report: true,
            historical: false,
        }
    }

    fn read_boundary_report(path: &Path) -> TestResult<serde_json::Value> {
        Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
    }

    #[test]
    fn boundary_registry_report_is_structural_without_accepted_baselines() -> TestResult {
        let temp = tempfile::tempdir()?;
        let registry = temp.path().join("registry.json");
        write_boundary_registry(&registry, sample_registry_entry())?;
        let output = temp.path().join("reports").join("boundary-registry.json");

        boundaries(boundary_config(registry, output.clone()))?;

        let report = read_boundary_report(&output)?;
        // Without baselines the registry can only be checked for shape, and the
        // receipt must say so rather than claiming current-evidence authority.
        assert_eq!(report["mode"], "structural");
        assert_eq!(report["valid"], true);
        assert_eq!(report["registry_entries"], 1);
        assert_eq!(report["baselines_checked"].as_array().map(Vec::len), Some(0));
        assert_eq!(report["bundles_checked"].as_array().map(Vec::len), Some(0));
        assert_eq!(report["counts"]["by_disposition"]["deferred_runtime"], 1);
        assert_eq!(report["counts"]["by_state"]["active"], 1);
        assert_eq!(report["counts"]["by_lock_scope"]["none"], 1);
        assert_eq!(report["counts"]["by_replacement_strategy"]["hir_semantics"], 1);
        assert_eq!(report["counts"]["by_profile"]["base"], 1);
        assert_eq!(report["counts"]["by_owner_issue"]["#4753"], 1);
        assert_eq!(report["counts"]["downstream_static_facts_blocked"], 1);
        Ok(())
    }

    #[test]
    fn boundary_registry_report_is_current_when_the_baseline_agrees() -> TestResult {
        let temp = tempfile::tempdir()?;
        let registry = temp.path().join("registry.json");
        write_boundary_registry(&registry, sample_registry_entry())?;
        let baseline = temp.path().join("baseline.json");
        let mut report = sample_compile_report();
        report.semantic_boundaries.push(sample_semantic_boundary());
        write_accepted_baseline(&baseline, &report)?;
        let output = temp.path().join("boundary-registry.json");

        boundaries(BoundaryRegistryConfig {
            baselines: vec![baseline.clone()],
            ..boundary_config(registry, output.clone())
        })?;

        let receipt = read_boundary_report(&output)?;
        assert_eq!(receipt["mode"], "current");
        assert_eq!(receipt["valid"], true);
        assert_eq!(receipt["violations"].as_array().map(Vec::len), Some(0));
        assert_eq!(receipt["baselines_checked"][0], baseline.display().to_string());
        Ok(())
    }

    #[test]
    fn boundary_registry_report_separates_missing_active_boundaries() -> TestResult {
        let temp = tempfile::tempdir()?;
        let registry = temp.path().join("registry.json");
        write_boundary_registry(&registry, sample_registry_entry())?;
        let baseline = temp.path().join("baseline.json");
        // The accepted baseline no longer emits the registered boundary.
        write_accepted_baseline(&baseline, &sample_compile_report())?;
        let output = temp.path().join("boundary-registry.json");

        let Err(error) = boundaries(BoundaryRegistryConfig {
            baselines: vec![baseline],
            ..boundary_config(registry, output.clone())
        }) else {
            bail!("an active boundary absent from fresh evidence must fail closed");
        };
        assert!(
            error.to_string().contains("semantic-boundary registry validation failed"),
            "unclear error: {error}"
        );

        // The receipt is still written, and routes the violation to the
        // dedicated missing-active channel rather than leaving it unclassified.
        let receipt = read_boundary_report(&output)?;
        assert_eq!(receipt["mode"], "current");
        assert_eq!(receipt["valid"], false);
        let missing = receipt["missing_active"]
            .as_array()
            .ok_or_else(|| color_eyre::eyre::eyre!("missing_active is not an array"))?;
        assert_eq!(missing.len(), 1);
        assert!(
            missing[0]
                .as_str()
                .is_some_and(|violation| violation.contains("runtime_symbolic_reference")
                    && violation.contains("absent from fresh baseline"))
        );
        assert_eq!(receipt["emitting_retired"].as_array().map(Vec::len), Some(0));
        Ok(())
    }

    #[test]
    fn boundary_registry_report_keeps_shape_violations_out_of_the_missing_active_channel()
    -> TestResult {
        let temp = tempfile::tempdir()?;
        let registry = temp.path().join("registry.json");
        // Two identical active entries produce a *duplicate* active-boundary
        // violation. It mentions an active registry boundary but says nothing
        // about absence, so it must stay in the general violation list.
        write_boundary_registry_entries(
            &registry,
            vec![sample_registry_entry(), sample_registry_entry()],
        )?;
        let output = temp.path().join("boundary-registry.json");

        let Err(_) = boundaries(boundary_config(registry, output.clone())) else {
            bail!("a duplicate active registry boundary must fail closed");
        };

        let receipt = read_boundary_report(&output)?;
        assert_eq!(receipt["valid"], false);
        assert_eq!(receipt["missing_active"].as_array().map(Vec::len), Some(0));
        assert_eq!(receipt["emitting_retired"].as_array().map(Vec::len), Some(0));
        assert!(receipt["violations"].as_array().is_some_and(|violations| violations.iter().any(
            |violation| {
                violation
                    .as_str()
                    .is_some_and(|text| text.contains("duplicate active registry boundary key"))
            }
        )));
        Ok(())
    }

    #[test]
    fn boundary_registry_report_separates_retired_boundaries_that_still_emit() -> TestResult {
        let temp = tempfile::tempdir()?;
        let registry = temp.path().join("registry.json");
        let mut entry = sample_registry_entry();
        entry.state = SemanticBoundaryRegistryState::Retired;
        entry.retirement_pr = Some("#5300".into());
        entry.retirement_bundle = Some("bundle-sha256:example".into());
        write_boundary_registry(&registry, entry)?;
        let baseline = temp.path().join("baseline.json");
        let mut report = sample_compile_report();
        report.semantic_boundaries.push(sample_semantic_boundary());
        write_accepted_baseline(&baseline, &report)?;
        let output = temp.path().join("boundary-registry.json");

        let Err(_) = boundaries(BoundaryRegistryConfig {
            baselines: vec![baseline],
            ..boundary_config(registry, output.clone())
        }) else {
            bail!("a retired boundary that still emits must fail closed");
        };

        let receipt = read_boundary_report(&output)?;
        let emitting = receipt["emitting_retired"]
            .as_array()
            .ok_or_else(|| color_eyre::eyre::eyre!("emitting_retired is not an array"))?;
        assert_eq!(emitting.len(), 1);
        assert!(
            emitting[0]
                .as_str()
                .is_some_and(|violation| violation.contains("still emits in baseline"))
        );
        assert_eq!(receipt["missing_active"].as_array().map(Vec::len), Some(0));
        assert_eq!(receipt["counts"]["by_state"]["retired"], 1);
        Ok(())
    }

    #[test]
    fn boundary_registry_historical_mode_accepts_boundaries_absent_from_the_baseline() -> TestResult
    {
        let temp = tempfile::tempdir()?;
        let registry = temp.path().join("registry.json");
        write_boundary_registry(&registry, sample_registry_entry())?;
        let baseline = temp.path().join("baseline.json");
        write_accepted_baseline(&baseline, &sample_compile_report())?;
        let output = temp.path().join("boundary-registry.json");

        // Historical replay compares registry shape and identity but must not
        // demand that a retained historical boundary still emits today.
        boundaries(BoundaryRegistryConfig {
            baselines: vec![baseline],
            historical: true,
            ..boundary_config(registry, output.clone())
        })?;

        let receipt = read_boundary_report(&output)?;
        assert_eq!(receipt["mode"], "historical");
        assert_eq!(receipt["valid"], true);
        assert_eq!(receipt["missing_active"].as_array().map(Vec::len), Some(0));
        Ok(())
    }

    #[test]
    fn boundary_registry_report_records_unreadable_baselines_as_violations() -> TestResult {
        let temp = tempfile::tempdir()?;
        let registry = temp.path().join("registry.json");
        write_boundary_registry(&registry, sample_registry_entry())?;
        let baseline = temp.path().join("not-a-baseline.json");
        fs::write(&baseline, "{ not json")?;
        let output = temp.path().join("boundary-registry.json");

        let Err(_) = boundaries(BoundaryRegistryConfig {
            baselines: vec![baseline.clone()],
            ..boundary_config(registry, output.clone())
        }) else {
            bail!("an undecodable baseline must fail closed");
        };

        let receipt = read_boundary_report(&output)?;
        // An unreadable baseline is a violation, not a silently skipped input,
        // and it must not be promoted to current-evidence authority.
        assert_eq!(receipt["mode"], "structural");
        assert_eq!(receipt["valid"], false);
        assert!(
            receipt["violations"][0]
                .as_str()
                .is_some_and(|violation| violation.contains("not-a-baseline.json"))
        );
        Ok(())
    }

    #[test]
    fn exact_retirement_requires_every_identity_field_to_match() -> TestResult {
        let baseline = baseline_v2_from_report(
            &sample_compile_report(),
            &sample_registry_series(),
            &sample_baseline_v2_config(),
            None,
            &[],
        )?;
        let mut entry = sample_registry_entry();
        entry.state = SemanticBoundaryRegistryState::Retiring;

        // Without a retirement bundle reference there is no exact evidence.
        assert!(!has_exact_retirement(&baseline, &entry));

        entry.retirement_bundle = Some("bundle-sha256:example".into());
        let retirement = BoundaryRetirement {
            schema_version: BOUNDARY_RETIREMENT_SCHEMA_VERSION.into(),
            path: entry.path.clone(),
            id: entry.id.clone(),
            source_start: entry.source_span.start,
            source_end: entry.source_span.end,
            series_id: baseline.series_id.clone(),
            manifest_hash: baseline.manifest_hash.clone(),
            measurement_sha: baseline.repository_commit.clone(),
            source_report_digest: baseline.source_report_digest.clone(),
            transition_id: "transition-1".into(),
            replacement_issue: "#5168".into(),
            evidence_bundle: "bundle-sha256:example".into(),
        };
        let mut accepted = baseline.clone();
        accepted.boundary_retirements = vec![retirement.clone()];
        assert!(has_exact_retirement(&accepted, &entry));

        // Every identity field participates: weakening any one of them must
        // stop the retirement from counting as exact evidence.
        let mutations: Vec<Box<dyn Fn(&mut BoundaryRetirement)>> = vec![
            Box::new(|value| value.path = "base/other.t".into()),
            Box::new(|value| value.id = "other_boundary".into()),
            Box::new(|value| value.source_start = value.source_start.saturating_add(1)),
            Box::new(|value| value.source_end = value.source_end.saturating_add(1)),
            Box::new(|value| value.series_id = "series-other".into()),
            Box::new(|value| value.manifest_hash = "manifest-other".into()),
            Box::new(|value| value.measurement_sha = "sha-other".into()),
            Box::new(|value| value.source_report_digest = "sha256:other".into()),
            Box::new(|value| value.evidence_bundle = "bundle-sha256:other".into()),
        ];
        for (index, mutate) in mutations.iter().enumerate() {
            let mut weakened = retirement.clone();
            mutate(&mut weakened);
            let mut candidate = baseline.clone();
            candidate.boundary_retirements = vec![weakened];
            assert!(
                !has_exact_retirement(&candidate, &entry),
                "mutation {index} was accepted as exact retirement evidence"
            );
        }
        Ok(())
    }

    #[test]
    fn prepare_ref_validation_rejects_empty_and_path_like_refs() -> TestResult {
        validate_prepare_ref("v5.42.0")?;
        validate_prepare_ref("refs/heads/blead")?;

        for rejected in ["", "   ", "../etc/passwd", "blead/../..", "windows\\path"] {
            let Err(error) = validate_prepare_ref(rejected) else {
                bail!("prepare --ref {rejected:?} must be rejected");
            };
            assert!(
                error.to_string().contains("perl-core-harness prepare --ref"),
                "unclear error for {rejected:?}: {error}"
            );
        }
        Ok(())
    }

    #[test]
    fn prepare_paths_sanitize_ref_characters_into_a_single_component() {
        assert_eq!(safe_path_component("refs/heads/blead"), "refs-heads-blead");
        assert_eq!(safe_path_component("v5.42.0-RC1_x"), "v5.42.0-RC1_x");
        assert_eq!(safe_path_component("a b:c"), "a-b-c");

        let output_dir = default_prepare_output_dir("refs/heads/blead");
        assert!(output_dir.ends_with("target/perl-core/upstream/refs-heads-blead"));
        let receipt = default_prepare_receipt_path("refs/heads/blead");
        assert!(receipt.ends_with("target/perl-core/prepare/refs-heads-blead/prepare.json"));
        assert!(
            default_baseline_path(HarnessMode::Compile, HarnessProfile::Base)
                .ends_with(".ci/perl-core-harness/base-compile-baseline.json")
        );
    }

    #[test]
    fn rail_states_distinguish_available_evidence_from_absence() {
        let available = available_rail(
            RUN_REPORT_SCHEMA_VERSION,
            "selected execution receipt validated".into(),
            vec!["bundle:bundle-1".into()],
        );
        assert_eq!(available.availability, CompatibilityRailAvailability::Available);
        assert_eq!(available.schema_version.as_deref(), Some(RUN_REPORT_SCHEMA_VERSION));
        assert_eq!(available.evidence_refs, vec!["bundle:bundle-1"]);

        let unavailable = unavailable_rail("no execution receipt was supplied");
        assert_eq!(unavailable.availability, CompatibilityRailAvailability::NotAvailable);
        // An unavailable rail must not advertise a schema or borrow evidence.
        assert!(unavailable.schema_version.is_none());
        assert!(unavailable.evidence_refs.is_empty());
    }
}

#[cfg(test)]
mod digest_intake_case_tests {
    //! #7725: Git identities and sha256 digests accepted by the publication
    //! and evidence intake validators must keep exactly one canonical
    //! serialized spelling: lower-case hexadecimal.

    use super::{validate_digest, validate_git_sha};

    #[test]
    fn git_shas_accept_only_canonical_lower_case_hex() {
        assert!(validate_git_sha(&"cd".repeat(20), "landed commit").is_ok());
        assert!(validate_git_sha(&"01".repeat(32), "landed commit").is_ok());
        assert!(validate_git_sha(&"CD".repeat(20), "landed commit").is_err());
        assert!(validate_git_sha(&"EF".repeat(32), "landed commit").is_err());
        assert!(validate_git_sha(&"cD".repeat(20), "landed commit").is_err());
        assert!(validate_git_sha(&"zz".repeat(20), "landed commit").is_err());
        assert!(validate_git_sha(&"cd".repeat(19), "landed commit").is_err());
    }

    #[test]
    fn sha256_digests_keep_prefix_policy_and_require_lower_case() {
        assert!(validate_digest(&format!("sha256:{}", "ab".repeat(32)), "receipt digest").is_ok());
        assert!(validate_digest(&format!("sha256:{}", "AB".repeat(32)), "receipt digest").is_err());
        assert!(validate_digest(&format!("sha256:{}", "aB".repeat(32)), "receipt digest").is_err());
        assert!(validate_digest(&"ab".repeat(32), "receipt digest").is_err());
        assert!(validate_digest(&format!("sha1:{}", "ab".repeat(32)), "receipt digest").is_err());
        assert!(validate_digest(&format!("sha256:{}", "ab".repeat(31)), "receipt digest").is_err());
    }
}
