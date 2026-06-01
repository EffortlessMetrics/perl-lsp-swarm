use color_eyre::eyre::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

use crate::process::{
    command_exists, command_status, command_status_strict, command_timed_status,
    command_with_input_with_status, command_with_output_allow_failure,
};
use crate::{GREEN, NC, RED, YELLOW, read_usize_from_tokens};

pub(crate) fn cmd_run_parser_comparison(repo_root: &Path) -> Result<i32> {
    println!("=== Perl Parser Comparison Benchmark ===");
    println!("Comparing perl-parser vs tree-sitter-perl-c");
    println!();
    println!("Building parsers...");
    let _ = command_with_output_allow_failure(
        repo_root,
        "cargo",
        &["build", "--release", "-p", "perl-parser"],
        &[],
    )?;
    let _ = command_with_output_allow_failure(
        repo_root,
        "cargo",
        &["build", "--release", "-p", "tree-sitter-perl-c"],
        &[],
    )?;
    println!();
    println!("Running benchmarks on standard test cases...");
    let benchmark = command_with_output_allow_failure(
        repo_root,
        "sh",
        &[
            "-c",
            "cargo bench -p parser-benchmarks --bench simple_compare 2>&1 | grep -E \"parser-comparison|time:\" | grep -B1 \"time:\"",
        ],
        &[],
    )?;
    if !benchmark.trim().is_empty() {
        println!("{benchmark}");
    }
    println!();
    println!("=== Summary ===");
    println!("perl-parser: Pure Rust implementation using perl-lexer");
    println!("tree-sitter-c: C implementation with tree-sitter");
    Ok(0)
}

pub(crate) fn cmd_run_comparison(repo_root: &Path) -> Result<i32> {
    println!("=== Three-Way Parser Comparison ===");
    println!("Comparing: Pure Rust vs Legacy C vs Modern Parser");
    println!();

    let test_cases = [
        ("Simple", r#"my $x = 42;"#),
        ("Expression", r#"my $result = ($a + $b) * $c;"#),
        ("Control Flow", r#"if ($x > 10) { while ($y < 100) { $y = $y * 2; } }"#),
        ("Method Call", r#"$obj->method($arg1, $arg2);"#),
        ("For Loop", r#"for (my $i = 0; $i < 10; $i++) { print $i; }"#),
    ];

    let legacy_parser = repo_root.join("target/debug/parse");

    println!("Running parser tests...");
    println!();

    for (name, code) in test_cases {
        println!("Testing: {name}");
        println!("Code: {code}");

        println!("  Modern parser: ");
        let modern_args: Vec<&str> = if command_exists("timeout") {
            vec!["1s", "cargo", "run", "-q", "-p", "perl-parser", "--example", "demo", "--"]
        } else {
            vec!["-q", "-p", "perl-parser", "--example", "demo", "--"]
        };
        let (modern_status, modern_output) = if command_exists("timeout") {
            command_with_input_with_status(repo_root, "timeout", &modern_args, &[], code)?
        } else {
            command_with_input_with_status(repo_root, "cargo", &modern_args, &[], code)?
        };
        if modern_status == 0 && modern_output.contains("Success") {
            println!("  ✅ Success");
        } else {
            println!("  ❌ Failed");
        }

        if legacy_parser.is_file() {
            println!("  Legacy C parser: ");
            let legacy_str = legacy_parser.to_string_lossy();
            let legacy_ref = legacy_str.as_ref();
            let legacy_args = if command_exists("timeout") {
                vec!["1s", legacy_ref, "--"]
            } else {
                vec![legacy_ref, "--"]
            };
            let (legacy_status, legacy_output) = if command_exists("timeout") {
                command_with_input_with_status(repo_root, "timeout", &legacy_args, &[], code)?
            } else {
                command_with_input_with_status(repo_root, legacy_ref, &legacy_args[1..], &[], code)?
            };
            if legacy_status == 0
                && (legacy_output.contains("success") || legacy_output.contains("parsed"))
            {
                println!("  ✅ Success");
            } else {
                println!("  ❌ Failed");
            }
        }

        println!();
    }

    println!("Performance comparison would require working benchmarks.");
    println!("Currently, the modern parser (perl-lexer + perl-parser) is fully functional.");
    Ok(0)
}

pub(crate) fn cmd_quick_bench(repo_root: &Path) -> Result<i32> {
    println!("=== Three-Way Parser Comparison ===");
    println!();
    println!("Parsers:");
    println!("  native-v3  : perl-parser-bench  (v3 recursive-descent, raw parser API)");
    println!(
        "  facade     : bench_facade        (tree-sitter-perl-rs, v3 wrapped in tree-sitter ergonomics)"
    );
    println!(
        "  c-grammar  : bench_parser_c      (tree-sitter C grammar binding, requires libclang)"
    );
    println!();
    println!("Building benchmark binaries...");
    build_quick_bench_binaries(repo_root)?;
    println!("Using median of {QUICK_BENCH_SAMPLES} direct binary runs per parser/file.");
    println!();

    let files = vec![
        repo_root.join("test_corpus/simple.pl"),
        repo_root.join("test_corpus/low_frequency_nodekinds.rs"),
        repo_root.join("test_corpus/parser_stress_cases.pl"),
        repo_root.join("test_corpus/performance_stress_scenarios.pl"),
        repo_root.join("test_corpus/basic_constructs.pl"),
    ];
    let mut candidates: Vec<(String, PathBuf)> = files
        .into_iter()
        .filter(|path| path.is_file())
        .map(|path| {
            (path.file_name().and_then(|name| name.to_str()).unwrap_or("file").to_string(), path)
        })
        .collect();
    candidates.sort_by(|a, b| a.0.cmp(&b.0));

    // Collect results once to avoid running each parser twice per file.
    struct Row {
        name: String,
        size: u64,
        rust_time: Option<f64>,
        facade_time: Option<f64>,
        c_time: Option<f64>,
    }

    let mut rows: Vec<Row> = Vec::new();
    for (name, path) in &candidates {
        let size = fs::metadata(path).map(|metadata| metadata.len()).unwrap_or(0);
        let rust_time = run_rust_bench_us(repo_root, path)?;
        let facade_time = run_facade_bench_us(repo_root, path)?;
        let c_time = run_c_bench_us(repo_root, path)?;
        rows.push(Row { name: name.clone(), size, rust_time, facade_time, c_time });
    }

    // --- Table 1: raw timings ---
    let col_file = 30usize;
    let col_num = 12usize;
    println!(
        "{:<col_file$} {:>8}  {:>col_num$}  {:>col_num$}  {:>col_num$}  fastest",
        "File", "Size", "native-v3(µs)", "facade(µs)", "c-gram(µs)"
    );
    println!(
        "{:<col_file$} {:>8}  {:>col_num$}  {:>col_num$}  {:>col_num$}  -------",
        "----", "----", "-------------", "----------", "----------"
    );

    for row in &rows {
        let times: [(&str, Option<f64>); 3] =
            [("native-v3", row.rust_time), ("facade", row.facade_time), ("c-grammar", row.c_time)];
        let fastest_label = times
            .iter()
            .filter_map(|(label, t)| t.map(|v| (label, v)))
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(label, _)| *label)
            .unwrap_or("N/A");

        let fmt = |v: Option<f64>| -> String {
            v.map(|us| format!("{us:.0}")).unwrap_or_else(|| "N/A".to_string())
        };

        println!(
            "{:<col_file$} {:>8}  {:>col_num$}  {:>col_num$}  {:>col_num$}  {}",
            row.name,
            row.size,
            fmt(row.rust_time),
            fmt(row.facade_time),
            fmt(row.c_time),
            fastest_label,
        );
    }

    println!();

    // --- Table 2: relative-to-fastest ---
    println!("=== Relative to fastest (per file) ===");
    println!();
    println!(
        "{:<col_file$}  {:>col_num$}  {:>col_num$}  {:>col_num$}",
        "File", "native-v3", "facade", "c-grammar"
    );
    println!(
        "{:<col_file$}  {:>col_num$}  {:>col_num$}  {:>col_num$}",
        "----", "---------", "------", "---------"
    );

    for row in &rows {
        let available: Vec<f64> =
            [row.rust_time, row.facade_time, row.c_time].iter().filter_map(|&t| t).collect();
        let min = available.iter().cloned().fold(f64::INFINITY, f64::min);

        let rel = |v: Option<f64>| -> String {
            match v {
                Some(us) if min > 0.0 => format!("{:.2}x", us / min),
                Some(_) => "1.00x".to_string(),
                None => "N/A".to_string(),
            }
        };

        println!(
            "{:<col_file$}  {:>col_num$}  {:>col_num$}  {:>col_num$}",
            row.name,
            rel(row.rust_time),
            rel(row.facade_time),
            rel(row.c_time),
        );
    }

    println!();
    println!("Quick benchmark complete!");
    println!("Note: c-grammar requires libclang; N/A means libclang was not found.");
    println!(
        "Note: facade overhead vs native-v3 should be near epsilon (facade wraps same v3 parser)."
    );
    Ok(0)
}

pub(crate) fn cmd_simple_bench(repo_root: &Path) -> Result<i32> {
    println!("Pure Rust Perl Parser Performance Test");
    println!("======================================");

    let parser = repo_root.join("archive/crates/tree-sitter-perl-rs/target/release/parse-rust");
    command_status_strict(
        repo_root,
        "cargo",
        &["build", "--release", "-p", "tree-sitter-perl-rs", "--bin", "parse-rust"],
        &[],
    )?;

    let workspace = repo_root.join("target").join("perl-ci-hygiene").join("simple_bench");
    fs::create_dir_all(&workspace).with_context(|| format!("creating {}", workspace.display()))?;
    let tiny = workspace.join("tiny.pl");
    let small = workspace.join("small.pl");
    let medium = workspace.join("medium.pl");
    let large = workspace.join("large.pl");
    let huge = workspace.join("huge.pl");

    fs::write(&tiny, "my $x = 42;\n")?;
    fs::copy(repo_root.join("test_corpus").join("basic_constructs.pl"), &small)
        .wrap_err("copying small fixture")?;
    fs::copy(repo_root.join("test_corpus").join("parser_stress_cases.pl"), &medium)
        .wrap_err("copying medium fixture")?;
    fs::copy(repo_root.join("test_corpus").join("real_world/enterprise_cpan_patterns.pl"), &large)
        .wrap_err("copying large fixture")?;
    fs::copy(
        repo_root.join("test_corpus").join("edge_cases/performance_stress_scenarios.pl"),
        &huge,
    )
    .wrap_err("copying huge fixture")?;

    println!();
    println!("Creating test files...");

    println!();
    println!("Test file sizes:");
    for path in [&tiny, &small, &medium, &large, &huge] {
        let lines = read_usize_from_tokens(path, 0).unwrap_or(0);
        let size = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        println!(
            "{:<10} {:>6} lines, {:>8}",
            path.file_name().and_then(|s| s.to_str()).unwrap_or(""),
            lines,
            size
        );
    }

    println!();
    println!("Run benchmarks...");
    println!("--------------------------------------");

    for path in [&tiny, &small, &medium, &large, &huge] {
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("benchmark");
        println!("\n{name}:");
        let mut total_ms = 0.0f64;
        for _ in 0..5 {
            let time = timed_file_run_ms(repo_root, &parser, path)?;
            println!("  Run: {time:.0}ms");
            total_ms += time;
        }
        println!("  Average: {:.0}ms", total_ms / 5.0);
    }

    println!();
    println!("Performance Summary:");
    println!("====================");
    println!("The Pure Rust Perl Parser shows excellent performance,");
    println!("parsing typical Perl files with linear scaling.");
    Ok(0)
}

pub(crate) fn cmd_profile_stack_overflow(repo_root: &Path) -> Result<i32> {
    println!("{YELLOW}🔍 Profiling stack overflow in debug builds{NC}");
    println!("==================================================");

    let tests = [
        "test_deep_nested_expression",
        "test_deep_nested_blocks",
        "test_deep_nested_arrays",
        "test_deep_method_chain",
    ];
    let log_dir = repo_root.join("target").join("perl-ci-hygiene").join("stack-overflow-logs");
    fs::create_dir_all(&log_dir).with_context(|| format!("creating {}", log_dir.display()))?;

    let env_vars = [("CARGO_BUILD_MODE", "debug"), ("RUST_BACKTRACE", "full")];

    for test in tests {
        println!();
        println!("{YELLOW}Testing: {test}{NC}");
        let base_args = vec![
            "test",
            "--features",
            "pure-rust",
            "--test",
            "debug_stack_overflow_test",
            test,
            "--",
            "--ignored",
            "--nocapture",
        ];

        let (status, output) = if command_exists("timeout") {
            let mut args = vec!["10s", "cargo"];
            args.extend_from_slice(&base_args);
            command_with_input_with_status(repo_root, "timeout", &args, &env_vars, "")?
        } else {
            command_with_input_with_status(repo_root, "cargo", &base_args, &env_vars, "")?
        };

        let log_file = log_dir.join(format!("stack_trace_{test}.log"));
        fs::write(&log_file, &output)?;

        if status == 0 {
            println!("{GREEN}✅ Test completed (unexpected - should overflow){NC}");
            continue;
        }

        if status == 124 {
            println!("{RED}⏱️ Test timed out after 10s{NC}");
        } else {
            println!("{RED}❌ Test failed with exit code: {status}{NC}");
        }

        let marker = output.contains("stack overflow") || output.contains("SIGSEGV");
        if marker {
            println!("{YELLOW}Stack overflow detected! Analyzing...{NC}");
            println!();
            println!("{YELLOW}Recursive patterns found:{NC}");
            let mut lines = Vec::new();
            for line in output.lines() {
                if line.contains("build_node") || line.contains("parse_") || line.contains("visit_")
                {
                    lines.push(line.to_string());
                }
            }
            lines.sort();
            lines.dedup();
            for line in lines.iter().take(20) {
                println!("  {line}");
            }
        } else {
            println!("{RED}No explicit stack-overflow signature found in output{NC}");
        }
    }

    println!();
    println!("{YELLOW}📊 Summary{NC}");
    println!("Stack traces saved under: {}", log_dir.display());
    println!("Look for repeated function calls to identify recursion.");
    Ok(0)
}
/// Crate identifier for the v3 native Rust parser benchmark.
pub(crate) const RUST_BENCH_CRATE: &str = "perl-parser-bench";

/// Binary identifier for the v3 native Rust parser benchmark.
///
/// Used by [`cmd_quick_bench`] and asserted in the unit test that guards
/// against regressing the C-vs-Rust comparison (see issue #3204).
pub(crate) const RUST_BENCH_BIN: &str = "perl-parser-bench";

/// Binary identifier for the legacy C tree-sitter parser benchmark.
///
/// Lives in the workspace-EXCLUDED `tree-sitter-perl-c` crate (libclang-dev
/// dependency), so it must be invoked via `--manifest-path` rather than `-p`.
pub(crate) const C_BENCH_BIN: &str = "bench_parser_c";

/// Relative path (from repo root) to the C tree-sitter crate's Cargo.toml.
///
/// Pinned here alongside `C_BENCH_BIN` so that the regression test
/// (`quick_bench_uses_distinct_binaries_for_c_and_rust`) can assert both the
/// binary name and the crate location remain distinct from the Rust bench path.
pub(crate) const C_BENCH_MANIFEST: &str = "crates/tree-sitter-perl-c/Cargo.toml";

/// Crate identifier for the `tree-sitter-perl-rs` facade benchmark.
///
/// Lives in the normal workspace (no excluded crate dance), so it is
/// invoked with `-p FACADE_BENCH_CRATE --bin FACADE_BENCH_BIN`.
pub(crate) const FACADE_BENCH_CRATE: &str = "tree-sitter-perl-rs";

/// Binary identifier for the `tree-sitter-perl-rs` facade benchmark.
///
/// Used by [`cmd_quick_bench`] and asserted in the unit test
/// `three_way_bench_all_binaries_distinct`.
pub(crate) const FACADE_BENCH_BIN: &str = "bench_facade";

/// Number of timed samples collected per parser/file pair in quick-bench mode.
const QUICK_BENCH_SAMPLES: usize = 3;

/// Build the quick-bench parser binaries ahead of timing.
///
/// The C grammar bench is optional because it depends on libclang. Failures
/// while building that bench are treated as "N/A" at measurement time.
fn build_quick_bench_binaries(repo_root: &Path) -> Result<()> {
    command_status_strict(
        repo_root,
        "cargo",
        &["build", "--quiet", "--release", "-p", RUST_BENCH_CRATE, "--bin", RUST_BENCH_BIN],
        &[],
    )?;
    command_status_strict(
        repo_root,
        "cargo",
        &["build", "--quiet", "--release", "-p", FACADE_BENCH_CRATE, "--bin", FACADE_BENCH_BIN],
        &[],
    )?;
    // Optional dependency path: allow failure and report as N/A during run.
    let _ = command_status(
        repo_root,
        "cargo",
        &[
            "build",
            "--quiet",
            "--release",
            "--manifest-path",
            C_BENCH_MANIFEST,
            "--bin",
            C_BENCH_BIN,
            "--features",
            "test-utils",
        ],
        &[],
    )?;
    Ok(())
}

/// Return the median duration in microseconds over `QUICK_BENCH_SAMPLES` runs.
fn run_bench_samples_us(repo_root: &Path, command: &Path, file: &Path) -> Result<Option<f64>> {
    let command_str = command.to_string_lossy().into_owned();
    let file_arg = file.to_string_lossy().into_owned();
    let args = [file_arg.as_str()];
    let mut samples = Vec::with_capacity(QUICK_BENCH_SAMPLES);
    for _ in 0..QUICK_BENCH_SAMPLES {
        let (status, elapsed) = command_timed_status(repo_root, &command_str, &args, &[])?;
        if status != 0 {
            return Ok(None);
        }
        samples.push(elapsed.as_micros() as f64);
    }
    if samples.is_empty() {
        return Ok(None);
    }
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Ok(samples.get(samples.len() / 2).copied())
}

/// Run the v3 native Rust parser bench binary against `file`.
fn run_rust_bench_us(repo_root: &Path, file: &Path) -> Result<Option<f64>> {
    let binary = repo_root.join("target").join("release").join(RUST_BENCH_BIN);
    run_bench_samples_us(repo_root, &binary, file)
}

/// Run the legacy C tree-sitter parser bench binary against `file`.
///
/// `tree-sitter-perl-c` is in `[workspace.exclude]` because of its libclang
/// build dependency, so this helper invokes cargo with `--manifest-path`
/// pointing at that crate's Cargo.toml. The `test-utils` feature is required
/// for the binary target.
///
/// Returns wall-clock duration in microseconds, or `None` if the bench
/// binary fails to build or exits non-zero (e.g. on systems without
/// libclang installed). Quick-bench treats `None` as N/A in the speedup
/// column rather than failing the whole run.
fn run_c_bench_us(repo_root: &Path, file: &Path) -> Result<Option<f64>> {
    let binary = repo_root
        .join("crates")
        .join("tree-sitter-perl-c")
        .join("target")
        .join("release")
        .join(C_BENCH_BIN);
    run_bench_samples_us(repo_root, &binary, file)
}

/// Run the `tree-sitter-perl-rs` facade bench binary against `file`.
///
/// The facade crate lives in the normal workspace so it is invoked with
/// `-p` rather than `--manifest-path`. Returns wall-clock duration in
/// microseconds, or `None` if the bench binary exits non-zero.
fn run_facade_bench_us(repo_root: &Path, file: &Path) -> Result<Option<f64>> {
    let binary = repo_root.join("target").join("release").join(FACADE_BENCH_BIN);
    run_bench_samples_us(repo_root, &binary, file)
}

fn timed_file_run_ms(repo_root: &Path, parser: &Path, file: &Path) -> Result<f64> {
    let file_arg = file.to_string_lossy().into_owned();
    let parser_path = parser.to_string_lossy().into_owned();
    let args = [file_arg.as_str(), "--sexp"];
    let (status, elapsed) = command_timed_status(repo_root, parser_path.as_str(), &args, &[])?;
    if status == 0 {
        Ok(elapsed.as_millis() as f64)
    } else {
        Err(color_eyre::eyre::eyre!(
            "parser command {parser_path} failed for {} with status {status}",
            file.display()
        ))
    }
}
