use crate::tasks::metrics::ratchet::{self, SubsystemBaseline};
use crate::utils::project_root;
use chrono::Utc;
use color_eyre::eyre::{Context, Result, bail};
use perl_lsp_ux_tests::{
    ScenarioScore, UxEvidenceClass, aggregate_editor_ux_scorecard, ensure_score_evidence_consistent,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;

const PUBLICATION_GENERATION_PREFIX: &str = "sha256:";
const STATUS_GENERATION_LABEL: &str = "Publication generation:";
const STAGING_SUFFIX: &str = ".ux-gen-stage";
const LOCK_SUFFIX: &str = ".ux-gen.lock";

const DEFAULT_INPUT: &str =
    "crates/perl-lsp-ux-tests/fixtures/editor_ux_scorecard_measurements.json";
const DEFAULT_OUTPUT: &str = "target/receipts/metrics/editor_ux_scorecard.json";
const DEFAULT_STATUS_MD: &str = "docs/project/status/editor_ux.md";
const FIXTURE_MATRIX: &str = "crates/perl-lsp-ux-tests/fixtures/editor_ux_fixture_matrix.json";
#[cfg(test)]
const BASELINE_PATH: &str = ".ci/metrics/baselines/editor_ux.json";

#[derive(Debug, Clone, Copy)]
pub enum UxScorecardFormat {
    Human,
    Json,
}

#[derive(Debug, Deserialize)]
struct ScenarioMeasurement {
    scenario_id: String,
    hover_correct: Option<bool>,
    completion_top1_correct: Option<bool>,
    completion_top5_correct: Option<bool>,
    definition_exact_hit: Option<bool>,
    symbol_correct: Option<bool>,
    diagnostics_correct: Option<bool>,
    rename_success: Option<bool>,
    cross_file_success: Option<bool>,
    /// Evidence class of the measured row.
    ///
    /// Defaults to [`UxEvidenceClass::SemanticProof`] so measurement fixtures
    /// written before the field existed keep parsing as full-proof rows; a
    /// transport-characterization row carrying semantic metric values is
    /// rejected by [`ensure_score_evidence_consistent`] before aggregation.
    #[serde(default)]
    evidence_class: UxEvidenceClass,
    latency_ms_by_request: BTreeMap<String, Vec<u64>>,
}

#[derive(Debug, Serialize)]
struct PercentMetric {
    value: Option<f64>,
}

#[derive(Debug, Serialize)]
struct LatencyPercentiles {
    p50_ms: Option<f64>,
    p95_ms: Option<f64>,
}

#[derive(Debug, Serialize)]
struct UxScorecardArtifact {
    schema_version: u32,
    measured_at: String,
    subsystem: &'static str,
    scenario_count: usize,
    scenario_ids: Vec<String>,
    rows: BTreeMap<String, PercentMetric>,
    latency_by_request_class: BTreeMap<String, LatencyPercentiles>,
    provenance: serde_json::Value,
    /// Identity of this publication generation, derived from candidate inputs.
    /// Empty only in unit fixtures that never go through the publisher.
    #[serde(skip_serializing_if = "String::is_empty")]
    publication_generation: String,
}

/// Injected dest-commit seam so tests can fail between published surfaces
/// without sleeping or racing the scheduler (#13321).
trait ArtifactPublisher {
    fn publish(&self, path: &Path, contents: &[u8]) -> Result<()>;
}

struct AtomicFsPublisher;

impl ArtifactPublisher for AtomicFsPublisher {
    fn publish(&self, path: &Path, contents: &[u8]) -> Result<()> {
        write_atomic(path, contents)
    }
}

struct PendingSurface {
    path: PathBuf,
    contents: Vec<u8>,
}

pub fn run(
    format: UxScorecardFormat,
    input: Option<PathBuf>,
    output: Option<PathBuf>,
    status_md: Option<PathBuf>,
    ratchet_check: bool,
) -> Result<()> {
    let root = project_root()?;
    run_at(&root, format, input, output, status_md, ratchet_check)
}

fn run_at(
    root: &Path,
    format: UxScorecardFormat,
    input: Option<PathBuf>,
    output: Option<PathBuf>,
    status_md: Option<PathBuf>,
    ratchet_check: bool,
) -> Result<()> {
    run_at_using(root, format, input, output, status_md, ratchet_check, &AtomicFsPublisher)
}

fn run_at_using(
    root: &Path,
    format: UxScorecardFormat,
    input: Option<PathBuf>,
    output: Option<PathBuf>,
    status_md: Option<PathBuf>,
    ratchet_check: bool,
    publisher: &dyn ArtifactPublisher,
) -> Result<()> {
    let input_path = root.join(input.unwrap_or_else(|| PathBuf::from(DEFAULT_INPUT)));
    let output_path = root.join(output.unwrap_or_else(|| PathBuf::from(DEFAULT_OUTPUT)));
    let status_path = root.join(status_md.unwrap_or_else(|| PathBuf::from(DEFAULT_STATUS_MD)));

    let raw_measurements = load_measurements_raw(&input_path)?;
    let scenarios = load_measurements(&raw_measurements);
    // Evidence-class consistency gate: a transport-characterization row
    // carrying semantic metric values is a misclassification and must fail
    // scorecard generation here instead of being silently filtered downstream
    // (#13570).
    for scenario in &scenarios {
        ensure_score_evidence_consistent(scenario)
            .map_err(|message| color_eyre::eyre::eyre!("{message}"))?;
    }
    let scorecard = aggregate_editor_ux_scorecard(&scenarios);
    let latencies = compute_latency_percentiles(&raw_measurements);
    let scenario_ids: Vec<String> =
        raw_measurements.iter().map(|m| m.scenario_id.clone()).collect();

    let declared_scenario_count = load_declared_scenario_count(root);

    // Validate the required baseline before creating any output. A malformed
    // ratchet input must not leave a partially updated scorecard behind.
    let baseline = if ratchet_check { Some(load_baseline(root)?) } else { load_baseline_opt(root) };

    let mut rows = BTreeMap::new();
    rows.insert(
        "hover_correctness_pct".to_string(),
        PercentMetric { value: scorecard.hover_correctness_pct },
    );
    rows.insert(
        "completion_top1_pct".to_string(),
        PercentMetric { value: scorecard.completion_top1_pct },
    );
    rows.insert(
        "completion_top5_pct".to_string(),
        PercentMetric { value: scorecard.completion_top5_pct },
    );
    rows.insert(
        "definition_exact_hit_pct".to_string(),
        PercentMetric { value: scorecard.definition_exact_hit_pct },
    );
    rows.insert(
        "symbol_correctness_pct".to_string(),
        PercentMetric { value: scorecard.symbol_correctness_pct },
    );
    rows.insert(
        "diagnostics_correct_pct".to_string(),
        PercentMetric { value: scorecard.diagnostics_correct_pct },
    );
    rows.insert(
        "rename_success_pct".to_string(),
        PercentMetric { value: scorecard.rename_success_pct },
    );
    rows.insert(
        "cross_file_success_pct".to_string(),
        PercentMetric { value: scorecard.cross_file_success_pct },
    );

    let mut artifact = UxScorecardArtifact {
        schema_version: 1,
        measured_at: Utc::now().to_rfc3339(),
        subsystem: "editor_ux",
        scenario_count: scorecard.scenario_count,
        scenario_ids,
        rows,
        latency_by_request_class: latencies,
        provenance: json!({
            "input": path_relative_to_root(root, &input_path),
            "generator": "cargo xtask ux-scorecard --format json",
            "ratchet_policy": "regression_only",
            "declared_scenario_count": declared_scenario_count
        }),
        publication_generation: String::new(),
    };
    artifact.publication_generation = compute_publication_generation(&artifact)?;

    if ratchet_check {
        let baseline = baseline
            .as_ref()
            .ok_or_else(|| color_eyre::eyre::eyre!("editor_ux ratchet baseline was not loaded"))?;
        enforce_ratchet(baseline, &artifact)?;
        // A ratchet check is read-only. Publication rewrites the tracked
        // status page with a fresh `measured_at` timestamp, so letting the
        // check path publish would dirty the working tree on every run.
        if matches!(format, UxScorecardFormat::Json) {
            println!("{}", serde_json::to_string_pretty(&artifact)?);
        } else {
            println!("editor_ux ratchet check passed (check-only; no artifacts written)");
        }
        return Ok(());
    }

    run_at_with_publisher(
        root,
        format,
        &output_path,
        &status_path,
        &artifact,
        baseline.as_ref(),
        publisher,
    )
}

fn run_at_with_publisher(
    root: &Path,
    format: UxScorecardFormat,
    output_path: &Path,
    status_path: &Path,
    artifact: &UxScorecardArtifact,
    baseline: Option<&SubsystemBaseline>,
    publisher: &dyn ArtifactPublisher,
) -> Result<()> {
    // Prepare every publication payload, including the optional receipt, before
    // replacing any existing artifact. In particular, a malformed receipt must
    // fail while the old scorecard and status remain intact.
    let scorecard_payload = serde_json::to_string_pretty(artifact)? + "\n";
    let status_payload = render_status_markdown(artifact, baseline);
    let receipt_payload = prepare_receipt_payload(root, artifact)?;

    let mut surfaces = vec![
        PendingSurface {
            path: output_path.to_path_buf(),
            contents: scorecard_payload.into_bytes(),
        },
        PendingSurface { path: status_path.to_path_buf(), contents: status_payload.into_bytes() },
    ];
    if let Some((receipt_path, payload)) = receipt_payload {
        surfaces.push(PendingSurface { path: receipt_path, contents: payload.into_bytes() });
    }

    publish_coherent_generation(publisher, &surfaces)?;

    match format {
        UxScorecardFormat::Human => {
            println!("UX scorecard updated: {}", output_path.display());
            println!("Status page updated: {}", status_path.display());
        }
        UxScorecardFormat::Json => {
            println!("{}", serde_json::to_string_pretty(artifact)?);
        }
    }

    Ok(())
}

fn load_measurements(raw: &[ScenarioMeasurement]) -> Vec<ScenarioScore> {
    raw.iter()
        .map(|m| {
            let mean_latency_ms = m
                .latency_ms_by_request
                .iter()
                .filter_map(|(class, samples)| {
                    if samples.is_empty() {
                        return None;
                    }
                    let mean = samples.iter().sum::<u64>() as f64 / samples.len() as f64;
                    Some((class.clone(), mean))
                })
                .collect();
            ScenarioScore {
                scenario_id: m.scenario_id.clone(),
                hover_correct: m.hover_correct,
                completion_top1_correct: m.completion_top1_correct,
                completion_top5_correct: m.completion_top5_correct,
                definition_exact_hit: m.definition_exact_hit,
                symbol_correct: m.symbol_correct,
                diagnostics_correct: m.diagnostics_correct,
                rename_success: m.rename_success,
                cross_file_success: m.cross_file_success,
                evidence_class: m.evidence_class,
                mean_latency_ms,
            }
        })
        .collect()
}

fn load_measurements_raw(path: &Path) -> Result<Vec<ScenarioMeasurement>> {
    let raw = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let rows = serde_json::from_str::<Vec<ScenarioMeasurement>>(&raw)
        .with_context(|| format!("parsing {}", path.display()))?;
    if rows.is_empty() {
        bail!("measurement fixture is empty: {}", path.display());
    }
    Ok(rows)
}

fn load_declared_scenario_count(root: &Path) -> Option<usize> {
    let path = root.join(FIXTURE_MATRIX);
    let raw = fs::read_to_string(&path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
    value.get("workflows")?.as_array().map(|arr| arr.len())
}

fn load_baseline_opt(root: &Path) -> Option<SubsystemBaseline> {
    load_baseline(root).ok()
}

fn load_baseline(root: &Path) -> Result<SubsystemBaseline> {
    ratchet::load_baseline(root, "editor_ux")
}

fn write_atomic(path: &Path, content: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    let parent =
        path.parent().filter(|parent| !parent.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let mut temporary = NamedTempFile::new_in(parent)
        .with_context(|| format!("creating temporary publication for {}", path.display()))?;
    temporary
        .write_all(content)
        .with_context(|| format!("writing temporary publication for {}", path.display()))?;
    temporary
        .as_file_mut()
        .sync_all()
        .with_context(|| format!("syncing temporary publication for {}", path.display()))?;
    temporary.persist(path).map_err(|error| {
        color_eyre::eyre::eyre!("atomically replacing {}: {}", path.display(), error.error)
    })?;
    Ok(())
}

struct PublicationLock {
    /// Held open so the flock stays exclusive until this publisher returns.
    /// The pathname is left in place: unlinking it would let a third process
    /// create a new inode while a waiter still holds the old one.
    _file: fs::File,
}

fn sibling_with_suffix(path: &Path, suffix: &str) -> Result<PathBuf> {
    let name = path.file_name().ok_or_else(|| {
        color_eyre::eyre::eyre!("publication path has no file name: {}", path.display())
    })?;
    let mut name = name.to_os_string();
    name.push(suffix);
    let parent =
        path.parent().filter(|parent| !parent.as_os_str().is_empty()).unwrap_or(Path::new("."));
    Ok(parent.join(name))
}

fn compute_publication_generation(artifact: &UxScorecardArtifact) -> Result<String> {
    // measured_at is intentionally time-bound and excluded so identical
    // candidate inputs produce the same generation identity.
    let payload = json!({
        "schema_version": artifact.schema_version,
        "subsystem": artifact.subsystem,
        "scenario_count": artifact.scenario_count,
        "scenario_ids": artifact.scenario_ids,
        "rows": artifact.rows,
        "latency_by_request_class": artifact.latency_by_request_class,
        "provenance": artifact.provenance,
    });
    let bytes =
        serde_json::to_vec(&payload).context("serializing publication generation inputs")?;
    let digest = Sha256::digest(bytes);
    let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    Ok(format!("{PUBLICATION_GENERATION_PREFIX}{hex}"))
}

fn restore_previous_surface(path: &Path, previous: Option<&[u8]>) -> Result<()> {
    match previous {
        Some(bytes) => write_atomic(path, bytes),
        None if path.exists() => {
            fs::remove_file(path).with_context(|| format!("rolling back {}", path.display()))
        }
        None => Ok(()),
    }
}

fn unique_destination_paths(surfaces: &[PendingSurface]) -> Result<()> {
    let mut seen = BTreeSet::new();
    for surface in surfaces {
        if !seen.insert(&surface.path) {
            bail!("duplicate publication destination: {}", surface.path.display());
        }
    }
    Ok(())
}

fn reject_destination_sidecar_collisions(surfaces: &[PendingSurface]) -> Result<()> {
    unique_destination_paths(surfaces)?;
    let dests: BTreeSet<PathBuf> = surfaces.iter().map(|surface| surface.path.clone()).collect();
    for surface in surfaces {
        let stage = sibling_with_suffix(&surface.path, STAGING_SUFFIX)?;
        if dests.contains(&stage) {
            bail!("publication destination collides with a staging sidecar: {}", stage.display());
        }
    }
    Ok(())
}

fn current_unix_uid() -> u32 {
    #[cfg(unix)]
    {
        // SAFETY: `getuid` has no preconditions and cannot fail.
        unsafe { libc::getuid() }
    }
    #[cfg(not(unix))]
    {
        0
    }
}

fn publication_lock_dir() -> PathBuf {
    std::env::temp_dir().join(format!("perl-lsp-ux-scorecard-locks-{}", current_unix_uid()))
}

fn ensure_publication_lock_dir(lock_dir: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::io::ErrorKind;
        use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
        if let Err(error) = fs::DirBuilder::new().mode(0o700).create(lock_dir) {
            if error.kind() != ErrorKind::AlreadyExists {
                return Err(error).with_context(|| {
                    format!("creating publication lock dir {}", lock_dir.display())
                });
            }
        }
        let meta = fs::symlink_metadata(lock_dir)
            .with_context(|| format!("inspecting publication lock dir {}", lock_dir.display()))?;
        if meta.file_type().is_symlink() {
            bail!("publication lock dir is a symlink: {}", lock_dir.display());
        }
        if !meta.file_type().is_dir() {
            bail!("publication lock dir is not a directory: {}", lock_dir.display());
        }
        if meta.uid() != current_unix_uid() {
            bail!("publication lock dir is not owned by the current user: {}", lock_dir.display());
        }
        let mode = meta.permissions().mode() & 0o777;
        if mode != 0o700 {
            fs::set_permissions(lock_dir, fs::Permissions::from_mode(0o700)).with_context(
                || format!("restricting publication lock dir {}", lock_dir.display()),
            )?;
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        fs::create_dir_all(lock_dir)
            .with_context(|| format!("creating publication lock dir {}", lock_dir.display()))
    }
}

fn publication_lock_path(dest: &Path) -> PathBuf {
    let digest = Sha256::digest(dest.to_string_lossy().as_bytes());
    let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    publication_lock_dir().join(format!("{hex}{LOCK_SUFFIX}"))
}

fn acquire_publication_locks(surfaces: &[PendingSurface]) -> Result<Vec<PublicationLock>> {
    let lock_dir = publication_lock_dir();
    ensure_publication_lock_dir(&lock_dir)?;
    let mut lock_paths = BTreeSet::new();
    for surface in surfaces {
        lock_paths.insert(publication_lock_path(&surface.path));
    }
    let mut locks = Vec::new();
    for lock_path in lock_paths {
        let lock_file = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
            .with_context(|| format!("opening publication lock {}", lock_path.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            lock_file
                .set_permissions(fs::Permissions::from_mode(0o600))
                .with_context(|| format!("restricting publication lock {}", lock_path.display()))?;
        }
        lock_file
            .lock()
            .with_context(|| format!("acquiring publication lock {}", lock_path.display()))?;
        locks.push(PublicationLock { _file: lock_file });
    }
    Ok(locks)
}

fn rollback_committed_surfaces(
    committed: &[(PathBuf, Option<Vec<u8>>)],
    error: color_eyre::eyre::Report,
) -> Result<()> {
    let mut rollback_errors = Vec::new();
    for (done_dest, done_previous) in committed.iter().rev() {
        if let Err(restore_error) = restore_previous_surface(done_dest, done_previous.as_deref()) {
            rollback_errors.push(restore_error);
        }
    }
    if rollback_errors.is_empty() {
        return Err(error);
    }
    let restore_summary =
        rollback_errors.iter().map(ToString::to_string).collect::<Vec<_>>().join("; ");
    Err(error.wrap_err(format!("publication failed and rollback also failed: {restore_summary}")))
}

fn remove_staging_files(paths: &[PathBuf]) {
    for path in paths {
        let _ = fs::remove_file(path);
    }
}

fn stage_publication_surfaces(
    surfaces: &[PendingSurface],
) -> Result<Vec<(PathBuf, PathBuf, Option<Vec<u8>>, Vec<u8>)>> {
    let mut staged = Vec::new();
    let mut created = Vec::new();
    let result: Result<()> = (|| {
        for surface in surfaces {
            let stage = sibling_with_suffix(&surface.path, STAGING_SUFFIX)?;
            if stage.exists() {
                fs::remove_file(&stage)
                    .with_context(|| format!("removing stale staging {}", stage.display()))?;
            }
            write_atomic(&stage, &surface.contents)?;
            created.push(stage.clone());
            let previous =
                if surface.path.exists() { Some(fs::read(&surface.path)?) } else { None };
            let staged_bytes =
                fs::read(&stage).with_context(|| format!("reading staged {}", stage.display()))?;
            staged.push((stage, surface.path.clone(), previous, staged_bytes));
        }
        Ok(())
    })();
    if let Err(error) = result {
        remove_staging_files(&created);
        return Err(error);
    }
    Ok(staged)
}

fn publish_coherent_generation(
    publisher: &dyn ArtifactPublisher,
    surfaces: &[PendingSurface],
) -> Result<()> {
    if surfaces.is_empty() {
        bail!("publication generation has no surfaces");
    }
    reject_destination_sidecar_collisions(surfaces)?;
    let _locks = acquire_publication_locks(surfaces)?;

    let staged = stage_publication_surfaces(surfaces)?;

    let mut committed: Vec<(PathBuf, Option<Vec<u8>>)> = Vec::new();
    let mut remaining_stages: Vec<PathBuf> =
        staged.iter().map(|(stage, _, _, _)| stage.clone()).collect();
    for (stage, dest, previous, staged_bytes) in staged {
        remaining_stages.retain(|path| path != &stage);
        if let Err(error) = publisher.publish(&dest, &staged_bytes) {
            remaining_stages.push(stage);
            remove_staging_files(&remaining_stages);
            return rollback_committed_surfaces(&committed, error);
        }
        let _ = fs::remove_file(&stage);
        committed.push((dest, previous));
    }
    let Some(scorecard) = surfaces.first() else {
        bail!("publication generation has no surfaces");
    };
    let Some(status) = surfaces.get(1) else {
        bail!("publication generation is missing the status surface");
    };
    let receipt = surfaces.get(2).map(|surface| surface.path.as_path());
    if let Err(error) = validate_published_generation(&scorecard.path, &status.path, receipt) {
        return rollback_committed_surfaces(&committed, error);
    }
    Ok(())
}

fn parse_status_generation(status: &str) -> Result<String> {
    for line in status.lines() {
        let trimmed = line.trim();
        let Some(rest) = trimmed.strip_prefix(STATUS_GENERATION_LABEL) else {
            continue;
        };
        let rest = rest.trim().trim_matches('`');
        if is_publication_generation(rest) {
            return Ok(rest.to_string());
        }
        bail!("status markdown has a malformed publication generation marker");
    }
    bail!("status markdown is missing a publication generation marker")
}

fn parse_scorecard_generation(scorecard: &serde_json::Value) -> Result<String> {
    let generation =
        scorecard.get("publication_generation").and_then(|value| value.as_str()).ok_or_else(
            || color_eyre::eyre::eyre!("scorecard JSON is missing publication_generation"),
        )?;
    if is_publication_generation(generation) {
        Ok(generation.to_string())
    } else {
        bail!("scorecard JSON has a malformed publication_generation")
    }
}

fn parse_receipt_generation(receipt: &serde_json::Value) -> Result<String> {
    let generation = receipt
        .get("ux_scorecard")
        .and_then(|block| block.get("publication_generation"))
        .and_then(|value| value.as_str())
        .ok_or_else(|| {
            color_eyre::eyre::eyre!(
                "embedded receipt ux_scorecard block is missing publication_generation"
            )
        })?;
    if is_publication_generation(generation) {
        Ok(generation.to_string())
    } else {
        bail!("embedded receipt ux_scorecard block has a malformed publication_generation")
    }
}

fn is_publication_generation(value: &str) -> bool {
    value.starts_with(PUBLICATION_GENERATION_PREFIX)
        && value.len() == PUBLICATION_GENERATION_PREFIX.len() + 64
        && value[PUBLICATION_GENERATION_PREFIX.len()..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Reader-side validator: refuse a mixed-generation published set.
fn validate_published_generation(
    scorecard_path: &Path,
    status_path: &Path,
    receipt_path: Option<&Path>,
) -> Result<String> {
    let scorecard_raw = fs::read_to_string(scorecard_path)
        .with_context(|| format!("reading {}", scorecard_path.display()))?;
    let scorecard: serde_json::Value = serde_json::from_str(&scorecard_raw)
        .with_context(|| format!("parsing {}", scorecard_path.display()))?;
    let scorecard_generation = parse_scorecard_generation(&scorecard)?;

    let status_raw = fs::read_to_string(status_path)
        .with_context(|| format!("reading {}", status_path.display()))?;
    let status_generation = parse_status_generation(&status_raw)?;
    if scorecard_generation != status_generation {
        bail!(
            "mixed publication generation: scorecard has {scorecard_generation}, status has {status_generation}"
        );
    }

    if let Some(receipt_path) = receipt_path {
        let receipt_raw = fs::read_to_string(receipt_path)
            .with_context(|| format!("reading {}", receipt_path.display()))?;
        let receipt: serde_json::Value = serde_json::from_str(&receipt_raw)
            .with_context(|| format!("parsing {}", receipt_path.display()))?;
        let receipt_generation = parse_receipt_generation(&receipt)?;
        if scorecard_generation != receipt_generation {
            bail!(
                "mixed publication generation: scorecard has {scorecard_generation}, receipt has {receipt_generation}"
            );
        }
    }

    Ok(scorecard_generation)
}

fn render_status_markdown(
    artifact: &UxScorecardArtifact,
    baseline: Option<&SubsystemBaseline>,
) -> String {
    let declared = artifact
        .provenance
        .get("declared_scenario_count")
        .and_then(|v| v.as_u64())
        .map(|n| n as usize);

    let coverage_line = match declared {
        Some(total) if total > 0 => {
            let pct = (artifact.scenario_count as f64 / total as f64 * 100.0).round() as usize;
            format!(
                "Scenarios measured: `{}` of `{}` declared ({pct}% fixture coverage)  \n",
                artifact.scenario_count, total
            )
        }
        _ => format!("Scenarios: `{}`  \n", artifact.scenario_count),
    };

    let mut text = String::new();
    text.push_str("# Editor UX Scorecard\n\n");
    text.push_str(&format!("Measured: `{}`  \n", artifact.measured_at));
    if !artifact.publication_generation.is_empty() {
        text.push_str(&format!(
            "{STATUS_GENERATION_LABEL} `{}`  \n",
            artifact.publication_generation
        ));
    }
    text.push_str(&coverage_line);

    if !artifact.scenario_ids.is_empty() {
        text.push('\n');
        text.push_str("**Measured scenarios:** ");
        let ids: Vec<&str> = artifact.scenario_ids.iter().map(String::as_str).collect();
        text.push_str(&ids.join(", "));
        text.push_str("  \n");
    }

    text.push_str("\n## Correctness\n\n| Metric | Value |\n|---|---:|\n");
    for (k, v) in &artifact.rows {
        let value = v.value.map(|n| format!("{n:.2}%")).unwrap_or_else(|| "n/a".to_string());
        text.push_str(&format!("| {k} | {value} |\n"));
    }

    text.push_str("\n## Latency (ms)\n\n");
    if baseline.is_some() {
        text.push_str("| Request class | p50 | p50 baseline | p95 | p95 baseline |\n");
        text.push_str("|---|---:|---:|---:|---:|\n");
    } else {
        text.push_str("| Request class | p50 | p95 |\n|---|---:|---:|\n");
    }
    for (k, v) in &artifact.latency_by_request_class {
        let p50 = v.p50_ms.map(|n| format!("{n:.2}")).unwrap_or_else(|| "n/a".to_string());
        let p95 = v.p95_ms.map(|n| format!("{n:.2}")).unwrap_or_else(|| "n/a".to_string());
        if let Some(bl) = baseline {
            let bl50_key = format!("latency_{k}_p50_ms");
            let bl95_key = format!("latency_{k}_p95_ms");
            let bl50 = bl
                .floor_metrics
                .get(&bl50_key)
                .and_then(|n| *n)
                .map(|n| format!("{n:.2}"))
                .unwrap_or_else(|| "—".to_string());
            let bl95 = bl
                .floor_metrics
                .get(&bl95_key)
                .and_then(|n| *n)
                .map(|n| format!("{n:.2}"))
                .unwrap_or_else(|| "—".to_string());
            text.push_str(&format!("| {k} | {p50} | {bl50} | {p95} | {bl95} |\n"));
        } else {
            text.push_str(&format!("| {k} | {p50} | {p95} |\n"));
        }
    }

    text.push_str("\n## Ratchet policy\n\nRegression-only ratchet: floor metrics may improve or stay flat; any statistically meaningful regression fails `cargo xtask ux-scorecard --ratchet-check`.\n");
    text
}

fn compute_latency_percentiles(
    scenarios: &[ScenarioMeasurement],
) -> BTreeMap<String, LatencyPercentiles> {
    let mut by_request: BTreeMap<String, Vec<u64>> = BTreeMap::new();
    for scenario in scenarios {
        for (request, samples) in &scenario.latency_ms_by_request {
            let entry = by_request.entry(request.clone()).or_default();
            entry.extend(samples.iter().copied());
        }
    }

    by_request
        .into_iter()
        .map(|(request, mut samples)| {
            samples.sort_unstable();
            (
                request,
                LatencyPercentiles {
                    p50_ms: percentile(&samples, 0.50),
                    p95_ms: percentile(&samples, 0.95),
                },
            )
        })
        .collect()
}

fn percentile(samples: &[u64], pct: f64) -> Option<f64> {
    if samples.is_empty() {
        return None;
    }
    let rank = ((samples.len() - 1) as f64 * pct).round() as usize;
    samples.get(rank).map(|value| *value as f64)
}

fn prepare_receipt_payload(
    root: &Path,
    artifact: &UxScorecardArtifact,
) -> Result<Option<(PathBuf, String)>> {
    let receipt_path = root.join("target/receipts/receipt.json");
    if !receipt_path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&receipt_path)
        .with_context(|| format!("reading {}", receipt_path.display()))?;
    let mut json_value: serde_json::Value = serde_json::from_str(&raw)
        .with_context(|| format!("parsing {}", receipt_path.display()))?;
    let validator = receipt_validator(root)?;
    validate_receipt_with(&validator, root, &json_value)?;
    if let Some(object) = json_value.as_object_mut() {
        let row = |k: &str| artifact.rows.get(k).and_then(|m| m.value);
        let mut block = json!({
            "hover_correctness_pct": row("hover_correctness_pct"),
            "completion_top1_pct": row("completion_top1_pct"),
            "completion_top5_pct": row("completion_top5_pct"),
            "definition_exact_hit_pct": row("definition_exact_hit_pct"),
            "symbol_correctness_pct": row("symbol_correctness_pct"),
            "diagnostics_correct_pct": row("diagnostics_correct_pct"),
            "rename_success_pct": row("rename_success_pct"),
            "cross_file_success_pct": row("cross_file_success_pct"),
        });
        if !artifact.publication_generation.is_empty()
            && let Some(block) = block.as_object_mut()
        {
            block.insert(
                "publication_generation".to_string(),
                json!(artifact.publication_generation),
            );
        }
        object.insert("ux_scorecard".to_string(), block);
    }
    validate_receipt_with(&validator, root, &json_value)?;
    Ok(Some((receipt_path, format!("{}\n", serde_json::to_string_pretty(&json_value)?))))
}

fn receipt_validator(root: &Path) -> Result<jsonschema::Validator> {
    let schema_path = root.join(".ci/receipt.schema.json");
    let schema_raw = fs::read_to_string(&schema_path)
        .with_context(|| format!("reading {}", schema_path.display()))?;
    let schema: serde_json::Value = serde_json::from_str(&schema_raw)
        .with_context(|| format!("parsing {}", schema_path.display()))?;
    jsonschema::validator_for(&schema)
        .with_context(|| format!("compiling {}", schema_path.display()))
}

fn validate_receipt_with(
    validator: &jsonschema::Validator,
    root: &Path,
    receipt: &serde_json::Value,
) -> Result<()> {
    let schema_path = root.join(".ci/receipt.schema.json");
    let violations: Vec<String> =
        validator.iter_errors(receipt).map(|error| error.to_string()).collect();
    if violations.is_empty() {
        Ok(())
    } else {
        bail!(
            "receipt schema validation failed for {}: {}",
            schema_path.display(),
            violations.join("; ")
        )
    }
}

/// Thin wrapper retained for the focused schema tests; production callers
/// compile the validator once via [`receipt_validator`] and reuse it.
#[cfg(test)]
fn validate_receipt_schema(root: &Path, receipt: &serde_json::Value) -> Result<()> {
    let validator = receipt_validator(root)?;
    validate_receipt_with(&validator, root, receipt)
}

#[derive(Debug, PartialEq)]
struct MissingRequiredMetric {
    metric: String,
    baseline_value: f64,
}

#[derive(Debug)]
struct EditorUxRatchetViolations {
    missing: Vec<MissingRequiredMetric>,
    regressions: Vec<ratchet::RatchetViolation>,
}

impl EditorUxRatchetViolations {
    fn is_empty(&self) -> bool {
        self.missing.is_empty() && self.regressions.is_empty()
    }

    fn len(&self) -> usize {
        self.missing.len() + self.regressions.len()
    }

    fn report_lines(&self) -> Vec<String> {
        let mut lines = Vec::with_capacity(self.len());
        lines.extend(self.missing.iter().map(|missing| {
            format!(
                "VIOLATION [editor_ux] {} baseline={:.3} current=missing",
                missing.metric, missing.baseline_value
            )
        }));
        lines.extend(self.regressions.iter().map(|regression| {
            format!(
                "VIOLATION [editor_ux] {} baseline={:.3} current={:.3} regression={:.2}%",
                regression.metric,
                regression.baseline_value,
                regression.current_value,
                regression.regression_pct * 100.0
            )
        }));
        lines
    }
}

fn evaluate_ratchet(
    baseline: &SubsystemBaseline,
    current: &BTreeMap<String, Option<f64>>,
) -> EditorUxRatchetViolations {
    let finite_current: BTreeMap<String, Option<f64>> = current
        .iter()
        .map(|(metric, value)| {
            let finite_value = (*value).filter(|value| value.is_finite());
            (metric.clone(), finite_value)
        })
        .collect();

    let missing = baseline
        .floor_metrics
        .iter()
        .filter_map(|(metric, baseline_value)| {
            let baseline_value = baseline_value.as_ref().copied()?;
            if finite_current.get(metric).and_then(|value| *value).is_some() {
                None
            } else {
                Some(MissingRequiredMetric { metric: metric.clone(), baseline_value })
            }
        })
        .collect();

    EditorUxRatchetViolations {
        missing,
        regressions: ratchet::check_floor_metrics(baseline, &finite_current),
    }
}

fn enforce_ratchet(baseline: &SubsystemBaseline, artifact: &UxScorecardArtifact) -> Result<()> {
    let mut current_floor = BTreeMap::new();
    for (k, v) in &artifact.rows {
        current_floor.insert(k.clone(), v.value);
    }
    for (request, latency) in &artifact.latency_by_request_class {
        current_floor.insert(format!("latency_{}_p50_ms", request), latency.p50_ms);
        current_floor.insert(format!("latency_{}_p95_ms", request), latency.p95_ms);
    }

    let violations = evaluate_ratchet(baseline, &current_floor);
    if violations.is_empty() {
        return Ok(());
    }

    for line in violations.report_lines() {
        eprintln!("{line}");
    }

    bail!("editor_ux ratchet check failed with {} violation(s)", violations.len())
}

fn path_relative_to_root(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn measurement(
        id: &str,
        hover: Option<bool>,
        top1: Option<bool>,
        top5: Option<bool>,
        def: Option<bool>,
        sym: Option<bool>,
        cross: Option<bool>,
        latency: &[(&str, Vec<u64>)],
    ) -> ScenarioMeasurement {
        ScenarioMeasurement {
            scenario_id: id.to_string(),
            hover_correct: hover,
            completion_top1_correct: top1,
            completion_top5_correct: top5,
            definition_exact_hit: def,
            symbol_correct: sym,
            diagnostics_correct: None,
            rename_success: None,
            cross_file_success: cross,
            evidence_class: UxEvidenceClass::SemanticProof,
            latency_ms_by_request: latency
                .iter()
                .map(|(k, v)| ((*k).to_string(), v.clone()))
                .collect(),
        }
    }

    #[test]
    fn computes_percentiles() {
        let rows = vec![measurement(
            "s1",
            None,
            None,
            None,
            None,
            None,
            None,
            &[("hover", vec![10, 20, 30, 40])],
        )];

        let latency = compute_latency_percentiles(&rows);
        let hover = latency.get("hover").expect("hover present");
        assert_eq!(hover.p50_ms, Some(30.0));
        assert_eq!(hover.p95_ms, Some(40.0));
    }

    #[test]
    fn single_sample_latency_p50_equals_p95() {
        let rows = vec![measurement(
            "single",
            None,
            None,
            None,
            None,
            None,
            None,
            &[("definition", vec![42])],
        )];
        let latency = compute_latency_percentiles(&rows);
        let def = latency.get("definition").expect("definition present");
        assert_eq!(def.p50_ms, Some(42.0));
        assert_eq!(def.p95_ms, Some(42.0));
    }

    #[test]
    fn two_sample_latency_percentiles() {
        // [10, 90]: p50 rank=(2-1)*0.5=0.5→1, samples[1]=90; p95 rank=(2-1)*0.95=0.95→1, samples[1]=90
        let rows = vec![measurement(
            "two",
            None,
            None,
            None,
            None,
            None,
            None,
            &[("hover", vec![10, 90])],
        )];
        let latency = compute_latency_percentiles(&rows);
        let h = latency.get("hover").expect("hover present");
        assert_eq!(h.p50_ms, Some(90.0));
        assert_eq!(h.p95_ms, Some(90.0));
    }

    #[test]
    fn compute_latency_percentiles_empty_input_returns_empty_map() {
        let latency = compute_latency_percentiles(&[]);
        assert!(latency.is_empty());
    }

    #[test]
    fn compute_latency_percentiles_merges_multiple_scenarios() {
        // Two scenarios each contributing to "hover".
        // Combined: [10, 30] sorted → p50: samples[1]=30, p95: samples[1]=30.
        let rows = vec![
            measurement("a", None, None, None, None, None, None, &[("hover", vec![10])]),
            measurement("b", None, None, None, None, None, None, &[("hover", vec![30])]),
        ];
        let latency = compute_latency_percentiles(&rows);
        let h = latency.get("hover").expect("hover present");
        assert_eq!(h.p50_ms, Some(30.0));
        assert_eq!(h.p95_ms, Some(30.0));
    }

    /// load_measurements propagates mean latency from raw samples into ScenarioScore.
    #[test]
    fn load_measurements_populates_mean_latency() {
        let raw = vec![ScenarioMeasurement {
            scenario_id: "latency_test".to_string(),
            hover_correct: None,
            completion_top1_correct: None,
            completion_top5_correct: None,
            definition_exact_hit: None,
            symbol_correct: None,
            diagnostics_correct: None,
            rename_success: None,
            cross_file_success: None,
            evidence_class: UxEvidenceClass::SemanticProof,
            latency_ms_by_request: BTreeMap::from([
                ("hover".to_string(), vec![10, 20, 30]),
                ("completion".to_string(), vec![5, 15]),
            ]),
        }];
        let scenarios = load_measurements(&raw);
        assert_eq!(scenarios.len(), 1);
        let s = &scenarios[0];
        assert_eq!(s.mean_latency_ms.get("hover"), Some(&20.0));
        assert_eq!(s.mean_latency_ms.get("completion"), Some(&10.0));
    }

    /// load_measurements populates all correctness fields including new ones.
    #[test]
    fn load_measurements_maps_all_correctness_fields() {
        let raw = vec![ScenarioMeasurement {
            scenario_id: "full_fields".to_string(),
            hover_correct: Some(true),
            completion_top1_correct: Some(false),
            completion_top5_correct: Some(true),
            definition_exact_hit: Some(true),
            symbol_correct: Some(true),
            diagnostics_correct: Some(false),
            rename_success: Some(true),
            cross_file_success: Some(true),
            evidence_class: UxEvidenceClass::SemanticProof,
            latency_ms_by_request: BTreeMap::new(),
        }];
        let scenarios = load_measurements(&raw);
        let s = &scenarios[0];
        assert_eq!(s.hover_correct, Some(true));
        assert_eq!(s.completion_top1_correct, Some(false));
        assert_eq!(s.symbol_correct, Some(true));
        assert_eq!(s.diagnostics_correct, Some(false));
        assert_eq!(s.rename_success, Some(true));
        assert_eq!(s.cross_file_success, Some(true));
    }

    #[test]
    fn load_measurements_maps_symbol_correct_into_scenario_score() {
        let raw = vec![
            measurement("sym-true", None, None, None, None, Some(true), None, &[]),
            measurement("sym-false", None, None, None, None, Some(false), None, &[]),
            measurement("sym-none", None, None, None, None, None, None, &[]),
        ];

        let scenarios = load_measurements(&raw);
        assert_eq!(scenarios[0].symbol_correct, Some(true));
        assert_eq!(scenarios[1].symbol_correct, Some(false));
        assert_eq!(scenarios[2].symbol_correct, None);

        let scorecard = aggregate_editor_ux_scorecard(&scenarios);
        // 1 true out of 2 measured = 50%
        assert_eq!(scorecard.symbol_correctness_pct, Some(50.0));
    }

    fn baseline_with_floor_metrics(
        floor_metrics: BTreeMap<String, Option<f64>>,
    ) -> SubsystemBaseline {
        SubsystemBaseline {
            floor_metrics,
            improvement_metrics: BTreeMap::new(),
            tolerance_pct: 0.1,
            lower_is_better: vec![],
            schema_version: 1,
            measured_at: String::new(),
            subsystem: "editor_ux".to_string(),
            commit: String::new(),
        }
    }

    fn check<T: std::fmt::Debug + PartialEq>(actual: &T, expected: &T, label: &str) -> Result<()> {
        if actual == expected {
            Ok(())
        } else {
            Err(color_eyre::eyre::eyre!("{label}: expected {expected:?}, got {actual:?}"))
        }
    }

    fn check_true(condition: bool, label: &str) -> Result<()> {
        if condition { Ok(()) } else { Err(color_eyre::eyre::eyre!("{label}")) }
    }

    #[test]
    fn ratchet_requires_non_null_current_values_for_instrumented_baselines() -> Result<()> {
        let baseline = baseline_with_floor_metrics(BTreeMap::from([
            ("absent_metric".to_string(), Some(100.0)),
            ("future_metric".to_string(), None),
            ("null_metric".to_string(), Some(50.0)),
            ("zero_metric".to_string(), Some(0.0)),
        ]));
        let current = BTreeMap::from([
            ("null_metric".to_string(), None),
            ("zero_metric".to_string(), Some(0.0)),
        ]);

        let violations = evaluate_ratchet(&baseline, &current);

        check(
            &violations.missing,
            &vec![
                MissingRequiredMetric {
                    metric: "absent_metric".to_string(),
                    baseline_value: 100.0,
                },
                MissingRequiredMetric { metric: "null_metric".to_string(), baseline_value: 50.0 },
            ],
            "missing required metrics",
        )?;
        check_true(violations.regressions.is_empty(), "unexpected numeric regressions")
    }

    #[test]
    fn ratchet_rejects_non_finite_current_values() -> Result<()> {
        let baseline = baseline_with_floor_metrics(BTreeMap::from([
            ("nan_metric".to_string(), Some(10.0)),
            ("negative_infinity_metric".to_string(), Some(20.0)),
            ("positive_infinity_metric".to_string(), Some(30.0)),
        ]));
        let current = BTreeMap::from([
            ("nan_metric".to_string(), Some(f64::NAN)),
            ("negative_infinity_metric".to_string(), Some(f64::NEG_INFINITY)),
            ("positive_infinity_metric".to_string(), Some(f64::INFINITY)),
        ]);

        let violations = evaluate_ratchet(&baseline, &current);
        let missing: Vec<&str> =
            violations.missing.iter().map(|item| item.metric.as_str()).collect();

        check(
            &missing,
            &vec!["nan_metric", "negative_infinity_metric", "positive_infinity_metric"],
            "non-finite metrics treated as missing",
        )?;
        check_true(violations.regressions.is_empty(), "unexpected numeric regressions")
    }

    #[test]
    fn ratchet_collects_missing_and_numeric_regressions_together() -> Result<()> {
        let baseline = baseline_with_floor_metrics(BTreeMap::from([
            ("hover_correctness_pct".to_string(), Some(100.0)),
            ("missing_metric".to_string(), Some(75.0)),
        ]));
        let current = BTreeMap::from([("hover_correctness_pct".to_string(), Some(80.0))]);

        let violations = evaluate_ratchet(&baseline, &current);

        check(&violations.len(), &2, "combined violation count")?;
        let missing = violations
            .missing
            .first()
            .ok_or_else(|| color_eyre::eyre::eyre!("missing violation absent"))?;
        check(&missing.metric, &"missing_metric".to_string(), "missing metric name")?;
        check(&violations.regressions.len(), &1, "numeric regression count")?;
        let regression = violations
            .regressions
            .first()
            .ok_or_else(|| color_eyre::eyre::eyre!("numeric regression absent"))?;
        check(
            &regression.metric,
            &"hover_correctness_pct".to_string(),
            "numeric regression metric name",
        )
    }

    #[test]
    fn enforce_ratchet_uses_fail_closed_editor_ux_boundary() -> Result<()> {
        let root = tempfile::tempdir()?;
        let baseline = baseline_with_floor_metrics(BTreeMap::from([
            ("future_metric".to_string(), None),
            ("hover_correctness_pct".to_string(), Some(100.0)),
            ("missing_metric".to_string(), Some(75.0)),
            ("nan_metric".to_string(), Some(50.0)),
            ("null_metric".to_string(), Some(25.0)),
            ("zero_metric".to_string(), Some(0.0)),
        ]));
        let baseline_path = root.path().join(BASELINE_PATH);
        let baseline_parent = baseline_path
            .parent()
            .ok_or_else(|| color_eyre::eyre::eyre!("baseline path has no parent"))?;
        fs::create_dir_all(baseline_parent)?;
        fs::write(&baseline_path, serde_json::to_string_pretty(&baseline)?)?;

        let artifact = UxScorecardArtifact {
            schema_version: 1,
            measured_at: "2026-01-01T00:00:00Z".to_string(),
            subsystem: "editor_ux",
            scenario_count: 1,
            scenario_ids: vec!["ratchet_boundary".to_string()],
            rows: BTreeMap::from([
                ("hover_correctness_pct".to_string(), PercentMetric { value: Some(80.0) }),
                ("nan_metric".to_string(), PercentMetric { value: Some(f64::NAN) }),
                ("null_metric".to_string(), PercentMetric { value: None }),
                ("zero_metric".to_string(), PercentMetric { value: Some(0.0) }),
            ]),
            latency_by_request_class: BTreeMap::new(),
            provenance: json!({}),
            publication_generation: String::new(),
        };

        let loaded_baseline = load_baseline(root.path())?;
        let error = match enforce_ratchet(&loaded_baseline, &artifact) {
            Ok(()) => {
                return Err(color_eyre::eyre::eyre!(
                    "missing, null, non-finite, and regressed metrics must fail"
                ));
            }
            Err(error) => error,
        };
        check(
            &error.to_string(),
            &"editor_ux ratchet check failed with 4 violation(s)".to_string(),
            "ratchet error",
        )?;

        let current = BTreeMap::from([
            ("hover_correctness_pct".to_string(), Some(80.0)),
            ("nan_metric".to_string(), Some(f64::NAN)),
            ("null_metric".to_string(), None),
            ("zero_metric".to_string(), Some(0.0)),
        ]);
        let violations = evaluate_ratchet(&loaded_baseline, &current);
        check(
            &violations.report_lines(),
            &vec![
                "VIOLATION [editor_ux] missing_metric baseline=75.000 current=missing".to_string(),
                "VIOLATION [editor_ux] nan_metric baseline=50.000 current=missing".to_string(),
                "VIOLATION [editor_ux] null_metric baseline=25.000 current=missing".to_string(),
                "VIOLATION [editor_ux] hover_correctness_pct baseline=100.000 current=80.000 regression=20.00%".to_string(),
            ],
            "violation report lines",
        )
    }

    fn minimal_measurement_json() -> &'static str {
        r#"[{"scenario_id":"malformed-input-regression","hover_correct":true,"completion_top1_correct":null,"completion_top5_correct":null,"definition_exact_hit":null,"symbol_correct":null,"diagnostics_correct":null,"rename_success":null,"cross_file_success":null,"latency_ms_by_request":{}}]"#
    }

    /// A transport-characterization measurement row carrying a semantic metric
    /// is a misclassification: production scorecard generation must reject it
    /// at ingestion instead of silently filtering it downstream (#13570).
    #[test]
    fn transport_row_with_semantic_metric_fails_before_writing_artifacts() -> Result<()> {
        let root = tempfile::tempdir()?;
        fs::write(
            root.path().join("measurements.json"),
            r#"[{"scenario_id":"scenario-24-post-edit-navigation","hover_correct":null,"completion_top1_correct":null,"completion_top5_correct":null,"definition_exact_hit":true,"symbol_correct":null,"diagnostics_correct":null,"rename_success":null,"cross_file_success":null,"evidence_class":"transport_characterization","latency_ms_by_request":{}}]"#,
        )?;
        let output_path = root.path().join("scorecard.json");
        let status_path = root.path().join("status.md");
        let result = run_at(
            root.path(),
            UxScorecardFormat::Human,
            Some(PathBuf::from("measurements.json")),
            Some(PathBuf::from("scorecard.json")),
            Some(PathBuf::from("status.md")),
            false,
        );

        let error = match result {
            Ok(()) => {
                return Err(color_eyre::eyre::eyre!(
                    "transport-characterization row carrying a semantic metric must fail"
                ));
            }
            Err(error) => error,
        };
        check_true(
            error.to_string().contains("transport characterization")
                && error.to_string().contains("definition_exact_hit"),
            "evidence-class rejection must name the row class and metric",
        )?;
        check_true(!output_path.exists(), "rejected row created scorecard artifact")?;
        check_true(!status_path.exists(), "rejected row created status artifact")
    }

    #[test]
    fn malformed_baseline_returns_before_writing_artifacts() -> Result<()> {
        let root = tempfile::tempdir()?;
        let baseline_path = root.path().join(BASELINE_PATH);
        let baseline_parent = baseline_path
            .parent()
            .ok_or_else(|| color_eyre::eyre::eyre!("baseline path has no parent"))?;
        fs::create_dir_all(baseline_parent)?;
        fs::write(&baseline_path, "{ malformed baseline")?;

        fs::write(root.path().join("measurements.json"), minimal_measurement_json())?;
        let output_path = root.path().join("scorecard.json");
        let status_path = root.path().join("status.md");
        let result = run_at(
            root.path(),
            UxScorecardFormat::Human,
            Some(PathBuf::from("measurements.json")),
            Some(PathBuf::from("scorecard.json")),
            Some(PathBuf::from("status.md")),
            true,
        );

        let error = match result {
            Ok(()) => return Err(color_eyre::eyre::eyre!("malformed baseline must fail")),
            Err(error) => error,
        };
        check_true(
            error.to_string().contains("parse baseline"),
            "malformed baseline error missing context",
        )?;
        check_true(!output_path.exists(), "malformed baseline created scorecard artifact")?;
        check_true(!status_path.exists(), "malformed baseline created status artifact")
    }

    #[test]
    fn mismatched_baseline_schema_returns_before_writing_artifacts() -> Result<()> {
        let root = tempfile::tempdir()?;
        let baseline_path = root.path().join(BASELINE_PATH);
        let baseline_parent = baseline_path
            .parent()
            .ok_or_else(|| color_eyre::eyre::eyre!("baseline path has no parent"))?;
        fs::create_dir_all(baseline_parent)?;
        let mut baseline = baseline_with_floor_metrics(BTreeMap::new());
        baseline.schema_version = 999;
        fs::write(&baseline_path, serde_json::to_string_pretty(&baseline)?)?;

        fs::write(root.path().join("measurements.json"), minimal_measurement_json())?;
        let output_path = root.path().join("scorecard.json");
        let status_path = root.path().join("status.md");
        let result = run_at(
            root.path(),
            UxScorecardFormat::Human,
            Some(PathBuf::from("measurements.json")),
            Some(PathBuf::from("scorecard.json")),
            Some(PathBuf::from("status.md")),
            true,
        );

        let error = match result {
            Ok(()) => return Err(color_eyre::eyre::eyre!("schema mismatch must fail")),
            Err(error) => error,
        };
        check_true(
            error.to_string().contains("schema version mismatch"),
            "schema mismatch error missing context",
        )?;
        check_true(!output_path.exists(), "schema mismatch created scorecard artifact")?;
        check_true(!status_path.exists(), "schema mismatch created status artifact")
    }

    #[test]
    fn failed_ratchet_preserves_existing_artifacts_for_missing_current_metric() -> Result<()> {
        let root = tempfile::tempdir()?;
        let baseline_path = root.path().join(BASELINE_PATH);
        let baseline_parent = baseline_path
            .parent()
            .ok_or_else(|| color_eyre::eyre::eyre!("baseline path has no parent"))?;
        fs::create_dir_all(baseline_parent)?;
        let baseline = baseline_with_floor_metrics(BTreeMap::from([(
            "completion_top1_pct".to_string(),
            Some(75.0),
        )]));
        fs::write(&baseline_path, serde_json::to_string_pretty(&baseline)?)?;

        let input_path = root.path().join("measurements.json");
        fs::write(&input_path, minimal_measurement_json())?;
        let output_path = root.path().join("scorecard.json");
        let status_path = root.path().join("status.md");
        let receipt_path = root.path().join("target/receipts/receipt.json");
        let receipt_parent = receipt_path
            .parent()
            .ok_or_else(|| color_eyre::eyre::eyre!("receipt path has no parent"))?;
        fs::create_dir_all(receipt_parent)?;
        let previous_output = "previous scorecard\n";
        let previous_status = "previous status\n";
        let previous_receipt = r#"{"previous":true}
"#;
        fs::write(&output_path, previous_output)?;
        fs::write(&status_path, previous_status)?;
        fs::write(&receipt_path, previous_receipt)?;

        let error = match run_at(
            root.path(),
            UxScorecardFormat::Human,
            Some(PathBuf::from("measurements.json")),
            Some(PathBuf::from("scorecard.json")),
            Some(PathBuf::from("status.md")),
            true,
        ) {
            Ok(()) => {
                return Err(color_eyre::eyre::eyre!(
                    "missing current floor metric must fail the ratchet"
                ));
            }
            Err(error) => error,
        };
        check_true(
            error.to_string().contains("ratchet check failed"),
            "missing metric error did not identify ratchet failure",
        )?;
        check(
            &fs::read_to_string(&output_path)?,
            &previous_output.to_string(),
            "scorecard artifact changed after failed ratchet",
        )?;
        check(
            &fs::read_to_string(&status_path)?,
            &previous_status.to_string(),
            "status artifact changed after failed ratchet",
        )?;
        check(
            &fs::read_to_string(&receipt_path)?,
            &previous_receipt.to_string(),
            "receipt artifact changed after failed ratchet",
        )
    }

    #[test]
    fn malformed_existing_receipt_preserves_all_artifacts_before_publication() -> Result<()> {
        let root = tempfile::tempdir()?;
        let baseline_path = root.path().join(BASELINE_PATH);
        let baseline_parent = baseline_path
            .parent()
            .ok_or_else(|| color_eyre::eyre::eyre!("baseline path has no parent"))?;
        fs::create_dir_all(baseline_parent)?;
        let baseline = baseline_with_floor_metrics(BTreeMap::new());
        fs::write(&baseline_path, serde_json::to_string_pretty(&baseline)?)?;

        fs::write(root.path().join("measurements.json"), minimal_measurement_json())?;
        let output_path = root.path().join("scorecard.json");
        let status_path = root.path().join("status.md");
        let receipt_path = root.path().join("target/receipts/receipt.json");
        fs::create_dir_all(
            receipt_path
                .parent()
                .ok_or_else(|| color_eyre::eyre::eyre!("receipt path has no parent"))?,
        )?;

        let previous_output = "previous scorecard\n";
        let previous_status = "previous status\n";
        let malformed_receipt = "{ malformed receipt\n";
        fs::write(&output_path, previous_output)?;
        fs::write(&status_path, previous_status)?;
        fs::write(&receipt_path, malformed_receipt)?;

        let error = match run_at(
            root.path(),
            UxScorecardFormat::Human,
            Some(PathBuf::from("measurements.json")),
            Some(PathBuf::from("scorecard.json")),
            Some(PathBuf::from("status.md")),
            false,
        ) {
            Ok(()) => {
                return Err(color_eyre::eyre::eyre!(
                    "malformed existing receipt must fail before publication"
                ));
            }
            Err(error) => error,
        };
        check_true(
            error.to_string().contains("parsing"),
            "malformed receipt error missing parsing context",
        )?;
        check(
            &fs::read_to_string(&output_path)?,
            &previous_output.to_string(),
            "scorecard artifact changed after malformed receipt",
        )?;
        check(
            &fs::read_to_string(&status_path)?,
            &previous_status.to_string(),
            "status artifact changed after malformed receipt",
        )?;
        check(
            &fs::read_to_string(&receipt_path)?,
            &malformed_receipt.to_string(),
            "receipt artifact changed after malformed receipt",
        )
    }

    #[test]
    fn ratchet_check_is_read_only_and_never_publishes() -> Result<()> {
        let root = tempfile::tempdir()?;
        let baseline_path = root.path().join(BASELINE_PATH);
        let baseline_parent = baseline_path
            .parent()
            .ok_or_else(|| color_eyre::eyre::eyre!("baseline path has no parent"))?;
        fs::create_dir_all(baseline_parent)?;
        let baseline = baseline_with_floor_metrics(BTreeMap::new());
        fs::write(&baseline_path, serde_json::to_string_pretty(&baseline)?)?;

        fs::write(root.path().join("measurements.json"), minimal_measurement_json())?;
        let output_path = root.path().join("scorecard.json");
        let status_path = root.path().join("status.md");
        let receipt_path = root.path().join("target/receipts/receipt.json");
        fs::create_dir_all(
            receipt_path
                .parent()
                .ok_or_else(|| color_eyre::eyre::eyre!("receipt path has no parent"))?,
        )?;

        let previous_output = "previous scorecard\n";
        let previous_status = "previous status\n";
        // Even a malformed receipt must not fail (or be touched) during a
        // check-only run: the ratchet check never reads or writes artifacts.
        let previous_receipt = "{ malformed receipt\n";
        fs::write(&output_path, previous_output)?;
        fs::write(&status_path, previous_status)?;
        fs::write(&receipt_path, previous_receipt)?;

        run_at(
            root.path(),
            UxScorecardFormat::Human,
            Some(PathBuf::from("measurements.json")),
            Some(PathBuf::from("scorecard.json")),
            Some(PathBuf::from("status.md")),
            true,
        )?;
        check(
            &fs::read_to_string(&output_path)?,
            &previous_output.to_string(),
            "ratchet check must not rewrite the scorecard artifact",
        )?;
        check(
            &fs::read_to_string(&status_path)?,
            &previous_status.to_string(),
            "ratchet check must not rewrite the tracked status page",
        )?;
        check(
            &fs::read_to_string(&receipt_path)?,
            &previous_receipt.to_string(),
            "ratchet check must not rewrite the receipt",
        )
    }

    struct FailAfterPublisher {
        succeed_before_fail: usize,
        published: std::cell::Cell<usize>,
    }

    impl ArtifactPublisher for FailAfterPublisher {
        fn publish(&self, path: &Path, contents: &[u8]) -> Result<()> {
            let n = self.published.get();
            if n >= self.succeed_before_fail {
                bail!("injected publication failure after {n} successful dest write(s)");
            }
            write_atomic(path, contents)?;
            self.published.set(n + 1);
            Ok(())
        }
    }

    fn install_receipt_schema(root: &Path) -> Result<()> {
        let schema_path = root.join(".ci/receipt.schema.json");
        let schema_parent = schema_path
            .parent()
            .ok_or_else(|| color_eyre::eyre::eyre!("receipt schema path has no parent"))?;
        fs::create_dir_all(schema_parent)?;
        fs::write(&schema_path, include_str!("../../../.ci/receipt.schema.json"))?;
        Ok(())
    }

    fn install_valid_receipt(root: &Path) -> Result<PathBuf> {
        install_receipt_schema(root)?;
        let receipt_path = root.join("target/receipts/receipt.json");
        fs::create_dir_all(
            receipt_path
                .parent()
                .ok_or_else(|| color_eyre::eyre::eyre!("receipt path has no parent"))?,
        )?;
        fs::write(&receipt_path, include_str!("../../../.ci/examples/receipt-pr-fast.json"))?;
        Ok(receipt_path)
    }

    fn measurement_json(hover_correct: bool) -> String {
        format!(
            r#"[{{"scenario_id":"malformed-input-regression","hover_correct":{hover_correct},"completion_top1_correct":null,"completion_top5_correct":null,"definition_exact_hit":null,"symbol_correct":null,"diagnostics_correct":null,"rename_success":null,"cross_file_success":null,"latency_ms_by_request":{{}}}}]"#
        )
    }

    fn fake_generation(nibble: char) -> String {
        format!("{PUBLICATION_GENERATION_PREFIX}{}", nibble.to_string().repeat(64))
    }

    fn publish_human(root: &Path, publisher: &dyn ArtifactPublisher) -> Result<()> {
        run_at_using(
            root,
            UxScorecardFormat::Human,
            Some(PathBuf::from("measurements.json")),
            Some(PathBuf::from("scorecard.json")),
            Some(PathBuf::from("status.md")),
            false,
            publisher,
        )
    }

    #[test]
    fn published_surfaces_share_one_generation_without_receipt() -> Result<()> {
        let root = tempfile::tempdir()?;
        fs::write(root.path().join("measurements.json"), measurement_json(true))?;
        let output_path = root.path().join("scorecard.json");
        let status_path = root.path().join("status.md");
        let receipt_path = root.path().join("target/receipts/receipt.json");

        publish_human(root.path(), &AtomicFsPublisher)?;

        let generation = validate_published_generation(&output_path, &status_path, None)?;
        check_true(!generation.is_empty(), "published generation was empty")?;
        check_true(!receipt_path.exists(), "no-receipt run created a receipt")?;
        let lock_path = publication_lock_path(&output_path);
        check_true(
            lock_path.exists(),
            "publication lock inode must remain so waiters cannot be bypassed",
        )?;
        check(
            &lock_path.parent().map(Path::to_path_buf),
            &Some(publication_lock_dir()),
            "publication lock must live in the uid-private temp dir",
        )?;
        check_true(
            publication_lock_dir().file_name().and_then(|name| name.to_str())
                != Some("perl-lsp-ux-scorecard-locks"),
            "publication lock dir must not use the shared unkeyed temp name",
        )?;
        check_true(
            !sibling_with_suffix(&status_path, LOCK_SUFFIX)?.exists(),
            "publication lock must not land next to the tracked status page",
        )?;
        let lock_file = fs::OpenOptions::new().write(true).open(&lock_path).with_context(|| {
            format!("opening released publication lock {}", lock_path.display())
        })?;
        check_true(
            lock_file.try_lock().is_ok(),
            "previous publisher still holds the publication flock",
        )?;
        let scorecard: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&output_path)?)?;
        check(&parse_scorecard_generation(&scorecard)?, &generation, "scorecard generation")?;
        check(
            &parse_status_generation(&fs::read_to_string(&status_path)?)?,
            &generation,
            "status generation",
        )
    }

    #[cfg(unix)]
    #[test]
    fn publication_lock_dir_is_uid_private_mode_700_and_rejects_symlink() -> Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir()?;
        let dir = tmp.path().join("locks");
        ensure_publication_lock_dir(&dir)?;
        check(&(fs::metadata(&dir)?.permissions().mode() & 0o777), &0o700, "fresh lock dir mode")?;

        fs::set_permissions(&dir, fs::Permissions::from_mode(0o777))?;
        ensure_publication_lock_dir(&dir)?;
        check(
            &(fs::metadata(&dir)?.permissions().mode() & 0o777),
            &0o700,
            "repaired lock dir mode",
        )?;

        let link = tmp.path().join("link");
        std::os::unix::fs::symlink(&dir, &link)?;
        let error = match ensure_publication_lock_dir(&link) {
            Ok(()) => {
                return Err(color_eyre::eyre::eyre!("symlink lock dir was accepted"));
            }
            Err(error) => error,
        };
        check_true(error.to_string().contains("symlink"), "symlink lock dir rejection")?;

        let expected = std::env::temp_dir()
            .join(format!("perl-lsp-ux-scorecard-locks-{}", current_unix_uid()));
        check(&publication_lock_dir(), &expected, "uid-keyed lock dir")?;
        check_true(
            publication_lock_dir().as_os_str()
                != std::env::temp_dir().join("perl-lsp-ux-scorecard-locks").as_os_str(),
            "lock dir must not be the shared unkeyed temp path",
        )
    }

    #[test]
    fn published_surfaces_share_one_generation_with_receipt() -> Result<()> {
        let root = tempfile::tempdir()?;
        fs::write(root.path().join("measurements.json"), measurement_json(true))?;
        let output_path = root.path().join("scorecard.json");
        let status_path = root.path().join("status.md");
        let receipt_path = install_valid_receipt(root.path())?;

        publish_human(root.path(), &AtomicFsPublisher)?;

        let generation =
            validate_published_generation(&output_path, &status_path, Some(&receipt_path))?;
        let receipt: serde_json::Value = serde_json::from_str(&fs::read_to_string(&receipt_path)?)?;
        check(&parse_receipt_generation(&receipt)?, &generation, "receipt generation")?;
        validate_receipt_schema(root.path(), &receipt)
    }

    #[test]
    fn injected_failure_after_first_dest_write_preserves_previous_generation() -> Result<()> {
        let root = tempfile::tempdir()?;
        fs::write(root.path().join("measurements.json"), measurement_json(true))?;
        let output_path = root.path().join("scorecard.json");
        let status_path = root.path().join("status.md");
        let receipt_path = install_valid_receipt(root.path())?;
        publish_human(root.path(), &AtomicFsPublisher)?;
        let previous =
            validate_published_generation(&output_path, &status_path, Some(&receipt_path))?;
        let previous_scorecard = fs::read_to_string(&output_path)?;
        let previous_status = fs::read_to_string(&status_path)?;
        let previous_receipt = fs::read_to_string(&receipt_path)?;

        fs::write(root.path().join("measurements.json"), measurement_json(false))?;
        let error = match publish_human(
            root.path(),
            &FailAfterPublisher { succeed_before_fail: 1, published: std::cell::Cell::new(0) },
        ) {
            Ok(()) => {
                return Err(color_eyre::eyre::eyre!(
                    "publisher must fail after the first dest write"
                ));
            }
            Err(error) => error,
        };
        check_true(
            error.to_string().contains("injected publication failure"),
            "missing injected failure context",
        )?;
        check(&fs::read_to_string(&output_path)?, &previous_scorecard, "scorecard dest mixed")?;
        check(&fs::read_to_string(&status_path)?, &previous_status, "status dest mixed")?;
        check(&fs::read_to_string(&receipt_path)?, &previous_receipt, "receipt dest mixed")?;
        check_true(
            !sibling_with_suffix(&output_path, STAGING_SUFFIX)?.exists()
                && !sibling_with_suffix(&status_path, STAGING_SUFFIX)?.exists()
                && !sibling_with_suffix(&receipt_path, STAGING_SUFFIX)?.exists(),
            "dest-commit failure left staging files next to published dests",
        )?;
        check(
            &validate_published_generation(&output_path, &status_path, Some(&receipt_path))?,
            &previous,
            "rolled-back generation",
        )
    }

    #[test]
    fn injected_failure_before_receipt_write_preserves_previous_generation() -> Result<()> {
        let root = tempfile::tempdir()?;
        fs::write(root.path().join("measurements.json"), measurement_json(true))?;
        let output_path = root.path().join("scorecard.json");
        let status_path = root.path().join("status.md");
        let receipt_path = install_valid_receipt(root.path())?;
        publish_human(root.path(), &AtomicFsPublisher)?;
        let previous =
            validate_published_generation(&output_path, &status_path, Some(&receipt_path))?;

        fs::write(root.path().join("measurements.json"), measurement_json(false))?;
        let error = match publish_human(
            root.path(),
            &FailAfterPublisher { succeed_before_fail: 2, published: std::cell::Cell::new(0) },
        ) {
            Ok(()) => {
                return Err(color_eyre::eyre::eyre!(
                    "publisher must fail after the status dest write"
                ));
            }
            Err(error) => error,
        };
        check_true(
            error.to_string().contains("injected publication failure"),
            "missing injected failure context",
        )?;
        check(
            &validate_published_generation(&output_path, &status_path, Some(&receipt_path))?,
            &previous,
            "rolled-back generation after status dest write",
        )
    }

    #[test]
    fn validator_rejects_hand_mixed_scorecard_and_status_generations() -> Result<()> {
        let root = tempfile::tempdir()?;
        let output_path = root.path().join("scorecard.json");
        let status_path = root.path().join("status.md");
        let generation_a = fake_generation('a');
        let generation_b = fake_generation('b');
        fs::write(&output_path, format!("{{\"publication_generation\":\"{generation_a}\"}}\n"))?;
        fs::write(&status_path, format!("{STATUS_GENERATION_LABEL} `{generation_b}`  \n"))?;

        let error = match validate_published_generation(&output_path, &status_path, None) {
            Ok(generation) => {
                return Err(color_eyre::eyre::eyre!(
                    "mixed generations were accepted as {generation}"
                ));
            }
            Err(error) => error,
        };
        check_true(
            error.to_string().contains("mixed publication generation"),
            "mixed-set error missing mix diagnosis",
        )
    }

    #[test]
    fn identical_inputs_reuse_generation_and_changed_input_does_not() -> Result<()> {
        let first = tempfile::tempdir()?;
        fs::write(first.path().join("measurements.json"), measurement_json(true))?;
        publish_human(first.path(), &AtomicFsPublisher)?;
        let first_generation = validate_published_generation(
            &first.path().join("scorecard.json"),
            &first.path().join("status.md"),
            None,
        )?;

        let second = tempfile::tempdir()?;
        fs::write(second.path().join("measurements.json"), measurement_json(true))?;
        publish_human(second.path(), &AtomicFsPublisher)?;
        let second_generation = validate_published_generation(
            &second.path().join("scorecard.json"),
            &second.path().join("status.md"),
            None,
        )?;
        check(&second_generation, &first_generation, "identical inputs must reuse generation")?;

        let third = tempfile::tempdir()?;
        fs::write(third.path().join("measurements.json"), measurement_json(false))?;
        publish_human(third.path(), &AtomicFsPublisher)?;
        let third_generation = validate_published_generation(
            &third.path().join("scorecard.json"),
            &third.path().join("status.md"),
            None,
        )?;
        check_true(
            third_generation != first_generation,
            "changed input reused the previous generation",
        )
    }

    #[test]
    fn publish_rejects_duplicate_destination_paths() -> Result<()> {
        let root = tempfile::tempdir()?;
        let path = root.path().join("same.json");
        let surfaces = [
            PendingSurface { path: path.clone(), contents: b"generation-a\n".to_vec() },
            PendingSurface { path, contents: b"generation-b\n".to_vec() },
        ];
        let error = match publish_coherent_generation(&AtomicFsPublisher, &surfaces) {
            Ok(()) => {
                return Err(color_eyre::eyre::eyre!(
                    "duplicate destinations must fail before staging"
                ));
            }
            Err(error) => error,
        };
        check_true(
            error.to_string().contains("duplicate publication destination"),
            "duplicate dest error missing diagnosis",
        )
    }

    #[test]
    fn publish_rejects_destination_colliding_with_staging_sidecar() -> Result<()> {
        let root = tempfile::tempdir()?;
        let dest = root.path().join("scorecard.json");
        let stage = sibling_with_suffix(&dest, STAGING_SUFFIX)?;
        let surfaces = [
            PendingSurface { path: dest, contents: b"generation-a\n".to_vec() },
            PendingSurface { path: stage, contents: b"generation-b\n".to_vec() },
        ];
        let error = match publish_coherent_generation(&AtomicFsPublisher, &surfaces) {
            Ok(()) => {
                return Err(color_eyre::eyre::eyre!(
                    "dest/sidecar collision must fail before staging"
                ));
            }
            Err(error) => error,
        };
        check_true(
            error.to_string().contains("staging sidecar"),
            "sidecar collision error missing diagnosis",
        )
    }

    #[test]
    fn staging_failure_after_first_surface_removes_created_stages() -> Result<()> {
        let root = tempfile::tempdir()?;
        fs::write(root.path().join("measurements.json"), measurement_json(true))?;
        let output_path = root.path().join("scorecard.json");
        let status_path = root.path().join("status.md");
        publish_human(root.path(), &AtomicFsPublisher)?;
        let previous = validate_published_generation(&output_path, &status_path, None)?;
        let previous_scorecard = fs::read_to_string(&output_path)?;
        let previous_status = fs::read_to_string(&status_path)?;

        let blocking_stage = sibling_with_suffix(&status_path, STAGING_SUFFIX)?;
        fs::create_dir(&blocking_stage)?;
        fs::write(root.path().join("measurements.json"), measurement_json(false))?;
        let error = match publish_human(root.path(), &AtomicFsPublisher) {
            Ok(()) => {
                return Err(color_eyre::eyre::eyre!(
                    "publisher must fail when a later staging sidecar cannot be replaced"
                ));
            }
            Err(error) => error,
        };
        check_true(
            error.to_string().contains("removing stale staging"),
            "staging failure missing stale-sidecar context",
        )?;
        check(&fs::read_to_string(&output_path)?, &previous_scorecard, "scorecard dest mixed")?;
        check(&fs::read_to_string(&status_path)?, &previous_status, "status dest mixed")?;
        check_true(
            !sibling_with_suffix(&output_path, STAGING_SUFFIX)?.exists(),
            "staging failure left a scorecard staging sidecar",
        )?;
        check_true(blocking_stage.is_dir(), "pre-existing blocking sidecar directory was removed")?;
        check(
            &validate_published_generation(&output_path, &status_path, None)?,
            &previous,
            "rolled-back generation after staging failure",
        )
    }

    #[test]
    fn validator_requires_expected_receipt_file() -> Result<()> {
        let root = tempfile::tempdir()?;
        fs::write(root.path().join("measurements.json"), measurement_json(true))?;
        let output_path = root.path().join("scorecard.json");
        let status_path = root.path().join("status.md");
        publish_human(root.path(), &AtomicFsPublisher)?;
        let missing_receipt = root.path().join("target/receipts/receipt.json");
        let error =
            match validate_published_generation(&output_path, &status_path, Some(&missing_receipt))
            {
                Ok(generation) => {
                    return Err(color_eyre::eyre::eyre!(
                        "missing expected receipt was accepted as {generation}"
                    ));
                }
                Err(error) => error,
            };
        check_true(
            error.to_string().contains("reading"),
            "missing expected receipt error missing read context",
        )
    }

    #[test]
    fn validation_failure_after_dest_commit_restores_previous_generation() -> Result<()> {
        let root = tempfile::tempdir()?;
        fs::write(root.path().join("measurements.json"), measurement_json(true))?;
        let output_path = root.path().join("scorecard.json");
        let status_path = root.path().join("status.md");
        publish_human(root.path(), &AtomicFsPublisher)?;
        let previous = validate_published_generation(&output_path, &status_path, None)?;
        let previous_scorecard = fs::read_to_string(&output_path)?;
        let previous_status = fs::read_to_string(&status_path)?;

        let surfaces = [
            PendingSurface {
                path: output_path.clone(),
                contents: b"{\"not\":\"a generation\"}\n".to_vec(),
            },
            PendingSurface {
                path: status_path.clone(),
                contents: b"# no generation marker\n".to_vec(),
            },
        ];
        let error = match publish_coherent_generation(&AtomicFsPublisher, &surfaces) {
            Ok(()) => {
                return Err(color_eyre::eyre::eyre!(
                    "invalid dest payloads must fail validation under the publication lock"
                ));
            }
            Err(error) => error,
        };
        check_true(
            error.to_string().contains("publication_generation")
                || error.to_string().contains("generation marker"),
            "validation failure missing generation diagnosis",
        )?;
        check(&fs::read_to_string(&output_path)?, &previous_scorecard, "scorecard dest mixed")?;
        check(&fs::read_to_string(&status_path)?, &previous_status, "status dest mixed")?;
        check(
            &validate_published_generation(&output_path, &status_path, None)?,
            &previous,
            "rolled-back generation after validation failure",
        )
    }

    #[test]
    fn schema_invalid_receipt_is_rejected_before_enrichment() -> Result<()> {
        let root = tempfile::tempdir()?;
        let schema_path = root.path().join(".ci/receipt.schema.json");
        let schema_parent = schema_path
            .parent()
            .ok_or_else(|| color_eyre::eyre::eyre!("receipt schema path has no parent"))?;
        fs::create_dir_all(schema_parent)?;
        fs::write(&schema_path, include_str!("../../../.ci/receipt.schema.json"))?;

        let error = match validate_receipt_schema(root.path(), &json!({})) {
            Ok(()) => return Err(color_eyre::eyre::eyre!("schema-invalid receipt was accepted")),
            Err(error) => error,
        };
        check_true(
            error.to_string().contains("receipt schema validation failed"),
            "schema-invalid receipt error missing context",
        )
    }

    #[test]
    fn valid_receipt_enrichment_remains_schema_valid() -> Result<()> {
        let root = tempfile::tempdir()?;
        let schema_path = root.path().join(".ci/receipt.schema.json");
        let schema_parent = schema_path
            .parent()
            .ok_or_else(|| color_eyre::eyre::eyre!("receipt schema path has no parent"))?;
        fs::create_dir_all(schema_parent)?;
        fs::write(&schema_path, include_str!("../../../.ci/receipt.schema.json"))?;
        let receipt_path = root.path().join("target/receipts/receipt.json");
        fs::create_dir_all(
            receipt_path
                .parent()
                .ok_or_else(|| color_eyre::eyre::eyre!("receipt path has no parent"))?,
        )?;
        fs::write(&receipt_path, include_str!("../../../.ci/examples/receipt-pr-fast.json"))?;

        let artifact = UxScorecardArtifact {
            schema_version: 1,
            measured_at: "2026-01-01T00:00:00Z".to_string(),
            subsystem: "editor_ux",
            scenario_count: 0,
            scenario_ids: Vec::new(),
            rows: BTreeMap::new(),
            latency_by_request_class: BTreeMap::new(),
            provenance: json!({}),
            publication_generation: String::new(),
        };
        let (_, payload) = prepare_receipt_payload(root.path(), &artifact)?
            .ok_or_else(|| color_eyre::eyre::eyre!("existing receipt was not prepared"))?;
        let enriched: serde_json::Value = serde_json::from_str(&payload)?;
        check_true(
            enriched.get("ux_scorecard").is_some(),
            "prepared receipt omitted ux_scorecard",
        )?;
        validate_receipt_schema(root.path(), &enriched)
    }

    #[test]
    fn render_status_markdown_shows_coverage_pct() {
        let mut rows = BTreeMap::new();
        rows.insert("hover_correctness_pct".to_string(), PercentMetric { value: Some(100.0) });
        let artifact = UxScorecardArtifact {
            schema_version: 1,
            measured_at: "2026-01-01T00:00:00Z".to_string(),
            subsystem: "editor_ux",
            scenario_count: 8,
            scenario_ids: vec!["hover_core".to_string()],
            rows,
            latency_by_request_class: BTreeMap::new(),
            provenance: serde_json::json!({"declared_scenario_count": 22}),
            publication_generation: String::new(),
        };
        let md = render_status_markdown(&artifact, None);
        assert!(md.contains("8` of `22"), "expected coverage fraction, got:\n{md}");
        assert!(md.contains("36%"), "expected 36% coverage, got:\n{md}");
        assert!(md.contains("hover_core"), "expected scenario id list, got:\n{md}");
    }

    #[test]
    fn render_status_markdown_shows_baseline_deltas() {
        let mut rows = BTreeMap::new();
        rows.insert("hover_correctness_pct".to_string(), PercentMetric { value: Some(100.0) });
        let mut latency_by_request_class = BTreeMap::new();
        latency_by_request_class.insert(
            "hover".to_string(),
            LatencyPercentiles { p50_ms: Some(22.0), p95_ms: Some(29.0) },
        );
        let artifact = UxScorecardArtifact {
            schema_version: 1,
            measured_at: "2026-01-01T00:00:00Z".to_string(),
            subsystem: "editor_ux",
            scenario_count: 5,
            scenario_ids: vec![],
            rows,
            latency_by_request_class,
            provenance: serde_json::json!({"declared_scenario_count": 22}),
            publication_generation: String::new(),
        };
        let mut floor_metrics = BTreeMap::new();
        floor_metrics.insert("latency_hover_p50_ms".to_string(), Some(24.0_f64));
        floor_metrics.insert("latency_hover_p95_ms".to_string(), Some(31.0_f64));
        let baseline = SubsystemBaseline {
            floor_metrics,
            improvement_metrics: BTreeMap::new(),
            tolerance_pct: 0.1,
            lower_is_better: vec![],
            schema_version: 1,
            measured_at: String::new(),
            subsystem: "editor_ux".to_string(),
            commit: String::new(),
        };
        let md = render_status_markdown(&artifact, Some(&baseline));
        assert!(md.contains("p50 baseline"), "expected baseline column header, got:\n{md}");
        assert!(md.contains("24.00"), "expected baseline p50 value, got:\n{md}");
        assert!(md.contains("31.00"), "expected baseline p95 value, got:\n{md}");
    }

    #[test]
    fn render_status_markdown_shows_na_for_missing_values() {
        let mut rows = BTreeMap::new();
        rows.insert("hover_correctness_pct".to_string(), PercentMetric { value: None });

        let artifact = UxScorecardArtifact {
            schema_version: 1,
            measured_at: "2026-01-01T00:00:00Z".to_string(),
            subsystem: "editor_ux",
            scenario_count: 0,
            scenario_ids: vec![],
            rows,
            latency_by_request_class: BTreeMap::new(),
            provenance: serde_json::json!({}),
            publication_generation: String::new(),
        };

        let md = render_status_markdown(&artifact, None);
        assert!(md.contains("n/a"), "missing n/a for absent metric");
    }

    /// Verifies the full measurement→rows→latency pipeline produces consistent
    /// values and that the artifact serialization round-trips without data loss.
    #[test]
    fn pipeline_round_trip_produces_correct_rows() {
        let raw = vec![
            ScenarioMeasurement {
                scenario_id: "hover_test".to_string(),
                hover_correct: Some(true),
                completion_top1_correct: None,
                completion_top5_correct: None,
                definition_exact_hit: None,
                symbol_correct: Some(true),
                diagnostics_correct: Some(true),
                rename_success: None,
                cross_file_success: None,
                evidence_class: UxEvidenceClass::SemanticProof,
                latency_ms_by_request: BTreeMap::from([("hover".to_string(), vec![10, 20, 30])]),
            },
            ScenarioMeasurement {
                scenario_id: "completion_test".to_string(),
                hover_correct: Some(false),
                completion_top1_correct: Some(true),
                completion_top5_correct: Some(true),
                definition_exact_hit: None,
                symbol_correct: Some(false),
                diagnostics_correct: None,
                rename_success: Some(true),
                cross_file_success: Some(true),
                evidence_class: UxEvidenceClass::SemanticProof,
                latency_ms_by_request: BTreeMap::from([
                    ("completion".to_string(), vec![5, 15, 25]),
                    ("hover".to_string(), vec![50, 60, 70]),
                ]),
            },
        ];

        let scenarios = load_measurements(&raw);
        let scorecard = aggregate_editor_ux_scorecard(&scenarios);

        assert_eq!(scorecard.hover_correctness_pct, Some(50.0));
        assert_eq!(scorecard.completion_top1_pct, Some(100.0));
        assert_eq!(scorecard.completion_top5_pct, Some(100.0));
        // definition_exact_hit: neither measured → None
        assert_eq!(scorecard.definition_exact_hit_pct, None);
        // symbol_correct: true, false → 50%
        assert_eq!(scorecard.symbol_correctness_pct, Some(50.0));
        // diagnostics_correct: only scenario 1 measured → 100%
        assert_eq!(scorecard.diagnostics_correct_pct, Some(100.0));
        // rename_success: only scenario 2 measured → 100%
        assert_eq!(scorecard.rename_success_pct, Some(100.0));
        // cross_file_success: only scenario 2 measured → 100%
        assert_eq!(scorecard.cross_file_success_pct, Some(100.0));

        let latencies = compute_latency_percentiles(&raw);

        // "hover" samples: [10, 20, 30, 50, 60, 70] sorted.
        // p50: rank = (6-1)*0.50 = 2.5 → 3, samples[3]=50
        // p95: rank = (6-1)*0.95 = 4.75 → 5, samples[5]=70
        let hover_lat = latencies.get("hover").expect("hover latency present");
        assert_eq!(hover_lat.p50_ms, Some(50.0));
        assert_eq!(hover_lat.p95_ms, Some(70.0));

        // "completion": [5, 15, 25]; p50→15, p95→25
        let comp_lat = latencies.get("completion").expect("completion latency present");
        assert_eq!(comp_lat.p50_ms, Some(15.0));
        assert_eq!(comp_lat.p95_ms, Some(25.0));

        // Artifact serialization round-trip.
        let mut rows = BTreeMap::new();
        rows.insert(
            "hover_correctness_pct".to_string(),
            PercentMetric { value: scorecard.hover_correctness_pct },
        );
        rows.insert(
            "symbol_correctness_pct".to_string(),
            PercentMetric { value: scorecard.symbol_correctness_pct },
        );
        rows.insert(
            "diagnostics_correct_pct".to_string(),
            PercentMetric { value: scorecard.diagnostics_correct_pct },
        );
        rows.insert(
            "rename_success_pct".to_string(),
            PercentMetric { value: scorecard.rename_success_pct },
        );

        let artifact = UxScorecardArtifact {
            schema_version: 1,
            measured_at: "2026-01-01T00:00:00Z".to_string(),
            subsystem: "editor_ux",
            scenario_count: scorecard.scenario_count,
            scenario_ids: vec!["hover_test".to_string(), "completion_test".to_string()],
            rows,
            latency_by_request_class: latencies,
            provenance: serde_json::json!({"input": "fixture", "generator": "test", "declared_scenario_count": 22}),
            publication_generation: String::new(),
        };

        let json_str =
            serde_json::to_string_pretty(&artifact).expect("artifact must serialize without error");
        let parsed: serde_json::Value =
            serde_json::from_str(&json_str).expect("serialized artifact must parse back");

        assert_eq!(parsed["schema_version"], 1);
        assert_eq!(parsed["subsystem"], "editor_ux");
        assert_eq!(parsed["rows"]["hover_correctness_pct"]["value"], serde_json::json!(50.0));
        assert_eq!(parsed["rows"]["symbol_correctness_pct"]["value"], serde_json::json!(50.0));
        assert_eq!(parsed["rows"]["diagnostics_correct_pct"]["value"], serde_json::json!(100.0));
        assert_eq!(parsed["rows"]["rename_success_pct"]["value"], serde_json::json!(100.0));
        assert!(parsed["latency_by_request_class"]["hover"]["p50_ms"].is_number());
        assert!(parsed["latency_by_request_class"]["completion"]["p95_ms"].is_number());
        // scenario_ids round-trips
        assert_eq!(parsed["scenario_ids"][0], "hover_test");
        assert_eq!(parsed["scenario_ids"][1], "completion_test");
    }
}
