//! Process-level proof that the `git-ancestry` CLI maps dispositions to the
//! documented typed exit codes.
//!
//! Fixture classification (#13697): identity-pinning and hermetic. The fixture
//! commits are built through `git_test_support::HermeticGit` and the CLI child
//! inherits the same hermetic environment, so ambient signing, hooks, filters,
//! object-format defaults, and locale cannot perturb the commit identities the
//! dispositions are asserted against.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

mod git_test_support;

use git_test_support::HermeticGit;

#[test]
fn ancestor_exits_zero() -> Result<()> {
    let (tmp, hermetic) = initialized_repository()?;
    let repository = tmp.path().join("repo");
    let base = hermetic.git(&repository, &["rev-parse", "HEAD"])?;
    commit_file(&hermetic, &repository, "second.txt", "second\n", "second")?;

    let output = run_cli(&hermetic, &repository, &base, "HEAD", &[])?;

    assert_eq!(output.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&output.stdout).contains("git-ancestry: ancestor"));
    Ok(())
}

#[test]
fn unrelated_exits_two() -> Result<()> {
    let (tmp, hermetic) = initialized_repository()?;
    let repository = tmp.path().join("repo");
    let original = hermetic.git(&repository, &["rev-parse", "HEAD"])?;
    hermetic.git(&repository, &["switch", "--orphan", "orphan"])?;
    hermetic.git(&repository, &["rm", "-rf", "--ignore-unmatch", "."])?;
    commit_file(&hermetic, &repository, "orphan.txt", "orphan\n", "orphan")?;

    let output = run_cli(&hermetic, &repository, &original, "HEAD", &[])?;

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stdout).contains("git-ancestry: unrelated"));
    Ok(())
}

#[test]
fn invalid_input_exits_four() -> Result<()> {
    let (tmp, hermetic) = initialized_repository()?;
    let repository = tmp.path().join("repo");

    let output = run_cli(&hermetic, &repository, "", "HEAD", &["--json"])?;

    assert_eq!(output.status.code(), Some(4));
    assert!(String::from_utf8_lossy(&output.stdout).contains("\"invalid_input\""));
    Ok(())
}

fn run_cli(
    hermetic: &HermeticGit,
    repository: &Path,
    base: &str,
    head: &str,
    extra: &[&str],
) -> Result<std::process::Output> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_git-ancestry"));
    command
        .arg("--repo")
        .arg(repository)
        .arg("--base")
        .arg(base)
        .arg("--head")
        .arg(head)
        .args(extra);
    hermetic.apply_env(&mut command);
    command.output().context("failed to execute git-ancestry CLI")
}

fn initialized_repository() -> Result<(TempDir, HermeticGit)> {
    let tmp = tempfile::tempdir()?;
    let hermetic = HermeticGit::at(&tmp.path().join("git-fixture-pins"))?;
    let repository: PathBuf = tmp.path().join("repo");
    hermetic.init_repo(&repository)?;
    commit_file(&hermetic, &repository, "tracked.txt", "base\n", "base")?;
    Ok((tmp, hermetic))
}

fn commit_file(
    hermetic: &HermeticGit,
    repository: &Path,
    path: &str,
    contents: &str,
    message: &str,
) -> Result<()> {
    std::fs::write(repository.join(path), contents)?;
    hermetic.git(repository, &["add", "--", path])?;
    hermetic.git(repository, &["commit", "-m", message])?;
    Ok(())
}
