//! Portable RIPR PR evidence and routing tasks.
//!
//! README badges stay repo-scoped. These commands produce diff-scoped artifacts
//! under `target/` for PR review, annotations, and mutation routing.

use crate::tasks::change_set::{self, ArtifactIdentity};
use crate::tasks::git_context::{default_windows_drive_mount_root, git_output_with_mount_root};
use color_eyre::eyre::{Context, Result, bail, eyre};
use glob::Pattern;
use serde::de::{self, DeserializeSeed, Deserializer, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fmt;
use std::fs;
use std::io::{BufReader, ErrorKind, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use xtask::git_ancestry::{AncestryDisposition, AncestryReceipt, classify_ancestry};

#[cfg(test)]
static RIPR_BIN_OVERRIDE: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

const DEFAULT_ROOT: &str = ".";
const DEFAULT_BASE: &str = "origin/main";
const DEFAULT_HEAD: &str = "HEAD";
const PR_EVIDENCE_JSON: &str = "target/ripr/pr/repo-exposure.json";
const PR_EVIDENCE_MD: &str = "target/ripr/pr/repo-exposure.md";
const PR_DIFF: &str = "target/ripr/pr/pr.diff";
const PR_DIFF_RECEIPT: &str = "target/ripr/pr/committed-diff.json";
/// Raw `ripr check --format json` output, uploaded as a CI artifact for diagnostics (#1346).
/// The `repo-exposure.json` summary only contains per-bucket counts, not the `findings[]`
/// array.  Without `findings[]` it is impossible to diagnose suppression mismatches offline.
const PR_RAW_CHECK_JSON: &str = "target/ripr/pr/raw-check.json";
const REVIEW_COMMENTS_JSON: &str = "target/ripr/review/comments.json";
const REVIEW_COMMENTS_MD: &str = "target/ripr/review/comments.md";
const ANNOTATIONS_TXT: &str = "target/ripr/review/annotations.txt";
const PR_SUMMARY_MD: &str = "target/ripr/pr/summary.md";
const IMPACTED_JSON: &str = "target/xtask/impacted-evidence/latest.json";
const IMPACTED_MD: &str = "target/xtask/impacted-evidence/latest.md";
const DEFAULT_RIPR_SUPPRESSIONS: &str = "policy/ripr-suppressions.toml";

pub fn ripr_pr(
    root: &str,
    base: &str,
    head: &str,
    pr_head: Option<&str>,
    check: bool,
) -> Result<()> {
    let repo = repo_root()?;
    let options = PrEvidenceOptions {
        root: normalized_option(root, DEFAULT_ROOT),
        base: normalized_option(base, DEFAULT_BASE),
        head: normalized_option(head, DEFAULT_HEAD),
        pr_head_sha: normalized_optional(pr_head),
    };
    if check { check_pr_evidence(&repo, &options) } else { write_pr_evidence(&repo, &options) }
}

pub fn ripr_plus(root: &str, receipt: &Path, suppressions: &Path, check: bool) -> Result<()> {
    let repo = repo_root()?;
    let options = RiprPlusOptions {
        root: normalized_option(root, DEFAULT_ROOT),
        suppressions: suppressions.to_path_buf(),
    };
    let packet = ripr_plus_packet(&repo, &options)?;
    let rendered = format_json(&packet)?;
    let receipt_path = repo.join(receipt);
    if check {
        let actual = fs::read_to_string(&receipt_path)
            .with_context(|| format!("missing or unreadable {}", receipt_path.display()))?;
        if actual != rendered {
            bail!(
                "{} is stale; run `cargo xtask ripr-plus --root {} --receipt {}`",
                receipt_path.display(),
                options.root,
                receipt.display()
            );
        }
        println!("RIPR+ receipt is current: {}", receipt_path.display());
    } else {
        write_text(&receipt_path, &rendered)?;
        println!("Wrote {}", receipt_path.display());
    }
    Ok(())
}

pub fn ripr_review_comments(
    root: &str,
    base: &str,
    head: &str,
    pr_head: Option<&str>,
    timeout_seconds: Option<u64>,
    check: bool,
) -> Result<()> {
    let repo = repo_root()?;
    let options = ReviewCommentsOptions {
        root: normalized_option(root, DEFAULT_ROOT),
        base: normalized_option(base, DEFAULT_BASE),
        head: normalized_option(head, DEFAULT_HEAD),
        pr_head_sha: normalized_optional(pr_head),
        timeout_seconds: timeout_seconds.filter(|seconds| *seconds > 0),
    };
    if check {
        check_review_comments(&repo, &options)
    } else {
        write_review_comments(&repo, &options)
    }
}

pub fn ripr_pr_summary(check: bool) -> Result<()> {
    let repo = repo_root()?;
    let summary = render_pr_evidence_summary(&repo);
    let path = repo.join(PR_SUMMARY_MD);
    if check {
        let actual = fs::read_to_string(&path)
            .with_context(|| format!("missing or unreadable {PR_SUMMARY_MD}"))?;
        if actual != summary {
            bail!("{PR_SUMMARY_MD} is stale; run `cargo xtask ripr-pr-summary`");
        }
        println!("PR evidence summary is current.");
    } else {
        write_text(&path, &summary)?;
        println!("Wrote {PR_SUMMARY_MD}");
    }
    Ok(())
}

pub fn ripr_annotations(comments: &str, out: &str, check: bool) -> Result<()> {
    let repo = repo_root()?;
    let comments = normalized_option(comments, REVIEW_COMMENTS_JSON);
    let out = normalized_option(out, ANNOTATIONS_TXT);
    let generated = render_annotations(&repo, &comments)?;
    let out_path = repo.join(&out);

    if check {
        if generated.comments_missing && !out_path.exists() {
            println!("RIPR annotations skipped: {comments} is missing.");
            return Ok(());
        }
        let actual = fs::read_to_string(&out_path)
            .with_context(|| format!("missing or unreadable {out}"))?;
        if actual != generated.text {
            bail!("{out} is stale; run `cargo xtask ripr-annotations`");
        }
        println!("RIPR annotations are current.");
    } else {
        write_text(&out_path, &generated.text)?;
        if generated.comments_missing {
            println!("RIPR annotations skipped: {comments} is missing.");
        } else if generated.text.is_empty() {
            println!("RIPR annotations: no comments[] guidance to emit.");
        } else {
            print!("{}", generated.text);
        }
        println!("Wrote {out}");
    }
    Ok(())
}

pub fn impacted_evidence(
    pr_evidence: &str,
    labels: &[String],
    labels_csv: Option<&str>,
    check: bool,
) -> Result<()> {
    let repo = repo_root()?;
    let options = ImpactedEvidenceOptions {
        pr_evidence: normalized_option(pr_evidence, PR_EVIDENCE_JSON),
        labels: merged_labels(labels, labels_csv),
    };
    let packet = impacted_evidence_packet(&repo, &options);
    let json_text = format_json(&packet)?;
    let markdown = render_impacted_evidence_markdown(&packet);

    if check {
        let actual_json = fs::read_to_string(repo.join(IMPACTED_JSON))
            .with_context(|| format!("missing or unreadable {IMPACTED_JSON}"))?;
        let actual_md = fs::read_to_string(repo.join(IMPACTED_MD))
            .with_context(|| format!("missing or unreadable {IMPACTED_MD}"))?;
        if actual_json != json_text || actual_md != markdown {
            bail!("impacted evidence is stale; run `cargo xtask impacted-evidence`");
        }
        println!("Impacted evidence is current.");
    } else {
        write_text(&repo.join(IMPACTED_JSON), &json_text)?;
        write_text(&repo.join(IMPACTED_MD), &markdown)?;
        println!("Wrote {IMPACTED_JSON}");
        println!("Wrote {IMPACTED_MD}");
    }
    Ok(())
}

fn normalized_option(value: &str, default: &str) -> String {
    if value.trim().is_empty() { default.to_string() } else { value.to_string() }
}

fn normalized_optional(value: Option<&str>) -> Option<String> {
    value.map(str::trim).filter(|value| !value.is_empty()).map(str::to_owned)
}

fn optional_sha_value(value: Option<&str>) -> Value {
    value.map_or(Value::Null, |sha| json!(sha))
}

fn repo_root() -> Result<PathBuf> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| eyre!("failed to resolve repository root from {}", manifest_dir.display()))
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PrEvidenceOptions {
    root: String,
    base: String,
    head: String,
    pr_head_sha: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RiprPlusOptions {
    root: String,
    suppressions: PathBuf,
}

fn ripr_plus_packet(repo: &Path, options: &RiprPlusOptions) -> Result<Value> {
    let root = command_root_arg(repo, &options.root)?;
    // Fetch repo-badge-json for the canonical actionable-gap counts (authoritative headline).
    let badge_raw = run_ripr(&[
        "check".to_string(),
        "--root".to_string(),
        root.clone(),
        "--format".to_string(),
        "repo-badge-json".to_string(),
    ])?;
    // Fetch repo-seams-json for the triage inventory (top_files / top_gap_kinds / clusters).
    let seams_raw = run_ripr(&[
        "check".to_string(),
        "--root".to_string(),
        root,
        "--format".to_string(),
        "repo-seams-json".to_string(),
    ])?;
    let suppressions = read_ripr_suppression_rules(repo, &options.suppressions)?;
    ripr_plus_packet_from_raw(options, &current_head(repo)?, &suppressions, &badge_raw, &seams_raw)
}

/// Parse both `repo-badge-json` and `repo-seams-json` output and build the
/// RIPR+ baseline receipt. Kept separate from the git/ripr I/O above so the
/// parsing and canonical-gap accounting are exercised by unit tests.
fn ripr_plus_packet_from_raw(
    options: &RiprPlusOptions,
    head: &str,
    suppressions: &RiprSuppressionRules,
    badge_raw: &str,
    seams_raw: &str,
) -> Result<Value> {
    let badge: Value =
        serde_json::from_str(badge_raw).context("ripr repo-badge-json was invalid JSON")?;
    let seams_value: Value =
        serde_json::from_str(seams_raw).context("ripr repo-seams-json was invalid JSON")?;
    let seams = seams_value
        .get("seams")
        .and_then(Value::as_array)
        .ok_or_else(|| eyre!("ripr repo-seams-json output did not include seams[]"))?;
    let seam_summary = ripr_plus_seam_summary(seams, suppressions, 10);
    Ok(ripr_plus_receipt_packet(options, head, suppressions, &badge, seam_summary))
}

fn ripr_plus_receipt_packet(
    options: &RiprPlusOptions,
    head: &str,
    suppressions: &RiprSuppressionRules,
    badge: &Value,
    seam_summary: RiprPlusSeamSummary,
) -> Value {
    // Canonical counts: authoritative headline from repo-badge-json.
    let counts = badge.get("counts");
    let count = |key: &str| counts.and_then(|v| v.get(key)).and_then(Value::as_u64).unwrap_or(0);
    let active_unresolved =
        count("unsuppressed_exposure_gaps") + count("unsuppressed_test_efficiency_findings");
    let suppressed_unresolved =
        count("suppressed_exposure_gaps") + count("suppressed_test_efficiency_findings");
    let basis = badge
        .get("basis")
        .and_then(Value::as_str)
        .unwrap_or("canonical_actionable_gap")
        .to_string();

    // Triage inventory: from the seam summary (repo-seams-json).
    let top_active_files = seam_summary.top_files;
    let top_suppressed_files = seam_summary.top_suppressed_files;
    let top_active_gap_kinds = seam_summary.top_gap_kinds;
    let top_suppressed_gap_kinds = seam_summary.top_suppressed_gap_kinds;

    json!({
        "schema_version": 2,
        "kind": "ripr_plus_baseline",
        "mode": "advisory",
        "head": head,
        "root": options.root,
        "source_format": "ripr check --format repo-badge-json (counts) + repo-seams-json (triage inventory)",
        "basis": basis,
        "unresolved": active_unresolved,
        "active_unresolved": active_unresolved,
        "suppressed_unresolved": suppressed_unresolved,
        "new_unresolved": null,
        "counts": counts.cloned().unwrap_or_else(|| json!({})),
        "reason_counts": badge.get("reason_counts").cloned().unwrap_or_else(|| json!({})),
        "top_files": top_active_files.clone(),
        "top_active_files": top_active_files,
        "top_suppressed_files": top_suppressed_files,
        "top_gap_kinds": top_active_gap_kinds.clone(),
        "top_active_gap_kinds": top_active_gap_kinds,
        "top_suppressed_gap_kinds": top_suppressed_gap_kinds,
        "recommended_first_clusters": seam_summary.recommended_first_clusters,
        "suppression_rule_count": suppressions.display_patterns.len(),
        "suppressions": {
            "path": display_path(&options.suppressions),
            "path_patterns": suppressions.display_patterns.clone(),
            "invalid_patterns": suppressions.invalid_patterns.clone(),
            "reasons": suppressions.suppression_reasons.clone(),
        },
        "decision": "advisory",
        "claim_boundary": [
            "Measurement only; this receipt does not enforce ripr+ zero.",
            "unresolved is the canonical_actionable_gap count from RIPR repo-badge-json: unsuppressed exposure gaps plus actionable test-efficiency findings.",
            "active_unresolved equals unresolved; suppressed_unresolved is suppressed_exposure_gaps + suppressed_test_efficiency_findings from repo-badge-json.",
            "top_files, top_gap_kinds, and recommended_first_clusters are triage aids derived from the seam inventory (repo-seams-json), not the headline count.",
            "new_unresolved is null until PR diff comparison is wired in the quality gate."
        ]
    })
}

#[derive(Debug)]
struct RiprPlusSeamSummary {
    /// Raw seam count of active (unsuppressed) seams in the inventory.
    /// Not used for the headline `unresolved` count in the receipt — that
    /// comes from `repo-badge-json` (canonical actionable gap basis). Kept
    /// here so that `ripr_plus_seam_summary` tests can assert the seam split.
    #[allow(dead_code)]
    unresolved: usize,
    /// Raw seam count of suppressed seams in the inventory. Same note as above.
    #[allow(dead_code)]
    suppressed: usize,
    top_files: Vec<Value>,
    top_suppressed_files: Vec<Value>,
    top_gap_kinds: Vec<Value>,
    top_suppressed_gap_kinds: Vec<Value>,
    recommended_first_clusters: Vec<Value>,
}

fn ripr_plus_seam_summary(
    seams: &[Value],
    suppressions: &RiprSuppressionRules,
    limit: usize,
) -> RiprPlusSeamSummary {
    let active = seams
        .iter()
        .filter(|seam| !suppression_matches_seam(suppressions, seam))
        .collect::<Vec<_>>();
    let suppressed = seams
        .iter()
        .filter(|seam| suppression_matches_seam(suppressions, seam))
        .collect::<Vec<_>>();

    let top_files = ripr_plus_top_files(active.iter().copied(), limit);
    let top_gap_kinds = ripr_plus_top_gap_kinds(active.iter().copied(), limit);
    let recommended_first_clusters =
        ripr_plus_recommended_first_clusters_from_seams(active.iter().copied(), limit);

    RiprPlusSeamSummary {
        unresolved: active.len(),
        suppressed: suppressed.len(),
        top_files,
        top_suppressed_files: ripr_plus_top_files(suppressed.iter().copied(), limit),
        top_gap_kinds,
        top_suppressed_gap_kinds: ripr_plus_top_gap_kinds(suppressed.iter().copied(), limit),
        recommended_first_clusters,
    }
}

fn ripr_plus_top_files<'a>(seams: impl IntoIterator<Item = &'a Value>, limit: usize) -> Vec<Value> {
    ripr_plus_count_rows(seams.into_iter().filter_map(ripr_plus_seam_path), limit)
}

fn ripr_plus_top_gap_kinds<'a>(
    seams: impl IntoIterator<Item = &'a Value>,
    limit: usize,
) -> Vec<Value> {
    ripr_plus_count_rows(seams.into_iter().filter_map(ripr_plus_seam_gap_kind), limit)
}

fn ripr_plus_count_rows(values: impl IntoIterator<Item = String>, limit: usize) -> Vec<Value> {
    let mut counts = BTreeMap::<String, u64>::new();
    for value in values {
        *counts.entry(value).or_default() += 1;
    }
    let mut rows = counts.into_iter().collect::<Vec<_>>();
    rows.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    rows.truncate(limit);
    rows.into_iter().map(|(name, count)| json!({ "name": name, "count": count })).collect()
}

fn ripr_plus_recommended_first_clusters_from_seams<'a>(
    seams: impl IntoIterator<Item = &'a Value>,
    limit: usize,
) -> Vec<Value> {
    let seams = seams.into_iter().collect::<Vec<_>>();
    let file_rows = ripr_plus_top_files(seams.iter().copied(), usize::MAX);
    let gap_kind_rows = ripr_plus_top_gap_kinds(seams.iter().copied(), usize::MAX);
    ripr_plus_recommended_first_clusters(&file_rows, &gap_kind_rows, limit)
}

fn ripr_plus_seam_path(seam: &Value) -> Option<String> {
    let direct = ["path", "file"].into_iter().find_map(|key| seam.get(key).and_then(Value::as_str));
    let nested = seam
        .get("location")
        .and_then(|value| value.get("path"))
        .and_then(Value::as_str)
        .or_else(|| {
            seam.get("placement").and_then(|value| value.get("path")).and_then(Value::as_str)
        })
        .or_else(|| {
            seam.get("evidence_record").and_then(|value| value.get("path")).and_then(Value::as_str)
        });
    direct.or(nested).map(normalize_path_text).filter(|path| !path.trim().is_empty())
}

fn ripr_plus_seam_gap_kind(seam: &Value) -> Option<String> {
    let direct = ["gap_kind", "seam_kind", "kind", "classification", "category", "reason"]
        .into_iter()
        .find_map(|key| ripr_plus_text_value(seam.get(key)));
    let nested = ["location", "placement", "evidence_record", "evidence"].into_iter().find_map(
        |object_key| {
            let object = seam.get(object_key)?;
            ["gap_kind", "seam_kind", "kind", "classification", "category", "reason"]
                .into_iter()
                .find_map(|key| ripr_plus_text_value(object.get(key)))
        },
    );
    direct.or(nested).map(normalize_inventory_label).filter(|kind| !kind.is_empty())
}

fn ripr_plus_text_value(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(value) => Some(value.clone()),
        Value::Array(values) => {
            let parts = values
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>();
            if parts.is_empty() { None } else { Some(parts.join(",")) }
        }
        _ => None,
    }
}

fn normalize_inventory_label(value: String) -> String {
    value.trim().replace('\\', "/").to_ascii_lowercase()
}

fn ripr_plus_recommended_first_clusters(
    top_files: &[Value],
    top_gap_kinds: &[Value],
    limit: usize,
) -> Vec<Value> {
    let mut clusters = BTreeMap::<String, RiprPlusClusterRecommendation>::new();
    for file in top_files {
        let Some(path) = file.get("name").and_then(Value::as_str) else {
            continue;
        };
        let count = file.get("count").and_then(Value::as_u64).unwrap_or(0);
        let (name, reason) = ripr_plus_cluster_for_path(path);
        clusters
            .entry(name.to_string())
            .or_insert_with(|| RiprPlusClusterRecommendation::new(name, reason))
            .push_file(path, count);
    }
    for gap_kind in top_gap_kinds {
        let Some(kind) = gap_kind.get("name").and_then(Value::as_str) else {
            continue;
        };
        let count = gap_kind.get("count").and_then(Value::as_u64).unwrap_or(0);
        let (name, reason) = ripr_plus_cluster_for_gap_kind(kind);
        clusters
            .entry(name.to_string())
            .or_insert_with(|| RiprPlusClusterRecommendation::new(name, reason))
            .push_gap_kind(kind, count);
    }

    let mut rows = clusters.into_values().collect::<Vec<_>>();
    rows.sort_by(|left, right| {
        right.score.cmp(&left.score).then_with(|| left.name.cmp(&right.name))
    });
    rows.truncate(limit);
    rows.into_iter().map(RiprPlusClusterRecommendation::into_json).collect()
}

fn ripr_plus_cluster_for_path(path: &str) -> (&'static str, &'static str) {
    let normalized = normalize_path_text(path);
    if normalized.starts_with("xtask/")
        || normalized.starts_with(".github/")
        || normalized.starts_with("policy/")
        || normalized.starts_with("scripts/")
        || normalized.starts_with("docs/ci/")
    {
        (
            "proof-infrastructure",
            "Proof tooling, policy, workflow, and report surfaces are owned by this lane.",
        )
    } else if normalized.contains("receipt")
        || normalized.contains("quality")
        || normalized.contains("ripr")
        || normalized.contains("coverage")
        || normalized.contains("report")
        || normalized.contains("summary")
    {
        (
            "ci-report-formatting",
            "Receipt and report formatting gaps should become agent repair packets.",
        )
    } else if normalized.contains("config") {
        ("config-parsing", "Configuration paths should be covered with focused parse cases.")
    } else if normalized.contains("error") || normalized.contains("diagnostic") {
        ("error-variants", "Failure variants should be covered with behavior assertions.")
    } else {
        (
            "active-ripr-inventory",
            "Use the top active files and gap kinds to split a focused burn-down PR.",
        )
    }
}

fn ripr_plus_cluster_for_gap_kind(kind: &str) -> (&'static str, &'static str) {
    if kind.contains("receipt") || kind.contains("report") || kind.contains("summary") {
        (
            "ci-report-formatting",
            "Receipt and report formatting gaps should become agent repair packets.",
        )
    } else if kind.contains("config") {
        ("config-parsing", "Configuration paths should be covered with focused parse cases.")
    } else if kind.contains("error") || kind.contains("failure") {
        ("error-variants", "Failure variants should be covered with behavior assertions.")
    } else if kind.contains("boundary") || kind.contains("predicate") {
        ("boundary-predicates", "Boundary branches should be covered with below/equal/above cases.")
    } else {
        (
            "active-ripr-inventory",
            "Use the top active files and gap kinds to split a focused burn-down PR.",
        )
    }
}

#[derive(Debug)]
struct RiprPlusClusterRecommendation {
    name: String,
    reason: String,
    score: u64,
    active_file_count: u64,
    gap_kind_count: u64,
    example_files: BTreeSet<String>,
    example_gap_kinds: BTreeSet<String>,
}

impl RiprPlusClusterRecommendation {
    fn new(name: &str, reason: &str) -> Self {
        Self {
            name: name.to_string(),
            reason: reason.to_string(),
            score: 0,
            active_file_count: 0,
            gap_kind_count: 0,
            example_files: BTreeSet::new(),
            example_gap_kinds: BTreeSet::new(),
        }
    }

    fn push_file(&mut self, path: &str, count: u64) {
        self.score += count;
        self.active_file_count += count;
        if self.example_files.len() < 3 {
            self.example_files.insert(path.to_string());
        }
    }

    fn push_gap_kind(&mut self, kind: &str, count: u64) {
        self.score += count;
        self.gap_kind_count += count;
        if self.example_gap_kinds.len() < 3 {
            self.example_gap_kinds.insert(kind.to_string());
        }
    }

    fn into_json(self) -> Value {
        json!({
            "name": self.name,
            "score": self.score,
            "active_file_count": self.active_file_count,
            "gap_kind_count": self.gap_kind_count,
            "reason": self.reason,
            "example_files": self.example_files.into_iter().collect::<Vec<_>>(),
            "example_gap_kinds": self.example_gap_kinds.into_iter().collect::<Vec<_>>(),
        })
    }
}

#[derive(Debug, Default, Deserialize)]
struct RiprSuppressionPolicy {
    #[serde(default, rename = "suppress")]
    suppressions: Vec<RiprSuppression>,
}

#[derive(Debug, Default, Deserialize)]
struct RiprSuppression {
    #[serde(default)]
    id: String,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    paths: Vec<String>,
    #[serde(default)]
    classification: Vec<String>,
    #[serde(default)]
    reason: String,
}

#[derive(Debug, Default)]
struct RiprSuppressionRules {
    display_patterns: Vec<String>,
    path_patterns: Vec<Pattern>,
    classification_patterns: Vec<Vec<String>>,
    invalid_patterns: Vec<String>,
    suppression_reasons: Vec<Value>,
}

fn read_ripr_suppression_rules(repo: &Path, path: &Path) -> Result<RiprSuppressionRules> {
    let policy_path = if path.is_absolute() { path.to_path_buf() } else { repo.join(path) };
    let raw = fs::read_to_string(&policy_path)
        .with_context(|| format!("reading RIPR suppressions {}", policy_path.display()))?;
    let policy: RiprSuppressionPolicy = toml::from_str(&raw)
        .with_context(|| format!("parsing RIPR suppressions {}", policy_path.display()))?;

    let mut rules = RiprSuppressionRules::default();
    for suppression in policy.suppressions {
        let paths =
            suppression.paths.iter().map(|path| normalize_path_text(path)).collect::<Vec<_>>();
        if !suppression.id.trim().is_empty()
            || !suppression.kind.trim().is_empty()
            || !suppression.reason.trim().is_empty()
        {
            rules.suppression_reasons.push(json!({
                "id": suppression.id,
                "kind": suppression.kind,
                "reason": suppression.reason,
                "paths": paths.clone(),
            }));
        }
        for path_pattern in paths {
            match Pattern::new(&path_pattern) {
                Ok(pattern) => {
                    rules.display_patterns.push(path_pattern);
                    rules.path_patterns.push(pattern);
                    rules.classification_patterns.push(suppression.classification.clone());
                }
                Err(_) => rules.invalid_patterns.push(path_pattern),
            }
        }
    }
    if !rules.invalid_patterns.is_empty() {
        bail!("invalid RIPR suppression path pattern(s): {}", rules.invalid_patterns.join(", "));
    }
    Ok(rules)
}

fn suppression_matches_seam(rules: &RiprSuppressionRules, seam: &Value) -> bool {
    let Some(path) = ripr_plus_seam_path(seam) else {
        return false;
    };
    let path = normalize_suppression_match_path(&path);
    rules.path_patterns.iter().any(|pattern| pattern.matches(&path))
}

fn current_head(repo: &Path) -> Result<String> {
    revision_sha(repo, "HEAD")
}

fn revision_sha(repo: &Path, revision: &str) -> Result<String> {
    Ok(run_git_output(repo, &["rev-parse", revision])?.trim().to_string())
}

fn write_pr_evidence(repo: &Path, options: &PrEvidenceOptions) -> Result<()> {
    verify_revision(repo, &options.base)?;
    verify_revision(repo, &options.head)?;
    if let Some(pr_head_sha) = &options.pr_head_sha {
        verify_revision(repo, pr_head_sha)?;
    }
    let base_sha = revision_sha(repo, &options.base)?;
    let head_sha = revision_sha(repo, &options.head)?;
    let diff_receipt = resolve_committed_diff(repo, &options.base, &options.head)?;
    let changed_file_count = diff_receipt.entries.len();
    write_pr_diff(repo, &diff_receipt)?;
    // Stream raw check output for offline diagnostics (#1346) straight to its
    // artifact path: repo-exposure.json only contains per-bucket counts; the
    // findings[] array (which carries per-finding classification and path) is
    // required to diagnose suppression mismatches.  This file is included in the
    // ripr-pr-evidence artifact upload so it is available without re-running ripr.
    run_ripr_check(repo, options)?;
    let suppressions = read_ripr_suppression_rules(repo, Path::new(DEFAULT_RIPR_SUPPRESSIONS))?;
    let head_extents = HeadLineExtents::from_committed_diff(repo, &diff_receipt);
    // Attribution basis for the new-gap count (#11690): findings must sit in a
    // changed workspace package or one of its real dependents. The production
    // surface (#12267 review repair) decides compiled-into-production status
    // for the structural non-production filter. Both derive from the same
    // cargo metadata run; a metadata failure keeps every finding counted —
    // the gate never under-attributes on an unreadable graph.
    let metadata = crate::tasks::ci_scope::load_metadata(repo).ok();
    let changed_paths = committed_diff_entry_paths(&diff_receipt.entries);
    let attribution_scope = metadata
        .as_ref()
        .and_then(|metadata| dependency_attribution_scope(metadata, &changed_paths).ok());
    let production_surface = metadata
        .as_ref()
        .and_then(|metadata| production_surface_from_metadata(repo, metadata).ok());
    let attribution = attribution_scope.as_ref().and_then(AttributionScope::applied);
    // The findings payload is unbounded (#12860: 2.1GB observed) — ingest it by
    // streaming one finding at a time into the summary buckets instead of
    // buffering the whole String plus a full serde_json DOM.
    let ingestion = ripr_check_ingestion_from_file(
        &repo.join(PR_RAW_CHECK_JSON),
        &suppressions,
        Some(&head_extents),
        attribution,
        production_surface.as_ref(),
    )?;
    let packet = pr_evidence_packet_from_summary(
        options,
        &ingestion,
        &base_sha,
        &head_sha,
        &suppressions,
        PrEvidenceContext {
            changed_file_count,
            attribution_scope: attribution_scope.as_ref(),
            production_surface: production_surface.as_ref(),
        },
    );
    validate_pr_evidence_packet(&packet, options, changed_file_count, true, &base_sha, &head_sha)?;
    write_text(&repo.join(PR_EVIDENCE_JSON), &format_json(&packet)?)?;
    write_text(&repo.join(PR_EVIDENCE_MD), &render_pr_evidence_markdown(&packet))?;
    println!("Wrote {PR_RAW_CHECK_JSON}");
    println!("Wrote {PR_EVIDENCE_JSON}");
    println!("Wrote {PR_EVIDENCE_MD}");
    Ok(())
}

fn check_pr_evidence(repo: &Path, options: &PrEvidenceOptions) -> Result<()> {
    verify_revision(repo, &options.base)?;
    verify_revision(repo, &options.head)?;
    if let Some(pr_head_sha) = &options.pr_head_sha {
        verify_revision(repo, pr_head_sha)?;
    }
    let base_sha = revision_sha(repo, &options.base)?;
    let head_sha = revision_sha(repo, &options.head)?;
    let diff_receipt = resolve_committed_diff(repo, &options.base, &options.head)?;
    let changed_file_count = diff_receipt.entries.len();
    let committed_diff_text = fs::read_to_string(repo.join(PR_DIFF_RECEIPT))
        .with_context(|| format!("missing or unreadable {PR_DIFF_RECEIPT}"))?;
    let committed_diff: CommittedDiffReceipt = serde_json::from_str(&committed_diff_text)
        .with_context(|| format!("{PR_DIFF_RECEIPT} is invalid"))?;
    if committed_diff != diff_receipt {
        bail!("{PR_DIFF_RECEIPT} is stale for the requested base/head range");
    }
    let text = fs::read_to_string(repo.join(PR_EVIDENCE_JSON))
        .with_context(|| format!("missing or unreadable {PR_EVIDENCE_JSON}"))?;
    let packet: Value =
        serde_json::from_str(&text).with_context(|| format!("{PR_EVIDENCE_JSON} is invalid"))?;
    validate_pr_evidence_packet(
        &packet,
        options,
        changed_file_count,
        repo.join(PR_EVIDENCE_MD).exists(),
        &base_sha,
        &head_sha,
    )?;
    println!("PR evidence contract ok: {PR_EVIDENCE_JSON}");
    Ok(())
}

/// Runs `ripr check --format json`, streaming its stdout straight into
/// [`PR_RAW_CHECK_JSON`] without buffering the payload in memory.
///
/// RIPR check output is unbounded — a 2.1GB payload killed a 16GB CI runner
/// (#12860) — so this transport never holds the whole document: the child
/// writes to the same regular temporary file [`run_output`] uses to keep large
/// stdout writes off a Windows pipe (#12569), then atomically publishes that
/// file by rename once the child succeeds.
fn run_ripr_check(repo: &Path, options: &PrEvidenceOptions) -> Result<()> {
    let diff = repo.join(PR_DIFF).display().to_string();
    let root = command_root_arg(repo, &options.root)?;
    run_ripr_streaming_to_file(
        &[
            "check".to_string(),
            "--root".to_string(),
            root,
            "--diff".to_string(),
            diff,
            "--format".to_string(),
            "json".to_string(),
        ],
        &repo.join(PR_RAW_CHECK_JSON),
    )
}

/// Runs RIPR, streaming stdout into a same-directory temporary file and
/// atomically publishing it at `out_path` after success. Stderr is captured in
/// full (diagnostics are small); the stdout excerpt in a failure message is
/// bounded because the payload itself is unbounded. Only one complete copy of
/// the unbounded payload exists on disk, and any failure before the rename
/// drops the temporary file without exposing a partial artifact at `out_path`.
fn run_ripr_streaming_to_file(args: &[String], out_path: &Path) -> Result<()> {
    let binary = ripr_binary()?;
    // A failed rerun must not leave an older raw artifact available to the
    // review-comments fallback. The artifact is published only after the child
    // succeeds and its complete stdout has been written.
    match fs::remove_file(out_path) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to remove stale {}", out_path.display()));
        }
    }
    let parent = match out_path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;
    let stdout_file =
        tempfile::NamedTempFile::new_in(parent).context("failed to create RIPR stdout file")?;
    let mut child = Command::new(&binary)
        .args(args)
        .stdout(Stdio::from(stdout_file.reopen().context("failed to reopen RIPR stdout file")?))
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to run {binary}"))?;
    let mut stderr_bytes = Vec::new();
    if let Some(mut stderr) = child.stderr.take() {
        stderr
            .read_to_end(&mut stderr_bytes)
            .with_context(|| format!("failed to read {binary} stderr"))?;
    }
    let status = child.wait().with_context(|| format!("failed to wait for {binary}"))?;
    if !status.success() {
        let mut stdout_excerpt = Vec::new();
        if let Ok(stdout_reader) = stdout_file.reopen() {
            let _ = stdout_reader.take(4096).read_to_end(&mut stdout_excerpt);
        }
        bail!(
            "{binary} failed with status {}\nstdout:\n{}\nstderr:\n{}",
            status,
            String::from_utf8_lossy(&stdout_excerpt).trim(),
            String::from_utf8_lossy(&stderr_bytes).trim()
        );
    }
    stdout_file
        .persist(out_path)
        .map_err(|error| error.error)
        .with_context(|| format!("failed to publish {binary} stdout to {}", out_path.display()))?;
    Ok(())
}

/// Streamed ingestion of one `ripr check --format json` payload (#12860): the
/// summary map plus the aggregate per-finding buckets. The findings array is
/// never materialized — it is the unbounded surface (2.1GB observed) that killed
/// evidence runners when the whole payload was buffered into a String and
/// parsed into a full serde_json DOM. Each finding is deserialized into one
/// `serde_json::Value` at a time and dropped before the next, so peak ingestion
/// memory is bounded by the largest single finding rather than the payload.
/// A payload whose memory is concentrated in one huge finding is not bounded by
/// this change; the observed #12860 shape is many findings carrying unconsumed
/// blobs, which this does bound.
#[derive(Debug)]
struct RiprCheckIngestion {
    summary_counts: RiprPrSummaryCounts,
    /// Mirrors `check_value.get("summary").and_then(Value::as_object).is_some()`:
    /// a `summary` key that is not an object still counts as absent.
    check_summary_present: bool,
}

/// Streams the raw payload at `raw_check_path` once, aggregating per-finding
/// buckets through [`RiprFindingBuckets::absorb`] as each finding is parsed.
/// Only the (small) summary object is retained.
fn ripr_check_ingestion_from_file(
    raw_check_path: &Path,
    suppressions: &RiprSuppressionRules,
    head_extents: Option<&HeadLineExtents>,
    attribution: Option<&DependencyAttribution>,
    production_surface: Option<&ProductionSurface>,
) -> Result<RiprCheckIngestion> {
    let raw_check = fs::File::open(raw_check_path)
        .with_context(|| format!("reading {}", raw_check_path.display()))?;
    let reader = BufReader::with_capacity(64 * 1024, raw_check);
    let mut buckets = RiprFindingBuckets::default();
    let payload = stream_ripr_check_payload_with_events(reader, &mut |event| match event {
        StreamFindingsEvent::Start => buckets = RiprFindingBuckets::default(),
        StreamFindingsEvent::Finding(finding) => {
            buckets.absorb(finding, suppressions, head_extents, attribution, production_surface)
        }
    })
    .context("ripr check output was not valid JSON")?;
    let check_summary = payload.summary.as_ref().and_then(Value::as_object);
    Ok(RiprCheckIngestion {
        summary_counts: ripr_summary_counts_merge(
            ripr_summary_counts_seed(check_summary),
            buckets,
            check_summary.is_some(),
        ),
        check_summary_present: check_summary.is_some(),
    })
}

/// Streams one JSON document from `reader`, invoking `on_finding` for every
/// element of the top-level `findings` array as it is parsed. The findings
/// array is never materialized: each element becomes one
/// `serde_json::Value` for the callback and is dropped before the next. Any
/// other top-level shape (arrays, scalars) is still validated but carries no
/// summary and no findings, which is what the previous DOM path saw through
/// `Value::get`. A single huge finding can still dominate memory; this bounds
/// the many-finding, unconsumed-blob shape observed in #12860.
///
/// Findings-only convenience wrapper over
/// [`stream_ripr_check_payload_with_events`]. Production ingestion needs the
/// array-boundary events for duplicate-key reset, so it drives the event API
/// directly and this wrapper stays test-only.
#[cfg(test)]
fn stream_ripr_check_payload<R, F>(
    reader: R,
    on_finding: &mut F,
) -> serde_json::Result<Option<Value>>
where
    R: Read,
    F: FnMut(&Value),
{
    stream_ripr_check_payload_with_events(reader, &mut |event| {
        if let StreamFindingsEvent::Finding(finding) = event {
            on_finding(finding);
        }
    })
    .map(|payload| payload.summary)
}

/// Events emitted while streaming a top-level `findings` value.
enum StreamFindingsEvent<'a> {
    Start,
    Finding(&'a Value),
}

/// Streaming payload parser with an event for top-level duplicate-key
/// semantics. serde_json's retained map representation is last-key-wins.
/// Emitting `Start` before every `findings` value lets a streaming sink discard
/// the prior value before consuming the replacement, including when the
/// replacement is not an array.
fn stream_ripr_check_payload_with_events<R, F>(
    reader: R,
    on_event: &mut F,
) -> serde_json::Result<RiprCheckPayload>
where
    R: Read,
    F: for<'a> FnMut(StreamFindingsEvent<'a>),
{
    let mut deserializer = serde_json::Deserializer::from_reader(reader);
    let payload = deserializer.deserialize_any(RiprCheckPayloadVisitor { on_event })?;
    deserializer.end()?;
    Ok(payload)
}

/// What a streaming parse of a `ripr check` payload retains.
#[derive(Default)]
struct RiprCheckPayload {
    summary: Option<Value>,
    base: Option<Value>,
}

/// Hand-driven map visitor so `findings` elements are consumed one at a time;
/// a typed `findings: Vec<...>` field would rebuild the unbounded array this
/// ingestion exists to avoid.
struct RiprCheckPayloadVisitor<'a, F> {
    on_event: &'a mut F,
}

impl<'de, F> Visitor<'de> for RiprCheckPayloadVisitor<'_, F>
where
    F: for<'a> FnMut(StreamFindingsEvent<'a>),
{
    type Value = RiprCheckPayload;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a ripr check JSON payload")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut payload = RiprCheckPayload::default();
        while let Some(key) = map.next_key::<String>()? {
            match key.as_str() {
                "summary" => payload.summary = Some(map.next_value()?),
                "findings" => {
                    (self.on_event)(StreamFindingsEvent::Start);
                    map.next_value_seed(StreamFindingsSeed { on_event: self.on_event })?;
                }
                "base" => payload.base = Some(map.next_value()?),
                // Values the receipt never consumes are skipped in place —
                // serde_json discards skipped tokens without buffering them.
                _ => {
                    map.next_value::<de::IgnoredAny>()?;
                }
            }
        }
        Ok(payload)
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        while seq.next_element::<de::IgnoredAny>()?.is_some() {}
        Ok(RiprCheckPayload::default())
    }

    fn visit_bool<E>(self, _value: bool) -> Result<Self::Value, E> {
        Ok(RiprCheckPayload::default())
    }

    fn visit_i64<E>(self, _value: i64) -> Result<Self::Value, E> {
        Ok(RiprCheckPayload::default())
    }

    fn visit_u64<E>(self, _value: u64) -> Result<Self::Value, E> {
        Ok(RiprCheckPayload::default())
    }

    fn visit_f64<E>(self, _value: f64) -> Result<Self::Value, E> {
        Ok(RiprCheckPayload::default())
    }

    fn visit_str<E>(self, _value: &str) -> Result<Self::Value, E> {
        Ok(RiprCheckPayload::default())
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(RiprCheckPayload::default())
    }
}

/// [`DeserializeSeed`] streaming the elements of one `findings` value through
/// the caller's callback. The array itself is never materialized; one
/// `serde_json::Value` is held for each callback and dropped before the next
/// element is deserialized. This bounds many findings with unconsumed blobs,
/// but not a single finding whose own value is huge.
struct StreamFindingsSeed<'a, F> {
    on_event: &'a mut F,
}

impl<'de, F> DeserializeSeed<'de> for StreamFindingsSeed<'_, F>
where
    F: for<'a> FnMut(StreamFindingsEvent<'a>),
{
    type Value = ();

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(self)
    }
}

impl<'de, F> Visitor<'de> for StreamFindingsSeed<'_, F>
where
    F: for<'a> FnMut(StreamFindingsEvent<'a>),
{
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("an array of ripr findings")
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        while let Some(finding) = seq.next_element::<Value>()? {
            (self.on_event)(StreamFindingsEvent::Finding(&finding));
        }
        Ok(())
    }

    // A `findings` value that is not an array behaves as absent, matching the
    // DOM path's `Value::as_array` guard.
    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        while map.next_entry::<de::IgnoredAny, de::IgnoredAny>()?.is_some() {}
        Ok(())
    }

    fn visit_bool<E>(self, _value: bool) -> Result<Self::Value, E> {
        Ok(())
    }

    fn visit_i64<E>(self, _value: i64) -> Result<Self::Value, E> {
        Ok(())
    }

    fn visit_u64<E>(self, _value: u64) -> Result<Self::Value, E> {
        Ok(())
    }

    fn visit_f64<E>(self, _value: f64) -> Result<Self::Value, E> {
        Ok(())
    }

    fn visit_str<E>(self, _value: &str) -> Result<Self::Value, E> {
        Ok(())
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(())
    }
}

#[derive(Debug, Default)]
struct RiprPrSummaryCounts {
    weakly_exposed: usize,
    reachable_unrevealed: usize,
    no_static_path: usize,
    suppressed_by_policy: usize,
    /// Suppressed findings whose classification was not recognized — cannot be attributed
    /// to a specific bucket, but their paths matched a suppression rule.  Used to decrement
    /// `severe_gaps` after per-bucket suppression has been applied.
    suppressed_unclassified: usize,
    /// Findings on a `(file, line)` that does not exist in the head revision (#6260) —
    /// probes on code the change removes. Reported for transparency; not a suppression.
    outside_head_revision: usize,
    /// Same, for findings whose classification was not recognized. Decrements
    /// `severe_gaps` directly, like `suppressed_unclassified`.
    outside_head_unclassified: usize,
    /// Classified findings sitting in a workspace package outside the changed
    /// packages' dependent closure (#11690). Dropped before bucket counting and
    /// reported for transparency; not a policy suppression.
    out_of_dependency_graph: usize,
    /// Findings on non-production paths (#11690) — archived sources and
    /// Cargo integration-test files no production artifact compiles, per
    /// [`classify_non_production`]. Dropped from the buckets before counting
    /// and reported for transparency; not a policy suppression. Takes
    /// precedence over dependency-graph attribution so an archived seam
    /// reports here even when the graph would also drop it.
    non_production_excluded: usize,
    /// Same, for findings whose classification was not recognized. Decrements
    /// `severe_gaps` directly, like `suppressed_unclassified`.
    non_production_unclassified: usize,
}

/// DOM-path aggregation, retained as the test oracle for the streaming
/// ingestion (#12860): production ingests via [`RiprFindingBuckets`] without
/// materializing the findings array.
#[cfg(test)]
fn ripr_pr_summary_counts(
    check_value: &Value,
    check_summary: Option<&Map<String, Value>>,
    suppressions: &RiprSuppressionRules,
    head_extents: Option<&HeadLineExtents>,
    attribution: Option<&DependencyAttribution>,
    production_surface: Option<&ProductionSurface>,
) -> RiprPrSummaryCounts {
    let mut buckets = RiprFindingBuckets::default();
    if let Some(findings) = check_value.get("findings").and_then(Value::as_array) {
        for finding in findings {
            buckets.absorb(finding, suppressions, head_extents, attribution, production_surface);
        }
    }
    ripr_summary_counts_merge(
        ripr_summary_counts_seed(check_summary),
        buckets,
        check_summary.is_some(),
    )
}

/// The bucket totals seeded from the payload's own summary object, before any
/// per-finding discounting.
fn ripr_summary_counts_seed(check_summary: Option<&Map<String, Value>>) -> RiprPrSummaryCounts {
    RiprPrSummaryCounts {
        weakly_exposed: count_field(check_summary, "weakly_exposed"),
        reachable_unrevealed: count_field(check_summary, "reachable_unrevealed"),
        no_static_path: count_field(check_summary, "no_static_path"),
        ..RiprPrSummaryCounts::default()
    }
}

/// Per-finding discounting buckets, accumulated one finding at a time so the
/// findings array never has to be materialized (#12860).
#[derive(Default)]
struct RiprFindingBuckets {
    suppressed: RiprPrSummaryCounts,
    outside_head: RiprPrSummaryCounts,
    non_production: RiprPrSummaryCounts,
    unsuppressed_from_findings: RiprPrSummaryCounts,
    out_of_graph_buckets: RiprPrSummaryCounts,
    out_of_graph_total: usize,
}

impl RiprFindingBuckets {
    fn absorb(
        &mut self,
        finding: &Value,
        suppressions: &RiprSuppressionRules,
        head_extents: Option<&HeadLineExtents>,
        attribution: Option<&DependencyAttribution>,
        production_surface: Option<&ProductionSurface>,
    ) {
        let Self {
            suppressed,
            outside_head,
            non_production,
            unsuppressed_from_findings,
            out_of_graph_buckets,
            out_of_graph_total,
        } = self;
        // ripr 0.5.x: "classification" field, values "weakly_exposed" | "reachable_unrevealed" | "no_static_path".
        // ripr 0.9.x: "grip_class" field, values "weakly_gripped" | "reachable_unrevealed" | "no_static_path".
        //   "weakly_gripped" findings are counted in summary.reachable_unrevealed in 0.9.x.
        // Accept both so suppression policy applies across ripr versions.
        let raw_class = finding
            .get("classification")
            .and_then(Value::as_str)
            .or_else(|| finding.get("grip_class").and_then(Value::as_str));
        // Map to the canonical summary-counter name for correct bucket subtraction.
        let canonical: Option<&str> = match raw_class {
            Some("weakly_exposed") => Some("weakly_exposed"),
            // ripr 0.9.x: weakly_gripped is reported under summary.reachable_unrevealed
            Some("weakly_gripped") => Some("reachable_unrevealed"),
            Some("reachable_unrevealed") => Some("reachable_unrevealed"),
            Some("no_static_path") => Some("no_static_path"),
            _ => None,
        };
        // A finding is discounted for exactly one reason, in precedence
        // order: suppression policy keeps `suppressed_by_policy` meaning what
        // it always meant, head-range filtering (#6260) applies only to what
        // policy left standing, the non-production filter (#11690) only to
        // what both left standing, and dependency-graph attribution last — so
        // an archived seam reports as `non_production_excluded` even though
        // the graph would also drop it (#12267): the receipt attributes the
        // advertised basis, not whichever branch happened to run first.
        let outside = head_extents.is_some_and(|extents| extents.finding_is_outside_head(finding));
        // No resolvable path — and no compiled-into-production view — is
        // never non-production: the structural filter, like #6260, must not
        // take a fail-open shortcut on ambiguous input.
        let non_production_kind = ripr_finding_path(finding)
            .and_then(|path| classify_non_production(production_surface, &path));
        // Path suppression is checked BEFORE the classification guard (#1346).
        // A finding whose classification is unrecognized must still be suppressed if its
        // path matches a policy rule — skipping only path-unknown findings, not
        // classification-unknown ones.
        let Some(canonical) = canonical else {
            if suppression_matches_finding(suppressions, finding) {
                suppressed.suppressed_by_policy += 1;
                suppressed.suppressed_unclassified += 1;
            } else if outside {
                outside_head.outside_head_revision += 1;
                outside_head.outside_head_unclassified += 1;
            } else if non_production_kind.is_some() {
                non_production.non_production_excluded += 1;
                non_production.non_production_unclassified += 1;
            }
            return;
        };
        let policy_suppressed = suppression_matches_finding(suppressions, finding);
        if !policy_suppressed
            && !outside
            && non_production_kind.is_none()
            && attribution.is_some_and(|attribution| attribution.finding_is_out_of_graph(finding))
        {
            // #11690: the finding sits in a workspace package that does not
            // depend on any changed package (or outside every package). It is
            // dropped before bucket counting and reported for transparency.
            // Policy suppression, head-revision filtering, and the structural
            // non-production filter all keep precedence.
            *out_of_graph_total += 1;
            match canonical {
                "weakly_exposed" => out_of_graph_buckets.weakly_exposed += 1,
                "reachable_unrevealed" => out_of_graph_buckets.reachable_unrevealed += 1,
                "no_static_path" => out_of_graph_buckets.no_static_path += 1,
                _ => {}
            }
            return;
        }
        let counts = if policy_suppressed {
            suppressed.suppressed_by_policy += 1;
            &mut *suppressed
        } else if outside {
            outside_head.outside_head_revision += 1;
            &mut *outside_head
        } else if non_production_kind.is_some() {
            non_production.non_production_excluded += 1;
            &mut *non_production
        } else {
            &mut *unsuppressed_from_findings
        };
        match canonical {
            "weakly_exposed" => counts.weakly_exposed += 1,
            "reachable_unrevealed" => counts.reachable_unrevealed += 1,
            "no_static_path" => counts.no_static_path += 1,
            _ => {}
        }
    }
}

/// Combines the summary seed with the per-finding buckets. When the payload
/// carried a summary object the buckets only discount it; otherwise the
/// recognized findings are the totals themselves.
fn ripr_summary_counts_merge(
    summary_counts: RiprPrSummaryCounts,
    buckets: RiprFindingBuckets,
    check_summary_present: bool,
) -> RiprPrSummaryCounts {
    let RiprFindingBuckets {
        suppressed,
        outside_head,
        non_production,
        unsuppressed_from_findings,
        out_of_graph_buckets,
        out_of_graph_total,
    } = buckets;
    if check_summary_present {
        // Per-bucket suppression: subtract classified suppressions from their respective buckets.
        // Unclassified suppressions (suppressed_unclassified) cannot be attributed to a bucket,
        // so they are carried through for the caller to subtract from severe_gaps directly.
        // Findings outside the head revision (#6260) are subtracted the same way, and so are
        // non-production findings (#11690).
        return RiprPrSummaryCounts {
            weakly_exposed: summary_counts
                .weakly_exposed
                .saturating_sub(suppressed.weakly_exposed)
                .saturating_sub(outside_head.weakly_exposed)
                .saturating_sub(out_of_graph_buckets.weakly_exposed)
                .saturating_sub(non_production.weakly_exposed),
            reachable_unrevealed: summary_counts
                .reachable_unrevealed
                .saturating_sub(suppressed.reachable_unrevealed)
                .saturating_sub(outside_head.reachable_unrevealed)
                .saturating_sub(out_of_graph_buckets.reachable_unrevealed)
                .saturating_sub(non_production.reachable_unrevealed),
            no_static_path: summary_counts
                .no_static_path
                .saturating_sub(suppressed.no_static_path)
                .saturating_sub(outside_head.no_static_path)
                .saturating_sub(out_of_graph_buckets.no_static_path)
                .saturating_sub(non_production.no_static_path),
            suppressed_by_policy: suppressed.suppressed_by_policy,
            suppressed_unclassified: suppressed.suppressed_unclassified,
            outside_head_revision: outside_head.outside_head_revision,
            outside_head_unclassified: outside_head.outside_head_unclassified,
            out_of_dependency_graph: out_of_graph_total,
            non_production_excluded: non_production.non_production_excluded,
            non_production_unclassified: non_production.non_production_unclassified,
        };
    }
    // Path B: no summary object — bucket totals come from `unsuppressed_from_findings`, which
    // only counts recognized-classification findings.  Unclassified findings were never added
    // to those buckets, so subtracting `suppressed_unclassified` in pr_evidence_packet would
    // over-subtract and could mask a real gap via saturating_sub.  Zero it out here; the
    // caller's `.saturating_sub(summary.suppressed_unclassified)` then becomes a no-op.
    // Non-production findings were likewise never added to `unsuppressed_from_findings`.
    RiprPrSummaryCounts {
        suppressed_by_policy: suppressed.suppressed_by_policy,
        suppressed_unclassified: 0,
        outside_head_revision: outside_head.outside_head_revision,
        outside_head_unclassified: 0,
        out_of_dependency_graph: out_of_graph_total,
        non_production_excluded: non_production.non_production_excluded,
        non_production_unclassified: 0,
        ..unsuppressed_from_findings
    }
}

fn suppression_matches_finding(rules: &RiprSuppressionRules, finding: &Value) -> bool {
    let Some(path) = ripr_finding_path(finding) else {
        return false;
    };
    let path = normalize_suppression_match_path(&path);
    let raw_classification = finding
        .get("classification")
        .and_then(Value::as_str)
        .or_else(|| finding.get("grip_class").and_then(Value::as_str));
    rules
        .path_patterns
        .iter()
        .zip(rules.display_patterns.iter())
        .zip(rules.classification_patterns.iter())
        .any(|((pattern, pattern_text), allowed_classifications)| {
            let path_matches = pattern.matches(&path)
                || suppression_directory_pattern_matches(pattern_text, &path);
            path_matches
                && (allowed_classifications.is_empty()
                    || raw_classification.is_some_and(|classification| {
                        allowed_classifications.iter().any(|allowed| {
                            canonical_suppression_classification(classification)
                                == canonical_suppression_classification(allowed)
                        })
                    }))
        })
}

fn canonical_suppression_classification(classification: &str) -> &str {
    match classification {
        // ripr 0.9.x renamed this class while the repository policy retains the
        // stable semantic name used by older receipts.
        "weakly_gripped" => "reachable_unrevealed",
        other => other,
    }
}

fn suppression_directory_pattern_matches(pattern: &str, path: &str) -> bool {
    let Some(prefix) = pattern.strip_suffix("/**").or_else(|| pattern.strip_suffix("/*")) else {
        return false;
    };
    let prefix = prefix.trim_end_matches('/');
    path == prefix || path.strip_prefix(prefix).is_some_and(|rest| rest.starts_with('/'))
}

fn ripr_finding_path(finding: &Value) -> Option<String> {
    // ripr 0.5.x: path lives under finding["probe"]["path"] or finding["probe"]["file"].
    // ripr 0.9.x: path lives under finding["seam"]["file"] (and probe may be absent).
    // Accept both so suppression policy applies across ripr versions.
    finding
        .get("probe")
        .and_then(|probe| {
            ["path", "file"].into_iter().find_map(|key| probe.get(key).and_then(Value::as_str))
        })
        .map(normalize_path_text)
        .or_else(|| {
            finding
                .get("seam")
                .and_then(|seam| {
                    ["file", "path"]
                        .into_iter()
                        .find_map(|key| seam.get(key).and_then(Value::as_str))
                })
                .map(normalize_path_text)
        })
        .or_else(|| ripr_plus_seam_path(finding))
        .filter(|path| !path.trim().is_empty())
}

/// Line a RIPR finding points at, across the receipt shapes xtask accepts.
///
/// ripr 0.5.x carries it under `probe.line`; 0.9.x under `seam.line`. A finding
/// with no line cannot be located in the head revision and is never filtered.
fn ripr_finding_line(finding: &Value) -> Option<u64> {
    ["probe", "seam"]
        .into_iter()
        .find_map(|key| finding.get(key).and_then(|node| node.get("line")))
        .or_else(|| finding.get("line"))
        .and_then(Value::as_u64)
        .filter(|line| *line > 0)
}

/// Where a finding's path sits in the head revision of the change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HeadPathState {
    /// The path exists at head with this many lines.
    Present(usize),
    /// The change removes the path (deleted outright, or renamed away).
    Removed,
    /// The path could not be tied to the change's file set. Never filtered.
    Unknown,
}

/// Line extents of the changed files as they exist in the **head** revision (#6260).
///
/// `ripr check --diff` probes both sides of the diff and reports findings by line
/// number, so a probe on a line the change *deletes* is emitted against the head
/// path. Counting those as new gaps produces an unsatisfiable required check: no
/// test can cover a line that no longer exists, and the guidance generator
/// correctly names no seam for them, so the count and the guidance disagree.
///
/// This index answers "does `(file, line)` exist at head?" for the change's own
/// file set. It is built from the committed-diff receipt, so it costs one
/// `git show` per changed file and needs no second ripr run.
///
/// Every ambiguous answer is `Unknown`, which does **not** filter: an unreadable
/// blob, a finding with no path or no line, and a path that cannot be tied to
/// the change all keep their finding counted. The gate keeps failing closed on
/// real new gaps; it stops counting probes on code the change removes.
#[derive(Debug, Default, Clone)]
struct HeadLineExtents {
    /// Repo-relative path -> line count in the head revision.
    present: BTreeMap<String, usize>,
    /// Repo-relative paths the change removes.
    removed: BTreeSet<String>,
}

impl HeadLineExtents {
    fn from_committed_diff(repo: &Path, diff: &CommittedDiffReceipt) -> Self {
        let mut present = BTreeMap::new();
        let mut removed = BTreeSet::new();
        for entry in &diff.entries {
            if let Some(new_path) = entry.new_path.as_deref() {
                // An unreadable blob yields no entry, so its findings resolve to
                // `Unknown` and stay counted.
                if let Some(lines) = head_file_line_count(repo, &diff.head_sha, new_path) {
                    present.insert(normalize_repo_relative_path(new_path), lines);
                }
            }
            // Removal is read from the status code, never inferred from "has an old
            // path but no extent". `M` and `T` carry `old_path == new_path`, so
            // inferring would turn a failed `git show` on a *modified* file into a
            // phantom deletion and silently drop its findings — fail-open, the one
            // direction this filter must never take. `C` leaves its source in place;
            // only `D` and `R` remove one.
            if entry.status.starts_with(['D', 'R'])
                && let Some(old_path) = entry.old_path.as_deref()
            {
                removed.insert(normalize_repo_relative_path(old_path));
            }
        }
        // A path some other entry adds back still exists at head and keeps its extent.
        removed.retain(|path| !present.contains_key(path));
        Self { present, removed }
    }

    fn resolve(&self, raw_path: &str) -> HeadPathState {
        let normalized = normalize_repo_relative_path(raw_path);
        if let Some(lines) = self.present.get(&normalized) {
            return HeadPathState::Present(*lines);
        }
        if self.removed.contains(&normalized) {
            return HeadPathState::Removed;
        }
        // Findings may carry an absolute or checkout-prefixed path. Accept a
        // unique repo-relative suffix; anything ambiguous stays `Unknown`.
        let candidates = self
            .present
            .iter()
            .map(|(path, lines)| (path, HeadPathState::Present(*lines)))
            .chain(self.removed.iter().map(|path| (path, HeadPathState::Removed)))
            .filter(|(path, _)| path_suffix_matches(&normalized, path))
            .map(|(_, state)| state)
            .collect::<Vec<_>>();
        match candidates.as_slice() {
            [state] => *state,
            _ => HeadPathState::Unknown,
        }
    }

    /// True only when the finding is positively known to sit outside the head revision.
    fn finding_is_outside_head(&self, finding: &Value) -> bool {
        let Some(path) = ripr_finding_path(finding) else {
            return false;
        };
        let Some(line) = ripr_finding_line(finding) else {
            return false;
        };
        match self.resolve(&path) {
            HeadPathState::Present(lines) => line > lines as u64,
            HeadPathState::Removed => true,
            HeadPathState::Unknown => false,
        }
    }
}

fn head_file_line_count(repo: &Path, head_sha: &str, path: &str) -> Option<usize> {
    let spec = format!("{head_sha}:{path}");
    run_git_output(repo, &["show", spec.as_str()]).ok().map(|blob| blob.lines().count())
}

fn normalize_repo_relative_path(path: &str) -> String {
    let normalized = normalize_path_text(path);
    normalized.strip_prefix("./").unwrap_or(&normalized).to_string()
}

/// True when `candidate` ends with `repo_path` at a path boundary.
fn path_suffix_matches(candidate: &str, repo_path: &str) -> bool {
    if repo_path.is_empty() {
        return false;
    }
    let Some(prefix) = candidate.strip_suffix(repo_path) else {
        return false;
    };
    prefix.is_empty() || prefix.ends_with('/')
}

// ---------------------------------------------------------------------------
// Dependency-graph attribution basis (#11690)
// ---------------------------------------------------------------------------

/// The attribution basis stamped into packets that narrow counted findings to
/// the real dependency graph: changed workspace packages plus their transitive
/// dependents, derived from `cargo metadata` resolve edges.
const ATTRIBUTION_BASIS: &str = "changed_plus_workspace_dependents";

/// Graph source recorded alongside the basis so receipts stay honest about
/// where the reachability decision came from.
const ATTRIBUTION_GRAPH_SOURCE: &str = "cargo_metadata";

/// How the producer attributed counted findings for this diff (#11690).
#[derive(Debug, Clone)]
enum AttributionScope {
    /// Filtering is active: only findings inside a reachable package count.
    Applied(DependencyAttribution),
    /// A shared workspace input (root `Cargo.toml`, `Cargo.lock`, `.cargo/`)
    /// can affect every member; nothing is excluded.
    SharedWorkspaceInput,
    /// No changed file mapped into a workspace package (docs-only and similar);
    /// there is no graph claim to make, so nothing is excluded.
    NoChangedPackage,
}

impl AttributionScope {
    fn applied(&self) -> Option<&DependencyAttribution> {
        match self {
            AttributionScope::Applied(attribution) => Some(attribution),
            _ => None,
        }
    }

    /// The receipt-facing status string for the packet's attribution stamp.
    fn status(&self) -> &'static str {
        match self {
            AttributionScope::Applied(_) => "applied",
            AttributionScope::SharedWorkspaceInput => "shared_workspace_input_kept_all",
            AttributionScope::NoChangedPackage => "no_changed_package_kept_all",
        }
    }

    fn changed_packages(&self) -> Vec<String> {
        match self {
            AttributionScope::Applied(attribution) => {
                attribution.changed_packages.iter().cloned().collect()
            }
            _ => Vec::new(),
        }
    }

    fn reachable_packages(&self) -> Vec<String> {
        match self {
            AttributionScope::Applied(attribution) => {
                attribution.reachable_packages.iter().cloned().collect()
            }
            _ => Vec::new(),
        }
    }
}

/// Package-level reachability derived from the real cargo dependency graph.
///
/// `package_dirs` holds repo-relative manifest directories (longest first) so a
/// finding path resolves to exactly one owning package. Paths outside every
/// workspace package — archived sources under `archive/**`, generated docs,
/// tooling configs — belong to no package and therefore cannot link against
/// changed crates through cargo edges.
#[derive(Debug, Clone)]
struct DependencyAttribution {
    changed_packages: BTreeSet<String>,
    /// Changed packages plus every transitive dependent, per the resolve graph.
    reachable_packages: BTreeSet<String>,
    package_dirs: Vec<(String, String)>,
}

/// Where a finding path sits relative to the reachable package set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AttributionPathState {
    InReachablePackage,
    OutOfGraph,
    Unknown,
}

impl DependencyAttribution {
    fn resolve(&self, raw_path: &str) -> AttributionPathState {
        let path = normalize_suppression_match_path(raw_path);
        // Longest-prefix-first ordering makes one owning package win.
        let owning = self.package_dirs.iter().find(|(dir, _)| {
            path == *dir
                || path.strip_prefix(dir.as_str()).is_some_and(|rest| rest.starts_with('/'))
        });
        if let Some((_, name)) = owning {
            return if self.reachable_packages.contains(name) {
                AttributionPathState::InReachablePackage
            } else {
                AttributionPathState::OutOfGraph
            };
        }

        // A path we can tie to the repository but to no workspace package is
        // positively outside every member: dependents must be workspace
        // packages built by cargo, so archived sources (`archive/**`), docs,
        // tooling configs, and stray unregistered directories cannot link
        // against changed crates (#11690).
        //
        // A host-prefixed path that anchors nowhere stays Unknown — the same
        // fail-open convention as [`HeadLineExtents`] (#6260): the gate never
        // under-attributes on an ambiguous answer.
        if path.starts_with('/') || path.as_bytes().get(1) == Some(&b':') {
            return AttributionPathState::Unknown;
        }
        AttributionPathState::OutOfGraph
    }

    /// True only when the finding is positively known to sit in a workspace
    /// package outside the reachable set. Unknown paths keep counting — the
    /// same fail-open convention as [`HeadLineExtents`] (#6260): the gate never
    /// under-attributes on an ambiguous answer.
    fn finding_is_out_of_graph(&self, finding: &Value) -> bool {
        let Some(path) = ripr_finding_path(finding) else {
            return false;
        };
        self.resolve(&path) == AttributionPathState::OutOfGraph
    }
}

/// Build the attribution scope for a committed diff from cargo metadata.
///
/// Errors mean the graph could not be read at all; callers must then skip
/// filtering entirely rather than guess.
fn dependency_attribution_scope(
    metadata: &Value,
    changed_paths: &[String],
) -> Result<AttributionScope> {
    let root = metadata
        .get("workspace_root")
        .and_then(Value::as_str)
        .ok_or_else(|| eyre!("cargo metadata missing workspace_root"))?
        .replace('\\', "/");
    let packages = metadata
        .get("packages")
        .and_then(Value::as_array)
        .ok_or_else(|| eyre!("cargo metadata missing packages array"))?
        .iter()
        .filter_map(|pkg| {
            let name = pkg.get("name").and_then(Value::as_str)?;
            let manifest = pkg.get("manifest_path").and_then(Value::as_str)?;
            let dir = manifest
                .replace('\\', "/")
                .strip_prefix(root.as_str())
                .and_then(|rest| rest.strip_prefix('/'))
                .and_then(|rest| rest.strip_suffix("/Cargo.toml"))?
                .to_string();
            Some((dir, name.to_string()))
        })
        .collect::<Vec<_>>();
    if packages.is_empty() {
        bail!("cargo metadata listed no workspace package manifests");
    }
    let all_package_names = packages.iter().map(|(_, name)| name.clone()).collect::<BTreeSet<_>>();

    let mut changed_packages = BTreeSet::new();
    let mut shared_input_touched = false;
    for file in changed_paths {
        let normalized = normalize_repo_relative_path(file);
        if normalized == "Cargo.toml"
            || normalized == "Cargo.lock"
            || normalized.starts_with(".cargo/")
        {
            shared_input_touched = true;
        }
        for (dir, name) in &packages {
            let inside = normalized == *dir
                || normalized.strip_prefix(dir.as_str()).is_some_and(|rest| rest.starts_with('/'));
            if inside {
                changed_packages.insert(name.clone());
                break;
            }
        }
    }

    if shared_input_touched {
        return Ok(AttributionScope::SharedWorkspaceInput);
    }
    if changed_packages.is_empty() {
        return Ok(AttributionScope::NoChangedPackage);
    }

    let mut package_dirs = packages;
    package_dirs
        .sort_by(|left, right| right.0.len().cmp(&left.0.len()).then_with(|| left.0.cmp(&right.0)));
    // The resolve graph reflects the currently resolved feature set, so a
    // dev-, build-, or optional-dependency edge can be absent from it while a
    // real configuration still links the changed crate. Union every declared
    // manifest edge into the reverse map: over-approximating reachability only
    // keeps more findings counted — under-attribution is the one direction
    // this filter must never take (#11690).
    let mut rev_deps = crate::tasks::ci_scope::build_reverse_dep_map(metadata);
    for pkg in metadata.get("packages").and_then(Value::as_array).into_iter().flatten() {
        let Some(pkg_name) = pkg.get("name").and_then(Value::as_str) else {
            continue;
        };
        for dep in pkg.get("dependencies").and_then(Value::as_array).into_iter().flatten() {
            let Some(dep_name) = dep.get("name").and_then(Value::as_str) else {
                continue;
            };
            if !dep_name.is_empty() {
                rev_deps.entry(dep_name.to_string()).or_default().insert(pkg_name.to_string());
            }
        }
    }
    let dependents = crate::tasks::ci_scope::reverse_dep_closure(
        &changed_packages,
        &rev_deps,
        &all_package_names,
    );
    let mut reachable_packages = changed_packages.clone();
    reachable_packages.extend(dependents);

    Ok(AttributionScope::Applied(DependencyAttribution {
        changed_packages,
        reachable_packages,
        package_dirs,
    }))
}

fn attribution_stamp(scope: Option<&AttributionScope>) -> Value {
    let Some(scope) = scope else {
        return json!({
            "basis": ATTRIBUTION_BASIS,
            "graph_source": ATTRIBUTION_GRAPH_SOURCE,
            "status": "unavailable",
            "changed_packages": [],
            "reachable_packages": [],
            "reason": "cargo metadata could not be read; no findings were excluded",
        });
    };
    json!({
        "basis": ATTRIBUTION_BASIS,
        "graph_source": ATTRIBUTION_GRAPH_SOURCE,
        "status": scope.status(),
        "changed_packages": scope.changed_packages(),
        "reachable_packages": scope.reachable_packages(),
    })
}

fn normalize_suppression_match_path(path: &str) -> String {
    let normalized = normalize_path_text(path);
    let normalized = normalized.strip_prefix("./").unwrap_or(&normalized);
    ["crates/", "docs/", "archive/", "xtask/", "scripts/", "policy/", ".ci/"]
        .into_iter()
        .filter_map(|anchor| normalized.find(anchor))
        .min()
        .map_or_else(|| normalized.to_string(), |index| normalized[index..].to_string())
}

/// The basis stamped into packets that classify findings as non-production:
/// a finding is excluded only when its file is provably outside the set of
/// sources compiled into workspace artifacts (#11690, #12267 review).
const NON_PRODUCTION_BASIS: &str = "compiled_into_workspace_artifacts";

/// Graph source recorded alongside the basis so receipts stay honest about
/// where the compiled-into-production decision came from.
const NON_PRODUCTION_SOURCE: &str = "cargo_metadata_target_membership";

/// Why a finding path is structurally non-production for the new-gap basis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NonProductionKind {
    /// Archived sources under the repository's `archive/**`.
    Archive,
    /// A file under a Cargo integration-test directory that no production
    /// artifact compiles.
    IntegrationTest,
}

/// The compiled-into-production view of the workspace (#12267 review repair).
///
/// Production for the blocking count means code a PR can be required to
/// reveal: active sources compiled into workspace artifacts. Membership is
/// decided by target graph and include closure, not by pathname shape:
///
/// - the `src_path` of every workspace target whose kinds contain at least one
///   production kind (anything other than `test`, `bench`, and `example`) — so
///   an explicit `[[bin]]` whose path lives under `tests/` is production;
/// - every `.rs` file under a workspace package's `src/` tree (a safe
///   over-approximation: mislabeling a dead source as production only keeps a
///   finding counted, the fail-closed direction);
/// - the transitive textual include closure from those seeds: `#[path = "…"]`
///   string-literal includes and plain `mod name;` sibling declarations. This
///   is what marks `xtask/tests/support/emacs_host_runner.rs` (compiled into
///   the exported `xtask::emacs_host_run` module) and the
///   `xtask/tests/support/zed_*.rs` validator substrates (compiled into the
///   `validate-zed-*` binaries) as production even though a `tests` component
///   appears in their paths.
///
/// Limitations, kept honest by the receipt stamp: the closure is textual, not
/// a cargo build graph — `include!`, macro-generated module paths, and
/// multi-line `#[path]` attributes are not followed, and `#[cfg(test)] mod`
/// declarations inside production files over-include their siblings (the safe
/// direction). A `tests/`-located file compiled through an unfollowed
/// mechanism would be misclassified as non-production.
#[derive(Debug, Clone)]
struct ProductionSurface {
    /// Normalized (forward-slash) absolute host path of the workspace root,
    /// from `cargo metadata`. Archive classification anchors here.
    repo_root: String,
    /// Repo-relative normalized paths of files compiled into production
    /// workspace artifacts.
    production_paths: BTreeSet<String>,
}

impl ProductionSurface {
    #[cfg(test)]
    fn from_parts(repo_root: &str, production_paths: &[&str]) -> Self {
        ProductionSurface {
            repo_root: repo_root.to_string(),
            production_paths: production_paths.iter().map(|path| path.to_string()).collect(),
        }
    }
}

/// Resolve a finding path (host-absolute or repo-relative) against the
/// repository root. Absolute paths strip the workspace root prefix; a path
/// that is absolute but outside the root resolves to `None` — fail closed —
/// because no repo-relative classification is possible. Relative paths pass
/// through. This deliberately does not reuse the earliest-anchor-substring
/// normalization: a checkout under `/work/archive/perl-lsp-swarm` would
/// otherwise normalize active sources to `archive/perl-lsp-swarm/...`.
fn repo_relative_surface_path(surface: &ProductionSurface, raw_path: &str) -> Option<String> {
    let normalized = normalize_path_text(raw_path);
    // Windows verbatim prefixes (`//?/F:/...`) from directory walks and host
    // tools must not defeat the root-prefix match against cargo's plain
    // `F:/...` workspace root.
    let normalized = normalized.strip_prefix("//?/").unwrap_or(&normalized);
    let normalized = normalized.strip_prefix("./").unwrap_or(normalized);
    let absolute = normalized.starts_with('/') || normalized.as_bytes().get(1) == Some(&b':');
    if !absolute {
        return Some(normalized.to_string());
    }
    let root = surface.repo_root.trim_end_matches('/');
    normalized.strip_prefix(root).and_then(|rest| rest.strip_prefix('/')).map(str::to_string)
}

/// True when a finding path belongs to a surface that is not production for
/// the new-gap basis (#11690).
///
/// Two structural classes are non-production, independent of any mutable
/// suppression policy:
///
/// - archived sources under the repository's `archive/**` — anchored at the
///   workspace root so an ancestor checkout directory named `archive` never
///   classifies active sources as archived;
/// - files under a Cargo integration-test directory (a path component exactly
///   `tests/`) that the production surface does not compile — the recurring
///   `test_receipt_surface` class behind #6842's 162-finding block. A
///   `tests/`-located file that IS compiled into a production artifact (the
///   `xtask::emacs_host_run` runner substrate, the `validate-zed-*` validator
///   substrates) stays counted.
///
/// `None` (including when no production surface could be built, or the path
/// cannot be resolved against the repository root) is never non-production:
/// the gate keeps failing closed on ambiguous input, the same convention as
/// #6260 and the dependency-graph filter.
fn classify_non_production(
    surface: Option<&ProductionSurface>,
    raw_path: &str,
) -> Option<NonProductionKind> {
    let surface = surface?;
    let path = repo_relative_surface_path(surface, raw_path)?;
    if path == "archive" || path.starts_with("archive/") {
        return Some(NonProductionKind::Archive);
    }
    if path.split('/').any(|component| component == "tests")
        && !surface.production_paths.contains(&path)
    {
        return Some(NonProductionKind::IntegrationTest);
    }
    None
}

/// Build the production surface from cargo metadata and the repo checkout.
/// Errors mean the surface could not be established; callers must then skip
/// non-production classification entirely rather than guess.
fn production_surface_from_metadata(repo: &Path, metadata: &Value) -> Result<ProductionSurface> {
    let root = metadata
        .get("workspace_root")
        .and_then(Value::as_str)
        .ok_or_else(|| eyre!("cargo metadata missing workspace_root"))?
        .replace('\\', "/");
    let packages = metadata
        .get("packages")
        .and_then(Value::as_array)
        .ok_or_else(|| eyre!("cargo metadata missing packages array"))?;
    let mut surface = ProductionSurface { repo_root: root, production_paths: BTreeSet::new() };
    let mut scan_queue: Vec<String> = Vec::new();
    for package in packages {
        let manifest = package.get("manifest_path").and_then(Value::as_str);
        let package_dir =
            manifest.map(|manifest| manifest.replace('\\', "/")).and_then(|manifest| {
                manifest
                    .strip_prefix(surface.repo_root.as_str())
                    .and_then(|rest| rest.strip_prefix('/'))
                    .and_then(|rest| rest.strip_suffix("/Cargo.toml"))
                    .map(str::to_string)
            });
        // Target membership: a target with any production kind compiles its
        // src_path into workspace artifacts, wherever that file lives.
        for target in package.get("targets").and_then(Value::as_array).into_iter().flatten() {
            let production_kind =
                target.get("kind").and_then(Value::as_array).is_some_and(|kinds| {
                    kinds.iter().any(|kind| {
                        kind.as_str()
                            .is_some_and(|kind| !matches!(kind, "test" | "bench" | "example"))
                    })
                });
            if !production_kind {
                continue;
            }
            let Some(src_path) = target.get("src_path").and_then(Value::as_str) else {
                continue;
            };
            let resolved = repo_relative_surface_path(&surface, src_path);
            if let Some(resolved) = resolved
                && surface.production_paths.insert(resolved.clone())
            {
                scan_queue.push(resolved);
            }
        }
        // Seed with the package's whole `src/` tree: a safe over-approximation
        // that also gives the include closure its production starting points.
        if let Some(package_dir) = package_dir {
            let src_dir = repo.join(&package_dir).join("src");
            if src_dir.is_dir() {
                for entry in
                    walkdir::WalkDir::new(&src_dir).into_iter().filter_map(|entry| entry.ok())
                {
                    let is_source = entry.file_type().is_file()
                        && entry.path().extension().is_some_and(|ext| ext == "rs");
                    if !is_source {
                        continue;
                    }
                    let entry_path = entry.path().display().to_string();
                    if let Some(resolved) = repo_relative_surface_path(&surface, &entry_path)
                        && surface.production_paths.insert(resolved.clone())
                    {
                        scan_queue.push(resolved);
                    }
                }
            }
        }
    }
    if surface.production_paths.is_empty() {
        bail!("cargo metadata resolved no workspace production sources");
    }
    scan_include_closure(repo, &mut surface.production_paths, scan_queue);
    Ok(surface)
}

/// Follow `#[path = "…"]` includes and plain `mod name;` declarations from the
/// seeded production files so sources compiled from outside `src/` trees
/// (notably under `tests/`) join the production set. Only files that newly
/// join the set are scanned, so cycles terminate.
fn scan_include_closure(repo: &Path, production_paths: &mut BTreeSet<String>, queue: Vec<String>) {
    let mut queue = std::collections::VecDeque::from(queue);
    while let Some(file) = queue.pop_front() {
        let Ok(text) = fs::read_to_string(repo.join(&file)) else {
            continue;
        };
        let file_dir = file.rsplit_once('/').map(|(dir, _)| dir.to_string()).unwrap_or_default();
        for target in module_include_targets(&text, &file_dir) {
            if production_paths.insert(target.clone()) {
                queue.push_back(target);
            }
        }
    }
}

/// Textually resolve the module targets a source file compiles: every
/// single-line `#[path = "…"]` string-literal include and every plain
/// `mod name;` declaration (sibling `name.rs` or `name/mod.rs`). A `mod` line
/// covered by its own preceding `#[path]` attribute uses that attribute's
/// target only. Paths are normalized lexically; an include that escapes the
/// repository root is ignored (fail closed: it cannot be a workspace source).
fn module_include_targets(text: &str, file_dir: &str) -> Vec<String> {
    let mut targets = Vec::new();
    let mut previous_line_was_path_attribute = false;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if let Some(relative) = path_attribute_target(trimmed) {
            if let Some(resolved) = lexically_join(file_dir, relative) {
                targets.push(resolved);
            }
            previous_line_was_path_attribute = true;
            continue;
        }
        if let Some(name) = plain_mod_declaration_name(trimmed)
            && !previous_line_was_path_attribute
        {
            let base =
                if file_dir.is_empty() { name.to_string() } else { format!("{file_dir}/{name}") };
            targets.push(format!("{base}.rs"));
            targets.push(format!("{base}/mod.rs"));
        }
        previous_line_was_path_attribute = false;
    }
    targets
}

/// Extract the quoted target of a `#[path = "…"]` attribute on one line.
/// `#[path` must be followed by `=`, whitespace, or `]` so a hypothetical
/// longer attribute name is not mistaken for the include attribute.
fn path_attribute_target(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("#[path")?;
    if !rest.starts_with(['=', ' ', '\t', ']']) {
        return None;
    }
    let quoted = line.split_once('"')?.1;
    let target = quoted.split_once('"')?.0;
    if target.is_empty() { None } else { Some(target) }
}

/// Extract the module name from a plain `mod name;` declaration (any
/// visibility prefix), skipping inline `mod name {` bodies and `#[path]`
/// lines, which carry their own target.
fn plain_mod_declaration_name(line: &str) -> Option<&str> {
    if line.starts_with("#[") {
        return None;
    }
    let mut words = line.split_whitespace();
    let mut word = words.next()?;
    while word == "pub" || word.starts_with("pub(") {
        word = words.next()?;
    }
    if word != "mod" {
        return None;
    }
    let name = words.next()?.strip_suffix(';')?;
    if name.is_empty()
        || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        || name.starts_with(|c: char| c.is_ascii_digit())
    {
        return None;
    }
    Some(name)
}

/// Join a relative include path onto its containing directory, resolving `.`
/// and `..` lexically. Returns `None` when the path would escape the root.
fn lexically_join(dir: &str, relative: &str) -> Option<String> {
    let relative = normalize_path_text(relative);
    let mut components: Vec<&str> = Vec::new();
    for component in dir.split('/').chain(relative.split('/')) {
        match component {
            "." | "" => {}
            // `..` above the root has nothing to pop: propagate the refusal
            // through the `Option` this function already returns.
            ".." => {
                components.pop()?;
            }
            other => components.push(other),
        }
    }
    if components.is_empty() {
        return None;
    }
    Some(components.join("/"))
}

/// The receipt-facing non-production stamp: what basis classified findings,
/// and whether that basis was available at all. Deliberately carries no host
/// path — receipts never emit CI-runner paths.
fn non_production_stamp(surface: Option<&ProductionSurface>) -> Value {
    let Some(surface) = surface else {
        return json!({
            "basis": NON_PRODUCTION_BASIS,
            "source": NON_PRODUCTION_SOURCE,
            "status": "unavailable",
            "reason": "cargo metadata could not be read; no findings were classified non-production",
        });
    };
    json!({
        "basis": NON_PRODUCTION_BASIS,
        "source": NON_PRODUCTION_SOURCE,
        "status": "applied",
        "production_sources": surface.production_paths.len(),
    })
}

#[cfg(test)]
fn pr_evidence_packet(
    options: &PrEvidenceOptions,
    changed_files: &[String],
    check_value: &Value,
    base_sha: &str,
    head_sha: &str,
    suppressions: &RiprSuppressionRules,
) -> Value {
    pr_evidence_packet_with_count(
        options,
        check_value,
        base_sha,
        head_sha,
        suppressions,
        None,
        PrEvidenceContext {
            changed_file_count: changed_files.len(),
            attribution_scope: None,
            production_surface: None,
        },
    )
}

#[cfg(test)]
fn pr_evidence_packet_on_surface(
    options: &PrEvidenceOptions,
    changed_files: &[String],
    check_value: &Value,
    base_sha: &str,
    head_sha: &str,
    suppressions: &RiprSuppressionRules,
    production_surface: Option<&ProductionSurface>,
) -> Value {
    pr_evidence_packet_with_count(
        options,
        check_value,
        base_sha,
        head_sha,
        suppressions,
        None,
        PrEvidenceContext {
            changed_file_count: changed_files.len(),
            attribution_scope: None,
            production_surface,
        },
    )
}

/// The stamped measurement context for one PR evidence packet: which diff was
/// measured, and which bases the receipt records as applied.
///
/// These three facts travel together — every caller that supplies one supplies
/// all of them — while the options, exact base/head identity, check payload,
/// and suppression policy stay explicit arguments. Grouping them keeps both
/// receipt builders inside the configured argument budget without weakening any
/// call site: the struct deliberately has no `Default`, so a caller that omits
/// a field fails to compile rather than silently measuring against an empty
/// basis (#13809).
///
/// Head-revision line extents are deliberately *not* a field. #12569 moved the
/// extent filter off the receipt builder and into ingestion, where it is
/// applied per finding as the payload streams; by the time either builder runs,
/// the counts are already scoped. Extents stay an explicit argument of the
/// filtering path so this struct cannot imply a filter the builder never runs.
#[derive(Clone, Copy)]
struct PrEvidenceContext<'a> {
    /// Number of files in the committed diff under evaluation.
    changed_file_count: usize,
    /// Dependency-attribution basis; `None` keeps every finding counted.
    attribution_scope: Option<&'a AttributionScope>,
    /// Compiled-production surface; `None` keeps every finding counted.
    production_surface: Option<&'a ProductionSurface>,
}

/// Pin the no-`Default` property the doc comment above claims, so it is
/// checked rather than merely documented (#13809 review).
///
/// Every field would satisfy `#[derive(Default)]` — `usize` and `Option<&_>`
/// both have one — so a future derive added to silence an unrelated lint
/// would compile silently and hand callers an empty measurement basis. The
/// focused tests cannot catch that: they construct the struct explicitly, so
/// a `Default` impl would simply go unused.
///
/// Two blanket impls overlap only when `PrEvidenceContext: Default`, which
/// makes the item reference below ambiguous and fails the build. This is the
/// `static_assertions::assert_not_impl_any!` pattern, inlined to avoid a new
/// dev-dependency for one assertion.
const _: fn() = || {
    trait AmbiguousIfDefault<A> {
        fn marker() {}
    }
    impl<T> AmbiguousIfDefault<()> for T {}
    impl<T: Default> AmbiguousIfDefault<u8> for T {}
    let _ = <PrEvidenceContext<'static> as AmbiguousIfDefault<_>>::marker;
};

/// DOM-path receipt builder, retained as the test oracle for the streaming
/// ingestion (#12860): receipt bytes must match
/// [`pr_evidence_packet_from_summary`] for the same payload.
#[cfg(test)]
fn pr_evidence_packet_with_count(
    options: &PrEvidenceOptions,
    check_value: &Value,
    base_sha: &str,
    head_sha: &str,
    suppressions: &RiprSuppressionRules,
    head_extents: Option<&HeadLineExtents>,
    context: PrEvidenceContext<'_>,
) -> Value {
    let PrEvidenceContext { attribution_scope, production_surface, .. } = context;
    let check_summary = check_value.get("summary").and_then(Value::as_object);
    let summary = ripr_pr_summary_counts(
        check_value,
        check_summary,
        suppressions,
        head_extents,
        attribution_scope.and_then(AttributionScope::applied),
        production_surface,
    );
    pr_evidence_packet_from_summary(
        options,
        &RiprCheckIngestion {
            summary_counts: summary,
            check_summary_present: check_summary.is_some(),
        },
        base_sha,
        head_sha,
        suppressions,
        context,
    )
}

/// Builds the receipt from streamed ingestion results. Receipt bytes are
/// identical to the DOM path (`pr_evidence_packet_with_count`, retained as the
/// test oracle) for the same payload; the #12860 compatibility tests pin this
/// byte for byte.
///
/// Takes the same [`PrEvidenceContext`] as the DOM path so the stamped
/// measurement basis has one spelling (#13809), not one per ingestion path.
fn pr_evidence_packet_from_summary(
    options: &PrEvidenceOptions,
    ingestion: &RiprCheckIngestion,
    base_sha: &str,
    head_sha: &str,
    suppressions: &RiprSuppressionRules,
    context: PrEvidenceContext<'_>,
) -> Value {
    let PrEvidenceContext { changed_file_count, attribution_scope, production_surface } = context;
    let summary = &ingestion.summary_counts;
    let weakly_exposed = summary.weakly_exposed;
    let reachable_unrevealed = summary.reachable_unrevealed;
    let no_static_path = summary.no_static_path;
    // Per-bucket suppressed counts have already been subtracted from their buckets above.
    // Findings suppressed by path but with an unrecognized classification (#1346) could not
    // be attributed to a bucket; subtract them from the severe_gaps total now. Non-production
    // findings with an unrecognized classification (#11690) subtract the same way.
    let severe_gaps = weakly_exposed
        .saturating_add(reachable_unrevealed)
        .saturating_add(no_static_path)
        .saturating_sub(summary.suppressed_unclassified)
        .saturating_sub(summary.outside_head_unclassified)
        .saturating_sub(summary.non_production_unclassified);
    let ripr_severe_gap = severe_gaps > 0;
    let warnings = if ingestion.check_summary_present {
        Vec::new()
    } else {
        vec![json!({
            "kind": "invalid_json",
            "message": "RIPR check output did not include a summary object.",
            "path": null
        })]
    };
    json!({
        "schema_version": "0.1",
        "tool": "ripr",
        "kind": "pr_evidence",
        "scope": "diff",
        "status": if warnings.is_empty() { "advisory" } else { "incomplete" },
        "root": options.root,
        "base": options.base,
        "base_sha": base_sha,
        "head": options.head,
        "head_sha": head_sha,
        "pr_head_sha": optional_sha_value(options.pr_head_sha.as_deref()),
        "evaluated_head": options.head,
        "evaluated_head_sha": head_sha,
        "summary": {
            "changed_files": changed_file_count,
            "comments": 0,
            "summary_only": 0,
            "suppressed": 0,
            "weakly_exposed": weakly_exposed,
            "reachable_unrevealed": reachable_unrevealed,
            "no_static_path": no_static_path,
            "severe_gaps": severe_gaps,
            "requires_targeted_mutation": ripr_severe_gap,
            "ripr_severe_gap": ripr_severe_gap,
            "routing_reason": if ripr_severe_gap { json!("ripr severe gap") } else { Value::Null },
            "suppressed_by_policy": summary.suppressed_by_policy,
            "outside_head_revision": summary.outside_head_revision,
            "out_of_dependency_graph": summary.out_of_dependency_graph,
            "non_production_excluded": summary.non_production_excluded,
            "suppression_patterns": suppressions.display_patterns.clone(),
        },
        "attribution": attribution_stamp(attribution_scope),
        "non_production": non_production_stamp(production_surface),
        "artifacts": [
            {
                "label": "PR evidence JSON",
                "path": PR_EVIDENCE_JSON,
                "kind": "json",
                "scope": "diff",
                "available": true,
                "required": true
            },
            {
                "label": "PR evidence Markdown",
                "path": PR_EVIDENCE_MD,
                "kind": "markdown",
                "scope": "diff",
                "available": true
            },
            {
                "label": "Analyzed PR diff",
                "path": PR_DIFF,
                "kind": "other",
                "scope": "diff",
                "available": true
            },
            {
                "label": "Committed diff status receipt",
                "path": PR_DIFF_RECEIPT,
                "kind": "json",
                "scope": "diff",
                "available": true,
                "required": true
            }
        ],
        "warnings": warnings,
        "advisory_limits": [
            "RIPR evidence is static and advisory by default.",
            "This packet does not post review comments or execute mutation.",
            "Public badge state must not be derived from this diff-scoped packet."
        ]
    })
}

fn validate_pr_evidence_packet(
    packet: &Value,
    options: &PrEvidenceOptions,
    expected_changed_files: usize,
    markdown_exists: bool,
    expected_base_sha: &str,
    expected_head_sha: &str,
) -> Result<()> {
    let mut violations = Vec::new();
    expect_string(packet, "schema_version", "0.1", &mut violations);
    expect_string(packet, "tool", "ripr", &mut violations);
    expect_string(packet, "kind", "pr_evidence", &mut violations);
    expect_string(packet, "scope", "diff", &mut violations);
    expect_string(packet, "root", &options.root, &mut violations);
    expect_string(packet, "base", &options.base, &mut violations);
    expect_string(packet, "base_sha", expected_base_sha, &mut violations);
    expect_string(packet, "head", &options.head, &mut violations);
    expect_string(packet, "head_sha", expected_head_sha, &mut violations);
    match (&options.pr_head_sha, packet.get("pr_head_sha")) {
        (Some(expected), Some(value)) => {
            expect_string_value(value, "pr_head_sha", expected, &mut violations)
        }
        (Some(_), None) => violations.push("pr_head_sha is missing".to_string()),
        (None, Some(value)) if !value.is_null() => {
            violations.push("pr_head_sha must be null when no PR head was supplied".to_string())
        }
        (None, None) => violations.push("pr_head_sha is missing".to_string()),
        (None, Some(_)) => {}
    }
    expect_string(packet, "evaluated_head", &options.head, &mut violations);
    expect_string(packet, "evaluated_head_sha", expected_head_sha, &mut violations);
    match packet.get("status").and_then(Value::as_str) {
        Some("advisory" | "incomplete" | "error") => {}
        Some(other) => violations.push(format!("status {other:?} is not valid")),
        None => violations.push("status is missing or not a string".to_string()),
    }
    let Some(summary) = packet.get("summary").and_then(Value::as_object) else {
        bail!("summary is missing or not an object");
    };
    for key in [
        "comments",
        "summary_only",
        "suppressed",
        "weakly_exposed",
        "reachable_unrevealed",
        "no_static_path",
        "severe_gaps",
    ] {
        if !summary.get(key).is_some_and(Value::is_u64) {
            violations.push(format!("summary.{key} is missing or not an integer"));
        }
    }
    match summary.get("changed_files").and_then(Value::as_u64) {
        Some(value) if value == expected_changed_files as u64 => {}
        Some(value) => violations
            .push(format!("summary.changed_files is {value}, expected {expected_changed_files}")),
        None => violations.push("summary.changed_files is missing or not an integer".to_string()),
    }
    for key in ["requires_targeted_mutation", "ripr_severe_gap"] {
        if !summary.get(key).is_some_and(Value::is_boolean) {
            violations.push(format!("summary.{key} is missing or not a boolean"));
        }
    }
    if !(summary.get("routing_reason").is_some_and(Value::is_string)
        || summary.get("routing_reason").is_some_and(Value::is_null))
    {
        violations.push("summary.routing_reason must be string or null".to_string());
    }
    if !summary.get("out_of_dependency_graph").is_some_and(Value::is_u64) {
        violations.push("summary.out_of_dependency_graph is missing or not an integer".to_string());
    }
    if !summary.get("non_production_excluded").is_some_and(Value::is_u64) {
        violations.push("summary.non_production_excluded is missing or not an integer".to_string());
    }
    match packet.get("attribution").and_then(Value::as_object) {
        Some(attribution) => {
            if attribution.get("basis").and_then(Value::as_str) != Some(ATTRIBUTION_BASIS) {
                violations.push(format!("attribution.basis must be {ATTRIBUTION_BASIS:?}"));
            }
            match attribution.get("status").and_then(Value::as_str) {
                Some(
                    "applied"
                    | "shared_workspace_input_kept_all"
                    | "no_changed_package_kept_all"
                    | "unavailable",
                ) => {}
                _ => violations.push("attribution.status is not a valid basis status".to_string()),
            }
        }
        None => violations.push("attribution is missing or not an object".to_string()),
    }
    match packet.get("non_production").and_then(Value::as_object) {
        Some(non_production) => {
            if non_production.get("basis").and_then(Value::as_str) != Some(NON_PRODUCTION_BASIS) {
                violations.push(format!("non_production.basis must be {NON_PRODUCTION_BASIS:?}"));
            }
            match non_production.get("status").and_then(Value::as_str) {
                Some("applied" | "unavailable") => {}
                _ => {
                    violations.push("non_production.status is not a valid basis status".to_string())
                }
            }
        }
        None => violations.push("non_production is missing or not an object".to_string()),
    }
    if !packet.get("warnings").is_some_and(Value::is_array) {
        violations.push("warnings is missing or not an array".to_string());
    }
    match packet.get("advisory_limits").and_then(Value::as_array) {
        Some(values) if !values.is_empty() => {}
        _ => violations.push("advisory_limits is missing or empty".to_string()),
    }
    for required in [PR_EVIDENCE_JSON, PR_EVIDENCE_MD, PR_DIFF_RECEIPT] {
        if !artifact_available(packet, required) {
            violations.push(format!("artifacts[] is missing {required}"));
        }
    }
    if !markdown_exists {
        violations.push(format!("{PR_EVIDENCE_MD} is missing"));
    }
    if violations.is_empty() {
        Ok(())
    } else {
        bail!("PR evidence contract violations:\n{}", bullet_list(&violations));
    }
}

fn render_pr_evidence_markdown(packet: &Value) -> String {
    let summary = packet.get("summary").and_then(Value::as_object);
    let mut out = String::new();
    out.push_str("# PR Evidence Summary\n\n");
    out.push_str("## Fast Gate\n\n");
    out.push_str(&format!("- status: `{}`\n", string_field(packet, "status", "unknown")));
    out.push_str(&format!("- root: `{}`\n", md_escape(string_field(packet, "root", DEFAULT_ROOT))));
    out.push_str(&format!("- base: `{}`\n", md_escape(string_field(packet, "base", DEFAULT_BASE))));
    out.push_str(&format!("- head: `{}`\n", md_escape(string_field(packet, "head", DEFAULT_HEAD))));
    out.push_str(&format!("- changed files: {}\n\n", count_field(summary, "changed_files")));
    out.push_str("## RIPR\n\n");
    out.push_str(&format!("- changed-line comments: {}\n", count_field(summary, "comments")));
    out.push_str(&format!("- summary-only guidance: {}\n", count_field(summary, "summary_only")));
    out.push_str(&format!("- suppressed guidance: {}\n", count_field(summary, "suppressed")));
    out.push_str(&format!("- weakly_exposed: {}\n", count_field(summary, "weakly_exposed")));
    out.push_str(&format!(
        "- reachable_unrevealed: {}\n",
        count_field(summary, "reachable_unrevealed")
    ));
    out.push_str(&format!("- no_static_path: {}\n", count_field(summary, "no_static_path")));
    out.push_str(&format!(
        "- suppressed_by_policy: {}\n",
        count_field(summary, "suppressed_by_policy")
    ));
    out.push_str(&format!(
        "- outside_head_revision: {}\n",
        count_field(summary, "outside_head_revision")
    ));
    out.push_str(&format!(
        "- non_production_excluded: {}\n",
        count_field(summary, "non_production_excluded")
    ));
    out.push_str(&format!("- severe gaps: {}\n\n", count_field(summary, "severe_gaps")));
    out.push_str("## Targeted Mutation\n\n");
    out.push_str(&format!(
        "- requires_targeted_mutation: {}\n",
        bool_field(summary, "requires_targeted_mutation")
    ));
    out.push_str(&format!(
        "- routing_reason: `{}`\n\n",
        summary_string_or_null(summary, "routing_reason")
    ));
    out.push_str("## Artifacts\n\n");
    out.push_str("| Artifact | Path | Scope | Available |\n");
    out.push_str("| --- | --- | --- | --- |\n");
    if let Some(artifacts) = packet.get("artifacts").and_then(Value::as_array) {
        for artifact in artifacts {
            out.push_str(&format!(
                "| {} | `{}` | {} | {} |\n",
                md_escape(string_field(artifact, "label", "artifact")),
                md_escape(string_field(artifact, "path", "unknown")),
                md_escape(string_field(artifact, "scope", "unknown")),
                artifact.get("available").and_then(Value::as_bool).unwrap_or(false)
            ));
        }
    }
    out.push_str(
        "\n_This packet is diff-scoped and advisory. Do not copy it into public badge state._\n",
    );
    out
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ReviewCommentsOptions {
    root: String,
    base: String,
    head: String,
    pr_head_sha: Option<String>,
    timeout_seconds: Option<u64>,
}

fn write_review_comments(repo: &Path, options: &ReviewCommentsOptions) -> Result<()> {
    verify_revision(repo, &options.base)?;
    verify_revision(repo, &options.head)?;
    if let Some(pr_head_sha) = &options.pr_head_sha {
        verify_revision(repo, pr_head_sha)?;
    }
    let root = command_root_arg(repo, &options.root)?;
    if current_pr_evidence_has_no_severe_gaps(repo, options)? {
        write_clean_review_comments(repo, options, &root)?;
    } else if let Err(err) = run_ripr_review_comments(repo, options, &root) {
        write_degraded_review_comments(repo, options, &root, &err.to_string())?;
    }
    stamp_review_comments_receipt(repo, options)?;
    validate_review_comments(repo, options, true)?;
    println!("Wrote {REVIEW_COMMENTS_JSON}");
    println!("Wrote {REVIEW_COMMENTS_MD}");
    Ok(())
}

fn check_review_comments(repo: &Path, options: &ReviewCommentsOptions) -> Result<()> {
    verify_revision(repo, &options.base)?;
    verify_revision(repo, &options.head)?;
    if let Some(pr_head_sha) = &options.pr_head_sha {
        verify_revision(repo, pr_head_sha)?;
    }
    validate_review_comments(repo, options, true)?;
    println!("Review comments contract ok: {REVIEW_COMMENTS_JSON}");
    Ok(())
}

fn run_ripr_review_comments(
    repo: &Path,
    options: &ReviewCommentsOptions,
    root: &str,
) -> Result<()> {
    let out = repo.join(REVIEW_COMMENTS_JSON);
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let out_arg = out.display().to_string();
    run_ripr_with_timeout(
        &[
            "review-comments".to_string(),
            "--root".to_string(),
            root.to_string(),
            "--base".to_string(),
            options.base.clone(),
            "--head".to_string(),
            options.head.clone(),
            "--out".to_string(),
            out_arg,
        ],
        options.timeout_seconds,
    )
    .map(|_| ())
}

fn current_pr_evidence_has_no_severe_gaps(
    repo: &Path,
    options: &ReviewCommentsOptions,
) -> Result<bool> {
    let path = repo.join(PR_EVIDENCE_JSON);
    let Ok(text) = fs::read_to_string(&path) else {
        return Ok(false);
    };
    let Ok(packet) = serde_json::from_str::<Value>(&text) else {
        return Ok(false);
    };
    if packet.get("base").and_then(Value::as_str) != Some(options.base.as_str())
        || packet.get("head").and_then(Value::as_str) != Some(options.head.as_str())
    {
        return Ok(false);
    }
    let base_sha = revision_sha(repo, &options.base)?;
    let head_sha = revision_sha(repo, &options.head)?;
    let packet_pr_head = packet.get("pr_head_sha").and_then(Value::as_str);
    if packet.get("base_sha").and_then(Value::as_str) != Some(base_sha.as_str())
        || packet.get("head_sha").and_then(Value::as_str) != Some(head_sha.as_str())
        || packet_pr_head != options.pr_head_sha.as_deref()
    {
        return Ok(false);
    }
    Ok(packet.pointer("/summary/severe_gaps").and_then(Value::as_u64) == Some(0))
}

fn validate_review_comments(
    repo: &Path,
    options: &ReviewCommentsOptions,
    markdown_required: bool,
) -> Result<()> {
    let text = fs::read_to_string(repo.join(REVIEW_COMMENTS_JSON))
        .with_context(|| format!("missing or unreadable {REVIEW_COMMENTS_JSON}"))?;
    let packet: Value = serde_json::from_str(&text)
        .with_context(|| format!("{REVIEW_COMMENTS_JSON} is not valid JSON"))?;
    let mut violations = Vec::new();
    expect_string(&packet, "schema_version", "0.1", &mut violations);
    expect_string(&packet, "tool", "ripr", &mut violations);
    expect_string(&packet, "base", &options.base, &mut violations);
    expect_string(&packet, "base_sha", &revision_sha(repo, &options.base)?, &mut violations);
    expect_string(&packet, "head", &options.head, &mut violations);
    expect_string(&packet, "head_sha", &revision_sha(repo, &options.head)?, &mut violations);
    match (&options.pr_head_sha, packet.get("pr_head_sha")) {
        (Some(expected), Some(value)) => {
            expect_string_value(value, "pr_head_sha", expected, &mut violations)
        }
        (Some(_), None) => violations.push("pr_head_sha is missing".to_string()),
        (None, Some(value)) if !value.is_null() => {
            violations.push("pr_head_sha must be null when no PR head was supplied".to_string())
        }
        (None, None) => violations.push("pr_head_sha is missing".to_string()),
        (None, Some(_)) => {}
    }
    expect_string(&packet, "evaluated_head", &options.head, &mut violations);
    expect_string(
        &packet,
        "evaluated_head_sha",
        &revision_sha(repo, &options.head)?,
        &mut violations,
    );
    match packet.get("status").and_then(Value::as_str) {
        Some("advisory" | "incomplete" | "error") => {}
        Some(other) => violations.push(format!("status {other:?} is not valid")),
        None => violations.push("status is missing or not a string".to_string()),
    }
    for key in ["comments", "summary_only", "suppressed", "warnings"] {
        if !packet.get(key).is_some_and(Value::is_array) {
            violations.push(format!("{key} is missing or not an array"));
        }
    }
    if !packet.get("summary").is_some_and(Value::is_object) {
        violations.push("summary is missing or not an object".to_string());
    }
    if markdown_required && !repo.join(REVIEW_COMMENTS_MD).exists() {
        violations.push(format!("{REVIEW_COMMENTS_MD} is missing"));
    }
    if violations.is_empty() {
        Ok(())
    } else {
        bail!("review comments contract violations:\n{}", bullet_list(&violations));
    }
}

fn stamp_review_comments_receipt(repo: &Path, options: &ReviewCommentsOptions) -> Result<()> {
    let path = repo.join(REVIEW_COMMENTS_JSON);
    let text = fs::read_to_string(&path)
        .with_context(|| format!("missing or unreadable {REVIEW_COMMENTS_JSON}"))?;
    let mut packet: Value = serde_json::from_str(&text)
        .with_context(|| format!("{REVIEW_COMMENTS_JSON} is not valid JSON"))?;
    let Some(object) = packet.as_object_mut() else {
        bail!("{REVIEW_COMMENTS_JSON} is not a JSON object");
    };
    object.insert("base_sha".to_string(), json!(revision_sha(repo, &options.base)?));
    object.insert("head_sha".to_string(), json!(revision_sha(repo, &options.head)?));
    object.insert("pr_head_sha".to_string(), optional_sha_value(options.pr_head_sha.as_deref()));
    object.insert("evaluated_head".to_string(), json!(options.head));
    object.insert("evaluated_head_sha".to_string(), json!(revision_sha(repo, &options.head)?));
    write_text(&path, &format_json(&packet)?)
}

fn write_clean_review_comments(
    repo: &Path,
    options: &ReviewCommentsOptions,
    root: &str,
) -> Result<()> {
    let packet = json!({
        "schema_version": "0.1",
        "tool": "ripr",
        "status": "advisory",
        "root": normalize_path_text(root),
        "base": options.base,
        "head": options.head,
        "pr_head_sha": optional_sha_value(options.pr_head_sha.as_deref()),
        "evaluated_head": options.head,
        "evaluated_head_sha": revision_sha(repo, &options.head)?,
        "mode": "pr_evidence_clean",
        "rendering_limits": {
            "max_inline_comments": 0,
            "max_summary_items": 0
        },
        "summary": {
            "comments": 0,
            "summary_only": 0,
            "suppressed": 0,
            "unchanged_tests": true,
            "source": "pr_evidence",
            "skip_reason": "pr_evidence_zero_severe_gaps"
        },
        "comments": [],
        "summary_only": [],
        "suppressed": [],
        "warnings": [],
        "limits_note": "Review guidance generation skipped because diff-scoped PR evidence reported zero severe gaps."
    });
    write_text(&repo.join(REVIEW_COMMENTS_JSON), &format_json(&packet)?)?;
    write_text(&repo.join(REVIEW_COMMENTS_MD), &render_clean_review_comments_markdown(&packet))
}

fn render_clean_review_comments_markdown(packet: &Value) -> String {
    format!(
        "# RIPR PR Guidance\n\n- status: advisory\n- base: `{}`\n- head: `{}`\n- line annotations: 0\n- summary-only recommendations: 0\n- suppressed recommendations: 0\n\nNo review guidance was generated because diff-scoped PR evidence reported zero severe gaps.\n",
        string_field(packet, "base", DEFAULT_BASE),
        string_field(packet, "head", DEFAULT_HEAD)
    )
}

fn write_error_review_comments(
    repo: &Path,
    options: &ReviewCommentsOptions,
    root: &str,
    error: &str,
) -> Result<()> {
    let packet = json!({
        "schema_version": "0.1",
        "tool": "ripr",
        "status": "error",
        "root": normalize_path_text(root),
        "base": options.base,
        "head": options.head,
        "pr_head_sha": optional_sha_value(options.pr_head_sha.as_deref()),
        "evaluated_head": options.head,
        "evaluated_head_sha": revision_sha(repo, &options.head)?,
        "mode": "fast",
        "rendering_limits": {
            "max_inline_comments": 0,
            "max_summary_items": 0
        },
        "summary": {
            "comments": 0,
            "summary_only": 0,
            "suppressed": 0,
            "unchanged_tests": true
        },
        "comments": [],
        "summary_only": [],
        "suppressed": [],
        "warnings": [
            {
                "kind": "tool_error",
                "message": first_line(error),
                "path": null
            }
        ],
        "limits_note": "Review guidance generation is advisory. The producer did not complete, so no comments are emitted."
    });
    write_text(&repo.join(REVIEW_COMMENTS_JSON), &format_json(&packet)?)?;
    write_text(&repo.join(REVIEW_COMMENTS_MD), &render_error_review_comments_markdown(&packet))
}

fn render_error_review_comments_markdown(packet: &Value) -> String {
    let warning = packet
        .get("warnings")
        .and_then(Value::as_array)
        .and_then(|warnings| warnings.first())
        .and_then(|warning| warning.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("review guidance generation did not complete");
    format!(
        "# RIPR PR Guidance\n\n- status: error\n- base: `{}`\n- head: `{}`\n- line annotations: 0\n- summary-only recommendations: 0\n- suppressed recommendations: 0\n\nNo review guidance was generated.\n\n## Warnings\n\n- tool_error: {}\n",
        string_field(packet, "base", DEFAULT_BASE),
        string_field(packet, "head", DEFAULT_HEAD),
        md_escape(warning)
    )
}

/// Cap on synthesized fallback seam names so one large diff cannot emit an
/// unbounded receipt.
const FALLBACK_GUIDANCE_LIMIT: usize = 25;

/// Raw-check classifications the merge gate can block on. Mirrors
/// `genuine_new_ripr_gap_count` in `quality_gate.rs`, whose blocking count is
/// `reachable_unrevealed + no_static_path`.
fn gate_actionable_classification(classification: &str) -> bool {
    matches!(classification, "reachable_unrevealed" | "no_static_path")
}

/// Suggested-proof text attached to fallback seam names. The diff-scoped
/// analysis identifies the seam; only the full review-comments pass derives
/// analyzer-specific proof suggestions, so this text is deliberately generic.
fn fallback_suggested_test(classification: &str) -> &'static str {
    match classification {
        "reachable_unrevealed" => {
            "Add a focused test that executes the owner of this changed seam and asserts a discriminating value, so the reachable seam is revealed."
        }
        _ => {
            "Add a focused test that statically exercises the owner of this changed seam. If the seam is a non-executable declaration or a body the analyzer cannot trace to its covering test (ripr#1429 class), say so in the PR instead of adding proof theatre."
        }
    }
}

/// Build named seam comments from the completed diff-scoped raw check when the
/// review-comments pass itself did not finish (#10054). Returns `None` when the
/// raw check is unavailable, stale against the requested base, or has no
/// unsuppressed actionable seam at the head revision, so the caller can fall
/// back to the plain error receipt.
fn fallback_guidance_comments(
    repo: &Path,
    options: &ReviewCommentsOptions,
) -> Result<Option<(Vec<Value>, usize)>> {
    let Ok(suppressions) = read_ripr_suppression_rules(repo, Path::new(DEFAULT_RIPR_SUPPRESSIONS))
    else {
        return Ok(None);
    };
    // Best-effort head-revision and dependency-graph filters, matching the
    // producer's counted set (#6260, #11690). If the diff cannot be resolved,
    // name without them — the direction is names ⊇ counted, which stays
    // fail-closed. The non-production filter (#12267 review repair) applies
    // here too, with the same precedence as the counted set, so truncated
    // fallback guidance cannot fill up with seams the count already dropped.
    let diff_receipt = resolve_committed_diff(repo, &options.base, &options.head).ok();
    let metadata = crate::tasks::ci_scope::load_metadata(repo).ok();
    let attribution = diff_receipt
        .as_ref()
        .and_then(|diff| {
            metadata.as_ref().and_then(|metadata| {
                dependency_attribution_scope(metadata, &committed_diff_entry_paths(&diff.entries))
                    .ok()
            })
        })
        .unwrap_or(AttributionScope::NoChangedPackage);
    let production_surface = metadata
        .as_ref()
        .and_then(|metadata| production_surface_from_metadata(repo, metadata).ok());
    let head_extents =
        diff_receipt.as_ref().map(|diff| HeadLineExtents::from_committed_diff(repo, diff));

    let raw_check = fs::File::open(repo.join(PR_RAW_CHECK_JSON)).ok();
    let Some(raw_check) = raw_check else { return Ok(None) };
    let mut accumulator = FallbackGuidanceAccumulator::default();
    let payload = stream_ripr_check_payload_with_events(
        BufReader::with_capacity(64 * 1024, raw_check),
        &mut |event| match event {
            StreamFindingsEvent::Start => accumulator.reset(),
            StreamFindingsEvent::Finding(finding) => accumulator.absorb(
                finding,
                &suppressions,
                head_extents.as_ref(),
                attribution.applied(),
                production_surface.as_ref(),
            ),
        },
    );
    let Ok(payload) = payload else { return Ok(None) };
    if payload.base.as_ref().and_then(Value::as_str) != Some(options.base.as_str()) {
        return Ok(None);
    }
    // `finish` already returns the bounded, sorted, path/line-unique set; no
    // further sort or dedup is needed here.
    let (seams, suppressed) = accumulator.finish();
    let comments = seams.into_iter().map(|(_, _, _, comment)| comment).collect::<Vec<_>>();
    if comments.is_empty() {
        return Ok(None);
    }
    Ok(Some((comments, suppressed)))
}

type FallbackSeam = (String, u64, String, Value);

enum FallbackSeamDecision {
    Ignore,
    Suppressed,
    Emit(FallbackSeam),
}

/// Keeps fallback guidance bounded while preserving the deterministic first
/// `FALLBACK_GUIDANCE_LIMIT` path/line entries that the old sort/dedup/truncate
/// implementation emitted. Its entries are always sorted by `(path, line, id)`,
/// `(path, line)` keys are unique, and the result is bounded to
/// `FALLBACK_GUIDANCE_LIMIT`. A later duplicate path/line replaces the retained
/// entry only when its id sorts first, matching the old dedup order.
#[derive(Default)]
struct FallbackGuidanceAccumulator {
    seams: Vec<FallbackSeam>,
    suppressed: usize,
}

impl FallbackGuidanceAccumulator {
    fn reset(&mut self) {
        self.seams.clear();
        self.suppressed = 0;
    }

    fn absorb(
        &mut self,
        finding: &Value,
        suppressions: &RiprSuppressionRules,
        head_extents: Option<&HeadLineExtents>,
        attribution: Option<&DependencyAttribution>,
        production_surface: Option<&ProductionSurface>,
    ) {
        match fallback_seam_decision(
            finding,
            suppressions,
            head_extents,
            attribution,
            production_surface,
        ) {
            FallbackSeamDecision::Ignore => {}
            FallbackSeamDecision::Suppressed => self.suppressed += 1,
            FallbackSeamDecision::Emit(entry) => {
                let key_matches =
                    |existing: &FallbackSeam| existing.0 == entry.0 && existing.1 == entry.1;
                if let Some(existing) = self.seams.iter_mut().find(|existing| key_matches(existing))
                {
                    if entry.2 < existing.2 {
                        *existing = entry;
                    }
                    return;
                }
                self.seams.push(entry);
                self.seams.sort_by(|left, right| {
                    (&left.0, left.1, &left.2).cmp(&(&right.0, right.1, &right.2))
                });
                self.seams.truncate(FALLBACK_GUIDANCE_LIMIT);
            }
        }
    }

    /// Returns entries sorted by `(path, line, id)`, with unique `(path, line)`
    /// keys and at most `FALLBACK_GUIDANCE_LIMIT` entries.
    fn finish(self) -> (Vec<FallbackSeam>, usize) {
        (self.seams, self.suppressed)
    }
}

/// Collect the gate-actionable seams fallback guidance should name, applying
/// the same filters — and the same precedence — as `ripr_pr_summary_counts`:
/// suppression policy, head-range (#6260), structural non-production
/// classification (#11690), then dependency-graph attribution. Non-production
/// seams are dropped before sorting and truncation so the
/// [`FALLBACK_GUIDANCE_LIMIT`] slice cannot crowd out a production seam that
/// actually keeps `new_unresolved` positive.
/// This function is retained as the pre-streaming DOM oracle for the parity
/// tests and has no production caller since the streaming accumulator replaced
/// it.
#[cfg(test)]
fn fallback_seam_entries(
    findings: &[Value],
    suppressions: &RiprSuppressionRules,
    head_extents: Option<&HeadLineExtents>,
    attribution: Option<&DependencyAttribution>,
    production_surface: Option<&ProductionSurface>,
) -> (Vec<(String, u64, String, Value)>, usize) {
    let mut suppressed = 0usize;
    let mut seams: Vec<(String, u64, String, Value)> = Vec::new();
    for finding in findings {
        match fallback_seam_decision(
            finding,
            suppressions,
            head_extents,
            attribution,
            production_surface,
        ) {
            FallbackSeamDecision::Ignore => {}
            FallbackSeamDecision::Suppressed => suppressed += 1,
            FallbackSeamDecision::Emit(entry) => seams.push(entry),
        }
    }
    (seams, suppressed)
}

fn fallback_seam_decision(
    finding: &Value,
    suppressions: &RiprSuppressionRules,
    head_extents: Option<&HeadLineExtents>,
    attribution: Option<&DependencyAttribution>,
    production_surface: Option<&ProductionSurface>,
) -> FallbackSeamDecision {
    // ripr 0.5.x: "classification"; ripr 0.9.x may emit "grip_class" with
    // "weakly_gripped" folded into the counted reachable_unrevealed bucket
    // (see ripr_pr_summary_counts). Accept both and name the counted class.
    let raw_class = finding
        .get("classification")
        .and_then(Value::as_str)
        .or_else(|| finding.get("grip_class").and_then(Value::as_str));
    let canonical = match raw_class {
        Some("weakly_gripped") => "reachable_unrevealed",
        Some(other) => other,
        None => return FallbackSeamDecision::Ignore,
    };
    if !gate_actionable_classification(canonical) {
        return FallbackSeamDecision::Ignore;
    }
    if suppression_matches_finding(suppressions, finding) {
        return FallbackSeamDecision::Suppressed;
    }
    if head_extents.is_some_and(|extents| extents.finding_is_outside_head(finding)) {
        return FallbackSeamDecision::Ignore;
    }
    if ripr_finding_path(finding)
        .is_some_and(|path| classify_non_production(production_surface, &path).is_some())
    {
        return FallbackSeamDecision::Ignore;
    }
    if let Some(attribution) = attribution
        && attribution.finding_is_out_of_graph(finding)
    {
        return FallbackSeamDecision::Ignore;
    }
    let Some(file) = ripr_finding_path(finding) else { return FallbackSeamDecision::Ignore };
    let path = normalize_suppression_match_path(&file);
    // Without a known anchor the normalized value is still an absolute host
    // path; never emit CI-runner paths into receipts.
    if path.starts_with('/') || path.as_bytes().get(1) == Some(&b':') {
        return FallbackSeamDecision::Ignore;
    }
    let Some(line) = ripr_finding_line(finding) else { return FallbackSeamDecision::Ignore };
    let id = finding
        .get("id")
        .and_then(Value::as_str)
        .or_else(|| finding.pointer("/probe/id").and_then(Value::as_str))
        .or_else(|| finding.pointer("/seam/id").and_then(Value::as_str))
        .unwrap_or("unknown-seam");
    let family = finding
        .pointer("/probe/family")
        .and_then(Value::as_str)
        .or_else(|| finding.pointer("/seam/family").and_then(Value::as_str))
        .unwrap_or(canonical);
    let expression = finding
        .pointer("/probe/expression")
        .and_then(Value::as_str)
        .or_else(|| finding.pointer("/seam/expression").and_then(Value::as_str))
        .unwrap_or("");
    let reach_summary = finding
        .pointer("/ripr/reach/summary")
        .and_then(Value::as_str)
        .unwrap_or("no static test path found");
    let comment = json!({
        "id": id,
        "path": path,
        "line": line,
        "seam": format!("{family}: {}", first_line(expression)),
        "reason": format!("{canonical}: {reach_summary}"),
        "suggested_test": fallback_suggested_test(canonical),
    });
    FallbackSeamDecision::Emit((path, line, id.to_string(), comment))
}

/// Emit an `incomplete` guidance receipt that names the gate-actionable seams
/// from the completed diff-scoped analysis when the review-comments pass did
/// not finish (#10054). The gate may then block on named evidence it has —
/// file, line, seam, reason — instead of an unnamed count.
fn write_fallback_review_comments(
    repo: &Path,
    options: &ReviewCommentsOptions,
    root: &str,
    error: &str,
    comments: &[Value],
    suppressed: usize,
) -> Result<()> {
    let packet = json!({
        "schema_version": "0.1",
        "tool": "ripr",
        "status": "incomplete",
        "root": normalize_path_text(root),
        "base": options.base,
        "head": options.head,
        "pr_head_sha": optional_sha_value(options.pr_head_sha.as_deref()),
        "evaluated_head": options.head,
        "evaluated_head_sha": revision_sha(repo, &options.head)?,
        "mode": "pr_evidence_fallback",
        "rendering_limits": {
            "max_inline_comments": 0,
            "max_summary_items": FALLBACK_GUIDANCE_LIMIT
        },
        "summary": {
            "comments": 0,
            "summary_only": comments.len(),
            "suppressed": suppressed,
            "unchanged_tests": true,
            "source": "raw_check_fallback"
        },
        "comments": [],
        "summary_only": comments,
        "suppressed": [],
        "warnings": [
            {
                "kind": "tool_error",
                "message": first_line(error),
                "path": null
            },
            {
                "kind": "guidance_fallback",
                "message": "review-comments pass did not complete; seam names were synthesized from the completed diff-scoped ripr check. Suggested-proof text is generic, not analyzer-derived.",
                "path": null
            }
        ],
        "limits_note": "Review guidance generation is advisory. The producer did not complete, so seam names come from the completed diff-scoped analysis rather than the guidance pass."
    });
    write_text(&repo.join(REVIEW_COMMENTS_JSON), &format_json(&packet)?)?;
    write_text(&repo.join(REVIEW_COMMENTS_MD), &render_fallback_review_comments_markdown(&packet))
}

fn write_degraded_review_comments(
    repo: &Path,
    options: &ReviewCommentsOptions,
    root: &str,
    error: &str,
) -> Result<()> {
    match fallback_guidance_comments(repo, options)? {
        Some((comments, suppressed)) => {
            write_fallback_review_comments(repo, options, root, error, &comments, suppressed)
        }
        None => write_error_review_comments(repo, options, root, error),
    }
}

fn render_fallback_review_comments_markdown(packet: &Value) -> String {
    let mut markdown = format!(
        "# RIPR PR Guidance\n\n- status: incomplete\n- base: `{}`\n- head: `{}`\n- line annotations: 0\n- summary-only recommendations: {}\n- suppressed recommendations: {}\n\nThe review-comments pass did not complete; the seam names below come from the completed diff-scoped ripr check.\n",
        string_field(packet, "base", DEFAULT_BASE),
        string_field(packet, "head", DEFAULT_HEAD),
        packet.pointer("/summary/summary_only").and_then(Value::as_u64).unwrap_or(0),
        packet.pointer("/summary/suppressed").and_then(Value::as_u64).unwrap_or(0),
    );
    if let Some(items) = packet.get("summary_only").and_then(Value::as_array) {
        markdown.push_str("\n## Named seams (fallback)\n\n");
        for item in items {
            markdown.push_str(&format!(
                "- `{}:{}` {} — {}\n",
                string_field(item, "path", "<unknown>"),
                item.get("line").and_then(Value::as_u64).unwrap_or(0),
                md_escape(string_field(item, "seam", "<unknown seam>")),
                md_escape(string_field(item, "reason", "<no reason>")),
            ));
        }
    }
    if let Some(warning) = packet
        .get("warnings")
        .and_then(Value::as_array)
        .and_then(|warnings| warnings.first())
        .and_then(|warning| warning.get("message"))
        .and_then(Value::as_str)
    {
        markdown.push_str(&format!("\n## Warnings\n\n- tool_error: {}\n", md_escape(warning)));
    }
    markdown
}

fn render_pr_evidence_summary(repo: &Path) -> String {
    let pr_evidence = load_json(repo, PR_EVIDENCE_JSON);
    let review_comments = load_json(repo, REVIEW_COMMENTS_JSON);
    let impacted = load_json(repo, IMPACTED_JSON);
    let pr_value = pr_evidence.value.as_ref();
    let review_value = review_comments.value.as_ref();
    let impacted_value = impacted.value.as_ref();
    let mut out = String::new();
    out.push_str("# PR Evidence Summary\n\n");
    out.push_str("## Fast Gate\n\n");
    out.push_str(&format!("- PR evidence JSON: {}\n", pr_evidence.state));
    out.push_str(&format!("- review guidance JSON: {}\n", review_comments.state));
    out.push_str(&format!("- impacted evidence JSON: {}\n", impacted.state));
    out.push_str(&format!("- PR evidence status: `{}`\n", option_string_field(pr_value, "status")));
    out.push_str(&format!(
        "- review guidance status: `{}`\n",
        option_string_field(review_value, "status")
    ));
    out.push_str(&format!("- base: `{}`\n", option_string_field(pr_value, "base")));
    out.push_str(&format!("- head: `{}`\n", option_string_field(pr_value, "head")));
    out.push_str(&format!(
        "- changed files: {}\n\n",
        option_summary_u64(pr_value, "changed_files")
    ));
    out.push_str("## RIPR\n\n");
    out.push_str(&format!(
        "- changed-line comments: {}\n",
        option_summary_u64(review_value.or(pr_value), "comments")
    ));
    out.push_str(&format!(
        "- summary-only guidance: {}\n",
        option_summary_u64(review_value.or(pr_value), "summary_only")
    ));
    out.push_str(&format!(
        "- suppressed guidance: {}\n",
        option_summary_u64(review_value.or(pr_value), "suppressed")
    ));
    out.push_str(&format!(
        "- weakly_exposed: {}\n",
        option_summary_u64(pr_value, "weakly_exposed")
    ));
    out.push_str(&format!(
        "- reachable_unrevealed: {}\n",
        option_summary_u64(pr_value, "reachable_unrevealed")
    ));
    out.push_str(&format!(
        "- no_static_path: {}\n",
        option_summary_u64(pr_value, "no_static_path")
    ));
    out.push_str(&format!("- severe gaps: {}\n\n", option_summary_u64(pr_value, "severe_gaps")));
    out.push_str("## Targeted Mutation\n\n");
    out.push_str(&format!(
        "- mutation_mode: `{}`\n",
        option_summary_string(impacted_value, "mutation_mode")
    ));
    out.push_str(&format!(
        "- requires_targeted_mutation: {}\n",
        option_summary_bool(impacted_value.or(pr_value), "requires_targeted_mutation")
    ));
    out.push_str(&format!(
        "- ripr_severe_gap: {}\n",
        option_summary_bool(impacted_value.or(pr_value), "ripr_severe_gap")
    ));
    out.push_str(&format!(
        "- routing_reason: `{}`\n\n",
        option_summary_string_or_null(impacted_value.or(pr_value), "routing_reason")
    ));
    out.push_str("## Artifacts\n\n");
    out.push_str("| Artifact | Path | State |\n");
    out.push_str("| --- | --- | --- |\n");
    out.push_str(&format!("| PR evidence JSON | `{PR_EVIDENCE_JSON}` | {} |\n", pr_evidence.state));
    out.push_str(&format!(
        "| PR evidence Markdown | `{PR_EVIDENCE_MD}` | {} |\n",
        file_state(repo, PR_EVIDENCE_MD)
    ));
    out.push_str(&format!(
        "| Review guidance JSON | `{REVIEW_COMMENTS_JSON}` | {} |\n",
        review_comments.state
    ));
    out.push_str(&format!(
        "| Review guidance Markdown | `{REVIEW_COMMENTS_MD}` | {} |\n",
        file_state(repo, REVIEW_COMMENTS_MD)
    ));
    out.push_str(&format!("| Impacted evidence JSON | `{IMPACTED_JSON}` | {} |\n", impacted.state));
    out.push_str(&format!(
        "| Impacted evidence Markdown | `{IMPACTED_MD}` | {} |\n",
        file_state(repo, IMPACTED_MD)
    ));
    out.push_str(&format!("| PR evidence summary Markdown | `{PR_SUMMARY_MD}` | generated |\n"));
    out.push_str("\n_This summary is generated from diff-scoped artifacts. Do not copy it into public badge state._\n");
    out
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct JsonInput {
    state: InputState,
    value: Option<Value>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum InputState {
    Present,
    Missing,
    Invalid(String),
}

impl std::fmt::Display for InputState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Present => f.write_str("present"),
            Self::Missing => f.write_str("missing"),
            Self::Invalid(err) => write!(f, "invalid: {}", md_escape(err)),
        }
    }
}

fn load_json(repo: &Path, relative: &str) -> JsonInput {
    let path = repo.join(relative);
    let Ok(text) = fs::read_to_string(&path) else {
        return JsonInput { state: InputState::Missing, value: None };
    };
    match serde_json::from_str::<Value>(&text) {
        Ok(value) => JsonInput { state: InputState::Present, value: Some(value) },
        Err(err) => {
            JsonInput { state: InputState::Invalid(first_line(&err.to_string())), value: None }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct AnnotationOutput {
    text: String,
    comments_missing: bool,
}

fn render_annotations(repo: &Path, comments: &str) -> Result<AnnotationOutput> {
    let comments_path = repo.join(comments);
    if !comments_path.exists() {
        return Ok(AnnotationOutput { text: String::new(), comments_missing: true });
    }
    let text =
        fs::read_to_string(&comments_path).with_context(|| format!("failed to read {comments}"))?;
    let packet: Value =
        serde_json::from_str(&text).with_context(|| format!("{comments} is not valid JSON"))?;
    let comments_array = packet
        .get("comments")
        .and_then(Value::as_array)
        .ok_or_else(|| eyre!("{comments} is missing comments[]"))?;
    let mut out = String::new();
    for item in comments_array {
        out.push_str(&annotation_from_comment(item)?);
        out.push('\n');
    }
    Ok(AnnotationOutput { text: out, comments_missing: false })
}

fn annotation_from_comment(item: &Value) -> Result<String> {
    let placement = item
        .get("placement")
        .and_then(Value::as_object)
        .ok_or_else(|| eyre!("comments[] item is missing placement object"))?;
    let path = string_key(placement, "path")?;
    let line = placement
        .get("line")
        .and_then(Value::as_u64)
        .ok_or_else(|| eyre!("comments[] placement.line is missing or not an integer"))?;
    let mode = string_key(placement, "mode")?;
    if !matches!(
        mode.as_str(),
        "exact_seam_line" | "owner_function_changed_line" | "same_file_changed_line"
    ) {
        bail!("comments[] placement mode {mode:?} is not annotation-safe");
    }
    let severity = item.get("severity").and_then(Value::as_str).unwrap_or("advisory");
    let kind = item.get("kind").and_then(Value::as_str).unwrap_or("focused_test");
    let reason = item.get("reason").and_then(Value::as_str).unwrap_or("RIPR review guidance");
    let intent =
        item.get("suggested_test").and_then(|test| test.get("intent")).and_then(Value::as_str);
    let mut message = reason.to_string();
    if let Some(intent) = intent {
        message.push_str(" Suggested test: ");
        message.push_str(intent);
    }
    Ok(format!(
        "::warning file={},line={},title={}::{}",
        escape_cmd(&path),
        line,
        escape_cmd(&format!("ripr {severity} {kind}")),
        escape_cmd(&message)
    ))
}

fn string_key(object: &Map<String, Value>, key: &str) -> Result<String> {
    object
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .ok_or_else(|| eyre!("comments[] placement.{key} is missing or empty"))
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ImpactedEvidenceOptions {
    pr_evidence: String,
    labels: Vec<String>,
}

fn impacted_evidence_packet(repo: &Path, options: &ImpactedEvidenceOptions) -> Value {
    let input = load_pr_evidence(repo, &options.pr_evidence);
    let ripr_severe_gap = input
        .value
        .as_ref()
        .and_then(|value| value.pointer("/summary/ripr_severe_gap"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let pr_requires_targeted = input
        .value
        .as_ref()
        .and_then(|value| value.pointer("/summary/requires_targeted_mutation"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let decision = routing_decision(&options.labels, ripr_severe_gap || pr_requires_targeted);
    let warnings = input.warning(&options.pr_evidence);
    json!({
        "schema_version": "0.1",
        "tool": "ripr",
        "kind": "impacted_evidence",
        "scope": "diff",
        "status": if warnings.is_empty() { "advisory" } else { "incomplete" },
        "inputs": {
            "pr_evidence": options.pr_evidence,
            "labels": options.labels
        },
        "summary": {
            "mutation_mode": decision.mode,
            "requires_targeted_mutation": decision.requires_targeted_mutation,
            "requires_full_owner_mutation": decision.requires_full_owner_mutation,
            "ripr_severe_gap": ripr_severe_gap,
            "routing_reason": decision.reason
        },
        "artifacts": [
            {
                "label": "impacted evidence JSON",
                "path": IMPACTED_JSON,
                "kind": "json",
                "scope": "diff",
                "available": true,
                "required": true
            },
            {
                "label": "impacted evidence Markdown",
                "path": IMPACTED_MD,
                "kind": "markdown",
                "scope": "diff",
                "available": true
            },
            {
                "label": "PR evidence JSON",
                "path": options.pr_evidence,
                "kind": "json",
                "scope": "diff",
                "available": input.value.is_some()
            }
        ],
        "warnings": warnings,
        "advisory_limits": [
            "Impacted evidence routes mutation; it does not execute mutation.",
            "Full-owner mutation requires an explicit mutation/full-owner label."
        ]
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RoutingDecision {
    mode: &'static str,
    requires_targeted_mutation: bool,
    requires_full_owner_mutation: bool,
    reason: Value,
}

fn routing_decision(labels: &[String], ripr_routes_targeted: bool) -> RoutingDecision {
    let has = |needle: &str| labels.iter().any(|label| label == needle);
    if has("mutation/full-owner") {
        return RoutingDecision {
            mode: "full_owner",
            requires_targeted_mutation: false,
            requires_full_owner_mutation: true,
            reason: json!("mutation/full-owner label"),
        };
    }
    if has("mutation") || has("mutation/targeted") {
        return targeted("mutation label");
    }
    if has("release-risk") {
        return targeted("release-risk label");
    }
    if ripr_routes_targeted {
        return targeted("ripr severe gap");
    }
    RoutingDecision {
        mode: "fast_only",
        requires_targeted_mutation: false,
        requires_full_owner_mutation: false,
        reason: Value::Null,
    }
}

fn targeted(reason: &'static str) -> RoutingDecision {
    RoutingDecision {
        mode: "targeted",
        requires_targeted_mutation: true,
        requires_full_owner_mutation: false,
        reason: json!(reason),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PrEvidenceInput {
    value: Option<Value>,
    state: InputState,
}

impl PrEvidenceInput {
    fn warning(&self, path: &str) -> Vec<Value> {
        match &self.state {
            InputState::Present => Vec::new(),
            InputState::Missing => vec![json!({
                "kind": "missing_artifact",
                "message": "PR evidence JSON is missing; mutation routing uses labels only.",
                "path": path
            })],
            InputState::Invalid(err) => vec![json!({
                "kind": "invalid_json",
                "message": format!("PR evidence JSON is invalid: {err}"),
                "path": path
            })],
        }
    }
}

fn load_pr_evidence(repo: &Path, relative: &str) -> PrEvidenceInput {
    let input = load_json(repo, relative);
    PrEvidenceInput { value: input.value, state: input.state }
}

fn render_impacted_evidence_markdown(packet: &Value) -> String {
    let summary = packet.get("summary").and_then(Value::as_object);
    let inputs = packet.get("inputs").and_then(Value::as_object);
    let labels = inputs
        .and_then(|inputs| inputs.get("labels"))
        .and_then(Value::as_array)
        .map(|labels| labels.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", "))
        .filter(|labels| !labels.is_empty())
        .unwrap_or_else(|| "none".to_string());
    let mut out = String::new();
    out.push_str("# Impacted Evidence\n\n");
    out.push_str("## Routing\n\n");
    out.push_str(&format!(
        "- mutation_mode: `{}`\n",
        summary_string(summary, "mutation_mode", "unknown")
    ));
    out.push_str(&format!(
        "- requires_targeted_mutation: {}\n",
        bool_field(summary, "requires_targeted_mutation")
    ));
    out.push_str(&format!(
        "- requires_full_owner_mutation: {}\n",
        bool_field(summary, "requires_full_owner_mutation")
    ));
    out.push_str(&format!("- ripr_severe_gap: {}\n", bool_field(summary, "ripr_severe_gap")));
    out.push_str(&format!(
        "- routing_reason: `{}`\n\n",
        summary_string_or_null(summary, "routing_reason")
    ));
    out.push_str("## Inputs\n\n");
    out.push_str(&format!(
        "- PR evidence: `{}`\n",
        inputs
            .and_then(|inputs| inputs.get("pr_evidence"))
            .and_then(Value::as_str)
            .map(md_escape)
            .unwrap_or_else(|| "not_available".to_string())
    ));
    out.push_str(&format!("- labels: `{}`\n\n", md_escape(&labels)));
    out.push_str("## Artifacts\n\n");
    out.push_str("| Artifact | Path | Available |\n");
    out.push_str("| --- | --- | --- |\n");
    if let Some(artifacts) = packet.get("artifacts").and_then(Value::as_array) {
        for artifact in artifacts {
            out.push_str(&format!(
                "| {} | `{}` | {} |\n",
                md_escape(string_field(artifact, "label", "artifact")),
                md_escape(string_field(artifact, "path", "unknown")),
                artifact.get("available").and_then(Value::as_bool).unwrap_or(false)
            ));
        }
    }
    if let Some(warnings) = packet.get("warnings").and_then(Value::as_array)
        && !warnings.is_empty()
    {
        out.push_str("\n## Warnings\n\n");
        for warning in warnings {
            out.push_str(&format!(
                "- {}: {}\n",
                md_escape(string_field(warning, "kind", "warning")),
                md_escape(string_field(warning, "message", "unknown warning"))
            ));
        }
    }
    out.push_str("\n_This receipt routes verification work. It does not execute mutation._\n");
    out
}

fn merged_labels(labels: &[String], labels_csv: Option<&str>) -> Vec<String> {
    let mut all = Vec::new();
    all.extend(labels.iter().cloned());
    if let Some(csv) = labels_csv {
        all.extend(split_labels(csv));
    }
    if all.is_empty() {
        all.extend(labels_from_env());
    }
    normalize_labels(&all)
}

fn labels_from_env() -> Vec<String> {
    env::var("GITHUB_PR_LABELS")
        .or_else(|_| env::var("PR_LABELS"))
        .map(|labels| split_labels(&labels))
        .unwrap_or_default()
}

fn split_labels(labels: &str) -> Vec<String> {
    labels
        .split([',', '\n', ';'])
        .map(str::trim)
        .filter(|label| !label.is_empty())
        .map(str::to_string)
        .collect()
}

fn normalize_labels(labels: &[String]) -> Vec<String> {
    labels
        .iter()
        .map(|label| label.trim().to_ascii_lowercase())
        .filter(|label| !label.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn verify_revision(repo: &Path, rev: &str) -> Result<()> {
    let commit = format!("{rev}^{{commit}}");
    run_git_output(repo, &["rev-parse", "--verify", commit.as_str()])
        .map(|_| ())
        .with_context(|| format!("bad base/head revision {rev:?}"))
}

#[cfg(test)]
fn changed_files(repo: &Path, base: &str, head: &str) -> Result<Vec<String>> {
    Ok(resolve_committed_diff(repo, base, head)?.changed_paths)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct CommittedDiffEntry {
    status: String,
    old_path: Option<String>,
    new_path: Option<String>,
}

/// Every path a diff entry touches. For renames and copies that is both
/// endpoints: a file moving from package A to unrelated package B still
/// removed content from A, so A and its dependent closure must stay
/// attributed instead of collapsing into `out_of_dependency_graph` (#11690).
fn committed_diff_entry_paths(entries: &[CommittedDiffEntry]) -> Vec<String> {
    entries
        .iter()
        .flat_map(|entry| [entry.old_path.as_deref(), entry.new_path.as_deref()])
        .flatten()
        .map(str::to_owned)
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct CommittedDiffReceipt {
    schema_version: String,
    base: String,
    head: String,
    base_sha: String,
    head_sha: String,
    diff_digest: String,
    changed_paths: Vec<String>,
    entries: Vec<CommittedDiffEntry>,
}

fn resolve_committed_diff(repo: &Path, base: &str, head: &str) -> Result<CommittedDiffReceipt> {
    let resolved = change_set::resolve_change_set(
        ArtifactIdentity::CommitRange { base: base.to_string(), head: head.to_string() },
        repo,
    )
    .with_context(|| merge_base_failure_guidance(repo, base, head))?;
    let (resolved_base, resolved_head) = match resolved.identity {
        ArtifactIdentity::CommitRange { base, head } => (base, head),
        ArtifactIdentity::StagedTree { .. } => {
            return Err(eyre!("committed diff resolver returned a staged-tree identity"));
        }
    };
    let base_sha = resolved.base_sha.ok_or_else(|| eyre!("committed diff has no base SHA"))?;
    let head_sha = resolved.head_sha.ok_or_else(|| eyre!("committed diff has no head SHA"))?;
    let range = format!("{resolved_base}...{resolved_head}");
    let raw = run_git_bytes(
        repo,
        &[
            "diff",
            "--name-status",
            "-z",
            "--find-renames",
            "--find-copies",
            "--find-copies-harder",
            "--diff-filter=ACDMRT",
            range.as_str(),
        ],
    )
    .with_context(|| merge_base_failure_guidance(repo, base, head))?;
    let entries = parse_name_status_z(&raw)?;
    let changed_paths = entries
        .iter()
        .flat_map(|entry| [entry.old_path.as_deref(), entry.new_path.as_deref()])
        .flatten()
        .map(str::to_string)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let mut hasher = Sha256::new();
    hasher.update(base_sha.as_bytes());
    hasher.update([0]);
    hasher.update(head_sha.as_bytes());
    hasher.update(serde_json::to_vec(&entries)?);
    let diff_digest = hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect();
    Ok(CommittedDiffReceipt {
        schema_version: "ripr_committed_diff.v1".to_string(),
        base: resolved_base,
        head: resolved_head,
        base_sha,
        head_sha,
        diff_digest,
        changed_paths,
        entries,
    })
}

fn parse_name_status_z(raw: &[u8]) -> Result<Vec<CommittedDiffEntry>> {
    let fields = raw.split(|byte| *byte == 0).filter(|field| !field.is_empty()).collect::<Vec<_>>();
    let mut entries = Vec::new();
    let mut index = 0;
    while index < fields.len() {
        let status = String::from_utf8(fields[index].to_vec())
            .context("git name-status output contained a non-UTF-8 status")?;
        let code = status.as_bytes().first().copied().unwrap_or_default() as char;
        let path = |field: &[u8]| {
            String::from_utf8(field.to_vec())
                .context("git name-status output contained a non-UTF-8 path")
        };
        match code {
            'R' | 'C' => {
                let old_path =
                    fields.get(index + 1).ok_or_else(|| eyre!("missing old path for {status}"))?;
                let new_path =
                    fields.get(index + 2).ok_or_else(|| eyre!("missing new path for {status}"))?;
                entries.push(CommittedDiffEntry {
                    status,
                    old_path: Some(path(old_path)?),
                    new_path: Some(path(new_path)?),
                });
                index += 3;
            }
            'A' | 'D' | 'M' | 'T' => {
                let value =
                    fields.get(index + 1).ok_or_else(|| eyre!("missing path for {status}"))?;
                let value = path(value)?;
                entries.push(CommittedDiffEntry {
                    old_path: (code == 'D' || code == 'M' || code == 'T').then_some(value.clone()),
                    new_path: (code != 'D').then_some(value),
                    status,
                });
                index += 2;
            }
            other => bail!("unsupported git name-status code {other:?} in {status:?}"),
        }
    }
    Ok(entries)
}

/// Actionable guidance for a committed-diff range that could not be resolved.
///
/// Interpreting an absent merge base is the shared ancestry authority's job, not
/// RIPR's. A shallow, partial, or object-incomplete checkout cannot prove that
/// two refs are unrelated, so this message reports the typed `not_proven_*`
/// disposition instead of asserting absent history (#10304).
fn merge_base_failure_guidance(repo: &Path, base: &str, head: &str) -> String {
    ancestry_failure_guidance(base, head, &classify_ancestry(repo, base, head))
}

/// Pure projection of one ancestry receipt into RIPR-specific operator guidance.
///
/// Only [`AncestryDisposition::Unrelated`] — which the classifier reaches solely
/// from a complete-enough local graph — may state that two refs share no common
/// history.
fn ancestry_failure_guidance(base: &str, head: &str, receipt: &AncestryReceipt) -> String {
    let mut message = format!(
        "cannot resolve diff range `{base}...{head}`: git ancestry is `{}` ({}).",
        receipt.disposition.as_str(),
        receipt.reason
    );
    match receipt.disposition {
        // The fetch remedies below deliberately carry no refspec operand. RIPR's
        // base is normally a remote-tracking name such as `origin/main`, and
        // `git fetch origin origin/main` fails with `couldn't find remote ref
        // origin/main` because the operand is resolved in the remote namespace.
        // Fetching the remote's configured refspec is both valid and sufficient.
        AncestryDisposition::NotProvenShallow => message.push_str(&format!(
            " A shallow checkout cannot prove whether `{base}` and `{head}` share history. \
             Deepen the clone before running diff-scoped RIPR locally, e.g. \
             `git fetch --unshallow` or `git fetch --deepen=200 origin`. \
             CI is unaffected: the RIPR workflow checks out with fetch-depth: 0."
        )),
        AncestryDisposition::NotProvenPartialClone => message.push_str(
            " A partial/promisor checkout can omit the objects this range needs. \
             Materialize the required commit graph, e.g. `git fetch --refetch origin`, \
             before running diff-scoped RIPR locally. \
             CI is unaffected: the RIPR workflow checks out with fetch-depth: 0.",
        ),
        AncestryDisposition::NotProvenMissingObject => {
            // Name the side the receipt actually found missing; "the requested
            // revision" is useless when only one of the two failed to resolve.
            let missing = match (receipt.base_object_exists, receipt.head_object_exists) {
                (false, true) => format!("`{base}`"),
                (true, false) => format!("`{head}`"),
                _ => format!("`{base}` and `{head}`"),
            };
            message.push_str(&format!(
                " {missing} could not be resolved locally, which does not establish that \
                 `{base}` and `{head}` are unrelated. Confirm the spelling, then materialize \
                 the missing objects: `git fetch origin` covers the remote's configured \
                 refspec, and a revision outside that refspec must be requested by its \
                 remote-side name."
            ));
        }
        AncestryDisposition::Unrelated => message.push_str(&format!(
            " Both commit objects are present in a complete local graph and share no \
             common history, so `{base}...{head}` has no diff range to compute. \
             Select a base that shares history with `{head}`."
        )),
        AncestryDisposition::Ancestor | AncestryDisposition::Diverged => {
            message.push_str(&format!(
                " `{base}` and `{head}` are related in this checkout, so ancestry does not \
             explain the failure; inspect the underlying git error above."
            ))
        }
        AncestryDisposition::InvalidInput => message.push_str(
            " Check the base and head revision values passed to the diff-scoped command.",
        ),
        AncestryDisposition::InstrumentFailure => message.push_str(
            " Git could not be inspected, so no ancestry conclusion is available for this range.",
        ),
    }
    for limitation in &receipt.limitations {
        message.push_str(&format!(" Limitation: {limitation}."));
    }
    message
}

fn write_pr_diff(repo: &Path, receipt: &CommittedDiffReceipt) -> Result<()> {
    let range = format!("{}...{}", receipt.base, receipt.head);
    let diff = run_git_output(repo, &["diff", "--binary", "--no-ext-diff", range.as_str()])?;
    write_text(&repo.join(PR_DIFF), &diff)?;
    let receipt_json = format_json(&serde_json::to_value(receipt)?)?;
    write_text(&repo.join(PR_DIFF_RECEIPT), &receipt_json)
}

fn run_git_bytes(repo: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let output = git_output_with_mount_root(repo, args, default_windows_drive_mount_root())?;
    if !output.status.success() {
        bail!(
            "git {} failed with status {}\nstderr:\n{}",
            args.join(" "),
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(output.stdout)
}

fn run_git_output(repo: &Path, args: &[&str]) -> Result<String> {
    let output = git_output_with_mount_root(repo, args, default_windows_drive_mount_root())?;
    output_to_string("git", output)
}

fn run_ripr(args: &[String]) -> Result<String> {
    let binary = ripr_binary()?;
    run_output(&binary, args)
}

fn run_ripr_with_timeout(args: &[String], timeout_seconds: Option<u64>) -> Result<String> {
    let binary = ripr_binary()?;
    match timeout_seconds {
        Some(seconds) => run_output_with_timeout(&binary, args, Duration::from_secs(seconds)),
        None => run_output(&binary, args),
    }
}

fn ripr_binary() -> Result<String> {
    #[cfg(test)]
    {
        let guard =
            RIPR_BIN_OVERRIDE.lock().map_err(|_| eyre!("RIPR_BIN test override lock poisoned"))?;
        if let Some(binary) = guard.as_ref() {
            return Ok(binary.clone());
        }
    }

    let binary = match env::var("RIPR_BIN") {
        Ok(value) if !value.trim().is_empty() => value,
        Ok(_) => bail!("RIPR_BIN is set but empty"),
        Err(_) => "ripr".to_string(),
    };
    Ok(binary)
}

fn run_output(cmd: &str, args: &[String]) -> Result<String> {
    // Keep machine-readable RIPR output off a Windows pipe.  A child can fail with
    // ERROR_INVALID_PARAMETER (87) while performing one large stdout write even when
    // the parent drains that pipe incrementally.  A regular temporary file removes
    // the pipe-size limit and keeps this transport independent of RIPR subcommand
    // support for an --out flag.
    let stdout_file =
        tempfile::NamedTempFile::new().context("failed to create RIPR stdout file")?;
    let mut child = Command::new(cmd)
        .args(args)
        .stdout(Stdio::from(stdout_file.reopen().context("failed to reopen RIPR stdout file")?))
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to run {cmd}"))?;
    let mut stderr_bytes = Vec::new();
    if let Some(mut stderr) = child.stderr.take() {
        std::io::Read::read_to_end(&mut stderr, &mut stderr_bytes)
            .with_context(|| format!("failed to read {cmd} stderr"))?;
    }
    let status = child.wait().with_context(|| format!("failed to wait for {cmd}"))?;
    let mut stdout_reader =
        stdout_file.reopen().with_context(|| format!("failed to reopen {cmd} stdout file"))?;
    let mut stdout_bytes = Vec::new();
    stdout_reader
        .read_to_end(&mut stdout_bytes)
        .with_context(|| format!("failed to read {cmd} stdout file"))?;
    if !status.success() {
        bail!(
            "{cmd} failed with status {}\nstdout:\n{}\nstderr:\n{}",
            status,
            String::from_utf8_lossy(&stdout_bytes).trim(),
            String::from_utf8_lossy(&stderr_bytes).trim()
        );
    }
    String::from_utf8(stdout_bytes).with_context(|| format!("{cmd} stdout was not UTF-8"))
}

fn run_output_with_timeout(cmd: &str, args: &[String], timeout: Duration) -> Result<String> {
    let mut child = Command::new(cmd)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to run {cmd}"))?;
    let started = Instant::now();
    loop {
        if child.try_wait().with_context(|| format!("failed to poll {cmd}"))?.is_some() {
            let output =
                child.wait_with_output().with_context(|| format!("failed to collect {cmd}"))?;
            return output_to_string(cmd, output);
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let output =
                child.wait_with_output().with_context(|| format!("failed to collect {cmd}"))?;
            bail!(
                "{cmd} timed out after {}s\nstdout:\n{}\nstderr:\n{}",
                timeout.as_secs(),
                String::from_utf8_lossy(&output.stdout).trim(),
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn output_to_string(cmd: &str, output: std::process::Output) -> Result<String> {
    if !output.status.success() {
        bail!(
            "{cmd} failed with status {}\nstdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout).trim(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    String::from_utf8(output.stdout).with_context(|| format!("{cmd} stdout was not UTF-8"))
}

fn command_root_arg(repo: &Path, root: &str) -> Result<String> {
    let repo = repo
        .canonicalize()
        .with_context(|| format!("failed to resolve repository root {}", repo.display()))?;
    let root_path = Path::new(root);
    let candidate =
        if root_path.is_absolute() { root_path.to_path_buf() } else { repo.join(root_path) };
    let canonical = candidate
        .canonicalize()
        .with_context(|| format!("failed to resolve RIPR root {}", candidate.display()))?;
    if !canonical.starts_with(&repo) {
        bail!(
            "RIPR root {} resolves outside repository root {}",
            canonical.display(),
            repo.display()
        );
    }
    Ok(canonical.display().to_string())
}

fn write_text(path: &Path, text: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    fs::write(path, text).with_context(|| format!("failed to write {}", path.display()))
}

fn format_json(value: &Value) -> Result<String> {
    Ok(format!("{}\n", serde_json::to_string_pretty(value)?))
}

fn expect_string(packet: &Value, key: &str, expected: &str, violations: &mut Vec<String>) {
    match packet.get(key).and_then(Value::as_str) {
        Some(actual) if actual == expected => {}
        Some(actual) => violations.push(format!("{key} is {actual:?}, expected {expected:?}")),
        None => violations.push(format!("{key} is missing or not a string")),
    }
}

fn expect_string_value(value: &Value, key: &str, expected: &str, violations: &mut Vec<String>) {
    match value.as_str() {
        Some(actual) if actual == expected => {}
        Some(actual) => violations.push(format!("{key} is {actual:?}, expected {expected:?}")),
        None => violations.push(format!("{key} is missing or not a string")),
    }
}

fn count_field(summary: Option<&Map<String, Value>>, key: &str) -> usize {
    summary
        .and_then(|summary| summary.get(key))
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(0)
}

fn bool_field(summary: Option<&Map<String, Value>>, key: &str) -> bool {
    summary.and_then(|summary| summary.get(key)).and_then(Value::as_bool).unwrap_or(false)
}

fn string_field<'a>(packet: &'a Value, key: &str, fallback: &'a str) -> &'a str {
    packet.get(key).and_then(Value::as_str).unwrap_or(fallback)
}

fn option_string_field(value: Option<&Value>, key: &str) -> String {
    value
        .and_then(|value| value.get(key))
        .and_then(Value::as_str)
        .map(md_escape)
        .unwrap_or_else(|| "not_available".to_string())
}

fn option_summary_u64(value: Option<&Value>, key: &str) -> String {
    value
        .and_then(|value| value.get("summary"))
        .and_then(|summary| summary.get(key))
        .and_then(Value::as_u64)
        .map(|value| value.to_string())
        .unwrap_or_else(|| "not_available".to_string())
}

fn option_summary_bool(value: Option<&Value>, key: &str) -> String {
    value
        .and_then(|value| value.get("summary"))
        .and_then(|summary| summary.get(key))
        .and_then(Value::as_bool)
        .map(|value| value.to_string())
        .unwrap_or_else(|| "not_available".to_string())
}

fn option_summary_string(value: Option<&Value>, key: &str) -> String {
    value
        .and_then(|value| value.get("summary"))
        .and_then(|summary| summary.get(key))
        .and_then(Value::as_str)
        .map(md_escape)
        .unwrap_or_else(|| "not_available".to_string())
}

fn option_summary_string_or_null(value: Option<&Value>, key: &str) -> String {
    let Some(value) =
        value.and_then(|value| value.get("summary")).and_then(|summary| summary.get(key))
    else {
        return "not_available".to_string();
    };
    if value.is_null() {
        "none".to_string()
    } else {
        value.as_str().map(md_escape).unwrap_or_else(|| "invalid".to_string())
    }
}

fn summary_string(summary: Option<&Map<String, Value>>, key: &str, fallback: &str) -> String {
    summary
        .and_then(|summary| summary.get(key))
        .and_then(Value::as_str)
        .map(md_escape)
        .unwrap_or_else(|| fallback.to_string())
}

fn summary_string_or_null(summary: Option<&Map<String, Value>>, key: &str) -> String {
    let Some(value) = summary.and_then(|summary| summary.get(key)) else {
        return "not_available".to_string();
    };
    if value.is_null() {
        "none".to_string()
    } else {
        value.as_str().map(md_escape).unwrap_or_else(|| "invalid".to_string())
    }
}

fn artifact_available(packet: &Value, required_path: &str) -> bool {
    packet.get("artifacts").and_then(Value::as_array).is_some_and(|artifacts| {
        artifacts.iter().any(|artifact| {
            artifact.get("path").and_then(Value::as_str) == Some(required_path)
                && artifact.get("scope").and_then(Value::as_str) == Some("diff")
                && artifact.get("available").and_then(Value::as_bool).unwrap_or(false)
        })
    })
}

fn file_state(repo: &Path, relative: &str) -> &'static str {
    if repo.join(relative).exists() { "present" } else { "missing" }
}

fn first_line(value: &str) -> String {
    value.lines().next().unwrap_or("unknown error").trim().to_string()
}

fn md_escape(value: &str) -> String {
    value.replace('|', "\\|").replace('\n', " ")
}

fn normalize_path_text(value: &str) -> String {
    value.replace('\\', "/")
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn escape_cmd(value: &str) -> String {
    value
        .replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
        .replace(',', "%2C")
        .replace(':', "%3A")
}

fn bullet_list(values: &[String]) -> String {
    values.iter().map(|value| format!("- {value}")).collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------------------------------------------------------------------
    // #13809 falsifiers: the two path helpers whose Clippy repairs are only
    // mechanical if their refusal semantics are exact. `main` carried no
    // focused coverage for either, so the "semantics preserved" claim rested
    // on reading alone.
    // ---------------------------------------------------------------------

    #[test]
    fn lexically_join_resolves_dot_and_parent_segments() {
        assert_eq!(
            lexically_join("crates/a/src", "lib.rs").as_deref(),
            Some("crates/a/src/lib.rs")
        );
        assert_eq!(
            lexically_join("crates/a/src", "./lib.rs").as_deref(),
            Some("crates/a/src/lib.rs")
        );
        // `..` pops the directory it follows rather than being retained.
        assert_eq!(lexically_join("crates/a/src", "../lib.rs").as_deref(), Some("crates/a/lib.rs"));
        assert_eq!(
            lexically_join("crates/a/src", "../../lib.rs").as_deref(),
            Some("crates/lib.rs")
        );
        // Backslashes normalize before joining, so Windows-shaped includes
        // resolve identically.
        assert_eq!(
            lexically_join("crates/a/src", r"..\lib.rs").as_deref(),
            Some("crates/a/lib.rs")
        );
    }

    #[test]
    fn lexically_join_refuses_paths_that_escape_the_root() {
        // Only this assertion *discriminates* against a default-to-root
        // repair (`let _ = components.pop();`): the exhausted pop is
        // swallowed, the trailing `etc/passwd` segments repopulate
        // `components`, and the join wrongly yields Some("etc/passwd").
        // Verified — that repair fails here and nowhere else in this test.
        assert_eq!(lexically_join("crates/a", "../../../etc/passwd"), None);

        // These two also reach `components.pop()?` on an exhausted stack and
        // return there, so they do exercise the changed branch. They do not
        // discriminate, for a different reason than the line below: under the
        // broken repair they fall through to the post-loop
        // `components.is_empty()` guard, which yields `None` as well. Same
        // answer by a different route, so the assertion cannot tell them
        // apart.
        assert_eq!(lexically_join("", ".."), None);
        assert_eq!(lexically_join("a", "../.."), None);

        // This one never reaches an exhausted pop at all: `..` pops "a"
        // successfully and the loop ends empty, so the post-loop guard is its
        // refusal path under both implementations. Popping exactly to empty
        // is refusal, not an empty join.
        assert_eq!(lexically_join("a", ".."), None);
    }

    #[test]
    fn repo_relative_surface_path_normalizes_and_fails_closed() {
        let surface = ProductionSurface::from_parts("/work/repo", &[]);

        // Relative paths pass through, including the `./` form whose borrow
        // this leaf simplified.
        assert_eq!(
            repo_relative_surface_path(&surface, "crates/a/src/lib.rs").as_deref(),
            Some("crates/a/src/lib.rs")
        );
        assert_eq!(
            repo_relative_surface_path(&surface, "./crates/a/src/lib.rs").as_deref(),
            Some("crates/a/src/lib.rs")
        );

        // Absolute paths inside the root strip it; backslash and Windows
        // verbatim (`//?/`) shapes normalize to the same repo-relative result.
        let windows = ProductionSurface::from_parts("F:/work/repo", &[]);
        assert_eq!(
            repo_relative_surface_path(&windows, r"F:\work\repo\crates\a\src\lib.rs").as_deref(),
            Some("crates/a/src/lib.rs")
        );
        assert_eq!(
            repo_relative_surface_path(&windows, r"\\?\F:\work\repo\crates\a\src\lib.rs")
                .as_deref(),
            Some("crates/a/src/lib.rs")
        );

        // Absolute but outside the root is None — fail closed, never a
        // silently mis-anchored relative path.
        assert_eq!(repo_relative_surface_path(&surface, "/other/repo/src/lib.rs"), None);
        // A sibling root sharing a prefix must not match on the prefix alone.
        assert_eq!(repo_relative_surface_path(&surface, "/work/repo-two/src/lib.rs"), None);
    }

    #[test]
    fn command_root_arg_allows_repo_relative_root() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let repo = temp.path();
        fs::create_dir(repo.join("crates"))?;

        let root = command_root_arg(repo, "crates")?;

        assert_eq!(PathBuf::from(root), repo.join("crates").canonicalize()?);
        Ok(())
    }

    #[test]
    fn command_root_arg_rejects_absolute_root_outside_repo() -> Result<()> {
        let repo = tempfile::tempdir()?;
        let outside = tempfile::tempdir()?;
        let outside_arg = outside.path().display().to_string();

        assert!(command_root_arg(repo.path(), &outside_arg).is_err());
        Ok(())
    }

    #[test]
    fn command_root_arg_rejects_relative_parent_escape() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let repo = temp.path().join("repo");
        let outside = temp.path().join("outside");
        fs::create_dir(&repo)?;
        fs::create_dir(&outside)?;

        assert!(command_root_arg(&repo, "../outside").is_err());
        Ok(())
    }

    #[test]
    fn ripr_plus_top_files_rank_repo_seams_across_path_shapes() {
        let seams = [
            json!({"file": "crates/perl-parser/src/lib.rs"}),
            json!({"path": "crates/perl-lexer/src/lib.rs"}),
            json!({"location": {"path": r"crates\perl-parser\src\lib.rs"}}),
            json!({"placement": {"path": "crates/perl-workspace/src/index.rs"}}),
            json!({"evidence_record": {"path": "crates/perl-lexer/src/lib.rs"}}),
            json!({"file": ""}),
            json!({}),
        ];

        let rows = ripr_plus_top_files(seams.iter(), 2);

        assert_eq!(
            rows,
            vec![
                json!({"name": "crates/perl-lexer/src/lib.rs", "count": 2}),
                json!({"name": "crates/perl-parser/src/lib.rs", "count": 2}),
            ]
        );
    }

    #[test]
    fn ripr_plus_top_gap_kinds_rank_repo_seams_across_kind_shapes() {
        let seams = [
            json!({"kind": "ReceiptParsing"}),
            json!({"gap_kind": "BoundaryPredicate"}),
            json!({"classification": ["StaticUnknown", "NoStaticPath"]}),
            json!({"evidence_record": {"kind": "ReceiptParsing"}}),
            json!({"location": {"reason": "BoundaryPredicate"}}),
            json!({"kind": false}),
            json!({"kind": ""}),
            json!({}),
        ];

        let rows = ripr_plus_top_gap_kinds(seams.iter(), 3);

        assert_eq!(
            rows,
            vec![
                json!({"name": "boundarypredicate", "count": 2}),
                json!({"name": "receiptparsing", "count": 2}),
                json!({"name": "staticunknown,nostaticpath", "count": 1}),
            ]
        );
    }

    #[test]
    fn ripr_plus_recommended_clusters_group_files_and_gap_kinds() {
        let top_files = vec![
            json!({"name": "xtask/src/tasks/ripr_evidence.rs", "count": 5}),
            json!({"name": "crates/perl-lsp-quality/src/lib.rs", "count": 4}),
            json!({"name": "crates/perl-config/src/lib.rs", "count": 3}),
            json!({"name": "crates/perl-diagnostics/src/error.rs", "count": 2}),
            json!({"name": "crates/perl-parser/src/lib.rs", "count": 1}),
            json!({"count": 99}),
        ];
        let top_gap_kinds = vec![
            json!({"name": "receipt_missing", "count": 8}),
            json!({"name": "config_parse", "count": 7}),
            json!({"name": "error_variant", "count": 6}),
            json!({"name": "boundary_predicate", "count": 5}),
            json!({"name": "call_presence", "count": 4}),
            json!({"count": 99}),
        ];

        let rows = ripr_plus_recommended_first_clusters(&top_files, &top_gap_kinds, 10);

        assert_eq!(rows[0].pointer("/name"), Some(&json!("ci-report-formatting")));
        assert_eq!(rows[0].pointer("/score"), Some(&json!(12)));
        assert_eq!(rows[0].pointer("/active_file_count"), Some(&json!(4)));
        assert_eq!(rows[0].pointer("/gap_kind_count"), Some(&json!(8)));
        assert!(rows.iter().any(|row| {
            row.pointer("/name") == Some(&json!("proof-infrastructure"))
                && row.pointer("/example_files/0")
                    == Some(&json!("xtask/src/tasks/ripr_evidence.rs"))
        }));
        assert!(rows.iter().any(|row| {
            row.pointer("/name") == Some(&json!("boundary-predicates"))
                && row.pointer("/example_gap_kinds/0") == Some(&json!("boundary_predicate"))
        }));
        assert!(rows.iter().any(|row| row.pointer("/name") == Some(&json!("config-parsing"))));
        assert!(rows.iter().any(|row| row.pointer("/name") == Some(&json!("error-variants"))));
        assert!(
            rows.iter().any(|row| row.pointer("/name") == Some(&json!("active-ripr-inventory")))
        );
    }

    #[test]
    fn ripr_plus_recommended_clusters_keep_proof_infra_when_below_top_files() {
        let mut seams = Vec::new();
        for file_index in 0..12 {
            for _ in 0..3 {
                seams.push(json!({
                    "file": format!("crates/product-{file_index}/src/lib.rs"),
                    "kind": "CallPresence"
                }));
            }
        }
        seams.push(json!({
            "file": "xtask/src/tasks/quality_gate.rs",
            "kind": "CallPresence"
        }));

        let summary = ripr_plus_seam_summary(&seams, &RiprSuppressionRules::default(), 10);

        assert!(
            !summary.top_files.iter().any(|row| {
                row.get("name").and_then(Value::as_str) == Some("xtask/src/tasks/quality_gate.rs")
            }),
            "xtask proof-infra file should sit below the truncated top_files list"
        );
        assert!(
            summary.recommended_first_clusters.iter().any(|row| {
                row.get("name").and_then(Value::as_str) == Some("proof-infrastructure")
                    && row.get("example_files").and_then(Value::as_array).is_some_and(|files| {
                        files
                            .iter()
                            .any(|file| file.as_str() == Some("xtask/src/tasks/quality_gate.rs"))
                    })
            }),
            "cluster recommendations must preserve proof-infra work below the display top list: {:?}",
            summary.recommended_first_clusters
        );
    }

    #[test]
    fn ripr_plus_cluster_mapping_covers_inventory_buckets() {
        assert_eq!(
            ripr_plus_cluster_for_path("xtask/src/tasks/ripr_evidence.rs").0,
            "proof-infrastructure"
        );
        assert_eq!(
            ripr_plus_cluster_for_path("crates/perl-lsp-quality/src/lib.rs").0,
            "ci-report-formatting"
        );
        assert_eq!(ripr_plus_cluster_for_path("crates/perl-config/src/lib.rs").0, "config-parsing");
        assert_eq!(
            ripr_plus_cluster_for_path("crates/perl-diagnostics/src/error.rs").0,
            "error-variants"
        );
        assert_eq!(
            ripr_plus_cluster_for_path("crates/perl-parser/src/lib.rs").0,
            "active-ripr-inventory"
        );

        assert_eq!(ripr_plus_cluster_for_gap_kind("receipt_missing").0, "ci-report-formatting");
        assert_eq!(ripr_plus_cluster_for_gap_kind("config_parse").0, "config-parsing");
        assert_eq!(ripr_plus_cluster_for_gap_kind("error_variant").0, "error-variants");
        assert_eq!(ripr_plus_cluster_for_gap_kind("boundary_predicate").0, "boundary-predicates");
        assert_eq!(ripr_plus_cluster_for_gap_kind("call_presence").0, "active-ripr-inventory");
    }

    #[test]
    fn ripr_plus_seam_summary_splits_active_and_suppressed_paths() -> Result<()> {
        let seams = vec![
            json!({"file": "crates/perl-parser/src/lib.rs", "kind": "BoundaryPredicate"}),
            json!({"path": "archive/crates/perl-parser/src/lib.rs", "kind": "Archived"}),
            json!({"location": {"path": r"docs\project\status\quality.rs", "kind": "Generated"}}),
            json!({"placement": {"path": "crates/perl-parser/src/lib.rs", "kind": "BoundaryPredicate"}}),
            json!({"file": ""}),
        ];
        let suppressions = RiprSuppressionRules {
            display_patterns: vec!["archive/**".to_string(), "docs/project/status/**".to_string()],
            path_patterns: vec![
                Pattern::new("archive/**")?,
                Pattern::new("docs/project/status/**")?,
            ],
            classification_patterns: vec![Vec::new(), Vec::new()],
            invalid_patterns: Vec::new(),
            suppression_reasons: Vec::new(),
        };

        let summary = ripr_plus_seam_summary(&seams, &suppressions, 10);

        assert_eq!(summary.unresolved, 3);
        assert_eq!(summary.suppressed, 2);
        assert_eq!(
            summary.top_files,
            vec![json!({"name": "crates/perl-parser/src/lib.rs", "count": 2})]
        );
        assert_eq!(
            summary.top_suppressed_files,
            vec![
                json!({"name": "archive/crates/perl-parser/src/lib.rs", "count": 1}),
                json!({"name": "docs/project/status/quality.rs", "count": 1}),
            ]
        );
        assert_eq!(summary.top_gap_kinds, vec![json!({"name": "boundarypredicate", "count": 2})]);
        assert_eq!(
            summary.top_suppressed_gap_kinds,
            vec![json!({"name": "archived", "count": 1}), json!({"name": "generated", "count": 1}),]
        );
        Ok(())
    }

    #[test]
    fn ripr_plus_receipt_packet_reports_active_and_suppressed_totals() {
        let options = RiprPlusOptions {
            root: ".".to_string(),
            suppressions: PathBuf::from("policy/ripr-suppressions.toml"),
        };
        let suppressions = RiprSuppressionRules {
            display_patterns: vec!["archive/**".to_string()],
            path_patterns: Vec::new(),
            classification_patterns: Vec::new(),
            invalid_patterns: vec!["archive/[".to_string()],
            suppression_reasons: vec![json!({
                "id": "ripr-suppress-archive",
                "kind": "generated_or_non_production_surface",
                "reason": "Archived source is not active behavior.",
                "paths": ["archive/**"],
            })],
        };
        // Badge supplies the canonical counts; seam summary supplies the triage inventory.
        let badge = json!({
            "basis": "canonical_actionable_gap",
            "counts": {
                "unsuppressed_exposure_gaps": 2,
                "unsuppressed_test_efficiency_findings": 0,
                "suppressed_exposure_gaps": 1,
                "suppressed_test_efficiency_findings": 0
            }
        });
        let packet = ripr_plus_receipt_packet(
            &options,
            "head-sha",
            &suppressions,
            &badge,
            RiprPlusSeamSummary {
                unresolved: 2,
                suppressed: 1,
                top_files: vec![json!({"name": "xtask/src/tasks/ripr_evidence.rs", "count": 2})],
                top_suppressed_files: vec![json!({"name": "archive/old.rs", "count": 1})],
                top_gap_kinds: vec![json!({"name": "receiptparsing", "count": 2})],
                top_suppressed_gap_kinds: vec![json!({"name": "archived", "count": 1})],
                recommended_first_clusters: vec![json!({
                    "name": "proof-infrastructure",
                    "score": 2,
                    "active_file_count": 2,
                    "gap_kind_count": 0,
                    "reason": "Proof tooling, policy, workflow, and report surfaces are owned by this lane.",
                    "example_files": ["xtask/src/tasks/ripr_evidence.rs"],
                    "example_gap_kinds": [],
                })],
            },
        );

        assert_eq!(packet["head"], json!("head-sha"));
        assert_eq!(packet["root"], json!("."));
        // Canonical counts from badge: 2 unsuppressed exposure gaps.
        assert_eq!(packet["unresolved"], json!(2));
        assert_eq!(packet["active_unresolved"], json!(2));
        assert_eq!(packet["suppressed_unresolved"], json!(1));
        assert_eq!(packet["basis"], json!("canonical_actionable_gap"));
        assert_eq!(packet["schema_version"], json!(2));
        // Triage inventory from seam summary.
        assert_eq!(
            packet.pointer("/top_files/0/name"),
            Some(&json!("xtask/src/tasks/ripr_evidence.rs"))
        );
        assert_eq!(
            packet.pointer("/top_active_files/0/name"),
            Some(&json!("xtask/src/tasks/ripr_evidence.rs"))
        );
        assert_eq!(packet.pointer("/top_suppressed_files/0/name"), Some(&json!("archive/old.rs")));
        assert_eq!(packet.pointer("/top_active_gap_kinds/0/name"), Some(&json!("receiptparsing")));
        assert_eq!(packet.pointer("/top_suppressed_gap_kinds/0/name"), Some(&json!("archived")));
        assert_eq!(
            packet.pointer("/recommended_first_clusters/0/name"),
            Some(&json!("proof-infrastructure"))
        );
        assert_eq!(
            packet.pointer("/suppressions/path"),
            Some(&json!("policy/ripr-suppressions.toml"))
        );
        assert_eq!(packet.pointer("/suppressions/path_patterns/0"), Some(&json!("archive/**")));
        assert_eq!(packet.pointer("/suppressions/invalid_patterns/0"), Some(&json!("archive/[")));
        assert_eq!(
            packet.pointer("/suppressions/reasons/0/id"),
            Some(&json!("ripr-suppress-archive"))
        );
        assert_eq!(packet["decision"], json!("advisory"));
    }

    #[test]
    fn ripr_plus_receipt_counts_canonical_gaps_not_raw_seam_inventory() {
        // Regression guard for the 120k-vs-2.7k over-count: the receipt must
        // count canonical_actionable_gap findings from repo-badge-json, never
        // the raw analyzed_seams inventory. A seam that already has a
        // discriminating test is an analyzed seam but not an actionable gap.
        let options = RiprPlusOptions {
            root: ".".to_string(),
            suppressions: PathBuf::from("policy/ripr-suppressions.toml"),
        };
        let suppressions = RiprSuppressionRules::default();
        let badge = json!({
            "basis": "canonical_actionable_gap",
            "counts": {
                "unsuppressed_exposure_gaps": 2722,
                "unsuppressed_test_efficiency_findings": 0,
                "suppressed_exposure_gaps": 0,
                "suppressed_test_efficiency_findings": 0,
                "analyzed_seams": 120408,
                "analyzed_gap_records": 90255
            },
            "reason_counts": { "smoke_oracle_only": 0, "no_assertion_detected": 0 }
        });
        let seam_summary = RiprPlusSeamSummary {
            unresolved: 120408,
            suppressed: 0,
            top_files: vec![],
            top_suppressed_files: vec![],
            top_gap_kinds: vec![],
            top_suppressed_gap_kinds: vec![],
            recommended_first_clusters: vec![],
        };

        let packet =
            ripr_plus_receipt_packet(&options, "head-sha", &suppressions, &badge, seam_summary);

        // unresolved is the actionable-gap count, not the 120_408 raw seam inventory.
        assert_eq!(packet["unresolved"], json!(2722));
        assert_ne!(packet["unresolved"], json!(120_408));
        assert_eq!(packet["basis"], json!("canonical_actionable_gap"));
        assert_eq!(
            packet["source_format"],
            json!(
                "ripr check --format repo-badge-json (counts) + repo-seams-json (triage inventory)"
            )
        );
        assert_eq!(packet.pointer("/counts/analyzed_seams"), Some(&json!(120_408)));
        assert_eq!(packet.pointer("/reason_counts/smoke_oracle_only"), Some(&json!(0)));
    }

    #[test]
    fn ripr_plus_packet_from_raw_parses_and_builds_receipt() -> Result<()> {
        let options = RiprPlusOptions {
            root: ".".to_string(),
            suppressions: PathBuf::from("policy/ripr-suppressions.toml"),
        };
        let badge_raw = json!({
            "basis": "canonical_actionable_gap",
            "counts": {
                "unsuppressed_exposure_gaps": 2722,
                "unsuppressed_test_efficiency_findings": 0,
                "analyzed_seams": 120408
            }
        })
        .to_string();
        let seams_raw = json!({"seams": []}).to_string();

        let packet = ripr_plus_packet_from_raw(
            &options,
            "head-sha",
            &RiprSuppressionRules::default(),
            &badge_raw,
            &seams_raw,
        )?;

        assert_eq!(packet["unresolved"], json!(2722));
        assert_eq!(packet["head"], json!("head-sha"));
        assert_eq!(packet["basis"], json!("canonical_actionable_gap"));
        Ok(())
    }

    #[test]
    fn ripr_plus_packet_invokes_badge_counts_and_seam_inventory_sources() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let repo = temp.path();
        init_git_repo(repo)?;
        fs::create_dir_all(repo.join("policy"))?;
        fs::write(
            repo.join("policy/ripr-suppressions.toml"),
            r#"schema_version = 1
policy = "ripr-suppressions"
owner = "EffortlessMetrics"
status = "advisory"
updated = "2026-05-28"

[[suppress]]
id = "ripr-suppress-archive"
kind = "generated_or_non_production_surface"
paths = ["archive/**"]
reason = "Archived source is not active workspace behavior."
"#,
        )?;
        let ripr = write_fake_ripr_binary(repo)?;
        let _override = override_ripr_bin(&ripr)?;
        let options = RiprPlusOptions {
            root: ".".to_string(),
            suppressions: PathBuf::from("policy/ripr-suppressions.toml"),
        };

        let packet = ripr_plus_packet(repo, &options)?;

        assert_eq!(packet["head"], json!(current_head(repo)?));
        assert_eq!(packet["basis"], json!("canonical_actionable_gap"));
        assert_eq!(packet["unresolved"], json!(9));
        assert_eq!(packet["active_unresolved"], json!(9));
        assert_eq!(packet["suppressed_unresolved"], json!(4));
        assert_eq!(packet.pointer("/counts/unsuppressed_exposure_gaps"), Some(&json!(7)));
        assert_eq!(packet.pointer("/reason_counts/no_assertion_detected"), Some(&json!(7)));
        assert_eq!(
            packet.pointer("/top_files/0/name"),
            Some(&json!("xtask/src/tasks/ripr_evidence.rs"))
        );
        assert_eq!(packet.pointer("/top_suppressed_files/0/name"), Some(&json!("archive/old.rs")));
        assert_eq!(packet.pointer("/top_active_gap_kinds/0/name"), Some(&json!("receipt parsing")));
        let clusters = packet
            .get("recommended_first_clusters")
            .and_then(Value::as_array)
            .ok_or_else(|| eyre!("missing recommended_first_clusters"))?;
        assert!(
            clusters.iter().any(|cluster| {
                cluster.get("name").and_then(Value::as_str) == Some("proof-infrastructure")
            }),
            "xtask seam inventory must recommend the proof-infrastructure cluster: {clusters:?}"
        );
        Ok(())
    }

    #[test]
    fn ripr_plus_packet_from_raw_rejects_invalid_badge_json() {
        let options = RiprPlusOptions {
            root: ".".to_string(),
            suppressions: PathBuf::from("policy/ripr-suppressions.toml"),
        };
        let result = ripr_plus_packet_from_raw(
            &options,
            "head-sha",
            &RiprSuppressionRules::default(),
            "this is not json",
            r#"{"seams":[]}"#,
        );
        assert!(result.is_err(), "invalid repo-badge-json must be rejected");
    }

    #[test]
    fn ripr_plus_packet_from_raw_rejects_invalid_seams_json() {
        let options = RiprPlusOptions {
            root: ".".to_string(),
            suppressions: PathBuf::from("policy/ripr-suppressions.toml"),
        };
        let result = ripr_plus_packet_from_raw(
            &options,
            "head-sha",
            &RiprSuppressionRules::default(),
            r#"{"basis":"canonical_actionable_gap","counts":{}}"#,
            "this is not json",
        );
        assert!(result.is_err(), "invalid repo-seams-json must be rejected");
    }

    #[test]
    fn ripr_plus_packet_from_raw_rejects_missing_seams_array() {
        let options = RiprPlusOptions {
            root: ".".to_string(),
            suppressions: PathBuf::from("policy/ripr-suppressions.toml"),
        };
        let result = ripr_plus_packet_from_raw(
            &options,
            "head-sha",
            &RiprSuppressionRules::default(),
            r#"{"basis":"canonical_actionable_gap","counts":{}}"#,
            r#"{"not_seams": []}"#,
        );
        assert!(result.is_err(), "seams-json without seams[] key must be rejected");
    }

    #[test]
    fn ripr_plus_suppression_rules_match_non_production_paths() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let repo = temp.path();
        fs::create_dir_all(repo.join("policy"))?;
        fs::write(
            repo.join("policy/ripr-suppressions.toml"),
            r#"schema_version = 1
policy = "ripr-suppressions"
owner = "EffortlessMetrics"
status = "advisory"
updated = "2026-05-28"

[[suppress]]
id = "ripr-suppress-archive"
kind = "generated_or_non_production_surface"
paths = ["archive/**"]
reason = "Archived source is not active workspace behavior."

[[suppress]]
id = "ripr-suppress-generated-status-docs"
paths = ["docs/project/status/**"]

[[suppress]]
id = "ripr-suppress-ux-receipt-tests"
kind = "test_receipt_surface"
paths = ["crates/perl-lsp-ux-tests/tests/**"]
reason = "UX receipt tests are proof inputs."
"#,
        )?;

        let rules = read_ripr_suppression_rules(repo, Path::new("policy/ripr-suppressions.toml"))?;

        assert!(suppression_matches_seam(
            &rules,
            &json!({"file": "archive/crates/perl-parser/src/lib.rs"})
        ));
        assert!(suppression_matches_seam(
            &rules,
            &json!({"location": {"path": r"docs\project\status\quality.rs"}})
        ));
        assert!(suppression_matches_finding(
            &rules,
            &json!({"probe": {"file": r".\crates/perl-lsp-ux-tests/tests/ux_scenario_62_project_test_assertion_inline_completion_quality.rs"}})
        ));
        assert!(!suppression_matches_seam(
            &rules,
            &json!({"file": "crates/perl-parser/src/lib.rs"})
        ));
        assert_eq!(
            rules.suppression_reasons[0],
            json!({
                "id": "ripr-suppress-archive",
                "kind": "generated_or_non_production_surface",
                "reason": "Archived source is not active workspace behavior.",
                "paths": ["archive/**"],
            })
        );
        Ok(())
    }

    #[test]
    fn ripr_plus_suppression_rules_reject_invalid_path_globs() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let repo = temp.path();
        fs::create_dir_all(repo.join("policy"))?;
        fs::write(
            repo.join("policy/ripr-suppressions.toml"),
            r#"schema_version = 1
policy = "ripr-suppressions"
owner = "EffortlessMetrics"
status = "advisory"
updated = "2026-05-28"

[[suppress]]
id = "ripr-suppress-invalid"
paths = ["archive/["]
"#,
        )?;

        assert!(
            read_ripr_suppression_rules(repo, Path::new("policy/ripr-suppressions.toml")).is_err()
        );
        Ok(())
    }

    #[test]
    fn parser_comparison_suppression_requires_an_admissible_classification() -> Result<()> {
        let rules =
            read_ripr_suppression_rules(&repo_root()?, Path::new("policy/ripr-suppressions.toml"))?;
        let path = "crates/perl-parser-comparison/src/evidence_payload.rs";

        for classification in ["no_static_path", "weakly_exposed"] {
            assert!(suppression_matches_finding(
                &rules,
                &json!({"classification": classification, "probe": {"file": path}})
            ));
        }
        assert!(!suppression_matches_finding(
            &rules,
            &json!({"classification": "reachable_unrevealed", "probe": {"file": path}})
        ));
        assert!(!suppression_matches_finding(
            &rules,
            &json!({"grip_class": "weakly_gripped", "seam": {"file": path}})
        ));
        Ok(())
    }

    #[test]
    fn mutation_label_routes_targeted() {
        let decision = routing_decision(&["mutation".to_string()], false);
        assert!(decision.requires_targeted_mutation);
        assert!(!decision.requires_full_owner_mutation);
        assert_eq!(decision.mode, "targeted");
    }

    #[test]
    fn full_owner_label_routes_full_owner() {
        let decision = routing_decision(&["mutation/full-owner".to_string()], false);
        assert!(!decision.requires_targeted_mutation);
        assert!(decision.requires_full_owner_mutation);
        assert_eq!(decision.mode, "full_owner");
    }

    #[test]
    fn empty_labels_keep_fast_only() {
        let decision = routing_decision(&[], false);
        assert!(!decision.requires_targeted_mutation);
        assert!(!decision.requires_full_owner_mutation);
        assert_eq!(decision.mode, "fast_only");
    }

    #[test]
    fn normalized_option_uses_default_for_blank_values() {
        assert_eq!(normalized_option("", DEFAULT_ROOT), DEFAULT_ROOT);
        assert_eq!(normalized_option("  ", DEFAULT_BASE), DEFAULT_BASE);
        assert_eq!(normalized_option("HEAD", DEFAULT_HEAD), "HEAD");
    }

    #[test]
    fn optional_sha_value_preserves_string_or_null_contract() {
        assert_eq!(optional_sha_value(Some("abc123")), json!("abc123"));
        assert_eq!(optional_sha_value(None), Value::Null);
    }

    #[test]
    fn revision_sha_reads_current_head() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let repo = temp.path();
        init_git_repo(repo)?;

        let head = revision_sha(repo, "HEAD")?;

        assert_eq!(current_head(repo)?, head);
        assert_eq!(head.len(), 40);
        Ok(())
    }

    #[test]
    fn revision_sha_rejects_missing_revision() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let repo = temp.path();
        init_git_repo(repo)?;

        assert!(revision_sha(repo, "refs/heads/does-not-exist").is_err());
        Ok(())
    }

    #[test]
    fn pr_evidence_packet_carries_revision_shas() -> Result<()> {
        let options = PrEvidenceOptions {
            root: ".".to_string(),
            base: "origin/main".to_string(),
            head: "HEAD".to_string(),
            pr_head_sha: Some("pr-head-sha".to_string()),
        };
        let check_value = json!({
            "summary": {
                "weakly_exposed": 1,
                "reachable_unrevealed": 0,
                "no_static_path": 0
            }
        });

        let packet = pr_evidence_packet(
            &options,
            &["xtask/src/tasks/ripr_evidence.rs".to_string()],
            &check_value,
            "base-sha",
            "head-sha",
            &RiprSuppressionRules::default(),
        );

        assert_eq!(packet["base_sha"], json!("base-sha"));
        assert_eq!(packet["head_sha"], json!("head-sha"));
        assert_eq!(packet["pr_head_sha"], json!("pr-head-sha"));
        assert_eq!(packet["evaluated_head"], json!("HEAD"));
        assert_eq!(packet["evaluated_head_sha"], json!("head-sha"));
        validate_pr_evidence_packet(&packet, &options, 1, true, "base-sha", "head-sha")?;
        Ok(())
    }

    #[test]
    fn pr_evidence_packet_suppresses_non_production_test_receipt_findings() -> Result<()> {
        let options = PrEvidenceOptions {
            root: ".".to_string(),
            base: "origin/main".to_string(),
            head: "HEAD".to_string(),
            pr_head_sha: None,
        };
        let check_value = json!({
            "summary": {
                "weakly_exposed": 1,
                "reachable_unrevealed": 1,
                "no_static_path": 0
            },
            "findings": [
                {
                    "classification": "reachable_unrevealed",
                    "probe": {
                        "file": r".\crates/perl-lsp-ux-tests/tests/ux_scenario_62_project_test_assertion_inline_completion_quality.rs"
                    }
                },
                {
                    "classification": "weakly_exposed",
                    "probe": {
                        "file": "crates/perl-lsp-rs-core/src/providers/inline_completion/mod.rs"
                    }
                }
            ]
        });
        let suppressions = RiprSuppressionRules {
            display_patterns: vec!["crates/perl-lsp-ux-tests/tests/**".to_string()],
            path_patterns: vec![Pattern::new("crates/perl-lsp-ux-tests/tests/**")?],
            classification_patterns: vec![Vec::new()],
            invalid_patterns: Vec::new(),
            suppression_reasons: Vec::new(),
        };

        let packet = pr_evidence_packet(
            &options,
            &[
                "crates/perl-lsp-ux-tests/tests/ux_scenario_62_project_test_assertion_inline_completion_quality.rs".to_string(),
                "crates/perl-lsp-rs-core/src/providers/inline_completion/mod.rs".to_string(),
            ],
            &check_value,
            "base-sha",
            "head-sha",
            &suppressions,
        );

        assert_eq!(packet.pointer("/summary/weakly_exposed"), Some(&json!(1)));
        assert_eq!(packet.pointer("/summary/reachable_unrevealed"), Some(&json!(0)));
        assert_eq!(packet.pointer("/summary/severe_gaps"), Some(&json!(1)));
        assert_eq!(packet.pointer("/summary/suppressed_by_policy"), Some(&json!(1)));
        assert_eq!(
            packet.pointer("/summary/suppression_patterns/0"),
            Some(&json!("crates/perl-lsp-ux-tests/tests/**"))
        );
        Ok(())
    }

    #[test]
    fn suppression_matches_windows_probe_paths_under_receipt_test_directory() -> Result<()> {
        let rules = RiprSuppressionRules {
            display_patterns: vec![
                "crates/perl-lsp-ux-tests/tests/*".to_string(),
                "crates/perl-lsp-ux-tests/tests/**".to_string(),
            ],
            path_patterns: vec![
                Pattern::new("crates/perl-lsp-ux-tests/tests/*")?,
                Pattern::new("crates/perl-lsp-ux-tests/tests/**")?,
            ],
            classification_patterns: vec![Vec::new(), Vec::new()],
            invalid_patterns: Vec::new(),
            suppression_reasons: Vec::new(),
        };
        let finding = json!({
            "classification": "reachable_unrevealed",
            "probe": {
                "file": r".\crates/perl-lsp-ux-tests/tests/ux_scenario_62_project_test_assertion_inline_completion_quality.rs"
            }
        });

        assert!(suppression_matches_finding(&rules, &finding));
        Ok(())
    }

    #[test]
    fn suppression_matches_absolute_probe_paths_under_receipt_test_directory() -> Result<()> {
        let rules = RiprSuppressionRules {
            display_patterns: vec!["crates/perl-lsp-ux-tests/tests/**".to_string()],
            path_patterns: vec![Pattern::new("crates/perl-lsp-ux-tests/tests/**")?],
            classification_patterns: vec![Vec::new()],
            invalid_patterns: Vec::new(),
            suppression_reasons: Vec::new(),
        };
        let finding = json!({
            "classification": "weakly_exposed",
            "probe": {
                "file": "//?/H:/Code/Rust3/perl-lsp-swarm\\crates/perl-lsp-ux-tests/tests/ux_scenario_62_project_test_assertion_inline_completion_quality.rs"
            }
        });

        assert!(suppression_matches_finding(&rules, &finding));
        Ok(())
    }

    #[test]
    fn ripr_0_9_x_grip_class_seam_file_suppressed_by_policy() -> Result<()> {
        // ripr 0.9.x uses "grip_class" (not "classification") and "seam.file" (not "probe.file").
        // Verify that the suppression machinery handles both field shapes so that
        // path-scoped suppressions in policy/ripr-suppressions.toml fire under 0.9.x.
        let options = PrEvidenceOptions {
            root: ".".to_string(),
            base: "origin/main".to_string(),
            head: "HEAD".to_string(),
            pr_head_sha: None,
        };
        // Simulate ripr 0.9.x check output: summary.reachable_unrevealed=3, findings use grip_class+seam.
        let check_value = json!({
            "summary": {
                "weakly_exposed": 0,
                "reachable_unrevealed": 3,
                "no_static_path": 0
            },
            "findings": [
                {
                    "grip_class": "weakly_gripped",
                    "kind": "call_presence",
                    "seam": {
                        "file": "crates/perl-dap/src/debug_adapter/execution.rs",
                        "line": 22
                    }
                },
                {
                    "grip_class": "weakly_gripped",
                    "kind": "call_presence",
                    "seam": {
                        "file": "crates/perl-dap/src/debug_adapter/execution.rs",
                        "line": 28
                    }
                },
                {
                    "grip_class": "weakly_gripped",
                    "kind": "call_presence",
                    "seam": {
                        "file": "crates/perl-dap/src/debug_adapter/execution.rs",
                        "line": 30
                    }
                }
            ]
        });
        let suppressions = RiprSuppressionRules {
            display_patterns: vec!["crates/perl-dap/src/debug_adapter/execution.rs".to_string()],
            path_patterns: vec![Pattern::new("crates/perl-dap/src/debug_adapter/execution.rs")?],
            classification_patterns: vec![Vec::new()],
            invalid_patterns: Vec::new(),
            suppression_reasons: Vec::new(),
        };

        let packet = pr_evidence_packet(
            &options,
            &["crates/perl-dap/src/debug_adapter/execution.rs".to_string()],
            &check_value,
            "base-sha",
            "head-sha",
            &suppressions,
        );

        // All 3 weakly_gripped findings are in the suppressed path and map to reachable_unrevealed.
        // After suppression: reachable_unrevealed = 3 - 3 = 0, severe_gaps = 0.
        assert_eq!(packet.pointer("/summary/reachable_unrevealed"), Some(&json!(0)));
        assert_eq!(packet.pointer("/summary/weakly_exposed"), Some(&json!(0)));
        assert_eq!(packet.pointer("/summary/severe_gaps"), Some(&json!(0)));
        assert_eq!(packet.pointer("/summary/suppressed_by_policy"), Some(&json!(3)));
        Ok(())
    }

    #[test]
    fn ripr_0_9_x_unsuppressed_grip_class_produces_severe_gaps() -> Result<()> {
        // Gate teeth: a ripr 0.9.x weakly_gripped finding on a path NOT covered by any
        // suppression rule must produce severe_gaps > 0, causing the quality gate to FAIL.
        // Before the grip_class fix, the gate silently skipped such findings because
        // grip_class was not recognized, so severe_gaps stayed 0 — the gate had no teeth.
        let options = PrEvidenceOptions {
            root: ".".to_string(),
            base: "origin/main".to_string(),
            head: "HEAD".to_string(),
            pr_head_sha: None,
        };
        // ripr 0.9.x output: 2 weakly_gripped findings on a file not in any suppression.
        let check_value = json!({
            "summary": {
                "weakly_exposed": 0,
                "reachable_unrevealed": 2,
                "no_static_path": 0
            },
            "findings": [
                {
                    "grip_class": "weakly_gripped",
                    "kind": "call_presence",
                    "seam": {
                        "file": "crates/perl-lsp-rs/src/some_new_file.rs",
                        "line": 10
                    }
                },
                {
                    "grip_class": "weakly_gripped",
                    "kind": "call_presence",
                    "seam": {
                        "file": "crates/perl-lsp-rs/src/some_new_file.rs",
                        "line": 20
                    }
                }
            ]
        });
        // Suppression only covers the DAP execution.rs — the LSP file is NOT suppressed.
        let suppressions = RiprSuppressionRules {
            display_patterns: vec!["crates/perl-dap/src/debug_adapter/execution.rs".to_string()],
            path_patterns: vec![Pattern::new("crates/perl-dap/src/debug_adapter/execution.rs")?],
            classification_patterns: vec![Vec::new()],
            invalid_patterns: Vec::new(),
            suppression_reasons: Vec::new(),
        };

        let packet = pr_evidence_packet(
            &options,
            &["crates/perl-lsp-rs/src/some_new_file.rs".to_string()],
            &check_value,
            "base-sha",
            "head-sha",
            &suppressions,
        );

        // The 2 unsuppressed weakly_gripped findings map to reachable_unrevealed bucket.
        // severe_gaps must be 2 (> 0) so the quality gate rejects this PR.
        assert_eq!(packet.pointer("/summary/reachable_unrevealed"), Some(&json!(2)));
        assert_eq!(packet.pointer("/summary/severe_gaps"), Some(&json!(2)));
        assert_eq!(packet.pointer("/summary/suppressed_by_policy"), Some(&json!(0)));
        assert_eq!(packet.pointer("/summary/ripr_severe_gap"), Some(&json!(true)));
        Ok(())
    }

    fn no_suppressions() -> RiprSuppressionRules {
        RiprSuppressionRules {
            display_patterns: Vec::new(),
            path_patterns: Vec::new(),
            classification_patterns: Vec::new(),
            invalid_patterns: Vec::new(),
            suppression_reasons: Vec::new(),
        }
    }

    fn packet_with_extents(
        check_value: &Value,
        suppressions: &RiprSuppressionRules,
        extents: &HeadLineExtents,
    ) -> Value {
        let options = PrEvidenceOptions {
            root: ".".to_string(),
            base: "origin/main".to_string(),
            head: "HEAD".to_string(),
            pr_head_sha: None,
        };
        pr_evidence_packet_with_count(
            &options,
            check_value,
            "base-sha",
            "head-sha",
            suppressions,
            Some(extents),
            PrEvidenceContext {
                changed_file_count: 1,
                attribution_scope: None,
                production_surface: None,
            },
        )
    }

    /// #6260 reproduction, from the `raw-check.json` of run 31273961774 on #6161:
    /// two `no_static_path` probes at `check_version_sync.rs:29`, a line the change
    /// deletes — the file is 13 lines long at head. No test can cover a line that no
    /// longer exists, so the required gate was unsatisfiable.
    #[test]
    fn deleted_line_findings_do_not_count_as_new_gaps() -> Result<()> {
        let check_value = json!({
            "summary": { "weakly_exposed": 0, "reachable_unrevealed": 0, "no_static_path": 2 },
            "findings": [
                {
                    "classification": "no_static_path",
                    "kind": "call_deletion",
                    "probe": { "path": "xtask/src/tasks/check_version_sync.rs", "line": 29 }
                },
                {
                    "classification": "no_static_path",
                    "kind": "return_value",
                    "probe": { "path": "xtask/src/tasks/check_version_sync.rs", "line": 29 }
                }
            ]
        });
        let extents = HeadLineExtents {
            present: BTreeMap::from([(
                "xtask/src/tasks/check_version_sync.rs".to_string(),
                13usize,
            )]),
            removed: BTreeSet::new(),
        };

        let packet = packet_with_extents(&check_value, &no_suppressions(), &extents);

        assert_eq!(packet.pointer("/summary/no_static_path"), Some(&json!(0)));
        assert_eq!(packet.pointer("/summary/severe_gaps"), Some(&json!(0)));
        assert_eq!(packet.pointer("/summary/outside_head_revision"), Some(&json!(2)));
        assert_eq!(packet.pointer("/summary/ripr_severe_gap"), Some(&json!(false)));
        // Head-range filtering is not suppression and must not inflate the policy count.
        assert_eq!(packet.pointer("/summary/suppressed_by_policy"), Some(&json!(0)));
        Ok(())
    }

    /// Gate teeth: the filter is bounded to lines that do not exist at head. A probe on a
    /// line the head revision still has must keep blocking, or #6260's fix would be worse
    /// than the bug it closes.
    #[test]
    fn findings_inside_the_head_revision_still_count_as_new_gaps() -> Result<()> {
        let check_value = json!({
            "summary": { "weakly_exposed": 0, "reachable_unrevealed": 0, "no_static_path": 2 },
            "findings": [
                {
                    "classification": "no_static_path",
                    "probe": { "path": "xtask/src/tasks/check_version_sync.rs", "line": 12 }
                },
                {
                    "classification": "no_static_path",
                    "probe": { "path": "xtask/src/tasks/check_version_sync.rs", "line": 13 }
                }
            ]
        });
        let extents = HeadLineExtents {
            present: BTreeMap::from([(
                "xtask/src/tasks/check_version_sync.rs".to_string(),
                13usize,
            )]),
            removed: BTreeSet::new(),
        };

        let packet = packet_with_extents(&check_value, &no_suppressions(), &extents);

        assert_eq!(packet.pointer("/summary/no_static_path"), Some(&json!(2)));
        assert_eq!(packet.pointer("/summary/severe_gaps"), Some(&json!(2)));
        assert_eq!(packet.pointer("/summary/outside_head_revision"), Some(&json!(0)));
        assert_eq!(packet.pointer("/summary/ripr_severe_gap"), Some(&json!(true)));
        Ok(())
    }

    /// A file the change deletes outright has no coverable line at all, whatever the
    /// reported line number.
    #[test]
    fn findings_on_a_deleted_file_do_not_count_as_new_gaps() -> Result<()> {
        let check_value = json!({
            "summary": { "weakly_exposed": 0, "reachable_unrevealed": 1, "no_static_path": 0 },
            "findings": [
                {
                    "grip_class": "reachable_unrevealed",
                    "seam": { "file": "crates/perl-lsp-rs/src/removed.rs", "line": 4 }
                }
            ]
        });
        let extents = HeadLineExtents {
            present: BTreeMap::new(),
            removed: BTreeSet::from(["crates/perl-lsp-rs/src/removed.rs".to_string()]),
        };

        let packet = packet_with_extents(&check_value, &no_suppressions(), &extents);

        assert_eq!(packet.pointer("/summary/reachable_unrevealed"), Some(&json!(0)));
        assert_eq!(packet.pointer("/summary/severe_gaps"), Some(&json!(0)));
        assert_eq!(packet.pointer("/summary/outside_head_revision"), Some(&json!(1)));
        Ok(())
    }

    /// Fail closed on every ambiguity: a path outside the change's file set, and a finding
    /// with no line at all, are both still counted.
    #[test]
    fn unlocatable_findings_stay_counted() -> Result<()> {
        let check_value = json!({
            "summary": { "weakly_exposed": 0, "reachable_unrevealed": 0, "no_static_path": 2 },
            "findings": [
                {
                    "classification": "no_static_path",
                    "probe": { "path": "crates/perl-lsp-rs/src/untracked_by_the_diff.rs", "line": 900 }
                },
                {
                    "classification": "no_static_path",
                    "probe": { "path": "xtask/src/tasks/check_version_sync.rs" }
                }
            ]
        });
        let extents = HeadLineExtents {
            present: BTreeMap::from([(
                "xtask/src/tasks/check_version_sync.rs".to_string(),
                13usize,
            )]),
            removed: BTreeSet::new(),
        };

        let packet = packet_with_extents(&check_value, &no_suppressions(), &extents);

        assert_eq!(packet.pointer("/summary/no_static_path"), Some(&json!(2)));
        assert_eq!(packet.pointer("/summary/severe_gaps"), Some(&json!(2)));
        assert_eq!(packet.pointer("/summary/outside_head_revision"), Some(&json!(0)));
        Ok(())
    }

    /// A suppressed finding that is also outside the head revision is discounted once, and
    /// stays attributed to policy so `suppressed_by_policy` keeps its established meaning.
    #[test]
    fn suppression_takes_precedence_over_head_range_filtering() -> Result<()> {
        let check_value = json!({
            "summary": { "weakly_exposed": 0, "reachable_unrevealed": 0, "no_static_path": 1 },
            "findings": [
                {
                    "classification": "no_static_path",
                    "probe": { "path": "archive/old.rs", "line": 99 }
                }
            ]
        });
        let suppressions = RiprSuppressionRules {
            display_patterns: vec!["archive/**".to_string()],
            path_patterns: vec![Pattern::new("archive/**")?],
            classification_patterns: vec![Vec::new()],
            invalid_patterns: Vec::new(),
            suppression_reasons: Vec::new(),
        };
        let extents = HeadLineExtents {
            present: BTreeMap::from([("archive/old.rs".to_string(), 4usize)]),
            removed: BTreeSet::new(),
        };

        let packet = packet_with_extents(&check_value, &suppressions, &extents);

        assert_eq!(packet.pointer("/summary/no_static_path"), Some(&json!(0)));
        assert_eq!(packet.pointer("/summary/severe_gaps"), Some(&json!(0)));
        assert_eq!(packet.pointer("/summary/suppressed_by_policy"), Some(&json!(1)));
        assert_eq!(packet.pointer("/summary/outside_head_revision"), Some(&json!(0)));
        Ok(())
    }

    /// #11690 reproduction, shaped from the two live incidents in the issue:
    /// #6842's 162-finding `no_static_path` block inside the PR's own
    /// `crates/perl-semantic-facts/tests/prop_json_roundtrip.rs`, and #6766's
    /// archive/caller inflation (`archive/crates/tree-sitter-perl-rs/...`).
    /// Neither class can be closed by a test the PR could write, so neither may
    /// inflate the blocking basis. Production seams — including production
    /// files that simply do not depend on the changed crate, which stay this
    /// filter's negative control — keep counting.
    #[test]
    fn non_production_test_and_archive_findings_do_not_inflate_the_new_gap_basis() -> Result<()> {
        let check_value = json!({
            "summary": { "weakly_exposed": 0, "reachable_unrevealed": 2, "no_static_path": 3 },
            "findings": [
                {
                    // #6842 class: proptest macro bodies in the PR's own new test file.
                    "classification": "no_static_path",
                    "probe": { "path": "crates/perl-semantic-facts/tests/prop_json_roundtrip.rs", "line": 41 }
                },
                {
                    // ripr 0.9.x shape: grip_class + seam.file, also inside a tests/ dir.
                    "grip_class": "weakly_gripped",
                    "seam": { "file": "crates/perl-lsp-ux-tests/tests/ux_scenario_62.rs", "line": 7 }
                },
                {
                    // #6766 class: archived sources pulled in by the analyzer's caller expansion.
                    "classification": "no_static_path",
                    "seam": { "file": "archive/crates/tree-sitter-perl-rs/src/scanner/mod.rs", "line": 100 }
                },
                {
                    // Production seam in the PR's own crate: still counts.
                    "grip_class": "reachable_unrevealed",
                    "seam": { "file": "crates/perl-core-harness/src/contract.rs", "line": 12 }
                },
                {
                    // Production non-dependent file: counts here. Excluding it needs the
                    // dependency-graph attribution slice, not the production-file filter.
                    "classification": "no_static_path",
                    "seam": { "file": "crates/perl-lsp-rs/src/runtime/scheduler.rs", "line": 88 }
                }
            ]
        });

        let packet = pr_evidence_packet_on_surface(
            &PrEvidenceOptions {
                root: ".".to_string(),
                base: "origin/main".to_string(),
                head: "HEAD".to_string(),
                pr_head_sha: None,
            },
            &["crates/perl-semantic-facts/tests/prop_json_roundtrip.rs".to_string()],
            &check_value,
            "base-sha",
            "head-sha",
            &no_suppressions(),
            Some(&ProductionSurface::from_parts(
                "/ws",
                &[
                    "crates/perl-core-harness/src/contract.rs",
                    "crates/perl-lsp-rs/src/runtime/scheduler.rs",
                ],
            )),
        );

        // weakly_gripped folds into reachable_unrevealed (0.9.x): the ux
        // test-file finding leaves that bucket (2 - 1 = 1), and the proptest
        // and archive findings leave no_static_path (3 - 2 = 1). The two
        // production seams — the changed crate and the non-dependent
        // scheduler.rs negative control — keep counting.
        assert_eq!(packet.pointer("/summary/reachable_unrevealed"), Some(&json!(1)));
        assert_eq!(packet.pointer("/summary/no_static_path"), Some(&json!(1)));
        assert_eq!(packet.pointer("/summary/severe_gaps"), Some(&json!(2)));
        assert_eq!(packet.pointer("/summary/non_production_excluded"), Some(&json!(3)));
        assert_eq!(packet.pointer("/summary/suppressed_by_policy"), Some(&json!(0)));
        assert_eq!(packet.pointer("/summary/ripr_severe_gap"), Some(&json!(true)));
        Ok(())
    }

    /// Gate teeth: the filter is bounded to non-production surfaces. An inline
    /// `#[cfg(test)]` seam in a production source file is NOT under a `tests/`
    /// directory component, so it keeps blocking — the calibration retires the
    /// test_receipt_surface treadmill, not test-worthiness itself.
    #[test]
    fn production_files_still_count_after_the_non_production_filter() -> Result<()> {
        let check_value = json!({
            "summary": { "weakly_exposed": 0, "reachable_unrevealed": 2, "no_static_path": 0 },
            "findings": [
                {
                    "classification": "reachable_unrevealed",
                    "seam": { "file": "crates/perl-core-harness/src/contract.rs", "line": 12 }
                },
                {
                    // A file literally named tests.rs is not a tests/ directory.
                    "classification": "reachable_unrevealed",
                    "seam": { "file": "crates/perl-core-harness/src/tests.rs", "line": 3 }
                }
            ]
        });

        let packet = pr_evidence_packet_on_surface(
            &PrEvidenceOptions {
                root: ".".to_string(),
                base: "origin/main".to_string(),
                head: "HEAD".to_string(),
                pr_head_sha: None,
            },
            &["crates/perl-core-harness/src/contract.rs".to_string()],
            &check_value,
            "base-sha",
            "head-sha",
            &no_suppressions(),
            Some(&ProductionSurface::from_parts(
                "/ws",
                &[
                    "crates/perl-core-harness/src/contract.rs",
                    "crates/perl-core-harness/src/tests.rs",
                ],
            )),
        );

        assert_eq!(packet.pointer("/summary/reachable_unrevealed"), Some(&json!(2)));
        assert_eq!(packet.pointer("/summary/severe_gaps"), Some(&json!(2)));
        assert_eq!(packet.pointer("/summary/non_production_excluded"), Some(&json!(0)));
        Ok(())
    }

    /// Suppression policy keeps precedence over the structural filter, so a
    /// reviewed archive suppression is still attributed to policy and the two
    /// mechanisms never double-count one finding.
    #[test]
    fn suppression_takes_precedence_over_the_non_production_filter() -> Result<()> {
        let check_value = json!({
            "summary": { "weakly_exposed": 0, "reachable_unrevealed": 0, "no_static_path": 1 },
            "findings": [
                {
                    "classification": "no_static_path",
                    "seam": { "file": "archive/crates/old-crate/src/lib.rs", "line": 9 }
                }
            ]
        });
        let suppressions = RiprSuppressionRules {
            display_patterns: vec!["archive/**".to_string()],
            path_patterns: vec![Pattern::new("archive/**")?],
            classification_patterns: vec![Vec::new()],
            invalid_patterns: Vec::new(),
            suppression_reasons: Vec::new(),
        };

        let packet = pr_evidence_packet_on_surface(
            &PrEvidenceOptions {
                root: ".".to_string(),
                base: "origin/main".to_string(),
                head: "HEAD".to_string(),
                pr_head_sha: None,
            },
            &["archive/crates/old-crate/src/lib.rs".to_string()],
            &check_value,
            "base-sha",
            "head-sha",
            &suppressions,
            Some(&ProductionSurface::from_parts("/ws", &[])),
        );

        assert_eq!(packet.pointer("/summary/no_static_path"), Some(&json!(0)));
        assert_eq!(packet.pointer("/summary/severe_gaps"), Some(&json!(0)));
        assert_eq!(packet.pointer("/summary/suppressed_by_policy"), Some(&json!(1)));
        assert_eq!(packet.pointer("/summary/non_production_excluded"), Some(&json!(0)));
        Ok(())
    }

    /// Path B (no summary object): bucket totals come from findings, so
    /// non-production findings are simply never added; the transparency total
    /// still reports them.
    #[test]
    fn non_production_findings_without_a_summary_object_do_not_count() -> Result<()> {
        let check_value = json!({
            "findings": [
                {
                    "classification": "no_static_path",
                    "probe": { "path": "crates/perl-semantic-facts/tests/prop_json_roundtrip.rs", "line": 41 }
                },
                {
                    "classification": "no_static_path",
                    "seam": { "file": "archive/crates/tree-sitter-perl-rs/src/scanner/mod.rs", "line": 100 }
                },
                {
                    "grip_class": "reachable_unrevealed",
                    "seam": { "file": "crates/perl-core-harness/src/contract.rs", "line": 12 }
                }
            ]
        });

        let packet = pr_evidence_packet_on_surface(
            &PrEvidenceOptions {
                root: ".".to_string(),
                base: "origin/main".to_string(),
                head: "HEAD".to_string(),
                pr_head_sha: None,
            },
            &["crates/perl-core-harness/src/contract.rs".to_string()],
            &check_value,
            "base-sha",
            "head-sha",
            &no_suppressions(),
            Some(&ProductionSurface::from_parts(
                "/ws",
                &["crates/perl-core-harness/src/contract.rs"],
            )),
        );

        assert_eq!(packet.pointer("/summary/no_static_path"), Some(&json!(0)));
        assert_eq!(packet.pointer("/summary/reachable_unrevealed"), Some(&json!(1)));
        assert_eq!(packet.pointer("/summary/severe_gaps"), Some(&json!(1)));
        assert_eq!(packet.pointer("/summary/non_production_excluded"), Some(&json!(2)));
        Ok(())
    }

    /// A finding whose classification the producer does not recognize is still
    /// dropped when its path is non-production: the defensive severe_gaps
    /// decrement mirrors `suppressed_unclassified` (#1346), and the transparency
    /// total carries it.
    #[test]
    fn unclassified_non_production_findings_decrement_severe_gaps_in_path_a() -> Result<()> {
        let check_value = json!({
            "summary": { "weakly_exposed": 0, "reachable_unrevealed": 1, "no_static_path": 0 },
            "findings": [
                {
                    "classification": "some_future_class",
                    "seam": { "file": "archive/crates/old-crate/src/lib.rs", "line": 9 }
                }
            ]
        });

        let packet = pr_evidence_packet_on_surface(
            &PrEvidenceOptions {
                root: ".".to_string(),
                base: "origin/main".to_string(),
                head: "HEAD".to_string(),
                pr_head_sha: None,
            },
            &["archive/crates/old-crate/src/lib.rs".to_string()],
            &check_value,
            "base-sha",
            "head-sha",
            &no_suppressions(),
            Some(&ProductionSurface::from_parts("/ws", &[])),
        );

        // The unrecognized class cannot be attributed to a bucket, so only the
        // severe_gaps-side decrement brings the count down.
        assert_eq!(packet.pointer("/summary/reachable_unrevealed"), Some(&json!(1)));
        assert_eq!(packet.pointer("/summary/severe_gaps"), Some(&json!(0)));
        assert_eq!(packet.pointer("/summary/non_production_excluded"), Some(&json!(1)));
        Ok(())
    }

    /// The classifier anchors the same path shapes the suppression matcher
    /// accepts (0.9.x Windows and checkout-prefixed absolute paths) — but
    /// against the workspace root, not the earliest anchor substring — and
    /// treats a file named `tests.rs` or a `perl-lsp-ux-tests` crate segment
    /// as production. Only a real `tests/` directory component that the
    /// production surface does not compile classifies.
    #[test]
    fn non_production_paths_classify_windows_and_absolute_forms() {
        // A checkout whose ancestor directory is itself named `archive`
        // (#12267 review repair): active sources must not classify as
        // archived just because an ancestor matches the anchor substring.
        let surface = ProductionSurface::from_parts(
            "/work/archive/perl-lsp-swarm",
            &["crates/perl-core-harness/src/contract.rs"],
        );
        let cases = [
            (r".\crates\perl-x\tests\case.rs", Some(NonProductionKind::IntegrationTest)),
            (
                "//?/H:/Code/Rust3/perl-lsp-swarm/crates/perl-x/tests/case.rs",
                None, // absolute path outside the surface's repository root: fail closed
            ),
            (
                "/work/archive/perl-lsp-swarm/xtask/tests/it/main.rs",
                Some(NonProductionKind::IntegrationTest),
            ),
            (
                // The reviewer's exact wrong candidate: an active source under
                // an ancestor `archive` directory must classify by repo root.
                "/work/archive/perl-lsp-swarm/crates/perl-lsp-rs/src/runtime.rs",
                None,
            ),
            ("./archive/crates/old-crate/src/lib.rs", Some(NonProductionKind::Archive)),
            ("archive/crates/old-crate/src/lib.rs", Some(NonProductionKind::Archive)),
            (
                "/work/archive/perl-lsp-swarm/archive/crates/old/src/lib.rs",
                Some(NonProductionKind::Archive),
            ),
            ("crates/perl-lsp-ux-tests/src/lib.rs", None),
            ("crates/perl-x/src/tests.rs", None),
            ("crates/perl-core-harness/src/contract.rs", None),
            ("xtask/src/tasks/ripr_evidence.rs", None),
        ];
        for (raw, expected) in cases {
            assert_eq!(
                classify_non_production(Some(&surface), raw),
                expected,
                "path {raw:?} classified against root {:?}",
                surface.repo_root
            );
        }
        // No compiled-into-production view at all: nothing is classified —
        // the gate never under-attributes on an unreadable workspace graph.
        assert_eq!(classify_non_production(None, "crates/perl-x/tests/case.rs"), None);
        assert_eq!(classify_non_production(None, "archive/crates/old/src/lib.rs"), None);
    }

    /// P1 regression (#12267 review): a `tests/`-located file that production
    /// code compiles via `#[path]` — the `xtask::emacs_host_run` runner
    /// substrate and the `validate-zed-*` validator substrates — must stay in
    /// the counted basis. Wrong candidate: the lexical `tests` component alone
    /// used to exclude it.
    #[test]
    fn path_included_test_sources_stay_in_the_production_basis() -> Result<()> {
        let surface = ProductionSurface::from_parts(
            "/ws",
            &[
                "xtask/tests/support/emacs_host_runner.rs",
                "xtask/tests/support/zed_host_compat.rs",
                "crates/perl-core-harness/src/contract.rs",
            ],
        );
        // The exact repository seams the reviewer named: exported module
        // substrate and validator binaries compiled from xtask/tests/support.
        assert_eq!(
            classify_non_production(Some(&surface), "xtask/tests/support/emacs_host_runner.rs"),
            None
        );
        assert_eq!(
            classify_non_production(Some(&surface), "/ws/xtask/tests/support/zed_host_compat.rs"),
            None
        );
        // A genuine integration test file no production artifact compiles
        // still classifies.
        assert_eq!(
            classify_non_production(Some(&surface), "crates/perl-x/tests/case.rs"),
            Some(NonProductionKind::IntegrationTest)
        );

        // Counts level: a finding on the `#[path]`-included substrate keeps
        // counting; the integration-test finding does not.
        let check_value = json!({
            "summary": { "weakly_exposed": 0, "reachable_unrevealed": 2, "no_static_path": 0 },
            "findings": [
                {
                    "grip_class": "reachable_unrevealed",
                    "seam": { "file": "xtask/tests/support/emacs_host_runner.rs", "line": 21 }
                },
                {
                    "grip_class": "reachable_unrevealed",
                    "seam": { "file": "crates/perl-x/tests/case.rs", "line": 7 }
                }
            ]
        });
        let packet = pr_evidence_packet_on_surface(
            &PrEvidenceOptions {
                root: ".".to_string(),
                base: "origin/main".to_string(),
                head: "HEAD".to_string(),
                pr_head_sha: None,
            },
            &["xtask/tests/support/emacs_host_runner.rs".to_string()],
            &check_value,
            "base-sha",
            "head-sha",
            &no_suppressions(),
            Some(&surface),
        );
        assert_eq!(packet.pointer("/summary/reachable_unrevealed"), Some(&json!(1)));
        assert_eq!(packet.pointer("/summary/severe_gaps"), Some(&json!(1)));
        assert_eq!(packet.pointer("/summary/non_production_excluded"), Some(&json!(1)));
        assert_eq!(packet.pointer("/non_production/status"), Some(&json!("applied")));
        Ok(())
    }

    /// P1 regression, closure level: the surface builder must actually find
    /// the `#[path]`-included files — target membership alone cannot, because
    /// cargo metadata does not list `#[path]` modules. The scan follows
    /// `#[path = "…"]` includes and plain `mod name;` siblings transitively
    /// from production seeds.
    #[test]
    fn include_closure_marks_path_compiled_test_support_as_production() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let repo = temp.path();
        fs::create_dir_all(repo.join("xtask/src/bin"))?;
        fs::create_dir_all(repo.join("xtask/tests/support"))?;
        // The repository's real shapes: an exported module substrate and a
        // validator binary that compile files from under tests/.
        fs::write(
            repo.join("xtask/src/emacs_host_run.rs"),
            "#[path = \"../tests/support/emacs_host_runner.rs\"]\npub mod emacs_host_runner;\n",
        )?;
        fs::write(
            repo.join("xtask/src/bin/validate-zed-host-receipt.rs"),
            "#[path = \"../../tests/support/zed_host_compat.rs\"]\nmod zed_host_compat;\nfn main() {}\n",
        )?;
        fs::write(
            repo.join("xtask/tests/support/emacs_host_runner.rs"),
            "// runner substrate\nmod deeper;\n",
        )?;
        fs::write(repo.join("xtask/tests/support/zed_host_compat.rs"), "// compat\n")?;
        fs::write(repo.join("xtask/tests/support/deeper.rs"), "// sibling module\n")?;

        let mut production = BTreeSet::from([
            "xtask/src/emacs_host_run.rs".to_string(),
            "xtask/src/bin/validate-zed-host-receipt.rs".to_string(),
        ]);
        scan_include_closure(
            repo,
            &mut production,
            vec![
                "xtask/src/emacs_host_run.rs".to_string(),
                "xtask/src/bin/validate-zed-host-receipt.rs".to_string(),
            ],
        );

        for included in [
            "xtask/tests/support/emacs_host_runner.rs",
            "xtask/tests/support/zed_host_compat.rs",
            // Plain `mod deeper;` from a production-compiled tests/ file joins
            // the closure too (over-inclusive direction: keeps findings counted).
            "xtask/tests/support/deeper.rs",
        ] {
            assert!(production.contains(included), "{included} must be production: {production:?}");
        }
        Ok(())
    }

    /// P1 regression, target membership: a workspace target with a production
    /// kind whose src_path lives under `tests/` compiles that file into an
    /// artifact, so it must not be excluded; a `test`-kind target still does.
    #[test]
    fn production_kind_targets_under_tests_directories_stay_counted() -> Result<()> {
        let metadata = json!({
            "workspace_root": "/ws",
            "packages": [
                {
                    "name": "xtask",
                    "manifest_path": "/ws/xtask/Cargo.toml",
                    "targets": [
                        { "kind": ["lib"], "src_path": "/ws/xtask/src/lib.rs" },
                        { "kind": ["bin"], "src_path": "/ws/xtask/tests/support/cli_harness.rs" },
                        { "kind": ["test"], "src_path": "/ws/xtask/tests/it.rs" }
                    ]
                }
            ]
        });
        let surface = production_surface_from_metadata(Path::new("/nonexistent"), &metadata)?;
        assert!(surface.production_paths.contains("xtask/tests/support/cli_harness.rs"));
        assert!(!surface.production_paths.contains("xtask/tests/it.rs"));
        assert_eq!(
            classify_non_production(Some(&surface), "xtask/tests/support/cli_harness.rs"),
            None
        );
        assert_eq!(
            classify_non_production(Some(&surface), "xtask/tests/it.rs"),
            Some(NonProductionKind::IntegrationTest)
        );
        Ok(())
    }

    /// P2 regression (#12267 review): fallback guidance applies the same
    /// non-production filter as the counted set, before sorting and
    /// truncation — so excluded test seams cannot fill the 25-item fallback
    /// slice and crowd out the production seam that keeps `new_unresolved`
    /// positive. Wrong candidate: without the filter, 25 lexically earlier
    /// `tests/` seams push `xtask/src/tools/prod.rs` past the limit.
    #[test]
    fn fallback_guidance_names_the_filtered_set_not_the_raw_check() -> Result<()> {
        let mut findings = Vec::new();
        for index in 0..25 {
            findings.push(raw_check_finding(
                &format!("probe:test{index:02}"),
                "no_static_path",
                &format!("crates/perl-x/tests/it/case_{index:02}.rs"),
                10 + index,
            ));
        }
        findings.push(raw_check_finding(
            "probe:prod",
            "no_static_path",
            "xtask/src/tools/prod.rs",
            5,
        ));
        let surface = ProductionSurface::from_parts("/ws", &["xtask/src/tools/prod.rs"]);

        let (seams, suppressed) =
            fallback_seam_entries(&findings, &no_suppressions(), None, None, Some(&surface));
        assert_eq!(suppressed, 0);
        let paths: Vec<&str> = seams.iter().map(|(path, _, _, _)| path.as_str()).collect();
        assert_eq!(paths, vec!["xtask/src/tools/prod.rs"], "only the production seam is named");

        // Fail-closed control: without a compiled-into-production view the
        // fallback keeps naming everything (names ⊇ counted still holds).
        let (unfiltered, _) =
            fallback_seam_entries(&findings, &no_suppressions(), None, None, None);
        assert_eq!(unfiltered.len(), 26);
        Ok(())
    }

    /// The pre-streaming DOM pipeline: `fallback_seam_entries` followed by the
    /// sort/dedup/truncate `fallback_guidance_comments` applied to its result.
    /// This is the oracle the streaming accumulator must match exactly.
    fn dom_fallback_pipeline(
        findings: &[Value],
        suppressions: &RiprSuppressionRules,
        head_extents: Option<&HeadLineExtents>,
        attribution: Option<&DependencyAttribution>,
        production_surface: Option<&ProductionSurface>,
    ) -> (Vec<FallbackSeam>, usize) {
        let (mut seams, suppressed) = fallback_seam_entries(
            findings,
            suppressions,
            head_extents,
            attribution,
            production_surface,
        );
        seams.sort_by(|left, right| (&left.0, left.1, &left.2).cmp(&(&right.0, right.1, &right.2)));
        seams.dedup_by(|next, previous| next.0 == previous.0 && next.1 == previous.1);
        seams.truncate(FALLBACK_GUIDANCE_LIMIT);
        (seams, suppressed)
    }

    /// Falsifies a fallback accumulator that loses DOM ordering, deduplication, or the bound.
    #[test]
    fn fallback_streaming_matches_dom_oracle_over_a_wide_payload() -> Result<()> {
        let mut findings = vec![
            raw_check_finding(
                "probe:suppressed",
                "no_static_path",
                "crates/suppressed/src/hidden.rs",
                2,
            ),
            raw_check_finding("probe:archive", "reachable_unrevealed", "archive/old.rs", 3),
        ];
        for index in 0..(FALLBACK_GUIDANCE_LIMIT * 3) {
            findings.push(raw_check_finding(
                &format!("probe:{index:03}"),
                if index % 2 == 0 { "no_static_path" } else { "reachable_unrevealed" },
                &format!("crates/wide/src/file{:02}.rs", (index * 7) % 11),
                ((index * 5) % 29 + 1) as u64,
            ));
        }
        let suppressions = RiprSuppressionRules {
            display_patterns: vec!["crates/suppressed/**".to_string()],
            path_patterns: vec![Pattern::new("crates/suppressed/**")?],
            classification_patterns: vec![Vec::new()],
            invalid_patterns: Vec::new(),
            suppression_reasons: Vec::new(),
        };
        let production_surface = ProductionSurface::from_parts("/ws", &[]);
        let payload = json!({
            "base": "origin/main",
            "summary": {
                "findings": findings.len(),
                "reachable_unrevealed": findings.len(),
                "no_static_path": findings.len()
            },
            "findings": findings
        });
        let payload_text = serde_json::to_string(&payload)?;
        let expected = dom_fallback_pipeline(
            payload
                .get("findings")
                .and_then(Value::as_array)
                .ok_or_else(|| eyre!("findings missing"))?,
            &suppressions,
            None,
            None,
            Some(&production_surface),
        );
        let mut accumulator = FallbackGuidanceAccumulator::default();
        stream_ripr_check_payload_with_events(
            BufReader::with_capacity(64 * 1024, std::io::Cursor::new(payload_text.as_bytes())),
            &mut |event| match event {
                StreamFindingsEvent::Start => accumulator.reset(),
                StreamFindingsEvent::Finding(finding) => accumulator.absorb(
                    finding,
                    &suppressions,
                    None,
                    None,
                    Some(&production_surface),
                ),
            },
        )?;
        assert_eq!(accumulator.finish(), expected);
        assert_eq!(
            expected.0.len(),
            FALLBACK_GUIDANCE_LIMIT,
            "the wide payload must exercise the fallback guidance truncation bound"
        );
        assert_eq!(expected.1, 1, "the suppressed finding must exercise the policy counter");
        assert!(
            expected.0.iter().all(|(path, _, _, _)| !path.starts_with("archive/")),
            "the non-production finding must not reach fallback guidance"
        );
        Ok(())
    }

    /// Falsifies a duplicate-key implementation that keeps a later, larger-id seam.
    #[test]
    fn fallback_streaming_keeps_smallest_id_for_duplicate_path_and_line() -> Result<()> {
        let findings = vec![
            raw_check_finding("probe:z", "no_static_path", "crates/x/src/lib.rs", 7),
            raw_check_finding("probe:a", "no_static_path", "crates/x/src/lib.rs", 7),
        ];
        let expected = dom_fallback_pipeline(&findings, &no_suppressions(), None, None, None);
        let mut accumulator = FallbackGuidanceAccumulator::default();
        for finding in &findings {
            accumulator.absorb(finding, &no_suppressions(), None, None, None);
        }
        let actual = accumulator.finish();
        assert_eq!(actual, expected);
        assert_eq!(actual.0.len(), 1);
        assert_eq!(actual.0[0].2, "probe:a");
        Ok(())
    }

    /// Falsifies a duplicate-findings implementation that accumulates the first array.
    #[test]
    fn fallback_streaming_duplicate_findings_uses_only_the_last_array() -> Result<()> {
        let suppressions = RiprSuppressionRules {
            display_patterns: vec!["crates/suppressed/**".to_string()],
            path_patterns: vec![Pattern::new("crates/suppressed/**")?],
            classification_patterns: vec![Vec::new()],
            invalid_patterns: Vec::new(),
            suppression_reasons: Vec::new(),
        };
        let first_findings = vec![raw_check_finding(
            "probe:first",
            "no_static_path",
            "crates/suppressed/src/hidden.rs",
            2,
        )];
        let second_findings = vec![raw_check_finding(
            "probe:second",
            "reachable_unrevealed",
            "crates/x/src/lib.rs",
            7,
        )];
        let first_text = serde_json::to_string(&first_findings)?;
        let second_text = serde_json::to_string(&second_findings)?;
        let payload_text = format!(
            r#"{{"base":"origin/main","summary":{{}},"findings":{first_text},"findings":{second_text}}}"#
        );
        let expected = dom_fallback_pipeline(&second_findings, &suppressions, None, None, None);
        let mut accumulator = FallbackGuidanceAccumulator::default();
        stream_ripr_check_payload_with_events(
            BufReader::with_capacity(64 * 1024, std::io::Cursor::new(payload_text.as_bytes())),
            &mut |event| match event {
                StreamFindingsEvent::Start => accumulator.reset(),
                StreamFindingsEvent::Finding(finding) => {
                    accumulator.absorb(finding, &suppressions, None, None, None)
                }
            },
        )?;
        let actual = accumulator.finish();
        assert_eq!(actual, expected);
        assert_eq!(actual.1, 0, "the first array's suppressed finding must be reset");
        assert_eq!(actual.0[0].2, "probe:second");
        Ok(())
    }

    /// Fail closed: a finding with no resolvable path is never classified
    /// non-production, exactly like the #6260 head-range filter.
    #[test]
    fn pathless_findings_are_never_non_production() -> Result<()> {
        let check_value = json!({
            "summary": { "weakly_exposed": 0, "reachable_unrevealed": 1, "no_static_path": 0 },
            "findings": [
                {
                    "classification": "reachable_unrevealed",
                    "seam": { "line": 12 }
                }
            ]
        });

        let packet = pr_evidence_packet(
            &PrEvidenceOptions {
                root: ".".to_string(),
                base: "origin/main".to_string(),
                head: "HEAD".to_string(),
                pr_head_sha: None,
            },
            &["crates/perl-core-harness/src/contract.rs".to_string()],
            &check_value,
            "base-sha",
            "head-sha",
            &no_suppressions(),
        );

        assert_eq!(packet.pointer("/summary/reachable_unrevealed"), Some(&json!(1)));
        assert_eq!(packet.pointer("/summary/severe_gaps"), Some(&json!(1)));
        assert_eq!(packet.pointer("/summary/non_production_excluded"), Some(&json!(0)));
        Ok(())
    }

    /// Findings carry checkout-prefixed and Windows-separator paths (see the 0.9.x
    /// suppression cases above), so head-range resolution must survive both.
    #[test]
    fn head_extents_resolve_absolute_and_windows_finding_paths() {
        let extents = HeadLineExtents {
            present: BTreeMap::from([(
                "xtask/src/tasks/check_version_sync.rs".to_string(),
                13usize,
            )]),
            removed: BTreeSet::new(),
        };

        assert_eq!(
            extents
                .resolve("/home/runner/work/perl-lsp-swarm/xtask/src/tasks/check_version_sync.rs"),
            HeadPathState::Present(13)
        );
        assert_eq!(
            extents.resolve("C:\\code\\perl-lsp-swarm\\xtask/src/tasks/check_version_sync.rs"),
            HeadPathState::Present(13)
        );
        // Suffix matching respects path boundaries rather than raw string ends.
        assert_eq!(extents.resolve("vendored_check_version_sync.rs"), HeadPathState::Unknown);
        assert_eq!(extents.resolve("some/other/file.rs"), HeadPathState::Unknown);
    }

    fn diff_receipt(head_sha: &str, entries: Vec<CommittedDiffEntry>) -> CommittedDiffReceipt {
        CommittedDiffReceipt {
            schema_version: "ripr_committed_diff.v1".to_string(),
            base: "origin/main".to_string(),
            head: "HEAD".to_string(),
            base_sha: "base-sha".to_string(),
            head_sha: head_sha.to_string(),
            diff_digest: "digest".to_string(),
            changed_paths: Vec::new(),
            entries,
        }
    }

    /// `git diff --name-status` gives `M` and `T` entries the same value for `old_path`
    /// and `new_path`. Treating "has an old path, has no extent" as removal therefore
    /// turns any failed `git show` on a *modified* file into a phantom deletion, which
    /// would drop every real finding on it — fail-open, the one direction this filter
    /// must never take. Removal is read from the status code instead, so a modified file
    /// whose blob cannot be read resolves `Unknown` and keeps its findings counted.
    #[test]
    fn a_modified_file_with_an_unreadable_blob_is_unknown_not_removed() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let repo = temp.path();
        init_git_repo(repo)?;
        let head = run_git(repo, &["rev-parse", "HEAD"])?;
        // `absent.rs` is not in this revision, so the extent lookup fails the same way an
        // unreadable blob would.
        let diff = diff_receipt(
            &head,
            vec![CommittedDiffEntry {
                status: "M".to_string(),
                old_path: Some("absent.rs".to_string()),
                new_path: Some("absent.rs".to_string()),
            }],
        );

        let extents = HeadLineExtents::from_committed_diff(repo, &diff);

        assert_eq!(extents.resolve("absent.rs"), HeadPathState::Unknown);
        assert!(!extents.finding_is_outside_head(&json!({
            "classification": "no_static_path",
            "probe": { "path": "absent.rs", "line": 4 }
        })));
        Ok(())
    }

    /// A copy leaves its source in place, so `C` must not remove the source path — the
    /// old-path inference this commit replaced would have. The source is not indexed at
    /// all (the index covers head-side paths), so it resolves `Unknown` and its findings
    /// stay counted; what matters is that it is never `Removed`.
    #[test]
    fn a_copy_source_is_not_treated_as_removed() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let repo = temp.path();
        init_git_repo(repo)?;
        let head = run_git(repo, &["rev-parse", "HEAD"])?;
        let diff = diff_receipt(
            &head,
            vec![CommittedDiffEntry {
                status: "C100".to_string(),
                old_path: Some("tracked.txt".to_string()),
                new_path: Some("copy.txt".to_string()),
            }],
        );

        let extents = HeadLineExtents::from_committed_diff(repo, &diff);

        assert_eq!(extents.resolve("tracked.txt"), HeadPathState::Unknown);
        assert!(!extents.finding_is_outside_head(&json!({
            "classification": "no_static_path",
            "probe": { "path": "tracked.txt", "line": 1 }
        })));
        // `copy.txt` is not in this fixture's head revision, so its extent lookup fails
        // and it too stays `Unknown` rather than becoming a phantom deletion.
        assert_eq!(extents.resolve("copy.txt"), HeadPathState::Unknown);
        Ok(())
    }

    /// Wiring proof: the extents actually come from the change's committed diff, so the
    /// filter sees the real head line count rather than a hand-built map.
    #[test]
    fn head_extents_are_built_from_the_committed_diff() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let repo = temp.path();
        let shrunk = repo.join("shrunk.rs");
        let deleted = repo.join("deleted.rs");
        fs::write(&shrunk, (1..=30).map(|n| format!("// line {n}\n")).collect::<String>())?;
        fs::write(&deleted, "// gone\n")?;
        init_git_repo(repo)?;
        run_git(repo, &["add", "shrunk.rs", "deleted.rs"])?;
        run_git(
            repo,
            &["-c", "user.name=test", "-c", "user.email=test@example.com", "commit", "-m", "base"],
        )?;
        let base = run_git(repo, &["rev-parse", "HEAD"])?;
        fs::write(&shrunk, (1..=13).map(|n| format!("// line {n}\n")).collect::<String>())?;
        fs::remove_file(&deleted)?;
        run_git(repo, &["add", "-A"])?;
        run_git(
            repo,
            &["-c", "user.name=test", "-c", "user.email=test@example.com", "commit", "-m", "head"],
        )?;

        let diff = resolve_committed_diff(repo, &base, "HEAD")?;
        let extents = HeadLineExtents::from_committed_diff(repo, &diff);

        assert_eq!(extents.resolve("shrunk.rs"), HeadPathState::Present(13));
        assert_eq!(extents.resolve("deleted.rs"), HeadPathState::Removed);
        assert!(extents.finding_is_outside_head(&json!({
            "classification": "no_static_path",
            "probe": { "path": "shrunk.rs", "line": 29 }
        })));
        assert!(!extents.finding_is_outside_head(&json!({
            "classification": "no_static_path",
            "probe": { "path": "shrunk.rs", "line": 13 }
        })));
        assert!(extents.finding_is_outside_head(&json!({
            "classification": "no_static_path",
            "probe": { "path": "deleted.rs", "line": 1 }
        })));
        Ok(())
    }

    #[test]
    fn write_review_comments_skips_ripr_when_current_pr_evidence_has_no_severe_gaps() -> Result<()>
    {
        let temp = tempfile::tempdir()?;
        let repo = temp.path();
        init_git_repo(repo)?;
        let ripr = write_fake_ripr_binary(repo)?;
        let _override = override_ripr_bin(&ripr)?;
        let options = ReviewCommentsOptions {
            root: ".".to_string(),
            base: "HEAD".to_string(),
            head: "HEAD".to_string(),
            pr_head_sha: None,
            timeout_seconds: Some(1),
        };
        let pr_options = PrEvidenceOptions {
            root: ".".to_string(),
            base: "HEAD".to_string(),
            head: "HEAD".to_string(),
            pr_head_sha: None,
        };
        let head = revision_sha(repo, "HEAD")?;
        let pr_packet = pr_evidence_packet(
            &pr_options,
            &["crates/perl-lsp-ux-tests/tests/ux_scenario_62_project_test_assertion_inline_completion_quality.rs".to_string()],
            &json!({
                "summary": {
                    "weakly_exposed": 0,
                    "reachable_unrevealed": 0,
                    "no_static_path": 0
                }
            }),
            &head,
            &head,
            &RiprSuppressionRules::default(),
        );
        write_text(&repo.join(PR_EVIDENCE_JSON), &format_json(&pr_packet)?)?;

        write_review_comments(repo, &options)?;

        let packet: Value =
            serde_json::from_str(&fs::read_to_string(repo.join(REVIEW_COMMENTS_JSON))?)?;
        let markdown = fs::read_to_string(repo.join(REVIEW_COMMENTS_MD))?;
        assert_eq!(packet["status"], json!("advisory"));
        assert_eq!(packet["mode"], json!("pr_evidence_clean"));
        assert_eq!(
            packet.pointer("/summary/skip_reason"),
            Some(&json!("pr_evidence_zero_severe_gaps"))
        );
        assert!(
            packet.get("comments").and_then(Value::as_array).is_some_and(|items| items.is_empty())
        );
        assert_eq!(packet["head_sha"], json!(head));
        assert!(markdown.contains("zero severe gaps"), "{markdown}");
        Ok(())
    }

    #[test]
    fn current_pr_evidence_has_no_severe_gaps_rejects_stale_receipt() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let repo = temp.path();
        init_git_repo(repo)?;
        fs::create_dir_all(repo.join("target/ripr/pr"))?;
        fs::write(
            repo.join(PR_EVIDENCE_JSON),
            format_json(&json!({
                "base": "HEAD",
                "base_sha": "stale-base",
                "head": "HEAD",
                "head_sha": "stale-head",
                "summary": {
                    "severe_gaps": 0
                }
            }))?,
        )?;
        let options = ReviewCommentsOptions {
            root: ".".to_string(),
            base: "HEAD".to_string(),
            head: "HEAD".to_string(),
            pr_head_sha: None,
            timeout_seconds: None,
        };

        assert!(!current_pr_evidence_has_no_severe_gaps(repo, &options)?);
        Ok(())
    }

    #[test]
    fn stamp_review_comments_receipt_records_current_revisions() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let repo = temp.path();
        init_git_repo(repo)?;
        fs::create_dir_all(repo.join("target/ripr/review"))?;
        fs::write(repo.join(REVIEW_COMMENTS_MD), "# Review comments\n")?;
        fs::write(
            repo.join(REVIEW_COMMENTS_JSON),
            format_json(&json!({
                "schema_version": "0.1",
                "tool": "ripr",
                "status": "advisory",
                "root": ".",
                "base": "HEAD",
                "head": "HEAD",
                "mode": "fast",
                "summary": {},
                "comments": [],
                "summary_only": [],
                "suppressed": [],
                "warnings": []
            }))?,
        )?;
        let options = ReviewCommentsOptions {
            root: ".".to_string(),
            base: "HEAD".to_string(),
            head: "HEAD".to_string(),
            pr_head_sha: None,
            timeout_seconds: None,
        };

        stamp_review_comments_receipt(repo, &options)?;
        validate_review_comments(repo, &options, true)?;
        let packet: Value =
            serde_json::from_str(&fs::read_to_string(repo.join(REVIEW_COMMENTS_JSON))?)?;
        let head = revision_sha(repo, "HEAD")?;

        assert_eq!(packet["base_sha"], json!(head));
        assert_eq!(packet["head_sha"], json!(head));
        Ok(())
    }

    #[test]
    fn stamp_review_comments_receipt_rejects_non_object() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let repo = temp.path();
        init_git_repo(repo)?;
        fs::create_dir_all(repo.join("target/ripr/review"))?;
        fs::write(repo.join(REVIEW_COMMENTS_JSON), "[]\n")?;
        let options = ReviewCommentsOptions {
            root: ".".to_string(),
            base: "HEAD".to_string(),
            head: "HEAD".to_string(),
            pr_head_sha: None,
            timeout_seconds: None,
        };

        assert!(stamp_review_comments_receipt(repo, &options).is_err());
        Ok(())
    }

    #[test]
    fn validate_review_comments_rejects_stale_revision_receipt() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let repo = temp.path();
        init_git_repo(repo)?;
        fs::create_dir_all(repo.join("target/ripr/review"))?;
        fs::write(repo.join(REVIEW_COMMENTS_MD), "# Review comments\n")?;
        fs::write(
            repo.join(REVIEW_COMMENTS_JSON),
            format_json(&json!({
                "schema_version": "0.1",
                "tool": "ripr",
                "status": "advisory",
                "root": ".",
                "base": "HEAD",
                "base_sha": "stale-base",
                "head": "HEAD",
                "head_sha": "stale-head",
                "mode": "fast",
                "summary": {},
                "comments": [],
                "summary_only": [],
                "suppressed": [],
                "warnings": []
            }))?,
        )?;
        let options = ReviewCommentsOptions {
            root: ".".to_string(),
            base: "HEAD".to_string(),
            head: "HEAD".to_string(),
            pr_head_sha: None,
            timeout_seconds: None,
        };

        assert!(validate_review_comments(repo, &options, true).is_err());
        Ok(())
    }

    #[test]
    fn write_error_review_comments_writes_stamped_error_receipts() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let repo = temp.path();
        init_git_repo(repo)?;
        let options = ReviewCommentsOptions {
            root: ".".to_string(),
            base: "HEAD".to_string(),
            head: "HEAD".to_string(),
            pr_head_sha: None,
            timeout_seconds: None,
        };

        write_error_review_comments(
            repo,
            &options,
            r"crates\perl-parser",
            "ripr review-comments failed | timeout\nsecondary detail",
        )?;
        stamp_review_comments_receipt(repo, &options)?;
        validate_review_comments(repo, &options, true)?;

        let packet: Value =
            serde_json::from_str(&fs::read_to_string(repo.join(REVIEW_COMMENTS_JSON))?)?;
        let markdown = fs::read_to_string(repo.join(REVIEW_COMMENTS_MD))?;
        let head = revision_sha(repo, "HEAD")?;

        assert_eq!(packet["status"], json!("error"));
        assert_eq!(packet["root"], json!("crates/perl-parser"));
        assert_eq!(packet["base_sha"], json!(head));
        assert_eq!(packet["head_sha"], json!(head));
        assert_eq!(
            packet.pointer("/warnings/0/message"),
            Some(&json!("ripr review-comments failed | timeout"))
        );
        assert_eq!(packet.pointer("/summary/comments"), Some(&json!(0)));
        assert_eq!(packet.pointer("/summary/summary_only"), Some(&json!(0)));
        assert_eq!(packet.pointer("/summary/suppressed"), Some(&json!(0)));
        assert!(markdown.contains("- status: error"), "{markdown}");
        assert!(markdown.contains("tool_error: ripr review-comments failed \\| timeout"));
        assert!(!markdown.contains("secondary detail"), "{markdown}");
        Ok(())
    }

    // ---------------------------------------------------------------------------
    // Dependency-graph attribution basis (#11690)
    // ---------------------------------------------------------------------------

    /// Fake `cargo metadata` shape: `(name, manifest dir, direct dependencies)`
    /// with manifests at `<root>/<dir>/Cargo.toml`, mirroring this workspace
    /// layout where members live under `crates/` and the root `xtask/`.
    fn fake_workspace_metadata(root: &str, packages: &[(&str, &str, &[&str])]) -> Value {
        let pkg_base = |name: &str, dir: &str| -> String {
            if dir.is_empty() { format!("{root}/{name}") } else { format!("{root}/{dir}/{name}") }
        };
        let pkgs = packages
            .iter()
            .map(|(name, dir, _)| {
                json!({
                    "id": format!("path+file://{}", pkg_base(name, dir)),
                    "name": name,
                    "manifest_path": format!("{}/Cargo.toml", pkg_base(name, dir)),
                })
            })
            .collect::<Vec<_>>();
        let nodes = packages
            .iter()
            .map(|(name, dir, deps)| {
                let edges = deps
                    .iter()
                    .map(|dep| {
                        let dep_dir = packages
                            .iter()
                            .find(|(n, _, _)| n == dep)
                            .map_or("crates", |(_, d, _)| d);
                        json!({"pkg": format!("path+file://{}", pkg_base(dep, dep_dir))})
                    })
                    .collect::<Vec<_>>();
                json!({
                    "id": format!("path+file://{}", pkg_base(name, dir)),
                    "deps": edges,
                })
            })
            .collect::<Vec<_>>();
        json!({
            "workspace_root": root,
            "packages": pkgs,
            "resolve": { "nodes": nodes },
        })
    }

    fn attribution_for(metadata: &Value, changed: &[&str]) -> Result<AttributionScope> {
        let changed = changed.iter().map(|path| path.to_string()).collect::<Vec<_>>();
        Ok(dependency_attribution_scope(metadata, &changed)?)
    }

    /// #6766 reproduction (#11690): a new module in a low-fan-in crate must not
    /// inherit gaps from crates that cannot depend on it. The recorded receipt
    /// scoped 120 production files for a 5-file change, including archived
    /// sources and perl-dap/perl-lsp-rs/perl-workspace/perl-parser — none of
    /// which reference `perl-core-harness`.
    #[test]
    fn low_fan_in_change_drops_non_dependents_and_archived_files() -> Result<()> {
        let metadata = fake_workspace_metadata(
            "/ws",
            &[
                ("perl-core-harness", "crates", &[] as &[&str]),
                ("perl-core-harness-types", "crates", &["perl-core-harness"]),
                ("perl-core-test-runner", "crates", &["perl-core-harness"]),
                ("xtask", "", &["perl-core-harness"]),
                ("perl-dap", "crates", &[]),
                ("perl-lsp-rs", "crates", &["perl-dap"]),
                ("perl-parser", "crates", &[]),
                ("perl-workspace", "crates", &["perl-lsp-rs"]),
            ],
        );
        let scope = attribution_for(&metadata, &["crates/perl-core-harness/src/contract.rs"])?;
        let Some(attribution) = scope.applied() else {
            bail!("a single-crate change must activate the graph filter");
        };
        assert_eq!(attribution.changed_packages, BTreeSet::from(["perl-core-harness".to_string()]),);
        assert_eq!(
            attribution.reachable_packages,
            BTreeSet::from([
                "perl-core-harness".to_string(),
                "perl-core-harness-types".to_string(),
                "perl-core-test-runner".to_string(),
                "xtask".to_string(),
            ]),
        );

        let keep = [
            "crates/perl-core-harness/src/contract.rs",
            "crates/perl-core-harness-types/src/lib.rs",
            "crates/perl-core-test-runner/src/runner.rs",
            "xtask/src/tasks/quality_gate.rs",
        ];
        for path in keep {
            assert_eq!(
                attribution.resolve(path),
                AttributionPathState::InReachablePackage,
                "{path} must stay attributed"
            );
        }

        // The exact non-dependent files #11690 names as wrongly scoped, plus an
        // archived crate that is excluded from the workspace entirely.
        let drop = [
            "archive/crates/tree-sitter-perl-rs/src/scanner/mod.rs",
            "archive/crates/perl-ts-heredoc-parser/src/heredoc_parser.rs",
            "crates/perl-dap/src/debug_adapter/evaluation.rs",
            "crates/perl-lsp-rs/src/runtime/scheduler.rs",
            "crates/perl-workspace/src/semantic/references.rs",
            "crates/perl-parser/src/incremental/incremental_v2.rs",
        ];
        for path in drop {
            assert_eq!(
                attribution.resolve(path),
                AttributionPathState::OutOfGraph,
                "{path} must drop out of scope"
            );
        }
        Ok(())
    }

    /// A dev/optional edge missing from the resolved graph must still keep its
    /// dependent in scope: declared manifest edges are unioned in so no real
    /// linking configuration loses findings (fail-closed, #11690).
    #[test]
    fn declared_edges_absent_from_resolve_keep_dependents_in_scope() -> Result<()> {
        let metadata = json!({
            "workspace_root": "/ws",
            "packages": [
                { "id": "p:a", "name": "gap-base", "manifest_path": "/ws/crates/gap-base/Cargo.toml",
                  "dependencies": [] },
                { "id": "p:b", "name": "gap-dep", "manifest_path": "/ws/crates/gap-dep/Cargo.toml",
                  "dependencies": [ { "name": "gap-base", "optional": true, "kind": null } ] },
                { "id": "p:c", "name": "gap-lone", "manifest_path": "/ws/crates/gap-lone/Cargo.toml",
                  "dependencies": [] },
            ],
            "resolve": { "nodes": [
                // gap-dep's optional dependency on gap-base is NOT resolved here.
                { "id": "p:a", "deps": [] },
                { "id": "p:b", "deps": [] },
                { "id": "p:c", "deps": [] },
            ] },
        });

        let scope = attribution_for(&metadata, &["crates/gap-base/src/lib.rs"])?;
        let Some(attribution) = scope.applied() else {
            bail!("expected applied attribution");
        };

        assert!(
            attribution.reachable_packages.contains("gap-dep"),
            "declared optional dependent must stay in scope"
        );
        assert!(
            !attribution.reachable_packages.contains("gap-lone"),
            "crate with no manifest edge must drop"
        );
        assert_eq!(
            attribution.resolve("crates/gap-dep/src/lib.rs"),
            AttributionPathState::InReachablePackage
        );
        assert_eq!(
            attribution.resolve("crates/gap-lone/src/lib.rs"),
            AttributionPathState::OutOfGraph
        );
        Ok(())
    }

    /// A cross-package rename touches both endpoints: the source package and
    /// its dependents must stay attributed even though only the destination
    /// path survives at HEAD — narrowing past that would reclassify source-
    /// side findings as `out_of_dependency_graph` without reachability basis.
    #[test]
    fn cross_package_rename_attributes_both_endpoints() -> Result<()> {
        let entries = [CommittedDiffEntry {
            status: "R100".to_string(),
            old_path: Some("crates/perl-origin/src/moved.rs".to_string()),
            new_path: Some("crates/perl-destination/src/moved.rs".to_string()),
        }];
        assert_eq!(
            committed_diff_entry_paths(&entries),
            vec![
                "crates/perl-origin/src/moved.rs".to_string(),
                "crates/perl-destination/src/moved.rs".to_string(),
            ]
        );

        let metadata = fake_workspace_metadata(
            "/ws",
            &[
                ("perl-origin", "crates", &[] as &[&str]),
                ("perl-origin-dependent", "crates", &["perl-origin"]),
                ("perl-destination", "crates", &[]),
                ("perl-unrelated", "crates", &[]),
            ],
        );
        let scope = dependency_attribution_scope(&metadata, &committed_diff_entry_paths(&entries))?;
        let Some(attribution) = scope.applied() else {
            bail!("a two-package rename must activate the graph filter");
        };
        assert_eq!(
            attribution.changed_packages,
            BTreeSet::from(["perl-origin".to_string(), "perl-destination".to_string(),])
        );
        for path in [
            "crates/perl-origin/src/still_here.rs",
            "crates/perl-origin-dependent/src/closure.rs",
            "crates/perl-destination/src/new_home.rs",
        ] {
            assert_eq!(
                attribution.resolve(path),
                AttributionPathState::InReachablePackage,
                "{path} must stay attributed"
            );
        }
        assert_eq!(
            attribution.resolve("crates/perl-unrelated/src/lib.rs"),
            AttributionPathState::OutOfGraph,
            "an untouched package must still drop"
        );
        Ok(())
    }

    #[test]
    fn high_fan_in_change_keeps_transitive_dependents_in_scope() -> Result<()> {
        let metadata = fake_workspace_metadata(
            "/ws",
            &[
                ("perl-parser", "crates", &[]),
                ("perl-semantic", "crates", &["perl-parser"]),
                ("perl-lsp-rs", "crates", &["perl-semantic"]),
            ],
        );
        let scope = attribution_for(&metadata, &["crates/perl-parser/src/lib.rs"])?;
        let Some(attribution) = scope.applied() else {
            bail!("a code change must activate the graph filter");
        };

        for path in [
            "crates/perl-parser/src/lib.rs",
            "crates/perl-semantic/src/analysis/index.rs",
            "crates/perl-lsp-rs/src/runtime/scheduler.rs",
        ] {
            assert_eq!(
                attribution.resolve(path),
                AttributionPathState::InReachablePackage,
                "transitive dependent {path} must remain in scope"
            );
        }
        Ok(())
    }

    #[test]
    fn unattributable_paths_stay_counted_fail_open() -> Result<()> {
        let metadata =
            fake_workspace_metadata("/ws", &[("perl-a", "crates", &[]), ("perl-b", "crates", &[])]);
        let scope = attribution_for(&metadata, &["crates/perl-a/src/lib.rs"])?;
        let Some(attribution) = scope.applied() else {
            bail!("expected applied attribution");
        };

        // A host-prefixed path that anchors to no known package, and a finding
        // without any path at all, resolve to Unknown — never filtered.
        assert_eq!(
            attribution.resolve(r"E:\elsewhere\mystery\src\lib.rs"),
            AttributionPathState::Unknown
        );
        assert!(!attribution.finding_is_out_of_graph(&json!({"classification": "no_static_path"})));
        Ok(())
    }

    #[test]
    fn shared_workspace_input_change_keeps_everything_counted() -> Result<()> {
        let metadata = fake_workspace_metadata(
            "/ws",
            &[("perl-a", "crates", &[]), ("perl-b", "crates", &["perl-a"])],
        );

        for shared in ["Cargo.lock", "Cargo.toml", ".cargo/config.toml"] {
            let scope = attribution_for(&metadata, &[shared])?;
            assert_eq!(scope.status(), "shared_workspace_input_kept_all", "{shared}");
            assert!(scope.applied().is_none(), "{shared} must not filter");
        }
        Ok(())
    }

    #[test]
    fn change_without_workspace_package_files_keeps_attribution_inactive() -> Result<()> {
        let metadata = fake_workspace_metadata("/ws", &[("perl-a", "crates", &[])]);
        let scope = attribution_for(&metadata, &["docs/ci/ripr.md"])?;

        assert_eq!(scope.status(), "no_changed_package_kept_all");
        assert!(scope.applied().is_none());
        Ok(())
    }

    #[test]
    fn malformed_metadata_reports_unavailable_instead_of_guessing() {
        let missing_packages = json!({ "workspace_root": "/ws" });
        let changed = ["crates/x/src/lib.rs".to_string()];
        assert!(dependency_attribution_scope(&missing_packages, &changed).is_err());

        let no_root = json!({ "packages": [] });
        assert!(dependency_attribution_scope(&no_root, &changed).is_err());
    }

    /// Packet-level #6766-style dry-run: four findings from a raw check whose
    /// scan expanded past the diff; only changed-plus-dependent seams survive,
    /// and the packet records exactly what was dropped and why. The archived
    /// seam reports under `non_production_excluded` even with the graph
    /// active: the structural filter takes precedence over graph attribution
    /// (#12267 review repair).
    #[test]
    fn packet_narrows_new_gaps_to_reachable_dependents_and_stamps_the_basis() -> Result<()> {
        let options = PrEvidenceOptions {
            root: ".".to_string(),
            base: "origin/main".to_string(),
            head: "HEAD".to_string(),
            pr_head_sha: None,
        };
        let suppressions = no_suppressions();
        let check_value = json!({
            "summary": { "weakly_exposed": 0, "reachable_unrevealed": 4, "no_static_path": 0 },
            "findings": [
                raw_check_finding("probe:changed", "reachable_unrevealed", "crates/perl-core-harness/src/contract.rs", 10),
                raw_check_finding("probe:dependent", "reachable_unrevealed", "crates/perl-core-test-runner/src/runner.rs", 20),
                raw_check_finding("probe:dap", "reachable_unrevealed", "crates/perl-dap/src/debug_adapter/evaluation.rs", 30),
                raw_check_finding("probe:archived", "reachable_unrevealed", "archive/crates/tree-sitter-perl-rs/src/scanner/mod.rs", 40),
            ]
        });
        let metadata = fake_workspace_metadata(
            "/ws",
            &[
                ("perl-core-harness", "crates", &[] as &[&str]),
                ("perl-core-test-runner", "crates", &["perl-core-harness"]),
                ("perl-dap", "crates", &[]),
            ],
        );
        let surface = ProductionSurface::from_parts(
            "/ws",
            &[
                "crates/perl-core-harness/src/contract.rs",
                "crates/perl-core-test-runner/src/runner.rs",
                "crates/perl-dap/src/debug_adapter/evaluation.rs",
            ],
        );
        let scope = attribution_for(&metadata, &["crates/perl-core-harness/src/contract.rs"])?;
        let packet = pr_evidence_packet_with_count(
            &options,
            &check_value,
            "base-sha",
            "head-sha",
            &suppressions,
            None,
            PrEvidenceContext {
                changed_file_count: 5,
                attribution_scope: Some(&scope),
                production_surface: Some(&surface),
            },
        );

        assert_eq!(packet.pointer("/summary/reachable_unrevealed"), Some(&json!(2)));
        assert_eq!(packet.pointer("/summary/severe_gaps"), Some(&json!(2)));
        // The archived seam is attributed to the structural non-production
        // filter, not swallowed by the earlier graph branch; only the
        // non-dependent perl-dap seam lands in the graph bucket.
        assert_eq!(packet.pointer("/summary/out_of_dependency_graph"), Some(&json!(1)));
        assert_eq!(packet.pointer("/summary/non_production_excluded"), Some(&json!(1)));
        assert_eq!(packet.pointer("/attribution/basis"), Some(&json!(ATTRIBUTION_BASIS)));
        assert_eq!(packet.pointer("/attribution/graph_source"), Some(&json!("cargo_metadata")));
        assert_eq!(packet.pointer("/attribution/status"), Some(&json!("applied")));
        assert_eq!(packet.pointer("/non_production/status"), Some(&json!("applied")));

        // The same raw check with no attribution scope still applies the
        // structural non-production filter (#11690): the archived seam drops
        // there too, while the non-dependent perl-dap seam only drops via the
        // graph — 3 versus the pre-#11690 basis of 4.
        let unfiltered = pr_evidence_packet_with_count(
            &options,
            &check_value,
            "base-sha",
            "head-sha",
            &suppressions,
            None,
            PrEvidenceContext {
                changed_file_count: 5,
                attribution_scope: None,
                production_surface: Some(&surface),
            },
        );
        assert_eq!(unfiltered.pointer("/summary/severe_gaps"), Some(&json!(3)));
        assert_eq!(unfiltered.pointer("/summary/non_production_excluded"), Some(&json!(1)));
        assert_eq!(unfiltered.pointer("/attribution/status"), Some(&json!("unavailable")));
        validate_pr_evidence_packet(&packet, &options, 5, true, "base-sha", "head-sha")?;
        Ok(())
    }

    #[test]
    fn validate_pr_evidence_packet_requires_the_attribution_stamp() -> Result<()> {
        let options = PrEvidenceOptions {
            root: ".".to_string(),
            base: "origin/main".to_string(),
            head: "HEAD".to_string(),
            pr_head_sha: None,
        };
        let suppressions = no_suppressions();
        let check_value = json!({
            "summary": { "weakly_exposed": 0, "reachable_unrevealed": 1, "no_static_path": 0 },
            "findings": [raw_check_finding("probe:x", "reachable_unrevealed", "crates/a/src/lib.rs", 5)]
        });
        let mut packet = pr_evidence_packet_with_count(
            &options,
            &check_value,
            "base-sha",
            "head-sha",
            &suppressions,
            None,
            PrEvidenceContext {
                changed_file_count: 1,
                attribution_scope: None,
                production_surface: None,
            },
        );
        let Some(packet_object) = packet.as_object_mut() else {
            bail!("packet must be a JSON object");
        };
        packet_object.remove("attribution");

        let Err(err) =
            validate_pr_evidence_packet(&packet, &options, 1, true, "base-sha", "head-sha")
        else {
            bail!("a packet without the attribution stamp must violate the contract");
        };
        assert!(err.to_string().contains("attribution"), "{err}");
        Ok(())
    }

    fn raw_check_finding(id: &str, classification: &str, file: &str, line: u64) -> Value {
        json!({
            "id": id,
            "classification": classification,
            "probe": {
                "id": id,
                "family": "error_path",
                "file": file,
                "line": line,
                "expression": "return Err(ModelError::Failed);"
            },
            "ripr": {
                "reach": { "state": "no", "summary": "No static test path found for the changed owner" }
            }
        })
    }

    #[test]
    fn write_degraded_review_comments_names_actionable_seams_from_raw_check() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let repo = temp.path();
        init_git_repo(repo)?;
        fs::create_dir_all(repo.join("policy"))?;
        fs::write(
            repo.join("policy/ripr-suppressions.toml"),
            "[[suppress]]\nid = \"test-suppression\"\nkind = \"test_receipt_surface\"\npaths = [\"crates/suppressed/**\"]\nreason = \"test fixture\"\n",
        )?;
        let raw_check = repo.join(PR_RAW_CHECK_JSON);
        if let Some(parent) = raw_check.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut large_finding = raw_check_finding(
            "probe:large",
            "reachable_unrevealed",
            "/abs/repo/crates/foo/src/large.rs",
            5,
        );
        large_finding["irrelevant_diagnostics"] = Value::String("x".repeat(4 * 1024 * 1024));
        fs::write(
            &raw_check,
            json!({
                "base": "HEAD",
                "findings": [
                    large_finding,
                    raw_check_finding("probe:b20", "reachable_unrevealed", "/abs/repo/crates/foo/src/b.rs", 20),
                    raw_check_finding("probe:a10b", "no_static_path", "/abs/repo/crates/foo/src/a.rs", 10),
                    raw_check_finding("probe:a10a", "no_static_path", "/abs/repo/crates/foo/src/a.rs", 10),
                    raw_check_finding("probe:c30", "exposed", "/abs/repo/crates/foo/src/c.rs", 30),
                    raw_check_finding("probe:d40", "no_static_path", "/abs/repo/crates/suppressed/src/d.rs", 40),
                    // ripr 0.9.x shape: grip_class + seam.file (no probe node).
                    {
                        "id": "probe:e50",
                        "grip_class": "no_static_path",
                        "seam": {
                            "id": "probe:e50",
                            "family": "match_arm",
                            "file": "/abs/repo/crates/foo/src/e.rs",
                            "line": 50,
                            "expression": "Self::Fallback => write!(f, \"fallback\")"
                        },
                        "ripr": { "reach": { "state": "no", "summary": "No static test path found" } }
                    }
                ]
            })
            .to_string(),
        )?;
        let options = ReviewCommentsOptions {
            root: ".".to_string(),
            base: "HEAD".to_string(),
            head: "HEAD".to_string(),
            pr_head_sha: None,
            timeout_seconds: None,
        };

        write_degraded_review_comments(repo, &options, ".", "ripr timed out after 600s\n detail")?;
        stamp_review_comments_receipt(repo, &options)?;
        validate_review_comments(repo, &options, true)?;

        let packet: Value =
            serde_json::from_str(&fs::read_to_string(repo.join(REVIEW_COMMENTS_JSON))?)?;
        assert_eq!(packet["status"], json!("incomplete"));
        assert_eq!(packet.pointer("/summary/summary_only"), Some(&json!(4)));
        assert_eq!(packet.pointer("/summary/suppressed"), Some(&json!(1)));
        assert_eq!(packet.pointer("/warnings/0/kind"), Some(&json!("tool_error")), "{packet}");
        assert_eq!(packet.pointer("/warnings/1/kind"), Some(&json!("guidance_fallback")));

        let items = packet
            .get("summary_only")
            .and_then(Value::as_array)
            .ok_or_else(|| eyre!("missing summary_only array"))?;
        assert_eq!(items[0]["path"], json!("crates/foo/src/a.rs"));
        assert_eq!(items[0]["line"], json!(10));
        // probe:a10b arrives before probe:a10a at the same (path, line). The
        // smaller id must win, and the pair must collapse to one entry —
        // without this the test passes whichever duplicate the fallback kept.
        assert_eq!(items[0]["id"], json!("probe:a10a"));
        assert_eq!(
            items.iter().filter(|item| item["path"] == json!("crates/foo/src/a.rs")).count(),
            1,
            "duplicate (path, line) seams must collapse to one entry"
        );
        assert_eq!(items[1]["path"], json!("crates/foo/src/b.rs"));
        assert_eq!(items[2]["path"], json!("crates/foo/src/e.rs"));
        assert_eq!(items[2]["line"], json!(50));
        assert_eq!(items[3]["path"], json!("crates/foo/src/large.rs"));
        assert_eq!(items[3]["line"], json!(5));
        for item in items {
            for key in ["id", "path", "seam", "reason", "suggested_test"] {
                assert!(
                    item.get(key).and_then(Value::as_str).is_some_and(|v| !v.is_empty()),
                    "{item}"
                );
            }
            assert!(item.get("line").and_then(Value::as_u64).is_some_and(|line| line > 0));
        }

        let markdown = fs::read_to_string(repo.join(REVIEW_COMMENTS_MD))?;
        assert!(markdown.contains("- status: incomplete"), "{markdown}");
        assert!(markdown.contains("crates/foo/src/a.rs:10"), "{markdown}");
        assert!(markdown.contains("tool_error: ripr timed out after 600s"), "{markdown}");
        Ok(())
    }

    /// Falsifies a fallback accumulator whose duplicate replacement and
    /// truncation bound interact, through the same production entry point.
    ///
    /// The wide-payload test below pins the bound, the sort-order window, and
    /// a late *smaller*-id duplicate. It cannot see the rest of the ordering
    /// algebra, because every one of its keys is distinct and its only
    /// duplicate happens to be the one that should win: an accumulator that
    /// simply took the last id for a key would pass it. Retaining bounded
    /// state makes three more cases reachable, and all three are silent
    /// wrong-answer bugs rather than crashes:
    ///
    /// - a *larger* id arriving for a retained key must not displace it;
    /// - a key already pushed past the bound must stay dropped when it
    ///   reappears with a smaller id, because ordering between distinct keys
    ///   never depends on the id;
    /// - a key sorting before the whole window must still get in and evict
    ///   the current largest.
    ///
    /// The DOM oracle sorts, dedups, and truncates the entire set, so it is
    /// indifferent to arrival order. This pins the streamed receipt to it.
    #[test]
    fn fallback_guidance_orders_duplicates_across_the_truncation_bound() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let repo = temp.path();
        init_git_repo(repo)?;
        fs::create_dir_all(repo.join("policy"))?;
        fs::write(repo.join("policy/ripr-suppressions.toml"), "")?;
        let raw_check = repo.join(PR_RAW_CHECK_JSON);
        if let Some(parent) = raw_check.parent() {
            fs::create_dir_all(parent)?;
        }

        let actionable = "no_static_path";
        let mut findings = Vec::new();
        // Saturate the bound with distinct mid-range keys and mid-range ids.
        for index in 0..FALLBACK_GUIDANCE_LIMIT {
            findings.push(raw_check_finding(
                &format!("probe:m{index:03}"),
                actionable,
                &format!("crates/m/src/f{index:03}.rs"),
                1,
            ));
        }
        // A retained key gains a smaller id after the bound is already full.
        findings.push(raw_check_finding("probe:a000", actionable, "crates/m/src/f005.rs", 1));
        // A retained key is offered a larger id, which must not displace it.
        findings.push(raw_check_finding("probe:z999", actionable, "crates/m/src/f006.rs", 1));
        // A key sorting past the bound is dropped, then reappears with a
        // smaller id: still out, because the id never orders distinct keys.
        findings.push(raw_check_finding("probe:z000", actionable, "crates/z/src/late.rs", 1));
        findings.push(raw_check_finding("probe:a001", actionable, "crates/z/src/late.rs", 1));
        // A key sorting before every retained entry must evict the largest.
        findings.push(raw_check_finding("probe:m999", actionable, "crates/0/src/early.rs", 1));

        fs::write(&raw_check, json!({ "base": "HEAD", "findings": findings }).to_string())?;
        let options = ReviewCommentsOptions {
            root: ".".to_string(),
            base: "HEAD".to_string(),
            head: "HEAD".to_string(),
            pr_head_sha: None,
            timeout_seconds: None,
        };
        write_degraded_review_comments(repo, &options, ".", "ripr timed out after 600s")?;

        let packet: Value =
            serde_json::from_str(&fs::read_to_string(repo.join(REVIEW_COMMENTS_JSON))?)?;
        let items = packet
            .get("summary_only")
            .and_then(Value::as_array)
            .ok_or_else(|| eyre!("missing summary_only array"))?;

        // The DOM oracle over the same arrival order, as the receipt renders it.
        let expected = dom_fallback_pipeline(&findings, &no_suppressions(), None, None, None);
        let expected_paths = expected.0.iter().map(|(path, ..)| path.clone()).collect::<Vec<_>>();
        let actual_paths =
            items.iter().map(|item| option_string_field(Some(item), "path")).collect::<Vec<_>>();
        assert_eq!(actual_paths, expected_paths, "retained window must match the DOM oracle");

        assert_eq!(items.len(), FALLBACK_GUIDANCE_LIMIT, "the bound must stay saturated");
        let id_for = |path: &str| {
            items
                .iter()
                .find(|item| item["path"] == json!(path))
                .map(|item| option_string_field(Some(item), "id"))
        };
        assert_eq!(
            id_for("crates/m/src/f005.rs").as_deref(),
            Some("probe:a000"),
            "a smaller id arriving after the bound is full must replace the retained seam"
        );
        assert_eq!(
            id_for("crates/m/src/f006.rs").as_deref(),
            Some("probe:m006"),
            "a larger id must not displace the retained seam"
        );
        assert_eq!(id_for("crates/z/src/late.rs"), None, "a key past the bound stays dropped");
        assert_eq!(
            id_for("crates/0/src/early.rs").as_deref(),
            Some("probe:m999"),
            "a key sorting before the window must evict the largest retained seam"
        );
        assert_eq!(
            id_for(&format!("crates/m/src/f{:03}.rs", FALLBACK_GUIDANCE_LIMIT - 1)),
            None,
            "the evicted seam must be the largest retained key"
        );
        Ok(())
    }

    /// Drives the production fallback seam (`fallback_guidance_comments` via
    /// `write_degraded_review_comments`) with the payload shape #12860 actually
    /// produced: many findings, each carrying an unconsumed blob the receipt
    /// never reads.
    ///
    /// The existing fallback tests either reconstruct the accumulator loop by
    /// hand — which leaves the production wiring (`BufReader` transport, the
    /// `Start`/`Finding` event match, the base-ref guard) unproven — or use a
    /// payload too narrow to reach `FALLBACK_GUIDANCE_LIMIT`. This closes both:
    /// truncation and duplicate-key replacement are exercised through the same
    /// entry point production uses, on a payload whose findings array is far
    /// larger than anything the fallback is allowed to retain.
    ///
    /// Falsifies three wrong implementations: one that buffers the findings
    /// array, one that truncates before applying a later smaller-id
    /// replacement, and one that lets a duplicate `(path, line)` occupy two of
    /// the bounded slots.
    #[test]
    fn fallback_guidance_streams_a_wide_payload_and_replaces_duplicates() -> Result<()> {
        const SEAM_COUNT: usize = 40;
        const BLOB_BYTES: usize = 256 * 1024;

        let temp = tempfile::tempdir()?;
        let repo = temp.path();
        init_git_repo(repo)?;
        fs::create_dir_all(repo.join("policy"))?;
        fs::write(repo.join("policy/ripr-suppressions.toml"), "")?;
        let raw_check = repo.join(PR_RAW_CHECK_JSON);
        if let Some(parent) = raw_check.parent() {
            fs::create_dir_all(parent)?;
        }

        // Distinct (path, line) seams, ordered so the retained window is the
        // first FALLBACK_GUIDANCE_LIMIT paths by sort order, not by arrival.
        let mut findings = Vec::with_capacity(SEAM_COUNT + 1);
        for index in (0..SEAM_COUNT).rev() {
            let mut finding = raw_check_finding(
                &format!("probe:seam{index:03}z"),
                "no_static_path",
                &format!("crates/foo/src/seam{index:03}.rs"),
                7,
            );
            finding["irrelevant_diagnostics"] = Value::String("x".repeat(BLOB_BYTES));
            findings.push(finding);
        }
        // A late duplicate of a retained seam, carrying a smaller id. It must
        // replace the retained entry rather than add a slot — and it arrives
        // after truncation has already discarded the tail.
        findings.push(raw_check_finding(
            "probe:seam000a",
            "no_static_path",
            "crates/foo/src/seam000.rs",
            7,
        ));

        let payload = json!({ "base": "HEAD", "findings": findings }).to_string();
        assert!(
            payload.len() > SEAM_COUNT * BLOB_BYTES,
            "the payload must be far larger than the retained window"
        );
        fs::write(&raw_check, &payload)?;

        let options = ReviewCommentsOptions {
            root: ".".to_string(),
            base: "HEAD".to_string(),
            head: "HEAD".to_string(),
            pr_head_sha: None,
            timeout_seconds: None,
        };
        write_degraded_review_comments(repo, &options, ".", "ripr timed out after 600s")?;

        let packet: Value =
            serde_json::from_str(&fs::read_to_string(repo.join(REVIEW_COMMENTS_JSON))?)?;
        let items = packet
            .get("summary_only")
            .and_then(Value::as_array)
            .ok_or_else(|| eyre!("missing summary_only array"))?;

        assert_eq!(
            items.len(),
            FALLBACK_GUIDANCE_LIMIT,
            "a payload of {SEAM_COUNT} seams must truncate to the guidance bound"
        );
        // The retained window is the sort-order prefix, not the arrival prefix.
        for (index, item) in items.iter().enumerate() {
            assert_eq!(item["path"], json!(format!("crates/foo/src/seam{index:03}.rs")), "{item}");
        }
        // The late duplicate replaced the retained entry in place.
        assert_eq!(items[0]["id"], json!("probe:seam000a"));
        assert_eq!(
            items.iter().filter(|item| item["path"] == json!("crates/foo/src/seam000.rs")).count(),
            1,
            "the duplicate (path, line) must not consume a second bounded slot"
        );
        // No blob reached the receipt: the fallback reads the seam, not the payload.
        assert!(
            !fs::read_to_string(repo.join(REVIEW_COMMENTS_JSON))?.contains(&"x".repeat(1024)),
            "unconsumed finding fields must not reach the receipt"
        );
        Ok(())
    }

    #[test]
    fn write_degraded_review_comments_without_raw_check_keeps_error_receipt() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let repo = temp.path();
        init_git_repo(repo)?;
        let options = ReviewCommentsOptions {
            root: ".".to_string(),
            base: "HEAD".to_string(),
            head: "HEAD".to_string(),
            pr_head_sha: None,
            timeout_seconds: None,
        };

        write_degraded_review_comments(repo, &options, ".", "ripr timed out after 600s")?;

        let packet: Value =
            serde_json::from_str(&fs::read_to_string(repo.join(REVIEW_COMMENTS_JSON))?)?;
        assert_eq!(packet["status"], json!("error"));
        assert_eq!(packet.pointer("/summary/summary_only"), Some(&json!(0)));
        Ok(())
    }

    #[test]
    fn render_review_comment_markdown_falls_back_and_escapes_warnings() {
        let clean = render_clean_review_comments_markdown(&json!({}));

        assert!(clean.contains(&format!("- base: `{DEFAULT_BASE}`")), "{clean}");
        assert!(clean.contains(&format!("- head: `{DEFAULT_HEAD}`")), "{clean}");
        assert!(clean.contains("No review guidance was generated"), "{clean}");

        let error = render_error_review_comments_markdown(&json!({
            "base": "feature/base",
            "head": "topic/head",
            "warnings": [
                {
                    "kind": "tool_error",
                    "message": "ripr failed | timed out\nsecondary detail"
                }
            ]
        }));

        assert!(error.contains("- status: error"), "{error}");
        assert!(error.contains("- base: `feature/base`"), "{error}");
        assert!(error.contains("- head: `topic/head`"), "{error}");
        assert!(
            error.contains("tool_error: ripr failed \\| timed out secondary detail"),
            "{error}"
        );

        let missing_warning = render_error_review_comments_markdown(&json!({}));
        assert!(
            missing_warning.contains("review guidance generation did not complete"),
            "{missing_warning}"
        );
    }

    #[test]
    fn render_pr_evidence_summary_surfaces_error_review_guidance() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let repo = temp.path();
        init_git_repo(repo)?;
        let options = ReviewCommentsOptions {
            root: ".".to_string(),
            base: "HEAD".to_string(),
            head: "HEAD".to_string(),
            pr_head_sha: None,
            timeout_seconds: None,
        };
        let pr_options = PrEvidenceOptions {
            root: ".".to_string(),
            base: "HEAD".to_string(),
            head: "HEAD".to_string(),
            pr_head_sha: None,
        };
        let head = revision_sha(repo, "HEAD")?;
        let pr_packet = pr_evidence_packet(
            &pr_options,
            &["xtask/src/tasks/ripr_evidence.rs".to_string()],
            &json!({
                "summary": {
                    "weakly_exposed": 0,
                    "reachable_unrevealed": 0,
                    "no_static_path": 0
                }
            }),
            &head,
            &head,
            &RiprSuppressionRules::default(),
        );
        write_text(&repo.join(PR_EVIDENCE_JSON), &format_json(&pr_packet)?)?;
        write_text(&repo.join(PR_EVIDENCE_MD), &render_pr_evidence_markdown(&pr_packet))?;
        write_error_review_comments(repo, &options, ".", "ripr review-comments failed")?;
        stamp_review_comments_receipt(repo, &options)?;

        let summary = render_pr_evidence_summary(repo);

        assert!(summary.contains("- PR evidence JSON: present"), "{summary}");
        assert!(summary.contains("- review guidance JSON: present"), "{summary}");
        assert!(summary.contains("- review guidance status: `error`"), "{summary}");
        assert!(summary.contains("- changed-line comments: 0"), "{summary}");
        assert!(summary.contains("- summary-only guidance: 0"), "{summary}");
        assert!(summary.contains("- suppressed guidance: 0"), "{summary}");
        assert!(
            summary.contains(
                "| Review guidance Markdown | `target/ripr/review/comments.md` | present |"
            ),
            "{summary}"
        );
        Ok(())
    }

    #[test]
    fn render_annotations_emits_escaped_github_warning_packets() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let repo = temp.path();
        fs::create_dir_all(repo.join("target/ripr/review"))?;
        fs::write(
            repo.join(REVIEW_COMMENTS_JSON),
            format_json(&json!({
                "comments": [
                    {
                        "placement": {
                            "path": "crates/perl-parser/src/lib.rs",
                            "line": 42,
                            "mode": "exact_seam_line"
                        },
                        "severity": "strong:gap",
                        "kind": "focused,test",
                        "reason": "branch lacks boundary proof: below, equal, above",
                        "suggested_test": {
                            "intent": "add % branch table"
                        }
                    }
                ]
            }))?,
        )?;

        let rendered = render_annotations(repo, REVIEW_COMMENTS_JSON)?;

        assert!(!rendered.comments_missing);
        assert!(rendered.text.contains("::warning file=crates/perl-parser/src/lib.rs,line=42"));
        assert!(rendered.text.contains("title=ripr strong%3Agap focused%2Ctest"));
        assert!(rendered.text.contains("boundary proof%3A below%2C equal%2C above"));
        assert!(rendered.text.contains("Suggested test%3A add %25 branch table"));
        Ok(())
    }

    #[test]
    fn render_annotations_rejects_summary_only_placement_as_not_annotation_safe() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let repo = temp.path();
        fs::create_dir_all(repo.join("target/ripr/review"))?;
        fs::write(
            repo.join(REVIEW_COMMENTS_JSON),
            format_json(&json!({
                "comments": [
                    {
                        "placement": {
                            "path": "crates/perl-parser/src/lib.rs",
                            "line": 42,
                            "mode": "summary_only"
                        },
                        "reason": "summary-only guidance should not become a line annotation"
                    }
                ]
            }))?,
        )?;

        let err = match render_annotations(repo, REVIEW_COMMENTS_JSON) {
            Ok(_) => bail!("summary-only placement must not become a warning annotation"),
            Err(err) => err,
        };

        assert!(err.to_string().contains("not annotation-safe"));
        Ok(())
    }

    #[test]
    fn render_annotations_skips_missing_comments_file() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let repo = temp.path();

        let rendered = render_annotations(repo, REVIEW_COMMENTS_JSON)?;

        assert!(rendered.comments_missing);
        assert!(rendered.text.is_empty());
        Ok(())
    }

    #[test]
    fn render_annotations_rejects_missing_comments_array() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let repo = temp.path();
        fs::create_dir_all(repo.join("target/ripr/review"))?;
        fs::write(repo.join(REVIEW_COMMENTS_JSON), format_json(&json!({ "summary": {} }))?)?;

        let err = match render_annotations(repo, REVIEW_COMMENTS_JSON) {
            Ok(_) => bail!("comments[] must be required for annotation rendering"),
            Err(err) => err,
        };

        assert!(err.to_string().contains("missing comments[]"));
        Ok(())
    }

    #[test]
    fn annotation_from_comment_rejects_missing_placement_fields() -> Result<()> {
        let missing_path = annotation_from_comment(&json!({
            "placement": {
                "line": 42,
                "mode": "exact_seam_line"
            }
        }));
        assert!(missing_path.is_err());

        let missing_line = annotation_from_comment(&json!({
            "placement": {
                "path": "xtask/src/tasks/ripr_evidence.rs",
                "mode": "exact_seam_line"
            }
        }));
        assert!(missing_line.is_err());

        let missing_mode = annotation_from_comment(&json!({
            "placement": {
                "path": "xtask/src/tasks/ripr_evidence.rs",
                "line": 42
            }
        }));
        assert!(missing_mode.is_err());
        Ok(())
    }

    #[test]
    fn annotation_from_comment_uses_defaults_and_rejects_empty_path() -> Result<()> {
        let annotation = annotation_from_comment(&json!({
            "placement": {
                "path": "xtask/src/tasks/ripr_evidence.rs",
                "line": 7,
                "mode": "owner_function_changed_line"
            }
        }))?;

        assert_eq!(
            annotation,
            "::warning file=xtask/src/tasks/ripr_evidence.rs,line=7,title=ripr advisory focused_test::RIPR review guidance"
        );

        let err = match annotation_from_comment(&json!({
            "placement": {
                "path": "   ",
                "line": 7,
                "mode": "owner_function_changed_line"
            }
        })) {
            Ok(annotation) => bail!("empty path must fail, got {annotation}"),
            Err(err) => err,
        };

        assert!(err.to_string().contains("placement.path"));
        Ok(())
    }

    #[test]
    fn impacted_evidence_routes_severe_ripr_gap_without_label() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let repo = temp.path();
        fs::create_dir_all(repo.join("target/ripr/pr"))?;
        fs::write(
            repo.join(PR_EVIDENCE_JSON),
            format_json(&json!({
                "summary": {
                    "ripr_severe_gap": true,
                    "requires_targeted_mutation": false
                }
            }))?,
        )?;
        let options = ImpactedEvidenceOptions {
            pr_evidence: PR_EVIDENCE_JSON.to_string(),
            labels: Vec::new(),
        };

        let packet = impacted_evidence_packet(repo, &options);

        assert_eq!(packet.pointer("/status"), Some(&json!("advisory")));
        assert_eq!(packet.pointer("/summary/mutation_mode"), Some(&json!("targeted")));
        assert_eq!(packet.pointer("/summary/requires_targeted_mutation"), Some(&json!(true)));
        assert_eq!(packet.pointer("/summary/routing_reason"), Some(&json!("ripr severe gap")));
        assert_eq!(packet.pointer("/artifacts/2/available"), Some(&json!(true)));
        Ok(())
    }

    #[test]
    fn impacted_evidence_missing_pr_receipt_keeps_label_route_and_warns() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let repo = temp.path();
        let options = ImpactedEvidenceOptions {
            pr_evidence: PR_EVIDENCE_JSON.to_string(),
            labels: vec!["release-risk".to_string()],
        };

        let packet = impacted_evidence_packet(repo, &options);
        let markdown = render_impacted_evidence_markdown(&packet);

        assert_eq!(packet.pointer("/status"), Some(&json!("incomplete")));
        assert_eq!(packet.pointer("/summary/mutation_mode"), Some(&json!("targeted")));
        assert_eq!(packet.pointer("/summary/routing_reason"), Some(&json!("release-risk label")));
        assert_eq!(packet.pointer("/artifacts/2/available"), Some(&json!(false)));
        assert_eq!(packet.pointer("/warnings/0/kind"), Some(&json!("missing_artifact")));
        assert!(markdown.contains("missing_artifact"));
        assert!(markdown.contains("release-risk label"));
        Ok(())
    }

    #[test]
    fn impacted_evidence_invalid_pr_receipt_warns_and_renders_warning() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let repo = temp.path();
        fs::create_dir_all(repo.join("target/ripr/pr"))?;
        fs::write(repo.join(PR_EVIDENCE_JSON), "{not json\n")?;
        let options = ImpactedEvidenceOptions {
            pr_evidence: PR_EVIDENCE_JSON.to_string(),
            labels: vec!["mutation".to_string()],
        };

        let packet = impacted_evidence_packet(repo, &options);
        let markdown = render_impacted_evidence_markdown(&packet);

        assert_eq!(packet.pointer("/status"), Some(&json!("incomplete")));
        assert_eq!(packet.pointer("/summary/mutation_mode"), Some(&json!("targeted")));
        assert_eq!(packet.pointer("/warnings/0/kind"), Some(&json!("invalid_json")));
        assert_eq!(packet.pointer("/artifacts/2/available"), Some(&json!(false)));
        assert!(markdown.contains("invalid_json"), "{markdown}");
        assert!(markdown.contains("PR evidence JSON is invalid"), "{markdown}");
        Ok(())
    }

    #[test]
    fn load_json_distinguishes_present_missing_and_invalid_inputs() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let repo = temp.path();
        let receipt = "target/ripr/pr/custom.json";

        let missing = load_json(repo, receipt);
        assert_eq!(missing.state, InputState::Missing);
        assert_eq!(missing.state.to_string(), "missing");
        assert!(missing.value.is_none());

        fs::create_dir_all(repo.join("target/ripr/pr"))?;
        fs::write(repo.join(receipt), "{not json\n")?;
        let invalid = load_json(repo, receipt);
        let InputState::Invalid(message) = invalid.state else {
            bail!("invalid receipt must report invalid state");
        };
        assert!(message.contains("key must be a string"), "{message}");
        assert!(invalid.value.is_none());
        assert_eq!(
            InputState::Invalid("bad | value\nnext".to_string()).to_string(),
            "invalid: bad \\| value next"
        );

        fs::write(repo.join(receipt), format_json(&json!({ "status": "advisory" }))?)?;
        let present = load_json(repo, receipt);
        assert_eq!(present.state, InputState::Present);
        assert_eq!(present.value, Some(json!({ "status": "advisory" })));
        Ok(())
    }

    #[test]
    fn render_impacted_evidence_markdown_escapes_labels_artifacts_and_warnings() {
        let markdown = render_impacted_evidence_markdown(&json!({
            "summary": {
                "mutation_mode": "fast_only",
                "requires_targeted_mutation": false,
                "requires_full_owner_mutation": false,
                "ripr_severe_gap": false,
                "routing_reason": null
            },
            "inputs": {
                "pr_evidence": "target/ripr/pr/repo|exposure.json",
                "labels": ["needs|ci", "line\nbreak"]
            },
            "artifacts": [
                {
                    "label": "PR|JSON",
                    "path": "target/ripr/pr/repo|exposure.json",
                    "available": true
                },
                {}
            ],
            "warnings": [
                {
                    "kind": "tool|warning",
                    "message": "first | line\nsecond line"
                },
                {}
            ]
        }));

        assert!(markdown.contains("- routing_reason: `none`"), "{markdown}");
        assert!(
            markdown.contains("- PR evidence: `target/ripr/pr/repo\\|exposure.json`"),
            "{markdown}"
        );
        assert!(markdown.contains("- labels: `needs\\|ci, line break`"), "{markdown}");
        assert!(
            markdown.contains("| PR\\|JSON | `target/ripr/pr/repo\\|exposure.json` | true |"),
            "{markdown}"
        );
        assert!(markdown.contains("| artifact | `unknown` | false |"), "{markdown}");
        assert!(markdown.contains("- tool\\|warning: first \\| line second line"), "{markdown}");
        assert!(markdown.contains("- warning: unknown warning"), "{markdown}");
    }

    #[test]
    fn render_pr_evidence_summary_names_missing_and_invalid_inputs() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let repo = temp.path();
        fs::create_dir_all(repo.join("target/ripr/pr"))?;
        fs::create_dir_all(repo.join("target/xtask/impacted-evidence"))?;
        fs::write(repo.join(PR_EVIDENCE_JSON), "{not json\n")?;
        fs::write(
            repo.join(IMPACTED_JSON),
            format_json(&json!({
                "status": "advisory",
                "summary": {
                    "mutation_mode": "fast_only",
                    "requires_targeted_mutation": false,
                    "ripr_severe_gap": false,
                    "routing_reason": null
                }
            }))?,
        )?;

        let summary = render_pr_evidence_summary(repo);

        assert!(summary.contains("- PR evidence JSON: invalid:"), "{summary}");
        assert!(summary.contains("- review guidance JSON: missing"), "{summary}");
        assert!(summary.contains("- impacted evidence JSON: present"), "{summary}");
        assert!(
            summary
                .contains("| PR evidence Markdown | `target/ripr/pr/repo-exposure.md` | missing |")
        );
        assert!(summary.contains("- mutation_mode: `fast_only`"), "{summary}");
        Ok(())
    }

    #[test]
    fn validate_pr_evidence_packet_reports_contract_violations() -> Result<()> {
        let options = PrEvidenceOptions {
            root: ".".to_string(),
            base: "origin/main".to_string(),
            head: "HEAD".to_string(),
            pr_head_sha: None,
        };
        let packet = json!({
            "schema_version": "0.1",
            "tool": "ripr",
            "kind": "pr_evidence",
            "scope": "diff",
            "status": "surprising",
            "root": ".",
            "base": "origin/main",
            "base_sha": "base-sha",
            "head": "HEAD",
            "head_sha": "head-sha",
            "summary": {
                "changed_files": 2,
                "comments": "zero",
                "summary_only": 0,
                "suppressed": 0,
                "weakly_exposed": 0,
                "reachable_unrevealed": 0,
                "no_static_path": 0,
                "severe_gaps": 0,
                "requires_targeted_mutation": "false",
                "ripr_severe_gap": false,
                "routing_reason": 7
            },
            "warnings": {},
            "advisory_limits": [],
            "artifacts": []
        });

        let err = match validate_pr_evidence_packet(
            &packet, &options, 1, false, "base-sha", "head-sha",
        ) {
            Ok(_) => bail!("invalid PR evidence packet must fail contract validation"),
            Err(err) => err,
        };
        let message = err.to_string();

        assert!(message.contains("status"));
        assert!(message.contains("summary.comments"));
        assert!(message.contains("summary.changed_files"));
        assert!(message.contains("summary.requires_targeted_mutation"));
        assert!(message.contains("summary.routing_reason"));
        assert!(message.contains("warnings"));
        assert!(message.contains("advisory_limits"));
        assert!(message.contains(PR_EVIDENCE_JSON));
        assert!(message.contains(PR_EVIDENCE_MD));
        Ok(())
    }

    #[test]
    fn validate_review_comments_reports_contract_violations() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let repo = temp.path();
        init_git_repo(repo)?;
        fs::create_dir_all(repo.join("target/ripr/review"))?;
        fs::write(
            repo.join(REVIEW_COMMENTS_JSON),
            format_json(&json!({
                "schema_version": "0.1",
                "tool": "ripr",
                "status": "surprising",
                "base": "HEAD",
                "base_sha": revision_sha(repo, "HEAD")?,
                "head": "HEAD",
                "head_sha": revision_sha(repo, "HEAD")?,
                "summary": [],
                "comments": [],
                "summary_only": "none",
                "suppressed": [],
                "warnings": []
            }))?,
        )?;
        let options = ReviewCommentsOptions {
            root: ".".to_string(),
            base: "HEAD".to_string(),
            head: "HEAD".to_string(),
            pr_head_sha: None,
            timeout_seconds: None,
        };

        let err = match validate_review_comments(repo, &options, true) {
            Ok(_) => bail!("invalid review comments packet must fail contract validation"),
            Err(err) => err,
        };
        let message = err.to_string();

        assert!(message.contains("status"));
        assert!(message.contains("summary_only"));
        assert!(message.contains("summary is missing or not an object"));
        assert!(message.contains(REVIEW_COMMENTS_MD));
        Ok(())
    }

    fn init_git_repo(repo: &Path) -> Result<()> {
        fs::write(repo.join("tracked.txt"), "base\n")?;
        run_git(repo, &["init"])?;
        run_git(repo, &["add", "tracked.txt"])?;
        run_git(
            repo,
            &["-c", "user.name=test", "-c", "user.email=test@example.com", "commit", "-m", "base"],
        )?;
        Ok(())
    }

    fn run_git(repo: &Path, args: &[&str]) -> Result<String> {
        let output = Command::new("git").args(args).current_dir(repo).output()?;
        if !output.status.success() {
            bail!("git {:?} failed with status {}", args, output.status);
        }
        Ok(String::from_utf8(output.stdout)
            .context("git command returned non-UTF8 output")?
            .trim()
            .to_string())
    }

    struct RiprBinOverrideGuard;

    impl Drop for RiprBinOverrideGuard {
        fn drop(&mut self) {
            if let Ok(mut guard) = RIPR_BIN_OVERRIDE.lock() {
                *guard = None;
            }
        }
    }

    fn override_ripr_bin(binary: &Path) -> Result<RiprBinOverrideGuard> {
        let mut guard =
            RIPR_BIN_OVERRIDE.lock().map_err(|_| eyre!("RIPR_BIN test override lock poisoned"))?;
        *guard = Some(binary.display().to_string());
        Ok(RiprBinOverrideGuard)
    }

    fn write_fake_ripr_binary(dir: &Path) -> Result<PathBuf> {
        let badge_json = r#"{"basis":"canonical_actionable_gap","counts":{"unsuppressed_exposure_gaps":7,"unsuppressed_test_efficiency_findings":2,"suppressed_exposure_gaps":1,"suppressed_test_efficiency_findings":3},"reason_counts":{"no_assertion_detected":7}}"#;
        let seams_json = r#"{"seams":[{"file":"xtask/src/tasks/ripr_evidence.rs","gap_kind":"receipt parsing"},{"file":"archive/old.rs","gap_kind":"archived"}]}"#;

        #[cfg(windows)]
        {
            let path = dir.join("ripr.cmd");
            write_text(
                &path,
                &format!(
                    r#"@echo off
echo %* | %SystemRoot%\System32\findstr.exe /C:"repo-badge-json" >NUL
if %ERRORLEVEL%==0 (
  echo {badge_json}
  exit /b 0
)
echo %* | %SystemRoot%\System32\findstr.exe /C:"repo-seams-json" >NUL
if %ERRORLEVEL%==0 (
  echo {seams_json}
  exit /b 0
)
echo unexpected ripr args: %* 1>&2
exit /b 2
"#
                ),
            )?;
            Ok(path)
        }

        #[cfg(not(windows))]
        {
            let path = dir.join("ripr");
            write_text(
                &path,
                &format!(
                    r#"#!/bin/sh
case "$*" in
  *repo-badge-json*)
    printf '%s\n' '{badge_json}'
    ;;
  *repo-seams-json*)
    printf '%s\n' '{seams_json}'
    ;;
  *)
    echo "unexpected ripr args: $*" >&2
    exit 2
    ;;
esac
"#
                ),
            )?;
            let mut permissions = fs::metadata(&path)?.permissions();
            use std::os::unix::fs::PermissionsExt;
            permissions.set_mode(0o755);
            fs::set_permissions(&path, permissions)?;
            Ok(path)
        }
    }

    // ---------------------------------------------------------------------------
    // run_output file transport tests (#12569)
    // ---------------------------------------------------------------------------

    /// Create a platform-specific helper that writes exactly `byte_count` ASCII `x` bytes
    /// to stdout in one large write.
    fn write_large_output_script(dir: &Path, byte_count: usize) -> Result<PathBuf> {
        let source = dir.join("large_output.rs");
        fs::write(
            &source,
            format!(
                "use std::io::Write;\n\
                 fn main() {{\n\
                     let payload = vec![b'x'; {byte_count}];\n\
                     if let Err(error) = std::io::stdout().write_all(&payload) {{\n\
                         eprintln!(\"{{error}}\");\n\
                         std::process::exit(1);\n\
                     }}\n\
                 }}\n"
            ),
        )?;
        #[cfg(windows)]
        let binary = dir.join("large_output.exe");
        #[cfg(not(windows))]
        let binary = dir.join("large_output");
        let output = Command::new("rustc")
            .arg(&source)
            .arg("-o")
            .arg(&binary)
            .output()
            .context("failed to compile large-output test helper")?;
        if !output.status.success() {
            bail!(
                "failed to compile large-output test helper:\n{}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(binary)
    }

    #[test]
    fn run_output_captures_small_stdout_and_propagates_exit_failure() -> Result<()> {
        // Basic smoke-test for the file-backed run_output implementation:
        // success path returns stdout; failure path returns an error containing stderr.
        #[cfg(not(windows))]
        {
            let tmp = tempfile::tempdir()?;

            // Success: printf a known value.
            let ok = tmp.path().join("ok.sh");
            fs::write(&ok, "#!/bin/sh\nprintf 'hello world'\n")?;
            use std::os::unix::fs::PermissionsExt;
            {
                let mut p = fs::metadata(&ok)?.permissions();
                p.set_mode(0o755);
                fs::set_permissions(&ok, p)?;
            }
            let out = run_output(&ok.display().to_string(), &[])?;
            assert_eq!(out, "hello world", "stdout must be captured verbatim");

            // Failure: non-zero exit must surface stderr in the error.
            let fail = tmp.path().join("fail.sh");
            fs::write(&fail, "#!/bin/sh\nprintf 'detailed error' >&2\nexit 2\n")?;
            {
                let mut p = fs::metadata(&fail)?.permissions();
                p.set_mode(0o755);
                fs::set_permissions(&fail, p)?;
            }
            let err = run_output(&fail.display().to_string(), &[]).unwrap_err();
            let msg = format!("{err:#}");
            assert!(msg.contains("detailed error"), "stderr must appear in error: {msg}");
            assert!(msg.contains("status"), "exit status must appear in error: {msg}");
        }
        Ok(())
    }

    #[test]
    fn run_output_reads_large_stdout_from_file() -> Result<()> {
        // Regression guard for the Windows "os error 87" panic (#12569).  The child
        // performs one multi-megabyte stdout write; run_output must give it a regular
        // file rather than a pipe.
        const TARGET_MB: usize = 5;
        const TARGET_BYTES: usize = TARGET_MB * 1024 * 1024;

        let tmp = tempfile::tempdir()?;
        let script = write_large_output_script(tmp.path(), TARGET_BYTES)?;

        let result = run_output(&script.display().to_string(), &[])?;

        assert_eq!(
            result.len(),
            TARGET_BYTES,
            "Expected exactly {TARGET_BYTES} bytes, captured {}",
            result.len()
        );
        assert!(
            result.bytes().all(|b| b == b'x'),
            "Output must consist entirely of 'x' bytes — got unexpected content"
        );
        Ok(())
    }

    #[test]
    fn run_git_reports_failure_status() -> Result<()> {
        let temp = tempfile::tempdir()?;

        assert!(run_git(temp.path(), &["definitely-not-a-git-command"]).is_err());
        Ok(())
    }

    /// One ancestry receipt carrying a chosen disposition, for projecting the
    /// RIPR guidance vocabulary without a repository.
    fn ancestry_receipt(disposition: AncestryDisposition, reason: &str) -> AncestryReceipt {
        AncestryReceipt {
            schema_version: "git_ancestry.v1".to_string(),
            repository: ".".to_string(),
            repository_root: None,
            git_dir: None,
            git_common_dir: None,
            base: "origin/main".to_string(),
            head: "HEAD".to_string(),
            base_sha: None,
            head_sha: None,
            merge_base: None,
            is_shallow_repository: None,
            is_partial_clone: None,
            base_object_exists: false,
            head_object_exists: false,
            base_is_ancestor_of_head: None,
            head_is_ancestor_of_base: None,
            disposition,
            reason: reason.to_string(),
            guidance: Vec::new(),
            limitations: Vec::new(),
        }
    }

    /// The false-history claim #10051/#10205 came from: a shallow checkout is
    /// missing history, not proof that two refs are unrelated.
    #[test]
    fn shallow_guidance_reports_not_proven_instead_of_absent_history() {
        let receipt = ancestry_receipt(
            AncestryDisposition::NotProvenShallow,
            "the checkout is shallow; local absence is not proof of unrelated history",
        );
        let message = ancestry_failure_guidance("origin/main", "HEAD", &receipt);

        assert!(message.contains("origin/main...HEAD"), "range echoed: {message}");
        assert!(message.contains("not_proven_shallow"), "typed disposition: {message}");
        assert!(message.contains("git fetch --unshallow"), "remedy preserved: {message}");
        assert!(message.contains("fetch-depth: 0"), "CI note preserved: {message}");
        assert!(
            !message.contains("share no common history"),
            "shallow must never assert absent history: {message}"
        );
        assert!(
            !message.contains("unrelated`"),
            "shallow must not report the unrelated disposition: {message}"
        );
    }

    /// A promisor/partial checkout is the same class of incomplete evidence.
    #[test]
    fn partial_clone_guidance_reports_not_proven_instead_of_absent_history() {
        let receipt = ancestry_receipt(
            AncestryDisposition::NotProvenPartialClone,
            "the checkout is partial; local absence is not proof of unrelated history",
        );
        let message = ancestry_failure_guidance("origin/main", "HEAD", &receipt);

        assert!(message.contains("not_proven_partial_clone"), "typed disposition: {message}");
        // Assert the arm's own remedy, not just the header-emitted label: the
        // label comes from the shared prefix, so without this a no-op or
        // swapped arm body would still pass.
        assert!(message.contains("git fetch --refetch origin`"), "partial remedy: {message}");
        assert!(
            message.contains("Materialize the required commit graph"),
            "partial cause: {message}"
        );
        assert!(!message.contains("--unshallow"), "must not blame shallow: {message}");
        assert!(
            !message.contains("share no common history"),
            "partial clone must never assert absent history: {message}"
        );
    }

    /// An unresolvable revision is a bad ref or missing object, not a history claim.
    #[test]
    fn missing_object_guidance_does_not_blame_history() {
        let receipt = ancestry_receipt(
            AncestryDisposition::NotProvenMissingObject,
            "the base commit object is unavailable",
        );
        let message = ancestry_failure_guidance("origin/main", "HEAD", &receipt);

        assert!(message.contains("not_proven_missing_object"), "typed disposition: {message}");
        assert!(message.contains("git fetch origin`"), "fetch remedy: {message}");
        assert!(
            !message.contains("share no common history"),
            "missing object must never assert absent history: {message}"
        );
        assert!(!message.contains("shallow"), "must not blame shallow: {message}");
    }

    /// Every fetch remedy must be a command the operator can actually run.
    ///
    /// RIPR's default base is the remote-tracking name `origin/main`, and
    /// `git fetch origin origin/main` fails with `couldn't find remote ref
    /// origin/main` because the operand resolves in the remote namespace. No
    /// disposition may interpolate the base after a remote name.
    #[test]
    fn guidance_never_emits_a_remote_prefixed_fetch_refspec() {
        for disposition in [
            AncestryDisposition::Ancestor,
            AncestryDisposition::Diverged,
            AncestryDisposition::Unrelated,
            AncestryDisposition::NotProvenShallow,
            AncestryDisposition::NotProvenPartialClone,
            AncestryDisposition::NotProvenMissingObject,
            AncestryDisposition::InvalidInput,
            AncestryDisposition::InstrumentFailure,
        ] {
            let receipt = ancestry_receipt(disposition, "reason");
            let message = ancestry_failure_guidance(DEFAULT_BASE, DEFAULT_HEAD, &receipt);

            assert!(
                !message.contains(&format!("origin {DEFAULT_BASE}")),
                "remedy must not resolve `{DEFAULT_BASE}` in the remote namespace: {message}"
            );
        }
    }

    /// `unrelated` is the one disposition permitted to state absent history,
    /// and the classifier reaches it only from a complete-enough local graph.
    #[test]
    fn only_proven_unrelated_guidance_states_absent_history() {
        let receipt = ancestry_receipt(
            AncestryDisposition::Unrelated,
            "both commit objects are present in a non-shallow, non-partial graph and no merge base exists",
        );
        let message = ancestry_failure_guidance("origin/main", "HEAD", &receipt);

        assert!(message.contains("unrelated"), "typed disposition: {message}");
        assert!(
            message.contains("share no common history"),
            "proven unrelated may state absent history: {message}"
        );
        assert!(!message.contains("--unshallow"), "must not blame shallow: {message}");
    }

    /// Related refs mean the diff failure has some other cause; the guidance
    /// must not misattribute it to ancestry.
    #[test]
    fn related_history_guidance_does_not_blame_ancestry() {
        for disposition in [AncestryDisposition::Ancestor, AncestryDisposition::Diverged] {
            let receipt = ancestry_receipt(disposition, "the requested refs are related");
            let message = ancestry_failure_guidance("origin/main", "HEAD", &receipt);

            assert!(message.contains("are related in this checkout"), "related: {message}");
            assert!(
                !message.contains("share no common history"),
                "related refs must never assert absent history: {message}"
            );
        }
    }

    /// Invalid input and instrument failure stay distinct from every history claim.
    #[test]
    fn invalid_and_instrument_guidance_make_no_history_claim() {
        // Each row pins the arm's own remedy as well as the label. The label is
        // emitted by the shared header, so asserting it alone cannot tell a
        // correct arm from an empty or swapped one.
        for (disposition, expected, remedy) in [
            (
                AncestryDisposition::InvalidInput,
                "invalid_input",
                "Check the base and head revision values",
            ),
            (
                AncestryDisposition::InstrumentFailure,
                "instrument_failure",
                "Git could not be inspected",
            ),
        ] {
            let receipt = ancestry_receipt(disposition, "no domain result was reached");
            let message = ancestry_failure_guidance("origin/main", "HEAD", &receipt);

            assert!(message.contains(expected), "typed disposition: {message}");
            assert!(message.contains(remedy), "{expected} remedy: {message}");
            assert!(
                !message.contains("share no common history"),
                "{expected} must never assert absent history: {message}"
            );
        }
    }

    /// The production seam, not just the formatter: a real shallow checkout must
    /// reach the shared authority and report `not_proven_shallow`. Re-mapping a
    /// failed merge base back to "unrelated" fails here.
    #[test]
    fn shallow_repository_production_guidance_uses_the_shared_authority() -> Result<()> {
        let origin = tempfile::tempdir()?;
        ancestry_git(origin.path(), &["init", "--quiet", "--initial-branch=main", "."])?;
        ancestry_git(origin.path(), &["config", "user.email", "ripr@example.invalid"])?;
        ancestry_git(origin.path(), &["config", "user.name", "RIPR Test"])?;
        for index in 0..3 {
            fs::write(origin.path().join("file.txt"), format!("revision {index}\n"))?;
            ancestry_git(origin.path(), &["add", "file.txt"])?;
            ancestry_git(origin.path(), &["commit", "--quiet", "-m", &format!("c{index}")])?;
        }

        let shallow = tempfile::tempdir()?;
        // `--depth` is ignored for a plain local path, so the fixture needs a
        // `file://` URL to actually become shallow.
        let source = ancestry_file_url(origin.path());
        let target = shallow.path().join("clone");
        ancestry_git(
            shallow.path(),
            &["clone", "--quiet", "--depth", "1", &source, &target.display().to_string()],
        )?;
        assert_eq!(
            ancestry_git(&target, &["rev-parse", "--is-shallow-repository"])?.trim(),
            "true",
            "fixture must actually be shallow"
        );

        let message = merge_base_failure_guidance(&target, "main~2", "HEAD");
        assert!(message.contains("not_proven_shallow"), "shared disposition: {message}");
        assert!(
            !message.contains("share no common history"),
            "shallow production path must never assert absent history: {message}"
        );
        Ok(())
    }

    /// A complete clone with genuinely unrelated orphan histories is the only
    /// production case allowed to report absent history.
    #[test]
    fn complete_unrelated_history_production_guidance_states_absent_history() -> Result<()> {
        let repo = tempfile::tempdir()?;
        ancestry_git(repo.path(), &["init", "--quiet", "--initial-branch=main", "."])?;
        ancestry_git(repo.path(), &["config", "user.email", "ripr@example.invalid"])?;
        ancestry_git(repo.path(), &["config", "user.name", "RIPR Test"])?;
        fs::write(repo.path().join("main.txt"), "main\n")?;
        ancestry_git(repo.path(), &["add", "main.txt"])?;
        ancestry_git(repo.path(), &["commit", "--quiet", "-m", "main"])?;

        ancestry_git(repo.path(), &["checkout", "--quiet", "--orphan", "other"])?;
        ancestry_git(repo.path(), &["rm", "-rf", "--quiet", "."])?;
        fs::write(repo.path().join("other.txt"), "other\n")?;
        ancestry_git(repo.path(), &["add", "other.txt"])?;
        ancestry_git(repo.path(), &["commit", "--quiet", "-m", "other"])?;

        let message = merge_base_failure_guidance(repo.path(), "main", "other");
        assert!(message.contains("unrelated"), "proven unrelated: {message}");
        assert!(
            message.contains("share no common history"),
            "complete graph may state absent history: {message}"
        );
        Ok(())
    }

    /// A bogus revision in a complete clone is a missing object, never unrelated
    /// history — the conflation the retired private helper encoded.
    #[test]
    fn missing_object_production_guidance_is_not_unrelated() -> Result<()> {
        let repo = tempfile::tempdir()?;
        ancestry_git(repo.path(), &["init", "--quiet", "--initial-branch=main", "."])?;
        ancestry_git(repo.path(), &["config", "user.email", "ripr@example.invalid"])?;
        ancestry_git(repo.path(), &["config", "user.name", "RIPR Test"])?;
        fs::write(repo.path().join("main.txt"), "main\n")?;
        ancestry_git(repo.path(), &["add", "main.txt"])?;
        ancestry_git(repo.path(), &["commit", "--quiet", "-m", "main"])?;

        let message = merge_base_failure_guidance(repo.path(), "ripr-no-such-base-xyz", "HEAD");
        assert!(message.contains("not_proven_missing_object"), "typed disposition: {message}");
        // Only the base is unresolvable here, so the remedy must name the base
        // rather than blaming both sides.
        assert!(
            message.contains("`ripr-no-such-base-xyz` could not be resolved locally"),
            "names the missing side: {message}"
        );
        assert!(
            !message.contains("`ripr-no-such-base-xyz` and `HEAD` could not be resolved"),
            "must not blame the resolvable head: {message}"
        );
        assert!(
            !message.contains("share no common history"),
            "a bad ref must never assert absent history: {message}"
        );
        Ok(())
    }

    /// A `file://` URL for a local path, so `git clone --depth` is honoured on
    /// both POSIX and Windows (`C:\a` must become `file:///C:/a`, not `file://C:\a`).
    fn ancestry_file_url(path: &Path) -> String {
        let text = path.display().to_string().replace('\\', "/");
        if text.starts_with('/') { format!("file://{text}") } else { format!("file:///{text}") }
    }

    /// Runs git for the ancestry fixtures with the ambient settings that would
    /// otherwise break a temporary repository neutralized. `GIT_CONFIG_GLOBAL`
    /// is deliberately not pointed at `/dev/null`: that path does not exist on
    /// Windows, and this crate's tests are in the Windows CI crate scope.
    fn ancestry_git(repository: &Path, arguments: &[&str]) -> Result<String> {
        let output = Command::new("git")
            .args([
                "-c",
                "commit.gpgsign=false",
                "-c",
                "core.autocrlf=false",
                "-c",
                "protocol.file.allow=always",
            ])
            .args(arguments)
            .current_dir(repository)
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .with_context(|| format!("running git {arguments:?}"))?;
        if !output.status.success() {
            bail!(
                "git {arguments:?} failed: {}",
                String::from_utf8_lossy(&output.stderr).trim().to_string()
            );
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    #[test]
    fn changed_files_reports_missing_merge_base_with_guidance() -> Result<()> {
        // The workspace root is a real git repo; a bogus base has no merge base
        // with HEAD, so changed_files must bail with the actionable guidance
        // instead of propagating a raw git failure.
        let repo = repo_root()?;
        match changed_files(&repo, "ripr-no-such-base-xyz", "HEAD") {
            Ok(files) => Err(eyre!("expected unresolvable-range error, got {files:?}")),
            Err(err) => {
                let message = format!("{err:#}");
                assert!(message.contains("cannot resolve diff range"), "guidance: {message}");
                // The workspace checkout may be shallow or complete; either way a
                // bogus base is never proof that the two refs are unrelated.
                assert!(
                    !message.contains("share no common history"),
                    "a bad ref must never assert absent history: {message}"
                );
                Ok(())
            }
        }
    }

    #[test]
    fn changed_files_succeeds_for_valid_range() -> Result<()> {
        // The workspace root is a real git repo; `HEAD...HEAD` is a valid range
        // with an empty symmetric diff, exercising the success path.
        let repo = repo_root()?;
        let files = changed_files(&repo, "HEAD", "HEAD")?;
        assert!(files.is_empty(), "HEAD...HEAD has no changed files: {files:?}");
        Ok(())
    }

    #[test]
    fn name_status_parser_preserves_acdmrt_entries() -> Result<()> {
        let raw = b"A\0added.rs\0C75\0source.rs\0copy.rs\0D\0deleted.rs\0M\0modified.rs\0R100\0old.rs\0renamed.rs\0T\0typed.rs\0";
        let entries = parse_name_status_z(raw)?;
        assert_eq!(entries.len(), 6);
        assert_eq!(entries[0].status, "A");
        assert_eq!(entries[0].new_path.as_deref(), Some("added.rs"));
        assert_eq!(entries[1].status, "C75");
        assert_eq!(entries[1].old_path.as_deref(), Some("source.rs"));
        assert_eq!(entries[1].new_path.as_deref(), Some("copy.rs"));
        assert_eq!(entries[2].old_path.as_deref(), Some("deleted.rs"));
        assert_eq!(entries[3].new_path.as_deref(), Some("modified.rs"));
        assert_eq!(entries[3].old_path.as_deref(), Some("modified.rs"));
        assert_eq!(entries[4].old_path.as_deref(), Some("old.rs"));
        assert_eq!(entries[4].new_path.as_deref(), Some("renamed.rs"));
        assert_eq!(entries[5].status, "T");
        assert_eq!(entries[5].old_path.as_deref(), Some("typed.rs"));
        assert_eq!(entries[5].new_path.as_deref(), Some("typed.rs"));
        Ok(())
    }

    #[test]
    fn split_labels_splits_on_comma_semicolon_and_newline_with_trim() {
        let labels = split_labels(" mutation , needs-ci-fix ;size/M\nsize/L ");
        assert_eq!(labels, vec!["mutation", "needs-ci-fix", "size/M", "size/L"]);
    }

    #[test]
    fn split_labels_drops_empty_and_whitespace_only_segments() {
        // Trailing/leading separators and blank segments must not yield empty
        // labels, otherwise downstream routing would match a "" label.
        let labels = split_labels(",, mutation ;; \n ; ,");
        assert_eq!(labels, vec!["mutation"]);
        assert!(split_labels("   ").is_empty());
        assert!(split_labels("").is_empty());
    }

    #[test]
    fn normalize_labels_lowercases_dedupes_and_sorts() {
        let input = vec!["Mutation".to_string(), "mutation".to_string(), "  CI  ".to_string()];
        let normalized = normalize_labels(&input);
        // case-folded, de-duplicated across cases, trimmed, and sorted.
        assert_eq!(normalized, vec!["ci".to_string(), "mutation".to_string()]);
    }

    #[test]
    fn normalize_labels_filters_blank_after_trim() {
        let input = vec!["   ".to_string(), "\t".to_string(), "keep".to_string()];
        assert_eq!(normalize_labels(&input), vec!["keep".to_string()]);
    }

    #[test]
    fn merged_labels_unions_explicit_and_csv_then_normalizes() {
        // Explicit labels are non-empty, so the env fallback path is not taken.
        // Duplicates across the two sources collapse; output is folded and sorted.
        let merged = merged_labels(&["Zeta".to_string()], Some("alpha, Zeta; BETA"));
        assert_eq!(merged, vec!["alpha".to_string(), "beta".to_string(), "zeta".to_string()]);
    }

    #[test]
    fn merged_labels_accepts_csv_only_without_explicit_labels() {
        let merged = merged_labels(&[], Some("needs-ci-fix,needs-ci-fix"));
        assert_eq!(merged, vec!["needs-ci-fix".to_string()]);
    }

    #[test]
    fn ripr_unrecognized_classification_with_suppressed_path_is_suppressed() -> Result<()> {
        // Regression test for #1346: a finding whose classification is NOT in the known
        // canonical match arms (e.g. "static_unknown", "infection_unknown", "exposed", or
        // any future ripr value) must still be suppressed when its path matches a policy
        // suppression glob.  Before the fix the code did `continue` on unrecognized
        // classification before reaching path-matching, so suppressed_by_policy stayed 0
        // and severe_gaps remained positive — a false-positive gate failure.
        let options = PrEvidenceOptions {
            root: ".".to_string(),
            base: "origin/main".to_string(),
            head: "HEAD".to_string(),
            pr_head_sha: None,
        };
        // Simulate ripr output with an unrecognized classification that is counted in the
        // summary but whose path is covered by our suppression policy.
        let check_value = json!({
            "summary": {
                "weakly_exposed": 0,
                "reachable_unrevealed": 2,
                "no_static_path": 0
            },
            "findings": [
                {
                    // Unrecognized classification — not in any canonical match arm.
                    "classification": "static_unknown",
                    "kind": "call_presence",
                    "seam": {
                        "file": "crates/perl-dap/src/debug_adapter/variables.rs",
                        "line": 584
                    }
                },
                {
                    // Also unrecognized, path matches suppression.
                    "classification": "infection_unknown",
                    "kind": "call_presence",
                    "seam": {
                        "file": "crates/perl-dap/src/debug_adapter/variables.rs",
                        "line": 591
                    }
                }
            ]
        });
        // Suppression covers the DAP variables.rs file.
        let suppressions = RiprSuppressionRules {
            display_patterns: vec!["crates/perl-dap/src/debug_adapter/variables.rs".to_string()],
            path_patterns: vec![Pattern::new("crates/perl-dap/src/debug_adapter/variables.rs")?],
            classification_patterns: vec![Vec::new()],
            invalid_patterns: Vec::new(),
            suppression_reasons: Vec::new(),
        };

        let packet = pr_evidence_packet(
            &options,
            &["crates/perl-dap/src/debug_adapter/variables.rs".to_string()],
            &check_value,
            "base-sha",
            "head-sha",
            &suppressions,
        );

        // Both findings are on a suppressed path → suppressed_by_policy=2, severe_gaps=0.
        assert_eq!(
            packet.pointer("/summary/suppressed_by_policy"),
            Some(&json!(2)),
            "unrecognized-classification findings on suppressed path must be counted as suppressed"
        );
        assert_eq!(
            packet.pointer("/summary/severe_gaps"),
            Some(&json!(0)),
            "severe_gaps must be 0 after suppressing all findings (even unrecognized classifications)"
        );
        // Note: unclassified suppressions cannot be attributed to a specific bucket —
        // reachable_unrevealed retains the raw summary value, but severe_gaps (the gate
        // criterion) is correctly decremented by suppressed_unclassified.
        assert_eq!(
            packet.pointer("/summary/ripr_severe_gap"),
            Some(&json!(false)),
            "ripr_severe_gap must be false when all findings are suppressed"
        );
        Ok(())
    }

    #[test]
    fn ripr_unrecognized_classification_without_suppressed_path_produces_severe_gaps() -> Result<()>
    {
        // Gate teeth: an unrecognized classification on a path NOT in suppressions
        // must still produce severe_gaps > 0.  This guards against a fix that
        // accidentally over-suppresses findings with unknown classifications.
        let options = PrEvidenceOptions {
            root: ".".to_string(),
            base: "origin/main".to_string(),
            head: "HEAD".to_string(),
            pr_head_sha: None,
        };
        let check_value = json!({
            "summary": {
                "weakly_exposed": 0,
                "reachable_unrevealed": 1,
                "no_static_path": 0
            },
            "findings": [
                {
                    "classification": "static_unknown",
                    "kind": "call_presence",
                    "seam": {
                        "file": "crates/perl-lsp-rs/src/some_new_file.rs",
                        "line": 10
                    }
                }
            ]
        });
        // Suppression only covers DAP variables.rs — NOT the LSP file.
        let suppressions = RiprSuppressionRules {
            display_patterns: vec!["crates/perl-dap/src/debug_adapter/variables.rs".to_string()],
            path_patterns: vec![Pattern::new("crates/perl-dap/src/debug_adapter/variables.rs")?],
            classification_patterns: vec![Vec::new()],
            invalid_patterns: Vec::new(),
            suppression_reasons: Vec::new(),
        };

        let packet = pr_evidence_packet(
            &options,
            &["crates/perl-lsp-rs/src/some_new_file.rs".to_string()],
            &check_value,
            "base-sha",
            "head-sha",
            &suppressions,
        );

        // Unsuppressed finding → severe_gaps > 0, gate must reject.
        assert_eq!(packet.pointer("/summary/suppressed_by_policy"), Some(&json!(0)));
        assert_eq!(
            packet.pointer("/summary/severe_gaps"),
            Some(&json!(1)),
            "unsuppressed unrecognized-classification finding must produce severe_gaps > 0"
        );
        assert_eq!(packet.pointer("/summary/ripr_severe_gap"), Some(&json!(true)));
        Ok(())
    }

    #[test]
    fn ripr_no_summary_mixed_recognized_gap_and_suppressed_unclassified_does_not_over_subtract()
    -> Result<()> {
        // Regression test for Path B (no summary object) over-subtract risk:
        // if a findings-only payload has both a real recognized unsuppressed gap AND
        // unclassified suppressed findings, suppressed_unclassified must NOT be subtracted
        // from the bucket totals (they were never added to them) — doing so would mask a
        // real gap via saturating_sub.
        let options = PrEvidenceOptions {
            root: ".".to_string(),
            base: "origin/main".to_string(),
            head: "HEAD".to_string(),
            pr_head_sha: None,
        };
        // No summary object — triggers Path B (findings-only mode).
        let check_value = json!({
            "findings": [
                {
                    // Real recognized gap — not in any suppression.
                    "classification": "reachable_unrevealed",
                    "kind": "call_presence",
                    "seam": {
                        "file": "crates/perl-lsp-rs/src/real_gap.rs",
                        "line": 10
                    }
                },
                {
                    // Unclassified but path-suppressed — must NOT subtract from real gap.
                    "classification": "static_unknown",
                    "kind": "call_presence",
                    "seam": {
                        "file": "crates/perl-dap/src/debug_adapter/variables.rs",
                        "line": 584
                    }
                },
                {
                    // Another unclassified suppressed — still must not cancel the real gap.
                    "classification": "infection_unknown",
                    "kind": "call_presence",
                    "seam": {
                        "file": "crates/perl-dap/src/debug_adapter/variables.rs",
                        "line": 591
                    }
                }
            ]
        });
        // Suppression covers DAP variables.rs only — not the LSP real_gap.rs.
        let suppressions = RiprSuppressionRules {
            display_patterns: vec!["crates/perl-dap/src/debug_adapter/variables.rs".to_string()],
            path_patterns: vec![Pattern::new("crates/perl-dap/src/debug_adapter/variables.rs")?],
            classification_patterns: vec![Vec::new()],
            invalid_patterns: Vec::new(),
            suppression_reasons: Vec::new(),
        };

        let packet = pr_evidence_packet(
            &options,
            &["crates/perl-lsp-rs/src/real_gap.rs".to_string()],
            &check_value,
            "base-sha",
            "head-sha",
            &suppressions,
        );

        // 2 unclassified are suppressed, 1 recognized is not — gate must still fire.
        assert_eq!(
            packet.pointer("/summary/suppressed_by_policy"),
            Some(&json!(2)),
            "two unclassified findings on suppressed path must be counted as suppressed"
        );
        assert_eq!(
            packet.pointer("/summary/severe_gaps"),
            Some(&json!(1)),
            "real recognized gap must not be cancelled by unclassified suppressed findings"
        );
        assert_eq!(
            packet.pointer("/summary/ripr_severe_gap"),
            Some(&json!(true)),
            "gate must fire: 1 real gap remains even though 2 unclassified are suppressed"
        );
        Ok(())
    }

    // ---------------------------------------------------------------------------
    // Streaming ingestion compatibility (#12860)
    //
    // `ripr check --format json` output is unbounded — a 2.1GB payload killed a
    // 16GB CI runner when write_pr_evidence buffered it into one String and then
    // parsed a full serde_json DOM. The ingestion now streams one finding at a
    // time into RiprFindingBuckets. These tests pin the new path to the retained
    // DOM oracle byte for byte, on realistic and degenerate payloads alike.
    // ---------------------------------------------------------------------------

    /// #12860 compatibility falsifier: for the same raw payload bytes, the
    /// streaming ingestion must produce receipt bytes identical to the DOM
    /// oracle (`serde_json::from_str` + `pr_evidence_packet_with_count`).
    fn assert_streaming_receipt_matches_dom(
        payload: &str,
        extents: Option<&HeadLineExtents>,
        suppressions: &RiprSuppressionRules,
    ) -> Result<()> {
        let options = PrEvidenceOptions {
            root: ".".to_string(),
            base: "origin/main".to_string(),
            head: "HEAD".to_string(),
            pr_head_sha: None,
        };
        let dom_packet = pr_evidence_packet_with_count(
            &options,
            &serde_json::from_str(payload).context("parity payload must be valid JSON")?,
            "base-sha",
            "head-sha",
            suppressions,
            extents,
            PrEvidenceContext {
                changed_file_count: 1,
                attribution_scope: None,
                production_surface: None,
            },
        );
        let temp = tempfile::tempdir()?;
        let raw_path = temp.path().join("raw-check.json");
        fs::write(&raw_path, payload)?;
        let ingestion =
            ripr_check_ingestion_from_file(&raw_path, suppressions, extents, None, None)?;
        let streamed_packet = pr_evidence_packet_from_summary(
            &options,
            &ingestion,
            "base-sha",
            "head-sha",
            suppressions,
            PrEvidenceContext {
                changed_file_count: 1,
                attribution_scope: None,
                production_surface: None,
            },
        );
        assert_eq!(
            format_json(&dom_packet)?,
            format_json(&streamed_packet)?,
            "streamed receipt bytes must match DOM receipt bytes"
        );
        Ok(())
    }

    #[test]
    fn streaming_ingestion_receipt_matches_dom_bytes() -> Result<()> {
        let payloads = [
            // Realistic 0.5.x shape: classification + probe paths, plus large
            // unconsumed per-finding structures like the 2.1GB payload carried.
            concat!(
                r#"{"schema_version":"0.2","tool":"ripr","summary":{"changed_rust_files":2,"findings":3,"weakly_exposed":1,"reachable_unrevealed":1,"no_static_path":1},"findings":["#,
                r#"{"id":"p1","classification":"weakly_exposed","severity":"info","confidence":1.0,"probe":{"id":"p1","family":"call_deletion","file":".\\xtask/src/a.rs","line":11,"expression":"type TestResult = anyhow::Result<()>;"},"ripr":{"reach":{"state":"yes","summary":"reaches"},"observations":[{"line":271,"value":"01","context":"assertion_argument"}]}},"#,
                r#"{"classification":"reachable_unrevealed","seam":{"file":"crates/perl-lsp-rs/src/b.rs","line":4}},"#,
                r#"{"classification":"no_static_path","probe":{"path":"xtask/src/c.rs","line":9},"notes":"unicode ✓ escaped \"quotes\""}]}"#
            ),
            // 0.9.x grip_class variant, mapped onto the canonical buckets.
            concat!(
                r#"{"summary":{"weakly_exposed":0,"reachable_unrevealed":2,"no_static_path":0},"findings":["#,
                r#"{"grip_class":"weakly_gripped","seam":{"file":"crates/perl-lsp-rs/src/removed.rs","line":4}},"#,
                r#"{"grip_class":"weakly_gripped","seam":{"file":"crates/perl-lsp-rs/src/kept.rs","line":40}}]}"#
            ),
            // Unrecognized classification on suppression-relevant paths (#1346).
            concat!(
                r#"{"summary":{"weakly_exposed":1},"findings":["#,
                r#"{"classification":"exposed","probe":{"path":"archive/old.rs"}},"#,
                r#"{"classification":"reachable_unrevealed","probe":{"path":"archive/old.rs"}}]}"#
            ),
            // Summary without findings.
            r#"{"summary":{"weakly_exposed":3,"reachable_unrevealed":0,"no_static_path":0}}"#,
            // Findings without summary (Path B counting).
            concat!(
                r#"{"findings":[{"classification":"no_static_path","probe":{"path":"a.rs"}},"#,
                r#"{"classification":"unknown","probe":{"path":"b.rs"}}]}"#
            ),
            // Neither.
            r#"{"tool":"ripr"}"#,
            r#"{}"#,
            // Degenerate summary/findings shapes the DOM path tolerated.
            r#"{"summary":"not-an-object","findings":null}"#,
            r#"{"summary":{"weakly_exposed":2},"findings":5}"#,
            r#"{"summary":{"weakly_exposed":2},"findings":{"a":1}}"#,
            r#"{"summary":{"weakly_exposed":2},"findings":["a-string",42,null,true,{"classification":"weakly_exposed","probe":{"path":"mixed.rs"}}]}"#,
            // Count fields the DOM path ignored (strings, negatives, floats).
            r#"{"summary":{"weakly_exposed":"3","reachable_unrevealed":-2,"no_static_path":1.5},"findings":[]}"#,
            // Duplicate keys: DOM semantics keep the last occurrence.
            r#"{"summary":{"weakly_exposed":1},"summary":{"weakly_exposed":7},"findings":[],"findings":[]}"#,
            // A non-empty duplicate findings value must replace, not add to,
            // the earlier array (serde_json DOM maps are last-key-wins).
            concat!(
                r#"{"summary":{"reachable_unrevealed":1,"no_static_path":1},"findings":["#,
                r#"{"classification":"no_static_path","probe":{"path":"first.rs","line":1}}],"findings":["#,
                r#"{"classification":"reachable_unrevealed","probe":{"path":"last.rs","line":2}}]}"#
            ),
            // Top-level non-object payloads validate but carry nothing.
            r#"[1,2,3]"#,
            r#""just a string""#,
            r#"42"#,
            r#"-7"#,
            r#"true"#,
            r#"null"#,
            // Trailing whitespace is allowed by both paths.
            "{}\n  ",
        ];
        for payload in payloads {
            assert_streaming_receipt_matches_dom(payload, None, &no_suppressions())?;
        }
        Ok(())
    }

    /// The #6260 reproduction payload: probes on a deleted line must not count
    /// as new gaps under either ingestion path.
    #[test]
    fn streaming_ingestion_receipt_matches_dom_bytes_with_extents() -> Result<()> {
        let payload = concat!(
            r#"{"summary":{"weakly_exposed":0,"reachable_unrevealed":0,"no_static_path":2},"findings":["#,
            r#"{"classification":"no_static_path","kind":"call_deletion","probe":{"path":"xtask/src/tasks/check_version_sync.rs","line":29}},"#,
            r#"{"classification":"no_static_path","kind":"return_value","probe":{"path":"xtask/src/tasks/check_version_sync.rs","line":29}}]}"#
        );
        let extents = HeadLineExtents {
            present: BTreeMap::from([(
                "xtask/src/tasks/check_version_sync.rs".to_string(),
                13usize,
            )]),
            removed: BTreeSet::new(),
        };
        assert_streaming_receipt_matches_dom(payload, Some(&extents), &no_suppressions())
    }

    #[test]
    fn streaming_ingestion_receipt_matches_dom_bytes_with_suppressions() -> Result<()> {
        let temp = tempfile::tempdir()?;
        fs::create_dir_all(temp.path().join("policy"))?;
        fs::write(
            temp.path().join("policy/ripr-suppressions.toml"),
            r#"schema_version = 1
policy = "ripr-suppressions"
owner = "EffortlessMetrics"
status = "advisory"
updated = "2026-05-28"

[[suppress]]
paths = ["archive/**"]
"#,
        )?;
        let rules =
            read_ripr_suppression_rules(temp.path(), Path::new("policy/ripr-suppressions.toml"))?;
        let payload = concat!(
            r#"{"summary":{"weakly_exposed":1,"reachable_unrevealed":2,"no_static_path":0},"findings":["#,
            r#"{"classification":"weakly_exposed","probe":{"path":"archive/old.rs","line":1}},"#,
            r#"{"classification":"reachable_unrevealed","seam":{"file":"archive/deep/nested.rs","line":2}},"#,
            r#"{"classification":"reachable_unrevealed","seam":{"file":"crates/live/src/lib.rs","line":3}}]}"#
        );
        assert_streaming_receipt_matches_dom(payload, None, &rules)
    }

    /// Both paths must reject invalid payloads, and the streaming error must
    /// carry the same ingestion context the DOM path used.
    #[test]
    fn streaming_ingestion_rejects_invalid_payloads_like_the_dom_path() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let suppressions = no_suppressions();
        for (name, payload) in [
            ("garbage", "not json at all"),
            ("truncated", r#"{"summary":{}"#),
            ("trailing-content", r#"{"summary":{}} trailing"#),
            ("bad-string", r#"{"findings":[{"probe":{"path":unterminated}}]"#),
        ] {
            let raw_path = temp.path().join(format!("{name}.json"));
            fs::write(&raw_path, payload)?;
            let streamed =
                ripr_check_ingestion_from_file(&raw_path, &suppressions, None, None, None);
            assert!(
                serde_json::from_str::<Value>(payload).is_err(),
                "{name}: DOM oracle must reject this payload"
            );
            let err = streamed.unwrap_err();
            let message = format!("{err:#}");
            assert!(
                message.contains("ripr check output was not valid JSON"),
                "{name}: error must carry the ingestion context: {message}"
            );
        }
        Ok(())
    }

    /// The raw artifact must carry ripr's stdout byte for byte — the contract
    /// the String-buffering path had via `write_text` (#1346) — while the
    /// streaming transport never holds the payload in memory (#12860).
    #[test]
    fn run_ripr_check_streams_stdout_verbatim_into_the_raw_artifact() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let repo = temp.path().join("repo");
        let stubs = temp.path().join("stubs");
        fs::create_dir_all(&repo)?;
        fs::create_dir_all(&stubs)?;
        let payload = concat!(
            r#"{"summary":{"changed_rust_files":1,"findings":2,"weakly_exposed":1,"reachable_unrevealed":0,"no_static_path":1},"findings":"#,
            r#"[{"classification":"weakly_exposed","probe":{"file":"crates/x/src/a.rs","line":3}},"#,
            r#"{"classification":"no_static_path","probe":{"path":"xtask/src/b.rs","line":8}}]}"#
        );
        let binary = write_ripr_stub(&stubs, "ripr-check-ok", payload, 0)?;
        let _override = override_ripr_bin(&binary)?;
        let options = PrEvidenceOptions {
            root: ".".to_string(),
            base: "origin/main".to_string(),
            head: "HEAD".to_string(),
            pr_head_sha: None,
        };

        run_ripr_check(&repo, &options)?;

        let raw_path = repo.join("target/ripr/pr/raw-check.json");
        let raw = fs::read(&raw_path)?;
        assert_eq!(raw, payload.as_bytes(), "raw artifact must carry stdout verbatim");

        let Some(parent) = raw_path.parent() else {
            bail!("raw artifact path has no parent");
        };
        let names = fs::read_dir(parent)?
            .map(|entry| entry.map(|entry| entry.file_name().to_string_lossy().into_owned()))
            .collect::<std::io::Result<BTreeSet<_>>>()?;
        assert_eq!(names, BTreeSet::from(["raw-check.json".to_string()]));

        let suppressions = no_suppressions();
        let ingestion = ripr_check_ingestion_from_file(&raw_path, &suppressions, None, None, None)?;
        assert!(ingestion.check_summary_present);
        assert_eq!(ingestion.summary_counts.weakly_exposed, 1);
        assert_eq!(ingestion.summary_counts.reachable_unrevealed, 0);
        assert_eq!(ingestion.summary_counts.no_static_path, 1);
        Ok(())
    }

    /// A failing ripr run must surface its status and a bounded stdout excerpt,
    /// and must not leave a raw artifact behind.
    #[test]
    fn run_ripr_check_failure_surfaces_status_without_an_artifact() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let repo = temp.path().join("repo");
        let stubs = temp.path().join("stubs");
        fs::create_dir_all(&repo)?;
        fs::create_dir_all(&stubs)?;
        let binary = write_ripr_stub(&stubs, "ripr-check-fail", "partial payload", 2)?;
        let _override = override_ripr_bin(&binary)?;
        let raw_path = repo.join(PR_RAW_CHECK_JSON);
        fs::create_dir_all(raw_path.parent().ok_or_else(|| eyre!("raw path has no parent"))?)?;
        fs::write(&raw_path, "stale prior run")?;
        let options = PrEvidenceOptions {
            root: ".".to_string(),
            base: "origin/main".to_string(),
            head: "HEAD".to_string(),
            pr_head_sha: None,
        };

        let err = run_ripr_check(&repo, &options).unwrap_err();
        let message = format!("{err:#}");
        assert!(message.contains("status"), "exit status must appear: {message}");
        assert!(
            message.contains("partial payload"),
            "bounded stdout excerpt must aid diagnosis: {message}"
        );
        assert!(
            !raw_path.exists(),
            "a failed run must remove a stale artifact rather than expose it to fallback"
        );
        Ok(())
    }

    /// Lazily generates a `ripr check` payload whose findings[] is ~100MB —
    /// the shape that killed the evidence runner (#12860): few findings, each
    /// carrying a huge unconsumed blob. The generator yields chunk by chunk and
    /// the streaming parser never materializes the findings array and retains
    /// one finding at a time, so this test completing with correct buckets is
    /// the memory-safety shape assertion in CI; the previous String-plus-DOM
    /// ingestion would have buffered all of it before producing anything.
    struct SyntheticCheckStream {
        header: Vec<u8>,
        header_pos: usize,
        findings_remaining: usize,
        pending: Vec<u8>,
        pending_pos: usize,
        footer_served: bool,
        filler: String,
    }

    impl SyntheticCheckStream {
        const FINDING_COUNT: usize = 400;
        /// ~256KB of unconsumed filler per finding, sized like the real
        /// payload (~1.2MB findings, most of it never read by the receipt).
        const FILLER_BYTES: usize = 256 * 1024;

        fn new() -> Self {
            Self {
                header: format!(
                    r#"{{"schema_version":"0.2","tool":"ripr","summary":{{"findings":{},"weakly_exposed":{}}},"findings":["#,
                    Self::FINDING_COUNT,
                    Self::FINDING_COUNT
                )
                .into_bytes(),
                header_pos: 0,
                findings_remaining: Self::FINDING_COUNT,
                pending: Vec::new(),
                pending_pos: 0,
                footer_served: false,
                filler: "x".repeat(Self::FILLER_BYTES),
            }
        }

        fn finding_bytes(&self, index: usize) -> Vec<u8> {
            let comma = if index == 0 { "" } else { "," };
            format!(
                "{comma}{{\"id\":\"probe:{index}\",\"classification\":\"weakly_exposed\",\"severity\":\"info\",\"confidence\":1.0,\"probe\":{{\"file\":\"crates/x/src/{index}.rs\",\"line\":3}},\"evidence\":\"{}\"}}",
                self.filler
            )
            .into_bytes()
        }
    }

    impl Read for SyntheticCheckStream {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.header_pos < self.header.len() {
                let len = (self.header.len() - self.header_pos).min(buf.len());
                buf[..len].copy_from_slice(&self.header[self.header_pos..][..len]);
                self.header_pos += len;
                return Ok(len);
            }
            if self.pending_pos < self.pending.len() {
                let len = (self.pending.len() - self.pending_pos).min(buf.len());
                buf[..len].copy_from_slice(&self.pending[self.pending_pos..][..len]);
                self.pending_pos += len;
                return Ok(len);
            }
            if self.findings_remaining > 0 {
                let index = Self::FINDING_COUNT - self.findings_remaining;
                self.findings_remaining -= 1;
                self.pending = self.finding_bytes(index);
                self.pending_pos = 0;
                return self.read(buf);
            }
            if !self.footer_served {
                self.footer_served = true;
                self.pending = b"]}".to_vec();
                self.pending_pos = 0;
                return self.read(buf);
            }
            Ok(0)
        }
    }

    #[test]
    fn streaming_ingestion_completes_on_large_payload_with_correct_buckets() -> Result<()> {
        let seen = std::cell::Cell::new(0usize);
        let summary = stream_ripr_check_payload(
            BufReader::with_capacity(64 * 1024, SyntheticCheckStream::new()),
            &mut |_finding| seen.set(seen.get() + 1),
        )?;
        assert_eq!(seen.get(), SyntheticCheckStream::FINDING_COUNT);
        let Some(summary) = summary else {
            bail!("synthetic payload carries a summary");
        };
        assert_eq!(
            count_field(summary.as_object(), "weakly_exposed"),
            SyntheticCheckStream::FINDING_COUNT
        );

        // Bucket correctness over the streamed findings, without the summary seed.
        let mut buckets = RiprFindingBuckets::default();
        let summary = stream_ripr_check_payload(
            BufReader::with_capacity(64 * 1024, SyntheticCheckStream::new()),
            &mut |finding| buckets.absorb(finding, &no_suppressions(), None, None, None),
        )?;
        assert!(summary.is_some());
        let counts = ripr_summary_counts_merge(ripr_summary_counts_seed(None), buckets, false);
        assert_eq!(counts.weakly_exposed, SyntheticCheckStream::FINDING_COUNT);
        assert_eq!(counts.reachable_unrevealed, 0);
        assert_eq!(counts.no_static_path, 0);
        Ok(())
    }

    /// Bounded local re-run against a retained diagnostics payload (#12860),
    /// for the wt-12860 lane's 2.1GB artifact. Skipped by default:
    /// `RIPR_LARGE_RAW_CHECK=<path> cargo test -p xtask -- --ignored streaming_ingestion_handles_retained_large_payload`
    ///
    /// The retained artifact is a truncated document (the diagnosed run was
    /// killed while ripr was still writing), so full-document ingestion cannot
    /// complete for it. What this probe pins instead is the absence of the
    /// kill signature: the whole payload streams through a fixed-size reader
    /// and the truncated document fails cleanly with the ingestion context —
    /// no multi-gigabyte String, no DOM, no abort.
    #[test]
    #[ignore]
    fn streaming_ingestion_handles_retained_large_payload_when_asked() -> Result<()> {
        let Some(path) = env::var_os("RIPR_LARGE_RAW_CHECK") else {
            bail!("set RIPR_LARGE_RAW_CHECK to a retained raw-check.json to run this probe");
        };
        let started = Instant::now();
        match ripr_check_ingestion_from_file(Path::new(&path), &no_suppressions(), None, None, None)
        {
            Ok(ingestion) => println!(
                "ingested {} in {:?}: weakly_exposed={} reachable_unrevealed={} no_static_path={}",
                Path::new(&path).display(),
                started.elapsed(),
                ingestion.summary_counts.weakly_exposed,
                ingestion.summary_counts.reachable_unrevealed,
                ingestion.summary_counts.no_static_path,
            ),
            Err(err) => {
                let message = format!("{err:#}");
                assert!(
                    message.contains("ripr check output was not valid JSON"),
                    "a truncated payload must fail cleanly at parse, not by exhaustion: {message}"
                );
                println!(
                    "streamed {} to a clean parse failure in {:?} (truncated diagnostics artifact): {message}",
                    Path::new(&path).display(),
                    started.elapsed(),
                );
            }
        }
        Ok(())
    }

    /// Compile a stub `ripr` binary that writes `stdout_text` to stdout and
    /// exits with `exit_code`, the way the #12569 transport helpers do.
    fn write_ripr_stub(
        dir: &Path,
        name: &str,
        stdout_text: &str,
        exit_code: i32,
    ) -> Result<PathBuf> {
        let source = dir.join(format!("{name}.rs"));
        fs::write(
            &source,
            format!(
                "fn main() {{\n    use std::io::Write;\n    let payload = {stdout_text:?};\n    let _ = std::io::stdout().write_all(payload.as_bytes());\n    std::process::exit({exit_code});\n}}\n"
            ),
        )?;
        #[cfg(windows)]
        let binary = dir.join(format!("{name}.exe"));
        #[cfg(not(windows))]
        let binary = dir.join(name);
        let output = Command::new("rustc")
            .arg(&source)
            .arg("-o")
            .arg(&binary)
            .output()
            .context("failed to compile ripr stub")?;
        if !output.status.success() {
            bail!(
                "failed to compile ripr stub:\n{}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(binary)
    }
}
