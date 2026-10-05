//! A1 receipt machine-readability bench (#17154).
//!
//! Agent-usability A1 requires the benchmark receipt to be machine-readable:
//! parse plus required keys at 100%. The receipt surface on main is the text
//! the `bench-receipt` recipe prints to stdout via the real
//! `cargo xtask bench-format --receipt` path (`xtask/src/tasks/benchmarks.rs`
//! shells out to `benchmarks/scripts/format-results.py` with
//! `benchmarks/results/latest.json` resolved against the child cwd). There is
//! no JSON receipt file or JSON schema; machine-readability here means the
//! stdout grammar carries every key a downstream agent needs in a stable,
//! strictly parseable line shape.
//!
//! Required keys (each asserted present with sane types/values):
//!
//! - `verdict` — the `STATUS:` line (`COMPLETE`, `INCOMPLETE (...)`, or
//!   `INVALID (...)`); parseable on success and failure exits alike.
//! - `run_id` — the `Run ID:` line (`bench-YYYYMMDD-HHMMSS` shape).
//! - `git_sha` — the `Git SHA:` line; must equal the staged fixture SHA
//!   (evidence ref).
//! - `version` — the `Version:` line; must equal the staged fixture version
//!   (evidence ref).
//! - `total_benchmarks` — the `Total benchmarks:` count; must equal the number
//!   of parsed timing rows (scores).
//! - `passed_targets` / `failed_targets` — the target score lines (scores);
//!   printed only when at least one target was scored, so absent (pinned
//!   `None`) on targetless receipts such as the vacuous run.
//! - `timings` — one `name duration [marker]` row per benchmark with a numeric
//!   `<float><unit>` duration (scores). Names are producer-verbatim and may
//!   contain spaces, so rows parse from the right.
//! - `failed_categories` — the `FAILED CATEGORIES:` section naming each failed
//!   category with its error (unresolved items); absent on a clean receipt.
//!
//! Harness (mirrors `ripr_e1_gap_precision_recall.rs`): each case stages a
//! minimal `benchmarks/results/latest.json` fixture into a hermetic temp dir
//! (no full bench run, no network), runs the real `xtask bench-format
//! --receipt` binary with cwd set to the stage, and parses stdout with a
//! strict line grammar. Three cases: a complete receipt (exit 0), a receipt
//! with a runner-failed category (exit nonzero, #17218), and a vacuous receipt
//! (exit nonzero, #3979).
//!
//! Mutants: 1 attempted, 1 kill — renaming the `STATUS:` verdict print to
//! `VERDICT:` in `format-results.py` fails all three cases at the `verdict`
//! extraction (`must_some: A1 required key ...`, exit 101), then the script
//! was restored byte-identical (`git status` clean for that path).
//!
//! NOT_PROVEN boundaries: `Run ID` wall-clock uniqueness (shape only),
//! `format-results.py` behavior on malformed JSON input (fixtures are valid by
//! construction), receipt stability across Python versions (CI pins the runner).

// The bench prints its score lines; the workspace-wide print denial is a
// production-code rule.
#![allow(clippy::print_stdout)]

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Output,
};

use anyhow::{Context, Result};
use assert_cmd::Command;
use perl_tdd_support::{must_some_with, must_with};
use serde_json::Value;

/// SHA the fixtures bind; the receipt must echo it back as the evidence ref.
const FIXTURE_SHA: &str = "a1bench0deadbeef1234567890abcdef12345678";
/// Version the fixtures bind; the receipt must echo it back.
const FIXTURE_VERSION: &str = "0.9.0";

/// Minimal simplified-format results (the shape `bench-extract` writes):
/// three benchmarks, one meeting its target, one missing it, and one spaced
/// name (`state transitions`, from the supported `run-benchmarks.sh`
/// producer), so the score lines discriminate instead of merely existing.
fn complete_fixture() -> Value {
    serde_json::json!({
        "version": FIXTURE_VERSION,
        "timestamp": "2026-10-05T07:00:00Z",
        "git_sha": FIXTURE_SHA,
        "git_dirty": false,
        "environment": {
            "os": "test",
            "rust_version": "test",
            "extracted_from": "criterion"
        },
        "results": {
            "parser": {
                "_category": "parser",
                "parse_simple_script": {"mean_ns": 1500, "meets_target": true},
                "parse_large_file": {"mean_ns": 2_500_000, "meets_target": false},
                "state transitions": {"mean_ns": 1_100_000, "meets_target": true}
            }
        }
    })
}

/// One passing benchmark plus a runner-failed category carrying the `_status`
/// marker the `#17218` verdict path keys on.
fn failed_category_fixture() -> Value {
    serde_json::json!({
        "version": FIXTURE_VERSION,
        "timestamp": "2026-10-05T07:00:00Z",
        "git_sha": FIXTURE_SHA,
        "git_dirty": false,
        "environment": {
            "os": "test",
            "rust_version": "test",
            "extracted_from": "criterion"
        },
        "results": {
            "parser": {
                "_category": "parser",
                "parse_simple_script": {"mean_ns": 1500, "meets_target": true}
            },
            "lexer": {"_status": "failed", "_error": "runner crashed: exit 3"}
        }
    })
}

/// No categories at all: the fail-closed `#3979` verdict path.
fn vacuous_fixture() -> Value {
    serde_json::json!({
        "version": FIXTURE_VERSION,
        "timestamp": "2026-10-05T07:00:00Z",
        "git_sha": FIXTURE_SHA,
        "git_dirty": false,
        "environment": {
            "os": "test",
            "rust_version": "test",
            "extracted_from": "criterion"
        },
        "results": {}
    })
}

/// Stage one fixture as `benchmarks/results/latest.json` under a hermetic temp
/// dir. The temp dir is returned so the caller keeps it alive for the run.
fn stage_results(payload: &Value) -> Result<(tempfile::TempDir, PathBuf)> {
    let temp = tempfile::tempdir().context("creating A1 stage tempdir")?;
    let stage = temp.path().to_path_buf();
    let results_dir = stage.join("benchmarks").join("results");
    fs::create_dir_all(&results_dir)
        .with_context(|| format!("creating {}", results_dir.display()))?;
    let latest = results_dir.join("latest.json");
    fs::write(&latest, serde_json::to_string_pretty(payload)?)
        .with_context(|| format!("writing {}", latest.display()))?;
    Ok((temp, stage))
}

/// Run the real `xtask bench-format --receipt` with cwd set to the stage, so
/// the formatter reads the staged `benchmarks/results/latest.json` through
/// the production relative-path resolution.
fn run_bench_format(stage: &Path) -> Result<Output> {
    let output = Command::cargo_bin("xtask")?
        .current_dir(stage)
        // The stage is hermetic on disk but the child inherits our env, and
        // `run_python_script` passes it on to `python3`: a foreign PYTHONPATH
        // could shadow the stdlib modules the formatter imports.
        .env_remove("PYTHONPATH")
        .args(["bench-format", "--receipt"])
        .output()
        .context("A1 bench-format --receipt spawn failed")?;
    Ok(output)
}

/// Every machine-extractable key of one parsed receipt.
struct Receipt {
    verdict: String,
    run_id: String,
    git_sha: String,
    version: String,
    total: u32,
    passed: Option<u32>,
    failed: Option<u32>,
    timings: BTreeMap<String, String>,
    failed_categories: BTreeMap<String, String>,
}

/// First line starting with `prefix` (after leading whitespace: the SUMMARY
/// block is indented), with the prefix stripped and the value trimmed.
fn prefixed_line<'a>(stdout: &'a str, prefix: &str) -> Option<&'a str> {
    stdout.lines().find_map(|line| line.trim_start().strip_prefix(prefix)).map(str::trim)
}

/// Parse the `<name> <duration> [[OK|FAIL]]` rows of one `<CATEGORY>
/// BENCHMARKS:` section. Names come from the producer verbatim and may
/// contain spaces (`state transitions` in the `run-benchmarks.sh` parse
/// fixtures), so rows parse from the right: an optional trailing marker,
/// then a numeric `<float><unit>` duration, then the name. Durations must
/// carry a known unit; anything else is a readability hole, not a row.
fn parse_timing_row(line: &str) -> Option<(String, String)> {
    let mut tokens = line.split_whitespace().collect::<Vec<_>>();
    if matches!(tokens.last(), Some(marker) if *marker == "[OK]" || *marker == "[FAIL]") {
        tokens.pop();
    }
    let duration = tokens.pop()?;
    let magnitude = duration
        .strip_suffix("ns")
        .or_else(|| duration.strip_suffix("us"))
        .or_else(|| duration.strip_suffix("ms"))
        .or_else(|| duration.strip_suffix('s'))?;
    magnitude.parse::<f64>().ok()?;
    let name = tokens.join(" ");
    if name.is_empty() {
        return None;
    }
    Some((name, duration.to_string()))
}

/// Strict parse of the receipt stdout grammar. Every required key extracts
/// through a `must_*_with` assertion boundary naming the key, so a dropped or
/// reworded line fails the bench at the key, not downstream.
fn parse_receipt(stdout: &str) -> Receipt {
    let verdict = must_some_with(
        prefixed_line(stdout, "STATUS:"),
        "A1 required key `verdict` (STATUS: line) must exist",
    )
    .to_string();
    let run_id = must_some_with(
        prefixed_line(stdout, "Run ID:"),
        "A1 required key `run_id` (Run ID: line) must exist",
    )
    .to_string();
    let git_sha = must_some_with(
        prefixed_line(stdout, "Git SHA:"),
        "A1 required key `git_sha` (Git SHA: line) must exist",
    )
    .to_string();
    let version = must_some_with(
        prefixed_line(stdout, "Version:"),
        "A1 required key `version` (Version: line) must exist",
    )
    .to_string();
    let total_text = must_some_with(
        prefixed_line(stdout, "Total benchmarks:"),
        "A1 required key `total_benchmarks` (Total benchmarks: line) must exist",
    );
    let total = must_with(total_text.parse::<u32>(), "A1 `total_benchmarks` must be a u32 count");
    let passed = prefixed_line(stdout, "Passed targets:").map(str::parse::<u32>).transpose();
    let passed = must_with(passed, "A1 `passed_targets` must be a u32 count when printed");
    let failed = prefixed_line(stdout, "Failed targets:").map(str::parse::<u32>).transpose();
    let failed = must_with(failed, "A1 `failed_targets` must be a u32 count when printed");

    let mut timings = BTreeMap::new();
    let mut failed_categories = BTreeMap::new();
    let mut in_timings = false;
    let mut in_failures = false;
    for line in stdout.lines() {
        if line.ends_with(" BENCHMARKS:") {
            in_timings = true;
            in_failures = false;
            continue;
        }
        if line == "FAILED CATEGORIES:" {
            in_timings = false;
            in_failures = true;
            continue;
        }
        if line == "SUMMARY:" || line.is_empty() {
            in_timings = false;
            if line == "SUMMARY:" {
                in_failures = false;
            }
            continue;
        }
        // Blank lines and SUMMARY: are consumed above, so every line reaching
        // the timings branch is a nonempty row candidate: a readability hole
        // must fail the bench, never drop silently.
        if in_timings {
            let (name, duration) = must_some_with(
                parse_timing_row(line),
                "A1 timing row must match `<name> <float><unit> [marker]`",
            );
            timings.insert(name, duration);
        } else if in_failures && let Some((category, error)) = line.trim().split_once(':') {
            failed_categories.insert(category.trim().to_string(), error.trim().to_string());
        }
    }

    Receipt { verdict, run_id, git_sha, version, total, passed, failed, timings, failed_categories }
}

fn assert_run_id_shape(run_id: &str) {
    assert!(
        run_id.starts_with("bench-") && run_id.len() == "bench-YYYYMMDD-HHMMSS".len(),
        "A1 `run_id` must keep the bench-YYYYMMDD-HHMMSS shape, got `{run_id}`"
    );
    let stamp = &run_id["bench-".len()..];
    assert!(
        stamp.chars().all(|c| c.is_ascii_digit() || c == '-'),
        "A1 `run_id` stamp must be digits and dashes, got `{run_id}`"
    );
}

/// Complete receipt: exit 0, `COMPLETE` verdict, both score lines present with
/// discriminating values, all three timing rows parseable (including the
/// spaced name), no unresolved items.
#[test]
fn a1_complete_receipt_has_all_required_keys() -> Result<()> {
    let (_temp, stage) = stage_results(&complete_fixture())?;
    let output = run_bench_format(&stage)?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "A1 complete receipt must exit 0\n{stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let receipt = parse_receipt(&stdout);
    assert_eq!(receipt.verdict, "COMPLETE", "A1 `verdict` must be COMPLETE\n{stdout}");
    assert_run_id_shape(&receipt.run_id);
    assert_eq!(receipt.git_sha, FIXTURE_SHA, "A1 `git_sha` must echo the fixture SHA\n{stdout}");
    assert_eq!(receipt.version, FIXTURE_VERSION, "A1 `version` must echo the fixture\n{stdout}");
    assert_eq!(
        receipt.timings.len(),
        3,
        "A1 `timings` must hold all three fixture benchmarks\n{stdout}"
    );
    assert!(
        receipt.timings.contains_key("parse_simple_script")
            && receipt.timings.contains_key("parse_large_file")
            && receipt.timings.contains_key("state transitions"),
        "A1 `timings` must name the fixture benchmarks\n{stdout}"
    );
    assert_eq!(
        receipt.total,
        receipt.timings.len() as u32,
        "A1 `total_benchmarks` must equal the timing row count\n{stdout}"
    );
    assert_eq!(receipt.passed, Some(2), "A1 `passed_targets` must be 2\n{stdout}");
    assert_eq!(receipt.failed, Some(1), "A1 `failed_targets` must be 1\n{stdout}");
    assert!(
        receipt.failed_categories.is_empty(),
        "A1 complete receipt must list no unresolved categories\n{stdout}"
    );

    println!(
        "A1 complete | verdict={} total={} passed={:?} failed={:?} timings={} unresolved={} | MATCH",
        receipt.verdict,
        receipt.total,
        receipt.passed,
        receipt.failed,
        receipt.timings.len(),
        receipt.failed_categories.len()
    );
    Ok(())
}

/// Failed-category receipt: nonzero exit, `INCOMPLETE` verdict naming the
/// category, and the unresolved item still machine-extractable with its error.
#[test]
fn a1_failed_category_receipt_names_unresolved_items() -> Result<()> {
    let (_temp, stage) = stage_results(&failed_category_fixture())?;
    let output = run_bench_format(&stage)?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!output.status.success(), "A1 failed-category receipt must exit nonzero\n{stdout}");

    let receipt = parse_receipt(&stdout);
    assert!(
        receipt.verdict.starts_with("INCOMPLETE") && receipt.verdict.contains("lexer"),
        "A1 `verdict` must be INCOMPLETE naming lexer\n{stdout}"
    );
    assert_run_id_shape(&receipt.run_id);
    assert_eq!(receipt.git_sha, FIXTURE_SHA, "A1 `git_sha` must echo the fixture SHA\n{stdout}");
    assert_eq!(receipt.version, FIXTURE_VERSION, "A1 `version` must echo the fixture\n{stdout}");
    assert_eq!(receipt.total, 1, "A1 `total_benchmarks` must be 1\n{stdout}");
    assert_eq!(
        receipt.failed_categories.len(),
        1,
        "A1 must surface exactly one unresolved category\n{stdout}"
    );
    assert_eq!(
        receipt.failed_categories.get("lexer").map(String::as_str),
        Some("runner crashed: exit 3"),
        "A1 unresolved `lexer` must carry its error\n{stdout}"
    );

    println!(
        "A1 failed-category | verdict={} total={} unresolved={:?} | MATCH",
        receipt.verdict, receipt.total, receipt.failed_categories
    );
    Ok(())
}

/// Vacuous receipt: nonzero exit, but the fail-closed `INVALID` verdict and
/// the zero totals must still parse — a downstream agent must never face an
/// unparseable receipt, even when nothing ran.
#[test]
fn a1_vacuous_receipt_verdict_stays_machine_readable() -> Result<()> {
    let (_temp, stage) = stage_results(&vacuous_fixture())?;
    let output = run_bench_format(&stage)?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!output.status.success(), "A1 vacuous receipt must exit nonzero\n{stdout}");

    let receipt = parse_receipt(&stdout);
    assert!(
        receipt.verdict.starts_with("INVALID"),
        "A1 `verdict` must be INVALID on a vacuous run\n{stdout}"
    );
    assert_run_id_shape(&receipt.run_id);
    assert_eq!(receipt.git_sha, FIXTURE_SHA, "A1 `git_sha` must echo the fixture SHA\n{stdout}");
    assert_eq!(receipt.version, FIXTURE_VERSION, "A1 `version` must echo the fixture\n{stdout}");
    assert_eq!(receipt.total, 0, "A1 `total_benchmarks` must be 0\n{stdout}");
    assert!(receipt.timings.is_empty(), "A1 vacuous receipt must have no timings\n{stdout}");
    // `format-results.py` prints the target lines only when a target was
    // scored: pin the absence so a formatter change fails loudly here.
    assert_eq!(receipt.passed, None, "A1 vacuous receipt must omit `passed_targets`\n{stdout}");
    assert_eq!(receipt.failed, None, "A1 vacuous receipt must omit `failed_targets`\n{stdout}");

    println!(
        "A1 vacuous | verdict={} total={} timings={} | MATCH",
        receipt.verdict,
        receipt.total,
        receipt.timings.len()
    );
    Ok(())
}
