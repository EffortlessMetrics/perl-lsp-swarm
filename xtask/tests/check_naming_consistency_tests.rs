//! Integration tests for `cargo xtask check-naming-consistency` (issue #2933 AC#3).
//!
//! These tests exercise the real xtask binary via `assert_cmd`, following the
//! established BDD pattern in `fmt_package_lookup_tests.rs`.  Each test describes
//! a concrete scenario and verifies the observable CLI behaviour: exit code,
//! stdout content, and stderr cleanliness.

use assert_cmd::Command;
use std::fs;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Fixture helpers
// ---------------------------------------------------------------------------

/// Create a minimal synthetic workspace where every `crates/<dir>` name matches.
fn make_consistent_workspace() -> TempDir {
    let dir = TempDir::new().expect("tempdir");
    let root = dir.path();

    fs::write(
        root.join("Cargo.toml"),
        r#"[workspace]
members = ["crates/perl-parser", "crates/perl-lexer"]
resolver = "2"
"#,
    )
    .unwrap();

    for name in ["perl-parser", "perl-lexer"] {
        let crate_dir = root.join("crates").join(name);
        fs::create_dir_all(crate_dir.join("src")).unwrap();
        fs::write(
            crate_dir.join("Cargo.toml"),
            format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
        )
        .unwrap();
        fs::write(crate_dir.join("src").join("lib.rs"), "").unwrap();
    }

    dir
}

/// Create a workspace with one mismatch: `crates/perl-lsp/` has `name = "perl-lsp-rs"`.
fn make_workspace_with_mismatch() -> TempDir {
    let dir = TempDir::new().expect("tempdir");
    let root = dir.path();

    fs::write(
        root.join("Cargo.toml"),
        r#"[workspace]
members = ["crates/perl-parser", "crates/perl-lsp"]
resolver = "2"
"#,
    )
    .unwrap();

    // OK crate.
    let parser_dir = root.join("crates/perl-parser");
    fs::create_dir_all(parser_dir.join("src")).unwrap();
    fs::write(
        parser_dir.join("Cargo.toml"),
        "[package]\nname = \"perl-parser\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::write(parser_dir.join("src").join("lib.rs"), "").unwrap();

    // Mismatched crate: directory is `perl-lsp`, package name is `perl-lsp-rs`.
    let lsp_dir = root.join("crates/perl-lsp");
    fs::create_dir_all(lsp_dir.join("src")).unwrap();
    fs::write(
        lsp_dir.join("Cargo.toml"),
        "[package]\nname = \"perl-lsp-rs\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::write(lsp_dir.join("src").join("lib.rs"), "").unwrap();

    dir
}

/// Create a workspace that contains a non-Rust directory (no `Cargo.toml`).
fn make_workspace_with_non_rust_dir() -> TempDir {
    let dir = TempDir::new().expect("tempdir");
    let root = dir.path();

    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/perl-parser\"]\nresolver = \"2\"\n",
    )
    .unwrap();

    // Rust crate (consistent).
    let crate_dir = root.join("crates/perl-parser");
    fs::create_dir_all(crate_dir.join("src")).unwrap();
    fs::write(
        crate_dir.join("Cargo.toml"),
        "[package]\nname = \"perl-parser\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::write(crate_dir.join("src").join("lib.rs"), "").unwrap();

    // Non-Rust directory (e.g. a JavaScript tree-sitter grammar).
    fs::create_dir_all(root.join("crates/tree-sitter-perl")).unwrap();

    dir
}

/// Create a workspace with multiple mismatches.
fn make_workspace_with_multiple_mismatches() -> TempDir {
    let dir = TempDir::new().expect("tempdir");
    let root = dir.path();

    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/a-dir\", \"crates/b-dir\"]\nresolver = \"2\"\n",
    )
    .unwrap();

    for (dir_name, pkg_name) in [("a-dir", "a-package"), ("b-dir", "b-package")] {
        let crate_dir = root.join("crates").join(dir_name);
        fs::create_dir_all(crate_dir.join("src")).unwrap();
        fs::write(
            crate_dir.join("Cargo.toml"),
            format!("[package]\nname = \"{pkg_name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
        )
        .unwrap();
        fs::write(crate_dir.join("src").join("lib.rs"), "").unwrap();
    }

    dir
}

// ---------------------------------------------------------------------------
// Scenario: consistent workspace → exit 0
// ---------------------------------------------------------------------------

#[test]
fn consistent_workspace_exits_zero() {
    // GIVEN a workspace where every crates/<dir> has package name == dir
    let ws = make_consistent_workspace();

    // WHEN check-naming-consistency is run
    let output = Command::cargo_bin("xtask")
        .unwrap()
        .current_dir(ws.path())
        .args(["check-naming-consistency"])
        .output()
        .expect("failed to run xtask");

    // THEN the command exits 0
    assert!(
        output.status.success(),
        "expected exit 0 for consistent workspace; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn consistent_workspace_stdout_reports_pass() {
    // GIVEN a consistent workspace
    let ws = make_consistent_workspace();

    // WHEN check-naming-consistency is run
    let output = Command::cargo_bin("xtask")
        .unwrap()
        .current_dir(ws.path())
        .args(["check-naming-consistency"])
        .output()
        .expect("failed to run xtask");

    // THEN stdout contains a success indicator
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("✅") || stdout.contains("All") && stdout.contains("matching"),
        "expected pass summary in stdout; got: {stdout}"
    );
}

// ---------------------------------------------------------------------------
// Scenario: mismatch present → exit non-zero
// ---------------------------------------------------------------------------

#[test]
fn mismatch_workspace_exits_nonzero() {
    // GIVEN a workspace where one crates/<dir> has a different package name
    let ws = make_workspace_with_mismatch();

    // WHEN check-naming-consistency is run
    let output = Command::cargo_bin("xtask")
        .unwrap()
        .current_dir(ws.path())
        .args(["check-naming-consistency"])
        .output()
        .expect("failed to run xtask");

    // THEN the command exits non-zero
    assert!(
        !output.status.success(),
        "expected non-zero exit for workspace with mismatch; stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn mismatch_workspace_stdout_names_the_offending_directory() {
    // GIVEN a workspace with a mismatch at crates/perl-lsp (package = perl-lsp-rs)
    let ws = make_workspace_with_mismatch();

    // WHEN check-naming-consistency is run
    let output = Command::cargo_bin("xtask")
        .unwrap()
        .current_dir(ws.path())
        .args(["check-naming-consistency"])
        .output()
        .expect("failed to run xtask");

    // THEN stdout identifies the mismatched directory and both names
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("perl-lsp"),
        "expected mismatched directory name in stdout; got: {stdout}"
    );
    assert!(stdout.contains("perl-lsp-rs"), "expected package name in stdout; got: {stdout}");
}

#[test]
fn mismatch_workspace_stdout_contains_mismatch_count() {
    // GIVEN a workspace with exactly one mismatch
    let ws = make_workspace_with_mismatch();

    // WHEN check-naming-consistency is run
    let output = Command::cargo_bin("xtask")
        .unwrap()
        .current_dir(ws.path())
        .args(["check-naming-consistency"])
        .output()
        .expect("failed to run xtask");

    // THEN stdout reports 1 mismatch
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("1 mismatch"), "expected '1 mismatch' in stdout; got: {stdout}");
}

// ---------------------------------------------------------------------------
// Scenario: non-Rust directory skipped
// ---------------------------------------------------------------------------

#[test]
fn non_rust_directory_skipped_not_an_error() {
    // GIVEN a workspace with a non-Rust crates/ subdirectory (no Cargo.toml)
    let ws = make_workspace_with_non_rust_dir();

    // WHEN check-naming-consistency is run
    let output = Command::cargo_bin("xtask")
        .unwrap()
        .current_dir(ws.path())
        .args(["check-naming-consistency"])
        .output()
        .expect("failed to run xtask");

    // THEN the command exits 0 (skipped dirs are not errors)
    assert!(
        output.status.success(),
        "expected exit 0 when non-Rust dir is skipped; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn non_rust_directory_appears_in_skipped_notice() {
    // GIVEN a workspace with crates/tree-sitter-perl (no Cargo.toml)
    let ws = make_workspace_with_non_rust_dir();

    // WHEN check-naming-consistency is run
    let output = Command::cargo_bin("xtask")
        .unwrap()
        .current_dir(ws.path())
        .args(["check-naming-consistency"])
        .output()
        .expect("failed to run xtask");

    // THEN stdout mentions the skipped directory
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("tree-sitter-perl"), "expected skipped dir in stdout; got: {stdout}");
}

// ---------------------------------------------------------------------------
// Scenario: multiple mismatches
// ---------------------------------------------------------------------------

#[test]
fn multiple_mismatches_all_reported() {
    // GIVEN a workspace with two mismatched crate directories
    let ws = make_workspace_with_multiple_mismatches();

    // WHEN check-naming-consistency is run
    let output = Command::cargo_bin("xtask")
        .unwrap()
        .current_dir(ws.path())
        .args(["check-naming-consistency"])
        .output()
        .expect("failed to run xtask");

    // THEN stdout reports both mismatches
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("a-dir"), "expected a-dir in output; got: {stdout}");
    assert!(stdout.contains("a-package"), "expected a-package in output; got: {stdout}");
    assert!(stdout.contains("b-dir"), "expected b-dir in output; got: {stdout}");
    assert!(stdout.contains("b-package"), "expected b-package in output; got: {stdout}");
}

#[test]
fn multiple_mismatches_count_in_output() {
    // GIVEN a workspace with two mismatched crate directories
    let ws = make_workspace_with_multiple_mismatches();

    // WHEN check-naming-consistency is run
    let output = Command::cargo_bin("xtask")
        .unwrap()
        .current_dir(ws.path())
        .args(["check-naming-consistency"])
        .output()
        .expect("failed to run xtask");

    // THEN stdout reports the total count as 2
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("2 mismatch"), "expected '2 mismatch(es)' in stdout; got: {stdout}");
}

// ---------------------------------------------------------------------------
// Scenario: --root flag selects workspace root
// ---------------------------------------------------------------------------

#[test]
fn root_flag_overrides_current_directory() {
    // GIVEN a consistent workspace at an explicit path
    let ws = make_consistent_workspace();

    // WHEN check-naming-consistency is invoked from a DIFFERENT directory
    // but with --root pointing to the workspace
    let tmp_cwd = TempDir::new().unwrap();
    let output = Command::cargo_bin("xtask")
        .unwrap()
        .current_dir(tmp_cwd.path())
        .args(["check-naming-consistency", "--root", ws.path().to_str().unwrap()])
        .output()
        .expect("failed to run xtask");

    // THEN the command succeeds (it found the crates/ under --root)
    assert!(
        output.status.success(),
        "expected exit 0 with explicit --root; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

// ---------------------------------------------------------------------------
// Scenario: real workspace passes
// ---------------------------------------------------------------------------

#[test]
fn real_workspace_passes() {
    // GIVEN the actual perl-lsp workspace
    // WHEN check-naming-consistency is run from the workspace root
    let mut workspace_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    workspace_root.pop(); // xtask/ -> workspace root

    let output = Command::cargo_bin("xtask")
        .unwrap()
        .current_dir(&workspace_root)
        .args(["check-naming-consistency"])
        .output()
        .expect("failed to run xtask");

    // THEN it exits 0 (all crate directories match their package names)
    assert!(
        output.status.success(),
        "real workspace should pass check-naming-consistency; stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
