//! Integration tests for `cargo xtask check-naming-consistency` (issue #2933 AC#3).
//!
//! These tests exercise the real xtask binary via `assert_cmd`, following the
//! established BDD pattern in `fmt_package_lookup_tests.rs`.  Each test describes
//! a concrete scenario and verifies the observable CLI behaviour: exit code,
//! stdout content, and stderr cleanliness.
//!
//! Fixture setup and command execution propagate errors with `?` rather than
//! `unwrap`/`expect`: the workspace denies `clippy::unwrap_used` and
//! `clippy::expect_used` for every target, and `fmt_package_lookup_tests.rs`
//! — the closest precedent — is written the same way.

use assert_cmd::Command;
use color_eyre::eyre::Result;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Fixture helpers
// ---------------------------------------------------------------------------

/// Write `crates/<dir_name>/` with a `Cargo.toml` declaring `pkg_name`.
fn write_crate(root: &Path, dir_name: &str, pkg_name: &str) -> Result<()> {
    let crate_dir = root.join("crates").join(dir_name);
    fs::create_dir_all(crate_dir.join("src"))?;
    fs::write(
        crate_dir.join("Cargo.toml"),
        format!("[package]\nname = \"{pkg_name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
    )?;
    fs::write(crate_dir.join("src").join("lib.rs"), "")?;
    Ok(())
}

/// Write the workspace root manifest listing `members`.
///
/// The `[workspace]` key is load-bearing: with `--root` omitted the command
/// searches upward for the enclosing workspace manifest, so this is what stops
/// the search at the fixture instead of escaping to the real repository.
fn write_workspace_root(root: &Path, members: &[&str]) -> Result<()> {
    let members_list =
        members.iter().map(|m| format!("\"crates/{m}\"")).collect::<Vec<_>>().join(", ");
    fs::write(
        root.join("Cargo.toml"),
        format!("[workspace]\nmembers = [{members_list}]\nresolver = \"2\"\n"),
    )?;
    Ok(())
}

/// Run `check-naming-consistency` from `cwd` with the given extra arguments.
fn run_check(cwd: &Path, extra_args: &[&str]) -> Result<std::process::Output> {
    let mut args = vec!["check-naming-consistency"];
    args.extend_from_slice(extra_args);
    let output = Command::cargo_bin("xtask")?.current_dir(cwd).args(args).output()?;
    Ok(output)
}

/// Create a minimal synthetic workspace where every `crates/<dir>` name matches.
fn make_consistent_workspace() -> Result<TempDir> {
    let dir = TempDir::new()?;
    let root = dir.path();
    write_workspace_root(root, &["perl-parser", "perl-lexer"])?;
    for name in ["perl-parser", "perl-lexer"] {
        write_crate(root, name, name)?;
    }
    Ok(dir)
}

/// Create a workspace with one mismatch: `crates/perl-lsp/` has `name = "perl-lsp-rs"`.
fn make_workspace_with_mismatch() -> Result<TempDir> {
    let dir = TempDir::new()?;
    let root = dir.path();
    write_workspace_root(root, &["perl-parser", "perl-lsp"])?;
    write_crate(root, "perl-parser", "perl-parser")?;
    // Directory is `perl-lsp`, package name is `perl-lsp-rs` — the exact
    // mismatch pattern that motivated #4512.
    write_crate(root, "perl-lsp", "perl-lsp-rs")?;
    Ok(dir)
}

/// Create a workspace containing a non-Rust directory under `crates/`.
fn make_workspace_with_non_rust_dir() -> Result<TempDir> {
    let dir = TempDir::new()?;
    let root = dir.path();
    write_workspace_root(root, &["perl-parser"])?;
    write_crate(root, "perl-parser", "perl-parser")?;
    // Non-Rust directory (e.g. a JavaScript tree-sitter grammar).
    fs::create_dir_all(root.join("crates").join("tree-sitter-perl"))?;
    Ok(dir)
}

/// Create a workspace with multiple mismatches.
fn make_workspace_with_multiple_mismatches() -> Result<TempDir> {
    let dir = TempDir::new()?;
    let root = dir.path();
    write_workspace_root(root, &["a-dir", "b-dir"])?;
    write_crate(root, "a-dir", "a-package")?;
    write_crate(root, "b-dir", "b-package")?;
    Ok(dir)
}

// ---------------------------------------------------------------------------
// Scenario: consistent workspace → exit 0
// ---------------------------------------------------------------------------

#[test]
fn consistent_workspace_exits_zero() -> Result<()> {
    // GIVEN a workspace where every crates/<dir> has package name == dir
    let ws = make_consistent_workspace()?;

    // WHEN check-naming-consistency is run
    let output = run_check(ws.path(), &[])?;

    // THEN the command exits 0
    assert!(
        output.status.success(),
        "expected exit 0 for consistent workspace; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

#[test]
fn consistent_workspace_stdout_reports_pass() -> Result<()> {
    // GIVEN a consistent workspace
    let ws = make_consistent_workspace()?;

    // WHEN check-naming-consistency is run
    let output = run_check(ws.path(), &[])?;

    // THEN stdout reports both crates as checked and none as mismatched
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Directories checked: 2"),
        "expected both fixture crates to be counted as checked; got: {stdout}"
    );
    assert!(
        stdout.contains("All 2 crate directories have matching package names."),
        "expected the pass summary in stdout; got: {stdout}"
    );
    assert!(!stdout.contains("mismatch(es) found"), "pass output must not report a mismatch");
    Ok(())
}

// ---------------------------------------------------------------------------
// Scenario: mismatch present → exit non-zero
// ---------------------------------------------------------------------------

#[test]
fn mismatch_workspace_exits_nonzero() -> Result<()> {
    // GIVEN a workspace where one crates/<dir> has a different package name
    let ws = make_workspace_with_mismatch()?;

    // WHEN check-naming-consistency is run
    let output = run_check(ws.path(), &[])?;

    // THEN the command exits non-zero
    assert!(
        !output.status.success(),
        "expected non-zero exit for workspace with mismatch; stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    Ok(())
}

#[test]
fn mismatch_workspace_stdout_names_the_offending_directory() -> Result<()> {
    // GIVEN a workspace with a mismatch at crates/perl-lsp (package = perl-lsp-rs)
    let ws = make_workspace_with_mismatch()?;

    // WHEN check-naming-consistency is run
    let output = run_check(ws.path(), &[])?;

    // THEN stdout identifies the mismatched directory and both names
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("crates/perl-lsp"),
        "expected mismatched directory path in stdout; got: {stdout}"
    );
    assert!(stdout.contains("perl-lsp-rs"), "expected package name in stdout; got: {stdout}");
    // The consistent sibling must not be reported as a finding.
    assert!(
        !stdout.contains("directory basename : perl-parser"),
        "the matching crate must not be reported as a mismatch; got: {stdout}"
    );
    Ok(())
}

#[test]
fn mismatch_workspace_stdout_contains_mismatch_count() -> Result<()> {
    // GIVEN a workspace with exactly one mismatch
    let ws = make_workspace_with_mismatch()?;

    // WHEN check-naming-consistency is run
    let output = run_check(ws.path(), &[])?;

    // THEN stdout reports 1 mismatch
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("1 mismatch"), "expected '1 mismatch' in stdout; got: {stdout}");
    Ok(())
}

// ---------------------------------------------------------------------------
// Scenario: non-Rust directory skipped
// ---------------------------------------------------------------------------

#[test]
fn non_rust_directory_skipped_not_an_error() -> Result<()> {
    // GIVEN a workspace with a non-Rust crates/ subdirectory (no Cargo.toml)
    let ws = make_workspace_with_non_rust_dir()?;

    // WHEN check-naming-consistency is run
    let output = run_check(ws.path(), &[])?;

    // THEN the command exits 0 (skipped dirs are not errors)
    assert!(
        output.status.success(),
        "expected exit 0 when non-Rust dir is skipped; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

#[test]
fn non_rust_directory_appears_in_skipped_notice() -> Result<()> {
    // GIVEN a workspace with crates/tree-sitter-perl (no Cargo.toml)
    let ws = make_workspace_with_non_rust_dir()?;

    // WHEN check-naming-consistency is run
    let output = run_check(ws.path(), &[])?;

    // THEN stdout mentions the skipped directory, and does not silently count
    // it as checked
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("tree-sitter-perl"), "expected skipped dir in stdout; got: {stdout}");
    assert!(
        stdout.contains("Directories skipped (no Cargo.toml): 1"),
        "expected the skipped count to be 1; got: {stdout}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Scenario: multiple mismatches
// ---------------------------------------------------------------------------

#[test]
fn multiple_mismatches_all_reported() -> Result<()> {
    // GIVEN a workspace with two mismatched crate directories
    let ws = make_workspace_with_multiple_mismatches()?;

    // WHEN check-naming-consistency is run
    let output = run_check(ws.path(), &[])?;

    // THEN stdout reports both mismatches
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("a-dir"), "expected a-dir in output; got: {stdout}");
    assert!(stdout.contains("a-package"), "expected a-package in output; got: {stdout}");
    assert!(stdout.contains("b-dir"), "expected b-dir in output; got: {stdout}");
    assert!(stdout.contains("b-package"), "expected b-package in output; got: {stdout}");
    Ok(())
}

#[test]
fn multiple_mismatches_count_in_output() -> Result<()> {
    // GIVEN a workspace with two mismatched crate directories
    let ws = make_workspace_with_multiple_mismatches()?;

    // WHEN check-naming-consistency is run
    let output = run_check(ws.path(), &[])?;

    // THEN stdout reports the total count as 2
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("2 mismatch"), "expected '2 mismatch(es)' in stdout; got: {stdout}");
    Ok(())
}

// ---------------------------------------------------------------------------
// Scenario: non-UTF-8 crate directory must fail closed, not be dropped
// ---------------------------------------------------------------------------

#[cfg(unix)]
#[test]
fn non_utf8_crate_directory_exits_nonzero_with_actionable_error() -> Result<()> {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    // GIVEN an otherwise-consistent workspace that also contains a crate
    // directory whose name is not valid UTF-8 (0xFF can never appear in UTF-8)
    let ws = make_consistent_workspace()?;
    fs::create_dir(ws.path().join("crates").join(OsStr::from_bytes(b"bad\xFFname")))?;

    // WHEN check-naming-consistency is run
    let output = run_check(ws.path(), &[])?;

    // THEN the command fails rather than silently reporting the other two
    // directories as "all matching"
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "expected non-zero exit for an unrepresentable crate directory; stdout: {stdout}"
    );
    assert!(
        stderr.contains("not valid UTF-8") || stdout.contains("not valid UTF-8"),
        "expected an actionable diagnostic; stdout: {stdout}\nstderr: {stderr}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Scenario: --root flag selects workspace root
// ---------------------------------------------------------------------------

#[test]
fn root_flag_overrides_current_directory() -> Result<()> {
    // GIVEN a consistent workspace at an explicit path
    let ws = make_consistent_workspace()?;
    let ws_path = ws.path().to_str().ok_or_else(|| {
        color_eyre::eyre::eyre!("fixture workspace path is not valid UTF-8: {:?}", ws.path())
    })?;

    // WHEN check-naming-consistency is invoked from a DIFFERENT directory
    // but with --root pointing to the workspace
    let tmp_cwd = TempDir::new()?;
    let output = run_check(tmp_cwd.path(), &["--root", ws_path])?;

    // THEN the command succeeds (it found the crates/ under --root)
    assert!(
        output.status.success(),
        "expected exit 0 with explicit --root; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Scenario: real workspace passes
// ---------------------------------------------------------------------------

#[test]
fn real_workspace_passes() -> Result<()> {
    // GIVEN the actual perl-lsp workspace
    // WHEN check-naming-consistency is run from the workspace root
    let mut workspace_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    workspace_root.pop(); // xtask/ -> workspace root

    let output = run_check(&workspace_root, &[])?;

    // THEN it exits 0 (all crate directories match their package names)
    assert!(
        output.status.success(),
        "real workspace should pass check-naming-consistency; stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}
