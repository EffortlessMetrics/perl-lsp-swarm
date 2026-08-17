#![warn(missing_docs)]
#![cfg_attr(clippy, allow(missing_docs))]

//! Upstream Perl core harness integration scaffold.
//!
//! The scaffold can discover tests from a prepared Perl source tree and run the
//! staged profile through a `t/perl` compatibility wrapper in parse and compile
//! modes. Execute mode is limited to explicit selected base tests.

mod normalization;
pub mod public_evidence;
mod series;
pub mod transition;

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
pub use series::{SeriesManifestConfig, series_manifest};

use normalization::{hex_lower, sha256_digest_bytes};
use public_evidence::PublicStringClass;
use serde::{Deserialize, de::DeserializeOwned};
use series::{read_series_manifest, validate_series_manifest};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Write};
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
        io::stdout()
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
            bail!("duplicate landed lineage path {}", path.display());
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

fn validate_git_sha(value: &str, label: &str) -> Result<()> {
    if value.len() != 40 && value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        bail!("{label} must be a 40- or 64-character hexadecimal SHA");
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
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("{label} must contain 64 hexadecimal characters");
    }
    Ok(())
}

fn repository_relative_path(root: &Path, path: &Path) -> Result<String> {
    let root = normalize_windows_extended_path(
        &fs::canonicalize(root)
            .with_context(|| format!("canonicalizing repository root {}", root.display()))?,
    );
    let candidate =
        if path.is_absolute() { normalize_windows_extended_path(path) } else { root.join(path) };
    let relative = candidate.strip_prefix(&root).map_err(|_| {
        color_eyre::eyre::eyre!("path {} is outside repository {}", path.display(), root.display())
    })?;
    let relative = relative.to_string_lossy().replace('\\', "/");
    validate_public_path(&relative, "repository-relative path")?;
    Ok(relative)
}

fn normalize_windows_extended_path(path: &Path) -> PathBuf {
    let value = path.to_string_lossy();
    PathBuf::from(value.strip_prefix("\\\\?\\").unwrap_or(&value))
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
        baseline_digest: sha256_digest_bytes(&fs::read(accepted_path)?),
        baseline_evidence_bundle_id: accepted_baseline.evidence_bundle.clone(),
        accepted_transition_id: accepted_baseline.accepted_transition_id.clone(),
        files_total: accepted_baseline.files_total,
        files_passed: accepted_baseline.files_passed,
    };
    let identity = CompatibilitySeriesIdentity {
        series_id: series.series_id.clone(),
        profile: series.profile,
        profile_roots: series.profile_roots.clone(),
        manifest_hash: series.manifest_hash.clone(),
        denominator: series.normalized_manifest.len(),
        repository_commit: series.repository_commit.clone(),
        perl_requested_ref: series.perl_requested_ref.clone(),
        perl_resolved_ref: series.perl_resolved_ref.clone(),
        runner: series.runner,
        compiler_subject_identity: series.compiler_subject_identity.clone(),
        invocation_identity: series.invocation_identity.clone(),
        capability_identity: series.capability_identity.clone(),
        environment_identity: series.environment_identity.clone(),
        preparation_receipt_id: series.preparation_receipt_id.clone(),
        preparation_receipt_digest: series.preparation_receipt_digest.clone(),
        measurement_sha: bundle.index.lineage.measurement_sha.clone(),
        publication_sha: bundle.index.lineage.publication_sha.clone(),
        landed_sha: bundle.index.lineage.landed_sha.clone(),
        evidence_bundle_id: bundle.index.bundle_id.clone(),
    };
    Ok(CompilerCompatibilitySeries {
        identity,
        current_observation: observation,
        transition_candidate: CompatibilityTransitionCandidate {
            transition,
            reason: transition_reason,
            requires_acceptance,
        },
        accepted_ratchet,
        parse: compatibility_run_state(&parse_report, PARSE_BASELINE_SCHEMA_VERSION, &bundle.index.bundle_id, parse_clusters.clusters.len()),
        compile: compatibility_run_state(
            &compile_report,
            &compile_baseline.schema_version,
            &bundle.index.bundle_id,
            compile_clusters.clusters.len(),
        ),
        debt,
        clusters: cluster_state,
        execution,
        curated_gold: unavailable_rail("curated semantic-gold receipt was not supplied"),
        differential_oracle: unavailable_rail("differential-oracle receipt was not supplied"),
        eir: unavailable_rail("EIR evaluation receipt was not supplied"),
        claim_boundary: "compile-harness and typed receipt state only; general semantics and runtime correctness are not implied".into(),
    })
}

fn validate_authority_artifact_bindings(
    observation_path: &Path,
    accepted_path: &Path,
    expected_observation_path: &str,
    expected_observation_digest: &str,
    expected_accepted_path: Option<&str>,
    expected_accepted_digest: Option<&str>,
    repository_root: Option<&Path>,
) -> Result<()> {
    let Some(root) = repository_root else {
        bail!("current-authority artifact binding requires a repository root");
    };
    let observation_relative = repository_relative_path(root, observation_path)?;
    if observation_relative != expected_observation_path {
        bail!("current-authority observation path does not match the supplied bundle");
    }
    let observation_digest = sha256_digest_bytes(&fs::read(observation_path)?);
    if observation_digest != expected_observation_digest {
        bail!("current-authority observation digest does not match the supplied bundle");
    }
    let Some(expected_accepted_path) = expected_accepted_path else {
        bail!("current-authority entry omits its accepted baseline path");
    };
    let Some(expected_accepted_digest) = expected_accepted_digest else {
        bail!("current-authority entry omits its accepted baseline digest");
    };
    let accepted_relative = repository_relative_path(root, accepted_path)?;
    if accepted_relative != expected_accepted_path {
        bail!("current-authority accepted-baseline path does not match the supplied baseline");
    }
    let accepted_digest = sha256_digest_bytes(&fs::read(accepted_path)?);
    if accepted_digest != expected_accepted_digest {
        bail!("current-authority accepted-baseline digest does not match the supplied baseline");
    }
    Ok(())
}

fn validate_accepted_ratchet_identity(
    baseline: &CompileBaselineV2,
    series: &SeriesManifest,
) -> Result<()> {
    if baseline.schema_version != COMPILE_BASELINE_V2_SCHEMA_VERSION
        || baseline.series_id != series.series_id
        || baseline.manifest_hash != series.manifest_hash
        || baseline.repository_commit != series.repository_commit
        || baseline.perl_resolved_ref != series.perl_resolved_ref
        || baseline.profile != series.profile
        || baseline.runner != series.runner
        || baseline.mode != HarnessMode::Compile
        || baseline.file_membership != series.normalized_manifest
        || baseline.files_total != series.normalized_manifest.len()
    {
        bail!("accepted baseline is not an identity match for series {}", series.series_id);
    }
    validate_result_summary_shape(
        baseline.files_total,
        baseline.files_passed,
        baseline.files_failed,
        baseline.tap_assertions_total,
        baseline.tap_assertions_passed,
        &baseline.file_results,
        "accepted baseline",
    )?;
    let membership = file_result_membership(&baseline.file_results)?;
    let expected = series.normalized_manifest.iter().cloned().collect::<BTreeSet<_>>();
    if membership != expected {
        bail!("accepted baseline file results do not match series {}", series.series_id);
    }
    let violations = validate_accepted_semantic_boundary_inventory(&baseline.semantic_boundaries);
    if !violations.is_empty() {
        bail!(
            "accepted baseline is invalid:\n{}",
            violations
                .iter()
                .map(|violation| violation.message.as_str())
                .collect::<Vec<_>>()
                .join("\n")
        );
    }
    Ok(())
}

fn classify_compatibility_transition(
    accepted: &CompileBaselineV2,
    current: &RunReport,
) -> (CompatibilityTransition, String, bool) {
    if current.summary.files_passed > accepted.files_passed {
        return (
            CompatibilityTransition::ImprovementCandidate,
            format!(
                "current compile observation improved from {}/{} to {}/{}",
                accepted.files_passed,
                accepted.files_total,
                current.summary.files_passed,
                current.summary.files_total
            ),
            true,
        );
    }
    if current.summary.files_passed < accepted.files_passed {
        return (
            CompatibilityTransition::Regression,
            format!(
                "current compile observation regressed from {}/{} to {}/{}",
                accepted.files_passed,
                accepted.files_total,
                current.summary.files_passed,
                current.summary.files_total
            ),
            false,
        );
    }
    let mut accepted_boundaries = accepted.semantic_boundaries.clone();
    let mut current_boundaries = current.semantic_boundaries.clone();
    accepted_boundaries.sort_by_key(semantic_boundary_key);
    current_boundaries.sort_by_key(semantic_boundary_key);
    if accepted_boundaries != current_boundaries {
        return (
            CompatibilityTransition::ContractCorrectionCandidate,
            "compile score is unchanged but semantic-boundary evidence changed".into(),
            true,
        );
    }
    (
        CompatibilityTransition::NoChange,
        "current compile observation matches the accepted ratchet".into(),
        false,
    )
}

fn validate_report_for_compatibility(
    report: &RunReport,
    series: &SeriesManifest,
    mode: HarnessMode,
) -> Result<()> {
    validate_report_against_series(report, series, mode)?;
    ensure_valid_report_shape(report)?;
    let membership = report_membership(report)?;
    let expected = series.normalized_manifest.iter().cloned().collect::<BTreeSet<_>>();
    if membership != expected {
        bail!("{} report membership differs from series {}", mode, series.series_id);
    }
    Ok(())
}

fn report_membership(report: &RunReport) -> Result<BTreeSet<String>> {
    file_result_membership(&report.file_results)
}

fn file_result_membership(file_results: &[RunFileResult]) -> Result<BTreeSet<String>> {
    let mut membership = BTreeSet::new();
    for result in file_results {
        let path = normalize_test_path(&result.path)
            .ok_or_else(|| color_eyre::eyre::eyre!("report contains an invalid test path"))?;
        if !membership.insert(path) {
            bail!("report contains duplicate file membership");
        }
    }
    Ok(membership)
}

fn compatibility_run_state(
    report: &RunReport,
    baseline_schema_version: &str,
    bundle_id: &str,
    cluster_count: usize,
) -> CompatibilityRunState {
    CompatibilityRunState {
        schema_version: RUN_REPORT_SCHEMA_VERSION.into(),
        mode: report.mode,
        files_total: report.summary.files_total,
        files_passed: report.summary.files_passed,
        files_failed: report.summary.files_failed,
        tap_assertions_total: report.summary.tap_assertions_total,
        tap_assertions_passed: report.summary.tap_assertions_passed,
        baseline_schema_version: baseline_schema_version.into(),
        report_schema_version: report.schema_version.clone(),
        evidence_bundle_id: bundle_id.into(),
        cluster_count,
    }
}

fn load_registry_state(
    path: Option<&Path>,
    baseline: &CompileBaselineV2,
    bundle: &BoundaryBundle,
) -> Result<CompatibilityRailState> {
    let Some(path) = path else {
        return Ok(unavailable_rail("semantic-boundary registry was not supplied"));
    };
    let raw = fs::read_to_string(path)
        .with_context(|| format!("reading compatibility boundary registry {}", path.display()))?;
    let registry: SemanticBoundaryRegistry = serde_json::from_str(&raw)
        .with_context(|| format!("decoding compatibility boundary registry {}", path.display()))?;
    let mut violations = validate_boundary_registry_shape(&registry);
    violations.extend(validate_registry_against_baseline(&registry, baseline, false));
    violations.extend(validate_bundle_against_baseline(bundle, baseline));
    if !violations.is_empty() {
        bail!(
            "boundary registry is not authoritative for {}:\n{}",
            baseline.series_id,
            violations.join("\n")
        );
    }
    Ok(available_rail(
        SEMANTIC_BOUNDARY_REGISTRY_SCHEMA_VERSION,
        format!("validated {} registry entries", registry.entries.len()),
        vec![format!("series:{}", baseline.series_id)],
    ))
}

fn load_cluster_history_state(
    path: Option<&Path>,
    clusters: &FailureClusterReport,
    series: &SeriesManifest,
) -> Result<(CompatibilityRailState, CompatibilityClusterState)> {
    let Some(path) = path else {
        return Ok((
            unavailable_rail("failure-cluster history was not supplied"),
            CompatibilityClusterState {
                active_count: clusters.clusters.len(),
                unassigned_count: clusters.clusters.len(),
                by_status: BTreeMap::from([("unassigned".into(), clusters.clusters.len())]),
                history_bundle_id: None,
            },
        ));
    };
    let history = read_cluster_history(path, false)?;
    let violations = validate_cluster_history_shape(&history);
    if !violations.is_empty() {
        bail!("cluster history is not authoritative:\n{}", violations.join("\n"));
    }
    let current_violations = validate_history_against_report(&history, clusters);
    if !current_violations.is_empty() {
        bail!(
            "cluster history is stale for series {}:\n{}",
            series.series_id,
            current_violations.join("\n")
        );
    }
    let current_ids = clusters
        .clusters
        .iter()
        .map(|cluster| cluster.cluster_id.as_str())
        .collect::<BTreeSet<_>>();
    let current_entries =
        history.entries.iter().filter(|entry| current_ids.contains(entry.cluster_id.as_str()));
    let mut by_status = BTreeMap::new();
    let mut unassigned_count = 0;
    let mut history_bundle_id = None;
    for entry in current_entries {
        *by_status.entry(enum_label(entry.status)).or_insert(0) += 1;
        if entry.status == FailureClusterHistoryStatus::Unassigned {
            unassigned_count += 1;
        }
        history_bundle_id = Some(entry.last_seen_bundle.clone());
    }
    Ok((
        available_rail(
            FAILURE_CLUSTER_HISTORY_SCHEMA_VERSION,
            format!("validated {} history entries", history.entries.len()),
            history_bundle_id.clone().into_iter().collect(),
        ),
        CompatibilityClusterState {
            active_count: clusters.clusters.len(),
            unassigned_count,
            by_status,
            history_bundle_id,
        },
    ))
}

fn load_execution_rail(
    path: &Path,
    series: &SeriesManifest,
    bundle_id: &str,
) -> Result<CompatibilityRailState> {
    let report = read_run_report(path)?;
    if report.mode != HarnessMode::Execute {
        bail!("execution rail report is not execute mode");
    }
    if report.commit != series.repository_commit
        || report.perl_ref != series.perl_resolved_ref
        || report.profile != series.profile
        || report.runner != series.runner
    {
        bail!("execution rail identity does not match series {}", series.series_id);
    }
    ensure_valid_report_shape(&report)?;
    Ok(available_rail(
        RUN_REPORT_SCHEMA_VERSION,
        "selected execution receipt validated".into(),
        vec![format!("bundle:{bundle_id}")],
    ))
}

fn build_compatibility_debt_state(
    baseline: &CompileBaselineV2,
    registry: CompatibilityRailState,
    history: CompatibilityRailState,
) -> CompatibilityDebtState {
    let mut by_disposition = BTreeMap::new();
    let mut by_lock_scope = BTreeMap::new();
    let mut source_locked_count = 0;
    let mut downstream_blocking_count = 0;
    for boundary in &baseline.semantic_boundaries {
        *by_disposition.entry(enum_label(boundary.disposition)).or_insert(0) += 1;
        *by_lock_scope.entry(enum_label(boundary.lock_scope)).or_insert(0) += 1;
        if boundary.disposition == SemanticBoundaryDisposition::SourceLockedCompatibility {
            source_locked_count += 1;
        }
        if boundary.blocks_downstream_static_facts {
            downstream_blocking_count += 1;
        }
    }
    CompatibilityDebtState {
        boundary_count: baseline.semantic_boundaries.len(),
        source_locked_count,
        downstream_blocking_count,
        by_disposition,
        by_lock_scope,
        registry,
        history,
    }
}

fn unavailable_rail(reason: &str) -> CompatibilityRailState {
    CompatibilityRailState {
        availability: CompatibilityRailAvailability::NotAvailable,
        reason: reason.into(),
        schema_version: None,
        evidence_refs: Vec::new(),
    }
}

fn available_rail(
    schema_version: &str,
    reason: String,
    evidence_refs: Vec<String>,
) -> CompatibilityRailState {
    CompatibilityRailState {
        availability: CompatibilityRailAvailability::Available,
        reason,
        schema_version: Some(schema_version.into()),
        evidence_refs,
    }
}

fn validate_cluster_history_shape(history: &FailureClusterHistory) -> Vec<String> {
    let mut violations = Vec::new();
    if history.schema_version != FAILURE_CLUSTER_HISTORY_SCHEMA_VERSION {
        violations.push(format!(
            "history schema {} is not {}",
            history.schema_version, FAILURE_CLUSTER_HISTORY_SCHEMA_VERSION
        ));
    }
    let mut cluster_ids = BTreeSet::new();
    for entry in &history.entries {
        if !cluster_ids.insert(entry.cluster_id.clone()) {
            violations.push(format!("duplicate cluster history entry {}", entry.cluster_id));
        }
        for (label, value) in [
            ("cluster_id", entry.cluster_id.as_str()),
            ("signature_schema_version", entry.signature_schema_version.as_str()),
            ("series_id", entry.series_id.as_str()),
            ("manifest_hash", entry.manifest_hash.as_str()),
            ("first_seen_series_id", entry.first_seen_series_id.as_str()),
            ("first_seen_manifest_hash", entry.first_seen_manifest_hash.as_str()),
            ("last_seen_series_id", entry.last_seen_series_id.as_str()),
            ("last_seen_manifest_hash", entry.last_seen_manifest_hash.as_str()),
            ("first_seen_bundle", entry.first_seen_bundle.as_str()),
            ("last_seen_bundle", entry.last_seen_bundle.as_str()),
            ("impacted_layer", entry.impacted_layer.as_str()),
            ("direct_reproduction", entry.direct_reproduction.as_str()),
            ("proposed_transition", entry.proposed_transition.as_str()),
            ("stop_condition", entry.stop_condition.as_str()),
        ] {
            if value.trim().is_empty() {
                violations.push(format!("cluster {} has empty {label}", entry.cluster_id));
            }
        }
        if entry.current_stage.as_deref().is_some_and(|stage| stage.trim().is_empty()) {
            violations.push(format!("cluster {} has empty current_stage", entry.cluster_id));
        }
        match entry.presence {
            FailureClusterHistoryPresence::Observed => {
                if !entry.observed_in_current_bundle
                    || entry.current_authority_bundle.as_deref().is_none_or(str::is_empty)
                    || entry.absence_since_bundle.is_some()
                    || entry.current_stage.is_none()
                {
                    violations.push(format!(
                        "observed cluster {} lacks current-authority state",
                        entry.cluster_id
                    ));
                }
            }
            FailureClusterHistoryPresence::AbsentUnresolved
            | FailureClusterHistoryPresence::Resolved
            | FailureClusterHistoryPresence::AcceptedDebt => {
                if entry.observed_in_current_bundle
                    || entry.current_authority_bundle.is_some()
                    || entry.absence_since_bundle.as_deref().is_none_or(str::is_empty)
                    || entry.current_stage.is_some()
                    || !entry.current_affected_files.is_empty()
                    || !entry.current_fact_classes.is_empty()
                    || !entry.current_lsp_surfaces.is_empty()
                {
                    violations.push(format!(
                        "absent cluster {} retains current-authority state",
                        entry.cluster_id
                    ));
                }
            }
        }
        if entry.status == FailureClusterHistoryStatus::Resolved
            && entry.presence != FailureClusterHistoryPresence::Resolved
        {
            violations
                .push(format!("resolved cluster {} has non-resolved presence", entry.cluster_id));
        }
        if entry.status == FailureClusterHistoryStatus::AcceptedDebt
            && entry.presence != FailureClusterHistoryPresence::AcceptedDebt
        {
            violations
                .push(format!("accepted-debt cluster {} has non-debt presence", entry.cluster_id));
        }
        if entry.status != FailureClusterHistoryStatus::Unassigned
            && !entry.owner_issue.as_deref().is_some_and(is_issue_reference)
        {
            violations.push(format!(
                "cluster {} requires an issue owner or explicit unassigned status",
                entry.cluster_id
            ));
        }
        if entry.presence == FailureClusterHistoryPresence::Resolved
            && entry.status != FailureClusterHistoryStatus::Resolved
        {
            violations.push(format!(
                "resolved-presence cluster {} has non-resolved status",
                entry.cluster_id
            ));
        }
        if entry.presence == FailureClusterHistoryPresence::AcceptedDebt
            && entry.status != FailureClusterHistoryStatus::AcceptedDebt
        {
            violations.push(format!(
                "accepted-debt presence cluster {} has non-debt status",
                entry.cluster_id
            ));
        }
        if entry.status == FailureClusterHistoryStatus::AcceptedDebt
            && entry.accepted_debt_refs.is_empty()
        {
            violations.push(format!(
                "accepted-debt cluster {} has no registry reference",
                entry.cluster_id
            ));
        }
        if entry.status == FailureClusterHistoryStatus::Resolved {
            if entry.resolution_pr.as_deref().is_none_or(|value| !is_pr_reference(value)) {
                violations
                    .push(format!("resolved cluster {} has no resolution PR", entry.cluster_id));
            }
            if entry.resolution_bundle.as_deref().is_none_or(|value| value.trim().is_empty()) {
                violations.push(format!(
                    "resolved cluster {} has no resolution bundle",
                    entry.cluster_id
                ));
            }
        }
        validate_sorted_unique(
            &entry.current_affected_files,
            &format!("cluster {} current affected files", entry.cluster_id),
            &mut violations,
        );
        validate_sorted_unique(
            &entry.historical_affected_files,
            &format!("cluster {} affected files", entry.cluster_id),
            &mut violations,
        );
        validate_sorted_unique(
            &entry.current_fact_classes,
            &format!("cluster {} current fact classes", entry.cluster_id),
            &mut violations,
        );
        validate_sorted_unique(
            &entry.fact_classes,
            &format!("cluster {} fact classes", entry.cluster_id),
            &mut violations,
        );
        validate_sorted_unique(
            &entry.current_lsp_surfaces,
            &format!("cluster {} current LSP surfaces", entry.cluster_id),
            &mut violations,
        );
        validate_sorted_unique(
            &entry.lsp_surfaces,
            &format!("cluster {} LSP surfaces", entry.cluster_id),
            &mut violations,
        );
        for path in
            entry.current_affected_files.iter().chain(entry.historical_affected_files.iter())
        {
            if validate_public_path(path, "cluster history file").is_err() {
                violations.push(format!(
                    "cluster {} has invalid affected file {}",
                    entry.cluster_id, path
                ));
            }
        }
        let mut transition_ids = BTreeSet::new();
        for transition in &entry.transitions {
            if !transition_ids.insert(transition.transition_id.clone()) {
                violations.push(format!(
                    "cluster {} has duplicate transition {}",
                    entry.cluster_id, transition.transition_id
                ));
            }
            for (label, value) in [
                ("transition_id", transition.transition_id.as_str()),
                ("from_cluster_id", transition.from_cluster_id.as_str()),
                ("from_stage", transition.from_stage.as_str()),
                ("to_stage", transition.to_stage.as_str()),
                ("before_series_id", transition.before_series_id.as_str()),
                ("before_manifest_hash", transition.before_manifest_hash.as_str()),
                ("before_bundle_id", transition.before_bundle_id.as_str()),
                ("after_series_id", transition.after_series_id.as_str()),
                ("after_manifest_hash", transition.after_manifest_hash.as_str()),
                ("after_bundle_id", transition.after_bundle_id.as_str()),
                ("proof_plan", transition.proof_plan.as_str()),
                ("stop_condition", transition.stop_condition.as_str()),
            ] {
                if value.trim().is_empty() {
                    violations.push(format!(
                        "cluster {} transition {} has empty {label}",
                        entry.cluster_id, transition.transition_id
                    ));
                }
            }
            if transition.to_cluster_id.is_none()
                && transition.to_presence == FailureClusterHistoryPresence::Observed
            {
                violations.push(format!(
                    "transition {} without a target cluster must not become observed",
                    transition.transition_id
                ));
            }
            if transition.to_cluster_id.as_deref() == Some(transition.from_cluster_id.as_str()) {
                violations.push(format!(
                    "transition {} has identical source and target clusters",
                    transition.transition_id
                ));
            }
            if transition.from_stage == transition.to_stage {
                violations.push(format!(
                    "transition {} has no stage or root-cause change",
                    transition.transition_id
                ));
            }
        }
        if entry.status == FailureClusterHistoryStatus::Resolved
            && !entry.transitions.iter().any(|transition| {
                transition.from_cluster_id == entry.cluster_id
                    && transition.before_series_id == entry.first_seen_series_id
                    && transition.before_manifest_hash == entry.first_seen_manifest_hash
                    && transition.before_bundle_id == entry.first_seen_bundle
                    && transition.to_cluster_id.is_none()
                    && transition.to_presence == FailureClusterHistoryPresence::Resolved
                    && transition.after_series_id == entry.series_id
                    && transition.after_manifest_hash == entry.manifest_hash
                    && transition.after_bundle_id
                        == entry.resolution_bundle.as_deref().unwrap_or_default()
            })
        {
            violations.push(format!(
                "resolved cluster {} lacks a matching before/after transition",
                entry.cluster_id
            ));
        }
    }
    violations
}

fn validate_sorted_unique(values: &[String], label: &str, violations: &mut Vec<String>) {
    let mut sorted = values.to_vec();
    sorted.sort();
    sorted.dedup();
    if sorted != values {
        violations.push(format!("{label} must be sorted and unique"));
    }
}

fn is_pr_reference(value: &str) -> bool {
    value.strip_prefix('#').is_some_and(|digits| {
        !digits.is_empty() && digits.chars().all(|character| character.is_ascii_digit())
    })
}

fn merge_cluster_history(
    mut history: FailureClusterHistory,
    report: &FailureClusterReport,
) -> Result<FailureClusterHistory> {
    let current_ids =
        report.clusters.iter().map(|cluster| cluster.cluster_id.as_str()).collect::<BTreeSet<_>>();
    for entry in &mut history.entries {
        if current_ids.contains(entry.cluster_id.as_str()) {
            continue;
        }
        if entry.presence == FailureClusterHistoryPresence::Observed {
            entry.presence = FailureClusterHistoryPresence::AbsentUnresolved;
            entry.observed_in_current_bundle = false;
            entry.current_authority_bundle = None;
            entry.absence_since_bundle = Some(report.bundle_id.clone());
            entry.current_affected_files.clear();
            entry.current_fact_classes.clear();
            entry.current_lsp_surfaces.clear();
            entry.current_stage = None;
        }
    }
    for cluster in &report.clusters {
        if let Some(entry) =
            history.entries.iter_mut().find(|entry| entry.cluster_id == cluster.cluster_id)
        {
            if entry.series_id != report.series_id || entry.manifest_hash != report.manifest_hash {
                bail!(
                    "cluster {} history identity differs from current report",
                    cluster.cluster_id
                );
            }
            if matches!(
                entry.status,
                FailureClusterHistoryStatus::Resolved | FailureClusterHistoryStatus::AcceptedDebt
            ) {
                bail!(
                    "cluster {} is recorded as {} but is active in the current bundle",
                    cluster.cluster_id,
                    enum_label(entry.status)
                );
            }
            entry.last_seen_bundle = report.bundle_id.clone();
            entry.series_id = report.series_id.clone();
            entry.manifest_hash = report.manifest_hash.clone();
            entry.last_seen_series_id = report.series_id.clone();
            entry.last_seen_manifest_hash = report.manifest_hash.clone();
            entry.current_affected_files = cluster.affected_files.clone();
            merge_sorted_unique(&mut entry.historical_affected_files, &cluster.affected_files);
            entry.current_fact_classes = cluster.fact_classes.clone();
            merge_sorted_unique(&mut entry.fact_classes, &cluster.fact_classes);
            entry.current_lsp_surfaces = cluster.lsp_surfaces.clone();
            merge_sorted_unique(&mut entry.lsp_surfaces, &cluster.lsp_surfaces);
            entry.occurrence_count = cluster.occurrence_count;
            entry.current_stage = Some(cluster.signature.stage.clone());
            entry.impacted_layer = cluster.impacted_layer.clone();
            entry.direct_reproduction = cluster.direct_reproduction.clone();
            entry.current_authority_bundle = Some(report.bundle_id.clone());
            entry.observed_in_current_bundle = true;
            entry.absence_since_bundle = None;
            entry.presence = FailureClusterHistoryPresence::Observed;
        } else {
            history.entries.push(history_entry_from_cluster(report, cluster));
        }
    }
    history.entries.sort_by(|left, right| left.cluster_id.cmp(&right.cluster_id));
    let violations = validate_cluster_history_shape(&history);
    if !violations.is_empty() {
        bail!("updated cluster history is invalid:\n{}", violations.join("\n"));
    }
    Ok(history)
}

fn history_entry_from_cluster(
    report: &FailureClusterReport,
    cluster: &FailureCluster,
) -> FailureClusterHistoryEntry {
    let mut affected_files = cluster.affected_files.clone();
    let mut fact_classes = cluster.fact_classes.clone();
    let mut lsp_surfaces = cluster.lsp_surfaces.clone();
    affected_files.sort();
    fact_classes.sort();
    lsp_surfaces.sort();
    FailureClusterHistoryEntry {
        cluster_id: cluster.cluster_id.clone(),
        signature_schema_version: cluster.signature.schema_version.clone(),
        identity_quality: FailureClusterIdentityQuality::Provisional,
        series_id: report.series_id.clone(),
        manifest_hash: report.manifest_hash.clone(),
        first_seen_series_id: report.series_id.clone(),
        first_seen_manifest_hash: report.manifest_hash.clone(),
        last_seen_series_id: report.series_id.clone(),
        last_seen_manifest_hash: report.manifest_hash.clone(),
        first_seen_bundle: report.bundle_id.clone(),
        last_seen_bundle: report.bundle_id.clone(),
        current_affected_files: affected_files.clone(),
        historical_affected_files: affected_files,
        current_fact_classes: fact_classes.clone(),
        fact_classes,
        current_lsp_surfaces: lsp_surfaces.clone(),
        lsp_surfaces,
        occurrence_count: cluster.occurrence_count,
        current_stage: Some(cluster.signature.stage.clone()),
        current_authority_bundle: Some(report.bundle_id.clone()),
        observed_in_current_bundle: true,
        absence_since_bundle: None,
        presence: FailureClusterHistoryPresence::Observed,
        impacted_layer: cluster.impacted_layer.clone(),
        owner_issue: None,
        status: FailureClusterHistoryStatus::Unassigned,
        direct_reproduction: cluster.direct_reproduction.clone(),
        proposed_transition: format!(
            "resolve {} with general compiler semantics",
            cluster.cluster_id
        ),
        stop_condition: format!("exact-series proof retires {}", cluster.cluster_id),
        accepted_debt_refs: Vec::new(),
        resolution_pr: None,
        resolution_bundle: None,
        transitions: Vec::new(),
    }
}

fn merge_sorted_unique(values: &mut Vec<String>, additions: &[String]) {
    values.extend(additions.iter().cloned());
    values.sort();
    values.dedup();
}

fn validate_history_against_report(
    history: &FailureClusterHistory,
    report: &FailureClusterReport,
) -> Vec<String> {
    let mut violations = Vec::new();
    let current = report
        .clusters
        .iter()
        .map(|cluster| (cluster.cluster_id.as_str(), cluster))
        .collect::<BTreeMap<_, _>>();
    for cluster in &report.clusters {
        let Some(entry) =
            history.entries.iter().find(|entry| entry.cluster_id == cluster.cluster_id)
        else {
            violations
                .push(format!("current cluster {} is missing from history", cluster.cluster_id));
            continue;
        };
        if entry.series_id != report.series_id || entry.manifest_hash != report.manifest_hash {
            violations
                .push(format!("current cluster {} has stale series identity", cluster.cluster_id));
        }
        if entry.last_seen_bundle != report.bundle_id {
            violations
                .push(format!("current cluster {} has stale last-seen bundle", cluster.cluster_id));
        }
        if entry.presence != FailureClusterHistoryPresence::Observed
            || !entry.observed_in_current_bundle
            || entry.current_authority_bundle.as_deref() != Some(report.bundle_id.as_str())
        {
            violations.push(format!(
                "current cluster {} is not marked observed in the current authority",
                cluster.cluster_id
            ));
        }
        if entry.current_stage.as_deref() != Some(cluster.signature.stage.as_str()) {
            violations.push(format!(
                "current cluster {} has an unrecorded stage transition",
                cluster.cluster_id
            ));
        }
        if matches!(
            entry.status,
            FailureClusterHistoryStatus::Resolved | FailureClusterHistoryStatus::AcceptedDebt
        ) {
            violations.push(format!(
                "historical {} cluster {} is active again",
                enum_label(entry.status),
                cluster.cluster_id
            ));
        }
    }
    for entry in &history.entries {
        if current.contains_key(entry.cluster_id.as_str()) {
            continue;
        }
        if entry.presence == FailureClusterHistoryPresence::Observed
            || entry.observed_in_current_bundle
            || entry.current_authority_bundle.is_some()
        {
            violations.push(format!(
                "history cluster {} is absent from the report but still marked current",
                entry.cluster_id
            ));
        }
        // `absence_since_bundle` records the first absence, not the latest check. A
        // later bundle may confirm absence without changing that lifecycle boundary.
        if entry.presence == FailureClusterHistoryPresence::AbsentUnresolved
            && entry.absence_since_bundle.as_deref().is_none_or(str::is_empty)
        {
            violations
                .push(format!("absent cluster {} has no first-absence bundle", entry.cluster_id));
        }
    }
    violations
}

fn render_cluster_history_markdown(history: &FailureClusterHistory) -> String {
    let mut counts = BTreeMap::<String, usize>::new();
    for entry in &history.entries {
        *counts.entry(enum_label(entry.status)).or_default() += 1;
    }
    let mut markdown = format!(
        "# Compiler failure cluster history\n\n- Schema: {}\n- Entries: {}\n\n",
        history.schema_version,
        history.entries.len()
    );
    markdown.push_str("## Status counts\n\n");
    for (status, count) in counts {
        markdown.push_str(&format!("- {status}: {count}\n"));
    }
    let mut high_leverage = history.entries.iter().collect::<Vec<_>>();
    high_leverage.sort_by(|left, right| {
        right
            .occurrence_count
            .cmp(&left.occurrence_count)
            .then_with(|| left.cluster_id.cmp(&right.cluster_id))
    });
    markdown.push_str("\n## High leverage\n\n");
    for entry in high_leverage.iter().take(10) {
        markdown.push_str(&format!(
            "- {}: {} occurrence(s), status {}, owner {}\n",
            entry.cluster_id,
            entry.occurrence_count,
            enum_label(entry.status),
            entry.owner_issue.as_deref().unwrap_or("unassigned")
        ));
    }
    markdown.push_str("\n## Clusters\n\n");
    for entry in &history.entries {
        markdown.push_str(&format!(
            "### {}\n\n- Status: {}\n- Owner: {}\n- Stage: {}\n- Current series: {}\n- First/last series: {} / {}\n- First/last bundle: {} / {}\n- Current files: {}\n- Historical files: {}\n- Occurrences: {}\n- Reproduction: {}\n",
            entry.cluster_id,
            enum_label(entry.status),
            entry.owner_issue.as_deref().unwrap_or("unassigned"),
            entry.current_stage.as_deref().unwrap_or("absent"),
            entry.series_id,
            entry.first_seen_series_id,
            entry.last_seen_series_id,
            entry.first_seen_bundle,
            entry.last_seen_bundle,
            entry.current_affected_files.join(", "),
            entry.historical_affected_files.join(", "),
            entry.occurrence_count,
            entry.direct_reproduction,
        ));
        for transition in &entry.transitions {
            markdown.push_str(&format!(
                "- Transition {}: {} -> {} ({} -> {})\n",
                transition.transition_id,
                transition.from_cluster_id,
                transition.to_cluster_id.as_deref().unwrap_or("<absence>"),
                transition.from_stage,
                transition.to_stage
            ));
        }
        markdown.push('\n');
    }
    markdown
}

fn bundle_artifact_path(bundle: &BoundaryBundle, kind: &str) -> Result<PathBuf> {
    let artifact = bundle
        .index
        .artifacts
        .iter()
        .find(|artifact| artifact.kind == kind)
        .ok_or_else(|| color_eyre::eyre::eyre!("evidence bundle has no {kind} artifact"))?;
    validate_public_path(&artifact.logical_path, "evidence bundle artifact")?;
    let path = bundle.path.parent().unwrap_or_else(|| Path::new(".")).join(&artifact.logical_path);
    if !path.is_file() {
        bail!("evidence bundle artifact {} is missing", path.display());
    }
    Ok(path)
}

fn validate_bundle_report_identity(bundle: &BoundaryBundle, report: &RunReport) -> Result<()> {
    if report.schema_version != RUN_REPORT_SCHEMA_VERSION {
        bail!("triage requires a v1 run report");
    }
    if report.mode != HarnessMode::Compile {
        bail!("triage requires a compile report");
    }
    if report.commit != bundle.index.repository_commit
        || report.perl_ref != bundle.index.perl_resolved_ref
        || report.profile != bundle.index.profile
        || report.runner != bundle.index.runner
    {
        bail!("compile report identity does not match evidence bundle");
    }
    let mut report_boundaries = report.semantic_boundaries.clone();
    report_boundaries.sort_by_key(semantic_boundary_key);
    if report_boundaries != bundle.semantic_boundaries {
        bail!("compile report semantic-boundary inventory does not match evidence bundle");
    }
    Ok(())
}

fn build_failure_cluster_report(
    bundle: &BoundaryBundle,
    report: &RunReport,
) -> Result<FailureClusterReport> {
    let mut grouped = BTreeMap::<String, (FailureClusterSignature, Vec<RunFailure>)>::new();
    for failure in &report.failures {
        let failure = normalize_failure(failure)?;
        let signature = failure_signature(bundle, report, &failure)?;
        let key = serde_json::to_vec(&signature).context("serializing failure signature")?;
        let cluster_key = hex_lower(&Sha256::digest(key));
        grouped.entry(cluster_key).or_insert_with(|| (signature, Vec::new())).1.push(failure);
    }

    let mut clusters = Vec::new();
    for (cluster_key, (signature, mut failures)) in grouped {
        failures.sort_by(|left, right| {
            left.path
                .cmp(&right.path)
                .then_with(|| left.phase.cmp(&right.phase))
                .then_with(|| left.first_diagnostic.cmp(&right.first_diagnostic))
        });
        let representative_failure = failures
            .first()
            .cloned()
            .ok_or_else(|| color_eyre::eyre::eyre!("failure cluster has no representative"))?;
        let mut affected_files =
            failures.iter().map(|failure| failure.path.clone()).collect::<Vec<_>>();
        affected_files.sort();
        affected_files.dedup();
        let cluster_id = format!("failure-{:.16}", cluster_key);
        clusters.push(FailureCluster {
            cluster_id,
            signature: signature.clone(),
            affected_files,
            representative_failure: representative_failure.clone(),
            direct_reproduction: format!(
                "bundle={} series={} mode={} profile={} test={}",
                bundle.index.bundle_id,
                bundle.index.series_id,
                report.mode,
                report.profile,
                representative_failure.path
            ),
            impacted_layer: impacted_layer(&signature.stage, &signature.bucket).into(),
            fact_classes: signature.fact_classes.clone(),
            lsp_surfaces: signature.lsp_surfaces.clone(),
            occurrence_count: failures.len(),
            exact_series_proof_required: true,
        });
    }
    clusters.sort_by(|left, right| left.cluster_id.cmp(&right.cluster_id));

    let mut debt_candidates = bundle
        .semantic_boundaries
        .iter()
        .filter(|boundary| {
            !matches!(
                boundary.disposition,
                SemanticBoundaryDisposition::ImplementedStatic
                    | SemanticBoundaryDisposition::StaticallyClassified
                    | SemanticBoundaryDisposition::OrdinaryRuntime
            )
        })
        .map(|boundary| FailureDebtCandidate {
            path: boundary.path.clone(),
            id: boundary.id.clone(),
            disposition: boundary.disposition,
            reason: boundary.reason.clone(),
            owner_workstream: boundary.owner_workstream.clone(),
            exact_series_proof_required: true,
        })
        .collect::<Vec<_>>();
    debt_candidates
        .sort_by(|left, right| left.path.cmp(&right.path).then_with(|| left.id.cmp(&right.id)));
    Ok(FailureClusterReport {
        schema_version: FAILURE_CLUSTER_SCHEMA_VERSION.into(),
        bundle_id: bundle.index.bundle_id.clone(),
        series_id: bundle.index.series_id.clone(),
        manifest_hash: bundle.index.manifest_hash.clone(),
        repository_commit: bundle.index.repository_commit.clone(),
        profile: report.profile,
        mode: report.mode,
        clusters,
        debt_candidates,
    })
}

fn normalize_failure(failure: &RunFailure) -> Result<RunFailure> {
    let path = normalize_test_path(&failure.path)
        .ok_or_else(|| color_eyre::eyre::eyre!("failure path is not a Perl test path"))?;
    validate_public_path(&path, "failure path")?;
    let mut normalized = failure.clone();
    normalized.path = path;
    normalized.first_diagnostic = normalize_diagnostic(&failure.first_diagnostic);
    Ok(normalized)
}

fn normalize_diagnostic(diagnostic: &str) -> String {
    diagnostic
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .split_whitespace()
        .map(|token| {
            let trimmed = token.trim_matches(|character: char| {
                matches!(character, '"' | '\'' | '(' | ')' | '[' | ']' | ',' | ';')
            });
            let is_url = trimmed.contains("://");
            let is_windows_path = trimmed.len() >= 3
                && trimmed.as_bytes()[0].is_ascii_alphabetic()
                && trimmed.as_bytes()[1] == b':'
                && matches!(trimmed.as_bytes()[2], b'/' | b'\\');
            if !is_url
                && (trimmed.starts_with('/')
                    || is_windows_path
                    || trimmed.contains("\\")
                    || trimmed.split('/').any(|part| {
                        matches!(part.to_ascii_lowercase().as_str(), "target" | "tmp" | "temp")
                    }))
            {
                "<host-path>"
            } else {
                token
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn failure_signature(
    bundle: &BoundaryBundle,
    report: &RunReport,
    failure: &RunFailure,
) -> Result<FailureClusterSignature> {
    let stage = failure_stage(&failure.bucket).ok_or_else(|| {
        color_eyre::eyre::eyre!("unclassifiable failure bucket {}", failure.bucket)
    })?;
    if failure.path.trim().is_empty() || failure.workstream.trim().is_empty() {
        bail!("failure record lacks a stable path or workstream");
    }
    validate_public_path(&failure.path, "failure path")?;
    let fact_classes = vec![failure.bucket.clone()];
    let mut lsp_surfaces = lsp_impact_for_bucket(&failure.bucket)
        .into_iter()
        .map(ToString::to_string)
        .chain(failure.lsp_impact.iter().cloned())
        .collect::<Vec<_>>();
    lsp_surfaces.sort();
    lsp_surfaces.dedup();
    let shape_seed =
        format!("{}|{}|{}|{}", stage, failure.bucket, failure.workstream, fact_classes.join(","));
    Ok(FailureClusterSignature {
        schema_version: FAILURE_CLUSTER_SCHEMA_VERSION.into(),
        series_id: bundle.index.series_id.clone(),
        profile: report.profile,
        mode: report.mode,
        stage: stage.into(),
        bucket: failure.bucket.clone(),
        workstream: failure.workstream.clone(),
        source_shape_fingerprint: format!("shape-{:.16}", hex_lower(&Sha256::digest(shape_seed))),
        fact_classes,
        lsp_surfaces,
    })
}

fn failure_stage(bucket: &str) -> Option<&'static str> {
    match bucket {
        "parse_recovery" | "source_decode" => Some("parse_recovery"),
        "hir_lowering" => Some("hir_unmodeled"),
        "compile_effect" => Some("compile_effect"),
        "scope_pad"
        | "package_stash"
        | "pragma_feature"
        | "module_resolution"
        | "runtime_value_model"
        | "runtime_control_flow"
        | "runtime_io"
        | "runtime_regex"
        | "runtime_require_use"
        | "runtime_test_harness" => Some("compile_effect"),
        "cli_switch" | "harness_prepare" | "process_timeout" | "process_signal" | "environment" => {
            Some("harness")
        }
        _ => None,
    }
}

fn impacted_layer(stage: &str, bucket: &str) -> &'static str {
    if stage == "harness" {
        return "harness_or_environment";
    }
    match bucket {
        "parse_recovery" | "source_decode" => "parser",
        "hir_lowering" => "hir",
        "compile_effect" | "scope_pad" | "package_stash" | "pragma_feature"
        | "module_resolution" => "compiler_world",
        _ => "compiler_semantics",
    }
}

fn render_failure_cluster_markdown(report: &FailureClusterReport) -> String {
    let mut markdown = format!(
        "# Compiler failure clusters\n\n- Bundle: `{}`\n- Series: `{}`\n- Profile/mode: `{}` / `{}`\n\n",
        report.bundle_id, report.series_id, report.profile, report.mode
    );
    markdown.push_str("## Clusters\n\n");
    if report.clusters.is_empty() {
        markdown.push_str("No product failure clusters were observed.\n\n");
    }
    for cluster in &report.clusters {
        markdown.push_str(&format!(
            "### `{}`\n\n- Stage: `{}`\n- Bucket: `{}`\n- Layer: `{}`\n- Occurrences: {}\n- Files: {}\n- Reproduction: `{}`\n\n",
            cluster.cluster_id,
            cluster.signature.stage,
            cluster.signature.bucket,
            cluster.impacted_layer,
            cluster.occurrence_count,
            cluster.affected_files.join(", "),
            cluster.direct_reproduction
        ));
    }
    markdown.push_str("## Semantic-boundary debt candidates\n\n");
    for debt in &report.debt_candidates {
        markdown.push_str(&format!(
            "- `{}` `{}` `{}` — {}\n",
            debt.path,
            debt.id,
            enum_label(debt.disposition),
            debt.reason
        ));
    }
    markdown
}

#[derive(Debug, Clone, serde::Serialize)]
struct BoundaryRegistryReport {
    schema_version: String,
    mode: String,
    registry_entries: usize,
    baselines_checked: Vec<String>,
    bundles_checked: Vec<String>,
    counts: BoundaryRegistryCounts,
    missing_active: Vec<String>,
    emitting_retired: Vec<String>,
    violations: Vec<String>,
    valid: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
struct BoundaryRegistryCounts {
    by_disposition: BTreeMap<String, usize>,
    by_lock_scope: BTreeMap<String, usize>,
    by_profile: BTreeMap<String, usize>,
    by_owner_issue: BTreeMap<String, usize>,
    by_replacement_strategy: BTreeMap<String, usize>,
    by_state: BTreeMap<String, usize>,
    downstream_static_facts_blocked: usize,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
struct EvidenceBundleIndex {
    schema_version: String,
    bundle_id: String,
    series_id: String,
    manifest_hash: String,
    repository_commit: String,
    profile: HarnessProfile,
    runner: HarnessRunner,
    perl_resolved_ref: String,
    lineage: EvidenceBundleLineage,
    artifacts: Vec<EvidenceBundleArtifact>,
    completeness: EvidenceBundleCompleteness,
    lifecycle: String,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
struct EvidenceBundleLineage {
    measurement_sha: String,
    #[serde(default)]
    publication_sha: Option<String>,
    #[serde(default)]
    landed_sha: Option<String>,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
struct EvidenceBundleArtifact {
    kind: String,
    logical_path: String,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
struct EvidenceBundleCompleteness {
    status: String,
    normalized_authority: bool,
}

#[derive(Debug, Clone)]
struct BoundaryBundle {
    path: PathBuf,
    index: EvidenceBundleIndex,
    semantic_boundaries: Vec<ObservedSemanticBoundary>,
}

fn read_boundary_bundle(path: &Path) -> Result<BoundaryBundle> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("reading evidence bundle index {}", path.display()))?;
    let index: EvidenceBundleIndex = serde_json::from_str(&raw)
        .with_context(|| format!("decoding evidence bundle index {}", path.display()))?;
    if index.schema_version != "perl_core_harness.evidence_bundle.v1" {
        bail!("unsupported evidence bundle schema {}", index.schema_version);
    }
    if index.bundle_id.trim().is_empty()
        || index.series_id.trim().is_empty()
        || index.manifest_hash.trim().is_empty()
        || index.repository_commit.trim().is_empty()
        || index.perl_resolved_ref.trim().is_empty()
        || index.lineage.measurement_sha.trim().is_empty()
    {
        bail!("evidence bundle index has incomplete subject identity");
    }
    if index.lifecycle != "published"
        || index.completeness.status != "complete"
        || !index.completeness.normalized_authority
    {
        bail!("evidence bundle is not a complete normalized authority");
    }
    let artifact =
        index.artifacts.iter().find(|artifact| artifact.kind == "semantic_boundaries").ok_or_else(
            || color_eyre::eyre::eyre!("evidence bundle has no semantic-boundaries artifact"),
        )?;
    validate_public_path(&artifact.logical_path, "evidence bundle artifact")?;
    let boundary_path =
        path.parent().unwrap_or_else(|| Path::new(".")).join(&artifact.logical_path);
    let boundary_raw = fs::read_to_string(&boundary_path).with_context(|| {
        format!("reading semantic-boundaries artifact {}", boundary_path.display())
    })?;
    let mut semantic_boundaries: Vec<ObservedSemanticBoundary> =
        serde_json::from_str(&boundary_raw).with_context(|| {
            format!("decoding semantic-boundaries artifact {}", boundary_path.display())
        })?;
    semantic_boundaries.sort_by_key(semantic_boundary_key);
    if semantic_boundaries
        .windows(2)
        .any(|pair| semantic_boundary_key(&pair[0]) == semantic_boundary_key(&pair[1]))
    {
        bail!("semantic-boundaries artifact contains a duplicate boundary key");
    }
    Ok(BoundaryBundle { path: path.to_path_buf(), index, semantic_boundaries })
}

fn validate_boundary_registry_shape(registry: &SemanticBoundaryRegistry) -> Vec<String> {
    let mut violations = Vec::new();
    if registry.schema_version != SEMANTIC_BOUNDARY_REGISTRY_SCHEMA_VERSION {
        violations.push(format!(
            "registry schema {} is not {}",
            registry.schema_version, SEMANTIC_BOUNDARY_REGISTRY_SCHEMA_VERSION
        ));
    }
    let mut active_keys = BTreeSet::new();
    let mut meanings = BTreeMap::<String, String>::new();
    for entry in &registry.entries {
        let key = registry_boundary_key(entry);
        let scoped_key = format!("{}:{}:{}", entry.series_id, entry.profile, key);
        if entry.state != SemanticBoundaryRegistryState::Retired && !active_keys.insert(scoped_key)
        {
            violations.push(format!("duplicate active registry boundary key {key}"));
        }
        let meaning_key = entry.id.clone();
        let fingerprint = format!(
            "{:?}|{}|{:?}|{}",
            entry.disposition, entry.source_kind, entry.lock_scope, entry.semantic_meaning
        );
        if let Some(previous) = meanings.insert(meaning_key.clone(), fingerprint.clone())
            && previous != fingerprint
        {
            violations
                .push(format!("stable registry ID {} is reused for another meaning", entry.id));
        }
        for (label, value) in [
            ("id", entry.id.as_str()),
            ("source_kind", entry.source_kind.as_str()),
            ("semantic_meaning", entry.semantic_meaning.as_str()),
            ("series_id", entry.series_id.as_str()),
            ("manifest_hash", entry.manifest_hash.as_str()),
            ("source_shape", entry.source_shape.as_str()),
            ("reason", entry.reason.as_str()),
            ("ambient_dependency", entry.ambient_dependency.as_str()),
            ("owner_issue", entry.owner_issue.as_str()),
            ("supporting_test", entry.supporting_test.as_str()),
            ("wrong_file_test", entry.wrong_file_test.as_str()),
            ("changed_shape_test", entry.changed_shape_test.as_str()),
            ("introduction_pr", entry.introduction_pr.as_str()),
            ("introduction_commit", entry.introduction_commit.as_str()),
            ("first_accepted_bundle", entry.first_accepted_bundle.as_str()),
        ] {
            if value.trim().is_empty() {
                violations.push(format!("registry boundary {} has empty {label}", entry.id));
            }
        }
        if let Err(error) = validate_public_path(&entry.path, "registry boundary path") {
            violations.push(format!("{}: {error}", entry.id));
        }
        for (label, value) in [
            ("supporting_test", entry.supporting_test.as_str()),
            ("wrong_file_test", entry.wrong_file_test.as_str()),
            ("changed_shape_test", entry.changed_shape_test.as_str()),
        ] {
            if let Err(error) = validate_public_path(value, label) {
                violations.push(format!("{}: {error}", entry.id));
            }
        }
        if entry.source_span.start >= entry.source_span.end {
            violations.push(format!("registry boundary {} has an invalid source span", entry.id));
        }
        if !is_issue_reference(&entry.owner_issue) {
            violations.push(format!("registry boundary {} has an invalid owner issue", entry.id));
        }
        if matches!(
            entry.disposition,
            SemanticBoundaryDisposition::Unknown | SemanticBoundaryDisposition::Unsupported
        ) {
            violations
                .push(format!("registry boundary {} has a non-admissible disposition", entry.id));
        }
        if entry.disposition == SemanticBoundaryDisposition::SourceLockedCompatibility
            && entry.lock_scope != SemanticBoundaryLockScope::PathAndSource
        {
            violations.push(format!("registry boundary {} widened source lock scope", entry.id));
        }
        if matches!(
            entry.state,
            SemanticBoundaryRegistryState::Retiring | SemanticBoundaryRegistryState::Retired
        ) && (entry.retirement_pr.as_deref().is_none_or(str::is_empty)
            || entry.retirement_bundle.as_deref().is_none_or(str::is_empty))
        {
            violations
                .push(format!("registry boundary {} has incomplete retirement lineage", entry.id));
        }
        if entry.replacement_strategy
            == SemanticBoundaryReplacementStrategy::LongLivedTestHarnessCompatibility
            && entry.review_after.is_none()
            && entry.permanent_boundary_rationale.is_none()
        {
            violations.push(format!(
                "registry boundary {} lacks review or permanent-debt rationale",
                entry.id
            ));
        }
    }
    violations.sort();
    violations
}

fn validate_registry_against_baseline(
    registry: &SemanticBoundaryRegistry,
    baseline: &CompileBaselineV2,
    historical: bool,
) -> Vec<String> {
    let mut violations = Vec::new();
    violations.extend(
        validate_accepted_semantic_boundary_inventory(&baseline.semantic_boundaries)
            .into_iter()
            .map(|violation| format!("baseline {}: {}", baseline.series_id, violation.message)),
    );
    let entries = registry
        .entries
        .iter()
        .filter(|entry| entry.series_id == baseline.series_id && entry.profile == baseline.profile)
        .collect::<Vec<_>>();
    let entry_by_key = entries
        .iter()
        .map(|entry| (registry_boundary_key(entry), *entry))
        .collect::<BTreeMap<_, _>>();
    let observed_keys = baseline
        .semantic_boundaries
        .iter()
        .map(registry_boundary_key_from_observed)
        .collect::<BTreeSet<_>>();
    for boundary in &baseline.semantic_boundaries {
        let key = registry_boundary_key_from_observed(boundary);
        let Some(entry) = entry_by_key.get(&registry_boundary_key_from_observed(boundary)) else {
            violations.push(format!(
                "series {} boundary {} has no registry entry",
                baseline.series_id, key
            ));
            continue;
        };
        violations.extend(compare_registry_entry(entry, boundary, baseline));
    }
    if !historical {
        for entry in entries {
            let key = registry_boundary_key(entry);
            if entry.state == SemanticBoundaryRegistryState::Active && !observed_keys.contains(&key)
            {
                violations.push(format!(
                    "active registry boundary {} is absent from fresh baseline {}",
                    entry.id, baseline.series_id
                ));
            }
            if entry.state == SemanticBoundaryRegistryState::Retired && observed_keys.contains(&key)
            {
                violations.push(format!(
                    "retired registry boundary {} still emits in baseline {}",
                    entry.id, baseline.series_id
                ));
            }
            if !observed_keys.contains(&key)
                && matches!(
                    entry.state,
                    SemanticBoundaryRegistryState::Retiring
                        | SemanticBoundaryRegistryState::Retired
                )
                && !has_exact_retirement(baseline, entry)
            {
                violations.push(format!(
                    "boundary {} disappeared without exact retirement evidence in baseline {}",
                    entry.id, baseline.series_id
                ));
            }
        }
    }
    violations.sort();
    violations
}

fn compare_registry_entry(
    entry: &SemanticBoundaryRegistryEntry,
    boundary: &ObservedSemanticBoundary,
    baseline: &CompileBaselineV2,
) -> Vec<String> {
    let mut violations = Vec::new();
    let checks = [
        (entry.id == boundary.id, "id"),
        (entry.path == boundary.path, "path"),
        (entry.disposition == boundary.disposition, "disposition"),
        (entry.source_kind == boundary.source_kind, "source_kind"),
        (entry.source_span == boundary.source_span, "source_span"),
        (entry.lock_scope == boundary.lock_scope, "lock_scope"),
        (entry.reason == boundary.reason, "reason"),
        (
            entry.blocks_downstream_static_facts == boundary.blocks_downstream_static_facts,
            "blocks_downstream_static_facts",
        ),
        (entry.series_id == baseline.series_id, "series_id"),
        (entry.profile == baseline.profile, "profile"),
        (entry.manifest_hash == baseline.manifest_hash, "manifest_hash"),
    ];
    for (matches, field) in checks {
        if !matches {
            violations.push(format!(
                "registry boundary {} disagrees with baseline {} in {field}",
                entry.id, baseline.series_id
            ));
        }
    }
    violations
}

fn has_exact_retirement(
    baseline: &CompileBaselineV2,
    entry: &SemanticBoundaryRegistryEntry,
) -> bool {
    let Some(bundle) = entry.retirement_bundle.as_deref() else { return false };
    baseline.boundary_retirements.iter().any(|retirement| {
        retirement.path == entry.path
            && retirement.id == entry.id
            && retirement.source_start == entry.source_span.start
            && retirement.source_end == entry.source_span.end
            && retirement.series_id == baseline.series_id
            && retirement.manifest_hash == baseline.manifest_hash
            && retirement.measurement_sha == baseline.repository_commit
            && retirement.source_report_digest == baseline.source_report_digest
            && retirement.evidence_bundle == bundle
    })
}

fn validate_bundle_against_baseline(
    bundle: &BoundaryBundle,
    baseline: &CompileBaselineV2,
) -> Vec<String> {
    let mut violations = Vec::new();
    let identity_checks = [
        (bundle.index.series_id == baseline.series_id, "series_id"),
        (bundle.index.manifest_hash == baseline.manifest_hash, "manifest_hash"),
        (bundle.index.repository_commit == baseline.repository_commit, "repository_commit"),
        (bundle.index.lineage.measurement_sha == baseline.repository_commit, "measurement_sha"),
        (bundle.index.profile == baseline.profile, "profile"),
    ];
    for (matches, field) in identity_checks {
        if !matches {
            violations.push(format!(
                "bundle {} disagrees with baseline {} in {field}",
                bundle.index.bundle_id, baseline.series_id
            ));
        }
    }
    let mut baseline_boundaries = baseline.semantic_boundaries.clone();
    baseline_boundaries.sort_by_key(semantic_boundary_key);
    if bundle.semantic_boundaries != baseline_boundaries {
        violations.push(format!(
            "bundle {} semantic-boundary inventory disagrees with baseline {}",
            bundle.index.bundle_id, baseline.series_id
        ));
    }
    violations
}

fn boundary_registry_report(
    registry: &SemanticBoundaryRegistry,
    baselines: &[(PathBuf, CompileBaselineV2)],
    bundles: &[BoundaryBundle],
    historical: bool,
    mut violations: Vec<String>,
) -> BoundaryRegistryReport {
    violations.sort();
    let missing_active = violations
        .iter()
        .filter(|violation| {
            violation.contains("active registry boundary") && violation.contains("absent")
        })
        .cloned()
        .collect();
    let emitting_retired = violations
        .iter()
        .filter(|violation| {
            violation.contains("retired registry boundary") && violation.contains("still emits")
        })
        .cloned()
        .collect();
    let mut counts = BoundaryRegistryCounts {
        by_disposition: BTreeMap::new(),
        by_lock_scope: BTreeMap::new(),
        by_profile: BTreeMap::new(),
        by_owner_issue: BTreeMap::new(),
        by_replacement_strategy: BTreeMap::new(),
        by_state: BTreeMap::new(),
        downstream_static_facts_blocked: 0,
    };
    for entry in &registry.entries {
        increment(&mut counts.by_disposition, enum_label(entry.disposition));
        increment(&mut counts.by_lock_scope, enum_label(entry.lock_scope));
        increment(&mut counts.by_profile, entry.profile.to_string());
        increment(&mut counts.by_owner_issue, entry.owner_issue.clone());
        increment(&mut counts.by_replacement_strategy, enum_label(entry.replacement_strategy));
        increment(&mut counts.by_state, enum_label(entry.state));
        if entry.blocks_downstream_static_facts {
            counts.downstream_static_facts_blocked += 1;
        }
    }
    BoundaryRegistryReport {
        schema_version: SEMANTIC_BOUNDARY_REGISTRY_SCHEMA_VERSION.to_string(),
        mode: if baselines.is_empty() {
            "structural".to_string()
        } else if historical {
            "historical".to_string()
        } else {
            "current".to_string()
        },
        registry_entries: registry.entries.len(),
        baselines_checked: baselines.iter().map(|(path, _)| path.display().to_string()).collect(),
        bundles_checked: bundles.iter().map(|bundle| bundle.path.display().to_string()).collect(),
        counts,
        missing_active,
        emitting_retired,
        valid: violations.is_empty(),
        violations,
    }
}

fn increment(map: &mut BTreeMap<String, usize>, key: String) {
    *map.entry(key).or_default() += 1;
}

fn enum_label<T: serde::Serialize>(value: T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(ToString::to_string))
        .unwrap_or_else(|| "unknown".into())
}

fn registry_boundary_key(entry: &SemanticBoundaryRegistryEntry) -> String {
    format!("{}:{}:{}:{}", entry.path, entry.id, entry.source_span.start, entry.source_span.end)
}

fn registry_boundary_key_from_observed(boundary: &ObservedSemanticBoundary) -> String {
    format!(
        "{}:{}:{}:{}",
        boundary.path, boundary.id, boundary.source_span.start, boundary.source_span.end
    )
}

/// Validate one declared public path field.
///
/// Declared path fields must be repository-relative. The structural classifier
/// in [`public_evidence`] owns detection of host-path material embedded
/// anywhere inside a public string; this function additionally rejects the
/// relative forms that are legal paths but still private (`..` traversal and
/// `target`/`tmp`/`temp` components).
///
/// The rejected value is deliberately not echoed: failures surface in public CI
/// logs, so reporting the classification has to be enough.
fn validate_public_path(value: &str, label: &str) -> Result<()> {
    if let Some(kind) = public_evidence::classify_public_string(value, PublicStringClass::Ordinary)
    {
        bail!("{label} contains a private host path ({}); the value was not echoed", kind.as_str());
    }

    let normalized = value.replace('\\', "/");
    let private_component = normalized.split('/').any(|component| {
        matches!(component.to_ascii_lowercase().as_str(), "target" | "tmp" | "temp")
    });
    if normalized.trim().is_empty()
        || normalized.starts_with('/')
        || normalized.contains(":/")
        || normalized.split('/').any(|part| part == "..")
        || private_component
    {
        bail!("{label} is not a public repository-relative path; the value was not echoed");
    }
    Ok(())
}

fn is_issue_reference(value: &str) -> bool {
    value.strip_prefix('#').is_some_and(|digits| {
        !digits.is_empty() && digits.chars().all(|character| character.is_ascii_digit())
    })
}

/// Prepare an upstream Perl source tree for advisory smoke runs.
pub fn prepare(config: PrepareConfig) -> Result<()> {
    validate_prepare_ref(&config.perl_ref)?;
    let started_at = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let output_dir =
        config.output_dir.clone().unwrap_or_else(|| default_prepare_output_dir(&config.perl_ref));
    let source_dir = output_dir.join("source").join("perl5");
    let prepared_tree = output_dir.join("perl5");
    let receipt_path = default_prepare_receipt_path(&config.perl_ref);
    let configure_command = "sh Configure -des -Dusedevel".to_string();
    let test_prep_command = "make test_prep".to_string();

    let result = prepare_inner(
        &config,
        &output_dir,
        &source_dir,
        &prepared_tree,
        &configure_command,
        &test_prep_command,
    );
    let (status, resolved_ref, first_error) = match result {
        Ok(resolved_ref) => (PrepareStatus::Pass, Some(resolved_ref), None),
        Err(err) => (PrepareStatus::Fail, None, Some(err.to_string())),
    };
    let receipt = PrepareReceipt {
        schema_version: PREPARE_SCHEMA_VERSION.to_string(),
        requested_ref: config.perl_ref,
        resolved_ref,
        source_url: PERL_SOURCE_URL.to_string(),
        source_dir: source_dir.display().to_string(),
        prepared_tree: prepared_tree.display().to_string(),
        host_os: std::env::consts::OS.to_string(),
        host_arch: std::env::consts::ARCH.to_string(),
        configure_command,
        test_prep_command,
        status,
        first_error,
        started_at,
        finished_at: Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
    };
    write_prepare_receipt(&receipt_path, &receipt)?;

    tracing::info!("perl-core-harness: prepare {:?} for {}", receipt.status, receipt.requested_ref);
    tracing::info!("wrote {}", receipt_path.display());

    if receipt.status == PrepareStatus::Fail {
        let detail = receipt.first_error.as_deref().unwrap_or("unknown prepare failure");
        bail!("perl-core-harness prepare failed: {detail}");
    }
    Ok(())
}

/// Run upstream Perl core tests through the compatibility runner.
pub fn run_mode(config: RunConfig) -> Result<()> {
    let selected_tests = normalize_selected_tests(config.profile, &config.tests)?;
    validate_execute_selection(config.mode, &selected_tests)?;

    let perl_tree = canonicalize_existing_dir(&config.perl_tree, "prepared Perl tree")?;
    let run_tree = prepare_run_copy(&perl_tree, config.runner, config.mode, config.profile)?;
    let t_dir = run_tree.join("t");
    let script = validate_runner_script(&t_dir, config.runner)?;
    install_t_perl_wrapper(&run_tree)?;
    let dumptests_args = if selected_tests.is_empty() {
        profile_runner_args(config.profile, &t_dir, config.runner)?
    } else {
        selected_tests.clone()
    };
    let dumptests_output = invoke_dumptests(&config.host_perl, &t_dir, &script, &dumptests_args)?;
    let discovered = filter_discovered_tests(
        parse_dumptests_output(&dumptests_output.stdout)?,
        &selected_tests,
    )?;

    let runner_binary = resolve_runner_binary(config.runner_binary.as_deref())?;
    let context_path = run_tree.join("target").join("perl-lsp-runner-records.jsonl");
    if context_path.exists() {
        let context = format!("removing stale context {}", context_path.display());
        fs::remove_file(&context_path).context(context)?;
    }

    let output = invoke_harness_run(
        &config.host_perl,
        &t_dir,
        &script,
        &dumptests_args,
        &runner_binary,
        &context_path,
        config.mode,
    )
    .with_context(|| format!("running Perl core tests via {} {}", config.runner, config.profile))?;

    let mut records = read_runner_records_or_empty(&context_path)?;
    let used_direct_runner = invoke_runner_for_missing_records(
        &t_dir,
        &discovered,
        &records,
        &runner_binary,
        &context_path,
        config.mode,
    )?;
    if used_direct_runner {
        records = read_runner_records_or_empty(&context_path)?;
    }
    let report = build_run_report(BuildRunReportInput {
        config: &config,
        perl_tree: &perl_tree,
        run_tree: &run_tree,
        discovered: &discovered,
        records: &records,
        harness_status: output.status.code(),
    });
    let output_path =
        config.output.unwrap_or_else(|| default_run_report_path(config.mode, config.profile));
    write_run_report(&output_path, &report)?;

    tracing::info!(
        "perl-core-harness: {} {}/{} files passed via {}",
        report.mode,
        report.summary.files_passed,
        report.summary.files_total,
        report.runner
    );
    tracing::info!("wrote {}", output_path.display());

    if report.summary.files_failed > 0 {
        bail!(
            "perl-core-harness {} {} failed for {} of {} files; see {}",
            report.mode,
            report.profile,
            report.summary.files_failed,
            report.summary.files_total,
            output_path.display()
        );
    }
    if !output.status.success() && !used_direct_runner {
        bail!(
            "upstream harness exited with status {} despite no recorded file failures\nstdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

/// Stub for future report rendering.
pub fn report() -> Result<()> {
    bail!("perl-core-harness report is not implemented until run receipts exist")
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

/// Run a manual real-tree discovery + parse/compile smoke and write receipts.
pub fn smoke(config: SmokeConfig) -> Result<()> {
    let modes = normalized_smoke_modes(&config.modes)?;

    let output_dir = config.output_dir.clone().unwrap_or_else(|| default_smoke_dir(config.profile));
    fs::create_dir_all(&output_dir)
        .with_context(|| format!("creating smoke output directory {}", output_dir.display()))?;
    let discovery_path = output_dir.join("discovery.json");
    let parse_path = output_dir.join("parse.json");
    let compile_path = output_dir.join("compile.json");
    let gap_map_path = output_dir.join("gap-map.json");
    let smoke_path = output_dir.join("smoke.json");

    discover(DiscoverConfig {
        perl_tree: config.perl_tree.clone(),
        host_perl: config.host_perl.clone(),
        runner: config.runner,
        profile: config.profile,
        output: Some(discovery_path.clone()),
    })?;
    let discovery = read_discovery_report(&discovery_path)?;

    let mut parse_report = None;
    let mut compile_report = None;
    for mode in &modes {
        let report_path = match mode {
            HarnessMode::Parse => parse_path.clone(),
            HarnessMode::Compile => compile_path.clone(),
            HarnessMode::Execute => {
                bail!("perl-core-harness smoke does not support execute mode");
            }
        };
        let run_result = run_mode(RunConfig {
            perl_tree: config.perl_tree.clone(),
            host_perl: config.host_perl.clone(),
            runner: config.runner,
            mode: *mode,
            profile: config.profile,
            tests: Vec::new(),
            output: Some(report_path.clone()),
            runner_binary: config.runner_binary.clone(),
        });
        if let Err(err) = &run_result {
            if !report_path.is_file() {
                bail!(
                    "{} smoke did not write {} after runner failure: {}",
                    mode,
                    report_path.display(),
                    err
                );
            }
            tracing::info!(
                "perl-core-harness: {mode} smoke preserved bucketed failure report: {err}"
            );
        }

        let report = read_run_report(&report_path)?;
        match mode {
            HarnessMode::Parse => parse_report = Some(report),
            HarnessMode::Compile => compile_report = Some(report),
            HarnessMode::Execute => unreachable!("execute mode is rejected above"),
        }
    }

    let gap_map =
        build_gap_map(config.profile, &modes, parse_report.as_ref(), compile_report.as_ref());
    write_gap_map(&gap_map_path, &gap_map)?;

    let smoke_report = build_smoke_report(BuildSmokeReportInput {
        config: &config,
        modes: &modes,
        discovery: &discovery,
        discovery_path: &discovery_path,
        parse_path: parse_report.as_ref().map(|_| parse_path.as_path()),
        parse_report: parse_report.as_ref(),
        compile_path: compile_report.as_ref().map(|_| compile_path.as_path()),
        compile_report: compile_report.as_ref(),
        gap_map_path: &gap_map_path,
    });
    write_smoke_report(&smoke_path, &smoke_report)?;

    tracing::info!(
        "perl-core-harness: smoke {} for {} via {}",
        match smoke_report.status {
            SmokeStatus::Pass => "passed",
            SmokeStatus::Fail => "failed",
        },
        smoke_report.profile,
        smoke_report.runner
    );
    tracing::info!("wrote {}", smoke_path.display());

    if smoke_report.status == SmokeStatus::Fail {
        let details = smoke_report
            .structural_failures
            .iter()
            .map(|failure| {
                let mode = failure.mode.map(|mode| mode.to_string()).unwrap_or_else(|| "-".into());
                let path = failure.path.as_deref().unwrap_or("-");
                format!("{:?} {mode} {path}: {}", failure.kind, failure.message)
            })
            .collect::<Vec<_>>()
            .join("\n");
        bail!(
            "perl-core-harness smoke failed receipt integrity with {} structural failure(s):\n{}",
            smoke_report.structural_failures.len(),
            details
        );
    }

    Ok(())
}

fn validate_prepare_ref(perl_ref: &str) -> Result<()> {
    if perl_ref.trim().is_empty() {
        bail!("perl-core-harness prepare --ref must not be empty");
    }
    if perl_ref.contains("..") || perl_ref.contains('\\') {
        bail!("perl-core-harness prepare --ref contains unsupported path-like syntax");
    }
    Ok(())
}

fn prepare_inner(
    config: &PrepareConfig,
    output_dir: &Path,
    source_dir: &Path,
    prepared_tree: &Path,
    configure_command: &str,
    test_prep_command: &str,
) -> Result<String> {
    if !cfg!(target_os = "linux") {
        bail!(
            "upstream Perl prepare is Linux-only in this slice; current host is {}",
            std::env::consts::OS
        );
    }
    ensure_host_tool("git")?;
    ensure_host_tool("perl")?;
    ensure_host_tool("make")?;
    ensure_host_tool("sh")?;

    fs::create_dir_all(output_dir)
        .with_context(|| format!("creating prepare output directory {}", output_dir.display()))?;
    if source_dir.join(".git").is_dir() {
        run_command(
            Command::new("git").arg("-C").arg(source_dir).args(["fetch", "--tags", "origin"]),
        )?;
    } else {
        if let Some(parent) = source_dir.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("creating source parent {}", parent.display()))?;
        }
        run_command(
            Command::new("git")
                .args(["clone", "--filter=blob:none", PERL_SOURCE_URL])
                .arg(source_dir),
        )?;
    }
    run_command(Command::new("git").arg("-C").arg(source_dir).args([
        "checkout",
        "--detach",
        &config.perl_ref,
    ]))?;
    let resolved_ref =
        command_stdout(Command::new("git").arg("-C").arg(source_dir).args(["rev-parse", "HEAD"]))?;

    if prepared_tree.exists() {
        fs::remove_dir_all(prepared_tree)
            .with_context(|| format!("removing prior prepared tree {}", prepared_tree.display()))?;
    }
    copy_dir_all_filtered(source_dir, prepared_tree, &|path| {
        path.file_name().is_some_and(|name| name == ".git")
    })?;

    run_command(Command::new("sh").current_dir(prepared_tree).args([
        "Configure",
        "-des",
        "-Dusedevel",
    ]))?;
    run_command(Command::new("make").current_dir(prepared_tree).arg("test_prep"))?;
    validate_prepared_tree(prepared_tree)?;

    tracing::info!(
        "perl-core-harness: ran `{configure_command}` and `{test_prep_command}` in {}",
        prepared_tree.display()
    );
    Ok(resolved_ref)
}

fn ensure_host_tool(tool: &str) -> Result<()> {
    let output = Command::new("sh")
        .args(["-c", &format!("command -v {tool}")])
        .output()
        .with_context(|| format!("checking for host tool {tool}"))?;
    if !output.status.success() {
        bail!("required host tool is missing from PATH: {tool}");
    }
    Ok(())
}

fn command_stdout(command: &mut Command) -> Result<String> {
    let display = format!("{command:?}");
    let output = command.output().with_context(|| format!("spawning {display}"))?;
    if !output.status.success() {
        bail!(
            "command failed: {display}\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let text = String::from_utf8(output.stdout).context("decoding command stdout")?;
    Ok(text.trim().to_string())
}

fn run_command(command: &mut Command) -> Result<()> {
    let display = format!("{command:?}");
    let output = command.output().with_context(|| format!("spawning {display}"))?;
    if !output.status.success() {
        bail!(
            "command failed: {display}\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

fn validate_prepared_tree(prepared_tree: &Path) -> Result<()> {
    let t_dir = prepared_tree.join("t");
    for required in [t_dir.join("TEST"), t_dir.join("harness")] {
        if !required.is_file() {
            bail!("prepared Perl tree is missing required file {}", required.display());
        }
    }
    let base_dir = t_dir.join("base");
    let has_base_tests = base_dir
        .read_dir()
        .with_context(|| format!("reading {}", base_dir.display()))?
        .filter_map(|entry| entry.ok())
        .any(|entry| entry.path().extension().is_some_and(|extension| extension == "t"));
    if !has_base_tests {
        bail!("prepared Perl tree has no t/base/*.t tests: {}", base_dir.display());
    }
    let has_test_perl = ["perl", "perl.exe"].iter().any(|name| t_dir.join(name).is_file());
    if !has_test_perl {
        bail!("prepared Perl tree is missing t/perl or t/perl.exe in {}", t_dir.display());
    }
    Ok(())
}

fn canonicalize_existing_dir(path: &Path, label: &str) -> Result<PathBuf> {
    if !path.is_dir() {
        bail!("{label} does not exist or is not a directory: {}", path.display());
    }
    path.canonicalize().with_context(|| format!("canonicalizing {label}: {}", path.display()))
}

fn validate_runner_script(t_dir: &Path, runner: HarnessRunner) -> Result<PathBuf> {
    if !t_dir.is_dir() {
        bail!("prepared Perl tree is missing t/ directory: {}", t_dir.display());
    }
    let script = t_dir.join(runner.script_name());
    if !script.is_file() {
        bail!(
            "prepared Perl tree is missing t/{} for {} runner: {}",
            runner.script_name(),
            runner,
            script.display()
        );
    }
    Ok(script)
}

fn invoke_dumptests(
    host_perl: &Path,
    t_dir: &Path,
    script: &Path,
    profile_args: &[String],
) -> Result<Output> {
    let script_name = script
        .file_name()
        .ok_or_else(|| color_eyre::eyre::eyre!("runner script has no file name"))?;
    let mut command = Command::new(host_perl);
    command.current_dir(t_dir);
    command.arg(script_name);
    command.arg("--dumptests");
    for arg in profile_args {
        command.arg(arg);
    }
    command.env("LC_ALL", "C");
    command.env_remove("PERL5LIB");
    command.env_remove("PERLLIB");
    command.env_remove("PERL5OPT");
    command.env_remove("PERL_UNICODE");
    command.env_remove("PERL_LOCAL_LIB_ROOT");
    command.env_remove("PERL_MB_OPT");

    let output =
        command.output().with_context(|| format!("spawning host Perl: {}", host_perl.display()))?;
    if !output.status.success() {
        bail!(
            "upstream harness --dumptests failed with status {}\nstdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(output)
}

fn invoke_harness_run(
    host_perl: &Path,
    t_dir: &Path,
    script: &Path,
    profile_args: &[String],
    runner_binary: &Path,
    context_path: &Path,
    mode: HarnessMode,
) -> Result<Output> {
    let script_name = script
        .file_name()
        .ok_or_else(|| color_eyre::eyre::eyre!("runner script has no file name"))?;
    let tap_dir = project_root()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("target")
        .join("perl-core")
        .join("tap")
        .join(mode.as_str());
    fs::create_dir_all(&tap_dir)
        .with_context(|| format!("creating TAP dump directory {}", tap_dir.display()))?;

    let mut command = Command::new(host_perl);
    command.current_dir(t_dir);
    command.arg(script_name);
    for arg in profile_args {
        command.arg(arg);
    }
    command.env("LC_ALL", "C");
    command.env("TEST_JOBS", "1");
    command.env("PERL_TEST_HARNESS_DUMP_TAP", tap_dir);
    command.env("PERL_LSP_HARNESS_MODE", mode.as_str());
    command.env("PERL_LSP_HARNESS_CONTEXT", context_path);
    command.env("PERL_LSP_CORE_TEST_RUNNER", runner_binary);
    sanitize_perl_env(&mut command);

    command.output().with_context(|| format!("spawning host Perl: {}", host_perl.display()))
}

fn invoke_runner_for_missing_records(
    t_dir: &Path,
    discovered: &[DiscoveredTest],
    records: &[RunnerRecord],
    runner_binary: &Path,
    context_path: &Path,
    mode: HarnessMode,
) -> Result<bool> {
    let recorded = records
        .iter()
        .filter_map(|record| normalize_test_path(&record.path))
        .collect::<BTreeSet<_>>();
    let missing =
        discovered.iter().filter(|test| !recorded.contains(&test.path)).collect::<Vec<_>>();
    if missing.is_empty() {
        return Ok(false);
    }

    if let Some(parent) = context_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating runner context directory {}", parent.display()))?;
    }

    for test in &missing {
        let output = invoke_direct_runner(t_dir, runner_binary, context_path, mode, &test.path)?;
        if !context_path.is_file() {
            bail!(
                "direct runner did not write context for {}\nstatus: {}\nstdout:\n{}\nstderr:\n{}",
                test.path,
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }

    let refreshed = read_runner_records_or_empty(context_path)?;
    let recorded_after = refreshed
        .iter()
        .filter_map(|record| normalize_test_path(&record.path))
        .collect::<BTreeSet<_>>();
    if let Some(test) = missing.iter().find(|test| !recorded_after.contains(&test.path)) {
        bail!("direct runner did not write a record for {}", test.path);
    }

    Ok(true)
}

fn invoke_direct_runner(
    t_dir: &Path,
    runner_binary: &Path,
    context_path: &Path,
    mode: HarnessMode,
    test_path: &str,
) -> Result<Output> {
    let mut command = Command::new(runner_binary);
    command.current_dir(t_dir);
    command.arg(test_path);
    command.env("LC_ALL", "C");
    command.env("PERL_LSP_HARNESS_MODE", mode.as_str());
    command.env("PERL_LSP_HARNESS_CONTEXT", context_path);
    sanitize_perl_env(&mut command);

    command
        .output()
        .with_context(|| format!("spawning perl-core-test-runner: {}", runner_binary.display()))
}

fn sanitize_perl_env(command: &mut Command) {
    command.env_remove("PERL5LIB");
    command.env_remove("PERLLIB");
    command.env_remove("PERL5OPT");
    command.env_remove("PERL_UNICODE");
    command.env_remove("PERL_LOCAL_LIB_ROOT");
    command.env_remove("PERL_MB_OPT");
}

fn parse_dumptests_output(stdout: &[u8]) -> Result<Vec<DiscoveredTest>> {
    let text = String::from_utf8(stdout.to_vec()).context("decoding --dumptests output")?;
    let mut tests = Vec::new();
    for line in text.lines() {
        let Some(path) = normalize_test_path(line) else {
            continue;
        };
        let root = path
            .split('/')
            .next()
            .filter(|part| !part.is_empty())
            .ok_or_else(|| color_eyre::eyre::eyre!("test path has no root: {path}"))?
            .to_string();
        tests.push(DiscoveredTest { path, root });
    }
    tests.sort_by(|left, right| left.path.cmp(&right.path));
    tests.dedup_by(|left, right| left.path == right.path);
    if tests.is_empty() {
        bail!("upstream harness --dumptests returned no .t files");
    }
    Ok(tests)
}

pub(crate) fn normalize_test_path(line: &str) -> Option<String> {
    let trimmed = line.trim().trim_matches('"').trim_matches('\'');
    if trimmed.is_empty() || !trimmed.ends_with(".t") {
        return None;
    }
    let normalized = trimmed.replace('\\', "/");
    let normalized = normalized.strip_prefix("./").unwrap_or(&normalized);
    let normalized = normalized.strip_prefix("t/").unwrap_or(normalized);
    Some(normalized.to_string())
}

fn default_discovery_path(profile: HarnessProfile) -> PathBuf {
    let root = project_root().unwrap_or_else(|_| PathBuf::from("."));
    root.join("target").join("perl-core").join("discovery").join(format!("{profile}.json"))
}

fn default_run_report_path(mode: HarnessMode, profile: HarnessProfile) -> PathBuf {
    let root = project_root().unwrap_or_else(|_| PathBuf::from("."));
    root.join("target").join("perl-core").join("reports").join(format!("{profile}-{mode}.json"))
}

fn default_baseline_path(mode: HarnessMode, profile: HarnessProfile) -> PathBuf {
    let root = project_root().unwrap_or_else(|_| PathBuf::from("."));
    root.join(".ci").join("perl-core-harness").join(format!("{profile}-{mode}-baseline.json"))
}

fn default_prepare_output_dir(perl_ref: &str) -> PathBuf {
    let root = project_root().unwrap_or_else(|_| PathBuf::from("."));
    root.join("target").join("perl-core").join("upstream").join(safe_path_component(perl_ref))
}

fn default_prepare_receipt_path(perl_ref: &str) -> PathBuf {
    let root = project_root().unwrap_or_else(|_| PathBuf::from("."));
    root.join("target")
        .join("perl-core")
        .join("prepare")
        .join(safe_path_component(perl_ref))
        .join("prepare.json")
}

fn default_smoke_dir(profile: HarnessProfile) -> PathBuf {
    let root = project_root().unwrap_or_else(|_| PathBuf::from("."));
    root.join("target").join("perl-core").join("smoke").join(profile.as_str())
}

fn safe_path_component(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '.' || ch == '-' || ch == '_' { ch } else { '-' }
        })
        .collect()
}

fn write_discovery_report(path: &Path, report: &DiscoveryReport) -> Result<()> {
    if let Some(parent) = path.parent() {
        let context = format!("creating output directory {}", parent.display());
        fs::create_dir_all(parent).context(context)?;
    }
    let json = serde_json::to_string_pretty(report).context("serializing discovery report")?;
    fs::write(path, format!("{json}\n"))
        .with_context(|| format!("writing discovery report {}", path.display()))
}

pub(crate) fn read_discovery_report(path: &Path) -> Result<DiscoveryReport> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("reading discovery report {}", path.display()))?;
    serde_json::from_str(&raw)
        .with_context(|| format!("decoding discovery report {}", path.display()))
}

fn write_run_report(path: &Path, report: &RunReport) -> Result<()> {
    if let Some(parent) = path.parent() {
        let context = format!("creating output directory {}", parent.display());
        fs::create_dir_all(parent).context(context)?;
    }
    let json = serde_json::to_string_pretty(report).context("serializing run report")?;
    fs::write(path, format!("{json}\n"))
        .with_context(|| format!("writing run report {}", path.display()))
}

fn read_run_report(path: &Path) -> Result<RunReport> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("reading run report {}", path.display()))?;
    serde_json::from_str(&raw).with_context(|| format!("decoding run report {}", path.display()))
}

fn read_compile_baseline(path: &Path) -> Result<CompileBaseline> {
    let raw =
        fs::read_to_string(path).with_context(|| format!("reading baseline {}", path.display()))?;
    serde_json::from_str(&raw).with_context(|| format!("decoding baseline {}", path.display()))
}

fn write_compile_baseline(path: &Path, baseline: &CompileBaseline) -> Result<()> {
    if let Some(parent) = path.parent() {
        let context = format!("creating baseline directory {}", parent.display());
        fs::create_dir_all(parent).context(context)?;
    }
    let json = serde_json::to_string_pretty(baseline).context("serializing compile baseline")?;
    fs::write(path, format!("{json}\n"))
        .with_context(|| format!("writing baseline {}", path.display()))
}

fn write_prepare_receipt(path: &Path, receipt: &PrepareReceipt) -> Result<()> {
    if let Some(parent) = path.parent() {
        let context = format!("creating prepare receipt directory {}", parent.display());
        fs::create_dir_all(parent).context(context)?;
    }
    let json = serde_json::to_string_pretty(receipt).context("serializing prepare receipt")?;
    fs::write(path, format!("{json}\n"))
        .with_context(|| format!("writing prepare receipt {}", path.display()))
}

fn write_smoke_report(path: &Path, report: &SmokeReport) -> Result<()> {
    if let Some(parent) = path.parent() {
        let context = format!("creating smoke report directory {}", parent.display());
        fs::create_dir_all(parent).context(context)?;
    }
    let json = serde_json::to_string_pretty(report).context("serializing smoke report")?;
    fs::write(path, format!("{json}\n"))
        .with_context(|| format!("writing smoke report {}", path.display()))
}

fn write_gap_map(path: &Path, gap_map: &GapMap) -> Result<()> {
    if let Some(parent) = path.parent() {
        let context = format!("creating gap-map directory {}", parent.display());
        fs::create_dir_all(parent).context(context)?;
    }
    let json = serde_json::to_string_pretty(gap_map).context("serializing gap map")?;
    fs::write(path, format!("{json}\n"))
        .with_context(|| format!("writing gap map {}", path.display()))
}

fn baseline_from_report(report: &RunReport) -> Result<CompileBaseline> {
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

fn baseline_v2_from_report(
    report: &RunReport,
    series: &SeriesManifest,
    config: &BaselineConfig,
    previous: Option<&CompileBaselineV2>,
    retirements: &[BoundaryRetirement],
) -> Result<CompileBaselineV2> {
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

struct V2Identities {
    compiler_subject_identity: String,
    invocation_identity: String,
    capability_identity: String,
    environment_identity: String,
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

fn ensure_valid_report_shape(report: &RunReport) -> Result<()> {
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

fn validate_result_summary_shape(
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

fn validate_report_against_series(
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
fn compare_baseline_v2(
    baseline: &CompileBaselineV2,
    report: &RunReport,
    series: &SeriesManifest,
) -> BaselineComparison {
    compare_baseline_v2_with_identities(baseline, report, series, None, None, &[])
}

fn compare_baseline_v2_with_identities(
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

fn compare_boundary_transition(
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

fn report_digest(report: &RunReport) -> Result<String> {
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

fn read_compile_baseline_v2(path: &Path) -> Result<CompileBaselineV2> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("reading v2 baseline {}", path.display()))?;
    let value: serde_json::Value = serde_json::from_str(&raw)
        .with_context(|| format!("decoding baseline envelope {}", path.display()))?;
    parse_compile_baseline_v2(value, &path.display().to_string())
}

fn parse_compile_baseline_v2(value: serde_json::Value, label: &str) -> Result<CompileBaselineV2> {
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
    let mut violations = validate_persisted_boundary_retirements(&baseline, None);
    violations.extend(validate_accepted_semantic_boundary_inventory(&baseline.semantic_boundaries));
    if !violations.is_empty() {
        bail_baseline_comparison(&BaselineComparison { violations })?;
    }
    Ok(baseline)
}

fn write_compile_baseline_v2(path: &Path, baseline: &CompileBaselineV2) -> Result<()> {
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

fn compare_baseline(baseline: &CompileBaseline, report: &RunReport) -> BaselineComparison {
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
struct SemanticBoundaryKey {
    path: String,
    id: String,
    source_start: usize,
    source_end: usize,
}

fn semantic_boundary_key(boundary: &ObservedSemanticBoundary) -> SemanticBoundaryKey {
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

fn validate_report_bucket_shape(report: &RunReport) -> Vec<BaselineViolation> {
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

struct BuildSmokeReportInput<'a> {
    config: &'a SmokeConfig,
    modes: &'a [HarnessMode],
    discovery: &'a DiscoveryReport,
    discovery_path: &'a Path,
    parse_path: Option<&'a Path>,
    parse_report: Option<&'a RunReport>,
    compile_path: Option<&'a Path>,
    compile_report: Option<&'a RunReport>,
    gap_map_path: &'a Path,
}

fn build_smoke_report(input: BuildSmokeReportInput<'_>) -> SmokeReport {
    let mut structural_failures = Vec::new();
    let modes_requested = input.modes.to_vec();

    if modes_requested.contains(&HarnessMode::Parse) {
        collect_smoke_report_failures(
            HarnessMode::Parse,
            input.config.profile,
            input.parse_report,
            &mut structural_failures,
        );
    }
    if modes_requested.contains(&HarnessMode::Compile) {
        collect_smoke_report_failures(
            HarnessMode::Compile,
            input.config.profile,
            input.compile_report,
            &mut structural_failures,
        );
    }

    let status = if structural_failures.is_empty() { SmokeStatus::Pass } else { SmokeStatus::Fail };

    SmokeReport {
        schema_version: SMOKE_SCHEMA_VERSION.to_string(),
        timestamp: Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        repo_commit: current_commit(),
        perl_requested_ref: input
            .config
            .perl_ref
            .clone()
            .unwrap_or_else(|| input.discovery.perl_ref.clone()),
        perl_resolved_ref: input.discovery.perl_ref.clone(),
        prepared_tree: input.discovery.prepared_tree.clone(),
        host_perl: input.discovery.host_perl.clone(),
        runner: input.config.runner,
        profile: input.config.profile,
        modes_requested,
        discovery_report: path_to_string(input.discovery_path),
        parse_report: input.parse_path.map(path_to_string),
        compile_report: input.compile_path.map(path_to_string),
        gap_map: path_to_string(input.gap_map_path),
        discovery_total: input.discovery.tests.len(),
        parse_files_total: input.parse_report.map(|report| report.summary.files_total),
        parse_files_passed: input.parse_report.map(|report| report.summary.files_passed),
        parse_files_failed: input.parse_report.map(|report| report.summary.files_failed),
        compile_files_total: input.compile_report.map(|report| report.summary.files_total),
        compile_files_passed: input.compile_report.map(|report| report.summary.files_passed),
        compile_files_failed: input.compile_report.map(|report| report.summary.files_failed),
        parse_buckets: input.parse_report.map(|report| report.buckets.clone()),
        compile_buckets: input.compile_report.map(|report| report.buckets.clone()),
        status,
        structural_failures,
    }
}

fn normalized_smoke_modes(modes: &[HarnessMode]) -> Result<Vec<HarnessMode>> {
    let selected = if modes.is_empty() {
        vec![HarnessMode::Parse, HarnessMode::Compile]
    } else {
        modes.to_vec()
    };
    let mut normalized = Vec::new();
    for mode in selected {
        if mode == HarnessMode::Execute {
            bail!("perl-core-harness smoke does not support execute mode");
        }
        if !normalized.contains(&mode) {
            normalized.push(mode);
        }
    }
    if normalized.is_empty() {
        bail!("perl-core-harness smoke needs at least one mode");
    }
    Ok(normalized)
}

fn build_gap_map(
    profile: HarnessProfile,
    modes: &[HarnessMode],
    parse_report: Option<&RunReport>,
    compile_report: Option<&RunReport>,
) -> GapMap {
    let mut total_files = 0usize;
    let mut passed_files = 0usize;
    let mut failed_files = 0usize;
    let mut buckets = BTreeMap::new();
    let mut files_by_bucket: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut first_failure_by_bucket = BTreeMap::new();
    let mut workstreams = BTreeMap::new();
    let mut lsp_impact = BTreeMap::new();

    for report in [parse_report, compile_report].into_iter().flatten() {
        total_files = total_files.saturating_add(report.summary.files_total);
        passed_files = passed_files.saturating_add(report.summary.files_passed);
        failed_files = failed_files.saturating_add(report.summary.files_failed);
        for (bucket, count) in &report.buckets {
            *buckets.entry(bucket.clone()).or_insert(0) += count;
        }
        for failure in &report.failures {
            files_by_bucket.entry(failure.bucket.clone()).or_default().push(failure.path.clone());
            first_failure_by_bucket
                .entry(failure.bucket.clone())
                .or_insert_with(|| failure.clone());
            *workstreams.entry(failure.workstream.clone()).or_insert(0) += 1;
            for impact in &failure.lsp_impact {
                *lsp_impact.entry(impact.clone()).or_insert(0) += 1;
            }
        }
    }
    for paths in files_by_bucket.values_mut() {
        paths.sort();
        paths.dedup();
    }

    GapMap {
        schema_version: GAP_MAP_SCHEMA_VERSION.to_string(),
        profile,
        mode: modes.iter().map(|mode| mode.as_str()).collect::<Vec<_>>().join(","),
        total_files,
        passed_files,
        failed_files,
        buckets,
        files_by_bucket,
        first_failure_by_bucket,
        workstreams,
        lsp_impact,
        top_parse_failures: top_failures(parse_report),
        top_compile_failures: top_failures(compile_report),
    }
}

fn top_failures(report: Option<&RunReport>) -> Vec<RunFailure> {
    report.map(|report| report.failures.iter().take(10).cloned().collect()).unwrap_or_default()
}

fn collect_smoke_report_failures(
    expected_mode: HarnessMode,
    expected_profile: HarnessProfile,
    report: Option<&RunReport>,
    failures: &mut Vec<SmokeStructuralFailure>,
) {
    let Some(report) = report else {
        failures.push(SmokeStructuralFailure {
            mode: Some(expected_mode),
            path: None,
            kind: SmokeFailureKind::MissingReport,
            message: format!("{expected_mode} report was not written"),
        });
        return;
    };

    if report.mode != expected_mode {
        failures.push(SmokeStructuralFailure {
            mode: Some(expected_mode),
            path: None,
            kind: SmokeFailureKind::ProfileMismatch,
            message: format!(
                "report mode {} does not match requested {expected_mode}",
                report.mode
            ),
        });
    }
    if report.profile != expected_profile {
        failures.push(SmokeStructuralFailure {
            mode: Some(expected_mode),
            path: None,
            kind: SmokeFailureKind::ProfileMismatch,
            message: format!(
                "report profile {} does not match requested {expected_profile}",
                report.profile
            ),
        });
    }

    for violation in validate_report_bucket_shape(report) {
        let kind = match violation.kind {
            BaselineViolationKind::UnknownBucket => SmokeFailureKind::UnknownBucket,
            BaselineViolationKind::UnbucketedFailure => SmokeFailureKind::UnbucketedFailure,
            _ => continue,
        };
        failures.push(SmokeStructuralFailure {
            mode: Some(expected_mode),
            path: violation.path,
            kind,
            message: violation.message,
        });
    }
    for violation in validate_semantic_boundary_shape(report) {
        failures.push(SmokeStructuralFailure {
            mode: Some(expected_mode),
            path: violation.path,
            kind: SmokeFailureKind::SemanticBoundary,
            message: violation.message,
        });
    }
}

fn validate_semantic_boundary_shape(report: &RunReport) -> Vec<BaselineViolation> {
    validate_semantic_boundary_inventory(&report.semantic_boundaries)
}

fn validate_semantic_boundary_inventory(
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

fn validate_accepted_semantic_boundary_inventory(
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

fn path_to_string(path: &Path) -> String {
    path.display().to_string()
}

fn prepare_run_copy(
    perl_tree: &Path,
    runner: HarnessRunner,
    mode: HarnessMode,
    profile: HarnessProfile,
) -> Result<PathBuf> {
    let nonce = RUN_COPY_COUNTER.fetch_add(1, Ordering::Relaxed);
    let run_tree = project_root()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("target")
        .join("perl-core")
        .join("runs")
        .join(format!("{runner}-{mode}-{profile}-{}-{nonce}", std::process::id()));
    if run_tree.exists() {
        fs::remove_dir_all(&run_tree)
            .with_context(|| format!("removing prior run tree {}", run_tree.display()))?;
    }
    copy_dir_all(perl_tree, &run_tree)?;
    Ok(run_tree)
}

fn copy_dir_all(source: &Path, destination: &Path) -> Result<()> {
    copy_dir_all_filtered(source, destination, &|_| false)
}

fn copy_dir_all_filtered(
    source: &Path,
    destination: &Path,
    skip: &dyn Fn(&Path) -> bool,
) -> Result<()> {
    let create_context = format!("creating directory {}", destination.display());
    fs::create_dir_all(destination).context(create_context)?;
    let read_context = format!("reading {}", source.display());
    for entry in fs::read_dir(source).context(read_context)? {
        let entry_context = format!("reading entry in {}", source.display());
        let entry = entry.context(entry_context)?;
        let entry_path = entry.path();
        if skip(&entry_path) {
            continue;
        }
        let type_context = format!("reading file type for {}", entry_path.display());
        let ty = entry.file_type().context(type_context)?;
        let child_destination = destination.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_all_filtered(&entry_path, &child_destination, skip)?;
        } else if ty.is_file() {
            let copy_context =
                format!("copying {} to {}", entry_path.display(), child_destination.display());
            fs::copy(entry_path, &child_destination).context(copy_context)?;
        }
    }
    Ok(())
}

fn install_t_perl_wrapper(run_tree: &Path) -> Result<()> {
    let source = project_root()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("scripts")
        .join("perl-core")
        .join("t-perl-wrapper.sh");
    let destination = run_tree.join("t").join("perl");
    let context =
        format!("installing t/perl wrapper {} -> {}", source.display(), destination.display());
    fs::copy(&source, &destination).context(context)?;
    set_executable(&destination)?;
    Ok(())
}

#[cfg(unix)]
fn set_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(path)
        .with_context(|| format!("reading permissions for {}", path.display()))?
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)
        .with_context(|| format!("setting executable bit on {}", path.display()))
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) -> Result<()> {
    Ok(())
}

fn resolve_runner_binary(configured: Option<&Path>) -> Result<PathBuf> {
    if let Some(path) = configured {
        if !path.is_file() {
            bail!("runner binary does not exist: {}", path.display());
        }
        let context = format!("canonicalizing {}", path.display());
        return path.canonicalize().context(context);
    }

    let root = project_root().unwrap_or_else(|_| PathBuf::from("."));
    let binary_name =
        if cfg!(windows) { "perl-core-test-runner.exe" } else { "perl-core-test-runner" };
    let binary = root.join("target").join("agent").join(binary_name);
    if binary.is_file() {
        let context = format!("canonicalizing {}", binary.display());
        return binary.canonicalize().context(context);
    }

    let status = Command::new("cargo")
        .current_dir(&root)
        .args(["build", "-p", "perl-core-test-runner", "--profile", "agent", "--locked"])
        .status()
        .context("building perl-core-test-runner")?;
    if !status.success() {
        bail!("cargo build -p perl-core-test-runner failed with status {status}");
    }
    if !binary.is_file() {
        bail!("runner build succeeded but binary was not found at {}", binary.display());
    }
    let context = format!("canonicalizing {}", binary.display());
    binary.canonicalize().context(context)
}

fn read_runner_records(path: &Path) -> Result<Vec<RunnerRecord>> {
    if !path.is_file() {
        bail!("runner context was not written: {}", path.display());
    }
    let raw = fs::read_to_string(path)
        .with_context(|| format!("reading runner context {}", path.display()))?;
    let mut records = Vec::new();
    for (index, line) in raw.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let record = serde_json::from_str(trimmed).with_context(|| {
            format!("decoding runner record {} in {}", index + 1, path.display())
        })?;
        records.push(record);
    }
    if records.is_empty() {
        bail!("runner context contained no records: {}", path.display());
    }
    Ok(records)
}

fn read_runner_records_or_empty(path: &Path) -> Result<Vec<RunnerRecord>> {
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let raw = fs::read_to_string(path)
        .with_context(|| format!("reading runner context {}", path.display()))?;
    if raw.lines().all(|line| line.trim().is_empty()) {
        return Ok(Vec::new());
    }
    read_runner_records(path)
}

struct BuildRunReportInput<'a> {
    config: &'a RunConfig,
    perl_tree: &'a Path,
    run_tree: &'a Path,
    discovered: &'a [DiscoveredTest],
    records: &'a [RunnerRecord],
    harness_status: Option<i32>,
}

fn build_run_report(input: BuildRunReportInput<'_>) -> RunReport {
    let records_by_path = input
        .records
        .iter()
        .filter_map(|record| normalize_test_path(&record.path).map(|path| (path, record)))
        .collect::<BTreeMap<_, _>>();
    let mut file_results = Vec::new();
    let mut failures = Vec::new();
    let mut buckets = BTreeMap::new();
    let mut assertions_total = 0usize;
    let mut assertions_passed = 0usize;

    for test in input.discovered {
        match records_by_path.get(&test.path) {
            Some(record) => {
                assertions_total = assertions_total.saturating_add(record.assertions_total);
                assertions_passed = assertions_passed.saturating_add(record.assertions_passed);
                file_results.push(RunFileResult {
                    path: test.path.clone(),
                    status: record.status,
                    assertions_passed: record.assertions_passed,
                    assertions_total: record.assertions_total,
                });
                if record.status == RunnerStatus::Fail {
                    let bucket = record.bucket.clone().unwrap_or_else(|| "unknown".to_string());
                    *buckets.entry(bucket.clone()).or_insert(0) += 1;
                    failures.push(failure_for_record(&test.path, &bucket, record));
                }
            }
            None => {
                assertions_total = assertions_total.saturating_add(1);
                file_results.push(RunFileResult {
                    path: test.path.clone(),
                    status: RunnerStatus::Fail,
                    assertions_passed: 0,
                    assertions_total: 1,
                });
                let bucket = "harness_prepare".to_string();
                *buckets.entry(bucket.clone()).or_insert(0) += 1;
                failures.push(RunFailure {
                    path: test.path.clone(),
                    phase: input.config.mode.as_str().to_string(),
                    bucket,
                    first_diagnostic: "test was discovered but produced no runner record"
                        .to_string(),
                    workstream: "harness_integration".to_string(),
                    lsp_impact: vec!["compiler_conformance".to_string()],
                });
            }
        }
    }

    let files_failed =
        file_results.iter().filter(|result| result.status == RunnerStatus::Fail).count();
    let files_total = file_results.len();
    let files_passed = files_total.saturating_sub(files_failed);
    let semantic_boundaries = input
        .discovered
        .iter()
        .filter_map(|test| {
            let path = normalize_test_path(&test.path)?;
            let record = records_by_path.get(&path)?;
            Some((path, *record))
        })
        .flat_map(|(path, record)| {
            record.semantic_boundaries.iter().map(move |boundary| ObservedSemanticBoundary {
                path: path.clone(),
                id: boundary.id.clone(),
                disposition: boundary.disposition,
                reason: boundary.reason.clone(),
                source_span: boundary.source_span,
                source_kind: boundary.source_kind.clone(),
                confidence: boundary.confidence,
                blocks_compilation: boundary.blocks_compilation,
                blocks_downstream_static_facts: boundary.blocks_downstream_static_facts,
                lock_scope: boundary.lock_scope,
                owner_workstream: boundary.owner_workstream.clone(),
                supporting_test: boundary.supporting_test.clone(),
            })
        })
        .collect::<Vec<_>>();

    RunReport {
        schema_version: RUN_REPORT_SCHEMA_VERSION.to_string(),
        commit: current_commit(),
        timestamp: Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        perl_ref: perl_tree_ref(input.perl_tree),
        prepared_tree: input.perl_tree.display().to_string(),
        run_tree: input.run_tree.display().to_string(),
        host_perl: input.config.host_perl.display().to_string(),
        runner: input.config.runner,
        mode: input.config.mode,
        profile: input.config.profile,
        harness_status: input.harness_status,
        summary: RunSummary {
            files_total,
            files_passed,
            files_failed,
            tap_assertions_total: assertions_total,
            tap_assertions_passed: assertions_passed,
        },
        buckets,
        file_results,
        failures,
        semantic_boundaries,
    }
}

fn failure_for_record(path: &str, bucket: &str, record: &RunnerRecord) -> RunFailure {
    RunFailure {
        path: path.to_string(),
        phase: record.mode.clone(),
        bucket: bucket.to_string(),
        first_diagnostic: record
            .first_diagnostic
            .clone()
            .unwrap_or_else(|| "runner reported failure without diagnostic".to_string()),
        workstream: workstream_for_bucket(bucket).to_string(),
        lsp_impact: lsp_impact_for_bucket(bucket).into_iter().map(ToString::to_string).collect(),
    }
}

fn current_commit() -> String {
    Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

fn perl_tree_ref(perl_tree: &Path) -> String {
    let Ok(top_level) = Command::new("git")
        .arg("-C")
        .arg(perl_tree)
        .args(["rev-parse", "--show-toplevel"])
        .output()
    else {
        return "unknown".to_string();
    };
    if !top_level.status.success() {
        return "unknown".to_string();
    }
    let Ok(top_level_text) = String::from_utf8(top_level.stdout) else {
        return "unknown".to_string();
    };
    let top_level_path = PathBuf::from(top_level_text.trim());
    let Ok(top_level_path) = top_level_path.canonicalize() else {
        return "unknown".to_string();
    };
    let Ok(perl_tree_path) = perl_tree.canonicalize() else {
        return "unknown".to_string();
    };
    if top_level_path != perl_tree_path {
        return "unknown".to_string();
    }

    Command::new("git")
        .arg("-C")
        .arg(perl_tree)
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::series::{build_series_manifest, write_series_manifest};
    use perl_core_harness_types::{
        FailureClusterHistoryTransition, SERIES_MANIFEST_NORMALIZATION_VERSION,
        SERIES_MANIFEST_SCHEMA_VERSION,
    };
    use perl_core_harness_types::{
        SemanticBoundaryConfidence, SemanticBoundaryDisposition, SemanticBoundaryLockScope,
        SemanticBoundaryRecord, SemanticBoundarySourceSpan,
    };

    type TestResult<T = ()> = Result<T>;

    /// Parity: the declared-path validator must not be weaker than the
    /// structural classifier. If a form is host-path material anywhere in a
    /// public string, a declared path field carrying that same value has to be
    /// rejected too, or the two publication surfaces disagree about what is
    /// publishable.
    #[test]
    fn declared_path_validation_is_not_weaker_than_structural_classification() {
        for value in [
            "/etc/passwd",
            "/home/runner/work/",
            "///etc/passwd",
            "//etc/hostname",
            r"C:\Users\runner\repo",
            "C:/Users/runner/repo",
            r"\\server\share\repo",
            r"\\?\C:\Users\runner",
            r"\??\C:\Users\runner",
            "file:///tmp/repo/report.json",
        ] {
            assert!(
                public_evidence::classify_public_string(value, PublicStringClass::Ordinary)
                    .is_some(),
                "structural classifier must reject {value}"
            );
            assert!(
                validate_public_path(value, "parity probe").is_err(),
                "declared-path validator must also reject {value}"
            );
        }
    }

    /// Both validators must keep publishing the repository-relative paths the
    /// harness legitimately writes into its receipts.
    #[test]
    fn public_validators_agree_on_repository_relative_paths() -> TestResult {
        for value in ["crates/perl-parser/src/lib.rs", "smoke/base/smoke.json", "base/if.t"] {
            assert_eq!(
                public_evidence::classify_public_string(value, PublicStringClass::Ordinary),
                None,
                "structural classifier must accept {value}"
            );
            validate_public_path(value, "parity probe")?;
        }
        Ok(())
    }

    /// #6882 acceptance: a public failure message must not republish the
    /// private path it rejected.
    #[test]
    fn declared_path_rejection_does_not_echo_the_private_value() {
        let Err(error) = validate_public_path("/home/runner/work/private", "failure path") else {
            unreachable!("absolute host path must be rejected")
        };
        let rendered = error.to_string();
        assert!(rendered.contains("failure path"), "label missing: {rendered}");
        assert!(rendered.contains("unix_absolute"), "classification missing: {rendered}");
        assert!(!rendered.contains("/home/runner"), "message echoed the private path: {rendered}");
    }

    #[test]
    fn parses_dumptests_paths_and_ignores_noise() -> TestResult {
        let output = b"base/if.t\n# note from harness\n./base/lex.t\n t/base/term.t \n";

        let tests = parse_dumptests_output(output)?;

        assert_eq!(
            tests,
            vec![
                DiscoveredTest { path: "base/if.t".into(), root: "base".into() },
                DiscoveredTest { path: "base/lex.t".into(), root: "base".into() },
                DiscoveredTest { path: "base/term.t".into(), root: "base".into() },
            ]
        );
        Ok(())
    }

    #[test]
    fn parses_windows_style_paths() -> TestResult {
        let tests = parse_dumptests_output(b"base\\if.t\n")?;

        assert_eq!(tests[0].path, "base/if.t");
        assert_eq!(tests[0].root, "base");
        Ok(())
    }

    #[test]
    fn deduplicates_normalized_dumptests_paths() -> TestResult {
        let output = b"base/if.t\n./base/if.t\nt/base/if.t\nbase\\if.t\nbase/lex.t\n";

        let tests = parse_dumptests_output(output)?;

        assert_eq!(
            tests,
            vec![
                DiscoveredTest { path: "base/if.t".into(), root: "base".into() },
                DiscoveredTest { path: "base/lex.t".into(), root: "base".into() },
            ]
        );
        Ok(())
    }

    #[test]
    fn rejects_empty_dumptests_output() -> TestResult {
        let Err(err) = parse_dumptests_output(b"no tests here\n") else {
            bail!("empty output should fail");
        };

        assert!(err.to_string().contains("no .t files"));
        Ok(())
    }

    #[test]
    fn runner_names_match_upstream_scripts_and_receipt_values() {
        assert_eq!(HarnessRunner::Test.script_name(), "TEST");
        assert_eq!(HarnessRunner::Test.as_str(), "test");
        assert_eq!(HarnessRunner::Harness.script_name(), "harness");
        assert_eq!(HarnessRunner::Harness.as_str(), "harness");
    }

    #[test]
    fn mode_and_profile_names_match_cli_and_receipt_values() {
        let modes = [
            (HarnessMode::Parse, "parse"),
            (HarnessMode::Compile, "compile"),
            (HarnessMode::Execute, "execute"),
        ];
        for (mode, expected) in modes {
            assert_eq!(mode.as_str(), expected);
            assert_eq!(mode.to_string(), expected);
        }

        let profiles = [
            (HarnessProfile::Base, "base"),
            (HarnessProfile::Comp, "comp"),
            (HarnessProfile::Run, "run"),
            (HarnessProfile::Core, "core"),
            (HarnessProfile::Lib, "lib"),
            (HarnessProfile::Full, "full"),
        ];
        for (profile, expected) in profiles {
            assert_eq!(profile.as_str(), expected);
            assert_eq!(profile.to_string(), expected);
            assert!(!profile.roots().is_empty(), "{expected} profile should have roots");
        }
    }

    #[test]
    fn profile_base_expands_test_runner_args_to_explicit_files() -> TestResult {
        let temp = tempfile::tempdir()?;
        let t_dir = temp.path().join("t");
        fs::create_dir_all(t_dir.join("base").join("nested"))?;
        fs::write(t_dir.join("base").join("ok.t"), "1;\n")?;
        fs::write(t_dir.join("base").join("nested").join("deep.t"), "1;\n")?;
        fs::write(t_dir.join("base").join("README"), "not a test\n")?;

        let args = profile_runner_args(HarnessProfile::Base, &t_dir, HarnessRunner::Test)?;

        assert_eq!(args, vec!["base/nested/deep.t", "base/ok.t"]);
        Ok(())
    }

    #[test]
    fn profile_base_uses_glob_for_tap_harness_runner() -> TestResult {
        let temp = tempfile::tempdir()?;
        let args = profile_runner_args(
            HarnessProfile::Base,
            &temp.path().join("t"),
            HarnessRunner::Harness,
        )?;

        assert_eq!(args, vec!["base/*.t"]);
        Ok(())
    }

    #[test]
    fn execute_mode_requires_explicit_selected_tests() -> TestResult {
        let config = RunConfig {
            perl_tree: PathBuf::from("unused"),
            host_perl: PathBuf::from("perl"),
            runner: HarnessRunner::Test,
            mode: HarnessMode::Execute,
            profile: HarnessProfile::Base,
            tests: Vec::new(),
            output: None,
            runner_binary: None,
        };

        let Err(err) = run_mode(config) else {
            bail!("execute mode without a selected test should fail");
        };

        assert!(err.to_string().contains("requires one or more explicit --test"));
        Ok(())
    }

    #[test]
    fn execute_mode_rejects_non_allowlisted_test_selection() -> TestResult {
        let config = RunConfig {
            perl_tree: PathBuf::from("unused"),
            host_perl: PathBuf::from("perl"),
            runner: HarnessRunner::Test,
            mode: HarnessMode::Execute,
            profile: HarnessProfile::Base,
            tests: vec!["base/rs.t".into()],
            output: None,
            runner_binary: None,
        };

        let Err(err) = run_mode(config) else {
            bail!("execute mode should reject non-allowlisted tests");
        };

        assert!(err.to_string().contains("supports only selected base tests"));
        assert!(err.to_string().contains("base/if.t"));
        assert!(err.to_string().contains("base/cond.t"));
        assert!(err.to_string().contains("base/num.t"));
        assert!(err.to_string().contains("base/pat.t"));
        assert!(err.to_string().contains("base/translate.t"));
        assert!(err.to_string().contains("base/while.t"));
        Ok(())
    }

    #[test]
    fn run_report_buckets_runner_records_and_missing_files() -> TestResult {
        let temp = tempfile::tempdir()?;
        let config = RunConfig {
            perl_tree: temp.path().join("perl"),
            host_perl: PathBuf::from("perl"),
            runner: HarnessRunner::Test,
            mode: HarnessMode::Parse,
            profile: HarnessProfile::Base,
            tests: Vec::new(),
            output: None,
            runner_binary: Some(PathBuf::from("runner")),
        };
        let discovered = vec![
            DiscoveredTest { path: "base/ok.t".into(), root: "base".into() },
            DiscoveredTest { path: "base/bad.t".into(), root: "base".into() },
            DiscoveredTest { path: "base/missing.t".into(), root: "base".into() },
        ];
        let records = vec![
            RunnerRecord {
                schema_version: "perl_core_harness.runner_record.v1".into(),
                mode: "parse".into(),
                path: "base/ok.t".into(),
                status: RunnerStatus::Pass,
                assertions_passed: 1,
                assertions_total: 1,
                bucket: None,
                first_diagnostic: None,
                semantic_boundaries: vec![SemanticBoundaryRecord {
                    id: "runtime_symbolic_reference".into(),
                    disposition: SemanticBoundaryDisposition::DeferredRuntime,
                    reason: "symbolic reference dereference is deferred to runtime".into(),
                    source_span: SemanticBoundarySourceSpan { start: 12, end: 28 },
                    source_kind: "SymbolicReferenceDeref".into(),
                    confidence: SemanticBoundaryConfidence::Conservative,
                    blocks_compilation: false,
                    blocks_downstream_static_facts: true,
                    lock_scope: SemanticBoundaryLockScope::None,
                    owner_workstream: "symbolic_reference_semantics".into(),
                    supporting_test: "base/ok.t".into(),
                }],
            },
            RunnerRecord {
                schema_version: "perl_core_harness.runner_record.v1".into(),
                mode: "parse".into(),
                path: "base/bad.t".into(),
                status: RunnerStatus::Fail,
                assertions_passed: 0,
                assertions_total: 1,
                bucket: Some("parse_recovery".into()),
                first_diagnostic: Some("expected expression".into()),
                semantic_boundaries: Vec::new(),
            },
        ];
        let run_tree = temp.path().join("run");

        let report = build_run_report(BuildRunReportInput {
            config: &config,
            perl_tree: temp.path(),
            run_tree: &run_tree,
            discovered: &discovered,
            records: &records,
            harness_status: Some(1),
        });

        assert_eq!(report.summary.files_total, 3);
        assert_eq!(report.summary.files_passed, 1);
        assert_eq!(report.summary.files_failed, 2);
        assert_eq!(report.buckets.get("parse_recovery"), Some(&1));
        assert_eq!(report.buckets.get("harness_prepare"), Some(&1));
        assert_eq!(report.failures.len(), 2);
        assert!(report.failures.iter().any(|failure| failure.path == "base/bad.t"));
        assert!(report.failures.iter().any(|failure| failure.path == "base/missing.t"));
        assert_eq!(report.semantic_boundaries.len(), 1);
        assert_eq!(report.semantic_boundaries[0].path, "base/ok.t");
        assert_eq!(
            report.semantic_boundaries[0].disposition,
            SemanticBoundaryDisposition::DeferredRuntime
        );
        assert_eq!(report.semantic_boundaries[0].source_span.start, 12);
        assert_eq!(report.semantic_boundaries[0].owner_workstream, "symbolic_reference_semantics");
        Ok(())
    }

    #[test]
    fn run_report_scopes_and_deduplicates_semantic_boundaries() -> TestResult {
        let temp = tempfile::tempdir()?;
        let config = RunConfig {
            perl_tree: temp.path().join("perl"),
            host_perl: PathBuf::from("perl"),
            runner: HarnessRunner::Test,
            mode: HarnessMode::Compile,
            profile: HarnessProfile::Base,
            tests: Vec::new(),
            output: None,
            runner_binary: Some(PathBuf::from("runner")),
        };
        let discovered = vec![DiscoveredTest { path: "base/ok.t".into(), root: "base".into() }];
        let boundary = SemanticBoundaryRecord {
            id: "runtime_symbolic_reference".into(),
            disposition: SemanticBoundaryDisposition::DeferredRuntime,
            reason: "symbolic reference dereference is deferred to runtime".into(),
            source_span: SemanticBoundarySourceSpan { start: 12, end: 28 },
            source_kind: "SymbolicReferenceDeref".into(),
            confidence: SemanticBoundaryConfidence::Conservative,
            blocks_compilation: false,
            blocks_downstream_static_facts: true,
            lock_scope: SemanticBoundaryLockScope::None,
            owner_workstream: "symbolic_reference_semantics".into(),
            supporting_test: "base/ok.t".into(),
        };
        let record = |path: &str| RunnerRecord {
            schema_version: "perl_core_harness.runner_record.v1".into(),
            mode: "compile".into(),
            path: path.into(),
            status: RunnerStatus::Pass,
            assertions_passed: 1,
            assertions_total: 1,
            bucket: None,
            first_diagnostic: None,
            semantic_boundaries: vec![boundary.clone()],
        };
        let records = vec![record("base/ok.t"), record("base/ok.t"), record("stale.t")];

        let report = build_run_report(BuildRunReportInput {
            config: &config,
            perl_tree: temp.path(),
            run_tree: &temp.path().join("run"),
            discovered: &discovered,
            records: &records,
            harness_status: Some(0),
        });

        assert_eq!(report.semantic_boundaries.len(), 1);
        assert_eq!(report.semantic_boundaries[0].path, "base/ok.t");
        Ok(())
    }

    #[test]
    fn discovery_report_schema_roundtrips() -> TestResult {
        let report = DiscoveryReport {
            schema_version: DISCOVERY_SCHEMA_VERSION.into(),
            commit: "abc".into(),
            timestamp: "2026-07-02T00:00:00Z".into(),
            perl_ref: "perl-ref".into(),
            prepared_tree: "/tmp/perl".into(),
            host_perl: "perl".into(),
            runner: HarnessRunner::Test,
            profile: HarnessProfile::Base,
            tests: vec![DiscoveredTest { path: "base/if.t".into(), root: "base".into() }],
        };

        let json = serde_json::to_string(&report)?;
        let back: DiscoveryReport = serde_json::from_str(&json)?;

        assert_eq!(back, report);
        Ok(())
    }

    #[test]
    fn compile_run_report_schema_roundtrips() -> TestResult {
        let report = RunReport {
            schema_version: RUN_REPORT_SCHEMA_VERSION.into(),
            commit: "abc".into(),
            timestamp: "2026-07-02T00:00:00Z".into(),
            perl_ref: "perl-ref".into(),
            prepared_tree: "/tmp/perl".into(),
            run_tree: "/tmp/run".into(),
            host_perl: "perl".into(),
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
                path: "base/ok.t".into(),
                status: RunnerStatus::Pass,
                assertions_passed: 1,
                assertions_total: 1,
            }],
            failures: Vec::new(),
            semantic_boundaries: Vec::new(),
        };

        let json = serde_json::to_string(&report)?;
        let back: RunReport = serde_json::from_str(&json)?;

        assert_eq!(back, report);
        Ok(())
    }

    #[test]
    fn smoke_report_schema_roundtrips() -> TestResult {
        let discovery = sample_discovery_report();
        let parse = sample_parse_report();
        let compile = sample_compile_report();
        let smoke = build_smoke_report(BuildSmokeReportInput {
            config: &sample_smoke_config(vec![HarnessMode::Parse, HarnessMode::Compile]),
            discovery: &discovery,
            modes: &[HarnessMode::Parse, HarnessMode::Compile],
            discovery_path: Path::new("target/perl-core/smoke/base/discovery.json"),
            parse_path: Some(Path::new("target/perl-core/smoke/base/parse.json")),
            parse_report: Some(&parse),
            compile_path: Some(Path::new("target/perl-core/smoke/base/compile.json")),
            compile_report: Some(&compile),
            gap_map_path: Path::new("target/perl-core/smoke/base/gap-map.json"),
        });

        let json = serde_json::to_string(&smoke)?;
        let back: SmokeReport = serde_json::from_str(&json)?;

        assert_eq!(back, smoke);
        assert_eq!(back.schema_version, SMOKE_SCHEMA_VERSION);
        assert_eq!(back.status, SmokeStatus::Pass);
        assert_eq!(back.discovery_total, 2);
        assert_eq!(back.parse_files_total, Some(2));
        assert_eq!(back.compile_files_total, Some(2));
        Ok(())
    }

    #[test]
    fn smoke_summary_records_bucketed_parse_failures_without_structural_failure() -> TestResult {
        let discovery = sample_discovery_report();
        let mut parse = sample_parse_report();
        mark_file_failed(&mut parse, "base/ok.t", "parse_recovery");

        let smoke = build_smoke_report(BuildSmokeReportInput {
            config: &sample_smoke_config(vec![HarnessMode::Parse]),
            discovery: &discovery,
            modes: &[HarnessMode::Parse],
            discovery_path: Path::new("discovery.json"),
            parse_path: Some(Path::new("parse.json")),
            parse_report: Some(&parse),
            compile_path: None,
            compile_report: None,
            gap_map_path: Path::new("gap-map.json"),
        });

        assert_eq!(smoke.status, SmokeStatus::Pass);
        assert_eq!(smoke.parse_files_failed, Some(1));
        assert_eq!(
            smoke.parse_buckets.as_ref().and_then(|buckets| buckets.get("parse_recovery")),
            Some(&1)
        );
        assert!(smoke.structural_failures.is_empty());
        Ok(())
    }

    #[test]
    fn smoke_summary_records_bucketed_compile_failures_without_structural_failure() -> TestResult {
        let discovery = sample_discovery_report();
        let mut compile = sample_compile_report();
        mark_file_failed(&mut compile, "base/lex.t", "compile_effect");

        let smoke = build_smoke_report(BuildSmokeReportInput {
            config: &sample_smoke_config(vec![HarnessMode::Compile]),
            discovery: &discovery,
            modes: &[HarnessMode::Compile],
            discovery_path: Path::new("discovery.json"),
            parse_path: None,
            parse_report: None,
            compile_path: Some(Path::new("compile.json")),
            compile_report: Some(&compile),
            gap_map_path: Path::new("gap-map.json"),
        });

        assert_eq!(smoke.status, SmokeStatus::Pass);
        assert_eq!(smoke.compile_files_failed, Some(1));
        assert_eq!(
            smoke.compile_buckets.as_ref().and_then(|buckets| buckets.get("compile_effect")),
            Some(&1)
        );
        assert!(smoke.structural_failures.is_empty());
        Ok(())
    }

    #[test]
    fn smoke_summary_fails_on_unknown_bucket() -> TestResult {
        let discovery = sample_discovery_report();
        let mut compile = sample_compile_report();
        mark_file_failed(&mut compile, "base/lex.t", "unknown");

        let smoke = build_smoke_report(BuildSmokeReportInput {
            config: &sample_smoke_config(vec![HarnessMode::Compile]),
            discovery: &discovery,
            modes: &[HarnessMode::Compile],
            discovery_path: Path::new("discovery.json"),
            parse_path: None,
            parse_report: None,
            compile_path: Some(Path::new("compile.json")),
            compile_report: Some(&compile),
            gap_map_path: Path::new("gap-map.json"),
        });

        assert_eq!(smoke.status, SmokeStatus::Fail);
        assert!(smoke.structural_failures.iter().any(|failure| {
            failure.kind == SmokeFailureKind::UnknownBucket
                && failure.mode == Some(HarnessMode::Compile)
                && failure.path.as_deref() == Some("base/lex.t")
        }));
        Ok(())
    }

    #[test]
    fn smoke_summary_fails_on_invalid_semantic_boundary_shape() -> TestResult {
        let discovery = sample_discovery_report();
        let mut compile = sample_compile_report();
        compile.semantic_boundaries.push(ObservedSemanticBoundary {
            path: "base/ok.t".into(),
            id: "source_locked:base/ok.t:PhaseBlock".into(),
            disposition: SemanticBoundaryDisposition::SourceLockedCompatibility,
            reason: "guarded phase probe".into(),
            source_span: SemanticBoundarySourceSpan { start: 8, end: 4 },
            source_kind: "PhaseBlock".into(),
            confidence: SemanticBoundaryConfidence::Conservative,
            blocks_compilation: true,
            blocks_downstream_static_facts: true,
            lock_scope: SemanticBoundaryLockScope::None,
            owner_workstream: "source_locked_compatibility".into(),
            supporting_test: "base/ok.t".into(),
        });

        let smoke = build_smoke_report(BuildSmokeReportInput {
            config: &sample_smoke_config(vec![HarnessMode::Compile]),
            discovery: &discovery,
            modes: &[HarnessMode::Compile],
            discovery_path: Path::new("discovery.json"),
            parse_path: None,
            parse_report: None,
            compile_path: Some(Path::new("compile.json")),
            compile_report: Some(&compile),
            gap_map_path: Path::new("gap-map.json"),
        });

        assert_eq!(smoke.status, SmokeStatus::Fail);
        assert!(smoke.structural_failures.iter().any(|failure| {
            failure.kind == SmokeFailureKind::SemanticBoundary
                && failure.path.as_deref() == Some("base/ok.t")
                && failure.message.contains("path_and_source")
        }));
        Ok(())
    }

    #[test]
    fn smoke_summary_fails_on_unbucketed_failure() -> TestResult {
        let discovery = sample_discovery_report();
        let mut compile = sample_compile_report();
        let Some(result) =
            compile.file_results.iter_mut().find(|result| result.path == "base/lex.t")
        else {
            bail!("sample report missing base/lex.t");
        };
        result.status = RunnerStatus::Fail;
        result.assertions_passed = 0;
        compile.summary.files_passed = 1;
        compile.summary.files_failed = 1;
        compile.summary.tap_assertions_passed = 1;

        let smoke = build_smoke_report(BuildSmokeReportInput {
            config: &sample_smoke_config(vec![HarnessMode::Compile]),
            discovery: &discovery,
            modes: &[HarnessMode::Compile],
            discovery_path: Path::new("discovery.json"),
            parse_path: None,
            parse_report: None,
            compile_path: Some(Path::new("compile.json")),
            compile_report: Some(&compile),
            gap_map_path: Path::new("gap-map.json"),
        });

        assert_eq!(smoke.status, SmokeStatus::Fail);
        assert!(smoke.structural_failures.iter().any(|failure| {
            failure.kind == SmokeFailureKind::UnbucketedFailure
                && failure.path.as_deref() == Some("base/lex.t")
        }));
        Ok(())
    }

    #[test]
    fn smoke_summary_fails_when_requested_report_is_missing() -> TestResult {
        let discovery = sample_discovery_report();
        let smoke = build_smoke_report(BuildSmokeReportInput {
            config: &sample_smoke_config(vec![HarnessMode::Compile]),
            discovery: &discovery,
            modes: &[HarnessMode::Compile],
            discovery_path: Path::new("discovery.json"),
            parse_path: None,
            parse_report: None,
            compile_path: None,
            compile_report: None,
            gap_map_path: Path::new("gap-map.json"),
        });

        assert_eq!(smoke.status, SmokeStatus::Fail);
        assert!(smoke.structural_failures.iter().any(|failure| {
            failure.kind == SmokeFailureKind::MissingReport
                && failure.mode == Some(HarnessMode::Compile)
        }));
        Ok(())
    }

    #[test]
    fn compile_baseline_schema_roundtrips() -> TestResult {
        let baseline = baseline_from_report(&sample_compile_report())?;

        let json = serde_json::to_string(&baseline)?;
        let back: CompileBaseline = serde_json::from_str(&json)?;

        assert_eq!(back, baseline);
        Ok(())
    }

    #[test]
    fn baseline_comparison_rejects_unknown_semantic_boundary() -> TestResult {
        let baseline = baseline_from_report(&sample_compile_report())?;
        let mut report = sample_compile_report();
        report.semantic_boundaries.push(ObservedSemanticBoundary {
            path: "base/ok.t".into(),
            id: "unknown".into(),
            disposition: SemanticBoundaryDisposition::Unknown,
            reason: "classifier did not resolve boundary".into(),
            source_span: SemanticBoundarySourceSpan { start: 0, end: 1 },
            source_kind: "PhaseBlock".into(),
            confidence: SemanticBoundaryConfidence::Unresolved,
            blocks_compilation: true,
            blocks_downstream_static_facts: true,
            lock_scope: SemanticBoundaryLockScope::None,
            owner_workstream: "compile_time_effects".into(),
            supporting_test: "base/ok.t".into(),
        });

        let comparison = compare_baseline(&baseline, &report);
        assert!(
            comparison
                .violations
                .iter()
                .any(|violation| violation.kind == BaselineViolationKind::SemanticBoundary)
        );
        Ok(())
    }

    #[test]
    fn baseline_comparison_rejects_added_classified_semantic_boundary() -> TestResult {
        let baseline = baseline_from_report(&sample_compile_report())?;
        let mut report = sample_compile_report();
        report.semantic_boundaries.push(sample_semantic_boundary());

        let comparison = compare_baseline(&baseline, &report);
        assert!(comparison.violations.iter().any(|violation| {
            violation.kind == BaselineViolationKind::SemanticBoundary
                && violation.message.contains("not accepted")
        }));
        Ok(())
    }

    #[test]
    fn baseline_acceptance_persists_semantic_boundary_inventory() -> TestResult {
        let mut report = sample_compile_report();
        report.semantic_boundaries.push(sample_semantic_boundary());

        let baseline = baseline_from_report(&report)?;

        assert_eq!(baseline.semantic_boundaries, Some(report.semantic_boundaries));
        Ok(())
    }

    #[test]
    fn baseline_comparison_rejects_changed_semantic_boundary_payload() -> TestResult {
        let mut baseline_report = sample_compile_report();
        baseline_report.semantic_boundaries.push(sample_semantic_boundary());
        let baseline = baseline_from_report(&baseline_report)?;
        let mut report = baseline_report;
        report.semantic_boundaries[0].reason = "changed explanation".into();

        let comparison = compare_baseline(&baseline, &report);
        assert!(comparison.violations.iter().any(|violation| {
            violation.kind == BaselineViolationKind::SemanticBoundary
                && violation.message.contains("changed")
        }));
        Ok(())
    }

    #[test]
    fn baseline_comparison_allows_removed_semantic_boundary() -> TestResult {
        let mut baseline_report = sample_compile_report();
        baseline_report.semantic_boundaries.push(sample_semantic_boundary());
        let baseline = baseline_from_report(&baseline_report)?;

        let comparison = compare_baseline(&baseline, &sample_compile_report());

        assert!(comparison.is_clean(), "removing a boundary should be an improvement");
        Ok(())
    }

    #[test]
    fn baseline_comparison_rejects_duplicate_semantic_boundary_keys() -> TestResult {
        let boundary = sample_semantic_boundary();
        let baseline = baseline_from_report(&sample_compile_report())?;
        let mut report = sample_compile_report();
        report.semantic_boundaries = vec![boundary.clone(), boundary];

        let comparison = compare_baseline(&baseline, &report);

        assert!(comparison.violations.iter().any(|violation| {
            violation.kind == BaselineViolationKind::SemanticBoundary
                && violation.message.contains("duplicate")
        }));
        Ok(())
    }

    #[test]
    fn baseline_acceptance_rejects_duplicate_semantic_boundary_keys() -> TestResult {
        let boundary = sample_semantic_boundary();
        let mut report = sample_compile_report();
        report.semantic_boundaries = vec![boundary.clone(), boundary];

        assert!(baseline_from_report(&report).is_err());
        Ok(())
    }

    #[test]
    fn baseline_acceptance_rejects_empty_semantic_boundary_path() -> TestResult {
        let mut report = sample_compile_report();
        let mut boundary = sample_semantic_boundary();
        boundary.path.clear();
        report.semantic_boundaries.push(boundary);

        assert!(baseline_from_report(&report).is_err());
        Ok(())
    }

    #[test]
    fn legacy_compile_baseline_without_boundary_inventory_remains_readable() -> TestResult {
        let baseline = baseline_from_report(&sample_compile_report())?;
        let mut value = serde_json::to_value(&baseline)?;
        let object = value.as_object_mut().ok_or_else(|| {
            color_eyre::eyre::eyre!("compile baseline should serialize as an object")
        })?;
        object.remove("semantic_boundaries");

        let decoded: CompileBaseline = serde_json::from_value(value)?;

        assert!(decoded.semantic_boundaries.is_none());
        Ok(())
    }

    #[test]
    fn comparison_series_normalizes_and_hashes_exact_membership() -> TestResult {
        let mut discovery = sample_discovery_report();
        discovery.tests = vec![
            DiscoveredTest { path: "./base/ok.t".into(), root: "base".into() },
            DiscoveredTest { path: "base\\lex.t".into(), root: "base".into() },
        ];
        let config = sample_series_config();
        let manifest = build_series_manifest(&discovery, &config, "2026-07-02T00:00:00Z".into())?;

        if manifest.normalized_manifest != vec!["base/lex.t", "base/ok.t"] {
            bail!("comparison series did not normalize and sort its file membership");
        }
        if manifest.manifest_hash.is_empty() {
            bail!("comparison series did not produce a manifest hash");
        }
        validate_series_manifest(&manifest)?;
        Ok(())
    }

    #[test]
    fn comparison_series_rejects_duplicate_normalized_membership() -> TestResult {
        let mut discovery = sample_discovery_report();
        discovery.tests.push(DiscoveredTest { path: "./base/ok.t".into(), root: "base".into() });

        let Err(error) = build_series_manifest(&discovery, &sample_series_config(), "now".into())
        else {
            bail!("duplicate normalized membership should fail closed");
        };
        if !error.to_string().contains("duplicate discovered test path") {
            bail!("unexpected duplicate error: {error}");
        }
        Ok(())
    }

    #[test]
    fn comparison_series_write_and_check_roundtrip() -> TestResult {
        let temp = tempfile::tempdir()?;
        let discovery_path = temp.path().join("discovery.json");
        let output_path = temp.path().join("series.json");
        write_discovery_report(&discovery_path, &sample_discovery_report())?;
        let mut config = sample_series_config();
        config.discovery = discovery_path;
        config.output = Some(output_path.clone());

        series_manifest(config.clone())?;
        let written: SeriesManifest = serde_json::from_str(&fs::read_to_string(&output_path)?)?;
        validate_series_manifest(&written)?;

        config.check = true;
        series_manifest(config)?;
        Ok(())
    }

    #[test]
    fn compile_baseline_v2_rejects_an_extra_passing_file() -> TestResult {
        let discovery = sample_discovery_report();
        let series = build_series_manifest(&discovery, &sample_series_config(), "now".into())?;
        let config = sample_baseline_v2_config();
        let baseline =
            baseline_v2_from_report(&sample_compile_report(), &series, &config, None, &[])?;
        let mut report = sample_compile_report();
        report.file_results.push(RunFileResult {
            path: "base/new.t".into(),
            status: RunnerStatus::Pass,
            assertions_passed: 1,
            assertions_total: 1,
        });
        report.summary.files_total = 3;
        report.summary.files_passed = 3;

        let comparison = compare_baseline_v2(&baseline, &report, &series);
        assert_violation(&comparison, BaselineViolationKind::UnexpectedFile);

        let mut drifted_baseline = baseline;
        drifted_baseline.file_membership.push("base/new.t".into());
        drifted_baseline.files_total = 3;
        let drift_comparison =
            compare_baseline_v2(&drifted_baseline, &sample_compile_report(), &series);
        assert_violation(&drift_comparison, BaselineViolationKind::ManifestMismatch);
        Ok(())
    }

    #[test]
    fn comparison_series_hash_includes_transition_metadata() -> TestResult {
        let discovery = sample_discovery_report();
        let first = build_series_manifest(&discovery, &sample_series_config(), "now".into())?;
        let mut replacement_config = sample_series_config();
        replacement_config.series_id = "selected-base-perl-5.42.3".into();
        replacement_config.replaces_series_id = Some(first.series_id.clone());
        replacement_config.change_reason = Some("reviewed denominator correction".into());
        let replacement = build_series_manifest(&discovery, &replacement_config, "now".into())?;
        if first.manifest_hash == replacement.manifest_hash {
            bail!("series replacement metadata must contribute to the manifest hash");
        }
        Ok(())
    }

    #[test]
    fn compile_baseline_v2_binds_identities_to_series() -> TestResult {
        let discovery = sample_discovery_report();
        let series = build_series_manifest(&discovery, &sample_series_config(), "now".into())?;
        let mut config = sample_baseline_v2_config();
        config.compiler_subject_identity = Some("different-compiler".into());
        let Err(error) =
            baseline_v2_from_report(&sample_compile_report(), &series, &config, None, &[])
        else {
            bail!("a baseline subject from a different compiler must fail closed");
        };
        if !error.to_string().contains("comparison series") {
            bail!("unexpected identity mismatch error: {error}");
        }
        Ok(())
    }

    #[test]
    fn compile_baseline_v2_requires_report_shape_validation() -> TestResult {
        let discovery = sample_discovery_report();
        let series = build_series_manifest(&discovery, &sample_series_config(), "now".into())?;
        let mut report = sample_compile_report();
        report
            .file_results
            .first_mut()
            .ok_or_else(|| color_eyre::eyre::eyre!("sample report has no file results"))?
            .status = RunnerStatus::Fail;
        let Err(error) =
            baseline_v2_from_report(&report, &series, &sample_baseline_v2_config(), None, &[])
        else {
            bail!("an unbucketed failing file must not be accepted into baseline v2");
        };
        if !error.to_string().contains("UnbucketedFailure") {
            bail!("unexpected report-shape error: {error}");
        }
        Ok(())
    }

    #[test]
    fn report_digest_ignores_disposable_measurement_metadata() -> TestResult {
        let report = sample_compile_report();
        let mut replay = report.clone();
        replay.timestamp = "different-time".into();
        replay.prepared_tree = "different-prepared-tree".into();
        replay.run_tree = "different-run-tree".into();
        replay.host_perl = "different-host-perl".into();
        replay.file_results.reverse();
        replay.failures.reverse();
        replay.semantic_boundaries.reverse();
        if report_digest(&report)? != report_digest(&replay)? {
            bail!("volatile paths, timestamps, and record order changed the report digest");
        }
        Ok(())
    }

    #[test]
    fn compile_baseline_v2_rejects_boundary_change_without_transition() -> TestResult {
        let discovery = sample_discovery_report();
        let series = build_series_manifest(&discovery, &sample_series_config(), "now".into())?;
        let baseline = baseline_v2_from_report(
            &sample_compile_report(),
            &series,
            &sample_baseline_v2_config(),
            None,
            &[],
        )?;
        let mut report = sample_compile_report();
        report.semantic_boundaries.push(sample_semantic_boundary());
        let comparison = compare_baseline_v2(&baseline, &report, &series);
        assert_violation(&comparison, BaselineViolationKind::SemanticBoundary);
        Ok(())
    }

    #[test]
    fn compile_baseline_v2_reports_boundary_change_once_without_transition() -> TestResult {
        let discovery = sample_discovery_report();
        let series = build_series_manifest(&discovery, &sample_series_config(), "now".into())?;
        let baseline = baseline_v2_from_report(
            &sample_compile_report(),
            &series,
            &sample_baseline_v2_config(),
            None,
            &[],
        )?;
        let mut report = sample_compile_report();
        report.semantic_boundaries.push(sample_semantic_boundary());

        let comparison = compare_baseline_v2(&baseline, &report, &series);
        let boundary_violations = comparison
            .violations
            .iter()
            .filter(|violation| violation.kind == BaselineViolationKind::SemanticBoundary)
            .count();
        if boundary_violations != 1 {
            bail!("expected one semantic-boundary violation, found {boundary_violations}");
        }
        Ok(())
    }

    #[test]
    fn compile_baseline_v2_requires_reviewed_boundary_retirement() -> TestResult {
        let discovery = sample_discovery_report();
        let series = build_series_manifest(&discovery, &sample_series_config(), "now".into())?;
        let mut previous_report = sample_compile_report();
        previous_report.semantic_boundaries.push(sample_semantic_boundary());
        let config = sample_baseline_v2_config();
        let previous = baseline_v2_from_report(&previous_report, &series, &config, None, &[])?;
        let current = sample_compile_report();
        let mut transition_config = config;
        transition_config.accepted_transition_id = Some("transition-1".into());

        let Err(error) =
            baseline_v2_from_report(&current, &series, &transition_config, Some(&previous), &[])
        else {
            bail!("a disappearing boundary without retirement should fail closed");
        };
        if !error.to_string().contains("retirement") {
            bail!("unexpected boundary retirement error: {error}");
        }

        let retirement = BoundaryRetirement {
            schema_version: BOUNDARY_RETIREMENT_SCHEMA_VERSION.into(),
            path: "base/ok.t".into(),
            id: "runtime_symbolic_reference".into(),
            source_start: 4,
            source_end: 12,
            series_id: series.series_id.clone(),
            manifest_hash: series.manifest_hash.clone(),
            measurement_sha: current.commit.clone(),
            source_report_digest: report_digest(&current)?,
            transition_id: "transition-1".into(),
            replacement_issue: "#5168".into(),
            evidence_bundle: "bundle-sha256:example".into(),
        };
        let accepted = baseline_v2_from_report(
            &current,
            &series,
            &transition_config,
            Some(&previous),
            &[retirement],
        )?;
        if !accepted.semantic_boundaries.is_empty() {
            bail!("retired boundary remained in the accepted inventory");
        }
        Ok(())
    }

    #[test]
    fn compile_baseline_v2_rejects_nonadmissible_accepted_boundaries() -> TestResult {
        let discovery = sample_discovery_report();
        let series = build_series_manifest(&discovery, &sample_series_config(), "now".into())?;

        for (label, disposition, blocks_compilation) in [
            ("unknown", SemanticBoundaryDisposition::Unknown, true),
            ("unsupported", SemanticBoundaryDisposition::Unsupported, true),
            ("compile-blocking", SemanticBoundaryDisposition::DeferredRuntime, true),
        ] {
            let mut report = sample_compile_report();
            let mut boundary = sample_semantic_boundary();
            boundary.disposition = disposition;
            boundary.blocks_compilation = blocks_compilation;
            if matches!(disposition, SemanticBoundaryDisposition::Unknown) {
                boundary.confidence = SemanticBoundaryConfidence::Unresolved;
            }
            if matches!(disposition, SemanticBoundaryDisposition::Unsupported) {
                boundary.confidence = SemanticBoundaryConfidence::Unresolved;
            }
            report.semantic_boundaries.push(boundary);

            let result =
                baseline_v2_from_report(&report, &series, &sample_baseline_v2_config(), None, &[]);
            if result.is_ok() {
                bail!("{label} semantic boundary was accepted into baseline v2");
            }
        }
        Ok(())
    }

    #[test]
    fn compile_baseline_v2_rejects_stale_boundary_retirement_receipt() -> TestResult {
        let discovery = sample_discovery_report();
        let series = build_series_manifest(&discovery, &sample_series_config(), "now".into())?;
        let mut previous_report = sample_compile_report();
        previous_report.semantic_boundaries.push(sample_semantic_boundary());
        let config = sample_baseline_v2_config();
        let previous = baseline_v2_from_report(&previous_report, &series, &config, None, &[])?;
        let current = sample_compile_report();
        let mut transition_config = config;
        transition_config.accepted_transition_id = Some("transition-1".into());
        let mut stale = BoundaryRetirement {
            schema_version: BOUNDARY_RETIREMENT_SCHEMA_VERSION.into(),
            path: "base/ok.t".into(),
            id: "runtime_symbolic_reference".into(),
            source_start: 4,
            source_end: 12,
            series_id: series.series_id.clone(),
            manifest_hash: series.manifest_hash.clone(),
            measurement_sha: current.commit.clone(),
            source_report_digest: report_digest(&current)?,
            transition_id: "transition-1".into(),
            replacement_issue: "#5168".into(),
            evidence_bundle: "bundle-sha256:example".into(),
        };
        stale.source_report_digest = "sha256:stale-report".into();

        let Err(error) = baseline_v2_from_report(
            &current,
            &series,
            &transition_config,
            Some(&previous),
            &[stale],
        ) else {
            bail!("a retirement receipt for a stale report must fail closed");
        };
        let message = error.to_string();
        if !message.contains("BoundaryRetirementReceiptMismatch")
            || !message.contains("base/ok.t")
            || !message.contains("stale")
        {
            bail!("unexpected stale retirement error: {error}");
        }
        Ok(())
    }

    #[test]
    fn compile_baseline_v2_rejects_still_present_boundary_retirement() -> TestResult {
        let discovery = sample_discovery_report();
        let series = build_series_manifest(&discovery, &sample_series_config(), "now".into())?;
        let mut report = sample_compile_report();
        report.semantic_boundaries.push(sample_semantic_boundary());
        let config = sample_baseline_v2_config();
        let previous = baseline_v2_from_report(&report, &series, &config, None, &[])?;
        let mut transition_config = config;
        transition_config.accepted_transition_id = Some("transition-1".into());
        let retirement = BoundaryRetirement {
            schema_version: BOUNDARY_RETIREMENT_SCHEMA_VERSION.into(),
            path: "base/ok.t".into(),
            id: "runtime_symbolic_reference".into(),
            source_start: 4,
            source_end: 12,
            series_id: series.series_id.clone(),
            manifest_hash: series.manifest_hash.clone(),
            measurement_sha: report.commit.clone(),
            source_report_digest: report_digest(&report)?,
            transition_id: "transition-1".into(),
            replacement_issue: "#5168".into(),
            evidence_bundle: "bundle-sha256:example".into(),
        };

        let Err(error) = baseline_v2_from_report(
            &report,
            &series,
            &transition_config,
            Some(&previous),
            &[retirement],
        ) else {
            bail!("a retirement receipt for a still-present boundary must fail closed");
        };
        if !error.to_string().contains("still present") {
            bail!("unexpected still-present retirement error: {error}");
        }
        Ok(())
    }

    #[test]
    fn boundary_retirement_validation_reports_specific_violation_kinds() -> TestResult {
        let discovery = sample_discovery_report();
        let series = build_series_manifest(&discovery, &sample_series_config(), "now".into())?;
        let mut previous_report = sample_compile_report();
        previous_report.semantic_boundaries.push(sample_semantic_boundary());
        let config = sample_baseline_v2_config();
        let previous = baseline_v2_from_report(&previous_report, &series, &config, None, &[])?;

        let invalid_retirement = BoundaryRetirement {
            schema_version: BOUNDARY_RETIREMENT_SCHEMA_VERSION.into(),
            path: "base/ok.t".into(),
            id: "runtime_symbolic_reference".into(),
            source_start: 4,
            source_end: 12,
            series_id: series.series_id.clone(),
            manifest_hash: series.manifest_hash.clone(),
            measurement_sha: previous.repository_commit.clone(),
            source_report_digest: previous.source_report_digest.clone(),
            transition_id: "wrong-transition".into(),
            replacement_issue: String::new(),
            evidence_bundle: String::new(),
        };
        let unknown_retirement = BoundaryRetirement {
            schema_version: BOUNDARY_RETIREMENT_SCHEMA_VERSION.into(),
            path: "base/missing.t".into(),
            id: "missing-boundary".into(),
            source_start: 1,
            source_end: 2,
            series_id: series.series_id.clone(),
            manifest_hash: series.manifest_hash.clone(),
            measurement_sha: previous.repository_commit.clone(),
            source_report_digest: previous.source_report_digest.clone(),
            transition_id: "transition-1".into(),
            replacement_issue: "#5168".into(),
            evidence_bundle: "bundle-sha256:example".into(),
        };
        let violations = compare_boundary_transition(
            &previous,
            &[],
            &[invalid_retirement, unknown_retirement],
            "transition-1",
            &series,
            &sample_compile_report(),
        );
        if !violations.iter().any(|violation| {
            violation.kind == BaselineViolationKind::BoundaryRetirementReceiptMismatch
        }) {
            bail!("invalid retirement metadata was not classified separately");
        }
        if !violations.iter().any(|violation| {
            violation.kind == BaselineViolationKind::BoundaryRetirementReferencesUnknownBoundary
        }) {
            bail!("unknown retirement boundary was not classified separately");
        }
        Ok(())
    }

    #[test]
    fn compile_baseline_v2_rejects_tampered_persisted_retirement() -> TestResult {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("baseline.json");
        let discovery = sample_discovery_report();
        let series = build_series_manifest(&discovery, &sample_series_config(), "now".into())?;
        let mut previous_report = sample_compile_report();
        previous_report.semantic_boundaries.push(sample_semantic_boundary());
        let config = sample_baseline_v2_config();
        let previous = baseline_v2_from_report(&previous_report, &series, &config, None, &[])?;
        let current = sample_compile_report();
        let mut transition_config = config;
        transition_config.accepted_transition_id = Some("transition-1".into());
        let retirement = BoundaryRetirement {
            schema_version: BOUNDARY_RETIREMENT_SCHEMA_VERSION.into(),
            path: "base/ok.t".into(),
            id: "runtime_symbolic_reference".into(),
            source_start: 4,
            source_end: 12,
            series_id: series.series_id.clone(),
            manifest_hash: series.manifest_hash.clone(),
            measurement_sha: current.commit.clone(),
            source_report_digest: report_digest(&current)?,
            transition_id: "transition-1".into(),
            replacement_issue: "#5168".into(),
            evidence_bundle: "bundle-sha256:example".into(),
        };
        let accepted = baseline_v2_from_report(
            &current,
            &series,
            &transition_config,
            Some(&previous),
            &[retirement],
        )?;
        let mut series_tampered = accepted.clone();
        series_tampered.boundary_retirements[0].series_id = "wrong-series".into();
        let comparison = compare_baseline_v2_with_identities(
            &series_tampered,
            &current,
            &series,
            None,
            Some("transition-1"),
            &[],
        );
        if !comparison.violations.iter().any(|violation| {
            violation.kind == BaselineViolationKind::BoundaryRetirementReceiptMismatch
        }) {
            bail!("series-bound retirement tampering was not rejected during comparison");
        }

        let mutations = [
            ("schema_version", serde_json::Value::String("other-schema".into())),
            ("path", serde_json::Value::String(String::new())),
            ("id", serde_json::Value::String(String::new())),
            ("source_start", serde_json::Value::from(12_u64)),
            ("series_id", serde_json::Value::String("wrong-series".into())),
            ("manifest_hash", serde_json::Value::String("sha256:wrong".into())),
            ("measurement_sha", serde_json::Value::String("wrong-measurement".into())),
            ("source_report_digest", serde_json::Value::String("sha256:tampered".into())),
            ("transition_id", serde_json::Value::String("wrong-transition".into())),
            ("replacement_issue", serde_json::Value::String(String::new())),
            ("evidence_bundle", serde_json::Value::String(String::new())),
        ];
        for (field, replacement) in mutations {
            let mut mutated = serde_json::to_value(&accepted)?;
            mutated["boundary_retirements"][0][field] = replacement;
            fs::write(&path, format!("{}\n", serde_json::to_string(&mutated)?))?;
            if read_compile_baseline_v2(&path).is_ok() {
                bail!("tampered persisted retirement field {field} was accepted");
            }
        }

        let mut value = serde_json::to_value(accepted)?;
        value["boundary_retirements"][0]["source_report_digest"] =
            serde_json::Value::String("sha256:tampered".into());
        fs::write(&path, format!("{}\n", serde_json::to_string(&value)?))?;

        let Err(error) = read_compile_baseline_v2(&path) else {
            bail!("tampered persisted retirement must fail closed");
        };
        if !error.to_string().contains("persisted boundary retirement") {
            bail!("unexpected persisted retirement error: {error}");
        }
        Ok(())
    }

    #[test]
    fn compile_baseline_v2_rejects_historical_v1_as_authority() -> TestResult {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("baseline.json");
        let baseline = baseline_from_report(&sample_compile_report())?;
        write_compile_baseline(&path, &baseline)?;

        let Err(error) = read_compile_baseline_v2(&path) else {
            bail!("historical v1 baseline should not be accepted as v2 authority");
        };
        if !error.to_string().contains("historical compile baseline v1") {
            bail!("unexpected v1 migration error: {error}");
        }
        Ok(())
    }

    #[test]
    fn compile_baseline_v2_requires_boundary_inventory_field() -> TestResult {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("baseline.json");
        let discovery = sample_discovery_report();
        let series = build_series_manifest(&discovery, &sample_series_config(), "now".into())?;
        let baseline = baseline_v2_from_report(
            &sample_compile_report(),
            &series,
            &sample_baseline_v2_config(),
            None,
            &[],
        )?;
        let mut value = serde_json::to_value(baseline)?;
        value
            .as_object_mut()
            .ok_or_else(|| color_eyre::eyre::eyre!("baseline was not an object"))?
            .remove("semantic_boundaries");
        fs::write(&path, format!("{}\n", serde_json::to_string(&value)?))?;
        let Err(error) = read_compile_baseline_v2(&path) else {
            bail!("a v2 baseline without a boundary inventory must fail closed");
        };
        if !error.to_string().contains("MissingBoundaryInventory") {
            bail!("unexpected missing inventory error: {error}");
        }
        Ok(())
    }

    #[test]
    fn compile_baseline_v2_reader_rejects_nonadmissible_inventory() -> TestResult {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("baseline.json");
        let discovery = sample_discovery_report();
        let series = build_series_manifest(&discovery, &sample_series_config(), "now".into())?;
        let mut report = sample_compile_report();
        report.semantic_boundaries.push(sample_semantic_boundary());
        let baseline =
            baseline_v2_from_report(&report, &series, &sample_baseline_v2_config(), None, &[])?;
        let mut value = serde_json::to_value(baseline)?;
        let boundary = value
            .get_mut("semantic_boundaries")
            .and_then(serde_json::Value::as_array_mut)
            .and_then(|boundaries| boundaries.first_mut())
            .ok_or_else(|| color_eyre::eyre::eyre!("baseline has no boundary inventory"))?;
        boundary["disposition"] = serde_json::Value::String("unsupported".into());
        boundary["confidence"] = serde_json::Value::String("unresolved".into());
        boundary["blocks_compilation"] = serde_json::Value::Bool(true);
        fs::write(&path, format!("{}\n", serde_json::to_string(&value)?))?;

        let Err(error) = read_compile_baseline_v2(&path) else {
            bail!("a malformed accepted boundary inventory must fail closed");
        };
        if !error.to_string().contains("unknown or unsupported") {
            bail!("unexpected malformed inventory error: {error}");
        }
        Ok(())
    }

    #[test]
    fn public_baseline_rejects_v2_options_without_series() -> TestResult {
        let temp = tempfile::tempdir()?;
        let report_path = temp.path().join("report.json");
        let baseline_path = temp.path().join("baseline.json");
        write_run_report(&report_path, &sample_compile_report())?;
        let mut config = sample_baseline_v2_config();
        config.report = Some(report_path);
        config.baseline = Some(baseline_path);
        let Err(error) = baseline(config) else {
            bail!("v2 identity options must not silently fall back to baseline v1");
        };
        if !error.to_string().contains("require a comparison-series manifest") {
            bail!("unexpected v2 option error: {error}");
        }
        Ok(())
    }

    #[test]
    fn public_baseline_requires_transition_for_existing_v2_baseline() -> TestResult {
        let temp = tempfile::tempdir()?;
        let report_path = temp.path().join("report.json");
        let series_path = temp.path().join("series.json");
        let baseline_path = temp.path().join("baseline.json");
        let discovery = sample_discovery_report();
        let series = build_series_manifest(&discovery, &sample_series_config(), "now".into())?;
        write_series_manifest(&series_path, &series)?;

        let mut previous_report = sample_compile_report();
        previous_report.semantic_boundaries.push(sample_semantic_boundary());
        let accepted = baseline_v2_from_report(
            &previous_report,
            &series,
            &sample_baseline_v2_config(),
            None,
            &[],
        )?;
        write_compile_baseline_v2(&baseline_path, &accepted)?;
        write_run_report(&report_path, &sample_compile_report())?;

        let mut config = sample_baseline_v2_config();
        config.report = Some(report_path);
        config.baseline = Some(baseline_path);
        config.series = Some(series_path);
        let Err(error) = baseline(config) else {
            bail!("overwriting an existing v2 baseline must require transition evidence");
        };
        if !error.to_string().contains("accepted-transition-id") {
            bail!("unexpected transition error: {error}");
        }
        Ok(())
    }

    #[test]
    fn public_baseline_rejects_retirements_without_transition_id() -> TestResult {
        let temp = tempfile::tempdir()?;
        let report_path = temp.path().join("report.json");
        let series_path = temp.path().join("series.json");
        let baseline_path = temp.path().join("baseline.json");
        let retirements_path = temp.path().join("retirements.json");
        let discovery = sample_discovery_report();
        let series = build_series_manifest(&discovery, &sample_series_config(), "now".into())?;
        write_series_manifest(&series_path, &series)?;
        write_run_report(&report_path, &sample_compile_report())?;
        fs::write(&retirements_path, "[]\n")?;

        let mut config = sample_baseline_v2_config();
        config.accept = false;
        config.report = Some(report_path);
        config.baseline = Some(baseline_path);
        config.series = Some(series_path);
        config.boundary_retirements = Some(retirements_path);
        config.accepted_transition_id = None;
        let Err(error) = baseline(config) else {
            bail!("retirement receipts without a transition identity must fail closed");
        };
        if !error.to_string().contains("accepted-transition-id") {
            bail!("unexpected missing transition identity error: {error}");
        }
        Ok(())
    }

    #[test]
    fn series_manifest_rejects_unknown_discovery_ref() -> TestResult {
        let mut discovery = sample_discovery_report();
        discovery.perl_ref = "unknown".into();
        let Err(error) = build_series_manifest(&discovery, &sample_series_config(), "now".into())
        else {
            bail!("an unknown discovery Perl ref must not become a resolved series identity");
        };
        if !error.to_string().contains("does not match discovery receipt") {
            bail!("unexpected unknown discovery ref error: {error}");
        }
        Ok(())
    }

    #[test]
    fn series_manifest_does_not_overwrite_without_replacement_identity() -> TestResult {
        let temp = tempfile::tempdir()?;
        let discovery_path = temp.path().join("discovery.json");
        let output_path = temp.path().join("series.json");
        write_discovery_report(&discovery_path, &sample_discovery_report())?;
        let mut config = sample_series_config();
        config.discovery = discovery_path;
        config.output = Some(output_path);
        series_manifest(config.clone())?;

        let Err(error) = series_manifest(config) else {
            bail!("existing comparison-series output must not be overwritten silently");
        };
        if !error.to_string().contains("manifest is immutable") {
            bail!("unexpected immutable manifest error: {error}");
        }
        Ok(())
    }

    #[test]
    fn series_manifest_rejects_reusing_replaced_series_id() -> TestResult {
        let temp = tempfile::tempdir()?;
        let discovery_path = temp.path().join("discovery.json");
        let output_path = temp.path().join("series.json");
        write_discovery_report(&discovery_path, &sample_discovery_report())?;
        let mut config = sample_series_config();
        config.discovery = discovery_path;
        config.output = Some(output_path.clone());
        series_manifest(config.clone())?;

        config.replaces_series_id = Some(config.series_id.clone());
        config.change_reason = Some("reviewed replacement".into());
        let Err(error) = series_manifest(config) else {
            bail!("a replacement must not reuse the existing comparison-series ID");
        };
        if !error.to_string().contains("must declare a new --series-id") {
            bail!("unexpected self-replacement error: {error}");
        }
        Ok(())
    }

    #[test]
    fn series_manifest_rejects_replacement_without_change_reason_on_new_path() -> TestResult {
        let temp = tempfile::tempdir()?;
        let discovery_path = temp.path().join("discovery.json");
        let output_path = temp.path().join("replacement.json");
        write_discovery_report(&discovery_path, &sample_discovery_report())?;
        let mut config = sample_series_config();
        config.discovery = discovery_path;
        config.output = Some(output_path);
        config.replaces_series_id = Some("selected-base-perl-5.42.1".into());
        config.change_reason = None;

        let Err(error) = series_manifest(config) else {
            bail!("a replacement without change metadata must fail on a new output path");
        };
        if !error.to_string().contains("non-empty --change-reason") {
            bail!("unexpected replacement metadata error: {error}");
        }
        Ok(())
    }

    #[test]
    fn identical_compile_report_passes_baseline() -> TestResult {
        let report = sample_compile_report();
        let baseline = baseline_from_report(&report)?;

        let comparison = compare_baseline(&baseline, &report);

        assert!(comparison.is_clean(), "identical report should pass: {comparison:?}");
        Ok(())
    }

    #[test]
    fn compile_baseline_file_results_are_order_independent() -> TestResult {
        let mut report = sample_compile_report();
        let baseline = baseline_from_report(&report)?;
        report.file_results.reverse();

        let comparison = compare_baseline(&baseline, &report);

        assert!(comparison.is_clean(), "reordered file results should pass: {comparison:?}");
        Ok(())
    }

    #[test]
    fn compile_baseline_fails_when_previously_passing_file_fails() -> TestResult {
        let mut report = sample_compile_report();
        let baseline = baseline_from_report(&report)?;
        mark_file_failed(&mut report, "base/ok.t", "parse_recovery");

        let comparison = compare_baseline(&baseline, &report);

        assert_violation(&comparison, BaselineViolationKind::PreviouslyPassingFileFailed);
        Ok(())
    }

    #[test]
    fn compile_baseline_fails_on_unexpected_new_failure() -> TestResult {
        let mut report = sample_compile_report();
        let baseline = baseline_from_report(&report)?;
        report.file_results.push(RunFileResult {
            path: "base/new.t".into(),
            status: RunnerStatus::Fail,
            assertions_passed: 0,
            assertions_total: 1,
        });
        report.failures.push(sample_failure("base/new.t", "parse_recovery"));
        report.buckets.insert("parse_recovery".into(), 1);
        report.summary.files_total = 3;
        report.summary.files_failed = 1;
        report.summary.files_passed = 2;
        report.summary.tap_assertions_total = 3;
        report.summary.tap_assertions_passed = 2;

        let comparison = compare_baseline(&baseline, &report);

        assert_violation(&comparison, BaselineViolationKind::UnexpectedNewFailure);
        Ok(())
    }

    #[test]
    fn compile_baseline_fails_on_unknown_bucket() -> TestResult {
        let mut report = sample_compile_report();
        let baseline = baseline_from_report(&report)?;
        mark_file_failed(&mut report, "base/ok.t", "unknown");

        let comparison = compare_baseline(&baseline, &report);

        assert_violation(&comparison, BaselineViolationKind::UnknownBucket);
        Ok(())
    }

    #[test]
    fn compile_baseline_fails_on_unbucketed_failure() -> TestResult {
        let mut report = sample_compile_report();
        let baseline = baseline_from_report(&report)?;
        let Some(result) = report.file_results.iter_mut().find(|result| result.path == "base/ok.t")
        else {
            bail!("sample report missing base/ok.t");
        };
        result.status = RunnerStatus::Fail;
        result.assertions_passed = 0;
        report.summary.files_passed = 1;
        report.summary.files_failed = 1;
        report.summary.tap_assertions_passed = 1;

        let comparison = compare_baseline(&baseline, &report);

        assert_violation(&comparison, BaselineViolationKind::UnbucketedFailure);
        Ok(())
    }

    #[test]
    fn compile_baseline_fails_when_bucket_count_increases() -> TestResult {
        let mut baseline_report = sample_compile_report();
        mark_file_failed(&mut baseline_report, "base/ok.t", "parse_recovery");
        let baseline = baseline_from_report(&baseline_report)?;
        let mut current_report = baseline_report.clone();
        current_report.file_results.push(RunFileResult {
            path: "base/new.t".into(),
            status: RunnerStatus::Fail,
            assertions_passed: 0,
            assertions_total: 1,
        });
        current_report.failures.push(sample_failure("base/new.t", "parse_recovery"));
        current_report.buckets.insert("parse_recovery".into(), 2);
        current_report.summary.files_total = 3;
        current_report.summary.files_passed = 1;
        current_report.summary.files_failed = 2;
        current_report.summary.tap_assertions_total = 3;
        current_report.summary.tap_assertions_passed = 1;

        let comparison = compare_baseline(&baseline, &current_report);

        assert_violation(&comparison, BaselineViolationKind::BucketCountIncreased);
        Ok(())
    }

    #[test]
    fn compile_baseline_fails_on_assertion_regression() -> TestResult {
        let mut report = sample_compile_report();
        let baseline = baseline_from_report(&report)?;
        let Some(result) = report.file_results.iter_mut().find(|result| result.path == "base/ok.t")
        else {
            bail!("sample report missing base/ok.t");
        };
        result.assertions_passed = 0;
        report.summary.tap_assertions_passed = 1;

        let comparison = compare_baseline(&baseline, &report);

        assert_violation(&comparison, BaselineViolationKind::AssertionRegression);
        Ok(())
    }

    #[test]
    fn compile_baseline_fails_on_mode_and_profile_mismatch() -> TestResult {
        let mut report = sample_compile_report();
        let mut baseline = baseline_from_report(&report)?;
        baseline.mode = HarnessMode::Parse;
        baseline.profile = HarnessProfile::Comp;
        report.mode = HarnessMode::Compile;
        report.profile = HarnessProfile::Base;

        let comparison = compare_baseline(&baseline, &report);

        assert_violation(&comparison, BaselineViolationKind::ModeMismatch);
        assert_violation(&comparison, BaselineViolationKind::ProfileMismatch);
        Ok(())
    }

    #[test]
    fn compile_baseline_fails_when_expected_file_is_missing() -> TestResult {
        let mut report = sample_compile_report();
        let baseline = baseline_from_report(&report)?;
        report.file_results.retain(|result| result.path != "base/ok.t");
        report.summary.files_total = 1;
        report.summary.files_passed = 1;
        report.summary.tap_assertions_total = 1;
        report.summary.tap_assertions_passed = 1;

        let comparison = compare_baseline(&baseline, &report);

        assert_violation(&comparison, BaselineViolationKind::MissingExpectedFile);
        Ok(())
    }

    #[test]
    fn compile_baseline_accept_writes_deterministic_sorted_json() -> TestResult {
        let temp = tempfile::tempdir()?;
        let report_path = temp.path().join("report.json");
        let baseline_path = temp.path().join("baseline.json");
        let mut report = sample_compile_report();
        report.file_results.reverse();
        write_run_report(&report_path, &report)?;

        baseline(BaselineConfig {
            mode: HarnessMode::Compile,
            profile: HarnessProfile::Base,
            report: Some(report_path),
            baseline: Some(baseline_path.clone()),
            accept: true,
            series: None,
            previous_baseline: None,
            boundary_retirements: None,
            compiler_subject_identity: None,
            invocation_identity: None,
            capability_identity: None,
            environment_identity: None,
            accepted_transition_id: None,
            evidence_bundle: None,
        })?;

        let raw = fs::read_to_string(&baseline_path)?;
        let accepted: CompileBaseline = serde_json::from_str(&raw)?;
        let paths =
            accepted.file_results.iter().map(|result| result.path.as_str()).collect::<Vec<_>>();
        assert_eq!(paths, vec!["base/lex.t", "base/ok.t"]);
        assert!(raw.ends_with('\n'));
        Ok(())
    }

    #[test]
    fn generated_two_file_compile_report_passes_checked_in_baseline() -> TestResult {
        let root = project_root()?;
        let baseline = read_compile_baseline(
            &root.join(".ci").join("perl-core-harness").join("base-compile-baseline.json"),
        )?;
        let report = sample_compile_report();

        let comparison = compare_baseline(&baseline, &report);

        assert!(
            comparison.is_clean(),
            "checked-in baseline should match fixture report: {comparison:?}"
        );
        Ok(())
    }

    #[test]
    fn selected_execute_base_report_passes_checked_in_baseline() -> TestResult {
        let root = project_root()?;
        let baseline = read_compile_baseline(
            &root.join(".ci").join("perl-core-harness").join("base-execute-baseline.json"),
        )?;
        let report = sample_execute_report();

        let comparison = compare_baseline(&baseline, &report);

        assert!(
            comparison.is_clean(),
            "checked-in execute baseline should match selected base report: {comparison:?}"
        );
        Ok(())
    }

    #[test]
    fn selected_execute_base_baseline_fails_on_assertion_regression() -> TestResult {
        let baseline = baseline_from_report(&sample_execute_report())?;
        let mut report = sample_execute_report();
        let cond = report
            .file_results
            .iter_mut()
            .find(|result| result.path == "base/cond.t")
            .ok_or_else(|| color_eyre::eyre::eyre!("missing base/cond.t result"))?;
        cond.assertions_passed = 3;
        report.summary.tap_assertions_passed = 9;

        let comparison = compare_baseline(&baseline, &report);

        assert!(
            comparison
                .violations
                .iter()
                .any(|violation| violation.kind == BaselineViolationKind::AssertionRegression
                    && violation.path.as_deref() == Some("base/cond.t")),
            "expected selected execute assertion regression: {comparison:?}"
        );
        Ok(())
    }

    #[test]
    fn non_git_perl_tree_has_unknown_ref() -> TestResult {
        let dir = tempfile::tempdir()?;

        assert_eq!(perl_tree_ref(dir.path()), "unknown");
        Ok(())
    }

    #[test]
    fn default_paths_match_receipt_layout() {
        assert!(
            default_discovery_path(HarnessProfile::Base)
                .ends_with("target/perl-core/discovery/base.json")
        );
        assert!(
            default_run_report_path(HarnessMode::Parse, HarnessProfile::Base)
                .ends_with("target/perl-core/reports/base-parse.json")
        );
        assert!(default_smoke_dir(HarnessProfile::Comp).ends_with("target/perl-core/smoke/comp"));
    }

    #[test]
    fn validate_runner_script_rejects_missing_tree_layouts() -> TestResult {
        let temp = tempfile::tempdir()?;
        let missing_t_dir = temp.path().join("missing-t");
        let Err(err) = validate_runner_script(&missing_t_dir, HarnessRunner::Test) else {
            bail!("missing t directory should fail");
        };
        assert!(err.to_string().contains("missing t/ directory"));

        let t_dir = temp.path().join("t");
        fs::create_dir_all(&t_dir)?;
        let Err(err) = validate_runner_script(&t_dir, HarnessRunner::Test) else {
            bail!("missing TEST script should fail");
        };
        assert!(err.to_string().contains("missing t/TEST"));
        Ok(())
    }

    #[test]
    fn configured_runner_binary_must_exist() -> TestResult {
        let temp = tempfile::tempdir()?;
        let missing = temp.path().join("missing-runner");

        let Err(err) = resolve_runner_binary(Some(&missing)) else {
            bail!("missing configured runner should fail");
        };

        assert!(err.to_string().contains("runner binary does not exist"));
        Ok(())
    }

    #[test]
    fn configured_runner_binary_returns_canonical_path() -> TestResult {
        let temp = tempfile::tempdir()?;
        let runner = temp.path().join("runner");
        fs::write(&runner, "")?;

        let resolved = resolve_runner_binary(Some(&runner))?;

        assert_eq!(resolved, runner.canonicalize()?);
        Ok(())
    }

    #[test]
    fn default_runner_binary_uses_existing_agent_binary_without_building() -> TestResult {
        let root = project_root()?;
        let binary_name =
            if cfg!(windows) { "perl-core-test-runner.exe" } else { "perl-core-test-runner" };
        let binary = root.join("target").join("agent").join(binary_name);
        let created = if binary.is_file() {
            false
        } else {
            if let Some(parent) = binary.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&binary, "")?;
            true
        };

        let expected = binary.canonicalize()?;
        let resolved = resolve_runner_binary(None);
        if created {
            fs::remove_file(&binary)?;
        }
        let resolved = resolved?;

        assert_eq!(resolved, expected);
        Ok(())
    }

    #[test]
    fn runner_context_reader_rejects_missing_empty_and_invalid_records() -> TestResult {
        let temp = tempfile::tempdir()?;
        let context = temp.path().join("records.jsonl");

        let Err(err) = read_runner_records(&context) else {
            bail!("missing runner context should fail");
        };
        assert!(err.to_string().contains("runner context was not written"));

        fs::write(&context, "\n")?;
        let Err(err) = read_runner_records(&context) else {
            bail!("empty runner context should fail");
        };
        assert!(err.to_string().contains("runner context contained no records"));

        fs::write(&context, "{not-json}\n")?;
        let Err(err) = read_runner_records(&context) else {
            bail!("invalid runner context should fail");
        };
        assert!(err.to_string().contains("decoding runner record 1"));
        Ok(())
    }

    #[test]
    fn bucket_metadata_maps_failure_classes() {
        assert_eq!(workstream_for_bucket("source_decode"), "source_loading");
        assert_eq!(workstream_for_bucket("hir_lowering"), "hir");
        assert_eq!(workstream_for_bucket("compile_effect"), "compile_time_effects");
        assert_eq!(workstream_for_bucket("scope_pad"), "scope_and_pad");
        assert_eq!(workstream_for_bucket("package_stash"), "package_stash");
        assert_eq!(workstream_for_bucket("pragma_feature"), "pragma_model");
        assert_eq!(workstream_for_bucket("module_resolution"), "module_resolution");
        assert_eq!(workstream_for_bucket("cli_switch"), "harness_cli_compat");
        assert_eq!(workstream_for_bucket("harness_prepare"), "harness_integration");
        assert_eq!(workstream_for_bucket("unknown_bucket"), "compiler_conformance");
        assert_eq!(lsp_impact_for_bucket("source_decode"), vec!["workspace_index", "diagnostics"]);
        assert_eq!(
            lsp_impact_for_bucket("compile_effect"),
            vec!["definition", "references", "diagnostics"]
        );
        assert_eq!(
            lsp_impact_for_bucket("module_resolution"),
            vec!["definition", "hover", "completion"]
        );
        assert_eq!(lsp_impact_for_bucket("cli_switch"), vec!["compiler_conformance"]);
        assert_eq!(lsp_impact_for_bucket("harness_prepare"), vec!["compiler_conformance"]);
        assert_eq!(lsp_impact_for_bucket("unknown_bucket"), vec!["compiler_conformance"]);
    }

    #[test]
    fn report_writers_create_parent_directories() -> TestResult {
        let temp = tempfile::tempdir()?;
        let discovery_path = temp.path().join("nested").join("discovery").join("base.json");
        let run_path = temp.path().join("nested").join("reports").join("base-parse.json");

        let discovery = DiscoveryReport {
            schema_version: DISCOVERY_SCHEMA_VERSION.into(),
            commit: "abc".into(),
            timestamp: "2026-07-02T00:00:00Z".into(),
            perl_ref: "perl-ref".into(),
            prepared_tree: "/tmp/perl".into(),
            host_perl: "perl".into(),
            runner: HarnessRunner::Test,
            profile: HarnessProfile::Base,
            tests: vec![DiscoveredTest { path: "base/if.t".into(), root: "base".into() }],
        };
        write_discovery_report(&discovery_path, &discovery)?;
        assert!(discovery_path.is_file());

        let config = RunConfig {
            perl_tree: temp.path().join("prepared"),
            host_perl: PathBuf::from("perl"),
            runner: HarnessRunner::Test,
            mode: HarnessMode::Parse,
            profile: HarnessProfile::Base,
            tests: Vec::new(),
            output: Some(run_path.clone()),
            runner_binary: Some(PathBuf::from("runner")),
        };
        let discovered = vec![DiscoveredTest { path: "base/if.t".into(), root: "base".into() }];
        let records = vec![RunnerRecord {
            schema_version: "perl_core_harness.runner_record.v1".into(),
            mode: "parse".into(),
            path: "base/if.t".into(),
            status: RunnerStatus::Pass,
            assertions_passed: 1,
            assertions_total: 1,
            bucket: None,
            first_diagnostic: None,
            semantic_boundaries: Vec::new(),
        }];
        let run_tree = temp.path().join("run");
        let report = build_run_report(BuildRunReportInput {
            config: &config,
            perl_tree: temp.path(),
            run_tree: &run_tree,
            discovered: &discovered,
            records: &records,
            harness_status: Some(0),
        });
        write_run_report(&run_path, &report)?;
        assert!(run_path.is_file());
        Ok(())
    }

    fn sample_compile_report() -> RunReport {
        RunReport {
            schema_version: RUN_REPORT_SCHEMA_VERSION.into(),
            commit: "abc".into(),
            timestamp: "2026-07-02T00:00:00Z".into(),
            perl_ref: "perl-ref".into(),
            prepared_tree: "/tmp/perl".into(),
            run_tree: "/tmp/run".into(),
            host_perl: "perl".into(),
            runner: HarnessRunner::Test,
            mode: HarnessMode::Compile,
            profile: HarnessProfile::Base,
            harness_status: Some(0),
            summary: RunSummary {
                files_total: 2,
                files_passed: 2,
                files_failed: 0,
                tap_assertions_total: 2,
                tap_assertions_passed: 2,
            },
            buckets: BTreeMap::new(),
            file_results: vec![
                RunFileResult {
                    path: "base/lex.t".into(),
                    status: RunnerStatus::Pass,
                    assertions_passed: 1,
                    assertions_total: 1,
                },
                RunFileResult {
                    path: "base/ok.t".into(),
                    status: RunnerStatus::Pass,
                    assertions_passed: 1,
                    assertions_total: 1,
                },
            ],
            failures: Vec::new(),
            semantic_boundaries: Vec::new(),
        }
    }

    fn sample_semantic_boundary() -> ObservedSemanticBoundary {
        ObservedSemanticBoundary {
            path: "base/ok.t".into(),
            id: "runtime_symbolic_reference".into(),
            disposition: SemanticBoundaryDisposition::DeferredRuntime,
            reason: "runtime value remains dynamic".into(),
            source_span: SemanticBoundarySourceSpan { start: 4, end: 12 },
            source_kind: "SymbolicReferenceDeref".into(),
            confidence: SemanticBoundaryConfidence::Conservative,
            blocks_compilation: false,
            blocks_downstream_static_facts: true,
            lock_scope: SemanticBoundaryLockScope::None,
            owner_workstream: "symbolic_reference_semantics".into(),
            supporting_test: "base/ok.t".into(),
        }
    }

    fn sample_registry_series() -> SeriesManifest {
        SeriesManifest {
            schema_version: SERIES_MANIFEST_SCHEMA_VERSION.into(),
            series_id: "series-1".into(),
            profile: HarnessProfile::Base,
            profile_roots: vec!["base".into()],
            repository_commit: "abc".into(),
            perl_requested_ref: "perl-ref".into(),
            perl_resolved_ref: "perl-ref".into(),
            runner: HarnessRunner::Test,
            normalized_manifest: vec!["base/lex.t".into(), "base/ok.t".into()],
            manifest_hash: "manifest-1".into(),
            preparation_receipt_id: "prepare-1".into(),
            preparation_receipt_digest: "prepare-digest-1".into(),
            harness_schema_version: RUN_REPORT_SCHEMA_VERSION.into(),
            compiler_subject_identity: "compiler-subject-1".into(),
            invocation_identity: "invocation-1".into(),
            capability_identity: "capability-1".into(),
            environment_identity: "environment-1".into(),
            normalization_version: SERIES_MANIFEST_NORMALIZATION_VERSION.into(),
            created_at: "2026-07-02T00:00:00Z".into(),
            replaces_series_id: None,
            change_reason: Some("test series".into()),
        }
    }

    fn sample_registry_entry() -> SemanticBoundaryRegistryEntry {
        let boundary = sample_semantic_boundary();
        SemanticBoundaryRegistryEntry {
            id: boundary.id,
            disposition: boundary.disposition,
            source_kind: boundary.source_kind,
            semantic_meaning: "symbolic reference remains dynamic".into(),
            series_id: "series-1".into(),
            profile: HarnessProfile::Base,
            path: boundary.path,
            manifest_hash: "manifest-1".into(),
            source_span: boundary.source_span,
            source_shape: "symbolic reference dereference".into(),
            lock_scope: boundary.lock_scope,
            reason: boundary.reason,
            ambient_dependency: "runtime symbol table".into(),
            blocks_downstream_static_facts: boundary.blocks_downstream_static_facts,
            owner_issue: "#4753".into(),
            supporting_test: boundary.supporting_test,
            wrong_file_test: "fixtures/semantic-boundaries/wrong-file.t".into(),
            changed_shape_test: "fixtures/semantic-boundaries/changed-shape.t".into(),
            introduction_pr: "#5202".into(),
            introduction_commit: "abc123".into(),
            first_accepted_bundle: "bundle-1".into(),
            replacement_strategy: SemanticBoundaryReplacementStrategy::HirSemantics,
            state: SemanticBoundaryRegistryState::Active,
            retirement_pr: None,
            retirement_bundle: None,
            review_after: None,
            permanent_boundary_rationale: None,
        }
    }

    #[test]
    fn boundary_registry_accepts_an_exact_owned_entry() {
        let registry = SemanticBoundaryRegistry {
            schema_version: SEMANTIC_BOUNDARY_REGISTRY_SCHEMA_VERSION.into(),
            entries: vec![sample_registry_entry()],
        };

        assert!(validate_boundary_registry_shape(&registry).is_empty());
    }

    #[test]
    fn boundary_registry_rejects_unknown_or_widened_debt() {
        let mut entry = sample_registry_entry();
        entry.disposition = SemanticBoundaryDisposition::Unknown;
        entry.lock_scope = SemanticBoundaryLockScope::Path;
        entry.owner_issue = "not-an-issue".into();
        entry.path = "C:/private/run/base/ok.t".into();
        let registry = SemanticBoundaryRegistry {
            schema_version: SEMANTIC_BOUNDARY_REGISTRY_SCHEMA_VERSION.into(),
            entries: vec![entry],
        };

        let violations = validate_boundary_registry_shape(&registry);
        assert!(violations.iter().any(|violation| violation.contains("non-admissible")));
        assert!(violations.iter().any(|violation| violation.contains("invalid owner issue")));
        // The Windows drive path is rejected under its structural
        // classification, and the violation must not republish the path it
        // rejected.
        assert!(violations.iter().any(|violation| violation.contains("windows_drive")));
        assert!(
            !violations.iter().any(|violation| violation.contains("C:/private")),
            "violation echoed the private path: {violations:?}"
        );
    }

    #[test]
    fn boundary_registry_matches_baseline_identity_and_inventory() -> TestResult {
        let mut report = sample_compile_report();
        report.semantic_boundaries.push(sample_semantic_boundary());
        let series = sample_registry_series();
        let baseline =
            baseline_v2_from_report(&report, &series, &sample_baseline_v2_config(), None, &[])?;
        let registry = SemanticBoundaryRegistry {
            schema_version: SEMANTIC_BOUNDARY_REGISTRY_SCHEMA_VERSION.into(),
            entries: vec![sample_registry_entry()],
        };

        assert!(validate_registry_against_baseline(&registry, &baseline, false).is_empty());

        let mut changed = registry;
        changed.entries[0].manifest_hash = "wrong-manifest".into();
        let violations = validate_registry_against_baseline(&changed, &baseline, false);
        assert!(violations.iter().any(|violation| violation.contains("manifest_hash")));
        Ok(())
    }

    #[test]
    fn boundary_bundle_must_match_the_accepted_baseline_inventory() -> TestResult {
        let temp = tempfile::tempdir()?;
        let normalized = temp.path().join("normalized");
        fs::create_dir_all(&normalized)?;
        let index_path = temp.path().join("index.json");
        let boundary_path = normalized.join("semantic-boundaries.json");
        let mut report = sample_compile_report();
        report.semantic_boundaries.push(sample_semantic_boundary());
        let series = sample_registry_series();
        let baseline =
            baseline_v2_from_report(&report, &series, &sample_baseline_v2_config(), None, &[])?;
        fs::write(&boundary_path, serde_json::to_string_pretty(&baseline.semantic_boundaries)?)?;
        let index = EvidenceBundleIndex {
            schema_version: "perl_core_harness.evidence_bundle.v1".into(),
            bundle_id: "bundle-1".into(),
            series_id: baseline.series_id.clone(),
            manifest_hash: baseline.manifest_hash.clone(),
            repository_commit: baseline.repository_commit.clone(),
            profile: baseline.profile,
            runner: HarnessRunner::Test,
            perl_resolved_ref: "perl-ref".into(),
            lineage: EvidenceBundleLineage {
                measurement_sha: "abc".into(),
                publication_sha: None,
                landed_sha: None,
            },
            artifacts: vec![EvidenceBundleArtifact {
                kind: "semantic_boundaries".into(),
                logical_path: "normalized/semantic-boundaries.json".into(),
            }],
            completeness: EvidenceBundleCompleteness {
                status: "complete".into(),
                normalized_authority: true,
            },
            lifecycle: "published".into(),
        };
        fs::write(&index_path, serde_json::to_string_pretty(&index)?)?;

        let bundle = read_boundary_bundle(&index_path)?;
        assert!(validate_bundle_against_baseline(&bundle, &baseline).is_empty());

        let mut duplicate_boundaries = baseline.semantic_boundaries.clone();
        duplicate_boundaries.push(sample_semantic_boundary());
        fs::write(&boundary_path, serde_json::to_string_pretty(&duplicate_boundaries)?)?;
        let error = match read_boundary_bundle(&index_path) {
            Ok(_) => return Err(color_eyre::eyre::eyre!("duplicate boundary key was accepted")),
            Err(error) => error,
        };
        assert!(error.to_string().contains("duplicate boundary key"));

        let mut changed = baseline;
        changed.semantic_boundaries.clear();
        let violations = validate_bundle_against_baseline(&bundle, &changed);
        assert!(violations.iter().any(|violation| violation.contains("inventory")));
        Ok(())
    }

    fn sample_boundary_bundle() -> BoundaryBundle {
        BoundaryBundle {
            path: PathBuf::from("bundles/bundle-1/index.json"),
            index: EvidenceBundleIndex {
                schema_version: "perl_core_harness.evidence_bundle.v1".into(),
                bundle_id: "bundle-1".into(),
                series_id: "series-1".into(),
                manifest_hash: "manifest-1".into(),
                repository_commit: "abc".into(),
                profile: HarnessProfile::Base,
                runner: HarnessRunner::Test,
                perl_resolved_ref: "perl-ref".into(),
                lineage: EvidenceBundleLineage {
                    measurement_sha: "abc".into(),
                    publication_sha: None,
                    landed_sha: None,
                },
                artifacts: Vec::new(),
                completeness: EvidenceBundleCompleteness {
                    status: "complete".into(),
                    normalized_authority: true,
                },
                lifecycle: "published".into(),
            },
            semantic_boundaries: Vec::new(),
        }
    }

    #[test]
    fn failure_clusters_group_typed_failures_and_keep_debt_separate() -> TestResult {
        let bundle = sample_boundary_bundle();
        let mut report = sample_compile_report();
        report.failures = vec![
            sample_failure("base/lex.t", "parse_recovery"),
            sample_failure("base/ok.t", "parse_recovery"),
            sample_failure("base/harness.t", "harness_prepare"),
            sample_failure("base/hir.t", "hir_lowering"),
        ];
        let mut bundle = bundle;
        bundle.semantic_boundaries.push(sample_semantic_boundary());
        bundle.semantic_boundaries.push(ObservedSemanticBoundary {
            disposition: SemanticBoundaryDisposition::ImplementedStatic,
            id: "implemented_static".into(),
            ..sample_semantic_boundary()
        });
        bundle.semantic_boundaries.push(ObservedSemanticBoundary {
            disposition: SemanticBoundaryDisposition::OrdinaryRuntime,
            id: "ordinary_runtime".into(),
            ..sample_semantic_boundary()
        });

        let triage = build_failure_cluster_report(&bundle, &report)?;

        assert_eq!(triage.clusters.len(), 3);
        assert_eq!(triage.debt_candidates.len(), 1);
        let parse_cluster = triage
            .clusters
            .iter()
            .find(|cluster| cluster.signature.bucket == "parse_recovery")
            .ok_or_else(|| color_eyre::eyre::eyre!("missing parse cluster"))?;
        assert_eq!(parse_cluster.occurrence_count, 2);
        assert_eq!(parse_cluster.affected_files, vec!["base/lex.t", "base/ok.t"]);
        assert_eq!(parse_cluster.signature.fact_classes, vec!["parse_recovery"]);
        assert_ne!(parse_cluster.signature.fact_classes, parse_cluster.signature.lsp_surfaces);
        let harness_cluster = triage
            .clusters
            .iter()
            .find(|cluster| cluster.signature.bucket == "harness_prepare")
            .ok_or_else(|| color_eyre::eyre::eyre!("missing harness cluster"))?;
        assert_eq!(harness_cluster.impacted_layer, "harness_or_environment");
        let hir_cluster = triage
            .clusters
            .iter()
            .find(|cluster| cluster.signature.bucket == "hir_lowering")
            .ok_or_else(|| color_eyre::eyre::eyre!("missing HIR cluster"))?;
        assert_eq!(hir_cluster.signature.stage, "hir_unmodeled");
        Ok(())
    }

    #[test]
    fn failure_cluster_normalization_preserves_urls_and_scrubs_host_paths() -> TestResult {
        let normalized =
            normalize_diagnostic("see https://example.com/tmp and /tmp/perl/target/report");
        assert_eq!(normalized, "see https://example.com/tmp and <host-path>");
        Ok(())
    }

    #[test]
    fn failure_cluster_triage_rejects_non_compile_reports() -> TestResult {
        let bundle = sample_boundary_bundle();
        let error = match validate_bundle_report_identity(&bundle, &sample_parse_report()) {
            Ok(()) => return Err(color_eyre::eyre::eyre!("parse report was accepted by triage")),
            Err(error) => error,
        };
        if !error.to_string().contains("triage requires a compile report") {
            bail!("unexpected non-compile triage error: {error}");
        }
        Ok(())
    }

    #[test]
    fn failure_cluster_triage_rejects_runner_mismatch() -> TestResult {
        let bundle = sample_boundary_bundle();
        let mut report = sample_compile_report();
        report.runner = HarnessRunner::Harness;
        let error = match validate_bundle_report_identity(&bundle, &report) {
            Ok(()) => return Err(color_eyre::eyre::eyre!("runner mismatch was accepted")),
            Err(error) => error,
        };
        if !error.to_string().contains("identity does not match evidence bundle") {
            bail!("unexpected runner mismatch error: {error}");
        }
        Ok(())
    }

    #[test]
    fn failure_cluster_triage_rejects_invalid_report_shape() -> TestResult {
        let bundle = sample_boundary_bundle();
        let mut report = sample_compile_report();
        let result = report
            .file_results
            .first_mut()
            .ok_or_else(|| color_eyre::eyre::eyre!("sample compile report has no files"))?;
        result.status = RunnerStatus::Fail;
        validate_bundle_report_identity(&bundle, &report)?;
        let error = match ensure_valid_report_shape(&report) {
            Ok(()) => return Err(color_eyre::eyre::eyre!("invalid report shape was accepted")),
            Err(error) => error,
        };
        if !error.to_string().contains("failing file has no failure bucket record") {
            bail!("unexpected invalid report-shape error: {error}");
        }
        Ok(())
    }

    #[test]
    fn failure_cluster_ids_are_stable_when_failure_order_changes() -> TestResult {
        let bundle = sample_boundary_bundle();
        let mut report = sample_compile_report();
        report.failures = vec![
            sample_failure("base/ok.t", "parse_recovery"),
            sample_failure("base/lex.t", "parse_recovery"),
        ];
        let first = build_failure_cluster_report(&bundle, &report)?;
        report.failures.reverse();
        let second = build_failure_cluster_report(&bundle, &report)?;

        assert_eq!(serde_json::to_value(&first)?, serde_json::to_value(&second)?);
        report.failures.push(sample_failure("base/new.t", "parse_recovery"));
        let with_new_membership = build_failure_cluster_report(&bundle, &report)?;
        assert_eq!(second.clusters[0].cluster_id, with_new_membership.clusters[0].cluster_id);
        assert_eq!(with_new_membership.clusters[0].occurrence_count, 3);
        Ok(())
    }

    #[test]
    fn failure_cluster_output_normalizes_host_paths_and_line_endings() -> TestResult {
        let bundle = sample_boundary_bundle();
        let mut report = sample_compile_report();
        let mut failure = sample_failure("base\\ok.t", "parse_recovery");
        failure.first_diagnostic = "C:\\tmp\\perl\\base\\ok.t:1\r\nparse failure".into();
        report.failures.push(failure);

        let triage = build_failure_cluster_report(&bundle, &report)?;
        let representative = &triage.clusters[0].representative_failure;
        assert_eq!(representative.path, "base/ok.t");
        assert_eq!(representative.first_diagnostic, "<host-path> parse failure");
        Ok(())
    }

    #[test]
    fn cluster_history_preserves_missing_clusters_and_tracks_membership_growth() -> TestResult {
        let bundle = sample_boundary_bundle();
        let mut report = sample_compile_report();
        report.failures = vec![sample_failure("base/lex.t", "parse_recovery")];
        let first_report = build_failure_cluster_report(&bundle, &report)?;
        let history = merge_cluster_history(
            FailureClusterHistory {
                schema_version: FAILURE_CLUSTER_HISTORY_SCHEMA_VERSION.into(),
                entries: Vec::new(),
            },
            &first_report,
        )?;
        assert_eq!(history.entries.len(), 1);
        assert_eq!(history.entries[0].status, FailureClusterHistoryStatus::Unassigned);
        assert_eq!(history.entries[0].identity_quality, FailureClusterIdentityQuality::Provisional);
        assert_eq!(history.entries[0].first_seen_series_id, "series-1");
        assert_eq!(history.entries[0].last_seen_series_id, "series-1");
        assert_eq!(history.entries[0].first_seen_bundle, "bundle-1");

        let mut second_report = first_report.clone();
        second_report.bundle_id = "bundle-2".into();
        second_report.clusters[0].affected_files.push("base/ok.t".into());
        second_report.clusters[0].affected_files.sort();
        let history = merge_cluster_history(history, &second_report)?;
        assert_eq!(history.entries[0].first_seen_bundle, "bundle-1");
        assert_eq!(history.entries[0].last_seen_bundle, "bundle-2");
        assert_eq!(history.entries[0].historical_affected_files, vec!["base/lex.t", "base/ok.t"]);

        let mut absent_report = second_report;
        absent_report.bundle_id = "bundle-3".into();
        absent_report.clusters.clear();
        let stale_absence = validate_history_against_report(&history, &absent_report);
        assert!(stale_absence.iter().any(|violation| violation.contains("still marked current")));
        let history_after_absence = merge_cluster_history(history.clone(), &absent_report)?;
        assert_eq!(
            history_after_absence.entries[0].presence,
            FailureClusterHistoryPresence::AbsentUnresolved
        );
        assert!(!history_after_absence.entries[0].observed_in_current_bundle);
        assert_eq!(
            history_after_absence.entries[0].absence_since_bundle.as_deref(),
            Some("bundle-3")
        );
        assert!(history_after_absence.entries[0].current_affected_files.is_empty());
        assert!(history_after_absence.entries[0].current_stage.is_none());
        assert!(validate_history_against_report(&history_after_absence, &absent_report).is_empty());

        let mut second_absent_report = absent_report;
        second_absent_report.bundle_id = "bundle-4".into();
        let history_after_second_absence =
            merge_cluster_history(history_after_absence, &second_absent_report)?;
        assert_eq!(
            history_after_second_absence.entries[0].absence_since_bundle.as_deref(),
            Some("bundle-3")
        );
        assert!(
            validate_history_against_report(&history_after_second_absence, &second_absent_report)
                .is_empty()
        );
        Ok(())
    }

    #[test]
    fn cluster_history_rejects_stale_identity_and_unproven_resolution() -> TestResult {
        let bundle = sample_boundary_bundle();
        let mut report = sample_compile_report();
        report.failures.push(sample_failure("base/lex.t", "parse_recovery"));
        let cluster_report = build_failure_cluster_report(&bundle, &report)?;
        let mut history = merge_cluster_history(
            FailureClusterHistory {
                schema_version: FAILURE_CLUSTER_HISTORY_SCHEMA_VERSION.into(),
                entries: Vec::new(),
            },
            &cluster_report,
        )?;
        history.entries[0].manifest_hash = "stale-manifest".into();
        let stale = validate_history_against_report(&history, &cluster_report);
        assert!(stale.iter().any(|violation| violation.contains("stale series identity")));

        history.entries[0].manifest_hash = cluster_report.manifest_hash.clone();
        history.entries[0].presence = FailureClusterHistoryPresence::Resolved;
        let inverse = validate_cluster_history_shape(&history);
        assert!(inverse.iter().any(|violation| violation.contains("resolved-presence cluster")));
        history.entries[0].presence = FailureClusterHistoryPresence::Observed;
        history.entries[0].status = FailureClusterHistoryStatus::Resolved;
        history.entries[0].owner_issue = Some("#5175".into());
        let invalid = validate_cluster_history_shape(&history);
        assert!(invalid.iter().any(|violation| violation.contains("resolution PR")));
        assert!(invalid.iter().any(|violation| violation.contains("resolution bundle")));
        assert!(invalid.iter().any(|violation| violation.contains("before/after transition")));
        Ok(())
    }

    #[test]
    fn cluster_history_accepts_only_an_explicit_bound_transition() -> TestResult {
        let bundle = sample_boundary_bundle();
        let mut report = sample_compile_report();
        report.failures.push(sample_failure("base/lex.t", "parse_recovery"));
        let cluster_report = build_failure_cluster_report(&bundle, &report)?;
        let mut history = merge_cluster_history(
            FailureClusterHistory {
                schema_version: FAILURE_CLUSTER_HISTORY_SCHEMA_VERSION.into(),
                entries: Vec::new(),
            },
            &cluster_report,
        )?;
        let entry = &mut history.entries[0];
        entry.owner_issue = Some("#5175".into());
        entry.status = FailureClusterHistoryStatus::Resolved;
        entry.presence = FailureClusterHistoryPresence::Resolved;
        entry.observed_in_current_bundle = false;
        entry.current_authority_bundle = None;
        entry.absence_since_bundle = Some("bundle-2".into());
        entry.current_affected_files.clear();
        entry.current_fact_classes.clear();
        entry.current_lsp_surfaces.clear();
        entry.current_stage = None;
        entry.resolution_pr = Some("#5300".into());
        entry.resolution_bundle = Some("bundle-2".into());
        let cluster_id = entry.cluster_id.clone();
        let first_seen_series_id = entry.first_seen_series_id.clone();
        let first_seen_manifest_hash = entry.first_seen_manifest_hash.clone();
        let first_seen_bundle = entry.first_seen_bundle.clone();
        entry.transitions.push(FailureClusterHistoryTransition {
            transition_id: "transition-1".into(),
            from_cluster_id: cluster_id,
            to_cluster_id: None,
            to_presence: FailureClusterHistoryPresence::Resolved,
            from_stage: "compile_effect".into(),
            to_stage: "general_semantics".into(),
            before_series_id: first_seen_series_id,
            before_manifest_hash: first_seen_manifest_hash,
            before_bundle_id: first_seen_bundle,
            after_series_id: "series-1".into(),
            after_manifest_hash: "manifest-1".into(),
            after_bundle_id: "bundle-2".into(),
            proof_plan: "focused typed proof plus exact-series replay".into(),
            stop_condition: "the cluster no longer emits".into(),
            implementation_pr: Some("#5300".into()),
        });
        assert!(validate_cluster_history_shape(&history).is_empty());
        Ok(())
    }

    #[test]
    fn compatibility_loader_keeps_rails_separate_and_optional_evidence_explicit() -> TestResult {
        let temp = tempfile::tempdir()?;
        let series = build_series_manifest(
            &sample_discovery_report(),
            &sample_series_config(),
            "2026-07-02T00:00:00Z".into(),
        )?;
        let baseline = baseline_v2_from_report(
            &sample_compile_report(),
            &series,
            &sample_baseline_v2_config(),
            None,
            &[],
        )?;
        let series_path = temp.path().join("series.json");
        let parse_path = temp.path().join("parse.json");
        let compile_path = temp.path().join("compile.json");
        let baseline_path = temp.path().join("baseline.json");
        let accepted_path = temp.path().join("accepted-baseline.json");
        let index_path = temp.path().join("bundle").join("index.json");
        let normalized = index_path
            .parent()
            .ok_or_else(|| color_eyre::eyre::eyre!("missing bundle parent"))?
            .join("normalized");
        fs::create_dir_all(&normalized)?;
        fs::write(&series_path, serde_json::to_string_pretty(&series)?)?;
        write_run_report(&parse_path, &sample_parse_report())?;
        write_run_report(&compile_path, &sample_compile_report())?;
        write_compile_baseline_v2(&baseline_path, &baseline)?;
        write_compile_baseline_v2(&accepted_path, &baseline)?;
        fs::write(
            normalized.join("semantic-boundaries.json"),
            serde_json::to_string_pretty(&baseline.semantic_boundaries)?,
        )?;
        fs::write(normalized.join("compile.json"), fs::read_to_string(&compile_path)?)?;
        let mut index = sample_boundary_bundle().index;
        index.series_id = series.series_id.clone();
        index.manifest_hash = series.manifest_hash.clone();
        index.repository_commit = series.repository_commit.clone();
        index.profile = series.profile;
        index.perl_resolved_ref = series.perl_resolved_ref.clone();
        index.artifacts = vec![
            EvidenceBundleArtifact {
                kind: "semantic_boundaries".into(),
                logical_path: "normalized/semantic-boundaries.json".into(),
            },
            EvidenceBundleArtifact {
                kind: "compile_report".into(),
                logical_path: "normalized/compile.json".into(),
            },
        ];
        fs::write(&index_path, serde_json::to_string_pretty(&index)?)?;

        let state = load_compatibility_state(CompatibilityLoadConfig {
            inputs: vec![CompatibilitySeriesInput {
                series_manifest: series_path,
                parse_report: parse_path,
                compile_report: normalized.join("compile.json"),
                compile_baseline: baseline_path,
                accepted_baseline: Some(accepted_path),
                evidence_bundle: index_path,
                boundary_registry: None,
                cluster_history: None,
                execute_report: None,
                current_authority: None,
            }],
            repository_commit: "abc".into(),
        })?;

        assert_eq!(state.schema_version, COMPILER_COMPATIBILITY_SCHEMA_VERSION);
        assert_eq!(state.series.len(), 1);
        assert_eq!(state.series[0].identity.denominator, 2);
        assert_eq!(state.series[0].parse.files_passed, 2);
        assert_eq!(state.series[0].compile.files_passed, 2);
        assert_eq!(
            state.series[0].transition_candidate.transition,
            CompatibilityTransition::NoChange
        );
        assert_eq!(
            state.series[0].curated_gold.availability,
            CompatibilityRailAvailability::NotAvailable
        );
        assert_eq!(
            state.series[0].debt.registry.availability,
            CompatibilityRailAvailability::NotAvailable
        );
        let encoded = serde_json::to_string_pretty(&state)?;
        assert!(encoded.contains("not_available"));
        let decoded: CompilerCompatibilityState = serde_json::from_str(&encoded)?;
        assert_eq!(decoded, state);
        Ok(())
    }

    #[test]
    fn compatibility_transition_classifies_regression_without_lowering_ratchet() -> TestResult {
        let series = build_series_manifest(
            &sample_discovery_report(),
            &sample_series_config(),
            "2026-07-02T00:00:00Z".into(),
        )?;
        let mut accepted = baseline_v2_from_report(
            &sample_compile_report(),
            &series,
            &sample_baseline_v2_config(),
            None,
            &[],
        )?;
        accepted.files_total = 25;
        accepted.files_passed = 25;
        let mut current = sample_compile_report();
        current.summary.files_total = 25;
        current.summary.files_passed = 24;
        current.summary.files_failed = 1;

        let (transition, reason, requires_acceptance) =
            classify_compatibility_transition(&accepted, &current);
        assert_eq!(transition, CompatibilityTransition::Regression);
        assert!(reason.contains("regressed"));
        assert!(!requires_acceptance);
        assert_eq!(accepted.files_passed, 25);
        Ok(())
    }

    #[test]
    fn compatibility_transition_classifies_improvement_as_candidate() -> TestResult {
        let series = build_series_manifest(
            &sample_discovery_report(),
            &sample_series_config(),
            "2026-07-02T00:00:00Z".into(),
        )?;
        let mut accepted = baseline_v2_from_report(
            &sample_compile_report(),
            &series,
            &sample_baseline_v2_config(),
            None,
            &[],
        )?;
        accepted.files_total = 25;
        accepted.files_passed = 24;
        let mut current = sample_compile_report();
        current.summary.files_total = 25;
        current.summary.files_passed = 25;
        current.summary.files_failed = 0;

        let (transition, reason, requires_acceptance) =
            classify_compatibility_transition(&accepted, &current);
        assert_eq!(transition, CompatibilityTransition::ImprovementCandidate);
        assert!(reason.contains("improved"));
        assert!(requires_acceptance);
        Ok(())
    }

    #[test]
    fn compatibility_authority_artifact_binding_rejects_local_tampering() -> TestResult {
        let temp = tempfile::tempdir()?;
        let observation = temp.path().join("bundle.json");
        let accepted = temp.path().join("accepted.json");
        fs::write(&observation, br#"{"bundle":"original"}"#)?;
        fs::write(&accepted, br#"{"baseline":"original"}"#)?;
        let observation_digest = sha256_digest_bytes(&fs::read(&observation)?);
        let accepted_digest = sha256_digest_bytes(&fs::read(&accepted)?);

        validate_authority_artifact_bindings(
            &observation,
            &accepted,
            "bundle.json",
            &observation_digest,
            Some("accepted.json"),
            Some(&accepted_digest),
            Some(temp.path()),
        )?;

        fs::write(&observation, br#"{"bundle":"tampered"}"#)?;
        let error = validate_authority_artifact_bindings(
            &observation,
            &accepted,
            "bundle.json",
            &observation_digest,
            Some("accepted.json"),
            Some(&accepted_digest),
            Some(temp.path()),
        );
        assert!(error.is_err());
        Ok(())
    }

    #[test]
    fn failure_cluster_triage_rejects_unknown_buckets() -> TestResult {
        let bundle = sample_boundary_bundle();
        let mut report = sample_compile_report();
        report.failures.push(sample_failure("base/lex.t", "unclassified"));

        let error = match build_failure_cluster_report(&bundle, &report) {
            Ok(_) => return Err(color_eyre::eyre::eyre!("unknown bucket was accepted")),
            Err(error) => error,
        };
        assert!(error.to_string().contains("unclassifiable failure bucket"));
        Ok(())
    }

    fn sample_parse_report() -> RunReport {
        let mut report = sample_compile_report();
        report.mode = HarnessMode::Parse;
        report
    }

    fn sample_execute_report() -> RunReport {
        RunReport {
            schema_version: RUN_REPORT_SCHEMA_VERSION.into(),
            commit: "abc".into(),
            timestamp: "2026-07-02T00:00:00Z".into(),
            perl_ref: "unknown".into(),
            prepared_tree: "/tmp/perl".into(),
            run_tree: "/tmp/run".into(),
            host_perl: "perl".into(),
            runner: HarnessRunner::Test,
            mode: HarnessMode::Execute,
            profile: HarnessProfile::Base,
            harness_status: Some(1),
            summary: RunSummary {
                files_total: 6,
                files_passed: 6,
                files_failed: 0,
                tap_assertions_total: 325,
                tap_assertions_passed: 325,
            },
            buckets: BTreeMap::new(),
            file_results: vec![
                RunFileResult {
                    path: "base/cond.t".into(),
                    status: RunnerStatus::Pass,
                    assertions_passed: 4,
                    assertions_total: 4,
                },
                RunFileResult {
                    path: "base/if.t".into(),
                    status: RunnerStatus::Pass,
                    assertions_passed: 2,
                    assertions_total: 2,
                },
                RunFileResult {
                    path: "base/num.t".into(),
                    status: RunnerStatus::Pass,
                    assertions_passed: 56,
                    assertions_total: 56,
                },
                RunFileResult {
                    path: "base/pat.t".into(),
                    status: RunnerStatus::Pass,
                    assertions_passed: 2,
                    assertions_total: 2,
                },
                RunFileResult {
                    path: "base/translate.t".into(),
                    status: RunnerStatus::Pass,
                    assertions_passed: 257,
                    assertions_total: 257,
                },
                RunFileResult {
                    path: "base/while.t".into(),
                    status: RunnerStatus::Pass,
                    assertions_passed: 4,
                    assertions_total: 4,
                },
            ],
            failures: Vec::new(),
            semantic_boundaries: Vec::new(),
        }
    }

    fn sample_discovery_report() -> DiscoveryReport {
        DiscoveryReport {
            schema_version: DISCOVERY_SCHEMA_VERSION.into(),
            commit: "abc".into(),
            timestamp: "2026-07-02T00:00:00Z".into(),
            perl_ref: "perl-ref".into(),
            prepared_tree: "/tmp/perl".into(),
            host_perl: "perl".into(),
            runner: HarnessRunner::Test,
            profile: HarnessProfile::Base,
            tests: vec![
                DiscoveredTest { path: "base/lex.t".into(), root: "base".into() },
                DiscoveredTest { path: "base/ok.t".into(), root: "base".into() },
            ],
        }
    }

    fn sample_series_config() -> SeriesManifestConfig {
        SeriesManifestConfig {
            discovery: PathBuf::from("discovery.json"),
            output: Some(PathBuf::from("series.json")),
            series_id: "selected-base-perl-5.42.2".into(),
            profile: HarnessProfile::Base,
            perl_requested_ref: "perl-5.42.2".into(),
            perl_resolved_ref: "perl-ref".into(),
            preparation_receipt_id: "prepare-1".into(),
            preparation_receipt_digest: "sha256:prepare".into(),
            compiler_subject_identity: "compiler-subject-1".into(),
            invocation_identity: "invocation-1".into(),
            capability_identity: "capability-1".into(),
            environment_identity: "environment-1".into(),
            replaces_series_id: None,
            change_reason: Some("initial selected series".into()),
            check: false,
        }
    }

    fn sample_baseline_v2_config() -> BaselineConfig {
        BaselineConfig {
            mode: HarnessMode::Compile,
            profile: HarnessProfile::Base,
            report: None,
            baseline: None,
            accept: true,
            series: None,
            previous_baseline: None,
            boundary_retirements: None,
            compiler_subject_identity: Some("compiler-subject-1".into()),
            invocation_identity: Some("invocation-1".into()),
            capability_identity: Some("capability-1".into()),
            environment_identity: Some("environment-1".into()),
            accepted_transition_id: None,
            evidence_bundle: Some("bundle-sha256:example".into()),
        }
    }

    fn sample_smoke_config(modes: Vec<HarnessMode>) -> SmokeConfig {
        SmokeConfig {
            perl_tree: PathBuf::from("/tmp/perl"),
            host_perl: PathBuf::from("perl"),
            runner: HarnessRunner::Test,
            profile: HarnessProfile::Base,
            modes,
            output_dir: Some(PathBuf::from("target/perl-core/smoke")),
            runner_binary: Some(PathBuf::from("runner")),
            perl_ref: Some("perl-ref".into()),
        }
    }

    fn mark_file_failed(report: &mut RunReport, path: &str, bucket: &str) {
        if let Some(result) = report.file_results.iter_mut().find(|result| result.path == path) {
            result.status = RunnerStatus::Fail;
            result.assertions_passed = 0;
        }
        report.failures.push(sample_failure(path, bucket));
        report.buckets.insert(bucket.to_string(), 1);
        report.summary.files_passed = report.summary.files_passed.saturating_sub(1);
        report.summary.files_failed = report.summary.files_failed.saturating_add(1);
        report.summary.tap_assertions_passed =
            report.summary.tap_assertions_passed.saturating_sub(1);
    }

    fn sample_failure(path: &str, bucket: &str) -> RunFailure {
        RunFailure {
            path: path.to_string(),
            phase: "compile".into(),
            bucket: bucket.into(),
            first_diagnostic: "sample failure".into(),
            workstream: workstream_for_bucket(bucket).into(),
            lsp_impact: lsp_impact_for_bucket(bucket)
                .into_iter()
                .map(ToString::to_string)
                .collect(),
        }
    }

    fn assert_violation(comparison: &BaselineComparison, kind: BaselineViolationKind) {
        assert!(
            comparison.violations.iter().any(|violation| violation.kind == kind),
            "expected {kind:?} in {comparison:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn discover_invokes_dumptests_and_writes_manifest() -> TestResult {
        let temp = tempfile::tempdir()?;
        let perl_tree = write_fake_perl_tree(temp.path())?;
        let output = temp.path().join("discovery.json");

        discover(DiscoverConfig {
            perl_tree: perl_tree.clone(),
            host_perl: PathBuf::from("/bin/sh"),
            runner: HarnessRunner::Test,
            profile: HarnessProfile::Base,
            output: Some(output.clone()),
        })?;

        let raw = fs::read_to_string(output)?;
        let report: DiscoveryReport = serde_json::from_str(&raw)?;
        assert_eq!(report.schema_version, DISCOVERY_SCHEMA_VERSION);
        assert_eq!(report.runner, HarnessRunner::Test);
        assert_eq!(report.profile, HarnessProfile::Base);
        assert_eq!(report.prepared_tree, perl_tree.canonicalize()?.display().to_string());
        assert_eq!(report.host_perl, "/bin/sh");
        assert_eq!(
            report.tests,
            vec![DiscoveredTest { path: "base/ok.t".into(), root: "base".into() }]
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn run_mode_invokes_wrapper_and_writes_parse_report() -> TestResult {
        let temp = tempfile::tempdir()?;
        let perl_tree = write_fake_perl_tree(temp.path())?;
        let runner = write_fake_runner(temp.path(), RunnerStatus::Pass)?;
        let output = temp.path().join("parse-report.json");

        run_mode(RunConfig {
            perl_tree: perl_tree.clone(),
            host_perl: PathBuf::from("/bin/sh"),
            runner: HarnessRunner::Test,
            mode: HarnessMode::Parse,
            profile: HarnessProfile::Base,
            tests: Vec::new(),
            output: Some(output.clone()),
            runner_binary: Some(runner),
        })?;

        let raw = fs::read_to_string(output)?;
        let report: RunReport = serde_json::from_str(&raw)?;
        assert_eq!(report.summary.files_total, 1);
        assert_eq!(report.summary.files_passed, 1);
        assert_eq!(report.summary.files_failed, 0);
        assert_eq!(report.file_results[0].path, "base/ok.t");
        assert_eq!(report.file_results[0].status, RunnerStatus::Pass);
        assert!(!perl_tree.join("t").join("perl").exists(), "source Perl tree must not be mutated");
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn run_mode_generated_fixture_smoke_runs_two_base_tests() -> TestResult {
        let temp = tempfile::tempdir()?;
        let perl_tree = write_fake_perl_tree_with_two_base_tests(temp.path())?;
        let runner = write_fake_runner(temp.path(), RunnerStatus::Pass)?;
        let output = temp.path().join("parse-report.json");

        run_mode(RunConfig {
            perl_tree: perl_tree.clone(),
            host_perl: PathBuf::from("/bin/sh"),
            runner: HarnessRunner::Test,
            mode: HarnessMode::Parse,
            profile: HarnessProfile::Base,
            tests: Vec::new(),
            output: Some(output.clone()),
            runner_binary: Some(runner),
        })?;

        let raw = fs::read_to_string(output)?;
        let report: RunReport = serde_json::from_str(&raw)?;
        assert_eq!(report.summary.files_total, 2);
        assert_eq!(report.summary.files_passed, 2);
        assert_eq!(report.summary.files_failed, 0);
        let mut paths =
            report.file_results.iter().map(|result| result.path.as_str()).collect::<Vec<_>>();
        paths.sort_unstable();
        assert_eq!(paths, vec!["base/lex.t", "base/ok.t"]);
        assert!(report.file_results.iter().all(|result| result.status == RunnerStatus::Pass));
        assert!(!perl_tree.join("t").join("perl").exists(), "source Perl tree must not be mutated");
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn run_mode_installs_wrapper_before_run_copy_dumptests() -> TestResult {
        let temp = tempfile::tempdir()?;
        let perl_tree = write_fake_perl_tree_requiring_t_perl_for_dumptests(temp.path())?;
        let runner = write_fake_runner(temp.path(), RunnerStatus::Pass)?;
        let output = temp.path().join("parse-report.json");

        run_mode(RunConfig {
            perl_tree: perl_tree.clone(),
            host_perl: PathBuf::from("/bin/sh"),
            runner: HarnessRunner::Test,
            mode: HarnessMode::Parse,
            profile: HarnessProfile::Base,
            tests: Vec::new(),
            output: Some(output.clone()),
            runner_binary: Some(runner),
        })?;

        let raw = fs::read_to_string(output)?;
        let report: RunReport = serde_json::from_str(&raw)?;
        assert_eq!(report.summary.files_total, 1);
        assert_eq!(report.summary.files_passed, 1);
        assert!(!perl_tree.join("t").join("perl").exists(), "source Perl tree must not be mutated");
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn run_mode_generated_fixture_smoke_runs_two_compile_tests() -> TestResult {
        let temp = tempfile::tempdir()?;
        let perl_tree = write_fake_perl_tree_with_two_base_tests(temp.path())?;
        let runner = write_fake_runner(temp.path(), RunnerStatus::Pass)?;
        let output = temp.path().join("compile-report.json");

        run_mode(RunConfig {
            perl_tree: perl_tree.clone(),
            host_perl: PathBuf::from("/bin/sh"),
            runner: HarnessRunner::Test,
            mode: HarnessMode::Compile,
            profile: HarnessProfile::Base,
            tests: Vec::new(),
            output: Some(output.clone()),
            runner_binary: Some(runner),
        })?;

        let raw = fs::read_to_string(output)?;
        let report: RunReport = serde_json::from_str(&raw)?;
        assert_eq!(report.mode, HarnessMode::Compile);
        assert_eq!(report.summary.files_total, 2);
        assert_eq!(report.summary.files_passed, 2);
        assert_eq!(report.summary.files_failed, 0);
        let mut paths =
            report.file_results.iter().map(|result| result.path.as_str()).collect::<Vec<_>>();
        paths.sort_unstable();
        assert_eq!(paths, vec!["base/lex.t", "base/ok.t"]);
        assert!(report.file_results.iter().all(|result| result.status == RunnerStatus::Pass));
        assert!(!perl_tree.join("t").join("perl").exists(), "source Perl tree must not be mutated");
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn run_mode_execute_runs_selected_base_if_test() -> TestResult {
        let temp = tempfile::tempdir()?;
        let perl_tree = write_fake_perl_tree_with_base_if_test(temp.path())?;
        let runner = write_fake_execute_runner(temp.path())?;
        let output = temp.path().join("execute-report.json");

        run_mode(RunConfig {
            perl_tree: perl_tree.clone(),
            host_perl: PathBuf::from("/bin/sh"),
            runner: HarnessRunner::Test,
            mode: HarnessMode::Execute,
            profile: HarnessProfile::Base,
            tests: vec!["base/if.t".into()],
            output: Some(output.clone()),
            runner_binary: Some(runner),
        })?;

        let raw = fs::read_to_string(output)?;
        let report: RunReport = serde_json::from_str(&raw)?;
        assert_eq!(report.mode, HarnessMode::Execute);
        assert_eq!(report.summary.files_total, 1);
        assert_eq!(report.summary.files_passed, 1);
        assert_eq!(report.summary.files_failed, 0);
        assert_eq!(report.summary.tap_assertions_total, 2);
        assert_eq!(report.summary.tap_assertions_passed, 2);
        assert_eq!(report.file_results[0].path, "base/if.t");
        assert_eq!(report.file_results[0].assertions_total, 2);
        assert!(!perl_tree.join("t").join("perl").exists(), "source Perl tree must not be mutated");
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn run_mode_execute_runs_selected_base_subset() -> TestResult {
        let temp = tempfile::tempdir()?;
        let perl_tree = write_fake_perl_tree_with_base_execute_subset(temp.path())?;
        let runner = write_fake_execute_runner(temp.path())?;
        let output = temp.path().join("execute-report.json");

        run_mode(RunConfig {
            perl_tree: perl_tree.clone(),
            host_perl: PathBuf::from("/bin/sh"),
            runner: HarnessRunner::Test,
            mode: HarnessMode::Execute,
            profile: HarnessProfile::Base,
            tests: vec![
                "base/if.t".into(),
                "base/cond.t".into(),
                "base/num.t".into(),
                "base/pat.t".into(),
                "base/translate.t".into(),
                "base/while.t".into(),
            ],
            output: Some(output.clone()),
            runner_binary: Some(runner),
        })?;

        let raw = fs::read_to_string(output)?;
        let report: RunReport = serde_json::from_str(&raw)?;
        assert_eq!(report.mode, HarnessMode::Execute);
        assert_eq!(report.summary.files_total, 6);
        assert_eq!(report.summary.files_passed, 6);
        assert_eq!(report.summary.files_failed, 0);
        assert_eq!(report.summary.tap_assertions_total, 325);
        assert_eq!(report.summary.tap_assertions_passed, 325);
        let mut paths =
            report.file_results.iter().map(|result| result.path.as_str()).collect::<Vec<_>>();
        paths.sort_unstable();
        assert_eq!(
            paths,
            vec![
                "base/cond.t",
                "base/if.t",
                "base/num.t",
                "base/pat.t",
                "base/translate.t",
                "base/while.t",
            ]
        );
        assert!(!perl_tree.join("t").join("perl").exists(), "source Perl tree must not be mutated");
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn smoke_writes_discovery_parse_compile_and_summary_reports() -> TestResult {
        let temp = tempfile::tempdir()?;
        let perl_tree = write_fake_perl_tree_with_two_base_tests(temp.path())?;
        let runner = write_fake_runner(temp.path(), RunnerStatus::Pass)?;
        let output_dir = temp.path().join("smoke");

        smoke(SmokeConfig {
            perl_tree: perl_tree.clone(),
            host_perl: PathBuf::from("/bin/sh"),
            runner: HarnessRunner::Test,
            profile: HarnessProfile::Base,
            modes: vec![HarnessMode::Parse, HarnessMode::Compile],
            output_dir: Some(output_dir.clone()),
            runner_binary: Some(runner),
            perl_ref: Some("fake-ref".into()),
        })?;

        for file in ["discovery.json", "parse.json", "compile.json", "gap-map.json", "smoke.json"] {
            assert!(output_dir.join(file).is_file(), "{file} should be written");
        }
        let raw = fs::read_to_string(output_dir.join("smoke.json"))?;
        let report: SmokeReport = serde_json::from_str(&raw)?;
        assert_eq!(report.schema_version, SMOKE_SCHEMA_VERSION);
        assert_eq!(report.status, SmokeStatus::Pass);
        assert_eq!(report.discovery_total, 2);
        assert_eq!(report.parse_files_total, Some(2));
        assert_eq!(report.parse_files_passed, Some(2));
        assert_eq!(report.compile_files_total, Some(2));
        assert_eq!(report.compile_files_passed, Some(2));
        assert!(report.structural_failures.is_empty());
        assert!(!perl_tree.join("t").join("perl").exists(), "source Perl tree must not be mutated");
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn smoke_writes_comp_profile_receipts() -> TestResult {
        let temp = tempfile::tempdir()?;
        let perl_tree = write_fake_perl_tree_with_two_comp_tests(temp.path())?;
        let runner = write_fake_runner(temp.path(), RunnerStatus::Pass)?;
        let output_dir = temp.path().join("smoke-comp");

        smoke(SmokeConfig {
            perl_tree: perl_tree.clone(),
            host_perl: PathBuf::from("/bin/sh"),
            runner: HarnessRunner::Test,
            profile: HarnessProfile::Comp,
            modes: vec![HarnessMode::Parse, HarnessMode::Compile],
            output_dir: Some(output_dir.clone()),
            runner_binary: Some(runner),
            perl_ref: Some("fake-ref".into()),
        })?;

        for file in ["discovery.json", "parse.json", "compile.json", "gap-map.json", "smoke.json"] {
            assert!(output_dir.join(file).is_file(), "{file} should be written");
        }

        let discovery: DiscoveryReport =
            serde_json::from_str(&fs::read_to_string(output_dir.join("discovery.json"))?)?;
        assert_eq!(discovery.profile, HarnessProfile::Comp);
        let mut discovered =
            discovery.tests.iter().map(|test| test.path.as_str()).collect::<Vec<_>>();
        discovered.sort_unstable();
        assert_eq!(discovered, vec!["comp/require.t", "comp/use.t"]);

        let smoke_report: SmokeReport =
            serde_json::from_str(&fs::read_to_string(output_dir.join("smoke.json"))?)?;
        assert_eq!(smoke_report.profile, HarnessProfile::Comp);
        assert_eq!(smoke_report.status, SmokeStatus::Pass);
        assert_eq!(smoke_report.discovery_total, 2);
        assert_eq!(smoke_report.parse_files_passed, Some(2));
        assert_eq!(smoke_report.compile_files_passed, Some(2));
        assert!(smoke_report.structural_failures.is_empty());
        assert!(!perl_tree.join("t").join("perl").exists(), "source Perl tree must not be mutated");
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn smoke_writes_run_profile_receipts() -> TestResult {
        let temp = tempfile::tempdir()?;
        let perl_tree = write_fake_perl_tree_with_two_run_tests(temp.path())?;
        let runner = write_fake_runner(temp.path(), RunnerStatus::Pass)?;
        let output_dir = temp.path().join("smoke-run");

        smoke(SmokeConfig {
            perl_tree: perl_tree.clone(),
            host_perl: PathBuf::from("/bin/sh"),
            runner: HarnessRunner::Test,
            profile: HarnessProfile::Run,
            modes: vec![HarnessMode::Parse, HarnessMode::Compile],
            output_dir: Some(output_dir.clone()),
            runner_binary: Some(runner),
            perl_ref: Some("fake-ref".into()),
        })?;

        for file in ["discovery.json", "parse.json", "compile.json", "gap-map.json", "smoke.json"] {
            assert!(output_dir.join(file).is_file(), "{file} should be written");
        }

        let discovery: DiscoveryReport =
            serde_json::from_str(&fs::read_to_string(output_dir.join("discovery.json"))?)?;
        assert_eq!(discovery.profile, HarnessProfile::Run);
        let mut discovered =
            discovery.tests.iter().map(|test| test.path.as_str()).collect::<Vec<_>>();
        discovered.sort_unstable();
        assert_eq!(discovered, vec!["run/import.t", "run/switches.t"]);

        let smoke_report: SmokeReport =
            serde_json::from_str(&fs::read_to_string(output_dir.join("smoke.json"))?)?;
        assert_eq!(smoke_report.profile, HarnessProfile::Run);
        assert_eq!(smoke_report.status, SmokeStatus::Pass);
        assert_eq!(smoke_report.discovery_total, 2);
        assert_eq!(smoke_report.parse_files_passed, Some(2));
        assert_eq!(smoke_report.compile_files_passed, Some(2));
        assert!(smoke_report.structural_failures.is_empty());
        assert!(!perl_tree.join("t").join("perl").exists(), "source Perl tree must not be mutated");
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn smoke_preserves_bucketed_parse_failures_as_gap_receipts() -> TestResult {
        let temp = tempfile::tempdir()?;
        let perl_tree = write_fake_perl_tree(temp.path())?;
        let runner = write_fake_runner(temp.path(), RunnerStatus::Fail)?;
        let output_dir = temp.path().join("smoke");

        smoke(SmokeConfig {
            perl_tree,
            host_perl: PathBuf::from("/bin/sh"),
            runner: HarnessRunner::Test,
            profile: HarnessProfile::Base,
            modes: vec![HarnessMode::Parse],
            output_dir: Some(output_dir.clone()),
            runner_binary: Some(runner),
            perl_ref: Some("fake-ref".into()),
        })?;

        let raw = fs::read_to_string(output_dir.join("smoke.json"))?;
        let report: SmokeReport = serde_json::from_str(&raw)?;
        assert_eq!(report.status, SmokeStatus::Pass);
        assert_eq!(report.parse_files_failed, Some(1));
        assert_eq!(
            report.parse_buckets.as_ref().and_then(|buckets| buckets.get("parse_recovery")),
            Some(&1)
        );
        assert!(output_dir.join("parse.json").is_file());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn smoke_preserves_bucketed_compile_failures_as_gap_receipts() -> TestResult {
        let temp = tempfile::tempdir()?;
        let perl_tree = write_fake_perl_tree(temp.path())?;
        let runner =
            write_fake_runner_with_bucket(temp.path(), RunnerStatus::Fail, Some("compile_effect"))?;
        let output_dir = temp.path().join("smoke");

        smoke(SmokeConfig {
            perl_tree,
            host_perl: PathBuf::from("/bin/sh"),
            runner: HarnessRunner::Test,
            profile: HarnessProfile::Base,
            modes: vec![HarnessMode::Compile],
            output_dir: Some(output_dir.clone()),
            runner_binary: Some(runner),
            perl_ref: Some("fake-ref".into()),
        })?;

        let raw = fs::read_to_string(output_dir.join("smoke.json"))?;
        let report: SmokeReport = serde_json::from_str(&raw)?;
        assert_eq!(report.status, SmokeStatus::Pass);
        assert_eq!(report.compile_files_failed, Some(1));
        assert_eq!(
            report.compile_buckets.as_ref().and_then(|buckets| buckets.get("compile_effect")),
            Some(&1)
        );
        assert!(output_dir.join("compile.json").is_file());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn smoke_fails_when_runner_emits_unknown_bucket() -> TestResult {
        let temp = tempfile::tempdir()?;
        let perl_tree = write_fake_perl_tree(temp.path())?;
        let runner =
            write_fake_runner_with_bucket(temp.path(), RunnerStatus::Fail, Some("unknown"))?;
        let output_dir = temp.path().join("smoke");

        let Err(err) = smoke(SmokeConfig {
            perl_tree,
            host_perl: PathBuf::from("/bin/sh"),
            runner: HarnessRunner::Test,
            profile: HarnessProfile::Base,
            modes: vec![HarnessMode::Compile],
            output_dir: Some(output_dir.clone()),
            runner_binary: Some(runner),
            perl_ref: Some("fake-ref".into()),
        }) else {
            bail!("unknown bucket should fail smoke receipt integrity");
        };

        assert!(err.to_string().contains("receipt integrity"));
        let raw = fs::read_to_string(output_dir.join("smoke.json"))?;
        let report: SmokeReport = serde_json::from_str(&raw)?;
        assert_eq!(report.status, SmokeStatus::Fail);
        assert!(
            report
                .structural_failures
                .iter()
                .any(|failure| failure.kind == SmokeFailureKind::UnknownBucket)
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn smoke_fails_when_runner_omits_bucket() -> TestResult {
        let temp = tempfile::tempdir()?;
        let perl_tree = write_fake_perl_tree(temp.path())?;
        let runner = write_fake_runner_with_bucket(temp.path(), RunnerStatus::Fail, None)?;
        let output_dir = temp.path().join("smoke");

        let Err(err) = smoke(SmokeConfig {
            perl_tree,
            host_perl: PathBuf::from("/bin/sh"),
            runner: HarnessRunner::Test,
            profile: HarnessProfile::Base,
            modes: vec![HarnessMode::Compile],
            output_dir: Some(output_dir.clone()),
            runner_binary: Some(runner),
            perl_ref: Some("fake-ref".into()),
        }) else {
            bail!("missing bucket should fail smoke receipt integrity");
        };

        assert!(err.to_string().contains("receipt integrity"));
        let raw = fs::read_to_string(output_dir.join("smoke.json"))?;
        let report: SmokeReport = serde_json::from_str(&raw)?;
        assert_eq!(report.status, SmokeStatus::Fail);
        assert!(
            report
                .structural_failures
                .iter()
                .any(|failure| failure.kind == SmokeFailureKind::UnknownBucket)
        );
        Ok(())
    }

    #[test]
    fn smoke_rejects_missing_prepared_tree() -> TestResult {
        let temp = tempfile::tempdir()?;

        let Err(err) = smoke(SmokeConfig {
            perl_tree: temp.path().join("missing"),
            host_perl: PathBuf::from("perl"),
            runner: HarnessRunner::Test,
            profile: HarnessProfile::Base,
            modes: vec![HarnessMode::Parse, HarnessMode::Compile],
            output_dir: Some(temp.path().join("smoke")),
            runner_binary: None,
            perl_ref: Some("fake-ref".into()),
        }) else {
            bail!("missing prepared tree should fail");
        };

        assert!(err.to_string().contains("prepared Perl tree"));
        Ok(())
    }

    #[test]
    fn smoke_rejects_missing_runner_script() -> TestResult {
        let temp = tempfile::tempdir()?;
        let perl_tree = temp.path().join("prepared-perl");
        fs::create_dir_all(perl_tree.join("t").join("base"))?;

        let Err(err) = smoke(SmokeConfig {
            perl_tree,
            host_perl: PathBuf::from("perl"),
            runner: HarnessRunner::Test,
            profile: HarnessProfile::Base,
            modes: vec![HarnessMode::Parse, HarnessMode::Compile],
            output_dir: Some(temp.path().join("smoke")),
            runner_binary: None,
            perl_ref: Some("fake-ref".into()),
        }) else {
            bail!("missing t/TEST should fail");
        };

        assert!(err.to_string().contains("missing t/TEST"));
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn run_mode_buckets_runner_failure_records() -> TestResult {
        let temp = tempfile::tempdir()?;
        let perl_tree = write_fake_perl_tree(temp.path())?;
        let runner = write_fake_runner(temp.path(), RunnerStatus::Fail)?;
        let output = temp.path().join("parse-report.json");

        let Err(err) = run_mode(RunConfig {
            perl_tree,
            host_perl: PathBuf::from("/bin/sh"),
            runner: HarnessRunner::Test,
            mode: HarnessMode::Parse,
            profile: HarnessProfile::Base,
            tests: Vec::new(),
            output: Some(output.clone()),
            runner_binary: Some(runner),
        }) else {
            bail!("failing runner record should fail the harness run");
        };

        assert!(err.to_string().contains("failed for 1 of 1 files"));
        let raw = fs::read_to_string(output)?;
        let report: RunReport = serde_json::from_str(&raw)?;
        assert_eq!(report.summary.files_total, 1);
        assert_eq!(report.summary.files_passed, 0);
        assert_eq!(report.summary.files_failed, 1);
        assert_eq!(report.buckets.get("parse_recovery"), Some(&1));
        assert_eq!(report.failures[0].workstream, "parser_recovery");
        assert_eq!(
            report.failures[0].lsp_impact,
            vec!["diagnostics", "syntax_tree", "semantic_tokens"]
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn run_mode_rejects_nonzero_harness_status_without_file_failures() -> TestResult {
        let temp = tempfile::tempdir()?;
        let perl_tree = write_fake_perl_tree_with_run_body(
            temp.path(),
            r#"./perl base/ok.t
exit 7
"#,
        )?;
        let runner = write_fake_runner(temp.path(), RunnerStatus::Pass)?;
        let output = temp.path().join("parse-report.json");

        let Err(err) = run_mode(RunConfig {
            perl_tree,
            host_perl: PathBuf::from("/bin/sh"),
            runner: HarnessRunner::Test,
            mode: HarnessMode::Parse,
            profile: HarnessProfile::Base,
            tests: Vec::new(),
            output: Some(output.clone()),
            runner_binary: Some(runner),
        }) else {
            bail!("nonzero harness status should fail even when runner records pass");
        };

        assert!(err.to_string().contains("upstream harness exited with status"));
        let raw = fs::read_to_string(output)?;
        let report: RunReport = serde_json::from_str(&raw)?;
        assert_eq!(report.summary.files_passed, 1);
        assert_eq!(report.summary.files_failed, 0);
        assert_eq!(report.harness_status, Some(7));
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn run_mode_invokes_runner_directly_when_harness_writes_no_records() -> TestResult {
        let temp = tempfile::tempdir()?;
        let perl_tree = write_fake_perl_tree_with_run_body(
            temp.path(),
            r#"# Deliberately do not invoke ./perl; real harness integration bugs should fall
# back to direct runner invocation for harness-selected files.
exit 7
"#,
        )?;
        let runner = write_fake_runner(temp.path(), RunnerStatus::Pass)?;
        let output = temp.path().join("parse-report.json");

        run_mode(RunConfig {
            perl_tree,
            host_perl: PathBuf::from("/bin/sh"),
            runner: HarnessRunner::Test,
            mode: HarnessMode::Parse,
            profile: HarnessProfile::Base,
            tests: Vec::new(),
            output: Some(output.clone()),
            runner_binary: Some(runner),
        })?;

        let raw = fs::read_to_string(output)?;
        let report: RunReport = serde_json::from_str(&raw)?;
        assert_eq!(report.summary.files_total, 1);
        assert_eq!(report.summary.files_passed, 1);
        assert_eq!(report.summary.files_failed, 0);
        assert!(report.buckets.is_empty());
        assert!(report.failures.is_empty());
        assert_eq!(report.harness_status, Some(7));
        Ok(())
    }

    #[cfg(unix)]
    fn write_fake_perl_tree_requiring_t_perl_for_dumptests(root: &Path) -> TestResult<PathBuf> {
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
        let perl_tree = root.join("prepared-perl-base-if");
        let t_dir = perl_tree.join("t");
        fs::create_dir_all(t_dir.join("base"))?;
        fs::write(t_dir.join("base").join("if.t"), "1;\n")?;
        let script = r#"#!/bin/sh
set -eu
if [ "${1:-}" = "--dumptests" ]; then
  echo "base/if.t"
  exit 0
fi
./perl base/if.t
"#;
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
}
