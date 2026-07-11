//! `cargo xtask spec preflight` — stale-worktree guard for issue specs.
//!
//! Narrower, spec-filing-specific sibling of `freshness-check` (#1819):
//! refuses to let a spec-planner agent file a spec from a checkout that is
//! behind the canonical base ref, or where specific paths the spec names
//! have changed on that base ref since the checkout's merge-base. Catches
//! the failure mode where a spec's file paths or function names are correct
//! for the worktree but wrong for the base branch.
//!
//! # Exit codes
//! - `0` — HEAD matches `--base` and no `--paths` entry changed on `--base`
//!   since the merge-base.
//! - `1` — HEAD is behind `--base` and/or a `--paths` entry changed on
//!   `--base` since the merge-base. Last stderr line is machine-readable:
//!   `STALE: behind=<n> paths_changed=<n>`.
//! - `2` — usage error: `--base` does not resolve to a commit, or a
//!   `--paths` entry does not exist at HEAD (likely a typo).
//! - `3` — `git fetch` failed (network unavailable). Never silently treated
//!   as up-to-date.

use color_eyre::eyre::{Context, Result, eyre};
use std::process::{Command, Stdio};

/// Configuration for the `spec preflight` subcommand.
pub struct SpecPreflightConfig {
    /// Base git reference to compare HEAD against (e.g. `origin/main`).
    pub base: String,
    /// Paths that must not have changed on `base` since the merge-base.
    pub paths: Vec<String>,
    /// Skip the `git fetch` step (test seam / offline use).
    pub no_fetch: bool,
}

/// Run the `spec preflight` subcommand. Exits the process directly for every
/// non-success outcome so callers get the documented exit codes.
pub fn run(config: SpecPreflightConfig) -> Result<()> {
    let base_ref = config.base.as_str();

    let remote = base_remote(base_ref);
    if let Some(ref remote_name) = remote {
        if !remote_configured(remote_name) {
            eprintln!(
                "spec preflight: remote {remote_name:?} (parsed from --base {base_ref:?}) is not configured"
            );
            std::process::exit(2);
        }
    }

    if !config.no_fetch {
        if let Some(ref remote_name) = remote {
            if !fetch_remote(remote_name, base_ref) {
                eprintln!("spec preflight: git fetch {remote_name} failed — network unavailable");
                eprintln!("STALE: behind=unknown paths_changed=unknown");
                std::process::exit(3);
            }
        }
    }

    let base_head = match git_short_sha(base_ref) {
        Some(sha) => sha,
        None => {
            eprintln!("spec preflight: --base {base_ref:?} does not resolve to a commit");
            std::process::exit(2);
        }
    };

    for path in &config.paths {
        if !path_exists_at("HEAD", path) {
            eprintln!("spec preflight: path {path:?} does not exist at HEAD (typo?)");
            std::process::exit(2);
        }
    }

    let head = git_short_sha("HEAD").ok_or_else(|| eyre!("failed to resolve HEAD"))?;
    let behind_by = compute_behind_by(base_ref)?;

    let mut changed_paths: Vec<(String, Vec<String>)> = Vec::new();
    if let Some(merge_base) = git_output(&["merge-base", "HEAD", base_ref]) {
        for path in &config.paths {
            let commits = changed_commits(&merge_base, base_ref, path);
            if !commits.is_empty() {
                changed_paths.push((path.clone(), commits));
            }
        }
    }

    if behind_by == 0 && changed_paths.is_empty() {
        println!(
            "spec preflight: HEAD {head} matches {base_ref} {base_head} for {} path(s)",
            config.paths.len()
        );
        return Ok(());
    }

    if behind_by > 0 {
        eprintln!(
            "spec preflight: HEAD is {behind_by} commit(s) behind {base_ref} — run `git pull --rebase`, or start a fresh worktree from {base_ref}"
        );
    }
    for (path, commits) in &changed_paths {
        eprintln!(
            "spec preflight: path {path:?} changed on {base_ref} since the merge-base (commits: {})",
            commits.join(", ")
        );
    }
    eprintln!("STALE: behind={behind_by} paths_changed={}", changed_paths.len());
    std::process::exit(1);
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn git_output(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).stderr(Stdio::null()).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if value.is_empty() { None } else { Some(value) }
}

fn git_short_sha(rev: &str) -> Option<String> {
    git_output(&["rev-parse", "--short", "--verify", rev])
}

/// Parse the remote name out of a `<remote>/<branch>` base ref, e.g.
/// `origin/main` -> `Some("origin")`. Returns `None` for a bare ref with no
/// slash (e.g. a local branch or tag), which has no remote to fetch.
fn base_remote(base_ref: &str) -> Option<String> {
    base_ref.split_once('/').map(|(remote, _)| remote.to_string())
}

fn remote_configured(remote_name: &str) -> bool {
    match Command::new("git").args(["remote"]).stderr(Stdio::null()).output() {
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).lines().any(|line| line.trim() == remote_name)
        }
        _ => false,
    }
}

fn fetch_remote(remote_name: &str, base_ref: &str) -> bool {
    let branch = base_ref.split_once('/').map(|(_, branch)| branch);
    let mut args = vec!["fetch", remote_name];
    if let Some(branch) = branch {
        args.push(branch);
    }
    Command::new("git")
        .args(&args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn compute_behind_by(base_ref: &str) -> Result<u64> {
    let count_str = git_output(&["rev-list", "--count", &format!("HEAD..{base_ref}")])
        .unwrap_or_else(|| "0".to_string());
    count_str
        .trim()
        .parse::<u64>()
        .with_context(|| format!("failed to parse rev-list --count output: {count_str:?}"))
}

fn path_exists_at(rev: &str, path: &str) -> bool {
    Command::new("git")
        .args(["cat-file", "-e", &format!("{rev}:{path}")])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

/// Short SHAs of the commits that touched `path` on `base_ref` after `merge_base`.
fn changed_commits(merge_base: &str, base_ref: &str, path: &str) -> Vec<String> {
    git_output(&["log", "--format=%h", &format!("{merge_base}..{base_ref}"), "--", path])
        .map(|out| out.lines().map(str::to_string).collect())
        .unwrap_or_default()
}
