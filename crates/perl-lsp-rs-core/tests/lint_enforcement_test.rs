//! Tests verifying that perl-lsp-rs-core enforces print-statement lint rules.
//!
//! GitHub issue #3224 identified inconsistent print-statement lint enforcement
//! across the workspace. This test verifies that the `perl-lsp-rs-core` crate
//! (which absorbed `perl-lsp-launcher`) carries:
//!
//! - workspace-level `print_stderr = "deny"` / `print_stdout = "deny"` in Cargo.toml
//! - `#![cfg_attr(test, allow(...))]` to suppress in test code
//! - `#[expect(clippy::print_stderr)]` on `startup_banner_with_quiet` (the one intentional exception)
//!
//! These tests read the actual source files via `CARGO_MANIFEST_DIR` so they would
//! catch any future accidental removal of the directives.

use perl_test_must::{must, must_some, must_some_with};
use std::fs;

/// Returns the path to perl-lsp-rs-core's `lib.rs`.
fn lib_rs_path() -> std::path::PathBuf {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest_dir.join("src/lib.rs")
}

/// Returns the path to the runtime launcher module.
fn launcher_mod_path() -> std::path::PathBuf {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest_dir.join("src/runtime/launcher/mod.rs")
}

fn read_source(path: &std::path::Path) -> String {
    must(fs::read_to_string(path))
}

fn find_line_number(source: &str, pattern: &str) -> Option<usize> {
    source.lines().position(|line| line.contains(pattern)).map(|pos| pos + 1)
}

fn immediately_preceding_attribute(source: &str, function_pattern: &str) -> Option<String> {
    let lines = source.lines().collect::<Vec<_>>();
    let function_line = lines.iter().position(|line| line.contains(function_pattern))?;
    let mut end = function_line;
    while end > 0 && lines[end - 1].trim().is_empty() {
        end -= 1;
    }
    if end == 0 || !lines[end - 1].trim_end().ends_with(']') {
        return None;
    }
    let mut start = end - 1;
    while start > 0 && !lines[start].trim_start().starts_with("#[") {
        start -= 1;
    }
    if !lines[start].trim_start().starts_with("#[") {
        return None;
    }
    Some(lines[start..end].join("\n"))
}

#[test]
fn test_workspace_cargo_has_print_deny() {
    // Navigate from manifest dir up to workspace root
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest
        .ancestors()
        .skip(1) // skip the crate dir
        .find(|p| p.join("Cargo.toml").exists() && p.join("Cargo.lock").exists())
        .map(std::path::Path::to_path_buf);
    let workspace_root = must_some(workspace_root);
    let cargo_toml = must(std::fs::read_to_string(workspace_root.join("Cargo.toml")));
    assert!(
        cargo_toml.contains("print_stderr = \"deny\""),
        "workspace Cargo.toml must contain print_stderr = \"deny\" in [workspace.lints.clippy]"
    );
    assert!(
        cargo_toml.contains("print_stdout = \"deny\""),
        "workspace Cargo.toml must contain print_stdout = \"deny\" in [workspace.lints.clippy]"
    );
}

#[test]
fn test_lib_has_cfg_attr_allow_in_test_mode() {
    let source = read_source(&lib_rs_path());
    let pattern = "#![cfg_attr(test, allow(clippy::print_stderr, clippy::print_stdout))]";
    assert!(
        find_line_number(&source, pattern).is_some(),
        "perl-lsp-rs-core/src/lib.rs is missing test-mode suppression directive:\n  {pattern}\n\n\
         Without this, test helpers that use eprintln!/println! would fail to compile."
    );
}

#[test]
fn test_startup_banner_has_allow_annotation() {
    let source = read_source(&launcher_mod_path());
    // The expect annotation must appear before the helper that emits stderr.
    // rustfmt may expand the attribute to multi-line format; search for the lint name
    // directly since it must appear in both single-line and multi-line forms.
    let allow_line = find_line_number(&source, "clippy::print_stderr");
    let fn_line = find_line_number(&source, "fn startup_banner_with_quiet(");
    let annotation = immediately_preceding_attribute(&source, "fn startup_banner_with_quiet(");

    assert!(
        allow_line.is_some(),
        "src/runtime/launcher/mod.rs: startup_banner_with_quiet is missing its \
         #[expect(clippy::print_stderr, reason = ...)] annotation.\n\
         The eprintln! in startup_banner_with_quiet fires before the tracing subscriber is configured \
         and is the one intentional exception in this crate. \
         Also verify that the source contains #[expect(clippy::print_stderr ...)]."
    );

    // Also verify the file contains an #[expect(... construct (not just #[allow(...)]).
    assert!(
        source.contains("#[expect("),
        "src/runtime/launcher/mod.rs: clippy::print_stderr suppression must use \
         #[expect(...)] not #[allow(...)]. Update startup_banner annotation."
    );

    let allow = must_some_with(allow_line, "clippy::print_stderr occurrence must be present");
    let func = must_some_with(fn_line, "startup_banner_with_quiet must be present");
    let annotation = must_some_with(
        annotation,
        "startup_banner_with_quiet must have an immediately preceding attribute",
    );
    assert!(
        allow < func,
        "The clippy::print_stderr annotation (line {allow}) must appear \
         before startup_banner_with_quiet (line {func})."
    );
    assert!(
        annotation.contains("#[expect(") && annotation.contains("clippy::print_stderr"),
        "The immediate attribute before startup_banner_with_quiet must expect clippy::print_stderr."
    );
}
