//! Process-level proof that the `main-history-event` CLI maps delivered push
//! events to the documented typed exit codes and always writes its receipt.

use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::path::Path;
use std::process::{Command, Output};

const ZERO: &str = "0000000000000000000000000000000000000000";

#[test]
fn fast_forward_push_exits_zero() -> Result<()> {
    let repository = initialized_repository()?;
    let before = git(repository.path(), &["rev-parse", "HEAD"])?;
    commit_file(repository.path(), "second.txt", "second\n", "second")?;

    let output = run_cli(repository.path(), &before, "HEAD", &[])?;

    assert_eq!(output.status.code(), Some(0));
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("main-history-event: fast_forward"),
        "stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    Ok(())
}

#[test]
fn re_rooted_push_exits_two() -> Result<()> {
    let repository = initialized_repository()?;
    let before = git(repository.path(), &["rev-parse", "HEAD"])?;
    git(repository.path(), &["switch", "--orphan", "rebuilt"])?;
    git(repository.path(), &["rm", "-rf", "--ignore-unmatch", "."])?;
    commit_file(repository.path(), "rebuilt.txt", "rebuilt\n", "rebuilt")?;

    let output = run_cli(repository.path(), &before, "HEAD", &["--event-forced"])?;

    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("main-history-event: non_fast_forward")
    );
    Ok(())
}

/// A shallow checkout must exit `not_proven` rather than claiming either a clean
/// fast-forward or a rewrite.
#[test]
fn shallow_checkout_exits_three() -> Result<()> {
    let source = initialized_repository()?;
    commit_file(source.path(), "second.txt", "second\n", "second")?;
    commit_file(source.path(), "third.txt", "third\n", "third")?;
    let clone_parent = tempfile::tempdir()?;
    let clone = clone_parent.path().join("repository");
    let source_argument = source.path().to_string_lossy().into_owned();
    let clone_argument = clone.to_string_lossy().into_owned();
    git(
        clone_parent.path(),
        &["clone", "--depth", "1", "--no-local", &source_argument, &clone_argument],
    )?;

    let output = run_cli(&clone, "HEAD~2", "HEAD", &["--json"])?;

    assert_eq!(output.status.code(), Some(3));
    let receipt: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(receipt["verdict"], "not_proven");
    assert_eq!(receipt["graph_disposition"], "not_proven_shallow");
    Ok(())
}

#[test]
fn incoherent_event_exits_four() -> Result<()> {
    let repository = initialized_repository()?;

    let output = run_cli(repository.path(), ZERO, ZERO, &["--json"])?;

    assert_eq!(output.status.code(), Some(4));
    let receipt: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(receipt["verdict"], "invalid_event");
    Ok(())
}

/// The receipt is the durable evidence a red detector run has to leave behind,
/// so it must be written even when the process reports a blocking verdict.
#[test]
fn receipt_is_written_for_a_blocking_verdict() -> Result<()> {
    let repository = initialized_repository()?;
    let first = git(repository.path(), &["rev-parse", "HEAD"])?;
    commit_file(repository.path(), "second.txt", "second\n", "second")?;
    let second = git(repository.path(), &["rev-parse", "HEAD"])?;
    let receipt_directory = tempfile::tempdir()?;
    // A nested path the command has to create for itself.
    let receipt_path = receipt_directory.path().join("history").join("main-event.json");

    let output = run_cli(
        repository.path(),
        &second,
        &first,
        &["--output", &receipt_path.to_string_lossy()],
    )?;

    assert_eq!(output.status.code(), Some(2), "an unforced rewind must block");
    let written = std::fs::read_to_string(&receipt_path)
        .context("the blocking run must still leave its receipt behind")?;
    let receipt: Value = serde_json::from_str(&written)?;
    assert_eq!(receipt["schema_version"], "main_history_event.v1");
    assert_eq!(receipt["verdict"], "non_fast_forward");
    assert_eq!(receipt["agreement"], "contradicts");
    assert_eq!(receipt["reference"], "refs/heads/main");
    Ok(())
}

/// The receipt keeps GitHub's delivered flags beside the independently proven
/// graph, so neither axis can be reconstructed from the other.
#[test]
fn receipt_retains_both_the_event_and_graph_axes() -> Result<()> {
    let repository = initialized_repository()?;
    let before = git(repository.path(), &["rev-parse", "HEAD"])?;
    commit_file(repository.path(), "second.txt", "second\n", "second")?;

    let output = run_cli(repository.path(), &before, "HEAD", &["--event-forced", "--json"])?;

    assert_eq!(output.status.code(), Some(0));
    let receipt: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(receipt["event_shape"], "forced");
    assert_eq!(receipt["event_forced"], true);
    assert_eq!(receipt["graph_disposition"], "ancestor");
    assert_eq!(receipt["verdict"], "fast_forward");
    assert!(receipt["graph"]["merge_base"].is_string());
    assert_eq!(receipt["graph"]["is_shallow_repository"], false);
    Ok(())
}

fn run_cli(repository: &Path, before: &str, after: &str, extra: &[&str]) -> Result<Output> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_main-history-event"));
    command
        .arg("--repo")
        .arg(repository)
        .arg("--before")
        .arg(before)
        .arg("--after")
        .arg(after)
        .arg("--ref")
        .arg("refs/heads/main")
        .args(extra);
    command.output().context("failed to execute main-history-event CLI")
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
