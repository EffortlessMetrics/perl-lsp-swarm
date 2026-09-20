use color_eyre::eyre::{Result, eyre};
use regex::Regex;
use std::path::Path;
use std::sync::LazyLock;

use crate::{
    display_path, first_cfg_test_line_number, read_lines, read_usize_file,
    walk_rust_source_files_for_ci_checks,
};

use self::allow_scopes::{
    AttrJoiner, PrintAllowScope, file_has_print_allow, line_is_whole_line_comment,
};
use self::exclusions::is_excluded_for_print_check;

mod allow_scopes;
mod exclusions;

static PRINT_MACRO_RE: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"(println!|eprintln!|print!\(|eprint!\()"));
static DEBUG_ASSERTIONS_ATTR_RE: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"#\[cfg\(debug_assertions\)\]"));

fn regex_from_static(
    regex: &'static LazyLock<Result<Regex, regex::Error>>,
    label: &str,
) -> Result<&'static Regex> {
    regex.as_ref().map_err(|err| eyre!("{label} regex failed to compile: {err}"))
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
pub(crate) fn check_print_in_lib(repo_root: &Path) -> Result<i32> {
    let print_re = regex_from_static(&PRINT_MACRO_RE, "print macro")?;
    let debug_attr_re = regex_from_static(&DEBUG_ASSERTIONS_ATTR_RE, "debug assertions attribute")?;
    let mut offenders = Vec::new();

    for path in walk_rust_source_files_for_ci_checks(repo_root)? {
        if is_excluded_for_print_check(&path) {
            continue;
        }
        let rel = display_path(repo_root, &path);
        let lines = read_lines(&path)?;

        // Skip files that have a file-level opt-out attribute.
        if file_has_print_allow(&lines) {
            continue;
        }

        let test_start = first_cfg_test_line_number(&path).unwrap_or(usize::MAX);

        let mut debug_assertions_scope = PrintAllowScope::default();
        let mut print_allow_scope = PrintAllowScope::default();
        let mut attrs = AttrJoiner::default();

        for (index, line) in lines.iter().enumerate() {
            let line_no = index + 1;
            if line_no >= test_start {
                break;
            }

            if debug_attr_re.is_match(line) {
                debug_assertions_scope.note_attribute();
            }

            // The joiner has to see every line, because an attribute rustfmt
            // wrapped is only recognisable once its last line arrives.
            let completed = attrs.feed(line);
            if let Some(attr) = &completed
                && !attr.inner
            {
                print_allow_scope.note_attribute();
            }
            let inside_attribute = attrs.in_attribute() || completed.is_some();

            if line_is_whole_line_comment(line) {
                continue;
            }

            // Every line is tested for a print macro, including one the joiner
            // believes is inside an attribute. Skipping those was the more
            // dangerous shape: an attribute the joiner failed to close would
            // silently swallow the rest of the file. Nothing is lost by
            // testing them, because an attribute's own text cannot contain a
            // print macro call -- `clippy::print_stdout` is not `println!`.
            if print_re.is_match(line)
                && !debug_assertions_scope.allows_current_line()
                && !print_allow_scope.allows_current_line()
            {
                offenders.push(format!("{rel}:{line_no}:{}", line.trim()));
            }

            // Brace counting is the part that must skip them: a wrapped
            // attribute's braces are not an item's braces, and counting them
            // would close a scope the attribute is still opening.
            if inside_attribute {
                continue;
            }

            debug_assertions_scope.observe_line(line);
            print_allow_scope.observe_line(line);
        }
    }

    let baseline = read_usize_file(&repo_root.join("ci/print_in_lib_baseline.txt"), 0)?;
    println!("print macros in library source: {} (baseline: {})", offenders.len(), baseline);
    if offenders.len() > baseline {
        println!("FAIL: print macro count ({}) exceeds baseline ({})", offenders.len(), baseline);
        println!();
        println!("Offenders (use tracing::{{debug,info,warn,error}} instead):");
        for line in offenders.iter().take(20) {
            println!("  {line}");
        }
        if let Some(withheld) = offenders.len().checked_sub(20).filter(|count| *count > 0) {
            println!("  ... and {withheld} more");
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
