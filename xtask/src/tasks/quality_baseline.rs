//! Coverage and RIPR baseline receipt helpers.
//!
//! These commands are measurement-only. They emit local receipts that future
//! quality gates can consume, but they do not change CI policy.

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use color_eyre::eyre::{Context, Result, bail, eyre};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const QUALITY_RECEIPT_SCHEMA_VERSION: u64 = 1;
const COVERAGE_SAMPLE_UNCOVERED_LINES: usize = 5;
const LOCAL_COMMAND_PREFIX: &str = "rtk";

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct CountRow {
    name: String,
    count: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
struct RiprSeamSample {
    #[serde(skip_serializing_if = "Option::is_none")]
    gap_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    line: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    seam: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    suggested_test: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
struct RiprFileCluster {
    name: String,
    count: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    sample_seams: Vec<RiprSeamSample>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct DeferredCountRow {
    name: String,
    count: u64,
    reason: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct CoverageCounters {
    branch_hit: u64,
    branch_found: u64,
    branch_coverage: f64,
    line_hit: u64,
    line_found: u64,
    line_coverage: f64,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct CoverageScope {
    kind: String,
    source_files: u64,
    roots: Vec<String>,
    required_roots: Vec<String>,
    missing_required_roots: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
struct CoverageFileRow {
    path: String,
    line_hit: u64,
    line_found: u64,
    line_coverage: f64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    sample_uncovered_lines: Vec<u64>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct CoverageBaselineReceipt {
    schema_version: u64,
    kind: String,
    mode: String,
    head: Option<String>,
    lcov: String,
    codecov_config: String,
    local_ratchet: BTreeMap<String, String>,
    codecov_status: Value,
    codecov_comment: Value,
    measured: CoverageCounters,
    coverage_scope: CoverageScope,
    files_below_target: Vec<CoverageFileRow>,
    target: f64,
    decision: String,
    next_actions: Vec<Value>,
    claim_boundary: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct RiprPlusReceipt {
    schema_version: u64,
    kind: String,
    mode: String,
    head: Option<String>,
    root: String,
    source_format: String,
    unresolved: u64,
    new_unresolved: Option<u64>,
    by_kind: Vec<CountRow>,
    top_files: Vec<RiprFileCluster>,
    top_actionable_files: Vec<RiprFileCluster>,
    deferred_files: Vec<DeferredCountRow>,
    decision: String,
    next_actions: Vec<Value>,
    claim_boundary: Vec<String>,
}

pub fn coverage_baseline(
    lcov: &Path,
    baseline: &Path,
    codecov: &Path,
    receipt: &Path,
    check: bool,
) -> Result<()> {
    let packet = coverage_baseline_receipt(lcov, baseline, codecov, receipt)?;
    write_or_check_receipt(
        receipt,
        &packet,
        check,
        "coverage baseline receipt",
        &coverage_baseline_command(lcov, baseline, codecov, receipt, false),
        &coverage_baseline_command(lcov, baseline, codecov, receipt, true),
    )
}

pub fn ripr_plus(root: &str, receipt: &Path, check: bool) -> Result<()> {
    let packet = ripr_plus_receipt(root, receipt)?;
    write_or_check_receipt(
        receipt,
        &packet,
        check,
        "ripr+ receipt",
        &ripr_plus_command(root, receipt, false),
        &ripr_plus_command(root, receipt, true),
    )
}

fn coverage_baseline_receipt(
    lcov: &Path,
    baseline: &Path,
    codecov: &Path,
    receipt: &Path,
) -> Result<CoverageBaselineReceipt> {
    let counters = parse_lcov(lcov)
        .with_context(|| format!("reading coverage counters from {}", lcov.display()))?;
    let local_ratchet = parse_key_value_file(baseline)
        .with_context(|| format!("reading local coverage ratchet {}", baseline.display()))?;
    let codecov_config = read_codecov_config(codecov)
        .with_context(|| format!("reading Codecov policy {}", codecov.display()))?;
    let codecov_status =
        codecov_config.pointer("/coverage/status").cloned().unwrap_or_else(|| json!({}));
    let codecov_comment = codecov_config.get("comment").cloned().unwrap_or_else(|| json!({}));

    let target = 95.0;
    let coverage_scope = coverage_scope(lcov)?;
    let files_below_target = coverage_files_below_target(lcov, target, 10)?;
    let mut next_actions = Vec::new();
    if counters.line_coverage < target {
        next_actions.push(json!({
            "kind": "project_coverage_gap",
            "path": display_path(lcov),
            "current": counters.line_coverage,
            "target": target,
            "top_files": files_below_target.iter().take(5).collect::<Vec<_>>(),
            "repair": "Add focused behavior tests for the top uncovered files until project coverage reaches 95%.",
            "suggested_test": "Use the LCOV file report to rank uncovered public API, error handling, serialization, config, scheduler, and provider-decision branches.",
            "verify": coverage_baseline_command(lcov, baseline, codecov, receipt, true),
            "receipt": coverage_baseline_command(lcov, baseline, codecov, receipt, false)
        }));
    }

    Ok(CoverageBaselineReceipt {
        schema_version: QUALITY_RECEIPT_SCHEMA_VERSION,
        kind: "coverage_baseline".to_string(),
        mode: "advisory".to_string(),
        head: git_head(),
        lcov: display_path(lcov),
        codecov_config: display_path(codecov),
        local_ratchet,
        codecov_status,
        codecov_comment,
        measured: counters,
        coverage_scope,
        files_below_target,
        target,
        decision: "advisory".to_string(),
        next_actions,
        claim_boundary: vec![
            "Measurement only; this receipt does not enforce Codecov status.".to_string(),
            "Patch coverage is reported by Codecov on PRs and is not derivable from a repo-wide LCOV file.".to_string(),
            "Project coverage here is the local LCOV line-coverage snapshot, not a live Codecov API read; final enforcement requires workspace coverage scope.".to_string(),
        ],
    })
}

fn ripr_plus_receipt(root: &str, receipt: &Path) -> Result<RiprPlusReceipt> {
    let raw = run_ripr_repo_seams(root)?;
    let value: Value =
        serde_json::from_slice(&raw).context("ripr repo-seams-json output was invalid JSON")?;
    let seams = value
        .get("seams")
        .and_then(Value::as_array)
        .ok_or_else(|| eyre!("ripr repo-seams-json output did not include seams[]"))?;
    let unresolved = u64::try_from(seams.len()).context("seam count exceeded u64")?;

    let by_kind = top_counts(seams, "kind", usize::MAX);
    let top_files = top_file_clusters(seams, 10);
    let (top_actionable_files, deferred_files) = classified_file_counts(seams, 10);
    let mut next_actions = top_actionable_files
        .iter()
        .take(5)
        .map(|row| ripr_seam_cluster_action(row, root, receipt))
        .collect::<Vec<_>>();
    if next_actions.is_empty() {
        next_actions.extend(
            deferred_files
                .iter()
                .filter(|row| row.reason == "missing_actionable_sample")
                .take(5)
                .map(|row| ripr_missing_actionable_sample_action(row, root, receipt)),
        );
    }

    Ok(RiprPlusReceipt {
        schema_version: QUALITY_RECEIPT_SCHEMA_VERSION,
        kind: "ripr_plus_baseline".to_string(),
        mode: "advisory".to_string(),
        head: git_head(),
        root: root.to_string(),
        source_format: "ripr check --format repo-seams-json".to_string(),
        unresolved,
        new_unresolved: None,
        by_kind,
        top_files,
        top_actionable_files,
        deferred_files,
        decision: "advisory".to_string(),
        next_actions,
        claim_boundary: vec![
            "Measurement only; this receipt does not enforce ripr+ zero.".to_string(),
            "unresolved is the repo-seam count from RIPR repo-seams-json, before reviewed baseline debt is separated from new gaps.".to_string(),
            "new_unresolved is null until PR diff comparison is wired in the quality gate.".to_string(),
            "top_files is raw evidence; top_actionable_files requires actionable sample seams, and deferred_files only classifies generated, archived, or incomplete receipt rows without suppressing seams.".to_string(),
        ],
    })
}

fn coverage_baseline_command(
    lcov: &Path,
    baseline: &Path,
    codecov: &Path,
    receipt: &Path,
    check: bool,
) -> String {
    let mut command = local_command(format!(
        "cargo xtask coverage-baseline --lcov {} --baseline {} --codecov {} --receipt {}",
        command_arg(&display_path(lcov)),
        command_arg(&display_path(baseline)),
        command_arg(&display_path(codecov)),
        command_arg(&display_path(receipt))
    ));
    if check {
        command.push_str(" --check");
    }
    command
}

fn local_command(command: impl AsRef<str>) -> String {
    format!("{LOCAL_COMMAND_PREFIX} {}", command.as_ref())
}

fn ripr_seam_cluster_action(row: &RiprFileCluster, root: &str, receipt: &Path) -> Value {
    let mut action = json!({
        "kind": "ripr_seam_cluster",
        "path": row.name,
        "unresolved": row.count,
        "repair": "Add focused tests that expose the named RIPR seam cluster before changing production code.",
        "suggested_test": "Add focused tests that reveal the predicate, return value, error variant, field construction, or observer behavior for this seam cluster.",
        "verify": ripr_plus_command(root, receipt, true),
        "receipt": ripr_plus_command(root, receipt, false)
    });
    if !row.sample_seams.is_empty()
        && let Some(object) = action.as_object_mut()
    {
        object.insert("sample_seams".to_string(), json!(&row.sample_seams));
    }
    action
}

fn ripr_missing_actionable_sample_action(
    row: &DeferredCountRow,
    root: &str,
    receipt: &Path,
) -> Value {
    json!({
        "kind": "ripr_receipt_gap_guidance_missing",
        "path": row.name,
        "unresolved": row.count,
        "reason": row.reason,
        "repair": "Regenerate or improve the RIPR+ receipt so this file cluster includes gap id, positive line, seam, reason, and suggested test before using it as a focused burn-down target.",
        "suggested_test": "Add receipt-schema or analyzer fixture coverage proving RIPR+ sample seams carry gap id, file, positive line, seam, reason, and suggested test guidance.",
        "verify": ripr_plus_command(root, receipt, true),
        "receipt": ripr_plus_command(root, receipt, false)
    })
}

fn ripr_plus_command(root: &str, receipt: &Path, check: bool) -> String {
    let mut command = local_command(format!(
        "cargo xtask ripr-plus --root {} --receipt {}",
        command_arg(root),
        command_arg(&display_path(receipt))
    ));
    if check {
        command.push_str(" --check");
    }
    command
}

fn run_ripr_repo_seams(root: &str) -> Result<Vec<u8>> {
    let binary = match env::var("RIPR_BIN") {
        Ok(value) if !value.trim().is_empty() => value,
        Ok(_) => bail!("RIPR_BIN is set but empty"),
        Err(_) => "ripr".to_string(),
    };
    let output = Command::new(&binary)
        .args(["check", "--root", root, "--format", "repo-seams-json"])
        .output()
        .with_context(|| format!("failed to run {binary}"))?;
    if !output.status.success() {
        bail!("{binary} check failed:\n{}", String::from_utf8_lossy(&output.stderr));
    }
    Ok(output.stdout)
}

fn parse_lcov(path: &Path) -> Result<CoverageCounters> {
    let raw = fs::read_to_string(path)?;
    let mut branch_hit = 0_u64;
    let mut branch_found = 0_u64;
    let mut line_hit = 0_u64;
    let mut line_found = 0_u64;

    for line in raw.lines() {
        if let Some(value) = line.strip_prefix("BRH:") {
            branch_hit = branch_hit.saturating_add(parse_lcov_count(value, "BRH")?);
        } else if let Some(value) = line.strip_prefix("BRF:") {
            branch_found = branch_found.saturating_add(parse_lcov_count(value, "BRF")?);
        } else if let Some(value) = line.strip_prefix("LH:") {
            line_hit = line_hit.saturating_add(parse_lcov_count(value, "LH")?);
        } else if let Some(value) = line.strip_prefix("LF:") {
            line_found = line_found.saturating_add(parse_lcov_count(value, "LF")?);
        }
    }

    if line_found == 0 {
        bail!("LCOV did not report any measured lines; expected at least one LF record");
    }

    Ok(CoverageCounters {
        branch_hit,
        branch_found,
        branch_coverage: percent(branch_hit, branch_found),
        line_hit,
        line_found,
        line_coverage: percent(line_hit, line_found),
    })
}

fn coverage_scope(path: &Path) -> Result<CoverageScope> {
    let source_paths = lcov_source_paths(path)?;
    let source_files =
        u64::try_from(source_paths.len()).context("LCOV source count exceeded u64")?;
    let roots = coverage_roots(&source_paths);
    let required_roots = required_coverage_roots()?;
    let missing_required_roots = required_roots
        .iter()
        .filter(|required| !roots.iter().any(|root| root == *required))
        .cloned()
        .collect::<Vec<_>>();
    let kind = if missing_required_roots.is_empty() { "workspace" } else { "partial" }.to_string();

    Ok(CoverageScope { kind, source_files, roots, required_roots, missing_required_roots })
}

pub(crate) fn required_coverage_roots() -> Result<Vec<String>> {
    let manifest = workspace_root().join("Cargo.toml");
    let raw = fs::read_to_string(&manifest)
        .with_context(|| format!("failed to read workspace manifest {}", manifest.display()))?;
    let parsed: toml::Value = toml::from_str(&raw)
        .with_context(|| format!("failed to parse workspace manifest {}", manifest.display()))?;
    let members = parsed
        .get("workspace")
        .and_then(|workspace| workspace.get("members"))
        .and_then(toml::Value::as_array)
        .ok_or_else(|| {
            color_eyre::eyre::eyre!("workspace manifest is missing workspace.members")
        })?;

    let mut roots = members
        .iter()
        .filter_map(toml::Value::as_str)
        .map(normalize_member_root)
        .filter(|member| !member.is_empty())
        .collect::<Vec<_>>();
    roots.sort();
    roots.dedup();
    Ok(roots)
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn normalize_member_root(member: &str) -> String {
    let mut normalized = member.trim().replace('\\', "/");
    while let Some(rest) = normalized.strip_prefix("./") {
        normalized = rest.to_string();
    }
    normalized.trim_end_matches('/').to_string()
}

fn lcov_source_paths(path: &Path) -> Result<Vec<String>> {
    let raw = fs::read_to_string(path)?;
    let mut paths = Vec::new();
    for line in raw.lines() {
        let Some(source_file) = line.strip_prefix("SF:") else {
            continue;
        };
        let normalized = normalize_coverage_path(source_file);
        if !normalized.is_empty() {
            paths.push(normalized);
        }
    }
    paths.sort();
    paths.dedup();
    Ok(paths)
}

fn coverage_roots(source_paths: &[String]) -> Vec<String> {
    let mut roots = BTreeSet::new();
    for path in source_paths {
        if path.starts_with("crates/") {
            let mut parts = path.split('/');
            if let (Some(root), Some(crate_name)) = (parts.next(), parts.next()) {
                roots.insert(format!("{root}/{crate_name}"));
            }
        } else if path.starts_with("xtask/") {
            roots.insert("xtask".to_string());
        } else if let Some((root, _)) = path.split_once('/') {
            roots.insert(root.to_string());
        } else if !path.is_empty() {
            roots.insert(path.to_string());
        }
    }
    roots.into_iter().collect()
}

fn normalize_coverage_path(path: &str) -> String {
    let mut normalized = path.trim().replace('\\', "/");
    while let Some(rest) = normalized.strip_prefix("./") {
        normalized = rest.to_string();
    }
    if let Some(relative) = repo_relative_coverage_path(&normalized) {
        return relative.to_string();
    }
    normalized
}

fn repo_relative_coverage_path(path: &str) -> Option<&str> {
    const REPO_ROOTS: &[&str] = &["archive/", "crates/", "fuzz/", "vscode-extension/", "xtask/"];

    for root in REPO_ROOTS {
        if path.starts_with(root) {
            return Some(path);
        }
    }

    REPO_ROOTS
        .iter()
        .filter_map(|root| {
            let needle = format!("/{root}");
            path.find(&needle).map(|index| &path[index + 1..])
        })
        .min_by_key(|relative| path.len() - relative.len())
}

fn coverage_files_below_target(
    path: &Path,
    target: f64,
    limit: usize,
) -> Result<Vec<CoverageFileRow>> {
    let raw = fs::read_to_string(path)?;
    let mut files = Vec::new();
    let mut current_path = None::<String>;
    let mut line_hit = 0_u64;
    let mut line_found = 0_u64;
    let mut sample_uncovered_lines = Vec::new();

    for line in raw.lines() {
        if let Some(source_file) = line.strip_prefix("SF:") {
            flush_coverage_file(
                &mut files,
                current_path.take(),
                line_hit,
                line_found,
                target,
                std::mem::take(&mut sample_uncovered_lines),
            );
            current_path = Some(normalize_coverage_path(source_file));
            line_hit = 0;
            line_found = 0;
        } else if let Some(value) = line.strip_prefix("LH:") {
            line_hit = parse_lcov_count(value, "LH")?;
        } else if let Some(value) = line.strip_prefix("LF:") {
            line_found = parse_lcov_count(value, "LF")?;
        } else if let Some(value) = line.strip_prefix("DA:") {
            let (line_number, hit_count) = parse_lcov_da(value)?;
            if hit_count == 0 && sample_uncovered_lines.len() < COVERAGE_SAMPLE_UNCOVERED_LINES {
                sample_uncovered_lines.push(line_number);
            }
        } else if line == "end_of_record" {
            flush_coverage_file(
                &mut files,
                current_path.take(),
                line_hit,
                line_found,
                target,
                std::mem::take(&mut sample_uncovered_lines),
            );
            line_hit = 0;
            line_found = 0;
        }
    }

    flush_coverage_file(
        &mut files,
        current_path,
        line_hit,
        line_found,
        target,
        sample_uncovered_lines,
    );
    files.sort_by(|left, right| {
        left.line_coverage
            .total_cmp(&right.line_coverage)
            .then_with(|| right.line_found.cmp(&left.line_found))
            .then_with(|| left.path.cmp(&right.path))
    });
    files.truncate(limit);
    Ok(files)
}

fn flush_coverage_file(
    files: &mut Vec<CoverageFileRow>,
    path: Option<String>,
    line_hit: u64,
    line_found: u64,
    target: f64,
    sample_uncovered_lines: Vec<u64>,
) {
    let Some(path) = path else {
        return;
    };
    if line_found == 0 {
        return;
    }
    let line_coverage = percent(line_hit, line_found);
    if line_coverage < target {
        files.push(CoverageFileRow {
            path,
            line_hit,
            line_found,
            line_coverage,
            sample_uncovered_lines,
        });
    }
}

fn parse_lcov_count(value: &str, label: &str) -> Result<u64> {
    value.trim().parse::<u64>().with_context(|| format!("invalid LCOV {label} count {value:?}"))
}

fn parse_lcov_da(value: &str) -> Result<(u64, u64)> {
    let mut fields = value.split(',');
    let line_number = fields
        .next()
        .ok_or_else(|| eyre!("invalid LCOV DA entry {value:?}: missing line number"))
        .and_then(|field| parse_lcov_count(field, "DA line"))?;
    if line_number == 0 {
        bail!("invalid LCOV DA entry {value:?}: line number must be positive");
    }
    let hit_count = fields
        .next()
        .ok_or_else(|| eyre!("invalid LCOV DA entry {value:?}: missing hit count"))
        .and_then(|field| parse_lcov_count(field, "DA hit"))?;
    Ok((line_number, hit_count))
}

fn percent(hit: u64, found: u64) -> f64 {
    if found == 0 {
        100.0
    } else {
        let pct = (hit as f64 / found as f64) * 100.0;
        (pct * 100.0).round() / 100.0
    }
}

fn parse_key_value_file(path: &Path) -> Result<BTreeMap<String, String>> {
    let raw = fs::read_to_string(path)?;
    let mut values = BTreeMap::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some((key, value)) = trimmed.split_once('=') else {
            continue;
        };
        values.insert(key.trim().to_string(), value.trim().to_string());
    }
    Ok(values)
}

fn read_codecov_config(path: &Path) -> Result<Value> {
    let raw = fs::read_to_string(path)?;
    let yaml: Value = serde_yaml_ng::from_str(&raw)?;
    Ok(yaml)
}

fn top_counts(seams: &[Value], field: &str, limit: usize) -> Vec<CountRow> {
    let mut counts = BTreeMap::<String, u64>::new();
    for seam in seams {
        if let Some(name) = ripr_count_field(seam, field) {
            *counts.entry(name.to_string()).or_default() += 1;
        }
    }
    let mut rows =
        counts.into_iter().map(|(name, count)| CountRow { name, count }).collect::<Vec<_>>();
    rows.sort_by(|left, right| {
        right.count.cmp(&left.count).then_with(|| left.name.cmp(&right.name))
    });
    rows.truncate(limit);
    rows
}

fn ripr_count_field(seam: &Value, field: &str) -> Option<String> {
    if field == "kind" {
        return ripr_seam_kind(seam);
    }
    seam.get(field)
        .and_then(Value::as_str)
        .filter(|name| !name.trim().is_empty())
        .map(ToOwned::to_owned)
}

fn top_file_clusters(seams: &[Value], limit: usize) -> Vec<RiprFileCluster> {
    file_clusters(seams.iter(), limit)
}

fn classified_file_counts(
    seams: &[Value],
    limit: usize,
) -> (Vec<RiprFileCluster>, Vec<DeferredCountRow>) {
    let mut deferred = BTreeMap::<(String, String), u64>::new();
    let mut actionable_seams = Vec::new();
    for seam in seams {
        let Some(path) = ripr_seam_path(seam) else {
            continue;
        };
        if let Some(reason) = deferred_ripr_file_reason(&path) {
            *deferred.entry((path.to_string(), reason.to_string())).or_default() += 1;
        } else if ripr_seam_sample_is_actionable(&ripr_seam_sample(seam)) {
            actionable_seams.push(seam);
        } else {
            *deferred
                .entry((path.to_string(), "missing_actionable_sample".to_string()))
                .or_default() += 1;
        }
    }

    let actionable = file_clusters(actionable_seams, limit);

    let mut deferred = deferred
        .into_iter()
        .map(|((name, reason), count)| DeferredCountRow { name, count, reason })
        .collect::<Vec<_>>();
    deferred.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.reason.cmp(&right.reason))
    });
    deferred.truncate(limit);

    (actionable, deferred)
}

fn file_clusters<'a, I>(seams: I, limit: usize) -> Vec<RiprFileCluster>
where
    I: IntoIterator<Item = &'a Value>,
{
    let mut clusters = BTreeMap::<String, RiprFileCluster>::new();
    for seam in seams {
        let Some(path) = ripr_seam_path(seam) else {
            continue;
        };
        let cluster = clusters.entry(path.to_string()).or_insert_with(|| RiprFileCluster {
            name: path.to_string(),
            count: 0,
            sample_seams: Vec::new(),
        });
        cluster.count = cluster.count.saturating_add(1);
        if cluster.sample_seams.len() < 3 {
            cluster.sample_seams.push(ripr_seam_sample(seam));
        }
    }

    let mut rows = clusters.into_values().collect::<Vec<_>>();
    rows.sort_by(|left, right| {
        right.count.cmp(&left.count).then_with(|| left.name.cmp(&right.name))
    });
    rows.truncate(limit);
    rows
}

fn ripr_seam_path(seam: &Value) -> Option<String> {
    first_string(
        seam,
        &[
            "/file",
            "/path",
            "/location/path",
            "/span/path",
            "/placement/path",
            "/evidence_record/path",
        ],
    )
    .map(|path| normalize_ripr_path(&path))
    .filter(|path| !path.is_empty())
}

fn ripr_seam_kind(seam: &Value) -> Option<String> {
    first_string(seam, &["/kind", "/evidence_record/kind", "/identity/kind", "/gap_kind"])
}

fn normalize_ripr_path(path: &str) -> String {
    let mut normalized = path.trim().replace('\\', "/");
    while let Some(rest) = normalized.strip_prefix("./") {
        normalized = rest.to_string();
    }
    normalized
}

fn ripr_seam_sample(seam: &Value) -> RiprSeamSample {
    RiprSeamSample {
        gap_id: first_string(
            seam,
            &[
                "/canonical_gap_id",
                "/gap_id",
                "/identity/canonical_gap_id",
                "/evidence_record/canonical_gap_id",
                "/evidence_record/gap_id",
                "/id",
            ],
        ),
        kind: ripr_seam_kind(seam),
        line: first_u64(
            seam,
            &["/line", "/location/line", "/span/line", "/placement/line", "/evidence_record/line"],
        ),
        seam: first_string(seam, &["/seam", "/placement/mode", "/owner", "/evidence_record/seam"]),
        reason: first_string(seam, &["/reason", "/why", "/message"]),
        suggested_test: first_string(
            seam,
            &[
                "/suggested_test/intent",
                "/suggested_test/name",
                "/suggested_test",
                "/repair",
                "/recommended_repair",
            ],
        ),
    }
}

fn ripr_seam_sample_is_actionable(sample: &RiprSeamSample) -> bool {
    string_option_is_filled(sample.gap_id.as_deref())
        && string_option_is_filled(sample.kind.as_deref())
        && sample.line.is_some_and(|line| line > 0)
        && string_option_is_filled(sample.seam.as_deref())
        && string_option_is_filled(sample.reason.as_deref())
        && string_option_is_filled(sample.suggested_test.as_deref())
}

fn string_option_is_filled(value: Option<&str>) -> bool {
    value.is_some_and(|text| !text.trim().is_empty())
}

fn first_string(value: &Value, paths: &[&str]) -> Option<String> {
    paths.iter().find_map(|path| {
        value
            .pointer(path)
            .and_then(Value::as_str)
            .filter(|text| !text.trim().is_empty())
            .map(ToOwned::to_owned)
    })
}

fn first_u64(value: &Value, paths: &[&str]) -> Option<u64> {
    paths.iter().find_map(|path| value.pointer(path).and_then(Value::as_u64))
}

fn deferred_ripr_file_reason(path: &str) -> Option<&'static str> {
    let normalized = normalize_ripr_path(path).to_ascii_lowercase();
    if normalized.starts_with("archive/") || normalized.contains("/archive/") {
        return Some("archive");
    }

    let components = normalized.split('/').collect::<Vec<_>>();
    let file_name = components.last().copied().unwrap_or_default();
    if components
        .iter()
        .any(|component| matches!(*component, "generated" | "gen" | "codegen" | "out"))
        || file_name.ends_with(".generated.rs")
        || file_name == "bindings.rs"
    {
        return Some("generated");
    }

    None
}

pub(crate) fn write_or_check_receipt<T>(
    receipt: &Path,
    packet: &T,
    check: bool,
    artifact: &str,
    refresh_command: &str,
    verify_command: &str,
) -> Result<()>
where
    T: Serialize,
{
    let rendered = format!("{}\n", serde_json::to_string_pretty(packet)?);
    if check {
        let actual = match fs::read_to_string(receipt) {
            Ok(actual) => actual,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                bail!(
                    "missing {artifact} {}; refresh with `{refresh_command}`, then verify with `{verify_command}`",
                    receipt.display()
                );
            }
            Err(error) => {
                bail!(
                    "unreadable {artifact} {}: {error}; refresh with `{refresh_command}`, then verify with `{verify_command}`",
                    receipt.display()
                );
            }
        };
        if actual != rendered {
            bail!(
                "{} is stale; refresh with `{refresh_command}`, then verify with `{verify_command}`",
                receipt.display()
            );
        }
        println!("Receipt is current: {}", receipt.display());
        return Ok(());
    }

    if let Some(parent) = receipt.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating receipt directory {}", parent.display()))?;
    }
    fs::write(receipt, rendered)
        .with_context(|| format!("writing receipt {}", receipt.display()))?;
    println!("Wrote {}", receipt.display());
    Ok(())
}

pub(crate) fn display_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

pub(crate) fn command_arg(value: &str) -> String {
    if value.is_empty() {
        return "''".to_string();
    }
    if value.bytes().any(|byte| byte.is_ascii_whitespace()) || value.contains(['\'', '"']) {
        format!("'{}'", value.replace('\'', "''"))
    } else {
        value.to_string()
    }
}

pub(crate) fn git_head() -> Option<String> {
    let output = Command::new("git").args(["rev-parse", "HEAD"]).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let head = String::from_utf8(output.stdout).ok()?;
    let trimmed = head.trim();
    if trimmed.is_empty() { None } else { Some(trimmed.to_string()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

    #[test]
    fn lcov_parser_totals_branch_and_line_counts() -> TestResult {
        let dir = tempfile::tempdir()?;
        let lcov = dir.path().join("lcov.info");
        fs::write(
            &lcov,
            "TN:\nBRF:4\nBRH:3\nLF:10\nLH:8\nend_of_record\nBRF:2\nBRH:1\nLF:5\nLH:5\n",
        )?;

        let counters = parse_lcov(&lcov)?;

        assert_eq!(counters.branch_found, 6);
        assert_eq!(counters.branch_hit, 4);
        assert_eq!(counters.branch_coverage, 66.67);
        assert_eq!(counters.line_found, 15);
        assert_eq!(counters.line_hit, 13);
        assert_eq!(counters.line_coverage, 86.67);
        Ok(())
    }

    #[test]
    fn lcov_parser_rejects_empty_line_measurements() -> TestResult {
        let dir = tempfile::tempdir()?;
        let lcov = dir.path().join("lcov.info");
        fs::write(&lcov, "TN:\nBRF:0\nBRH:0\nend_of_record\n")?;

        let Err(error) = parse_lcov(&lcov) else {
            return Err("empty LCOV line measurement should fail".into());
        };

        assert!(error.to_string().contains("expected at least one LF record"));
        Ok(())
    }

    #[test]
    fn lcov_file_rows_rank_below_target_files() -> TestResult {
        let dir = tempfile::tempdir()?;
        let lcov = dir.path().join("lcov.info");
        fs::write(
            &lcov,
            concat!(
                "SF:crates/a/src/lib.rs\n",
                "DA:1,0\nDA:2,1\nDA:3,0\nDA:4,0\nDA:5,1\nDA:6,0\nDA:7,0\nDA:8,0\n",
                "LF:10\nLH:4\nend_of_record\n",
                "SF:crates/b/src/lib.rs\nDA:10,1\nDA:11,0\nDA:12,1\nLF:20\nLH:18\nend_of_record\n",
                "SF:crates/c/src/lib.rs\nDA:20,1\nLF:8\nLH:8\nend_of_record\n",
            ),
        )?;

        let rows = coverage_files_below_target(&lcov, 95.0, 10)?;

        assert_eq!(
            rows,
            vec![
                CoverageFileRow {
                    path: "crates/a/src/lib.rs".to_string(),
                    line_hit: 4,
                    line_found: 10,
                    line_coverage: 40.0,
                    sample_uncovered_lines: vec![1, 3, 4, 6, 7],
                },
                CoverageFileRow {
                    path: "crates/b/src/lib.rs".to_string(),
                    line_hit: 18,
                    line_found: 20,
                    line_coverage: 90.0,
                    sample_uncovered_lines: vec![11],
                },
            ]
        );
        Ok(())
    }

    #[test]
    fn coverage_scope_records_required_workspace_roots() -> TestResult {
        let dir = tempfile::tempdir()?;
        let lcov = dir.path().join("lcov.info");
        let required_roots = required_coverage_roots()?;
        fs::write(&lcov, lcov_for_roots(&required_roots))?;

        let scope = coverage_scope(&lcov)?;
        let source_files =
            u64::try_from(required_roots.len()).context("required root count exceeded u64")?;

        assert_eq!(
            scope,
            CoverageScope {
                kind: "workspace".to_string(),
                source_files,
                roots: required_roots.clone(),
                required_roots,
                missing_required_roots: Vec::new(),
            }
        );
        Ok(())
    }

    #[test]
    fn coverage_scope_normalizes_absolute_lcov_source_paths() -> TestResult {
        let dir = tempfile::tempdir()?;
        let lcov = dir.path().join("lcov.info");
        fs::write(
            &lcov,
            concat!(
                "SF:/home/runner/work/perl-lsp-swarm/perl-lsp-swarm/crates/perl-parser/src/lib.rs\n",
                "LF:10\nLH:10\nend_of_record\n",
                "SF:C:\\work\\perl-lsp-swarm\\xtask\\src\\tasks\\quality_gate.rs\n",
                "LF:5\nLH:5\nend_of_record\n",
            ),
        )?;

        let scope = coverage_scope(&lcov)?;

        assert_eq!(scope.kind, "partial");
        assert_eq!(scope.roots, vec!["crates/perl-parser".to_string(), "xtask".to_string()]);
        assert!(scope.missing_required_roots.iter().any(|root| root == "crates/perl-lsp-rs"));
        Ok(())
    }

    #[test]
    fn coverage_scope_classifies_parser_plus_xtask_lcov_as_partial() -> TestResult {
        let dir = tempfile::tempdir()?;
        let lcov = dir.path().join("lcov.info");
        fs::write(
            &lcov,
            concat!(
                "SF:crates/perl-parser/src/lib.rs\nLF:10\nLH:10\nend_of_record\n",
                "SF:xtask/src/tasks/quality_gate.rs\nLF:5\nLH:5\nend_of_record\n",
            ),
        )?;

        let scope = coverage_scope(&lcov)?;

        assert_eq!(scope.kind, "partial");
        assert_eq!(scope.roots, vec!["crates/perl-parser".to_string(), "xtask".to_string()]);
        assert!(!scope.missing_required_roots.iter().any(|root| root == "xtask"));
        assert!(scope.missing_required_roots.iter().any(|root| root == "crates/perl-lsp-rs"));
        Ok(())
    }

    #[test]
    fn coverage_rows_normalize_absolute_lcov_source_paths() -> TestResult {
        let dir = tempfile::tempdir()?;
        let lcov = dir.path().join("lcov.info");
        fs::write(
            &lcov,
            concat!(
                "SF:/home/runner/work/perl-lsp-swarm/perl-lsp-swarm/crates/perl-parser/src/lib.rs\n",
                "DA:12,0\nDA:13,1\nLF:10\nLH:4\nend_of_record\n",
            ),
        )?;

        let rows = coverage_files_below_target(&lcov, 95.0, 10)?;

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].path, "crates/perl-parser/src/lib.rs");
        assert_eq!(rows[0].sample_uncovered_lines, vec![12]);
        Ok(())
    }

    #[test]
    fn coverage_path_normalization_preserves_archive_root() {
        assert_eq!(
            normalize_coverage_path(
                "/home/runner/work/perl-lsp-swarm/perl-lsp-swarm/archive/crates/old/src/lib.rs",
            ),
            "archive/crates/old/src/lib.rs"
        );
    }

    fn lcov_for_roots(roots: &[String]) -> String {
        roots
            .iter()
            .enumerate()
            .map(|(index, root)| {
                let line = index + 1;
                format!("SF:{root}/src/lib.rs\nDA:{line},1\nLF:1\nLH:1\nend_of_record\n")
            })
            .collect::<String>()
    }

    #[test]
    fn lcov_da_parser_rejects_missing_hit_counts() -> TestResult {
        let dir = tempfile::tempdir()?;
        let lcov = dir.path().join("lcov.info");
        fs::write(&lcov, "SF:crates/a/src/lib.rs\nDA:12\nLF:1\nLH:0\nend_of_record\n")?;

        let Err(error) = coverage_files_below_target(&lcov, 95.0, 10) else {
            return Err("malformed DA entry should fail the coverage receipt".into());
        };

        assert!(error.to_string().contains("missing hit count"));
        Ok(())
    }

    #[test]
    fn lcov_da_parser_rejects_zero_line_numbers() -> TestResult {
        let dir = tempfile::tempdir()?;
        let lcov = dir.path().join("lcov.info");
        fs::write(&lcov, "SF:crates/a/src/lib.rs\nDA:0,0\nLF:1\nLH:0\nend_of_record\n")?;

        let Err(error) = coverage_files_below_target(&lcov, 95.0, 10) else {
            return Err("DA line 0 should fail the coverage receipt".into());
        };

        assert!(error.to_string().contains("line number must be positive"));
        Ok(())
    }

    #[test]
    fn coverage_baseline_receipt_gap_action_includes_repair_and_receipt_commands() -> TestResult {
        let dir = tempfile::tempdir()?;
        let lcov = dir.path().join("lcov.info");
        let baseline = dir.path().join("coverage-baseline.txt");
        let codecov = dir.path().join("codecov.yml");
        let receipt_path = dir.path().join("custom-coverage-baseline.json");
        fs::write(&lcov, "SF:crates/a/src/lib.rs\nLF:10\nLH:4\nend_of_record\n")?;
        fs::write(&baseline, "schema_version=1\nbaseline_branch_coverage=50.00\n")?;
        fs::write(
            &codecov,
            "coverage:\n  status:\n    patch:\n      default:\n        target: 95%\n        threshold: 0%\ncomment:\n  layout: reach,diff,files\n",
        )?;

        let receipt = coverage_baseline_receipt(&lcov, &baseline, &codecov, &receipt_path)?;
        let action = receipt.next_actions.first().ok_or("coverage gap action missing")?;

        assert_eq!(action.get("kind").and_then(Value::as_str), Some("project_coverage_gap"));
        assert_eq!(
            action.get("repair").and_then(Value::as_str),
            Some(
                "Add focused behavior tests for the top uncovered files until project coverage reaches 95%."
            )
        );
        assert_eq!(
            action.pointer("/top_files/0/path").and_then(Value::as_str),
            Some("crates/a/src/lib.rs")
        );
        let verify = action.get("verify").and_then(Value::as_str).ok_or("verify missing")?;
        assert!(verify.starts_with("rtk cargo xtask coverage-baseline"));
        assert!(verify.contains(&format!("--lcov {}", display_path(&lcov))));
        assert!(verify.contains(&format!("--baseline {}", display_path(&baseline))));
        assert!(verify.contains(&format!("--codecov {}", display_path(&codecov))));
        assert!(verify.contains(&format!("--receipt {}", display_path(&receipt_path))));
        assert!(verify.contains("--check"));
        let receipt_command =
            action.get("receipt").and_then(Value::as_str).ok_or("receipt missing")?;
        assert!(receipt_command.starts_with("rtk cargo xtask coverage-baseline"));
        assert!(receipt_command.contains(&format!("--receipt {}", display_path(&receipt_path))));
        assert!(!receipt_command.contains("--check"));
        Ok(())
    }

    #[test]
    fn top_counts_orders_by_count_then_name() -> TestResult {
        let seams = serde_json::from_value::<Vec<Value>>(json!([
            {"kind": "return_value", "file": "b.rs"},
            {"kind": "call_presence", "file": "a.rs"},
            {"kind": "call_presence", "file": "a.rs"},
            {"kind": "return_value", "file": "a.rs"},
            {"evidence_record": {"kind": "return_value", "path": "c.rs"}},
            {"identity": {"kind": "predicate_boundary"}, "path": "d.rs"},
            {"gap_kind": "predicate_boundary", "path": "e.rs"}
        ]))?;

        let rows = top_counts(&seams, "kind", 10);

        assert_eq!(
            rows,
            vec![
                CountRow { name: "return_value".to_string(), count: 3 },
                CountRow { name: "call_presence".to_string(), count: 2 },
                CountRow { name: "predicate_boundary".to_string(), count: 2 },
            ]
        );
        Ok(())
    }

    #[test]
    fn ripr_file_clusters_accept_multiple_path_shapes() -> TestResult {
        let seams = serde_json::from_value::<Vec<Value>>(json!([
            {"kind": "return_value", "path": "./crates/perl-parser/src/lib.rs"},
            {"kind": "call_presence", "location": {"path": "crates/perl-parser/src/lib.rs"}},
            {"kind": "predicate_boundary", "evidence_record": {"path": "crates\\perl-lexer\\src\\lib.rs"}}
        ]))?;

        let rows = top_file_clusters(&seams, 10);

        assert_eq!(
            rows,
            vec![
                RiprFileCluster {
                    name: "crates/perl-parser/src/lib.rs".to_string(),
                    count: 2,
                    sample_seams: vec![
                        RiprSeamSample {
                            gap_id: None,
                            kind: Some("return_value".to_string()),
                            line: None,
                            seam: None,
                            reason: None,
                            suggested_test: None,
                        },
                        RiprSeamSample {
                            gap_id: None,
                            kind: Some("call_presence".to_string()),
                            line: None,
                            seam: None,
                            reason: None,
                            suggested_test: None,
                        },
                    ],
                },
                RiprFileCluster {
                    name: "crates/perl-lexer/src/lib.rs".to_string(),
                    count: 1,
                    sample_seams: vec![RiprSeamSample {
                        gap_id: None,
                        kind: Some("predicate_boundary".to_string()),
                        line: None,
                        seam: None,
                        reason: None,
                        suggested_test: None,
                    }],
                },
            ]
        );
        Ok(())
    }

    #[test]
    fn ripr_file_clusters_preserve_placement_sample_details() -> TestResult {
        let seams = serde_json::from_value::<Vec<Value>>(json!([
            {
                "identity": {"canonical_gap_id": "RIPR-SPEC-0088"},
                "gap_kind": "focused_test",
                "placement": {
                    "path": "./crates/perl-parser/src/lib.rs",
                    "line": 88,
                    "mode": "exact_seam_line"
                },
                "reason": "changed branch has no focused proof",
                "recommended_repair": "add a parser branch regression test"
            }
        ]))?;

        let rows = top_file_clusters(&seams, 10);

        assert_eq!(
            rows,
            vec![RiprFileCluster {
                name: "crates/perl-parser/src/lib.rs".to_string(),
                count: 1,
                sample_seams: vec![RiprSeamSample {
                    gap_id: Some("RIPR-SPEC-0088".to_string()),
                    kind: Some("focused_test".to_string()),
                    line: Some(88),
                    seam: Some("exact_seam_line".to_string()),
                    reason: Some("changed branch has no focused proof".to_string()),
                    suggested_test: Some("add a parser branch regression test".to_string()),
                }],
            }]
        );
        Ok(())
    }

    #[test]
    fn ripr_file_clusters_separate_actionable_and_deferred_files() -> TestResult {
        let seams = serde_json::from_value::<Vec<Value>>(json!([
            {"kind": "return_value", "file": "archive/crates/old-parser/src/lib.rs"},
            {"kind": "call_presence", "file": "archive/crates/old-parser/src/lib.rs"},
            {
                "canonical_gap_id": "RIPR-SPEC-0007",
                "kind": "predicate_boundary",
                "file": "crates/perl-parser/src/lib.rs",
                "line": 42,
                "seam": "parse_expr",
                "reason": "boundary branch is unobserved",
                "suggested_test": {"intent": "prove parser boundary branch"}
            },
            {"kind": "predicate_boundary", "file": "crates/perl-parser/src/lib.rs", "line": 43},
            {"kind": "return_value", "file": "crates/perl-lexer/src/generated/bindings.rs"}
        ]))?;

        let (actionable, deferred) = classified_file_counts(&seams, 10);

        assert_eq!(
            actionable,
            vec![RiprFileCluster {
                name: "crates/perl-parser/src/lib.rs".to_string(),
                count: 1,
                sample_seams: vec![RiprSeamSample {
                    gap_id: Some("RIPR-SPEC-0007".to_string()),
                    kind: Some("predicate_boundary".to_string()),
                    line: Some(42),
                    seam: Some("parse_expr".to_string()),
                    reason: Some("boundary branch is unobserved".to_string()),
                    suggested_test: Some("prove parser boundary branch".to_string()),
                }],
            }]
        );
        assert_eq!(
            deferred,
            vec![
                DeferredCountRow {
                    name: "archive/crates/old-parser/src/lib.rs".to_string(),
                    count: 2,
                    reason: "archive".to_string(),
                },
                DeferredCountRow {
                    name: "crates/perl-lexer/src/generated/bindings.rs".to_string(),
                    count: 1,
                    reason: "generated".to_string(),
                },
                DeferredCountRow {
                    name: "crates/perl-parser/src/lib.rs".to_string(),
                    count: 1,
                    reason: "missing_actionable_sample".to_string(),
                },
            ]
        );
        Ok(())
    }

    #[test]
    fn actionable_ripr_file_clusters_require_positive_line_and_repair_guidance() -> TestResult {
        let seams = serde_json::from_value::<Vec<Value>>(json!([
            {
                "canonical_gap_id": "RIPR-SPEC-0007",
                "kind": "predicate_boundary",
                "file": "crates/perl-parser/src/lib.rs",
                "line": 0,
                "seam": "parse_expr_zero",
                "reason": "zero line cannot identify a repair seam",
                "suggested_test": "prove parser zero-line row is rejected"
            },
            {
                "canonical_gap_id": "RIPR-SPEC-0008",
                "kind": "predicate_boundary",
                "file": "crates/perl-parser/src/lib.rs",
                "line": 44,
                "seam": "parse_expr_missing_test",
                "reason": "repair guidance is missing"
            },
            {
                "canonical_gap_id": "RIPR-SPEC-0009",
                "kind": "predicate_boundary",
                "file": "crates/perl-parser/src/lib.rs",
                "line": 45,
                "seam": "parse_expr_actionable",
                "reason": "boundary branch is unobserved",
                "suggested_test": "prove parser actionable branch"
            }
        ]))?;

        let (actionable, deferred) = classified_file_counts(&seams, 10);

        assert_eq!(
            actionable,
            vec![RiprFileCluster {
                name: "crates/perl-parser/src/lib.rs".to_string(),
                count: 1,
                sample_seams: vec![RiprSeamSample {
                    gap_id: Some("RIPR-SPEC-0009".to_string()),
                    kind: Some("predicate_boundary".to_string()),
                    line: Some(45),
                    seam: Some("parse_expr_actionable".to_string()),
                    reason: Some("boundary branch is unobserved".to_string()),
                    suggested_test: Some("prove parser actionable branch".to_string()),
                }],
            }]
        );
        assert_eq!(
            deferred,
            vec![DeferredCountRow {
                name: "crates/perl-parser/src/lib.rs".to_string(),
                count: 2,
                reason: "missing_actionable_sample".to_string(),
            }]
        );
        Ok(())
    }

    #[test]
    fn ripr_file_deferred_classifier_handles_windows_paths() -> TestResult {
        assert_eq!(
            deferred_ripr_file_reason(r#".\archive\crates\old-parser\src\lib.rs"#),
            Some("archive")
        );
        assert_eq!(
            deferred_ripr_file_reason(r#"crates\perl-lexer\src\generated\bindings.rs"#),
            Some("generated")
        );
        assert_eq!(deferred_ripr_file_reason("crates/perl-parser/src/lib.rs"), None);
        Ok(())
    }

    #[test]
    fn ripr_seam_cluster_action_includes_repair_and_receipt_commands() -> TestResult {
        let dir = tempfile::tempdir()?;
        let receipt_path = dir.path().join("custom-ripr-plus.json");
        let row = RiprFileCluster {
            name: "crates/perl-parser/src/lib.rs".to_string(),
            count: 3,
            sample_seams: vec![RiprSeamSample {
                gap_id: Some("RIPR-SPEC-0007".to_string()),
                kind: Some("predicate_boundary".to_string()),
                line: Some(42),
                seam: Some("parse_expr".to_string()),
                reason: Some("boundary branch is unobserved".to_string()),
                suggested_test: Some("prove parser boundary branch".to_string()),
            }],
        };

        let action = ripr_seam_cluster_action(&row, ".", &receipt_path);

        assert_eq!(action.get("kind").and_then(Value::as_str), Some("ripr_seam_cluster"));
        assert_eq!(
            action.get("repair").and_then(Value::as_str),
            Some(
                "Add focused tests that expose the named RIPR seam cluster before changing production code."
            )
        );
        assert_eq!(
            action.get("path").and_then(Value::as_str),
            Some("crates/perl-parser/src/lib.rs")
        );
        assert_eq!(action.get("unresolved").and_then(Value::as_u64), Some(3));
        assert_eq!(
            action.pointer("/sample_seams/0/gap_id").and_then(Value::as_str),
            Some("RIPR-SPEC-0007")
        );
        assert_eq!(action.pointer("/sample_seams/0/line").and_then(Value::as_u64), Some(42));
        assert_eq!(
            action.pointer("/sample_seams/0/suggested_test").and_then(Value::as_str),
            Some("prove parser boundary branch")
        );
        let expected_verify = format!(
            "rtk cargo xtask ripr-plus --root . --receipt {} --check",
            display_path(&receipt_path)
        );
        let expected_receipt =
            format!("rtk cargo xtask ripr-plus --root . --receipt {}", display_path(&receipt_path));
        assert_eq!(action.get("verify").and_then(Value::as_str), Some(expected_verify.as_str()));
        assert_eq!(action.get("receipt").and_then(Value::as_str), Some(expected_receipt.as_str()));
        Ok(())
    }

    #[test]
    fn missing_actionable_sample_action_names_receipt_gap() -> TestResult {
        let dir = tempfile::tempdir()?;
        let receipt_path = dir.path().join("custom-ripr-plus.json");
        let row = DeferredCountRow {
            name: "crates/perl-parser/src/lib.rs".to_string(),
            count: 5,
            reason: "missing_actionable_sample".to_string(),
        };

        let action = ripr_missing_actionable_sample_action(&row, ".", &receipt_path);

        assert_eq!(
            action.get("kind").and_then(Value::as_str),
            Some("ripr_receipt_gap_guidance_missing")
        );
        assert_eq!(
            action.get("path").and_then(Value::as_str),
            Some("crates/perl-parser/src/lib.rs")
        );
        assert_eq!(action.get("unresolved").and_then(Value::as_u64), Some(5));
        assert_eq!(action.get("reason").and_then(Value::as_str), Some("missing_actionable_sample"));
        assert!(action
            .get("repair")
            .and_then(Value::as_str)
            .is_some_and(|repair| repair.contains("gap id") && repair.contains("positive line")));
        let expected_verify = format!(
            "rtk cargo xtask ripr-plus --root . --receipt {} --check",
            display_path(&receipt_path)
        );
        let expected_receipt =
            format!("rtk cargo xtask ripr-plus --root . --receipt {}", display_path(&receipt_path));
        assert_eq!(action.get("verify").and_then(Value::as_str), Some(expected_verify.as_str()));
        assert_eq!(action.get("receipt").and_then(Value::as_str), Some(expected_receipt.as_str()));
        Ok(())
    }

    #[test]
    fn proof_commands_quote_paths_with_spaces() -> TestResult {
        let dir = tempfile::tempdir()?;
        let spaced_dir = dir.path().join("quality receipts");
        let lcov = spaced_dir.join("lcov.info");
        let baseline = spaced_dir.join("coverage baseline.txt");
        let codecov = spaced_dir.join("codecov policy.yml");
        let receipt = spaced_dir.join("coverage baseline.json");

        let command = coverage_baseline_command(&lcov, &baseline, &codecov, &receipt, true);

        assert!(command.starts_with("rtk cargo xtask coverage-baseline"));
        assert!(command.contains(&format!("--lcov '{}'", display_path(&lcov))));
        assert!(command.contains(&format!("--baseline '{}'", display_path(&baseline))));
        assert!(command.contains(&format!("--codecov '{}'", display_path(&codecov))));
        assert!(command.contains(&format!("--receipt '{}'", display_path(&receipt))));
        assert!(command.ends_with(" --check"));

        let ripr = ripr_plus_command("workspace root", &spaced_dir.join("ripr plus.json"), false);

        assert!(ripr.starts_with("rtk cargo xtask ripr-plus"));
        assert!(ripr.contains("--root 'workspace root'"));
        assert!(ripr.contains("ripr plus.json'"));
        assert!(!ripr.contains("--check"));
        Ok(())
    }

    #[test]
    fn coverage_baseline_check_fails_when_receipt_is_missing_and_names_commands() -> TestResult {
        let dir = tempfile::tempdir()?;
        let lcov = dir.path().join("lcov.info");
        let baseline = dir.path().join("coverage-baseline.txt");
        let codecov = dir.path().join("codecov.yml");
        let receipt = dir.path().join("coverage-baseline.json");
        write_minimal_coverage_inputs(&lcov, &baseline, &codecov)?;

        let result = coverage_baseline(&lcov, &baseline, &codecov, &receipt, true);

        let error = result.err().ok_or("missing coverage receipt check should fail")?;
        let message = error.to_string();
        assert!(message.contains("missing coverage baseline receipt"), "{message}");
        assert_receipt_check_error_names_commands(
            &message,
            &coverage_baseline_command(&lcov, &baseline, &codecov, &receipt, false),
            &coverage_baseline_command(&lcov, &baseline, &codecov, &receipt, true),
            &receipt,
        );
        Ok(())
    }

    #[test]
    fn coverage_baseline_check_fails_when_receipt_is_stale_and_names_commands() -> TestResult {
        let dir = tempfile::tempdir()?;
        let lcov = dir.path().join("lcov.info");
        let baseline = dir.path().join("coverage-baseline.txt");
        let codecov = dir.path().join("codecov.yml");
        let receipt = dir.path().join("coverage-baseline.json");
        write_minimal_coverage_inputs(&lcov, &baseline, &codecov)?;
        coverage_baseline(&lcov, &baseline, &codecov, &receipt, false)?;
        fs::write(&receipt, "stale receipt\n")?;

        let result = coverage_baseline(&lcov, &baseline, &codecov, &receipt, true);

        let error = result.err().ok_or("stale coverage receipt check should fail")?;
        let message = error.to_string();
        assert!(message.contains("is stale"), "{message}");
        assert_receipt_check_error_names_commands(
            &message,
            &coverage_baseline_command(&lcov, &baseline, &codecov, &receipt, false),
            &coverage_baseline_command(&lcov, &baseline, &codecov, &receipt, true),
            &receipt,
        );
        Ok(())
    }

    #[test]
    fn ripr_plus_check_fails_when_receipt_is_missing_and_names_commands() -> TestResult {
        let dir = tempfile::tempdir()?;
        let receipt = dir.path().join("ripr-plus.json");
        let refresh = ripr_plus_command(".", &receipt, false);
        let verify = ripr_plus_command(".", &receipt, true);

        let result = write_or_check_receipt(
            &receipt,
            &json!({"schema_version": 1, "kind": "ripr_plus_baseline"}),
            true,
            "ripr+ receipt",
            &refresh,
            &verify,
        );

        let error = result.err().ok_or("missing ripr+ receipt check should fail")?;
        let message = error.to_string();
        assert!(message.contains("missing ripr+ receipt"), "{message}");
        assert_receipt_check_error_names_commands(&message, &refresh, &verify, &receipt);
        Ok(())
    }

    #[test]
    fn ripr_plus_check_fails_when_receipt_is_stale_and_names_commands() -> TestResult {
        let dir = tempfile::tempdir()?;
        let receipt = dir.path().join("ripr-plus.json");
        let refresh = ripr_plus_command(".", &receipt, false);
        let verify = ripr_plus_command(".", &receipt, true);
        write_or_check_receipt(
            &receipt,
            &json!({"schema_version": 1, "kind": "ripr_plus_baseline"}),
            false,
            "ripr+ receipt",
            &refresh,
            &verify,
        )?;
        fs::write(&receipt, "stale receipt\n")?;

        let result = write_or_check_receipt(
            &receipt,
            &json!({"schema_version": 1, "kind": "ripr_plus_baseline"}),
            true,
            "ripr+ receipt",
            &refresh,
            &verify,
        );

        let error = result.err().ok_or("stale ripr+ receipt check should fail")?;
        let message = error.to_string();
        assert!(message.contains("is stale"), "{message}");
        assert_receipt_check_error_names_commands(&message, &refresh, &verify, &receipt);
        Ok(())
    }

    #[test]
    fn key_value_parser_ignores_comments_and_blank_lines() -> TestResult {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("coverage-baseline.txt");
        fs::write(&path, "# comment\nschema_version=1\n\nbaseline_branch_coverage=50.00\n")?;

        let values = parse_key_value_file(&path)?;

        assert_eq!(values.get("schema_version"), Some(&"1".to_string()));
        assert_eq!(values.get("baseline_branch_coverage"), Some(&"50.00".to_string()));
        Ok(())
    }

    fn write_minimal_coverage_inputs(lcov: &Path, baseline: &Path, codecov: &Path) -> TestResult {
        fs::write(lcov, "SF:crates/perl-parser/src/lib.rs\nDA:1,1\nLF:1\nLH:1\nend_of_record\n")?;
        fs::write(baseline, "schema_version=1\nbaseline_branch_coverage=100.00\n")?;
        fs::write(
            codecov,
            concat!(
                "coverage:\n",
                "  status:\n",
                "    patch:\n",
                "      default:\n",
                "        target: 95%\n",
                "        threshold: 0%\n",
                "        if_ci_failed: error\n",
                "    project:\n",
                "      default:\n",
                "        target: 95%\n",
                "        threshold: 2%\n",
                "        informational: true\n",
                "comment:\n",
                "  layout: reach,diff,files\n",
                "  require_head: true\n",
            ),
        )?;
        Ok(())
    }

    fn assert_receipt_check_error_names_commands(
        message: &str,
        refresh: &str,
        verify: &str,
        receipt: &Path,
    ) {
        assert!(message.contains(&format!("refresh with `{refresh}`")), "{message}");
        assert!(message.contains(&format!("then verify with `{verify}`")), "{message}");
        assert!(message.contains(&format!("--receipt {}", display_path(receipt))), "{message}");
        assert!(message.contains(" --check`"), "{message}");
    }
}
