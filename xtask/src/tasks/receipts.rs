//! Generate canonical receipts for documentation truth
//!
//! This module replaces `scripts/generate-receipts.sh` with a Rust implementation
//! that runs workspace tests and doc builds, parses their output, and produces
//! consolidated JSON artifacts.
//!
//! # Typed completeness (#15350)
//!
//! A count is never authoritative without its completeness and subject identity.
//! Every domain receipt carries a [`DomainStatus`], the repository subject
//! (head + dirty state), a run identity, and a digest of the raw producer output
//! it was parsed from. Producers run with their exit status checked: a failed,
//! killed, or compile-broken invocation can never yield a `complete_*` domain
//! status, and a documentation-truth bundle publisher must refuse domains that
//! are not `complete_pass` or `complete_with_failures`.
//!
//! # Usage
//!
//! ```bash
//! cargo xtask receipts                    # Run all receipt generation
//! cargo xtask receipts --tests-only       # Only generate test receipts
//! cargo xtask receipts --docs-only        # Only generate doc receipts
//! cargo xtask receipts --output-dir path  # Custom output directory
//! ```

use color_eyre::eyre::{Context, Result};
use duct::cmd;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crate::utils::project_root;

// =============================================================================
// Configuration
// =============================================================================

/// Configuration for receipt generation
pub struct ReceiptsConfig {
    /// Only generate test receipts
    pub tests_only: bool,
    /// Only generate doc receipts
    pub docs_only: bool,
    /// Output directory (default: artifacts/)
    pub output_dir: Option<PathBuf>,
    /// Thread count for test execution
    pub test_threads: u32,
}

impl Default for ReceiptsConfig {
    fn default() -> Self {
        Self { tests_only: false, docs_only: false, output_dir: None, test_threads: 2 }
    }
}

// =============================================================================
// Output Types
// =============================================================================

/// Typed completeness of one observation domain (#15350).
///
/// A domain that is not `complete_pass` or `complete_with_failures` cannot
/// back a documentation-truth, test-health, or review-complete claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DomainStatus {
    /// The declared producer population ran to completion with no failures.
    CompletePass,
    /// The declared producer population ran to completion; observed failures
    /// are recorded in the counts.
    CompleteWithFailures,
    /// The mode excluded this domain; it was not run and carries no counts.
    NotRunByMode,
    /// The instrument itself failed (compile error, killed process, empty
    /// output). Any parsed counts are partial diagnostics only.
    InstrumentFailed,
}

impl DomainStatus {
    /// Whether this status can back a complete review/documentation bundle.
    pub(crate) fn is_complete(self) -> bool {
        matches!(self, DomainStatus::CompletePass | DomainStatus::CompleteWithFailures)
    }
}

/// One declared producer invocation with its checked outcome.
#[derive(Debug, Serialize)]
pub(crate) struct ProducerReceipt {
    /// Stable producer name (e.g. "workspace", "comparison-historical").
    pub(crate) name: &'static str,
    /// Process exit code; `None` means the process was terminated by a signal.
    pub(crate) exit_code: Option<i32>,
    /// Whether the producer exited successfully.
    pub(crate) success: bool,
    /// Number of `test result:` lines parsed from this producer's output.
    pub(crate) result_lines: usize,
    /// Digest of this producer's raw output.
    pub(crate) output_digest: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct TestSummary {
    pub(crate) passed: u64,
    pub(crate) failed: u64,
    pub(crate) ignored: u64,
    pub(crate) active_tests: u64,
    pub(crate) total_all_tests: u64,
    pub(crate) pass_rate_active: f64,
    pub(crate) pass_rate_total: f64,
    /// Typed completeness; zeroes without this are not evidence (#15350).
    pub(crate) status: DomainStatus,
    /// Per-producer outcomes backing the aggregate.
    pub(crate) producers: Vec<ProducerReceipt>,
    /// Digest of the concatenated raw test output the counts were parsed from.
    pub(crate) raw_output_digest: String,
}

impl TestSummary {
    /// Summary for a domain the current mode excluded. Zeroes here are
    /// explicitly `not_run_by_mode`, never a measured zero (#15350).
    pub(crate) fn not_run_by_mode() -> Self {
        Self {
            passed: 0,
            failed: 0,
            ignored: 0,
            active_tests: 0,
            total_all_tests: 0,
            pass_rate_active: 0.0,
            pass_rate_total: 0.0,
            status: DomainStatus::NotRunByMode,
            producers: Vec::new(),
            raw_output_digest: "not-run".to_string(),
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct DocSummary {
    pub(crate) missing_docs: u64,
    /// Typed completeness; a rustdoc failure is `instrument_failed`, never
    /// zero debt (#15350).
    pub(crate) status: DomainStatus,
    /// Digest of the raw rustdoc log the count was parsed from.
    pub(crate) raw_output_digest: String,
}

impl DocSummary {
    /// Summary for a domain the current mode excluded (#15350).
    pub(crate) fn not_run_by_mode() -> Self {
        Self {
            missing_docs: 0,
            status: DomainStatus::NotRunByMode,
            raw_output_digest: "not-run".to_string(),
        }
    }
}

/// Repository subject a run is bound to (#15350).
#[derive(Debug, Serialize)]
pub(crate) struct SubjectIdentity {
    /// Full head commit SHA at generation time.
    pub(crate) head: String,
    /// Whether the working tree had uncommitted changes.
    pub(crate) dirty: bool,
}

#[derive(Debug, Serialize)]
struct ConsolidatedState {
    version: String,
    /// Unique identity of this generation run; stale-file mixtures are
    /// rejected against it downstream.
    run_id: String,
    /// Repository subject this run observed.
    subject: SubjectIdentity,
    tests: TestSummary,
    docs: DocSummary,
    generated_at: String,
}

// =============================================================================
// Digest + identity helpers
// =============================================================================

/// Lowercase hex SHA-256 digest of `bytes`.
pub(crate) fn digest_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Generate a unique run identity for this invocation.
fn new_run_id() -> String {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let pid = u64::from(std::process::id());
    let noise = now.subsec_nanos() as u64 ^ (pid << 32);
    format!("run-{}-{noise:016x}", now.as_millis())
}

/// Establish the repository subject (head + dirty state) for a run.
///
/// Refuses to generate receipts when the subject cannot be established: an
/// unbound receipt cannot prove freshness downstream (#15350).
fn current_subject(root: &Path) -> Result<SubjectIdentity> {
    let head_out = cmd!("git", "rev-parse", "HEAD")
        .dir(root)
        .stdout_capture()
        .stderr_capture()
        .unchecked()
        .run()
        .context("Failed to execute git rev-parse HEAD")?;
    if !head_out.status.success() {
        return Err(color_eyre::eyre::eyre!(
            "cannot establish repository subject: git rev-parse HEAD failed; \
             receipts must be bound to a repository head (#15350)"
        ));
    }
    let head = String::from_utf8_lossy(&head_out.stdout).trim().to_string();
    if head.is_empty() {
        return Err(color_eyre::eyre::eyre!(
            "cannot establish repository subject: git rev-parse HEAD printed nothing (#15350)"
        ));
    }

    let status_out = cmd!("git", "status", "--porcelain")
        .dir(root)
        .stdout_capture()
        .stderr_capture()
        .unchecked()
        .run()
        .context("Failed to execute git status --porcelain")?;
    let dirty = status_out.status.success()
        && !String::from_utf8_lossy(&status_out.stdout).trim().is_empty();

    Ok(SubjectIdentity { head, dirty })
}

// =============================================================================
// Domain status decisions (pure, unit-tested)
// =============================================================================

/// Decide the test domain status from checked producer outcomes.
///
/// - all producers succeeded: complete (with failures if any test failed);
/// - any failing producer shows instrument failure (compile error, kill, or
///   no parseable results): the whole aggregate is `instrument_failed` —
///   partial counts from earlier crates cannot mint a complete aggregate;
/// - otherwise (producers ran to completion, cargo exited non-zero because
///   tests failed): `complete_with_failures`.
fn test_domain_status(
    producers: &[ProducerReceipt],
    outputs: &[&str],
    failed_total: u64,
) -> DomainStatus {
    if producers.iter().all(|p| p.success) {
        return if failed_total > 0 {
            DomainStatus::CompleteWithFailures
        } else {
            DomainStatus::CompletePass
        };
    }
    let instrument_failed = producers
        .iter()
        .zip(outputs.iter())
        .any(|(p, output)| !p.success && producer_instrument_failed(p, output));
    if instrument_failed {
        DomainStatus::InstrumentFailed
    } else {
        DomainStatus::CompleteWithFailures
    }
}

/// Whether one failed producer invocation means the instrument (not the tests)
/// failed: killed by a signal, zero parseable results, or compile/link errors.
fn producer_instrument_failed(producer: &ProducerReceipt, output: &str) -> bool {
    if producer.success {
        return false;
    }
    if producer.exit_code.is_none() {
        return true;
    }
    if producer.result_lines == 0 {
        return true;
    }
    const INSTRUMENT_MARKERS: [&str; 5] = [
        "could not compile",
        "error[",
        "linking with",
        "build failed",
        "error: process terminated",
    ];
    INSTRUMENT_MARKERS.iter().any(|marker| output.contains(marker))
}

/// Decide the doc domain status: a rustdoc failure is instrument failure,
/// never zero missing-documentation debt (#15350).
fn doc_domain_status(success: bool) -> DomainStatus {
    if success { DomainStatus::CompletePass } else { DomainStatus::InstrumentFailed }
}

// =============================================================================
// Main Entry Point
// =============================================================================

/// Run receipt generation
pub fn run(config: ReceiptsConfig) -> Result<()> {
    let root = project_root()?;
    std::env::set_current_dir(&root).context("Failed to change to project root")?;

    let artifacts_dir = config.output_dir.clone().unwrap_or_else(|| root.join("artifacts"));
    fs::create_dir_all(&artifacts_dir)
        .with_context(|| format!("Failed to create artifacts dir: {}", artifacts_dir.display()))?;

    let run_id = new_run_id();
    let subject = current_subject(&root)?;
    println!("Receipt run id: {run_id}");
    println!(
        "Receipt subject: head {}, dirty: {}",
        subject.head,
        if subject.dirty { "yes" } else { "no" }
    );

    let test_summary = if !config.docs_only {
        println!("=== Generating Test Receipts ===");
        let summary = generate_test_receipts(&artifacts_dir, config.test_threads)?;
        println!(
            "Test summary: {} passed, {} failed, {} ignored (status: {:?})",
            summary.passed, summary.failed, summary.ignored, summary.status
        );
        summary
    } else {
        println!("=== Test Receipts Not Run (mode: --docs-only) ===");
        TestSummary::not_run_by_mode()
    };

    let doc_summary = if !config.tests_only {
        println!();
        println!("=== Generating Doc Receipts ===");
        let summary = generate_doc_receipts(&artifacts_dir)?;
        println!(
            "Doc summary: {} missing docs (status: {:?})",
            summary.missing_docs, summary.status
        );
        summary
    } else {
        println!("=== Doc Receipts Not Run (mode: --tests-only) ===");
        DocSummary::not_run_by_mode()
    };

    if !config.tests_only && !config.docs_only {
        for (domain, status) in [("tests", test_summary.status), ("docs", doc_summary.status)] {
            if !status.is_complete() {
                println!(
                    "WARNING: {domain} domain status is {status:?}; the counts are partial \
                     diagnostics only. A documentation-truth bundle publisher must refuse \
                     this run (#15350)."
                );
            }
        }
    }

    println!();
    println!("=== Generating Consolidated State ===");
    let state = generate_consolidated_state(run_id, subject, test_summary, doc_summary)?;
    let state_path = artifacts_dir.join("state.json");
    let state_json =
        serde_json::to_string_pretty(&state).context("Failed to serialize consolidated state")?;
    fs::write(&state_path, &state_json)
        .with_context(|| format!("Failed to write state to {}", state_path.display()))?;
    println!("Consolidated state saved to {}", state_path.display());
    println!("{state_json}");

    println!();
    println!("=== Receipt Generation Complete ===");
    println!("Artifacts:");
    println!("  - {}/test-output.txt     (raw test output)", artifacts_dir.display());
    println!("  - {}/test-summary.json   (parsed test metrics)", artifacts_dir.display());
    println!("  - {}/rustdoc.log         (doc build output)", artifacts_dir.display());
    println!("  - {}/doc-summary.json    (doc metrics)", artifacts_dir.display());
    println!("  - {}/state.json          (typed consolidated state)", artifacts_dir.display());

    Ok(())
}

// =============================================================================
// Test Receipt Generation
// =============================================================================

/// Run one declared test producer with its exit status captured and checked.
fn run_test_producer(
    name: &'static str,
    command: duct::Expression,
) -> Result<(ProducerReceipt, String)> {
    let result = command.run().with_context(|| format!("Failed to execute {name} producer"))?;
    let output = String::from_utf8_lossy(&result.stdout).to_string();
    let result_lines =
        output.lines().filter(|line| line.trim().starts_with("test result:")).count();
    let receipt = ProducerReceipt {
        name,
        exit_code: result.status.code(),
        success: result.status.success(),
        result_lines,
        output_digest: digest_hex(output.as_bytes()),
    };
    Ok((receipt, output))
}

/// Run workspace tests and parse results into a typed summary.
///
/// Every declared producer's exit status is checked (#15350): a compile
/// failure, killed process, or zero-result invocation marks the domain
/// `instrument_failed` instead of yielding a complete-looking aggregate.
fn generate_test_receipts(artifacts_dir: &Path, test_threads: u32) -> Result<TestSummary> {
    let start = Instant::now();
    let test_output_path = artifacts_dir.join("test-output.txt");
    let test_summary_path = artifacts_dir.join("test-summary.json");

    let threads_str = test_threads.to_string();

    // Run cargo test, capturing output
    // Exclude xtask which may have compilation issues in some configurations
    let (workspace_receipt, workspace_output) = run_test_producer(
        "workspace",
        cmd!(
            "cargo",
            "+stable",
            "test",
            "--workspace",
            "--exclude",
            "xtask",
            // perl-parser-comparison cannot join the --all-features run: its
            // historical and current-upstream Tree-sitter grammar features export
            // the same native symbol, so linking both into one test binary fails.
            // It is tested separately below, one grammar feature at a time (#7255).
            "--exclude",
            "perl-parser-comparison",
            "--all-features",
            "--no-fail-fast",
            "--",
            "--test-threads",
            &threads_str
        )
        .env("RUST_TEST_THREADS", &threads_str)
        .env("LC_ALL", "C")
        .stderr_to_stdout()
        .stdout_capture()
        .unchecked(),
    )?;

    // One grammar feature per invocation; see the exclusion above.
    let (historical_receipt, historical_output) = run_test_producer(
        "comparison-historical",
        cmd!(
            "cargo",
            "+stable",
            "test",
            "-p",
            "perl-parser-comparison",
            "--no-fail-fast",
            "--",
            "--test-threads",
            &threads_str
        )
        .env("RUST_TEST_THREADS", &threads_str)
        .env("LC_ALL", "C")
        .stderr_to_stdout()
        .stdout_capture()
        .unchecked(),
    )?;
    let (upstream_receipt, upstream_output) = run_test_producer(
        "comparison-current-upstream",
        cmd!(
            "cargo",
            "+stable",
            "test",
            "-p",
            "perl-parser-comparison",
            "--no-default-features",
            "--features",
            "current-upstream",
            "--no-fail-fast",
            "--",
            "--test-threads",
            &threads_str
        )
        .env("RUST_TEST_THREADS", &threads_str)
        .env("LC_ALL", "C")
        .stderr_to_stdout()
        .stdout_capture()
        .unchecked(),
    )?;

    let producers = vec![workspace_receipt, historical_receipt, upstream_receipt];
    let output = format!("{}\n{}\n{}", workspace_output, historical_output, upstream_output);

    // Write raw output
    fs::write(&test_output_path, output.as_bytes()).with_context(|| {
        format!("Failed to write test output to {}", test_output_path.display())
    })?;

    let elapsed = start.elapsed();
    println!(
        "Tests completed in {:.1}s (exit codes: workspace={}, comparison-historical={}, comparison-current-upstream={})",
        elapsed.as_secs_f64(),
        producers[0].exit_code.unwrap_or(-1),
        producers[1].exit_code.unwrap_or(-1),
        producers[2].exit_code.unwrap_or(-1)
    );

    // Parse test output
    let summary = parse_test_output(&output);
    let status = test_domain_status(
        &producers,
        &[&workspace_output, &historical_output, &upstream_output],
        summary.failed,
    );

    let typed = TestSummary {
        passed: summary.passed,
        failed: summary.failed,
        ignored: summary.ignored,
        active_tests: summary.active_tests,
        total_all_tests: summary.total_all_tests,
        pass_rate_active: summary.pass_rate_active,
        pass_rate_total: summary.pass_rate_total,
        status,
        producers,
        raw_output_digest: digest_hex(output.as_bytes()),
    };

    // Write test summary
    let summary_json =
        serde_json::to_string_pretty(&typed).context("Failed to serialize test summary")?;
    fs::write(&test_summary_path, &summary_json).with_context(|| {
        format!("Failed to write test summary to {}", test_summary_path.display())
    })?;
    println!("Test summary saved to {}", test_summary_path.display());

    Ok(typed)
}

/// Parse cargo test output to extract aggregate test counts
///
/// Looks for lines matching: `test result: ok. N passed; N failed; N ignored; ...`
/// and sums across all crate test runs.
fn parse_test_output(output: &str) -> TestSummary {
    let mut total_passed: u64 = 0;
    let mut total_failed: u64 = 0;
    let mut total_ignored: u64 = 0;

    for line in output.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("test result:") {
            continue;
        }

        // Parse "test result: ok. 272 passed; 0 failed; 818 ignored; 0 measured; 0 filtered out"
        // The numbers appear as: N passed, N failed, N ignored
        if let Some(passed) = extract_count_before(trimmed, "passed") {
            total_passed += passed;
        }
        if let Some(failed) = extract_count_before(trimmed, "failed") {
            total_failed += failed;
        }
        if let Some(ignored) = extract_count_before(trimmed, "ignored") {
            total_ignored += ignored;
        }
    }

    let active_tests = total_passed + total_failed;
    let total_all_tests = active_tests + total_ignored;

    let pass_rate_active =
        if active_tests > 0 { (total_passed as f64 / active_tests as f64) * 100.0 } else { 0.0 };

    let pass_rate_total = if total_all_tests > 0 {
        (total_passed as f64 / total_all_tests as f64) * 100.0
    } else {
        0.0
    };

    // Round to 1 decimal place
    let pass_rate_active = (pass_rate_active * 10.0).round() / 10.0;
    let pass_rate_total = (pass_rate_total * 10.0).round() / 10.0;

    // Status is decided by `test_domain_status` from checked producer
    // outcomes; parsing alone is diagnostics only (#15350).
    TestSummary {
        passed: total_passed,
        failed: total_failed,
        ignored: total_ignored,
        active_tests,
        total_all_tests,
        pass_rate_active,
        pass_rate_total,
        status: DomainStatus::CompletePass,
        producers: Vec::new(),
        raw_output_digest: String::new(),
    }
}

/// Extract the number immediately before a keyword in a test result line
///
/// For input "272 passed; 0 failed; 818 ignored" and keyword "passed",
/// returns Some(272).
fn extract_count_before(line: &str, keyword: &str) -> Option<u64> {
    // Find the keyword position
    let keyword_pos = line.find(keyword)?;
    let before = &line[..keyword_pos];

    // The number is the last whitespace-delimited token before the keyword
    before.split_whitespace().last()?.parse().ok()
}

// =============================================================================
// Doc Receipt Generation
// =============================================================================

/// Run cargo doc and count missing documentation warnings.
///
/// A rustdoc failure is recorded as `instrument_failed` (#15350): zero
/// missing-doc warnings from a failed build is instrument failure, not zero debt.
fn generate_doc_receipts(artifacts_dir: &Path) -> Result<DocSummary> {
    let rustdoc_log_path = artifacts_dir.join("rustdoc.log");
    let doc_summary_path = artifacts_dir.join("doc-summary.json");

    // Run cargo doc, capturing stderr (where warnings go)
    let result = cmd!("cargo", "+stable", "doc", "--no-deps", "--workspace", "--exclude", "xtask")
        .stdout_capture()
        .stderr_capture()
        .unchecked()
        .run()
        .context("Failed to execute cargo doc")?;

    let stderr = String::from_utf8_lossy(&result.stderr);
    let status = doc_domain_status(result.status.success());
    if status == DomainStatus::InstrumentFailed {
        println!(
            "WARNING: cargo doc exited with failure (exit code {:?}); missing-docs counts are \
             partial diagnostics, not zero documentation debt (#15350)",
            result.status.code()
        );
    }

    // Write rustdoc log
    fs::write(&rustdoc_log_path, stderr.as_bytes()).with_context(|| {
        format!("Failed to write rustdoc log to {}", rustdoc_log_path.display())
    })?;

    // Count "warning: missing documentation" lines
    let missing_docs =
        stderr.lines().filter(|line| line.starts_with("warning: missing documentation")).count()
            as u64;

    let summary =
        DocSummary { missing_docs, status, raw_output_digest: digest_hex(stderr.as_bytes()) };

    // Write doc summary
    let summary_json =
        serde_json::to_string_pretty(&summary).context("Failed to serialize doc summary")?;
    fs::write(&doc_summary_path, &summary_json).with_context(|| {
        format!("Failed to write doc summary to {}", doc_summary_path.display())
    })?;
    println!("Doc summary saved to {}", doc_summary_path.display());

    Ok(summary)
}

// =============================================================================
// Consolidated State
// =============================================================================

/// Build the consolidated typed state for one run
fn generate_consolidated_state(
    run_id: String,
    subject: SubjectIdentity,
    tests: TestSummary,
    docs: DocSummary,
) -> Result<ConsolidatedState> {
    let version = extract_version()?;
    let generated_at = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    Ok(ConsolidatedState { version, run_id, subject, tests, docs, generated_at })
}

/// Extract perl-parser version from cargo metadata
fn extract_version() -> Result<String> {
    let metadata_output = cmd!("cargo", "metadata", "-q", "--format-version=1")
        .stdout_capture()
        .run()
        .context("Failed to run cargo metadata")?;

    let metadata_str = String::from_utf8_lossy(&metadata_output.stdout);

    // Parse JSON to find perl-parser package version
    let metadata: serde_json::Value =
        serde_json::from_str(&metadata_str).context("Failed to parse cargo metadata JSON")?;

    let packages = metadata
        .get("packages")
        .and_then(|p| p.as_array())
        .ok_or_else(|| color_eyre::eyre::eyre!("No 'packages' array in cargo metadata"))?;

    for package in packages {
        let name = package.get("name").and_then(|n| n.as_str()).unwrap_or("");
        if name == "perl-parser" {
            let version = package.get("version").and_then(|v| v.as_str()).unwrap_or("unknown");
            return Ok(version.to_string());
        }
    }

    Ok("unknown".to_string())
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn producer(
        name: &'static str,
        exit_code: Option<i32>,
        result_lines: usize,
    ) -> ProducerReceipt {
        ProducerReceipt {
            name,
            exit_code,
            success: exit_code == Some(0),
            result_lines,
            output_digest: String::new(),
        }
    }

    const FULL_RESULT_LINE: &str =
        "test result: ok. 10 passed; 2 failed; 3 ignored; 0 measured; 0 filtered out";

    #[test]
    fn parse_test_output_extracts_counts_from_single_crate() {
        let output = "test result: ok. 10 passed; 2 failed; 3 ignored; 0 measured; 0 filtered out";
        let summary = parse_test_output(output);
        assert_eq!(summary.passed, 10);
        assert_eq!(summary.failed, 2);
        assert_eq!(summary.ignored, 3);
        assert_eq!(summary.active_tests, 12);
        assert_eq!(summary.total_all_tests, 15);
    }

    #[test]
    fn parse_test_output_aggregates_multiple_crates() {
        let output = "\
running 5 tests
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

running 10 tests
test result: ok. 8 passed; 1 failed; 1 ignored; 0 measured; 0 filtered out
";
        let summary = parse_test_output(output);
        assert_eq!(summary.passed, 13);
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.ignored, 1);
        assert_eq!(summary.active_tests, 14);
        assert_eq!(summary.total_all_tests, 15);
    }

    #[test]
    fn parse_test_output_handles_empty_output() {
        let summary = parse_test_output("");
        assert_eq!(summary.passed, 0);
        assert_eq!(summary.failed, 0);
        assert_eq!(summary.ignored, 0);
        assert_eq!(summary.pass_rate_active, 0.0);
        assert_eq!(summary.pass_rate_total, 0.0);
    }

    #[test]
    fn parse_test_output_handles_no_test_results() {
        let output = "Compiling foo v0.1.0\nFinished test\n";
        let summary = parse_test_output(output);
        assert_eq!(summary.passed, 0);
        assert_eq!(summary.total_all_tests, 0);
    }

    #[test]
    fn extract_count_before_finds_passed() {
        let line = "test result: ok. 272 passed; 0 failed; 818 ignored; 0 measured; 0 filtered out";
        assert_eq!(extract_count_before(line, "passed"), Some(272));
        assert_eq!(extract_count_before(line, "failed"), Some(0));
        assert_eq!(extract_count_before(line, "ignored"), Some(818));
    }

    #[test]
    fn extract_count_before_returns_none_for_missing_keyword() {
        let line = "test result: ok. 5 passed; 0 failed";
        assert_eq!(extract_count_before(line, "ignored"), None);
    }

    #[test]
    fn pass_rate_calculations_are_correct() {
        let output = "test result: ok. 90 passed; 10 failed; 0 ignored; 0 measured; 0 filtered out";
        let summary = parse_test_output(output);
        assert!((summary.pass_rate_active - 90.0).abs() < 0.01);
        assert!((summary.pass_rate_total - 90.0).abs() < 0.01);
    }

    #[test]
    fn pass_rate_with_ignored_tests() {
        let output = "test result: ok. 80 passed; 0 failed; 20 ignored; 0 measured; 0 filtered out";
        let summary = parse_test_output(output);
        assert!((summary.pass_rate_active - 100.0).abs() < 0.01);
        assert!((summary.pass_rate_total - 80.0).abs() < 0.01);
    }

    // =========================================================================
    // Typed completeness decisions (#15350)
    // =========================================================================

    #[test]
    fn all_producers_success_without_failures_is_complete_pass() {
        let producers = vec![
            producer("workspace", Some(0), 3),
            producer("comparison-historical", Some(0), 1),
            producer("comparison-current-upstream", Some(0), 1),
        ];
        let outputs: Vec<&str> = vec![FULL_RESULT_LINE, FULL_RESULT_LINE, FULL_RESULT_LINE];
        assert_eq!(test_domain_status(&producers, &outputs, 0), DomainStatus::CompletePass);
    }

    #[test]
    fn all_producers_success_with_test_failures_is_complete_with_failures() {
        let producers = vec![producer("workspace", Some(0), 1)];
        let outputs = vec![FULL_RESULT_LINE];
        assert_eq!(test_domain_status(&producers, &outputs, 2), DomainStatus::CompleteWithFailures);
    }

    #[test]
    fn compile_failure_after_successful_crate_is_instrument_failed() {
        // One crate's tests ran (result lines exist), then the build broke:
        // the aggregate population is incomplete, so counts are not evidence.
        let producers = vec![
            producer("workspace", Some(101), 2),
            producer("comparison-historical", Some(0), 1),
            producer("comparison-current-upstream", Some(0), 1),
        ];
        let workspace_output = "\
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
error: could not compile `perl-parser` (lib test) due to 1 previous error";
        let outputs = vec![workspace_output, FULL_RESULT_LINE, FULL_RESULT_LINE];
        assert_eq!(test_domain_status(&producers, &outputs, 0), DomainStatus::InstrumentFailed);
    }

    #[test]
    fn failing_comparison_invocation_with_compile_error_is_instrument_failed() {
        let producers = vec![
            producer("workspace", Some(0), 3),
            producer("comparison-historical", Some(101), 0),
            producer("comparison-current-upstream", Some(0), 1),
        ];
        let outputs = vec![FULL_RESULT_LINE, "error[E0432]: unresolved import", FULL_RESULT_LINE];
        assert_eq!(test_domain_status(&producers, &outputs, 0), DomainStatus::InstrumentFailed);
    }

    #[test]
    fn killed_producer_is_instrument_failed() {
        // Signal-terminated producer: exit code is None.
        let producers = vec![
            producer("workspace", None, 1),
            producer("comparison-historical", Some(0), 1),
            producer("comparison-current-upstream", Some(0), 1),
        ];
        let outputs = vec![FULL_RESULT_LINE, FULL_RESULT_LINE, FULL_RESULT_LINE];
        assert_eq!(test_domain_status(&producers, &outputs, 0), DomainStatus::InstrumentFailed);
    }

    #[test]
    fn failed_producer_with_no_parseable_results_is_instrument_failed() {
        // Empty output must be incomplete: "no usable evidence" is not a
        // proven-empty population (#15350).
        let producers = vec![producer("workspace", Some(1), 0)];
        let outputs = vec![""];
        assert_eq!(test_domain_status(&producers, &outputs, 0), DomainStatus::InstrumentFailed);
    }

    #[test]
    fn nonzero_exit_with_results_and_no_compile_markers_is_complete_with_failures() {
        // cargo exits non-zero when tests fail; with full result lines and no
        // compile markers the population still ran to completion.
        let output = "\
test result: ok. 5 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out
error: test failed, to rerun pass `-p perl-parser`";
        let producers = vec![producer("workspace", Some(101), 1)];
        let outputs = vec![output];
        assert_eq!(test_domain_status(&producers, &outputs, 1), DomainStatus::CompleteWithFailures);
    }

    #[test]
    fn doc_failure_is_instrument_failed_not_zero_debt() {
        assert_eq!(doc_domain_status(false), DomainStatus::InstrumentFailed);
        assert_eq!(doc_domain_status(true), DomainStatus::CompletePass);
    }

    #[test]
    fn not_run_by_mode_summaries_are_explicitly_not_run() {
        let tests = TestSummary::not_run_by_mode();
        assert_eq!(tests.status, DomainStatus::NotRunByMode);
        let docs = DocSummary::not_run_by_mode();
        assert_eq!(docs.status, DomainStatus::NotRunByMode);

        let tests_json = serde_json::to_string(&tests).unwrap();
        let docs_json = serde_json::to_string(&docs).unwrap();
        assert!(tests_json.contains("not_run_by_mode"), "{tests_json}");
        assert!(docs_json.contains("not_run_by_mode"), "{docs_json}");
        assert!(!tests_json.contains("complete"), "{tests_json}");
        assert!(!docs_json.contains("complete"), "{docs_json}");
    }

    #[test]
    fn only_complete_statuses_back_a_bundle() {
        assert!(DomainStatus::CompletePass.is_complete());
        assert!(DomainStatus::CompleteWithFailures.is_complete());
        assert!(!DomainStatus::NotRunByMode.is_complete());
        assert!(!DomainStatus::InstrumentFailed.is_complete());
    }

    #[test]
    fn digest_hex_is_stable_and_hex_encoded() {
        // SHA-256 of the empty string.
        assert_eq!(
            digest_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(digest_hex(b"abc").len(), 64);
    }
}
