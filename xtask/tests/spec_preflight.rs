//! Integration tests for `cargo xtask spec preflight`.
//!
//! Each test creates an isolated git repository (or two-repo remote/clone
//! setup) using `tempfile` and `std::process::Command`, then invokes the
//! xtask binary and asserts on exit codes and stderr/stdout content.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use anyhow::Result;
use assert_cmd::cargo::cargo_bin_cmd;
use std::{
    fs,
    path::Path,
    process::{Command, Stdio},
};
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Helpers (mirrors xtask/tests/freshness_check.rs conventions)
// ---------------------------------------------------------------------------

/// Initialize a bare "remote" repo with one commit and a working clone of it.
/// Returns `(remote_dir, work_dir)`.
fn init_synced_repo() -> Result<(TempDir, TempDir)> {
    let remote_dir = TempDir::new()?;
    let work_dir = TempDir::new()?;

    git_cmd(
        &["init", "--bare", "--initial-branch=master", remote_dir.path().to_str().expect("path")],
        None,
    )
    .or_else(|_| {
        git_cmd(&["init", "--bare", remote_dir.path().to_str().expect("path")], None)?;
        git_cmd(&["symbolic-ref", "HEAD", "refs/heads/master"], Some(remote_dir.path()))
    })?;

    let source_dir = TempDir::new()?;
    git_cmd(&["init", "-b", "master", source_dir.path().to_str().expect("path")], None)
        .or_else(|_| git_cmd(&["init", source_dir.path().to_str().expect("path")], None))?;
    git_cmd(&["config", "user.email", "test@test.com"], Some(source_dir.path()))?;
    git_cmd(&["config", "user.name", "Test"], Some(source_dir.path()))?;
    git_cmd(&["config", "commit.gpgsign", "false"], Some(source_dir.path()))?;
    let _ = git_cmd(&["checkout", "-b", "master"], Some(source_dir.path()));
    fs::write(source_dir.path().join("README.md"), "init")?;
    fs::write(source_dir.path().join("watched.rs"), "fn watched() {}\n")?;
    git_cmd(&["add", "."], Some(source_dir.path()))?;
    git_cmd(&["commit", "-m", "init"], Some(source_dir.path()))?;
    git_cmd(
        &["remote", "add", "origin", remote_dir.path().to_str().expect("path")],
        Some(source_dir.path()),
    )?;
    git_cmd(&["push", "origin", "HEAD:master"], Some(source_dir.path()))?;

    git_cmd(
        &[
            "clone",
            remote_dir.path().to_str().expect("path"),
            work_dir.path().to_str().expect("path"),
        ],
        None,
    )?;
    git_cmd(&["config", "user.email", "test@test.com"], Some(work_dir.path()))?;
    git_cmd(&["config", "user.name", "Test"], Some(work_dir.path()))?;
    git_cmd(&["config", "commit.gpgsign", "false"], Some(work_dir.path()))?;
    let _ = git_cmd(&["checkout", "master"], Some(work_dir.path()));

    Ok((remote_dir, work_dir))
}

fn add_commit(repo_dir: &Path, message: &str) -> Result<()> {
    let file = repo_dir.join(format!("file_{}.txt", message.replace(' ', "_")));
    fs::write(&file, message)?;
    git_cmd(&["add", "."], Some(repo_dir))?;
    git_cmd(&["commit", "-m", message], Some(repo_dir))?;
    Ok(())
}

fn modify_watched_and_commit(repo_dir: &Path, message: &str) -> Result<()> {
    fs::write(repo_dir.join("watched.rs"), format!("fn watched() {{ /* {message} */ }}\n"))?;
    git_cmd(&["add", "."], Some(repo_dir))?;
    git_cmd(&["commit", "-m", message], Some(repo_dir))?;
    Ok(())
}

fn push_to_remote(repo_dir: &Path) -> Result<()> {
    git_cmd(&["push", "origin", "HEAD:master"], Some(repo_dir))?;
    Ok(())
}

fn clone_of(remote_dir: &Path) -> Result<TempDir> {
    let clone_dir = TempDir::new()?;
    git_cmd(
        &["clone", remote_dir.to_str().expect("path"), clone_dir.path().to_str().expect("path")],
        None,
    )?;
    git_cmd(&["config", "user.email", "test@test.com"], Some(clone_dir.path()))?;
    git_cmd(&["config", "user.name", "Test"], Some(clone_dir.path()))?;
    git_cmd(&["config", "commit.gpgsign", "false"], Some(clone_dir.path()))?;
    Ok(clone_dir)
}

fn git_cmd(args: &[&str], cwd: Option<&Path>) -> Result<()> {
    let mut cmd = Command::new("git");
    cmd.args(args).stdout(Stdio::null()).stderr(Stdio::piped());
    cmd.env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_INDEX_FILE");
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    let output = cmd.output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git {:?} failed ({}): {}", args, output.status, stderr.trim());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// A freshly-cloned repo (HEAD == remote master, watched path untouched)
/// passes with exit 0.
#[test]
fn clean_checkout_passes() -> Result<()> {
    let (_remote, work_dir) = init_synced_repo()?;

    let mut cmd = cargo_bin_cmd!("xtask");
    cmd.current_dir(work_dir.path())
        .args([
            "spec",
            "preflight",
            "--base",
            "origin/master",
            "--no-fetch",
            "--paths",
            "watched.rs",
        ])
        .assert()
        .success();

    Ok(())
}

/// When the remote has commits HEAD hasn't fetched/merged, preflight fails
/// with exit 1 and a machine-readable STALE line reporting `behind_by`.
#[test]
fn behind_base_fails_with_behind_count() -> Result<()> {
    let (remote_dir, work_dir) = init_synced_repo()?;

    let source2 = clone_of(remote_dir.path())?;
    add_commit(source2.path(), "remote change")?;
    push_to_remote(source2.path())?;
    git_cmd(&["fetch", "origin"], Some(work_dir.path()))?;

    let mut cmd = cargo_bin_cmd!("xtask");
    let output = cmd
        .current_dir(work_dir.path())
        .args(["spec", "preflight", "--base", "origin/master", "--no-fetch"])
        .assert()
        .failure()
        .code(1)
        .get_output()
        .stderr
        .clone();

    let stderr = String::from_utf8(output)?;
    assert!(stderr.contains("STALE: behind=1 paths_changed=0"), "stderr was: {stderr}");

    Ok(())
}

/// When a `--paths` entry changed on `--base` since the merge-base, preflight
/// fails with exit 1 and reports the changed path and count, even though the
/// commit itself may be unrelated to other tracked files.
#[test]
fn path_changed_on_base_fails() -> Result<()> {
    let (remote_dir, work_dir) = init_synced_repo()?;

    let source2 = clone_of(remote_dir.path())?;
    modify_watched_and_commit(source2.path(), "touch watched")?;
    push_to_remote(source2.path())?;
    git_cmd(&["fetch", "origin"], Some(work_dir.path()))?;

    let mut cmd = cargo_bin_cmd!("xtask");
    let output = cmd
        .current_dir(work_dir.path())
        .args([
            "spec",
            "preflight",
            "--base",
            "origin/master",
            "--no-fetch",
            "--paths",
            "watched.rs",
        ])
        .assert()
        .failure()
        .code(1)
        .get_output()
        .stderr
        .clone();

    let stderr = String::from_utf8(output)?;
    assert!(stderr.contains("watched.rs"), "stderr was: {stderr}");
    assert!(stderr.contains("STALE: behind=1 paths_changed=1"), "stderr was: {stderr}");

    Ok(())
}

/// A `--paths` entry that does not exist at HEAD (a typo) fails fast with
/// exit 2, distinct from the exit-1 staleness path.
#[test]
fn missing_path_at_head_errors_with_exit_2() -> Result<()> {
    let (_remote, work_dir) = init_synced_repo()?;

    let mut cmd = cargo_bin_cmd!("xtask");
    let output = cmd
        .current_dir(work_dir.path())
        .args([
            "spec",
            "preflight",
            "--base",
            "origin/master",
            "--no-fetch",
            "--paths",
            "does/not/exist.rs",
        ])
        .assert()
        .failure()
        .code(2)
        .get_output()
        .stderr
        .clone();

    let stderr = String::from_utf8(output)?;
    assert!(stderr.contains("does/not/exist.rs"), "stderr was: {stderr}");

    Ok(())
}

/// A `--base` naming a remote that isn't configured fails fast with exit 2
/// and never attempts a fetch.
#[test]
fn unconfigured_remote_errors_with_exit_2() -> Result<()> {
    let (_remote, work_dir) = init_synced_repo()?;

    let mut cmd = cargo_bin_cmd!("xtask");
    cmd.current_dir(work_dir.path())
        .args(["spec", "preflight", "--base", "nonexistent-remote/master"])
        .assert()
        .failure()
        .code(2);

    Ok(())
}
