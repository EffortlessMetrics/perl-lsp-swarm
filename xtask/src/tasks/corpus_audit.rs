//! Corpus audit task implementation
//!
//! This module provides comprehensive corpus coverage analysis including:
//! - Corpus inventory and structure analysis
//! - NodeKind reachability analysis
//! - GA feature-to-fixture alignment
//! - Timeout/hang risk detection
//! - Machine-readable report generation

use color_eyre::eyre::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

mod corpus;
mod ga_alignment;
mod nodekind_analysis;
mod report;
mod timeout_detection;

use corpus::{CorpusFile, parse_corpus_files};
use ga_alignment::check_ga_feature_alignment;
use nodekind_analysis::analyze_nodekind_coverage;
use report::generate_report;
use timeout_detection::{ParseOutcome, detect_timeout_risks, parse_with_timeout};

pub use report::{AuditReport, FailingFile, ParseOutcomesSummary};

/// Default timeout for parsing individual files
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// Maximum nesting depth to prevent stack overflow
const MAX_NESTING_DEPTH: usize = 100;

/// Maximum regex operations to prevent exponential backtracking
const MAX_REGEX_OPERATIONS: usize = 10_000;

/// Maximum heredoc nesting depth
const MAX_HEREDOC_DEPTH: usize = 100;

/// Maximum heredoc content size (1MB)
const MAX_HEREDOC_SIZE: usize = 1_000_000;

/// Configuration for corpus audit
#[derive(Debug, Clone)]
pub struct AuditConfig {
    /// Path to corpus directory
    pub corpus_path: PathBuf,
    /// Output path for JSON report
    pub output_path: PathBuf,
    /// Timeout for parsing individual files
    pub timeout: Duration,
    /// Whether to regenerate reports (--fresh flag)
    pub fresh: bool,
    /// Whether to run in check mode for CI (--check flag)
    pub check: bool,
}

impl Default for AuditConfig {
    fn default() -> Self {
        Self {
            corpus_path: PathBuf::from("crates/perl-corpus"),
            output_path: PathBuf::from("target/corpus-audit-report.json"),
            timeout: DEFAULT_TIMEOUT,
            fresh: false,
            check: false,
        }
    }
}

/// Lightweight corpus snapshot for status reporting.
#[derive(Debug, Clone)]
pub struct StatusSummary {
    pub total_files: usize,
    pub ok_files: usize,
    pub error_files: usize,
    pub timeout_files: usize,
    pub panic_files: usize,
    pub test_corpus_files: usize,
    pub perl_corpus_files: usize,
    pub nodekind_covered: usize,
    pub nodekind_total: usize,
    pub nodekind_never_seen: usize,
    pub nodekind_allowlisted_never_seen: usize,
    pub nodekind_actionable_never_seen: usize,
    pub ga_covered: usize,
    pub ga_total: usize,
}

/// Compute a lightweight repo-corpus summary for `update-status`.
///
/// This reuses the same corpus discovery and parse logic as `parser-audit`, but
/// stops after the metrics needed for `docs/project/status/parser.md`.
pub fn compute_status_summary(corpus_path: &Path, timeout: Duration) -> Result<StatusSummary> {
    let corpus_files = parse_corpus_files(corpus_path)?;
    let inventory = corpus::generate_inventory(&corpus_files);
    let parse_results = parse_corpus_with_timeout(&corpus_files, timeout)?;
    let nodekind_stats = analyze_nodekind_coverage(&parse_results);
    let ga_coverage = check_ga_feature_alignment(&corpus_files)?;

    let mut ok_files = 0usize;
    let mut error_files = 0usize;
    let mut timeout_files = 0usize;
    let mut panic_files = 0usize;

    for outcome in parse_results.values() {
        match outcome {
            ParseOutcome::Ok { .. } => ok_files += 1,
            ParseOutcome::Error { .. } => error_files += 1,
            ParseOutcome::Timeout { .. } => timeout_files += 1,
            ParseOutcome::Panic { .. } => panic_files += 1,
        }
    }

    let mut test_corpus_files = 0usize;
    let mut perl_corpus_files = 0usize;
    for layer_count in inventory.files_by_layer {
        match layer_count.layer {
            corpus::CorpusLayer::TestCorpus => test_corpus_files = layer_count.count,
            corpus::CorpusLayer::PerlCorpus => perl_corpus_files = layer_count.count,
            _ => {}
        }
    }

    Ok(StatusSummary {
        total_files: inventory.total_files,
        ok_files,
        error_files,
        timeout_files,
        panic_files,
        test_corpus_files,
        perl_corpus_files,
        nodekind_covered: nodekind_stats.covered_count,
        nodekind_total: nodekind_stats.total_count,
        nodekind_never_seen: nodekind_stats.never_seen.len(),
        nodekind_allowlisted_never_seen: nodekind_stats.allowlisted_never_seen.len(),
        nodekind_actionable_never_seen: nodekind_stats.actionable_never_seen.len(),
        ga_covered: ga_coverage.covered_count,
        ga_total: ga_coverage.total_count,
    })
}

/// Run corpus audit with the given configuration
pub fn run(config: AuditConfig) -> Result<()> {
    let start_time = Instant::now();

    println!("🔍 Starting corpus audit...");
    println!("   Corpus path: {}", config.corpus_path.display());
    println!("   Output path: {}", config.output_path.display());
    println!("   Timeout: {:?}", config.timeout);
    println!("   Mode: {}", if config.check { "check (CI)" } else { "full" });

    // Create output directory if needed
    if let Some(parent) = config.output_path.parent() {
        fs::create_dir_all(parent).context("Failed to create output directory")?;
    }

    // Check if report already exists and not in fresh mode
    if !config.fresh && config.output_path.exists() && config.check {
        println!("ℹ️  Using existing report (use --fresh to regenerate)");
        let report_content =
            fs::read_to_string(&config.output_path).context("Failed to read existing report")?;
        let report: AuditReport =
            serde_json::from_str(&report_content).context("Failed to parse existing report")?;

        // In check mode, validate the report and exit
        return validate_report_for_ci(&report);
    }

    // Step 1: Parse corpus files with timeout protection
    println!("\n📂 Step 1: Parsing corpus files...");
    let corpus_files = parse_corpus_files(&config.corpus_path)?;
    let parse_results = parse_corpus_with_timeout(&corpus_files, config.timeout)?;

    // Step 2: Analyze NodeKind coverage
    println!("\n🔢 Step 2: Analyzing NodeKind coverage...");
    let nodekind_stats = analyze_nodekind_coverage(&parse_results);

    // Step 3: Check GA feature alignment
    println!("\n🎯 Step 3: Checking GA feature alignment...");
    let ga_coverage = check_ga_feature_alignment(&corpus_files)?;

    // Step 4: Detect timeout/hang risks
    println!("\n⏱️  Step 4: Detecting timeout/hang risks...");
    let timeout_risks = detect_timeout_risks(&corpus_files);

    // Step 5: Generate report
    println!("\n📊 Step 5: Generating report...");
    let report = generate_report(
        corpus_files,
        parse_results,
        nodekind_stats,
        ga_coverage,
        timeout_risks,
        start_time.elapsed(),
    );

    // Write report to file
    let report_json =
        serde_json::to_string_pretty(&report).context("Failed to serialize report")?;
    fs::write(&config.output_path, report_json).context("Failed to write report file")?;

    println!("\n✅ Corpus audit completed successfully!");
    println!("   Report written to: {}", config.output_path.display());

    // Print summary
    print_audit_summary(&report);

    // In check mode, validate and exit with appropriate code
    if config.check {
        return validate_report_for_ci(&report);
    }

    Ok(())
}

pub fn run_parse_one(path: PathBuf) -> Result<()> {
    timeout_detection::run_parse_one(path)
}

/// Parse all corpus files with timeout protection
fn parse_corpus_with_timeout(
    corpus_files: &[CorpusFile],
    timeout: Duration,
) -> Result<HashMap<PathBuf, ParseOutcome>> {
    let spinner = ProgressBar::new(corpus_files.len() as u64);
    spinner.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} {msg}")
            .unwrap_or_else(|_| ProgressStyle::default_bar())
            .progress_chars("=>-"),
    );

    let mut results = HashMap::new();

    for file in corpus_files {
        spinner.set_message(format!("Parsing {}", file.path.display()));

        let outcome = parse_with_timeout(&file.path, &file.content, timeout);

        results.insert(file.path.clone(), outcome);

        spinner.inc(1);
    }

    spinner.finish_with_message("Parsing complete");

    Ok(results)
}

/// Print a summary of the audit results
fn print_audit_summary(report: &AuditReport) {
    println!("\n📋 Audit Summary:");
    println!("   Total files: {}", report.inventory.total_files);
    println!("   Parse results:");
    println!("     - OK: {} ✅", report.parse_outcomes.ok);
    println!("     - Error: {} ❌", report.parse_outcomes.error);
    println!("     - Timeout: {} ⏱️", report.parse_outcomes.timeout);
    println!("     - Panic: {} 💥", report.parse_outcomes.panic);
    println!(
        "   NodeKind coverage: {}/{} ({:.1}%)",
        report.nodekind_coverage.covered_count,
        report.nodekind_coverage.total_count,
        report.nodekind_coverage.coverage_percentage
    );
    println!("   Never-seen NodeKinds: {}", report.nodekind_coverage.never_seen.len());
    if !report.nodekind_coverage.allowlisted_never_seen.is_empty() {
        println!(
            "   Allowlisted never-seen NodeKinds: {}",
            report.nodekind_coverage.allowlisted_never_seen.len()
        );
        for entry in &report.nodekind_coverage.allowlisted_never_seen {
            println!("     - {}: {}", entry.name, entry.rationale);
        }
    }
    if !report.nodekind_coverage.actionable_never_seen.is_empty() {
        println!(
            "   Actionable never-seen NodeKinds: {}",
            report.nodekind_coverage.actionable_never_seen.len()
        );
        println!("     Names: {}", report.nodekind_coverage.actionable_never_seen.join(", "));
    }
    println!("   At-risk NodeKinds (<5 occurrences): {}", report.nodekind_coverage.at_risk.len());
    println!(
        "   GA features covered: {}/{} ({:.1}%)",
        report.ga_coverage.covered_count,
        report.ga_coverage.total_count,
        report.ga_coverage.coverage_percentage
    );
    println!("   Timeout/hang risks: {}", report.timeout_risks.len());

    if !report.timeout_risks.is_empty() {
        println!("\n⚠️  Timeout/Hang Risks:");
        for risk in &report.timeout_risks {
            println!(
                "   - {:?}: {} ({})",
                risk.priority,
                risk.description,
                risk.file_path.display()
            );
        }
    }
}

/// Validate report for CI gate with baseline ratcheting (Issue #180)
///
/// Returns Ok(()) if report passes validation, otherwise returns error.
/// Parse errors use baseline ratcheting (can only decrease, never increase).
fn validate_report_for_ci(report: &AuditReport) -> Result<()> {
    println!("\n🔬 Validating report for CI gate...");

    let mut failures = Vec::new();

    // Parse error ratchet: read baseline and compare (Issue #180)
    let baseline_path = std::path::Path::new("ci/parse_errors_baseline.txt");
    let current_errors = report.parse_outcomes.error;

    if baseline_path.exists() {
        let baseline_str =
            fs::read_to_string(baseline_path).context("Failed to read parse errors baseline")?;
        let baseline: usize =
            baseline_str.trim().parse().context("Failed to parse baseline as number")?;

        println!("   Parse errors: {} (baseline: {})", current_errors, baseline);

        if current_errors > baseline {
            failures.push(format!(
                "Parse error regression: {} > {} baseline. Fix parser or update baseline.",
                current_errors, baseline
            ));
        } else if current_errors < baseline {
            println!(
                "   📉 IMPROVEMENT: {} fewer errors! Update baseline: echo {} > ci/parse_errors_baseline.txt",
                baseline - current_errors,
                current_errors
            );
        }
    } else {
        // No baseline file - just report the count
        println!("   Parse errors: {} (no baseline file)", current_errors);
    }

    // Timeouts should always be zero
    if report.parse_outcomes.timeout > 0 {
        failures.push(format!("Parse timeouts: {} files timed out", report.parse_outcomes.timeout));
    }

    // Panics should always be zero
    if report.parse_outcomes.panic > 0 {
        failures.push(format!("Parse panics: {} files caused panics", report.parse_outcomes.panic));
    }

    // Check for critical timeout risks
    let critical_risks: Vec<_> = report
        .timeout_risks
        .iter()
        .filter(|r| r.priority == timeout_detection::RiskPriority::P0)
        .collect();

    if !critical_risks.is_empty() {
        failures
            .push(format!("Critical timeout risks: {} P0 risks detected", critical_risks.len()));
    }

    // Check GA feature coverage
    if report.ga_coverage.coverage_percentage < 80.0 {
        failures.push(format!(
            "Low GA feature coverage: {:.1}% (target: 80%)",
            report.ga_coverage.coverage_percentage
        ));
    }

    // Parser closeout ratchets sourced from .ci/metrics/baselines/parser.json.
    // Propagate errors so a corrupted baseline does not silently pass.
    let floor_metrics = load_parser_floor_metrics()?;

    failures.extend(parser_ratchet_failure_messages(
        &floor_metrics,
        &report.nodekind_coverage,
        report.parse_outcomes.error,
    ));

    // Print error category breakdown if there are errors (Issue #180)
    if current_errors > 0 && !report.parse_outcomes.error_by_category.is_empty() {
        println!("\n   Error breakdown by category:");
        let mut categories: Vec<_> = report.parse_outcomes.error_by_category.iter().collect();
        categories.sort_by(|a, b| b.1.cmp(a.1)); // Sort by count descending
        for (category, count) in categories {
            println!("     - {}: {}", category, count);
        }
    }

    if failures.is_empty() {
        println!("\n✅ CI gate passed!");
        Ok(())
    } else {
        println!("\n❌ CI gate failed:");
        for failure in &failures {
            println!("   - {}", failure);
        }
        Err(color_eyre::eyre::eyre!("CI gate validation failed: {}", failures.join("; ")))
    }
}

fn parser_ratchet_failure_messages(
    floor_metrics: &BTreeMap<String, Option<f64>>,
    nodekind: &nodekind_analysis::NodeKindStats,
    parse_error: usize,
) -> Vec<String> {
    let mut failures = Vec::new();

    if !nodekind.actionable_never_seen.is_empty() {
        failures.push(format!(
            "Actionable never-seen NodeKinds ({}): {}",
            nodekind.actionable_never_seen.len(),
            nodekind.actionable_never_seen.join(", ")
        ));
    }

    // An empty actionable list is only trustworthy evidence when the omission
    // classification forms a complete, canonical, duplicate-free partition of
    // never_seen; otherwise the zero-actionable ratchet cannot be believed.
    failures.extend(nodekind_analysis::omission_partition_failures(nodekind));

    if let Some(Some(baseline_nodekind)) = floor_metrics.get("node_kind_coverage") {
        let current_nodekind = if nodekind.total_count == 0 {
            0.0
        } else {
            nodekind.covered_count as f64 / nodekind.total_count as f64
        };
        if current_nodekind + 1e-6 < *baseline_nodekind {
            failures.push(format!(
                "NodeKind coverage regression: {current_nodekind:.4} < {baseline_nodekind:.4} baseline"
            ));
        }
    }

    if let Some(Some(baseline_gap_count)) = floor_metrics.get("valid_parser_gap_count") {
        let current_gap_count = parse_error as f64;
        if current_gap_count > *baseline_gap_count {
            failures.push(format!(
                "Valid parser gap regression: {current_gap_count:.0} > {baseline_gap_count:.0} baseline"
            ));
        }
    }

    failures
}

fn load_parser_floor_metrics() -> Result<BTreeMap<String, Option<f64>>> {
    load_parser_floor_metrics_from(std::path::Path::new(".ci/metrics/baselines/parser.json"))
}

fn load_parser_floor_metrics_from(
    baseline_path: &std::path::Path,
) -> Result<BTreeMap<String, Option<f64>>> {
    if !baseline_path.exists() {
        return Ok(BTreeMap::new());
    }

    let baseline_raw =
        fs::read_to_string(baseline_path).context("Failed to read parser metrics baseline")?;
    let baseline_json: serde_json::Value =
        serde_json::from_str(&baseline_raw).context("Failed to parse parser metrics baseline")?;

    // Malformed baseline (missing/non-object "floor_metrics") is a hard error,
    // not a silent pass, so regressions cannot be hidden by a corrupt baseline.
    let obj = baseline_json.get("floor_metrics").and_then(|v| v.as_object()).ok_or_else(|| {
        color_eyre::eyre::eyre!(
            "Parser metrics baseline is missing or has invalid 'floor_metrics' object: {}",
            baseline_path.display()
        )
    })?;

    let floor_metrics =
        obj.iter().map(|(k, v)| (k.clone(), v.as_f64())).collect::<BTreeMap<String, Option<f64>>>();
    Ok(floor_metrics)
}

/// Test function to verify corpus audit functionality
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    #[test]
    fn test_default_config() {
        let config = AuditConfig::default();
        assert_eq!(config.corpus_path, PathBuf::from("crates/perl-corpus"));
        assert_eq!(config.timeout, DEFAULT_TIMEOUT);
        assert!(!config.fresh);
        assert!(!config.check);
    }

    #[test]
    fn test_timeout_constants() {
        assert_eq!(DEFAULT_TIMEOUT.as_secs(), 30);
        assert_eq!(MAX_NESTING_DEPTH, 100);
        assert_eq!(MAX_REGEX_OPERATIONS, 10_000);
        assert_eq!(MAX_HEREDOC_DEPTH, 100);
        assert_eq!(MAX_HEREDOC_SIZE, 1_000_000);
    }

    // --------------------------------------------------------------------------
    // load_parser_floor_metrics_from
    // --------------------------------------------------------------------------

    fn write_temp_baseline(json: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().expect("tempfile");
        write!(f, "{json}").expect("write");
        f
    }

    #[test]
    fn test_load_floor_metrics_parses_float_and_int_values() {
        let f = write_temp_baseline(
            r#"{"floor_metrics":{"node_kind_coverage":0.942029,"valid_parser_gap_count":0}}"#,
        );
        let metrics = load_parser_floor_metrics_from(f.path()).expect("should parse");
        assert_eq!(metrics.get("node_kind_coverage"), Some(&Some(0.942029)));
        // JSON integer 0 must coerce to Some(0.0), not None
        assert_eq!(metrics.get("valid_parser_gap_count"), Some(&Some(0.0)));
    }

    #[test]
    fn test_load_floor_metrics_absent_file_returns_empty() {
        let path = std::path::Path::new("/tmp/this-file-definitely-does-not-exist-xyz.json");
        let metrics = load_parser_floor_metrics_from(path).expect("should return empty");
        assert!(metrics.is_empty());
    }

    #[test]
    fn test_load_floor_metrics_malformed_json_is_error() {
        let f = write_temp_baseline("not valid json");
        assert!(load_parser_floor_metrics_from(f.path()).is_err());
    }

    #[test]
    fn test_load_floor_metrics_missing_floor_metrics_key_is_error() {
        let f = write_temp_baseline(r#"{"other_key":42}"#);
        assert!(
            load_parser_floor_metrics_from(f.path()).is_err(),
            "missing 'floor_metrics' object must be a hard error, not a silent pass"
        );
    }

    #[test]
    fn test_load_floor_metrics_non_numeric_value_maps_to_none() {
        let f = write_temp_baseline(r#"{"floor_metrics":{"node_kind_coverage":"not-a-number"}}"#);
        let metrics = load_parser_floor_metrics_from(f.path()).expect("should parse");
        // A non-numeric value becomes None; the ratchet guard uses `if let Some(Some(...))` so it
        // silently skips the check rather than panicking or failing.
        assert_eq!(metrics.get("node_kind_coverage"), Some(&None));
    }

    // --------------------------------------------------------------------------
    // Parser floor ratchet logic (actionable NodeKind / ratio / gap-count)
    // --------------------------------------------------------------------------

    /// Build a minimal floor_metrics map for ratchet tests.
    fn floor_metrics_from(
        nodekind: Option<f64>,
        gap_count: Option<f64>,
    ) -> BTreeMap<String, Option<f64>> {
        let mut m = BTreeMap::new();
        m.insert("node_kind_coverage".to_string(), nodekind);
        m.insert("valid_parser_gap_count".to_string(), gap_count);
        m
    }

    /// Exercise the production ratchet with a fully covered, partition-clean
    /// NodeKindStats (no omissions at all).
    fn ratchet_failure_messages(
        floor_metrics: &BTreeMap<String, Option<f64>>,
        nodekind_covered: usize,
        nodekind_total: usize,
        parse_error: usize,
    ) -> Vec<String> {
        let stats = nodekind_analysis::NodeKindStats {
            total_count: nodekind_total,
            covered_count: nodekind_covered,
            coverage_percentage: if nodekind_total == 0 {
                0.0
            } else {
                nodekind_covered as f64 / nodekind_total as f64 * 100.0
            },
            never_seen: Vec::new(),
            allowlisted_never_seen: Vec::new(),
            actionable_never_seen: Vec::new(),
            at_risk: Vec::new(),
            frequency: HashMap::new(),
        };
        parser_ratchet_failure_messages(floor_metrics, &stats, parse_error)
    }

    /// Build NodeKindStats with the exact omission evidence given; used to
    /// construct both partition-consistent and deliberately inconsistent
    /// fixtures.
    fn synthetic_stats(
        never_seen: Vec<String>,
        allowlisted_never_seen: Vec<nodekind_analysis::AllowlistedNodeKind>,
        actionable_never_seen: Vec<String>,
        covered_count: usize,
    ) -> nodekind_analysis::NodeKindStats {
        nodekind_analysis::NodeKindStats {
            total_count: 76,
            covered_count,
            coverage_percentage: covered_count as f64 / 76.0 * 100.0,
            never_seen,
            allowlisted_never_seen,
            actionable_never_seen,
            at_risk: Vec::new(),
            frequency: HashMap::new(),
        }
    }

    fn allowlisted(name: &str) -> nodekind_analysis::AllowlistedNodeKind {
        nodekind_analysis::AllowlistedNodeKind {
            name: name.to_string(),
            rationale: "test recovery-only kind".to_string(),
        }
    }

    fn synthetic_report_from_stats(stats: nodekind_analysis::NodeKindStats) -> AuditReport {
        AuditReport {
            metadata: report::ReportMetadata {
                generated_at: String::new(),
                version: String::new(),
                duration_secs: 0,
            },
            inventory: corpus::CorpusInventory {
                total_files: 0,
                files_by_layer: Vec::new(),
                total_size_bytes: 0,
                total_line_count: 0,
            },
            parse_outcomes: ParseOutcomesSummary {
                total: 0,
                ok: 0,
                error: 0,
                timeout: 0,
                panic: 0,
                error_by_category: HashMap::new(),
                failing_files: Vec::new(),
            },
            nodekind_coverage: stats,
            ga_coverage: ga_alignment::GAFeatureCoverage {
                total_count: 1,
                covered_count: 1,
                coverage_percentage: 100.0,
                features: Vec::new(),
                uncovered_critical: Vec::new(),
                uncovered_partial: Vec::new(),
            },
            timeout_risks: Vec::new(),
        }
    }

    /// A partition-consistent report: never_seen is exactly the union of the
    /// two classification buckets.
    fn synthetic_report(
        actionable_never_seen: Vec<String>,
        allowlisted_never_seen: Vec<nodekind_analysis::AllowlistedNodeKind>,
        covered_count: usize,
    ) -> AuditReport {
        let mut never_seen: Vec<String> = actionable_never_seen
            .iter()
            .cloned()
            .chain(allowlisted_never_seen.iter().map(|entry| entry.name.clone()))
            .collect();
        never_seen.sort();
        synthetic_report_from_stats(synthetic_stats(
            never_seen,
            allowlisted_never_seen,
            actionable_never_seen,
            covered_count,
        ))
    }

    #[test]
    fn test_validate_report_for_ci_rejects_actionable_nodekind_omission() {
        let report = synthetic_report(vec!["AmperCall".to_string()], Vec::new(), 72);

        let result = validate_report_for_ci(&report);
        assert!(result.is_err(), "validator must reject reports with actionable omissions");
        if let Err(error) = result {
            assert!(
                error.to_string().contains("Actionable never-seen NodeKinds (1): AmperCall"),
                "validator error should identify the actionable omission: {error}"
            );
        }
    }

    #[test]
    fn test_nodekind_ratchet_no_regression() {
        let m = floor_metrics_from(Some(0.90), None);
        // 95/100 = 0.95 >= 0.90 → no failure
        let msgs = ratchet_failure_messages(&m, 95, 100, 0);
        assert!(msgs.is_empty(), "expected no failure: {msgs:?}");
    }

    #[test]
    fn test_nodekind_ratchet_fires_on_regression() {
        let m = floor_metrics_from(Some(0.95), None);
        // 90/100 = 0.90 < 0.95 → failure
        let msgs = ratchet_failure_messages(&m, 90, 100, 0);
        assert!(
            msgs.iter().any(|s| s.contains("NodeKind coverage regression")),
            "expected NodeKind failure: {msgs:?}"
        );
    }

    #[test]
    fn test_actionable_nodekind_ratchet_fires_above_ratio_floor() {
        let m = floor_metrics_from(Some(0.942_029), None);

        // 72/76 is still above the historical ratio floor, so only the semantic
        // actionable-omission ratchet can reject this regression.
        let stats = synthetic_stats(
            vec!["AmperCall".to_string()],
            Vec::new(),
            vec!["AmperCall".to_string()],
            72,
        );
        let msgs = parser_ratchet_failure_messages(&m, &stats, 0);

        assert!(
            msgs.iter().any(|s| s == "Actionable never-seen NodeKinds (1): AmperCall"),
            "expected actionable NodeKind failure: {msgs:?}"
        );
        assert!(
            !msgs.iter().any(|s| s.contains("NodeKind coverage regression")),
            "fixture must remain above the legacy ratio floor: {msgs:?}"
        );
    }

    #[test]
    fn test_recovery_only_nodekind_omissions_remain_allowed() {
        let m = floor_metrics_from(Some(0.942_029), None);

        // The current 73/76 state has three recovery-only omissions and no
        // actionable omissions; never_seen is exactly the allowlisted union,
        // so both the semantic ratchet and the partition validator stay green.
        let report = synthetic_report(
            Vec::new(),
            vec![allowlisted("Error"), allowlisted("MissingBlock"), allowlisted("UnknownRest")],
            73,
        );
        assert_eq!(
            report.nodekind_coverage.never_seen.len(),
            3,
            "control fixture must exercise a real recovery-only partition"
        );

        let msgs = parser_ratchet_failure_messages(&m, &report.nodekind_coverage, 0);

        assert!(msgs.is_empty(), "recovery-only omissions must remain allowed: {msgs:?}");
    }

    #[test]
    fn test_validate_report_for_ci_rejects_mismatched_empty_omission_partition() {
        // A cached report claiming never_seen = [AmperCall] with BOTH
        // classification buckets empty: presence-valid, partition-invalid.
        let stats = synthetic_stats(vec!["AmperCall".to_string()], Vec::new(), Vec::new(), 73);
        let report = synthetic_report_from_stats(stats);

        let result = validate_report_for_ci(&report);
        assert!(
            result.is_err(),
            "mismatched-empty cached evidence must not pass the zero-actionable gate"
        );
        if let Err(error) = result {
            assert!(
                error.to_string().contains("Omission partition mismatch"),
                "validator error should identify the partition mismatch: {error}"
            );
        }
    }

    #[test]
    fn test_validate_report_for_ci_rejects_non_recovery_allowlist_entry() {
        // A canonical non-recovery kind can satisfy the partition shape and
        // still be dishonest zero-actionable evidence unless eligibility is
        // checked on the production validation path.
        let report = synthetic_report(Vec::new(), vec![allowlisted("AmperCall")], 73);

        let result = validate_report_for_ci(&report);
        assert!(result.is_err(), "non-recovery allowlist entries must be rejected");
        if let Err(error) = result {
            let message = error.to_string();
            assert!(
                message.contains("'AmperCall'") && message.contains("recovery kind"),
                "validator error should identify the invalid allowlist membership: {error}"
            );
        }
    }

    #[test]
    fn test_validate_report_for_ci_rejects_actionable_recovery_entry() {
        // The opposite dishonest partition is also invalid: recovery-only
        // kinds cannot be made actionable by changing their cached bucket.
        let report = synthetic_report(vec!["Error".to_string()], Vec::new(), 73);

        let result = validate_report_for_ci(&report);
        assert!(result.is_err(), "actionable recovery entries must be rejected");
        if let Err(error) = result {
            let message = error.to_string();
            assert!(
                message.contains("'Error'") && message.contains("recovery kind"),
                "validator error should identify the invalid actionable membership: {error}"
            );
        }
    }

    #[test]
    fn test_zero_actionable_with_dropped_member_is_rejected_by_ratchet() {
        let m = floor_metrics_from(Some(0.942_029), None);
        let stats = synthetic_stats(
            vec!["AmperCall".to_string(), "VString".to_string()],
            Vec::new(),
            vec!["AmperCall".to_string()],
            73,
        );

        let msgs = parser_ratchet_failure_messages(&m, &stats, 0);

        assert!(
            msgs.iter().any(|s| s.contains("Omission partition mismatch")),
            "dropped member must fail the ratchet: {msgs:?}"
        );
        assert!(
            msgs.iter().any(|s| s.contains("'VString'")),
            "dropped member must be named in the failure: {msgs:?}"
        );
    }

    #[test]
    fn test_omission_overlap_between_buckets_is_rejected_by_ratchet() {
        let m = floor_metrics_from(Some(0.942_029), None);
        let stats = synthetic_stats(
            vec!["AmperCall".to_string()],
            vec![allowlisted("AmperCall")],
            vec!["AmperCall".to_string()],
            73,
        );

        let msgs = parser_ratchet_failure_messages(&m, &stats, 0);

        assert!(
            msgs.iter().any(|s| s.contains("Omission classification overlap")),
            "overlap must fail the ratchet: {msgs:?}"
        );
        assert!(
            msgs.iter().any(|s| s.contains("Duplicate NodeKind omission entries")),
            "the same name in both buckets is also a duplicate: {msgs:?}"
        );
    }

    #[test]
    fn test_non_canonical_omission_name_is_rejected_by_ratchet() {
        let m = floor_metrics_from(Some(0.942_029), None);
        let stats = synthetic_stats(
            vec!["AmperCal!".to_string()],
            Vec::new(),
            vec!["AmperCal!".to_string()],
            73,
        );

        let msgs = parser_ratchet_failure_messages(&m, &stats, 0);

        assert!(
            msgs.iter().any(|s| s.contains("Non-canonical NodeKind omission names")),
            "typo'd omission names must fail the ratchet: {msgs:?}"
        );
    }

    #[test]
    fn test_duplicate_omission_entries_are_rejected_by_ratchet() {
        let m = floor_metrics_from(Some(0.942_029), None);
        let stats = synthetic_stats(
            vec!["AmperCall".to_string(), "AmperCall".to_string()],
            Vec::new(),
            vec!["AmperCall".to_string(), "AmperCall".to_string()],
            73,
        );

        let msgs = parser_ratchet_failure_messages(&m, &stats, 0);

        assert!(
            msgs.iter().any(|s| s.contains("Duplicate NodeKind omission entries")),
            "duplicates must fail the ratchet: {msgs:?}"
        );
    }

    #[test]
    fn test_empty_allowlist_rationale_is_rejected_by_ratchet() {
        let m = floor_metrics_from(Some(0.942_029), None);
        let stats = synthetic_stats(
            vec!["MissingBlock".to_string()],
            vec![nodekind_analysis::AllowlistedNodeKind {
                name: "MissingBlock".to_string(),
                rationale: "   ".to_string(),
            }],
            Vec::new(),
            73,
        );

        let msgs = parser_ratchet_failure_messages(&m, &stats, 0);

        assert!(
            msgs.iter().any(|s| s.contains("empty rationale")),
            "blank allowlist rationale must fail the ratchet: {msgs:?}"
        );
    }

    #[test]
    fn test_nodekind_ratchet_epsilon_prevents_false_positive() {
        // current = 0.9420290 + epsilon is still >= 0.9420290, should not fire
        let m = floor_metrics_from(Some(0.942_029), None);
        // 942029/1000000 ≈ 0.942029 — should be within epsilon of baseline
        let msgs = ratchet_failure_messages(&m, 942_029, 1_000_000, 0);
        assert!(msgs.is_empty(), "epsilon guard should prevent false positive: {msgs:?}");
    }

    #[test]
    fn test_gap_count_ratchet_zero_baseline_fires_on_any_error() {
        let m = floor_metrics_from(None, Some(0.0));
        // baseline = 0 errors, current = 1 error → fires
        let msgs = ratchet_failure_messages(&m, 100, 100, 1);
        assert!(
            msgs.iter().any(|s| s.contains("Valid parser gap regression")),
            "expected gap-count failure: {msgs:?}"
        );
    }

    #[test]
    fn test_gap_count_ratchet_zero_baseline_passes_with_no_errors() {
        let m = floor_metrics_from(None, Some(0.0));
        let msgs = ratchet_failure_messages(&m, 100, 100, 0);
        assert!(msgs.is_empty(), "zero errors should pass zero baseline: {msgs:?}");
    }

    #[test]
    fn test_all_ratchets_pass_when_floor_metrics_absent() {
        // Empty map → all `if let Some(Some(...))` arms are skipped → no failures
        let m = BTreeMap::new();
        let msgs = ratchet_failure_messages(&m, 0, 0, 999);
        assert!(msgs.is_empty(), "absent floor metrics must not fail: {msgs:?}");
    }
}
