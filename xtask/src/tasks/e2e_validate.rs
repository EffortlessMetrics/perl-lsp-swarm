//! End-to-end validation task
//!
//! Orchestrates a comprehensive validation sweep across the workspace:
//! 1. Exact real-process contracts for Cargo's public `perllsp` binary
//! 2. Release-mode tests for core crates (parser, LSP, DAP)
//! 3. Large-workspace process-liveness smoke (not protocol proof)
//! 4. Benchmark compilation check
//! 5. Structured JSON report with actual pass/fail tracking
//!
//! This replaces the shell script `scripts/e2e-validation.sh` with a
//! focused, non-duplicative Rust implementation that produces reliable
//! results (the shell script swallowed errors with `2>/dev/null` and
//! emitted a static report regardless of outcomes).

use chrono::Utc;
use color_eyre::eyre::{Context, Result};
use console::Style;
use duct::cmd;
use indicatif::{ProgressBar, ProgressStyle};
use serde::Serialize;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::utils::project_root;

// =============================================================================
// Configuration
// =============================================================================

/// The two exact public-process targets governed by issue #13057, exercised
/// as E2E proof. This is the issue's selected contract set, deliberately not
/// the full `*_process.rs` inventory under `crates/perllsp/tests/`; the
/// `governed_public_process_targets_exist_in_perllsp_test_inventory` test
/// fails if any governed target disappears from that inventory.
const PUBLIC_PROCESS_TARGETS: &[&str] =
    &["lsp_stdio_process_contract", "lsp_document_lifecycle_process"];

/// How long the large-workspace liveness smoke waits before requiring the
/// public binary to still be running. A liveness-only signal, not protocol
/// proof; named so the window is greppable and single-sourced.
const LIVENESS_WINDOW: Duration = Duration::from_secs(2);

/// Core crates whose release-mode lib tests are exercised.
const CORE_CRATES: &[&str] = &["perl-parser", "perl-lsp-rs", "perl-dap"];

// =============================================================================
// Public API
// =============================================================================

/// Configuration for the e2e-validate subcommand.
pub struct E2eConfig {
    /// Number of files to generate for the large-workspace test.
    pub workspace_size: usize,
    /// Path for the JSON report (None = skip report).
    pub report_path: Option<PathBuf>,
    /// Skip the large-workspace liveness smoke.
    pub skip_workspace: bool,
    /// Skip the benchmark compilation check.
    pub skip_bench: bool,
    /// Verbose output.
    pub verbose: bool,
}

/// Run the end-to-end validation sweep.
pub fn run(config: E2eConfig) -> Result<()> {
    let root = project_root()?;
    std::env::set_current_dir(&root).context("Failed to change to project root")?;

    let bold = Style::new().bold();
    println!("\n{}", bold.apply_to("End-to-End Validation"));
    println!("{}", bold.apply_to("====================="));

    let overall_start = Instant::now();
    let mut results = Vec::new();

    // ── Phase 1: Exact public-binary process contracts ─────────────────
    println!("\n{}", bold.apply_to("Phase 1: Exact public perllsp process contracts"));
    for target in PUBLIC_PROCESS_TARGETS {
        let outcome = run_public_process_test(target, config.verbose)?;
        results.push(outcome);
    }

    // ── Phase 2: Release-mode core-crate tests ──────────────────────────
    println!("\n{}", bold.apply_to("Phase 2: Release-mode core crate tests"));
    for crate_name in CORE_CRATES {
        let outcome = run_crate_test(crate_name, config.verbose)?;
        results.push(outcome);
    }

    // ── Phase 3: Large-workspace process-liveness smoke ─────────────────
    if !config.skip_workspace {
        println!("\n{}", bold.apply_to("Phase 3: Large-workspace process-liveness smoke"));
        let outcome = run_workspace_liveness_smoke_test(config.workspace_size, &root)?;
        results.push(outcome);
    }

    // ── Phase 4: Benchmark compilation ──────────────────────────────────
    if !config.skip_bench {
        println!("\n{}", bold.apply_to("Phase 4: Benchmark compilation check"));
        let outcome = run_bench_compile_check()?;
        results.push(outcome);
    }

    // ── Summary ─────────────────────────────────────────────────────────
    let elapsed = overall_start.elapsed();
    print_summary(&results, elapsed);

    // ── Optional JSON report ────────────────────────────────────────────
    if let Some(path) = &config.report_path {
        write_report(&results, elapsed, path)?;
        println!("\nReport written to {}", path.display());
    }

    // Fail if any step failed
    let failures: Vec<&StepOutcome> = results.iter().filter(|r| !r.passed).collect();
    if !failures.is_empty() {
        let names: Vec<&str> = failures.iter().map(|f| f.name.as_str()).collect();
        Err(color_eyre::eyre::eyre!("{} step(s) failed: {}", failures.len(), names.join(", ")))
    } else {
        Ok(())
    }
}

// =============================================================================
// Step implementations
// =============================================================================

fn public_process_test_args(target: &str, verbose: bool) -> Vec<&str> {
    let mut args = vec![
        "test",
        "-p",
        "perllsp",
        "--test",
        target,
        "--release",
        "--locked",
        "--",
        "--test-threads=1",
    ];
    if !verbose {
        args.push("-q");
    }
    args
}

/// Run one exact public `perllsp` integration target and capture its result.
fn run_public_process_test(target: &str, verbose: bool) -> Result<StepOutcome> {
    let spinner = make_spinner()?;
    spinner.set_message(format!("Testing public perllsp process target {target}..."));

    let start = Instant::now();
    let result = cmd("cargo", public_process_test_args(target, verbose))
        .stderr_to_stdout()
        .unchecked()
        .run();

    let elapsed = start.elapsed();
    let (passed, detail) = match result {
        Ok(output) => {
            let success = output.status.success();
            let detail = if success {
                None
            } else {
                Some(String::from_utf8_lossy(&output.stdout).to_string())
            };
            (success, detail)
        }
        Err(e) => (false, Some(format!("Failed to execute: {e}"))),
    };

    let label = format!("perllsp exact-process target {target}");
    print_step_result(&spinner, &label, passed, elapsed);
    Ok(StepOutcome { name: label, passed, duration: elapsed, detail })
}

/// Run `cargo test -p <crate> --lib --release` and capture result.
fn run_crate_test(crate_name: &str, verbose: bool) -> Result<StepOutcome> {
    let spinner = make_spinner()?;
    spinner.set_message(format!("Testing {} (release)...", crate_name));

    let start = Instant::now();
    let mut args = vec!["test", "-p", crate_name, "--lib", "--release"];
    if !verbose {
        args.extend_from_slice(&["--", "-q"]);
    }

    let result = cmd("cargo", &args).stderr_to_stdout().unchecked().run();

    let elapsed = start.elapsed();
    let (passed, detail) = match result {
        Ok(output) => {
            let success = output.status.success();
            let detail = if success {
                None
            } else {
                Some(String::from_utf8_lossy(&output.stdout).to_string())
            };
            (success, detail)
        }
        Err(e) => (false, Some(format!("Failed to execute: {e}"))),
    };

    let label = format!("{} release tests", crate_name);
    print_step_result(&spinner, &label, passed, elapsed);
    Ok(StepOutcome { name: label, passed, duration: elapsed, detail })
}

fn perllsp_release_build_args() -> &'static [&'static str] {
    &["build", "-p", "perllsp", "--bin", "perllsp", "--release", "--locked"]
}

fn perllsp_binary_path(project_root: &std::path::Path, profile: &str) -> PathBuf {
    project_root
        .join("target")
        .join(profile)
        .join(format!("perllsp{}", std::env::consts::EXE_SUFFIX))
}

/// Generate N Perl files in a temp directory, start the public LSP binary,
/// wait briefly, then verify the process remains alive. This is deliberately
/// a liveness signal only; the exact-process targets above own protocol proof.
fn run_workspace_liveness_smoke_test(
    file_count: usize,
    project_root: &std::path::Path,
) -> Result<StepOutcome> {
    let spinner = make_spinner()?;
    spinner.set_message(format!("Generating {} Perl files...", file_count));

    let start = Instant::now();

    // Create temp directory
    let tmp_dir = tempdir_for_workspace()?;
    let tmp_path = tmp_dir.path();

    // Generate files
    for i in 1..=file_count {
        let content = format!(
            "#!/usr/bin/perl\nuse strict;\nuse warnings;\n\npackage Test{i};\n\nsub test_function_{i} {{\n    my ($param) = @_;\n    return \"result_{i}\";\n}}\n\n1;\n"
        );
        let file_path = tmp_path.join(format!("test_{i}.pl"));
        fs::write(&file_path, content)
            .with_context(|| format!("Failed to write {}", file_path.display()))?;
    }

    spinner.set_message("Starting perllsp liveness smoke against large workspace...");

    // Locate binary
    let binary = find_lsp_binary(project_root);
    let (passed, detail) = match binary {
        Some(bin) => run_lsp_liveness_smoke(&bin, tmp_path)?,
        None => {
            // Try building the actual public binary first.
            spinner.set_message("Building public perllsp binary (release)...");
            let build_result =
                cmd("cargo", perllsp_release_build_args()).stderr_to_stdout().unchecked().run();
            match build_result {
                Ok(output) if output.status.success() => {
                    let bin = perllsp_binary_path(project_root, "release");
                    if bin.exists() {
                        run_lsp_liveness_smoke(&bin, tmp_path)?
                    } else {
                        (
                            false,
                            Some(format!(
                                "perllsp binary not found after build: {}",
                                bin.display()
                            )),
                        )
                    }
                }
                Ok(output) => (
                    false,
                    Some(format!(
                        "Failed to build perllsp: {}",
                        String::from_utf8_lossy(&output.stdout)
                    )),
                ),
                Err(e) => (false, Some(format!("Build command failed: {e}"))),
            }
        }
    };

    let elapsed = start.elapsed();
    let label = format!("Large-workspace process-liveness smoke ({file_count} files)");
    print_step_result(&spinner, &label, passed, elapsed);

    // Clean up
    drop(tmp_dir);

    Ok(StepOutcome { name: label, passed, duration: elapsed, detail })
}

/// Verify that benchmarks compile (without running them).
fn run_bench_compile_check() -> Result<StepOutcome> {
    let spinner = make_spinner()?;
    spinner.set_message("Checking benchmark compilation...");

    let start = Instant::now();
    let result = cmd("cargo", &["bench", "--no-run"]).stderr_to_stdout().unchecked().run();

    let elapsed = start.elapsed();
    let (passed, detail) = match result {
        Ok(output) => {
            let success = output.status.success();
            let detail = if success {
                None
            } else {
                Some(String::from_utf8_lossy(&output.stdout).to_string())
            };
            (success, detail)
        }
        Err(e) => (false, Some(format!("Failed to execute: {e}"))),
    };

    let label = "Benchmark compilation".to_string();
    print_step_result(&spinner, &label, passed, elapsed);
    Ok(StepOutcome { name: label, passed, duration: elapsed, detail })
}

// =============================================================================
// Helpers
// =============================================================================

/// Outcome of a single validation step.
#[derive(Serialize)]
struct StepOutcome {
    name: String,
    passed: bool,
    #[serde(serialize_with = "serialize_duration")]
    duration: Duration,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

fn serialize_duration<S: serde::Serializer>(
    d: &Duration,
    s: S,
) -> std::result::Result<S::Ok, S::Error> {
    s.serialize_f64(d.as_secs_f64())
}

fn make_spinner() -> Result<ProgressBar> {
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {wide_msg}")
            .context("Failed to create spinner template")?,
    );
    Ok(spinner)
}

fn print_step_result(spinner: &ProgressBar, label: &str, passed: bool, duration: Duration) {
    let icon = if passed { "PASS" } else { "FAIL" };
    let style = if passed { Style::new().green() } else { Style::new().red() };
    spinner.finish_with_message(format!(
        "[{}] {} ({:.1}s)",
        style.apply_to(icon),
        label,
        duration.as_secs_f64()
    ));
}

fn print_summary(results: &[StepOutcome], total: Duration) {
    let passed = results.iter().filter(|r| r.passed).count();
    let failed = results.len() - passed;
    let bold = Style::new().bold();

    println!("\n{}", bold.apply_to("Summary"));
    println!("{}", bold.apply_to("-------"));
    println!(
        "  Total: {} steps, {} passed, {} failed ({:.1}s)",
        results.len(),
        passed,
        failed,
        total.as_secs_f64()
    );

    if failed > 0 {
        let red = Style::new().red().bold();
        println!("\n  {}", red.apply_to("Failed steps:"));
        for r in results.iter().filter(|r| !r.passed) {
            println!("    - {}", r.name);
        }
    }
}

fn write_report(results: &[StepOutcome], total: Duration, path: &std::path::Path) -> Result<()> {
    let passed = results.iter().filter(|r| r.passed).count();
    let failed = results.len() - passed;

    // We cannot move out of the slice, so rebuild the steps for serialization.
    // Instead, serialize the slice reference directly since StepOutcome implements Serialize.
    let report = serde_json::json!({
        "timestamp": Utc::now().to_rfc3339(),
        "total_duration_secs": total.as_secs_f64(),
        "passed": passed,
        "failed": failed,
        "steps": results,
    });

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory {}", parent.display()))?;
    }

    let mut file =
        fs::File::create(path).with_context(|| format!("Failed to create {}", path.display()))?;
    let json_bytes = serde_json::to_vec_pretty(&report).context("Failed to serialize report")?;
    file.write_all(&json_bytes).with_context(|| format!("Failed to write {}", path.display()))?;

    Ok(())
}

/// Locate an existing public `perllsp` binary (release preferred).
fn find_lsp_binary(project_root: &std::path::Path) -> Option<PathBuf> {
    let release = perllsp_binary_path(project_root, "release");
    if release.exists() {
        return Some(release);
    }
    let debug = perllsp_binary_path(project_root, "debug");
    if debug.exists() {
        return Some(debug);
    }
    None
}

/// What the liveness window observed about the spawned server process.
#[derive(Debug)]
enum LivenessObservation {
    /// The process was still running when the window closed.
    Alive,
    /// The process exited before the window closed.
    ExitedEarly { status: std::process::ExitStatus, stderr: String },
    /// The probe itself failed (e.g. `try_wait` errored).
    ProbeFailed(String),
}

/// Map a liveness observation to the step's pass/fail surface. Diagnostic
/// wording lives only here, so tests assert on the classification instead of
/// coupling to message text.
fn render_liveness(observation: LivenessObservation) -> (bool, Option<String>) {
    match observation {
        LivenessObservation::Alive => (true, None),
        LivenessObservation::ExitedEarly { status, stderr } => (
            false,
            Some(format!(
                "LSP server exited before the liveness window completed with status {status}: {stderr}"
            )),
        ),
        LivenessObservation::ProbeFailed(reason) => (false, Some(reason)),
    }
}

/// Start the LSP binary with `--stdio`, wait one liveness window, and
/// classify what the window observed. Protocol behavior is proved separately
/// by the exact public-process targets.
fn observe_liveness_window(
    binary: &std::path::Path,
    workspace_dir: &std::path::Path,
) -> Result<LivenessObservation> {
    use std::process::{Command, Stdio};

    let mut child = Command::new(binary)
        .arg("--stdio")
        .current_dir(workspace_dir)
        .env("PERL_LSP_QUIET", "1")
        .stdin(Stdio::piped())
        // The liveness window does not observe the server's stdout, and a
        // piped-but-never-drained handle could block the child once the OS
        // pipe buffer fills, so send it to null instead.
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("Failed to spawn {}", binary.display()))?;

    // Drain stderr in the background for the whole window so the child can
    // never block on a full stderr pipe either; the drained bytes remain
    // available as early-exit diagnostics.
    let stderr_tail = child.stderr.take().map(|mut stderr| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = std::io::Read::read_to_end(&mut stderr, &mut buf);
            String::from_utf8_lossy(&buf).to_string()
        })
    });

    // Give the server a moment to initialize (or exit/crash).
    std::thread::sleep(LIVENESS_WINDOW);

    match child.try_wait() {
        Ok(Some(status)) => {
            let stderr = stderr_tail.and_then(|handle| handle.join().ok()).unwrap_or_default();
            Ok(LivenessObservation::ExitedEarly { status, stderr })
        }
        Ok(None) => {
            let _ = child.kill();
            let _ = child.wait();
            Ok(LivenessObservation::Alive)
        }
        Err(e) => {
            Ok(LivenessObservation::ProbeFailed(format!("Failed to check process status: {e}")))
        }
    }
}

/// Start the LSP binary, observe one liveness window, and report pass/fail
/// with diagnostics for the step outcome.
fn run_lsp_liveness_smoke(
    binary: &std::path::Path,
    workspace_dir: &std::path::Path,
) -> Result<(bool, Option<String>)> {
    Ok(render_liveness(observe_liveness_window(binary, workspace_dir)?))
}

/// Create a temporary directory for the large-workspace test.
///
/// We use a wrapper that calls `std::fs::create_dir_all` + removal on drop,
/// avoiding a dev-dependency on `tempfile` in the production binary.
fn tempdir_for_workspace() -> Result<TempDir> {
    let base = std::env::temp_dir().join("perl-lsp-e2e-workspace");
    // Clean up any leftover from a previous run
    if base.exists() {
        fs::remove_dir_all(&base).ok();
    }
    fs::create_dir_all(&base)
        .with_context(|| format!("Failed to create temp dir {}", base.display()))?;
    Ok(TempDir(base))
}

/// Simple RAII temp directory.
struct TempDir(PathBuf);

impl TempDir {
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use color_eyre::Result;

    fn unique_path(suffix: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "perl-lsp-e2e-{}-{}-{suffix}",
            std::process::id(),
            Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ))
    }

    #[test]
    fn exact_public_process_targets_are_selected() {
        assert_eq!(
            PUBLIC_PROCESS_TARGETS,
            &["lsp_stdio_process_contract", "lsp_document_lifecycle_process"]
        );
    }

    #[test]
    fn governed_public_process_targets_exist_in_perllsp_test_inventory() -> Result<()> {
        let root = project_root()?;
        for target in PUBLIC_PROCESS_TARGETS {
            let test_path =
                root.join("crates").join("perllsp").join("tests").join(format!("{target}.rs"));
            assert!(
                test_path.is_file(),
                "governed target {target} missing from crates/perllsp/tests: {}",
                test_path.display()
            );
        }
        Ok(())
    }

    #[test]
    fn liveness_rendering_maps_classifications_to_pass_fail() {
        assert_eq!(render_liveness(LivenessObservation::Alive), (true, None));
        assert_eq!(
            render_liveness(LivenessObservation::ProbeFailed("boom".to_string())),
            (false, Some("boom".to_string()))
        );
    }

    #[test]
    fn public_process_contract_args_are_release_locked_and_serial() {
        assert_eq!(
            public_process_test_args("lsp_stdio_process_contract", true),
            vec![
                "test",
                "-p",
                "perllsp",
                "--test",
                "lsp_stdio_process_contract",
                "--release",
                "--locked",
                "--",
                "--test-threads=1",
            ]
        );
        assert_eq!(
            public_process_test_args("lsp_document_lifecycle_process", false),
            vec![
                "test",
                "-p",
                "perllsp",
                "--test",
                "lsp_document_lifecycle_process",
                "--release",
                "--locked",
                "--",
                "--test-threads=1",
                "-q",
            ]
        );
    }

    #[test]
    fn public_binary_fallback_builds_the_perllsp_package_and_binary() {
        assert_eq!(
            perllsp_release_build_args(),
            &["build", "-p", "perllsp", "--bin", "perllsp", "--release", "--locked"]
        );
    }

    #[test]
    fn find_lsp_binary_prefers_release() -> Result<()> {
        let root = unique_path("bin-locate");
        let release = perllsp_binary_path(&root, "release");
        let debug = perllsp_binary_path(&root, "debug");
        let release_dir = release
            .parent()
            .ok_or_else(|| color_eyre::eyre::eyre!("release path must have a parent"))?;
        let debug_dir = debug
            .parent()
            .ok_or_else(|| color_eyre::eyre::eyre!("debug path must have a parent"))?;
        fs::create_dir_all(release_dir)?;
        fs::create_dir_all(debug_dir)?;

        fs::write(&release, "#!/bin/sh\nexit 0\n")?;
        fs::write(&debug, "#!/bin/sh\nexit 0\n")?;

        let found = find_lsp_binary(&root);
        assert_eq!(found, Some(release));

        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn write_report_persists_step_counts_and_details() -> Result<()> {
        let report_path = unique_path("report").join("nested/report.json");
        let steps = vec![
            StepOutcome {
                name: "step 1".to_string(),
                passed: true,
                duration: Duration::from_millis(500),
                detail: None,
            },
            StepOutcome {
                name: "step 2".to_string(),
                passed: false,
                duration: Duration::from_secs(1),
                detail: Some("boom".to_string()),
            },
        ];

        write_report(&steps, Duration::from_secs(2), &report_path)?;

        let raw = fs::read_to_string(&report_path)?;
        let report_json: serde_json::Value = serde_json::from_str(&raw)?;
        assert_eq!(report_json["passed"].as_u64(), Some(1));
        assert_eq!(report_json["failed"].as_u64(), Some(1));
        assert_eq!(report_json["steps"][1]["detail"].as_str(), Some("boom"));

        let parent = report_path
            .parent()
            .ok_or_else(|| color_eyre::eyre::eyre!("report path must have a parent"))?;
        fs::remove_dir_all(parent)?;
        Ok(())
    }

    #[cfg(unix)]
    mod unix_tests {
        use super::*;
        use std::os::unix::fs::PermissionsExt;

        fn write_executable_script(path: &std::path::Path, content: &str) -> Result<()> {
            fs::write(path, content)?;
            let mut perms = fs::metadata(path)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(path, perms)?;
            Ok(())
        }

        #[test]
        fn liveness_smoke_reports_nonzero_exit() -> Result<()> {
            let temp_root = unique_path("nonzero");
            fs::create_dir_all(&temp_root)?;
            let script = temp_root.join("fake_perllsp.sh");
            write_executable_script(&script, "#!/usr/bin/env bash\necho crash >&2\nexit 7\n")?;

            let observation = observe_liveness_window(&script, &temp_root)?;
            let (status, stderr) = match observation {
                LivenessObservation::ExitedEarly { status, stderr } => (status, stderr),
                other => panic!("expected ExitedEarly, got {other:?}"),
            };
            assert_eq!(status.code(), Some(7));
            assert!(stderr.contains("crash"));

            fs::remove_dir_all(temp_root)?;
            Ok(())
        }

        #[test]
        fn liveness_smoke_rejects_early_clean_exit() -> Result<()> {
            let temp_root = unique_path("early-clean-exit");
            fs::create_dir_all(&temp_root)?;
            let script = temp_root.join("fake_perllsp.sh");
            write_executable_script(&script, "#!/usr/bin/env bash\nexit 0\n")?;

            let observation = observe_liveness_window(&script, &temp_root)?;
            let status = match observation {
                LivenessObservation::ExitedEarly { status, .. } => status,
                other => panic!("expected ExitedEarly, got {other:?}"),
            };
            assert!(status.success());

            fs::remove_dir_all(temp_root)?;
            Ok(())
        }

        #[test]
        fn liveness_smoke_accepts_a_running_process() -> Result<()> {
            let temp_root = unique_path("long-running");
            fs::create_dir_all(&temp_root)?;
            let script = temp_root.join("fake_perllsp.sh");
            write_executable_script(&script, "#!/usr/bin/env bash\nsleep 5\n")?;

            let observation = observe_liveness_window(&script, &temp_root)?;
            assert!(
                matches!(observation, LivenessObservation::Alive),
                "expected Alive, got {observation:?}"
            );

            fs::remove_dir_all(temp_root)?;
            Ok(())
        }
    }
}
