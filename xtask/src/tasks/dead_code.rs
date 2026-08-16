//! Dead code detection for the perl-lsp workspace
//!
//! Rust source/item liveness: clippy dead_code lints.
//! Dependency-unused analysis has moved to `dependency-hygiene` (issue #9364).
//!
//! ## Compatibility boundary
//!
//! `dead-code check` is a Rust source/item liveness check and does not run a
//! dependency instrument. The legacy `cargo-udeps` invocation remains only in
//! baseline/report generation to preserve their existing output schemas; those
//! counts are advisory and must not be treated as a dependency verdict. The
//! canonical dependency-unused authority is `dependency-hygiene`.
//!
//! Supports three modes:
//! - `check`: Compare current state against baseline thresholds
//! - `baseline`: Generate a new baseline YAML file
//! - `report`: Generate a JSON report for CI integration

use std::fs;
use std::io::BufRead;
use std::path::{Path, PathBuf};

use chrono::Utc;
use color_eyre::eyre::{Context, Result, eyre};
use duct::cmd;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// The mode in which to run dead code detection.
#[derive(Clone, Debug, clap::ValueEnum)]
pub enum DeadCodeMode {
    /// Check current state against baseline (default)
    Check,
    /// Generate a new baseline YAML
    Baseline,
    /// Generate a JSON report for CI
    Report,
}

/// Configuration for the dead-code subcommand.
pub struct DeadCodeConfig {
    pub mode: DeadCodeMode,
    pub strict: bool,
}

/// Entry point called from main.
pub fn run(config: DeadCodeConfig) -> Result<()> {
    let root = crate::utils::project_root()?;
    std::env::set_current_dir(&root).context("Failed to change to project root")?;

    println!("[INFO] Dead Code Detection (mode: {:?})", config.mode);
    println!();

    check_tools()?;

    match config.mode {
        DeadCodeMode::Check => run_check(&root, config.strict),
        DeadCodeMode::Baseline => run_baseline(&root),
        DeadCodeMode::Report => run_report(&root),
    }
}

// ---------------------------------------------------------------------------
// Baseline YAML model (read-only — we write it as a template string)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct BaselineFile {
    #[serde(default)]
    thresholds: Thresholds,
    #[serde(default)]
    baseline: BaselineCounts,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct Thresholds {
    #[serde(default = "default_max_5")]
    max_unused_dependencies: u64,
    #[serde(default = "default_max_10")]
    max_dead_code_items: u64,
}

impl Default for Thresholds {
    fn default() -> Self {
        Self { max_unused_dependencies: default_max_5(), max_dead_code_items: default_max_10() }
    }
}

fn default_max_5() -> u64 {
    5
}
fn default_max_10() -> u64 {
    10
}

/// Baseline counts parsed from the YAML file.
///
/// All four fields are deserialized for compatibility. The check phase only
/// enforces the Rust item liveness count; dependency-unused authority belongs
/// to `dependency-hygiene`.
#[derive(Debug, Default, Deserialize)]
#[allow(dead_code)]
struct BaselineCounts {
    #[serde(default)]
    unused_dependencies: u64,
    #[serde(default)]
    dead_code_items: u64,
    #[serde(default)]
    unused_imports: u64,
    #[serde(default)]
    unused_variables: u64,
}

// ---------------------------------------------------------------------------
// JSON report model
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct JsonReport {
    schema_version: u32,
    timestamp: String,
    results: JsonReportResults,
    details: JsonReportDetails,
}

#[derive(Serialize)]
struct JsonReportResults {
    /// Legacy/advisory count retained for report-schema compatibility.
    unused_dependencies: u64,
    dead_code_items: u64,
    unused_imports: u64,
    unused_variables: u64,
    total_issues: u64,
}

#[derive(Serialize)]
struct JsonReportDetails {
    udeps_output: String,
    clippy_output: String,
}

// ---------------------------------------------------------------------------
// Tool checks
// ---------------------------------------------------------------------------

fn check_tools() -> Result<()> {
    // cargo must be on PATH — if it isn't, duct will fail anyway, but give a
    // better error message.
    cmd("cargo", ["--version"])
        .stdout_null()
        .stderr_null()
        .run()
        .context("cargo is not available on PATH")?;

    // Check that nightly toolchain exists (needed for cargo-udeps)
    let rustup_output = cmd("rustup", ["toolchain", "list"]).read().unwrap_or_default();

    if !rustup_output.contains("nightly") {
        println!("[WARN] Nightly toolchain not installed (required for cargo-udeps)");
        println!("[INFO] Install with: rustup toolchain install nightly");
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Running external tools and counting output
// ---------------------------------------------------------------------------

/// Ensure the `target/dead-code/` directory exists and return its path.
fn output_dir(root: &Path) -> Result<PathBuf> {
    let dir = root.join("target/dead-code");
    fs::create_dir_all(&dir).context("Failed to create target/dead-code directory")?;
    Ok(dir)
}

/// Probe cargo-udeps without installing it.
///
/// `run_udeps` must use `.unchecked()`, because cargo-udeps exits non-zero both
/// when it legitimately finds unused dependencies and when it is missing or
/// broken. Those cannot be told apart after the fact, and `count_in_file`
/// counts occurrences of `"unused"` — so an `error: no such command: 'udeps'`
/// capture yields 0 and is written to the baseline/report as a measured zero.
///
/// Probing first is what keeps "the instrument did not run" from being recorded
/// as "the instrument found nothing". This deliberately does not install
/// anything: acquiring a toolchain is not a side effect a check may have.
fn ensure_udeps_available() -> Result<()> {
    let probe = cmd("cargo", ["+nightly", "udeps", "--version"])
        .stdout_null()
        .stderr_null()
        .unchecked()
        .run()
        .context("Failed to probe cargo-udeps")?;

    if !probe.status.success() {
        return Err(eyre!(
            "cargo-udeps is unavailable, so the legacy unused-dependency count cannot be \
             measured. Install it with `cargo install cargo-udeps --locked` (requires the \
             nightly toolchain), or use `cargo xtask dependency-hygiene`, which is the \
             canonical dependency-unused authority. Refusing to write a baseline/report \
             recording an unmeasured 0."
        ));
    }
    Ok(())
}

/// Run legacy cargo-udeps with the historical target/feature scope and write
/// output. This is advisory compatibility data, not the active dependency
/// hygiene verdict.
fn run_udeps(root: &Path) -> Result<PathBuf> {
    let dir = output_dir(root)?;
    let out_path = dir.join("udeps-output.txt");

    println!("[INFO] Checking for unused dependencies with cargo-udeps...");

    let result = cmd("cargo", ["+nightly", "udeps", "--workspace", "--all-targets", "--locked"])
        .stdout_capture()
        .stderr_capture()
        .unchecked()
        .run()
        .context("Failed to run cargo-udeps")?;

    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    fs::write(&out_path, &combined)
        .with_context(|| format!("Failed to write {}", out_path.display()))?;

    Ok(out_path)
}

/// Run clippy with dead-code lints and write output. Returns output path.
fn run_clippy_dead_code(root: &Path) -> Result<PathBuf> {
    let dir = output_dir(root)?;
    let out_path = dir.join("clippy-dead-code.txt");

    println!("[INFO] Checking for dead code with clippy...");

    let result = cmd(
        "cargo",
        [
            "clippy",
            "--workspace",
            "--lib",
            "--bins",
            "--locked",
            "--",
            "-W",
            "dead_code",
            "-W",
            "unused_imports",
            "-W",
            "unused_variables",
        ],
    )
    .stdout_capture()
    .stderr_capture()
    .unchecked()
    .run()
    .context("Failed to run clippy")?;

    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    fs::write(&out_path, &combined)
        .with_context(|| format!("Failed to write {}", out_path.display()))?;

    Ok(out_path)
}

// ---------------------------------------------------------------------------
// Counting helpers
// ---------------------------------------------------------------------------

/// Count how many lines in `text` contain `needle`.
fn count_occurrences(text: &str, needle: &str) -> u64 {
    text.as_bytes().lines().map_while(Result::ok).filter(|line| line.contains(needle)).count()
        as u64
}

/// Count occurrences of `needle` in a file. Returns 0 if the file cannot be read.
fn count_in_file(path: &Path, needle: &str) -> u64 {
    match fs::read_to_string(path) {
        Ok(content) => count_occurrences(&content, needle),
        Err(_) => 0,
    }
}

// ---------------------------------------------------------------------------
// Modes
// ---------------------------------------------------------------------------

fn run_check(root: &Path, strict: bool) -> Result<()> {
    let baseline_path = root.join(".ci/dead-code-baseline.yaml");

    if baseline_path.exists() {
        check_against_baseline(root, &baseline_path, strict)
    } else {
        println!("[INFO] No baseline file, running Rust item liveness checks...");
        run_clippy_dead_code(root)?;
        println!("[SUCCESS] Rust item liveness checks completed");
        Ok(())
    }
}

fn check_against_baseline(root: &Path, baseline_path: &Path, strict: bool) -> Result<()> {
    println!("[INFO] Checking against baseline...");

    // Parse baseline
    let baseline_content = fs::read_to_string(baseline_path)
        .with_context(|| format!("Failed to read {}", baseline_path.display()))?;
    let baseline_file: BaselineFile = serde_yaml_ng::from_str(&baseline_content)
        .with_context(|| format!("Failed to parse {}", baseline_path.display()))?;

    // Dependency-unused analysis is owned by dependency-hygiene. Keep the
    // legacy baseline fields readable, but do not run cargo-udeps or compare
    // its count here as a required dead-code gate.
    let clippy_path = run_clippy_dead_code(root)?;

    // Count current issues
    let current_dead_code = count_in_file(&clippy_path, "dead_code");

    let bl = &baseline_file.baseline;
    let th = &baseline_file.thresholds;

    println!("[INFO] Comparison:");
    println!("  Unused dependencies: advisory only (dependency-hygiene owns this check)");
    println!(
        "  Dead code items:     {} (baseline: {}, max: {})",
        current_dead_code, bl.dead_code_items, th.max_dead_code_items
    );

    let mut failed = false;

    // Check thresholds
    if current_dead_code > th.max_dead_code_items {
        println!(
            "[ERROR] Dead code items ({}) exceeds threshold ({})",
            current_dead_code, th.max_dead_code_items
        );
        failed = true;
    }

    // Check regression against baseline
    if current_dead_code > bl.dead_code_items {
        println!("[WARN] Dead code increased from {} to {}", bl.dead_code_items, current_dead_code);
        if strict {
            failed = true;
        }
    }

    if failed {
        Err(eyre!("Dead code checks failed"))
    } else {
        println!("[SUCCESS] Dead code checks passed");
        Ok(())
    }
}

fn run_baseline(root: &Path) -> Result<()> {
    println!("[INFO] Generating dead code baseline...");

    ensure_udeps_available()?;
    let udeps_path = run_udeps(root)?;
    let clippy_path = run_clippy_dead_code(root)?;

    let unused_deps = count_in_file(&udeps_path, "unused");
    let dead_code = count_in_file(&clippy_path, "dead_code");
    let unused_imports = count_in_file(&clippy_path, "unused_imports");
    let unused_vars = count_in_file(&clippy_path, "unused_variables");

    let now = Utc::now();
    let date_str = now.format("%Y-%m-%d").to_string();
    let datetime_str = now.format("%Y-%m-%d %H:%M:%S UTC").to_string();

    // Compute next review date (30 days from now)
    let next_review = now + chrono::Duration::days(30);
    let next_review_str = next_review.format("%Y-%m-%d").to_string();

    let baseline_content = format!(
        r##"# Dead Code Detection Baseline
# Generated: {datetime_str}
#
# This file tracks the baseline for dead code detection.
# When new dead code is introduced, the checks will fail.
#
# To update this baseline: just dead-code-baseline

schema_version: 1
last_updated: "{date_str}"

# Thresholds (fail if exceeded)
thresholds:
  # Maximum allowed unused dependencies
  max_unused_dependencies: 5

  # Maximum allowed dead code items (functions, types, etc.)
  max_dead_code_items: 10

  # Maximum allowed unused imports
  max_unused_imports: 20

  # Maximum allowed unused variables
  max_unused_variables: 10

# Current baseline counts
baseline:
  unused_dependencies: {unused_deps}
  dead_code_items: {dead_code}
  unused_imports: {unused_imports}
  unused_variables: {unused_vars}

# Allowed exceptions (items that are intentionally unused)
allowed_exceptions:
  # Example: functions that are part of public API but not used internally
  # - crate: perl-parser
  #   type: function
  #   name: parse_legacy_syntax
  #   reason: "Public API for backward compatibility"

  # Example: dependencies used only in specific build configurations
  # - crate: perl-lsp
  #   dependency: tokio
  #   reason: "Used in async runtime feature"

# Known issues to be addressed
known_issues:
  # Track specific dead code items that need cleanup
  # - crate: perl-parser-core
  #   type: dead_code
  #   item: legacy_parser_function
  #   issue: "#XXX"
  #   notes: "Remove after migration to v3 parser"

# Policy
policy:
  # Enforcement level: strict, warn, or disabled
  enforcement: warn

  # Auto-update baseline on PR (requires manual approval)
  auto_update_baseline: false

  # Fail CI if baseline is exceeded
  fail_on_baseline_exceeded: true

  # Warn if approaching threshold (80% of max)
  warn_threshold_percent: 80

# Maintenance
maintenance:
  # Review dead code baseline every N days
  review_interval_days: 30

  # Next scheduled review
  next_review: "{next_review_str}"
"##
    );

    let baseline_path = root.join(".ci/dead-code-baseline.yaml");
    fs::create_dir_all(
        baseline_path.parent().ok_or_else(|| eyre!("baseline path has no parent directory"))?,
    )
    .context("Failed to create .ci/ directory")?;
    fs::write(&baseline_path, baseline_content)
        .with_context(|| format!("Failed to write {}", baseline_path.display()))?;

    println!("[SUCCESS] Baseline saved to {}", baseline_path.display());
    println!();
    println!("[INFO] Current counts:");
    println!("  Unused dependencies: {unused_deps}");
    println!("  Dead code items:     {dead_code}");
    println!("  Unused imports:      {unused_imports}");
    println!("  Unused variables:    {unused_vars}");

    Ok(())
}

fn run_report(root: &Path) -> Result<()> {
    println!("[INFO] Generating JSON report...");

    ensure_udeps_available()?;
    let udeps_path = run_udeps(root)?;
    let clippy_path = run_clippy_dead_code(root)?;

    let unused_deps = count_in_file(&udeps_path, "unused");
    let dead_code = count_in_file(&clippy_path, "dead_code");
    let unused_imports = count_in_file(&clippy_path, "unused_imports");
    let unused_vars = count_in_file(&clippy_path, "unused_variables");

    let report = JsonReport {
        schema_version: 1,
        timestamp: Utc::now().to_rfc3339(),
        results: JsonReportResults {
            unused_dependencies: unused_deps,
            dead_code_items: dead_code,
            unused_imports,
            unused_variables: unused_vars,
            total_issues: unused_deps + dead_code + unused_imports + unused_vars,
        },
        details: JsonReportDetails {
            udeps_output: udeps_path.display().to_string(),
            clippy_output: clippy_path.display().to_string(),
        },
    };

    let report_path = output_dir(root)?.join("report.json");
    let json = serde_json::to_string_pretty(&report).context("Failed to serialize report")?;
    fs::write(&report_path, &json)
        .with_context(|| format!("Failed to write {}", report_path.display()))?;

    println!("[SUCCESS] Report saved to {}", report_path.display());
    println!("{json}");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_count_occurrences_empty() {
        assert_eq!(count_occurrences("", "unused"), 0);
    }

    #[test]
    fn test_count_occurrences_no_match() {
        assert_eq!(count_occurrences("hello\nworld\n", "unused"), 0);
    }

    #[test]
    fn test_count_occurrences_single_match() {
        assert_eq!(count_occurrences("line1\nunused dep foo\nline3\n", "unused"), 1);
    }

    #[test]
    fn test_count_occurrences_multiple_matches() {
        let text = "unused dep a\nok\nunused dep b\nunused dep c\n";
        assert_eq!(count_occurrences(text, "unused"), 3);
    }

    #[test]
    fn test_count_occurrences_dead_code() {
        let text = "warning: dead_code in foo\nwarning: dead_code in bar\nother line\n";
        assert_eq!(count_occurrences(text, "dead_code"), 2);
    }

    #[test]
    fn test_baseline_deserialization() -> color_eyre::eyre::Result<()> {
        let yaml = r#"
schema_version: 1
last_updated: "2026-01-28"
thresholds:
  max_unused_dependencies: 5
  max_dead_code_items: 10
baseline:
  unused_dependencies: 2
  dead_code_items: 3
  unused_imports: 4
  unused_variables: 1
"#;
        let parsed: BaselineFile = serde_yaml_ng::from_str(yaml)?;
        assert_eq!(parsed.thresholds.max_unused_dependencies, 5);
        assert_eq!(parsed.thresholds.max_dead_code_items, 10);
        assert_eq!(parsed.baseline.unused_dependencies, 2);
        assert_eq!(parsed.baseline.dead_code_items, 3);
        assert_eq!(parsed.baseline.unused_imports, 4);
        assert_eq!(parsed.baseline.unused_variables, 1);
        Ok(())
    }

    #[test]
    fn test_baseline_deserialization_defaults() -> color_eyre::eyre::Result<()> {
        let yaml = r#"
schema_version: 1
"#;
        let parsed: BaselineFile = serde_yaml_ng::from_str(yaml)?;
        assert_eq!(parsed.thresholds.max_unused_dependencies, 5);
        assert_eq!(parsed.thresholds.max_dead_code_items, 10);
        assert_eq!(parsed.baseline.unused_dependencies, 0);
        assert_eq!(parsed.baseline.dead_code_items, 0);
        Ok(())
    }

    #[test]
    fn test_count_in_file_missing_file() {
        assert_eq!(count_in_file(Path::new("/nonexistent/file.txt"), "foo"), 0);
    }
}
