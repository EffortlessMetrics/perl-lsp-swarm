//! End-to-end checks for the committed builtin semantic catalog and status.

use assert_cmd::Command;
use color_eyre::eyre::{Result, bail, eyre};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Output;

const CATALOG_RELATIVE: &str = "contracts/compiler/perl_builtin_semantics.v1.toml";
const STATUS_RELATIVE: &str = "docs/project/status/perl_builtin_semantics.md";

fn repo_root() -> Result<&'static Path> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or_else(|| eyre!("xtask manifest must have a repository parent"))
}

fn run_check(
    repo_root: &Path,
    catalog: Option<&Path>,
    status: Option<&Path>,
) -> Result<Output> {
    let mut command = Command::cargo_bin("compiler-builtin-catalog")?;
    command.current_dir(repo_root).arg("--check");
    if let Some(catalog) = catalog {
        command.arg("--catalog").arg(catalog);
    }
    if let Some(status) = status {
        command.arg("--status").arg(status);
    }
    Ok(command.output()?)
}

fn expect_success(output: Output, description: &str) -> Result<()> {
    if output.status.success() {
        return Ok(());
    }
    bail!(
        "{description} failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn expect_failure(output: Output, description: &str) -> Result<()> {
    if !output.status.success() {
        return Ok(());
    }
    bail!("{description} unexpectedly succeeded")
}

fn committed_paths(root: &Path) -> (PathBuf, PathBuf) {
    (root.join(CATALOG_RELATIVE), root.join(STATUS_RELATIVE))
}

#[test]
fn committed_builtin_status_is_current_through_cli() -> Result<()> {
    let root = repo_root()?;
    expect_success(run_check(root, None, None)?, "committed catalog check")
}

#[test]
fn changed_catalog_row_is_rejected_against_committed_status() -> Result<()> {
    let root = repo_root()?;
    let (catalog, status) = committed_paths(root);
    let temp = tempfile::tempdir()?;
    let changed_catalog = temp.path().join("changed-catalog.toml");
    let source = fs::read_to_string(&catalog)?;
    let changed = source.replace(
        "name = \"defined\"",
        "name = \"changed_defined\"",
    );
    if changed == source {
        bail!("catalog mutation marker was not found")
    }
    fs::write(&changed_catalog, changed)?;
    expect_failure(
        run_check(root, Some(&changed_catalog), Some(&status))?,
        "changed catalog row check",
    )
}

#[test]
fn changed_generated_status_is_rejected_against_committed_catalog() -> Result<()> {
    let root = repo_root()?;
    let (catalog, status) = committed_paths(root);
    let temp = tempfile::tempdir()?;
    let changed_status = temp.path().join("changed-status.md");
    let source = fs::read_to_string(&status)?;
    fs::write(&changed_status, format!("{source}\n<!-- stale -->\n"))?;
    expect_failure(
        run_check(root, Some(&catalog), Some(&changed_status))?,
        "changed generated status check",
    )
}

#[test]
fn missing_catalog_is_rejected() -> Result<()> {
    let root = repo_root()?;
    let (_, status) = committed_paths(root);
    let temp = tempfile::tempdir()?;
    let missing_catalog = temp.path().join("missing-catalog.toml");
    expect_failure(
        run_check(root, Some(&missing_catalog), Some(&status))?,
        "missing catalog check",
    )
}

#[test]
fn disconnected_fixture_path_cannot_satisfy_catalog_check() -> Result<()> {
    let root = repo_root()?;
    let (_, status) = committed_paths(root);
    let temp = tempfile::tempdir()?;
    let disconnected_fixture = temp.path().join("disconnected-fixture.toml");
    fs::write(
        &disconnected_fixture,
        "schema_version = \"fixture-only\"\ncatalog_id = \"disconnected\"\n",
    )?;
    expect_failure(
        run_check(root, Some(&disconnected_fixture), Some(&status))?,
        "disconnected fixture check",
    )
}
