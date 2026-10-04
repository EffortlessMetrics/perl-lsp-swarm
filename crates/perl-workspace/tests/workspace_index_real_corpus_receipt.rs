//! Cold scan+index receipt for the real-project corpus — benchmark slice #5
//! `workspace_index_real_corpus` (issue #17159, benchmark matrix row 5).
//!
//! Like every numeric lane, this receipt is `#[ignore]`d; run it explicitly
//! (use `--release`: the provisional <= 1 s bound is release-calibrated and
//! only enforced there):
//!
//! ```text
//! cargo test --release -p perl-workspace --test workspace_index_real_corpus_receipt -- --ignored
//! ```
//!
//! Per project: 5 COLD samples (fresh `TempDir` corpus copy + fresh
//! `WorkspaceIndex` per sample, NO warmup discard — cold IS the metric), then
//! `.ci/metrics/workspace_index_real_corpus.json` records p50/p95 cold
//! scan+index wallclock, phase splits, and the median files/sec. The
//! provisional acceptance bound is <= 1 s cold index per skeleton; the
//! -/+20% alert policy applies only after this baseline is calibrated.
//!
//! WARM/re-index behavior is intentionally out of scope here: the existing
//! "incremental update single file" and "early exit content hash check"
//! criterion groups already cover it.
//!
//! The always-on (non-ignored) tests below pin the shared sampler's
//! deterministic arithmetic and staging so a broken sampler fails fast in
//! ordinary `cargo test` runs too.

// The receipt prints diagnostic output for CI troubleshooting; this is not
// the LSP server's stdio transport, so print_stdout/print_stderr don't apply
// the way they do to production code (real_project_latency.rs precedent).
#![allow(clippy::print_stdout, clippy::print_stderr)]

#[path = "../benches/support/index_real_corpus.rs"]
mod index_real_corpus;

use index_real_corpus::{
    COLD_SAMPLES, ColdScanIndexSample, PROVISIONAL_COLD_INDEX_LIMIT_MS, REAL_PROJECTS_RELATIVE,
    RECEIPT_RELATIVE_PATH, SYNTHETIC_MODULES_PER_PACKAGE, SYNTHETIC_PACKAGES, percentile,
    stage_copy, workspace_root, write_synthetic_tree,
};
use serde_json::json;
use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tempfile::TempDir;

/// Per-project receipt: summaries over [`COLD_SAMPLES`] cold runs.
#[derive(Debug)]
struct ProjectReceipt {
    name: String,
    files_discovered: usize,
    files_indexed: usize,
    index_errors: usize,
    symbols: usize,
    discovery_method: &'static str,
    p50_cold_scan_index: Duration,
    p95_cold_scan_index: Duration,
    p50_scan_discovery: Duration,
    p50_index_files: Duration,
    files_per_sec_median: f64,
    synthetic: bool,
}

/// One receipt lane run: per project, [`COLD_SAMPLES`] cold samples, no warmup.
fn sample_project(
    name: &str,
    source_root: &Path,
    synthetic: bool,
) -> Result<ProjectReceipt, String> {
    let mut cold = Vec::with_capacity(COLD_SAMPLES);
    let mut discovery = Vec::with_capacity(COLD_SAMPLES);
    let mut index_files = Vec::with_capacity(COLD_SAMPLES);
    let mut throughput = Vec::with_capacity(COLD_SAMPLES);
    let mut files_discovered = 0;
    let mut files_indexed = 0;
    let mut index_errors = 0;
    let mut symbols = 0;
    let mut discovery_method = "";

    for run in 0..COLD_SAMPLES {
        let temp = TempDir::new().map_err(|error| format!("run {run} tempdir: {error}"))?;
        let staged = stage_copy(source_root, temp.path())?;
        // Deliberately no warmup run: cold IS the metric (matrix row 5).
        let sample = index_real_corpus::cold_scan_index(temp.path())?;

        // Indexed predicate (per run): every admitted file's index_file
        // returned Ok AND file_count() == files_indexed AND symbol_count() > 0.
        assert!(
            sample.indexed_predicate_holds(),
            "run {run}: indexed predicate failed for {name}: {sample:?}"
        );
        assert!(sample.files_discovered > 0, "run {run}: discovery found no Perl files for {name}");
        assert!(
            sample.files_discovered <= staged,
            "run {run}: discovery saw {} files but only {staged} were staged for {name}",
            sample.files_discovered
        );

        // files/sec uses per-run admitted files (indexed Ok + admitted-but-
        // erroring) over the total cold wall time.
        let admitted = sample.files_admitted();
        assert!(admitted > 0, "run {run}: admission rejected every discovered file for {name}");

        files_discovered = files_discovered.max(sample.files_discovered);
        files_indexed = files_indexed.max(sample.files_indexed);
        index_errors += sample.index_errors;
        symbols = symbols.max(sample.symbols);
        discovery_method = sample.method;

        cold.push(sample.total);
        discovery.push(sample.discovery);
        index_files.push(sample.read_admit_index);
        throughput.push(admitted as f64 / sample.total.as_secs_f64());
    }

    cold.sort_unstable();
    discovery.sort_unstable();
    index_files.sort_unstable();
    throughput.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    Ok(ProjectReceipt {
        name: name.to_string(),
        files_discovered,
        files_indexed,
        index_errors,
        symbols,
        discovery_method,
        p50_cold_scan_index: index_real_corpus::median(&cold),
        p95_cold_scan_index: percentile(&cold, 95),
        p50_scan_discovery: index_real_corpus::median(&discovery),
        p50_index_files: index_real_corpus::median(&index_files),
        files_per_sec_median: throughput[COLD_SAMPLES / 2],
        synthetic,
    })
}

/// Short commit SHA for provenance (best-effort, "unknown" outside a repo).
fn git_short_sha() -> String {
    std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

fn now_epoch_seconds() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |duration| duration.as_secs())
}

/// Write the receipt JSON (creates parent directories; a failed write fails
/// the receipt — the artifact is the deliverable).
fn write_receipt(receipts: &[ProjectReceipt], output_path: &Path) -> Result<(), String> {
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("create_dir_all {}: {error}", parent.display()))?;
    }

    let mut projects = serde_json::Map::new();
    for receipt in receipts {
        projects.insert(
            receipt.name.clone(),
            json!({
                "files_discovered": receipt.files_discovered,
                "files_indexed": receipt.files_indexed,
                "index_errors_total": receipt.index_errors,
                "symbols": receipt.symbols,
                "discovery_method": receipt.discovery_method,
                "cold_scan_index": {
                    "p50_ms": receipt.p50_cold_scan_index.as_millis() as f64,
                    "p95_ms": receipt.p95_cold_scan_index.as_millis() as f64,
                    "samples": COLD_SAMPLES,
                    "unit": "ms"
                },
                "scan_discovery": {
                    "p50_ms": receipt.p50_scan_discovery.as_millis() as f64,
                    "samples": COLD_SAMPLES,
                    "unit": "ms"
                },
                "index_files": {
                    "p50_ms": receipt.p50_index_files.as_millis() as f64,
                    "samples": COLD_SAMPLES,
                    "unit": "ms"
                },
                "files_per_sec": {
                    "median": receipt.files_per_sec_median,
                    "samples": COLD_SAMPLES
                },
                "synthetic": receipt.synthetic
            }),
        );
    }

    // Baseline lifecycle (matrix row 5): this run records the baseline; the
    // -/+20% alert policy activates only after the baseline is calibrated.
    let output = json!({
        "schema_version": 1,
        "lane": "workspace_index_real_corpus",
        "issue": 17159,
        "measured_at_epoch_s": now_epoch_seconds(),
        "commit": git_short_sha(),
        "samples_per_project": COLD_SAMPLES,
        "warmup_discarded_runs": 0,
        "cold_definition": "fresh TempDir corpus copy (no .git) + fresh WorkspaceIndex per sample; discovery then per-file read/admit/decode/index_file in discovery's lexical order; OS page cache may stay warm across samples (documented limitation; p50 damps it)",
        "projects": projects,
        "provisional_threshold": {
            "cold_scan_index_p50_ms": PROVISIONAL_COLD_INDEX_LIMIT_MS,
            "scope": "per real-corpus skeleton"
        },
        "alert_policy": {
            "tolerance_pct": 20,
            "state": "pending_baseline_calibration"
        }
    });

    let rendered = serde_json::to_string_pretty(&output)
        .map_err(|error| format!("serialise receipt: {error}"))?;
    fs::write(output_path, rendered + "\n")
        .map_err(|error| format!("write {}: {error}", output_path.display()))?;
    Ok(())
}

#[test]
#[ignore = "numeric lane (issue #17159 row 5): run via `cargo test -p perl-workspace --test workspace_index_real_corpus_receipt -- --ignored`"]
fn cold_scan_index_receipt_real_corpus() -> Result<(), String> {
    let root = workspace_root();
    let real_projects = index_real_corpus::real_corpus_projects();

    // Synthetic source tree, staged once outside the timed samples.
    let synthetic_source = TempDir::new().map_err(|error| format!("tempdir: {error}"))?;
    let synthetic_files = write_synthetic_tree(synthetic_source.path())?;
    assert_eq!(
        synthetic_files,
        (SYNTHETIC_PACKAGES * SYNTHETIC_MODULES_PER_PACKAGE) as usize,
        "synthetic tree must hold 40x10 = 400 files"
    );

    let mut receipts = Vec::new();
    if real_projects.is_empty() {
        eprintln!("receipt: {REAL_PROJECTS_RELATIVE} absent; recording the synthetic entry only");
    }
    for project in &real_projects {
        let name = project
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| format!("non-utf8 project directory: {}", project.display()))?;
        receipts.push(sample_project(name, project, false)?);
    }
    receipts.push(sample_project("synthetic_400_files", synthetic_source.path(), true)?);

    for receipt in &receipts {
        println!(
            "{:>22}: p50 {:>5} ms (p95 {:>5} ms), discovery p50 {:>4} ms, index p50 {:>4} ms, {} files, {} sym, {:>9.0} files/sec, method={}",
            receipt.name,
            receipt.p50_cold_scan_index.as_millis(),
            receipt.p95_cold_scan_index.as_millis(),
            receipt.p50_scan_discovery.as_millis(),
            receipt.p50_index_files.as_millis(),
            receipt.files_indexed,
            receipt.symbols,
            receipt.files_per_sec_median,
            receipt.discovery_method,
        );
    }

    // Provisional acceptance: <= 1 s cold scan+index per real skeleton (the
    // synthetic 400-file tree is not a skeleton and is exempt). The bound is
    // only enforced in release builds — dev-profile indexing is an order of
    // magnitude slower, so a debug run records numbers but does not enforce.
    let enforce = !cfg!(debug_assertions);
    if !enforce {
        eprintln!(
            "receipt: debug build; recording only (provisional {PROVISIONAL_COLD_INDEX_LIMIT_MS} ms threshold is enforced with --release)"
        );
    }
    for receipt in &receipts {
        if receipt.synthetic {
            continue;
        }
        assert!(
            !enforce || receipt.p50_cold_scan_index.as_millis() <= PROVISIONAL_COLD_INDEX_LIMIT_MS,
            "provisional threshold exceeded for {}: p50 cold scan+index {} ms > {PROVISIONAL_COLD_INDEX_LIMIT_MS} ms",
            receipt.name,
            receipt.p50_cold_scan_index.as_millis()
        );
    }

    let output_path = root.join(RECEIPT_RELATIVE_PATH);
    write_receipt(&receipts, &output_path)?;
    eprintln!("receipt written to {}", output_path.display());
    Ok(())
}

// ---- Always-on sampler proof (runs in ordinary `cargo test`) ----------------

#[test]
fn percentile_uses_nearest_rank_scorecard_arithmetic() {
    let micros = |value: u64| Duration::from_micros(value);

    // Deterministic 20-sample sequence: p95 must be index 18 (not the max),
    // exactly like the perf_scorecard.rs arithmetic this module mirrors.
    let mut samples: Vec<Duration> = (0..20).map(micros).collect();
    samples.sort_unstable();
    assert_eq!(percentile(&samples, 95), micros(18));
    assert_eq!(index_real_corpus::median(&samples), micros(10));

    // N=5 (the receipt lane): p95 is the max and p50 the middle sample;
    // the receipt records `samples: 5` so readers know p95 == max here.
    let five: Vec<Duration> = (10..15).map(micros).collect();
    assert_eq!(percentile(&five, 95), micros(14));
    assert_eq!(index_real_corpus::median(&five), micros(12));

    // Degenerate inputs stay panic-free.
    assert_eq!(percentile(&[], 95), Duration::ZERO);
    assert_eq!(index_real_corpus::median(&[]), Duration::ZERO);
}

#[test]
fn indexed_predicate_requires_clean_index_with_symbols() {
    let clean = ColdScanIndexSample {
        total: Duration::from_millis(5),
        discovery: Duration::from_millis(1),
        read_admit_index: Duration::from_millis(4),
        files_discovered: 3,
        files_in_index: 3,
        files_indexed: 3,
        index_errors: 0,
        symbols: 30,
        method: "walk",
    };
    assert!(clean.indexed_predicate_holds());
    assert_eq!(clean.files_admitted(), 3);

    let errored = ColdScanIndexSample { index_errors: 1, ..clean.clone() };
    assert!(
        !errored.indexed_predicate_holds(),
        "admitted-but-erroring files must fail the predicate"
    );
    assert_eq!(errored.files_admitted(), 4, "errors still count as admitted");

    let no_symbols = ColdScanIndexSample { symbols: 0, ..clean.clone() };
    assert!(!no_symbols.indexed_predicate_holds());

    let lost_file = ColdScanIndexSample { files_in_index: 2, ..clean };
    assert!(!lost_file.indexed_predicate_holds(), "file_count must equal files_indexed");
}

#[test]
fn discovery_method_labels_are_stable() {
    assert_eq!(
        index_real_corpus::discovery_method_label(perl_workspace::discovery::DiscoveryMethod::Walk),
        "walk"
    );
    assert_eq!(
        index_real_corpus::discovery_method_label(perl_workspace::discovery::DiscoveryMethod::Git),
        "git"
    );
}

#[test]
fn stage_copy_copies_regular_files_with_content_deterministically() -> Result<(), String> {
    let source = TempDir::new().map_err(|error| format!("tempdir: {error}"))?;
    let destination = TempDir::new().map_err(|error| format!("tempdir: {error}"))?;

    // Nested layout with deliberately unordered names plus a non-Perl file:
    // staging copies everything; discovery decides what is Perl.
    let files = [
        ("lib/B/Second.pm", "package B::Second;\nsub second { }\n1;\n"),
        ("lib/A/First.pm", "package A::First;\nsub first { }\n1;\n"),
        ("script.pl", "#!/usr/bin/env perl\nprint 1;\n"),
        ("README", "not perl\n"),
    ];
    for (relative, contents) in files {
        let path = source.path().join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| format!("create_dir_all: {error}"))?;
        }
        fs::write(&path, contents).map_err(|error| format!("write {relative}: {error}"))?;
    }

    let copied = stage_copy(source.path(), destination.path())?;
    assert_eq!(copied, files.len(), "every regular file must be copied");

    for (relative, contents) in files {
        let staged = fs::read_to_string(destination.path().join(relative))
            .map_err(|error| format!("read staged {relative}: {error}"))?;
        assert_eq!(staged, contents, "staged {relative} must preserve content");
    }

    // An empty source stages zero files rather than failing.
    let empty_source = TempDir::new().map_err(|error| format!("tempdir: {error}"))?;
    let empty_destination = TempDir::new().map_err(|error| format!("tempdir: {error}"))?;
    assert_eq!(stage_copy(empty_source.path(), empty_destination.path())?, 0);
    Ok(())
}
