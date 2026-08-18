//! Process-level proof that the `git-ancestry` CLI maps dispositions to the
//! documented typed exit codes.

use anyhow::{Context, Result, bail};
use std::path::Path;
use std::process::Command;

#[test]
fn ancestor_exits_zero() -> Result<()> {
    let repository = initialized_repository()?;
    let base = git(repository.path(), &["rev-parse", "HEAD"])?;
    commit_file(repository.path(), "second.txt", "second\n", "second")?;

    let output = run_cli(repository.path(), &base, "HEAD", &[])?;

    assert_eq!(output.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&output.stdout).contains("git-ancestry: ancestor"));
    Ok(())
}

#[test]
fn unrelated_exits_two() -> Result<()> {
    let repository = initialized_repository()?;
    let original = git(repository.path(), &["rev-parse", "HEAD"])?;
    git(repository.path(), &["switch", "--orphan", "orphan"])?;
    git(repository.path(), &["rm", "-rf", "--ignore-unmatch", "."])?;
    commit_file(repository.path(), "orphan.txt", "orphan\n", "orphan")?;

    let output = run_cli(repository.path(), &original, "HEAD", &[])?;

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stdout).contains("git-ancestry: unrelated"));
    Ok(())
}

#[test]
fn invalid_input_exits_four() -> Result<()> {
    let repository = initialized_repository()?;

    let output = run_cli(repository.path(), "", "HEAD", &["--json"])?;

    assert_eq!(output.status.code(), Some(4));
    assert!(String::from_utf8_lossy(&output.stdout).contains("\"invalid_input\""));
    Ok(())
}

fn run_cli(
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
    command.output().context("failed to execute git-ancestry CLI")
}

fn initialized_repository() -> Result<tempfile::TempDir> {
    let repository = tempfile::tempdir()?;
    git(repository.path(), &["init", "--initial-branch", "main"])?;
    git(repository.path(), &["config", "user.name", "test"])?;
    git(repository.path(), &["config", "user.email", "test@example.com"])?;
    commit_file(repository.path(), "tracked.txt", "base\n", "base")?;
    Ok(repository)
}

fn commit_file(repository: &Path, path: &str, contents: &str, message: &str) -> Result<()> {
    std::fs::write(repository.join(path), contents)?;
    git(repository, &["add", "--", path])?;
    git(repository, &["commit", "-m", message])?;
    Ok(())
}

fn git(repository: &Path, arguments: &[&str]) -> Result<String> {
    let output = Command::new("git").args(arguments).current_dir(repository).output()?;
    if !output.status.success() {
        bail!(
            "git {} failed with status {}\nstderr:\n{}",
            arguments.join(" "),
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    String::from_utf8(output.stdout)
        .context("git command returned non-UTF-8 output")
        .map(|value| value.trim().to_string())
}
