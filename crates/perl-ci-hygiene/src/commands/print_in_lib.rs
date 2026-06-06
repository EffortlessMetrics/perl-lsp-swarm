//! Print-macro policy for library source files.
//!
//! Keeps the `check-print-in-lib` command focused on its single CI hygiene
//! responsibility instead of living in the CLI dispatcher.

use color_eyre::eyre::Result;
use regex::Regex;
use std::ffi::OsStr;
use std::path::Path;
use std::sync::LazyLock;

use crate::{
    display_path, first_cfg_test_line_number, read_lines, read_usize_file,
    walk_rust_source_files_for_ci_checks,
};

static PRINT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(println!|eprintln!|print!\(|eprint!\()").unwrap_or_else(|err| {
        unreachable!("print macro regex is a known-good static pattern: {err}")
    })
});

static DEBUG_ASSERTIONS_ATTR_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"#\[cfg\(debug_assertions\)\]").unwrap_or_else(|err| {
        unreachable!("debug assertions regex is a known-good static pattern: {err}")
    })
});

/// Returns `true` for paths that should be skipped by the print-in-lib check.
///
/// This is a superset of `is_excluded_test_path` with extra exclusions specific
/// to the print-macro policy:
///   - `build.rs` files: Cargo build scripts use `println!("cargo:...")` to communicate
///     with Cargo itself.  This is the standard mechanism; it is not "library output".
///   - Files whose name starts with `test_` (e.g. `test_parser.rs`): these are test
///     driver / helper files that live alongside library source but are only invoked
///     during test runs.
///   - Test-support crates whose primary purpose is emitting diagnostic output during
///     test execution (e.g. `perl-lsp-ux-tests`).
fn is_excluded_for_print_check(path: &Path) -> bool {
    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

    // Build scripts may use println!("cargo:...") — standard Cargo convention.
    if file_name == "build.rs" {
        return true;
    }

    // Test-driver files next to src/ but not inside tests/ directory.
    if file_name.starts_with("test_") && file_name.ends_with(".rs") {
        return true;
    }

    // Test-support and UX-test crates use print output intentionally.
    if path.components().any(|c| c.as_os_str() == OsStr::new("perl-lsp-ux-tests")) {
        return true;
    }

    false
}

/// Returns `true` when a source file should be skipped wholesale by the print-macro check.
///
/// Files with a file-level `#![allow(clippy::print_stderr)]` or
/// `#![allow(clippy::print_stdout)]` attribute have been explicitly opted out of the
/// rule (e.g. `cli.rs` in the LSP binary crate).  The attribute must appear in the
/// first 30 lines of the file (the module-doc / crate-doc block).
fn file_has_print_allow(lines: &[String]) -> bool {
    for line in lines.iter().take(30) {
        if line_has_inner_print_allow_attr(line) {
            return true;
        }
    }
    false
}

fn line_has_inner_print_allow_attr(line: &str) -> bool {
    line.contains("#![allow(")
        && (line.contains("clippy::print_stderr")
            || line.contains("clippy::print_stdout")
            || line.contains("clippy::print_"))
}

fn line_has_outer_print_allow_attr(line: &str) -> bool {
    line.contains("#[allow(")
        && (line.contains("clippy::print_stderr")
            || line.contains("clippy::print_stdout")
            || line.contains("clippy::print_"))
}

fn line_is_whole_line_comment(line: &str) -> bool {
    line.trim_start().starts_with("//")
}

#[derive(Default)]
struct PrintAllowScope {
    pending_attr: bool,
    active_brace_depth: usize,
}

impl PrintAllowScope {
    fn note_attribute(&mut self) {
        self.pending_attr = true;
    }

    fn allows_current_line(&self) -> bool {
        self.pending_attr || self.active_brace_depth > 0
    }

    fn observe_line(&mut self, line: &str) {
        let trimmed = line.trim();
        if trimmed.is_empty() || line_is_whole_line_comment(line) || trimmed.starts_with("#[") {
            return;
        }

        if self.active_brace_depth > 0 {
            self.apply_brace_delta(line);
            return;
        }

        if self.pending_attr {
            self.pending_attr = false;
            let delta = brace_delta(line);
            if delta > 0 {
                self.active_brace_depth = delta as usize;
            }
        }
    }

    fn apply_brace_delta(&mut self, line: &str) {
        let delta = brace_delta(line);
        if delta.is_negative() {
            self.active_brace_depth = self.active_brace_depth.saturating_sub(delta.unsigned_abs());
        } else {
            self.active_brace_depth = self.active_brace_depth.saturating_add(delta as usize);
        }
    }
}

fn brace_delta(line: &str) -> isize {
    let opens = line.chars().filter(|ch| *ch == '{').count() as isize;
    let closes = line.chars().filter(|ch| *ch == '}').count() as isize;
    opens - closes
}

/// Enforce that library source files do not contain raw `println!` / `eprintln!` /
/// `print!` / `eprint!` calls.
///
/// Library code should use `tracing::{debug,info,warn,error}` for all diagnostic
/// output.  Raw print macros:
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
/// This check mirrors the pattern of `cmd_check_unwraps_prod`.  The baseline is stored
/// in `ci/print_in_lib_baseline.txt`; the check fails if the current count exceeds it.
pub(crate) fn cmd_check_print_in_lib(repo_root: &Path) -> Result<i32> {
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

            if PRINT_RE.is_match(line) {
                if !debug_assertions_scope.allows_current_line()
                    && !print_allow_scope.allows_current_line()
                {
                    offenders.push(format!("{rel}:{line_no}:{}", line.trim()));
                }
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
