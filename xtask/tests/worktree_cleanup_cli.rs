// Worktree-cleanup CLI integration tests (#4097).
//
// Runs the real `cargo xtask worktree-cleanup --root ...` entry point
// against fixture git repositories with real linked worktrees (dirty,
// locked, clean-with-open-PR, clean-with-no-PR), asserting that the guard
// never force-removes a dirty / locked / open-PR / root worktree, and that
// a clean worktree with an affirmative "no open PR" answer from `gh` is
// removed only under `--force` (never under the dry-run default).
//
// `gh` is stubbed via the `XTASK_WORKTREE_CLEANUP_GH_BIN` env var override
// (see xtask/src/tasks/worktrees.rs) rather than PATH manipulation, so the
// exact stub binary (with the correct platform extension) is resolved
// deterministically regardless of PATH/PATHEXT search order.
use anyhow::{Result, bail};
use assert_cmd::cargo::cargo_bin_cmd;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const GH_BIN_ENV: &str = "XTASK_WORKTREE_CLEANUP_GH_BIN";

fn run_git(dir: &Path, args: &[&str]) -> Result<()> {
    let status = Command::new("git").current_dir(dir).args(args).status()?;
    if !status.success() {
        bail!("git {args:?} failed in {}", dir.display());
    }
    Ok(())
}

/// Initializes a fresh git repo at `<tmp>/repo` with one commit on `main`.
fn init_fixture_repo(tmp: &Path) -> Result<PathBuf> {
    let repo = tmp.join("repo");
    fs::create_dir_all(&repo)?;
    if run_git(&repo, &["init", "-q", "-b", "main"]).is_err() {
        // Older git without `-b` support on `init`.
        run_git(&repo, &["init", "-q"])?;
    }
    run_git(&repo, &["config", "user.email", "test@test.local"])?;
    run_git(&repo, &["config", "user.name", "Test"])?;
    run_git(&repo, &["config", "commit.gpgsign", "false"])?;
    fs::write(repo.join("README.md"), "init\n")?;
    run_git(&repo, &["add", "README.md"])?;
    run_git(&repo, &["commit", "-q", "-m", "init"])?;
    Ok(repo)
}

/// Adds a linked worktree under `<repo>/.claude/worktrees/<name>` on a new
/// branch `<name>`. Returns the worktree's absolute path.
fn add_agent_worktree(repo: &Path, name: &str) -> Result<PathBuf> {
    let wt_path = repo.join(".claude").join("worktrees").join(name);
    run_git(repo, &["worktree", "add", "-q", "-b", name, &wt_path.to_string_lossy()])?;
    Ok(wt_path)
}

#[cfg(windows)]
fn write_gh_stub(dir: &Path, exit_code: i32, stdout: &str) -> Result<PathBuf> {
    let path = dir.join("gh.cmd");
    let mut body = String::from("@echo off\r\n");
    if !stdout.is_empty() {
        body.push_str(&format!("echo {stdout}\r\n"));
    }
    body.push_str(&format!("exit /b {exit_code}\r\n"));
    fs::write(&path, body)?;
    Ok(path)
}

#[cfg(unix)]
fn write_gh_stub(dir: &Path, exit_code: i32, stdout: &str) -> Result<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join("gh");
    let mut body = String::from("#!/bin/sh\n");
    if !stdout.is_empty() {
        body.push_str(&format!("echo {stdout}\n"));
    }
    body.push_str(&format!("exit {exit_code}\n"));
    fs::write(&path, body)?;
    let mut perms = fs::metadata(&path)?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&path, perms)?;
    Ok(path)
}

/// A `gh` stub that records its own invocation cwd to `marker` and reports
/// no open PR. Used to pin exactly which directory `gh` is invoked from —
/// a cwd-blind stub (like [`write_gh_stub`]) cannot catch a regression
/// where `gh` silently queries the wrong repo.
#[cfg(windows)]
fn write_cwd_probe_gh_stub(dir: &Path, marker: &Path) -> Result<PathBuf> {
    let path = dir.join("gh.cmd");
    let body = format!("@echo off\r\necho %CD%>\"{}\"\r\nexit /b 0\r\n", marker.display());
    fs::write(&path, body)?;
    Ok(path)
}

#[cfg(unix)]
fn write_cwd_probe_gh_stub(dir: &Path, marker: &Path) -> Result<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join("gh");
    let body = format!("#!/bin/sh\npwd > \"{}\"\nexit 0\n", marker.display());
    fs::write(&path, body)?;
    let mut perms = fs::metadata(&path)?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&path, perms)?;
    Ok(path)
}

fn run_xtask_cleanup(root: &Path, force: bool, gh_bin: Option<&Path>) -> Result<(bool, String)> {
    let mut cmd = cargo_bin_cmd!("xtask");
    cmd.arg("worktree-cleanup").arg("--root").arg(root);
    if force {
        cmd.arg("--force");
    }
    if let Some(gh) = gh_bin {
        cmd.env(GH_BIN_ENV, gh);
    } else {
        // Ensure no ambient override leaks in from the test-runner's own env.
        cmd.env_remove(GH_BIN_ENV);
    }
    let output = cmd.output()?;
    let combined = format!(
        "{}\n---stderr---\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok((output.status.success(), combined))
}

// ── Core safety proof: dirty survives, clean-no-PR is removed ──────────────

#[test]
fn dry_run_keeps_dirty_and_flags_clean_for_removal_without_deleting_either() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let repo = init_fixture_repo(tmp.path())?;
    let dirty_wt = add_agent_worktree(&repo, "wt-dirty")?;
    fs::write(dirty_wt.join("uncommitted.txt"), "uncommitted work\n")?;
    let clean_wt = add_agent_worktree(&repo, "wt-clean")?;

    let gh_stub_dir = tmp.path().join("gh-nopr");
    fs::create_dir_all(&gh_stub_dir)?;
    let gh_stub = write_gh_stub(&gh_stub_dir, 0, "")?;

    let (ok, output) = run_xtask_cleanup(&repo, false, Some(&gh_stub))?;
    assert!(ok, "dry-run must exit 0: {output}");
    assert!(
        output.contains("KEEP") && output.contains("dirty"),
        "expected dirty worktree to be reported KEEP with a dirty reason: {output}"
    );
    assert!(
        output.contains("REMOVE"),
        "expected clean worktree to be reported REMOVE-eligible in dry-run: {output}"
    );
    assert!(dirty_wt.exists(), "dry-run must never delete the dirty worktree");
    assert!(
        clean_wt.exists(),
        "dry-run must never delete anything, including REMOVE-eligible ones"
    );
    Ok(())
}

#[test]
fn force_removes_only_the_clean_worktree_and_never_the_dirty_one() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let repo = init_fixture_repo(tmp.path())?;
    let dirty_wt = add_agent_worktree(&repo, "wt-dirty-force")?;
    fs::write(dirty_wt.join("uncommitted.txt"), "uncommitted work\n")?;
    let clean_wt = add_agent_worktree(&repo, "wt-clean-force")?;

    let gh_stub_dir = tmp.path().join("gh-nopr");
    fs::create_dir_all(&gh_stub_dir)?;
    let gh_stub = write_gh_stub(&gh_stub_dir, 0, "")?;

    let (ok, output) = run_xtask_cleanup(&repo, true, Some(&gh_stub))?;
    assert!(ok, "--force run must exit 0: {output}");

    assert!(
        dirty_wt.exists(),
        "SAFETY VIOLATION: dirty worktree was removed under --force: {output}"
    );
    assert!(
        !clean_wt.exists(),
        "clean, no-open-PR worktree should have been removed under --force: {output}"
    );
    Ok(())
}

// ── Open-PR guard ────────────────────────────────────────────────────────

#[test]
fn open_pr_worktree_is_never_removed_even_under_force() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let repo = init_fixture_repo(tmp.path())?;
    let pr_wt = add_agent_worktree(&repo, "wt-open-pr")?;

    let gh_stub_dir = tmp.path().join("gh-openpr");
    fs::create_dir_all(&gh_stub_dir)?;
    // Reports PR #123 open for every `gh pr list --head <branch> ...` call.
    let gh_stub = write_gh_stub(&gh_stub_dir, 0, "123")?;

    let (dry_ok, dry_output) = run_xtask_cleanup(&repo, false, Some(&gh_stub))?;
    assert!(dry_ok, "dry-run must exit 0: {dry_output}");
    assert!(
        dry_output.contains("KEEP") && dry_output.contains("open PR"),
        "expected open-PR worktree to be reported KEEP with an open-PR reason: {dry_output}"
    );

    let (force_ok, force_output) = run_xtask_cleanup(&repo, true, Some(&gh_stub))?;
    assert!(force_ok, "--force run must exit 0: {force_output}");
    assert!(
        pr_wt.exists(),
        "SAFETY VIOLATION: worktree with an open PR was removed under --force: {force_output}"
    );
    Ok(())
}

// ── gh must be invoked with the classified repo as cwd, not the ambient
//    process cwd (a `--root` that differs from where the xtask process
//    happens to be launched from must not silently query the wrong repo).

#[test]
fn gh_is_invoked_with_the_classified_root_as_cwd_not_the_ambient_process_cwd() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let repo = init_fixture_repo(tmp.path())?;
    let wt = add_agent_worktree(&repo, "wt-cwd-probe")?;

    // A directory that is NOT the fixture repo — simulates the xtask
    // process being launched from somewhere other than the repo passed
    // via --root (e.g. a different worktree, or the shell's own cwd).
    let ambient_cwd = tmp.path().join("ambient-cwd-decoy");
    fs::create_dir_all(&ambient_cwd)?;

    let marker = tmp.path().join("gh-invocation-cwd.txt");
    let gh_stub_dir = tmp.path().join("gh-cwd-probe");
    fs::create_dir_all(&gh_stub_dir)?;
    let gh_stub = write_cwd_probe_gh_stub(&gh_stub_dir, &marker)?;

    let mut cmd = cargo_bin_cmd!("xtask");
    cmd.current_dir(&ambient_cwd)
        .arg("worktree-cleanup")
        .arg("--root")
        .arg(&repo)
        .arg("--force")
        .env(GH_BIN_ENV, &gh_stub);
    let output = cmd.output()?;
    let combined = format!(
        "{}\n---stderr---\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.status.success(), "run must exit 0: {combined}");

    let recorded = fs::read_to_string(&marker)
        .map_err(|e| anyhow::anyhow!("gh stub did not write its cwd marker: {e}\n{combined}"))?;
    let recorded_cwd = PathBuf::from(recorded.trim());
    let recorded_canon = recorded_cwd.canonicalize().map_err(|e| {
        anyhow::anyhow!("recorded cwd '{}' does not canonicalize: {e}", recorded_cwd.display())
    })?;
    let expected_canon = repo.canonicalize()?;

    assert_eq!(
        recorded_canon,
        expected_canon,
        "gh must be invoked with cwd = the classified repo root ({}), not the ambient \
         process cwd ({}); gh actually ran in: {}\n{combined}",
        expected_canon.display(),
        ambient_cwd.display(),
        recorded_cwd.display()
    );

    // Sanity: the stub reports "no PR", so with a correctly-scoped gh call
    // the worktree is still eligible for removal (this test is about cwd
    // correctness, not about the open-PR guard itself).
    assert!(!wt.exists(), "expected the no-PR worktree to be removed under --force: {combined}");
    Ok(())
}

// ── Locked guard ─────────────────────────────────────────────────────────

#[test]
fn locked_worktree_is_never_removed_even_under_force() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let repo = init_fixture_repo(tmp.path())?;
    let locked_wt = add_agent_worktree(&repo, "wt-locked")?;
    run_git(
        &repo,
        &[
            "worktree",
            "lock",
            "--reason",
            "claude agent wt-locked (pid 1)",
            &locked_wt.to_string_lossy(),
        ],
    )?;

    let gh_stub_dir = tmp.path().join("gh-nopr-locked");
    fs::create_dir_all(&gh_stub_dir)?;
    let gh_stub = write_gh_stub(&gh_stub_dir, 0, "")?;

    let (ok, output) = run_xtask_cleanup(&repo, true, Some(&gh_stub))?;
    assert!(ok, "--force run must exit 0: {output}");
    assert!(
        locked_wt.exists(),
        "SAFETY VIOLATION: locked worktree was removed under --force: {output}"
    );
    Ok(())
}

// ── gh-unavailable safety default ───────────────────────────────────────

#[test]
fn gh_failure_yields_unknown_pr_status_and_keeps_the_worktree() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let repo = init_fixture_repo(tmp.path())?;
    let wt = add_agent_worktree(&repo, "wt-gh-unknown")?;

    let gh_stub_dir = tmp.path().join("gh-fail");
    fs::create_dir_all(&gh_stub_dir)?;
    let gh_stub = write_gh_stub(&gh_stub_dir, 1, "")?;

    let (ok, output) = run_xtask_cleanup(&repo, true, Some(&gh_stub))?;
    assert!(ok, "--force run must exit 0: {output}");
    assert!(
        wt.exists(),
        "SAFETY VIOLATION: worktree was removed under --force when gh PR status was unknown: {output}"
    );
    assert!(
        output.contains("could not be determined") || output.contains("unknown"),
        "expected the unknown-PR-status reason to be reported: {output}"
    );
    Ok(())
}

// ── Root checkout guard ──────────────────────────────────────────────────

#[test]
fn no_agent_worktrees_reports_nothing_to_clean() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let repo = init_fixture_repo(tmp.path())?;

    let (ok, output) = run_xtask_cleanup(&repo, false, None)?;
    assert!(ok, "dry-run on a repo with no agent worktrees must exit 0: {output}");
    assert!(
        output.contains("No stale worktrees to clean up"),
        "expected an explicit nothing-to-clean message: {output}"
    );
    Ok(())
}
