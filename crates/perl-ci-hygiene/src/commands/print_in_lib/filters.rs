use std::ffi::OsStr;
use std::path::Path;

/// Returns `true` for paths that should be skipped by the print-in-lib check.
///
/// This is a superset of `is_excluded_test_path` with extra exclusions specific
/// to the print-macro policy:
///   - `build.rs` files: Cargo build scripts use `println!("cargo:...")` to communicate
///     with Cargo itself. This is the standard mechanism; it is not "library output".
///   - Files whose name starts with `test_` (e.g. `test_parser.rs`): these are test
///     driver / helper files that live alongside library source but are only invoked
///     during test runs.
///   - Test-support crates whose primary purpose is emitting diagnostic output during
///     test execution (e.g. `perl-lsp-ux-tests`).
pub(super) fn is_excluded_for_print_check(path: &Path) -> bool {
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
/// rule (e.g. `cli.rs` in the LSP binary crate). The attribute must appear in the
/// first 30 lines of the file (the module-doc / crate-doc block).
pub(super) fn file_has_print_allow(lines: &[String]) -> bool {
    lines.iter().take(30).any(|line| line_has_inner_print_allow_attr(line))
}

fn line_has_inner_print_allow_attr(line: &str) -> bool {
    line.contains("#![allow(") && line_contains_print_allow(line)
}

pub(super) fn line_has_outer_print_allow_attr(line: &str) -> bool {
    line.contains("#[allow(") && line_contains_print_allow(line)
}

fn line_contains_print_allow(line: &str) -> bool {
    line.contains("clippy::print_stderr")
        || line.contains("clippy::print_stdout")
        || line.contains("clippy::print_")
}

pub(super) fn line_has_print_macro(line: &str) -> bool {
    line.contains("println!")
        || line.contains("eprintln!")
        || line.contains("print!(")
        || line.contains("eprint!(")
}

pub(super) fn line_is_whole_line_comment(line: &str) -> bool {
    line.trim_start().starts_with("//")
}
