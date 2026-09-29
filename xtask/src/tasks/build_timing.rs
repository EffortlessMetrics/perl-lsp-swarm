//! Build timing collection and comparison automation.

use crate::utils::project_root;
use chrono::Utc;
use color_eyre::eyre::{Context, Result, eyre};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

const ARTIFACTS_DIR: &str = "artifacts";
const TIMING_RECEIPT_FILE: &str = "build-timing-receipt.json";
const TIMING_BASELINE_FILE: &str = "build-timing-baseline.json";
const LSP_PROVIDERS_LIB: &str = "crates/perl-lsp-providers/src/lib.rs";
const PARSER_LIB: &str = "crates/perl-parser/src/lib.rs";

/// Improvement values with magnitude below this threshold (in percentage
/// points) are treated as exact ties rather than improvements or regressions.
///
/// Build timing measurements are wall-clock durations recorded at `f64`
/// precision by `Instant::elapsed().as_secs_f64()`. Two measurements of the
/// same command typically differ by tens of microseconds, which after the
/// percentage transform `(base - curr) / base * 100.0` produces a residual
/// in the `1e-9 .. 1e-3` range. Branching on raw `> 0.0` / `< 0.0` against
/// such residuals classifies float-rounding noise as an improvement or
/// regression.
///
/// This constant is a named, single source of truth for the comparison
/// gate. Keep it aligned with `parser_corpus_sweep::ratchet::RATCHET_EPS`
/// (1e-9) for ratios, or with the corpus_audit epsilon pattern when the
/// metric is a percentage rather than a ratio.
const IMPROVEMENT_EPS: f64 = 1e-3;

/// Result of classifying a single per-row improvement value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ImprovementClass {
    /// Improvement or regression magnitude is at or below `IMPROVEMENT_EPS`;
    /// counts as neither.
    Tie,
    /// `improvement > IMPROVEMENT_EPS`.
    Improvement,
    /// `improvement < -IMPROVEMENT_EPS`.
    Regression,
}

/// Classify a per-row improvement percentage using an epsilon guard so that
/// exact-tie / float-rounding residuals are not recorded as verdicts.
///
/// Extracted from `run_compare` so the branch logic can be unit-tested
/// without constructing `BuildTimingReceipt` values on disk.
fn classify_improvement(improvement: f64) -> ImprovementClass {
    if improvement.abs() <= IMPROVEMENT_EPS {
        ImprovementClass::Tie
    } else if improvement > 0.0 {
        ImprovementClass::Improvement
    } else {
        ImprovementClass::Regression
    }
}

/// Schema version stamped by the producer and required (fail-closed) by
/// consumers — `run_compare` here and `metrics release-health` in
/// `tasks/metrics/release_health.rs` (#15357).
pub(crate) const SCHEMA_VERSION: u32 = 1;

#[derive(Serialize)]
struct BuildTimingReceipt {
    schema_version: u32,
    timestamp: String,
    toolchain: String,
    system: SystemInfo,
    measurements: BTreeMap<String, BuildMeasurement>,
}

#[derive(Serialize)]
struct SystemInfo {
    cpu_cores: Value,
    memory_gb: Value,
    os: String,
}

#[derive(Serialize, Deserialize)]
struct BuildMeasurement {
    duration_seconds: f64,
    command: String,
}

#[derive(Deserialize)]
struct MeasurableReceipt {
    /// No serde default: a receipt without the envelope field fails to parse
    /// (fail-closed) instead of being silently compared (#15357).
    schema_version: u32,
    timestamp: String,
    toolchain: String,
    measurements: BTreeMap<String, BuildMeasurement>,
}

pub fn run_receipt(
    run_clean: bool,
    run_incremental: bool,
    run_tests: bool,
    output: Option<PathBuf>,
    baseline: bool,
) -> Result<()> {
    let root = project_root()?;
    let mut run_clean = run_clean;
    let mut run_incremental = run_incremental;
    let mut run_tests = run_tests;

    if !run_clean && !run_incremental && !run_tests {
        run_clean = true;
        run_incremental = true;
        run_tests = true;
    }

    let artifacts = root.join(ARTIFACTS_DIR);
    fs::create_dir_all(&artifacts)
        .with_context(|| format!("Failed to create artifacts directory {}", artifacts.display()))?;

    let mut output_path = if let Some(output) = output {
        if output.is_absolute() { output } else { root.join(output) }
    } else {
        artifacts.join(TIMING_RECEIPT_FILE)
    };

    if baseline {
        output_path = artifacts.join(TIMING_BASELINE_FILE);
    }

    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create output directory {}", parent.display()))?;
    }

    let system = collect_system_info(&root);
    let mut measurements = BTreeMap::new();

    if run_clean {
        let measurement = measure_command(
            &root,
            "clean_build_workspace",
            &["cargo", "build", "--workspace", "--locked"],
            Some(&["cargo", "clean"]),
        );
        measurements.insert("clean_build_workspace".to_string(), measurement);
    }

    if run_incremental {
        let providers_dir = root.join("crates/perl-lsp-providers");
        run_command_silently(&root, &["cargo", "build", "--workspace", "--locked"]);
        if providers_dir.exists() {
            touch_file(&root.join(LSP_PROVIDERS_LIB))?;
            let measurement = measure_command(
                &root,
                "incremental_build_providers",
                &["cargo", "build", "-p", "perl-lsp-providers", "--locked"],
                None,
            );
            measurements.insert("incremental_build_providers".to_string(), measurement);
        } else {
            touch_file(&root.join(PARSER_LIB))?;
            let measurement = measure_command(
                &root,
                "incremental_build_parser",
                &["cargo", "build", "-p", "perl-parser", "--locked"],
                None,
            );
            measurements.insert("incremental_build_parser".to_string(), measurement);
        }
    }

    if run_tests {
        let measurement = measure_command(
            &root,
            "test_build_workspace",
            &["cargo", "test", "--workspace", "--lib", "--locked"],
            None,
        );
        measurements.insert("test_build_workspace".to_string(), measurement);
    }

    let receipt = BuildTimingReceipt {
        schema_version: SCHEMA_VERSION,
        timestamp: Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        toolchain: command_output_or_unknown(&["rustc", "--version"]),
        system,
        measurements,
    };

    let payload = serde_json::to_string_pretty(&receipt)
        .context("Failed to serialize build timing receipt")?;
    fs::write(&output_path, format!("{payload}\n")).context("Failed to write timing receipt")?;

    println!("=== Build Timing Receipt Generated ===");
    println!("Output: {}", output_path.display());
    println!();
    println!("{payload}");

    if baseline {
        println!();
        println!("Baseline saved to: {}", output_path.display());
        println!("Use this baseline to compare against future measurements:");
        println!(
            "  cargo xtask compare-build-timing {} <new-measurement.json>",
            output_path.display()
        );
    }

    Ok(())
}

pub fn run_compare(baseline: PathBuf, current: PathBuf) -> Result<()> {
    let baseline_path = resolve_path(&baseline)?;
    let current_path = resolve_path(&current)?;

    let baseline_raw = fs::read_to_string(&baseline_path)
        .with_context(|| format!("Failed to read {}", baseline_path.display()))?;
    let current_raw = fs::read_to_string(&current_path)
        .with_context(|| format!("Failed to read {}", current_path.display()))?;

    let baseline: MeasurableReceipt =
        serde_json::from_str(&baseline_raw).context("Failed to parse baseline receipt")?;
    let current: MeasurableReceipt =
        serde_json::from_str(&current_raw).context("Failed to parse current receipt")?;

    // Fail closed on schema drift: a receipt written by a different producer
    // generation must not be silently compared (#15357).
    for (label, path, receipt) in
        [("baseline", &baseline_path, &baseline), ("current", &current_path, &current)]
    {
        if receipt.schema_version != SCHEMA_VERSION {
            let found = receipt.schema_version;
            return Err(eyre!(
                "Build timing {label} receipt schema version mismatch at {}: \
                 expected {SCHEMA_VERSION}, got {found}",
                path.display()
            ));
        }
    }

    println!("# Build Timing Comparison");
    println!();
    println!("**Generated:** {}", Utc::now().format("%Y-%m-%dT%H:%M:%SZ"));
    println!();
    println!("## Metadata");
    println!("| Property | Baseline | Current |");
    println!("|----------|----------|---------|");
    println!("| Timestamp | {} | {} |", baseline.timestamp, current.timestamp);
    println!("| Toolchain | {} | {} |", baseline.toolchain, current.toolchain);
    println!();
    println!("## Build Timing Results");
    println!();
    println!("| Metric | Baseline | Current | Change | Improvement |");
    println!("|--------|----------|---------|---------|-------------|");

    let mut all_keys = BTreeSet::new();
    all_keys.extend(baseline.measurements.keys().cloned());
    all_keys.extend(current.measurements.keys().cloned());

    let mut total_metrics = 0usize;
    let mut improvements = 0usize;
    let mut regressions = 0usize;

    for key in all_keys {
        total_metrics += 1;
        let baseline_measurement = baseline.measurements.get(&key);
        let current_measurement = current.measurements.get(&key);
        match (baseline_measurement, current_measurement) {
            (Some(base), Some(curr)) => {
                let change = curr.duration_seconds - base.duration_seconds;
                let improvement = if base.duration_seconds != 0.0 {
                    (base.duration_seconds - curr.duration_seconds) / base.duration_seconds * 100.0
                } else {
                    0.0
                };

                let mut improvement_label = format!("{improvement:.1}%");
                match classify_improvement(improvement) {
                    ImprovementClass::Improvement => {
                        improvements += 1;
                        improvement_label = format!("🟢 {improvement_label}");
                    }
                    ImprovementClass::Regression => {
                        regressions += 1;
                        improvement_label = format!("🔴 {improvement_label}");
                    }
                    ImprovementClass::Tie => {}
                }

                println!(
                    "| {key} | {} | {} | {} | {} |",
                    format_seconds(base.duration_seconds),
                    format_seconds(curr.duration_seconds),
                    format_signed_seconds(change),
                    improvement_label
                );
            }
            (Some(base), None) => {
                println!("| {key} | {} | N/A | N/A | N/A |", format_seconds(base.duration_seconds));
            }
            (None, Some(curr)) => {
                println!("| {key} | N/A | {} | N/A | N/A |", format_seconds(curr.duration_seconds));
            }
            _ => {}
        }
    }

    println!();
    println!("## Summary");
    println!();
    println!("- **Total metrics compared:** {total_metrics}");
    println!("- **Improvements:** {improvements}");
    println!("- **Regressions:** {regressions}");
    println!();

    println!("## Target Validation");
    println!();

    print_target_validation(
        "Full Workspace Build (Target: 40% faster)",
        baseline.measurements.get("clean_build_workspace").map(|m| m.duration_seconds),
        current.measurements.get("clean_build_workspace").map(|m| m.duration_seconds),
        Some(40.0),
    );

    let baseline_incremental = baseline
        .measurements
        .get("incremental_build_providers")
        .map(|m| m.duration_seconds)
        .or_else(|| {
            baseline.measurements.get("incremental_build_parser").map(|m| m.duration_seconds)
        });
    let current_incremental = current
        .measurements
        .get("incremental_build_providers")
        .map(|m| m.duration_seconds)
        .or_else(|| {
            current.measurements.get("incremental_build_parser").map(|m| m.duration_seconds)
        });

    print_target_validation(
        "Incremental Build (Target: 67% faster)",
        baseline_incremental,
        current_incremental,
        Some(67.0),
    );

    print_target_validation_no_target(
        "Test Build",
        baseline.measurements.get("test_build_workspace").map(|m| m.duration_seconds),
        current.measurements.get("test_build_workspace").map(|m| m.duration_seconds),
    );

    Ok(())
}

fn resolve_path(path: &PathBuf) -> Result<PathBuf> {
    let root = project_root()?;
    let candidate = if path.is_absolute() { path.clone() } else { root.join(path) };
    Ok(candidate)
}

fn format_seconds(value: f64) -> String {
    format!("{value:.1}s")
}

fn format_signed_seconds(value: f64) -> String {
    format!("{value:+.1}s")
}

fn print_target_validation(
    label: &str,
    baseline: Option<f64>,
    current: Option<f64>,
    target: Option<f64>,
) {
    match (baseline, current) {
        (Some(base), Some(cur)) => {
            let improvement = if base != 0.0 { (base - cur) / base * 100.0 } else { 0.0 };

            println!("### {label}");
            println!("- Baseline: {}", format_seconds(base));
            println!("- Current: {}", format_seconds(cur));
            println!("- Improvement: {improvement:.1}%");
            if let Some(target) = target {
                if improvement >= target {
                    println!("- Status: ✅ **Target Met**");
                } else {
                    println!("- Status: ❌ **Target Not Met**");
                }
            }
            println!();
        }
        _ => {
            println!("### {label}");
            println!("- Status: ⚠️ **No data available**");
            println!();
        }
    }
}

fn print_target_validation_no_target(label: &str, baseline: Option<f64>, current: Option<f64>) {
    match (baseline, current) {
        (Some(base), Some(cur)) => {
            let improvement = if base != 0.0 { (base - cur) / base * 100.0 } else { 0.0 };

            println!("### {label}");
            println!("- Baseline: {}", format_seconds(base));
            println!("- Current: {}", format_seconds(cur));
            println!("- Improvement: {improvement:.1}%");
            println!();
        }
        _ => {
            println!("### {label}");
            println!("- Status: ⚠️ **No data available**");
            println!();
        }
    }
}

fn collect_system_info(_root: &Path) -> SystemInfo {
    SystemInfo {
        cpu_cores: command_output_parse_or_unknown(&["nproc"])
            .or_else(|| command_output_parse_or_unknown(&["getconf", "_NPROCESSORS_ONLN"]))
            .unwrap_or_else(|| Value::String("unknown".to_string())),
        memory_gb: detect_memory_gb(),
        os: command_output_or_unknown(&["uname", "-s", "-r"]),
    }
}

fn detect_memory_gb() -> Value {
    if let Some(mem) = parse_memory_from_free() {
        return Value::from(mem);
    }

    command_output_parse_f64_or_unknown(&["sysctl", "-n", "hw.memsize"])
        .and_then(|value| {
            if value.is_number()
                && let Some(raw) = value.as_f64()
            {
                return Some(Value::from((raw / 1024.0 / 1024.0 / 1024.0).round() as u64));
            }
            None
        })
        .unwrap_or_else(|| Value::String("unknown".to_string()))
}

fn parse_memory_from_free() -> Option<u64> {
    let output = command_output(&["free", "-g"])?;
    for line in output.lines() {
        if line.starts_with("Mem:") {
            return line.split_whitespace().nth(1).and_then(|value| value.parse::<u64>().ok());
        }
    }
    None
}

fn command_output_or_unknown(command: &[&str]) -> String {
    match command_output(command) {
        Some(value) => value,
        None => "unknown".to_string(),
    }
}

fn command_output_parse_or_unknown(command: &[&str]) -> Option<Value> {
    command_output(command).and_then(|value| value.trim().parse::<u64>().ok().map(Value::from))
}

fn command_output_parse_f64_or_unknown(command: &[&str]) -> Option<Value> {
    command_output(command).and_then(|value| value.trim().parse::<f64>().ok().map(Value::from))
}

fn command_output(command: &[&str]) -> Option<String> {
    let (program, args) = command.split_first()?;
    let output = Command::new(program).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn touch_file(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {} parent directories", parent.display()))?;
    }

    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(&[])?;
    Ok(())
}

fn measure_command(
    root: &Path,
    name: &str,
    command: &[&str],
    pre_command: Option<&[&str]>,
) -> BuildMeasurement {
    println!("=== Measuring: {name} ===");
    println!("Command: {}", command_to_string(command));

    if let Some(pre) = pre_command {
        println!("Pre-command: {}", command_to_string(pre));
        run_command_silently(root, pre);
    }

    let duration = run_command_silently(root, command);
    println!("Duration: {duration:.4}s");
    println!();

    BuildMeasurement { duration_seconds: duration, command: command_to_string(command) }
}

fn run_command_silently(root: &Path, command: &[&str]) -> f64 {
    let start = Instant::now();
    let status = Command::new(command[0])
        .current_dir(root)
        .args(&command[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    if let Ok(status) = status {
        if !status.success() {
            println!("⚠️ Command exited with status {status}");
        }
    } else {
        println!("⚠️ Command failed to launch: {}", command_to_string(command));
    }

    start.elapsed().as_secs_f64()
}

fn command_to_string(command: &[&str]) -> String {
    command.join(" ")
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    /// A float-rounding residual on a 100ns timing difference (one second
    /// baseline) is `~1e-5`. The defect (raw `> 0.0` branching) classifies
    /// that as an improvement.
    #[test]
    fn classify_improvement_treats_float_rounding_residual_as_tie() {
        let improvement: f64 = (1.0 - 1.0000001) / 1.0 * 100.0;
        assert!(improvement.abs() > 0.0);
        assert!(improvement.abs() < IMPROVEMENT_EPS);
        assert_eq!(classify_improvement(improvement), ImprovementClass::Tie);
        assert_eq!(classify_improvement(-improvement), ImprovementClass::Tie);
    }

    /// Exactly-zero improvement is always a tie, regardless of epsilon.
    #[test]
    fn classify_improvement_treats_exact_zero_as_tie() {
        assert_eq!(classify_improvement(0.0), ImprovementClass::Tie);
    }

    /// Values strictly larger than `IMPROVEMENT_EPS` are improvements,
    /// regardless of how close they are to the boundary.
    #[test]
    fn classify_improvement_above_eps_is_improvement() {
        assert_eq!(classify_improvement(IMPROVEMENT_EPS * 2.0), ImprovementClass::Improvement);
        assert_eq!(classify_improvement(5.0), ImprovementClass::Improvement);
        assert_eq!(classify_improvement(100.0), ImprovementClass::Improvement);
    }

    /// Values strictly below `-IMPROVEMENT_EPS` are regressions.
    #[test]
    fn classify_improvement_below_neg_eps_is_regression() {
        assert_eq!(classify_improvement(-IMPROVEMENT_EPS * 2.0), ImprovementClass::Regression);
        assert_eq!(classify_improvement(-5.0), ImprovementClass::Regression);
    }

    /// The boundary values themselves are ties (closed interval).
    #[test]
    fn classify_improvement_at_boundary_is_tie() {
        assert_eq!(classify_improvement(IMPROVEMENT_EPS), ImprovementClass::Tie);
        assert_eq!(classify_improvement(-IMPROVEMENT_EPS), ImprovementClass::Tie);
    }

    /// A regression whose magnitude equals `IMPROVEMENT_EPS` is a tie, not a
    /// regression — symmetric with the positive case above.
    #[test]
    fn classify_improvement_negative_at_eps_is_tie() {
        assert_eq!(classify_improvement(-1e-3), ImprovementClass::Tie);
    }

    /// `run_compare` enforces the schema gate on both inputs: a wrong or
    /// missing version on either side must fail, so removing or reversing
    /// the gate (or validating only one side) turns this test red (#16024
    /// review).
    #[test]
    fn run_compare_rejects_wrong_or_missing_schema_version_on_either_side() -> Result<()> {
        fn receipt(version: Option<serde_json::Value>) -> serde_json::Value {
            let mut map = serde_json::Map::new();
            if let Some(version) = version {
                map.insert("schema_version".to_string(), version);
            }
            map.insert("timestamp".to_string(), serde_json::Value::from("2026-01-01T00:00:00Z"));
            map.insert("toolchain".to_string(), serde_json::Value::from("rustc (test)"));
            map.insert(
                "measurements".to_string(),
                serde_json::json!({
                    "clean_build_workspace": {
                        "duration_seconds": 1.0,
                        "command": "cargo build",
                    },
                }),
            );
            serde_json::Value::Object(map)
        }

        let dir = tempfile::tempdir()?;
        let baseline = dir.path().join("baseline.json");
        let current = dir.path().join("current.json");
        let write = |path: &std::path::Path, value: &serde_json::Value| -> Result<()> {
            std::fs::write(path, serde_json::to_string(value)?)?;
            Ok(())
        };

        // Both current: the gate passes (comparison itself may report drift;
        // here the inputs are identical so it succeeds).
        write(&baseline, &receipt(Some(serde_json::Value::from(SCHEMA_VERSION))))?;
        write(&current, &receipt(Some(serde_json::Value::from(SCHEMA_VERSION))))?;
        run_compare(baseline.clone(), current.clone())?;

        // Wrong version on either side fails through the schema gate.
        for (label, baseline_version, current_version) in [
            ("baseline-wrong", Some(serde_json::Value::from(SCHEMA_VERSION + 1)), None),
            ("current-wrong", None, Some(serde_json::Value::from(SCHEMA_VERSION + 1))),
        ] {
            // The `None` side keeps the current version; the `Some` side is
            // the wrong one under test.
            write(
                &baseline,
                &receipt(baseline_version.or(Some(serde_json::Value::from(SCHEMA_VERSION)))),
            )?;
            write(
                &current,
                &receipt(current_version.or(Some(serde_json::Value::from(SCHEMA_VERSION)))),
            )?;
            let err = run_compare(baseline.clone(), current.clone()).err().ok_or_else(|| {
                color_eyre::eyre::eyre!("run_compare must reject a wrong schema version ({label})")
            })?;
            assert!(
                err.to_string().contains("schema version mismatch"),
                "expected the schema gate, got: {err}"
            );
        }

        // Missing version on either side fails at parse (no serde default).
        for (label, drop_baseline, drop_current) in
            [("baseline-missing", true, false), ("current-missing", false, true)]
        {
            write(
                &baseline,
                &receipt(if drop_baseline {
                    None
                } else {
                    Some(serde_json::Value::from(SCHEMA_VERSION))
                }),
            )?;
            write(
                &current,
                &receipt(if drop_current {
                    None
                } else {
                    Some(serde_json::Value::from(SCHEMA_VERSION))
                }),
            )?;
            let err = run_compare(baseline.clone(), current.clone()).err().ok_or_else(|| {
                color_eyre::eyre::eyre!(
                    "run_compare must reject a missing schema version ({label})"
                )
            })?;
            assert!(
                err.to_string().contains("Failed to parse"),
                "expected a parse failure, got: {err}"
            );
        }

        Ok(())
    }

    /// Producer round-trip: the receipt stamps the current schema version and
    /// the consumer-facing view parses it back (#15357).
    #[test]
    fn receipt_stamps_current_schema_version() -> Result<()> {
        let receipt = BuildTimingReceipt {
            schema_version: SCHEMA_VERSION,
            timestamp: "2026-01-01T00:00:00Z".to_string(),
            toolchain: "rustc (test)".to_string(),
            system: SystemInfo {
                cpu_cores: Value::from(8u64),
                memory_gb: Value::from(16.0),
                os: "test".to_string(),
            },
            measurements: BTreeMap::from([(
                "clean_build_workspace".to_string(),
                BuildMeasurement { duration_seconds: 1.0, command: "cargo build".to_string() },
            )]),
        };
        let raw = serde_json::to_string(&receipt).context("serialize receipt")?;
        let parsed: MeasurableReceipt = serde_json::from_str(&raw).context("parse receipt")?;
        assert_eq!(parsed.schema_version, SCHEMA_VERSION);
        assert!(
            parsed.measurements.contains_key("clean_build_workspace"),
            "round-trip must keep measurements"
        );
        Ok(())
    }
}
