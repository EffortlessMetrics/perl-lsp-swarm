use color_eyre::eyre::Result;
use regex::Regex;
use std::path::Path;
use std::sync::LazyLock;

mod policy;
mod scope;

use policy::{
    file_has_print_allow, is_excluded_for_print_check, line_has_outer_print_allow_attr,
    line_is_whole_line_comment,
};
use scope::PrintAllowScope;

static PRINT_MACRO_RE: LazyLock<Regex> =
    LazyLock::new(|| compile_regex(r"(println!|eprintln!|print!\(|eprint!\()"));
static DEBUG_ASSERTIONS_ATTR_RE: LazyLock<Regex> =
    LazyLock::new(|| compile_regex(r"#\[cfg\(debug_assertions\)\]"));

fn compile_regex(pattern: &str) -> Regex {
    match Regex::new(pattern) {
        Ok(regex) => regex,
        Err(err) => unreachable!("print-in-lib regex is a known-good static pattern: {err}"),
    }
}

/// Enforce that library source files do not contain raw `println!` / `eprintln!` /
/// `print!` / `eprint!` calls.
///
/// Library code should use `tracing::{debug,info,warn,error}` for all diagnostic
/// output. Raw print macros:
///   - Bypass the structured logging pipeline (no span context, no log level filtering).
///   - Appear in release builds and pollute the LSP's stdout/stderr channels.
///   - Make test output noisy when tests fail.
///
/// Allowed exceptions (enforced at the call site):
///   - Files with a file-level `#![allow(clippy::print_stderr/stdout)]` attribute (e.g.
///     `cli.rs` in the LSP binary crate — user-facing output is their product).
///   - Lines inside `#[cfg(debug_assertions)]` blocks (debug-only guardrails).
///   - The startup banner in `launcher/mod.rs` (function-level `#[allow]`).
///   - Any future deliberate exception must add the clippy allow attribute with a
///     comment explaining why.
///
/// This check mirrors the pattern of `cmd_check_unwraps_prod`. The baseline is stored
/// in `ci/print_in_lib_baseline.txt`; the check fails if the current count exceeds it.
pub(crate) fn cmd_check_print_in_lib(repo_root: &Path) -> Result<i32> {
    let offenders = collect_print_offenders(repo_root)?;
    let baseline = crate::read_usize_file(&repo_root.join("ci/print_in_lib_baseline.txt"), 0)?;

    println!("print macros in library source: {} (baseline: {})", offenders.len(), baseline);
    if offenders.len() > baseline {
        println!("FAIL: print macro count ({}) exceeds baseline ({})", offenders.len(), baseline);
        println!();
        println!("Offenders (use tracing::{{debug,info,warn,error}} instead):");
        for line in offenders.iter().take(20) {
            println!("  {line}");
        }
        println!();
        println!("If the print macro is intentional, add #[allow(clippy::print_stderr)] or");
        println!("#[allow(clippy::print_stdout)] with a comment explaining why.");
        println!(
            "If you removed print macros, update ci/print_in_lib_baseline.txt with the new lower count."
        );
        return Ok(1);
    }

    if offenders.len() < baseline {
        println!(
            "NOTE: count ({}) is below baseline ({}). Update ci/print_in_lib_baseline.txt to ratchet down.",
            offenders.len(),
            baseline
        );
    }

    Ok(0)
}

fn collect_print_offenders(repo_root: &Path) -> Result<Vec<String>> {
    let mut offenders = Vec::new();

    for path in crate::walk_rust_source_files_for_ci_checks(repo_root)? {
        if is_excluded_for_print_check(&path) {
            continue;
        }

        let rel = crate::display_path(repo_root, &path);
        let lines = crate::read_lines(&path)?;

        if file_has_print_allow(&lines) {
            continue;
        }

        let test_start = crate::first_cfg_test_line_number(&path).unwrap_or(usize::MAX);
        offenders.extend(scan_source_lines(&rel, &lines, test_start));
    }

    Ok(offenders)
}

fn scan_source_lines(rel: &str, lines: &[String], test_start: usize) -> Vec<String> {
    let mut offenders = Vec::new();
    let mut debug_assertions_scope = PrintAllowScope::default();
    let mut print_allow_scope = PrintAllowScope::default();

    for (index, line) in lines.iter().enumerate() {
        let line_no = index + 1;
        if line_no >= test_start {
            break;
        }

        if DEBUG_ASSERTIONS_ATTR_RE.is_match(line) {
            debug_assertions_scope.note_attribute();
        }
        if line_has_outer_print_allow_attr(line) {
            print_allow_scope.note_attribute();
        }

        if line_is_whole_line_comment(line) {
            continue;
        }

        if PRINT_MACRO_RE.is_match(line)
            && !debug_assertions_scope.allows_current_line()
            && !print_allow_scope.allows_current_line()
        {
            offenders.push(format!("{rel}:{line_no}:{}", line.trim()));
        }

        debug_assertions_scope.observe_line(line);
        print_allow_scope.observe_line(line);
    }

    offenders
}
