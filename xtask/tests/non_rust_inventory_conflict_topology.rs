//! Conflict-topology regression for the tracked non-Rust inventory projection.
//!
//! Two independent candidates that add unrelated tracked files must not acquire
//! a shared write to `docs/policy/NON_RUST_INVENTORY.md`. The generated
//! pre-commit hook is exercised, rather than inferred from source text, because
//! the defect lives at the index/branch boundary.

use color_eyre::eyre::{Context, Result, ensure, eyre};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn project_root() -> Result<PathBuf> {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| eyre!("xtask must live below the repository root"))
}

fn bounded_first_line(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("")
        .chars()
        .take(160)
        .collect()
}

fn run(command: &mut Command, label: &str) -> Result<Output> {
    let output = command.output().with_context(|| format!("running {label}"))?;
    if !output.status.success() {
        let stderr = bounded_first_line(&output.stderr);
        let stdout = bounded_first_line(&output.stdout);
        return Err(eyre!(
            "{label} failed (status={}): stderr=`{stderr}` stdout=`{stdout}`",
            output.status
        ));
    }
    Ok(output)
}

fn git(repo: &Path, args: &[&str]) -> Result<Output> {
    run(Command::new("git").current_dir(repo).args(args), &format!("git {}", args.join(" ")))
}

fn extract_pre_commit_hook(source: &str) -> Result<&str> {
    const START: &str = "pub(super) const PRE_COMMIT_HOOK: &str = r#\"";
    let body = source
        .split_once(START)
        .map(|(_, body)| body)
        .ok_or_else(|| eyre!("generated pre-commit hook start marker is missing"))?;
    body.split_once("\n\"#;")
        .map(|(hook, _)| hook)
        .ok_or_else(|| eyre!("generated pre-commit hook end marker is missing"))
}

#[cfg(unix)]
fn write_executable(path: &Path, content: &str) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    fs::write(path, content).with_context(|| format!("writing {}", path.display()))?;
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(unix)]
fn run_hook(repo: &Path, hook: &Path, fake_bin: &Path) -> Result<()> {
    let inherited_path = std::env::var("PATH").unwrap_or_default();
    let path = format!("{}:{inherited_path}", fake_bin.display());
    run(
        Command::new("bash").current_dir(repo).arg(hook).env("PATH", path),
        "generated pre-commit hook",
    )?;
    Ok(())
}

#[cfg(unix)]
fn candidate_changed_paths(repo: &Path, branch: &str) -> Result<Vec<String>> {
    let output = git(repo, &["diff", "--name-only", "main", branch])?;
    Ok(String::from_utf8(output.stdout)?
        .lines()
        .map(str::to_owned)
        .collect())
}

#[cfg(unix)]
#[test]
fn independent_candidates_do_not_share_the_inventory_snapshot_write() -> Result<()> {
    let root = project_root()?;
    let hook_source = fs::read_to_string(root.join("crates/perl-ci-hygiene/src/git_hooks.rs"))?;
    let hook = extract_pre_commit_hook(&hook_source)?;

    let fixture = tempfile::tempdir()?;
    let repo = fixture.path();
    git(repo, &["init", "--quiet"])?;
    git(repo, &["branch", "-M", "main"])?;
    git(repo, &["config", "user.name", "Topology Fixture"])?;
    git(repo, &["config", "user.email", "topology@example.com"])?;

    fs::create_dir_all(repo.join("docs/policy"))?;
    fs::create_dir_all(repo.join("policy"))?;
    fs::write(repo.join("README.md"), "baseline\n")?;
    fs::write(
        repo.join("docs/policy/NON_RUST_INVENTORY.md"),
        "# baseline publication snapshot\n",
    )?;
    fs::write(repo.join("policy/non-rust-allowlist.toml"), "# fixture allowlist\n")?;
    git(repo, &["add", "."])?;
    git(repo, &["commit", "--quiet", "-m", "baseline"])?;

    let fake_bin = repo.join("fake-bin");
    fs::create_dir_all(&fake_bin)?;
    write_executable(
        &fake_bin.join("cargo"),
        r#"#!/usr/bin/env bash
set -euo pipefail
case "$*" in
  "xtask non-rust inventory --write")
    mkdir -p docs/policy
    {
      printf '# generated from this candidate\n'
      git ls-files
    } > docs/policy/NON_RUST_INVENTORY.md
    ;;
esac
"#,
    )?;
    let hook_path = repo.join("pre-commit");
    write_executable(&hook_path, hook)?;

    git(repo, &["checkout", "--quiet", "-b", "candidate-a", "main"])?;
    fs::write(repo.join("candidate-a.json"), "{}\n")?;
    git(repo, &["add", "candidate-a.json"])?;
    run_hook(repo, &hook_path, &fake_bin)?;
    git(repo, &["commit", "--quiet", "-m", "candidate a"])?;

    git(repo, &["checkout", "--quiet", "main"])?;
    git(repo, &["checkout", "--quiet", "-b", "candidate-b"])?;
    fs::write(repo.join("candidate-b.json"), "{}\n")?;
    git(repo, &["add", "candidate-b.json"])?;
    run_hook(repo, &hook_path, &fake_bin)?;
    git(repo, &["commit", "--quiet", "-m", "candidate b"])?;

    let a_paths = candidate_changed_paths(repo, "candidate-a")?;
    let b_paths = candidate_changed_paths(repo, "candidate-b")?;
    let snapshot = "docs/policy/NON_RUST_INVENTORY.md";
    ensure!(
        !a_paths.iter().any(|path| path == snapshot)
            && !b_paths.iter().any(|path| path == snapshot),
        "independent candidates acquired the shared inventory write: candidate-a={a_paths:?}, candidate-b={b_paths:?}"
    );

    let merge = Command::new("git")
        .current_dir(repo)
        .args(["merge-tree", "--write-tree", "candidate-a", "candidate-b"])
        .output()
        .context("running git merge-tree for independent candidates")?;
    ensure!(
        merge.status.success(),
        "unrelated candidates conflict after the generated hook: stderr=`{}` stdout=`{}`",
        bounded_first_line(&merge.stderr),
        bounded_first_line(&merge.stdout)
    );

    Ok(())
}
