//! Worktree maintenance helpers used by local agent operations.
//!
//! `cleanup` classifies every worktree under `.claude/worktrees/` as either
//! `Keep` (with a reason) or `Remove` before touching anything. It defaults
//! to a **dry-run report** — nothing is deleted unless the caller opts in
//! with `--force`. Even under `--force`, a worktree is never removed when
//! it is:
//!
//! - **dirty** — `git status --porcelain` reports uncommitted changes;
//! - **locked** — `git worktree list --porcelain` reports a `locked` entry;
//! - on a branch with an **open PR** (or PR status could not be determined —
//!   "unknown" is treated as unsafe, never as "no PR", to avoid destroying a
//!   worktree that turns out to have a live PR when `gh` is unavailable);
//! - the **root checkout**.
//!
//! This mirrors `scripts/swarm-clean`'s dry-run-first, classify-before-delete
//! convention. It intentionally does *not* implement that script's full
//! KEEP/CACHE-ONLY/REMOVE/SALVAGE/REVIEW bucket taxonomy (branch-merged
//! detection, active-lock-with-live-pid, etc.) — this is the narrow safety
//! guard against unconditional force-removal; the fuller classification is
//! tracked separately (#3957 W2). See issue #4097.

use crate::utils::project_root;
use color_eyre::eyre::{Result, bail};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::str;

/// One worktree entry parsed from `git worktree list --porcelain`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct WorktreeEntry {
    path: PathBuf,
    /// `None` for a detached-HEAD worktree.
    branch: Option<String>,
    locked: bool,
    lock_reason: Option<String>,
}

/// Classification outcome for a single worktree.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Verdict {
    Remove,
    Keep(String),
}

/// Tri-state result of asking whether a branch has an open PR.
///
/// `Unknown` (gh absent, unauthenticated, or the query otherwise failed)
/// must never be treated the same as `None` — doing so would let a
/// worktree with a live PR be removed on a machine where `gh` can't answer.
#[derive(Debug, Clone, PartialEq, Eq)]
enum PrStatus {
    Open(u64),
    None,
    Unknown,
}

/// Environment variable that overrides the `gh` binary invoked to check PR
/// status. Used by tests to inject a stub; unset in production, in which
/// case the real `gh` on `PATH` is used.
const GH_BIN_OVERRIDE_ENV: &str = "XTASK_WORKTREE_CLEANUP_GH_BIN";

fn gh_program() -> String {
    std::env::var(GH_BIN_OVERRIDE_ENV).unwrap_or_else(|_| "gh".to_string())
}

/// Report (and, with `force`, remove) stale `.claude/worktrees` entries.
///
/// `root` defaults to the perl-lsp workspace root (via [`project_root`])
/// when `None`; tests pass an explicit fixture root.
pub fn cleanup(root: Option<PathBuf>, force: bool) -> Result<()> {
    let root = match root {
        Some(root) => root,
        None => project_root()?,
    };
    run(&root, force)
}

fn run(root: &Path, force: bool) -> Result<()> {
    let prune_status =
        Command::new("git").current_dir(root).args(["worktree", "prune"]).status()?;
    if !prune_status.success() {
        bail!("failed to prune git worktrees");
    }

    let list_output = Command::new("git")
        .current_dir(root)
        .args(["worktree", "list", "--porcelain"])
        .stdout(Stdio::piped())
        .output()?;
    if !list_output.status.success() {
        bail!("failed to list git worktrees");
    }

    let list = str::from_utf8(&list_output.stdout)?;
    let entries: Vec<WorktreeEntry> =
        parse_worktree_list(list).into_iter().filter(|e| is_agent_worktree(&e.path)).collect();

    println!("Found {} agent worktree(s) under .claude/worktrees/", entries.len());

    if entries.is_empty() {
        println!("No stale worktrees to clean up");
        return Ok(());
    }

    println!(
        "=== {} ===",
        if force {
            "Removing worktrees classified REMOVE"
        } else {
            "Dry-run report (pass --force to remove REMOVE-classified worktrees)"
        }
    );

    let mut to_remove: Vec<&WorktreeEntry> = Vec::new();
    let mut keep_count = 0usize;
    for entry in &entries {
        let verdict = classify(root, entry);
        match &verdict {
            Verdict::Remove => {
                let action = if force { "REMOVE" } else { "REMOVE (dry-run: would remove)" };
                println!("{action:<32} {}", entry.path.display());
                to_remove.push(entry);
            }
            Verdict::Keep(reason) => {
                println!("KEEP                             {} ({reason})", entry.path.display());
                keep_count += 1;
            }
        }
    }

    println!();
    println!(
        "=== summary: {} to remove, {keep_count} kept ===",
        if force {
            to_remove.len().to_string()
        } else {
            format!("{} would be removed", to_remove.len())
        }
    );

    if !force {
        println!();
        println!("(Dry-run. Re-run with --force to remove REMOVE-classified worktrees.)");
        return Ok(());
    }

    for entry in &to_remove {
        println!("Removing: {}", entry.path.display());
        let remove_status = Command::new("git")
            .current_dir(root)
            .args(["worktree", "remove", "--force"])
            .arg(&entry.path)
            .status()?;
        if !remove_status.success() {
            bail!("git worktree remove failed for {}", entry.path.display());
        }
    }

    let final_prune_status =
        Command::new("git").current_dir(root).args(["worktree", "prune"]).status()?;
    if !final_prune_status.success() {
        bail!("failed to prune git worktrees after cleanup");
    }

    println!("Cleanup complete");
    Ok(())
}

/// Does `path` live under a `.claude/worktrees/` directory? Git always
/// normalizes worktree list paths to forward slashes, even on Windows.
fn is_agent_worktree(path: &Path) -> bool {
    path.to_string_lossy().replace('\\', "/").contains(".claude/worktrees/")
}

/// Classify a single worktree as `Remove` or `Keep(reason)`.
///
/// Order matters: cheaper, purely-local checks (root, locked, dirty) run
/// before the network-dependent PR check, so a dirty or locked worktree
/// never needs a `gh` round trip to be kept safe.
fn classify(root: &Path, entry: &WorktreeEntry) -> Verdict {
    if paths_match(&entry.path, root) {
        return Verdict::Keep("root checkout — never removed".to_string());
    }

    if entry.locked {
        return match &entry.lock_reason {
            Some(reason) if !reason.is_empty() => Verdict::Keep(format!("locked: {reason}")),
            _ => Verdict::Keep("locked".to_string()),
        };
    }

    match worktree_dirty(&entry.path) {
        Ok(true) => return Verdict::Keep("dirty — uncommitted changes present".to_string()),
        Ok(false) => {}
        Err(error) => {
            return Verdict::Keep(format!(
                "could not determine dirty status ({error}) — not safe to remove"
            ));
        }
    }

    let Some(branch) = &entry.branch else {
        return Verdict::Keep("detached HEAD — no branch to verify PR status".to_string());
    };

    match branch_pr_status(branch) {
        PrStatus::Open(number) => {
            Verdict::Keep(format!("open PR #{number} exists for branch '{branch}'"))
        }
        PrStatus::None => Verdict::Remove,
        PrStatus::Unknown => Verdict::Keep(format!(
            "PR status for branch '{branch}' could not be determined (gh unavailable) \
             — not safe to remove"
        )),
    }
}

fn paths_match(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// `true` if the worktree at `path` has uncommitted changes.
fn worktree_dirty(path: &Path) -> Result<bool> {
    let output = Command::new("git")
        .current_dir(path)
        .args(["status", "--porcelain"])
        .stdout(Stdio::piped())
        .output()?;
    if !output.status.success() {
        bail!("git status --porcelain exited non-zero");
    }
    Ok(!output.stdout.is_empty())
}

/// Query whether `branch` has an open PR, via `gh pr list`. Any failure
/// (gh absent, unauthenticated, no network, no remote configured) yields
/// `Unknown` rather than `None` — the caller must treat `Unknown` as unsafe.
fn branch_pr_status(branch: &str) -> PrStatus {
    let output = Command::new(gh_program())
        .args([
            "pr",
            "list",
            "--head",
            branch,
            "--state",
            "open",
            "--json",
            "number",
            "--jq",
            ".[0].number",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output();

    match output {
        Ok(out) if out.status.success() => {
            let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if text.is_empty() {
                PrStatus::None
            } else {
                match text.parse::<u64>() {
                    Ok(number) => PrStatus::Open(number),
                    Err(_) => PrStatus::Unknown,
                }
            }
        }
        _ => PrStatus::Unknown,
    }
}

/// Parse `git worktree list --porcelain` output into entries.
fn parse_worktree_list(list: &str) -> Vec<WorktreeEntry> {
    let mut entries = Vec::new();
    let mut path: Option<PathBuf> = None;
    let mut branch: Option<String> = None;
    let mut locked = false;
    let mut lock_reason: Option<String> = None;

    let flush = |path: &mut Option<PathBuf>,
                 branch: &mut Option<String>,
                 locked: &mut bool,
                 lock_reason: &mut Option<String>,
                 entries: &mut Vec<WorktreeEntry>| {
        if let Some(p) = path.take() {
            entries.push(WorktreeEntry {
                path: p,
                branch: branch.take(),
                locked: *locked,
                lock_reason: lock_reason.take(),
            });
        }
        *locked = false;
    };

    for line in list.lines() {
        if let Some(rest) = line.strip_prefix("worktree ") {
            flush(&mut path, &mut branch, &mut locked, &mut lock_reason, &mut entries);
            path = Some(PathBuf::from(rest));
        } else if let Some(rest) = line.strip_prefix("branch ") {
            branch = Some(rest.strip_prefix("refs/heads/").unwrap_or(rest).to_string());
        } else if let Some(rest) = line.strip_prefix("locked") {
            locked = true;
            let reason = rest.strip_prefix(' ').unwrap_or("").trim().to_string();
            lock_reason = if reason.is_empty() { None } else { Some(reason) };
        }
        // "HEAD <sha>", "detached", and blank separator lines carry no
        // information this classifier needs.
    }
    flush(&mut path, &mut branch, &mut locked, &mut lock_reason, &mut entries);

    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_worktree_list ──────────────────────────────────────────────

    #[test]
    fn parses_branch_locked_and_detached_entries() {
        let porcelain = "\
worktree /repo
HEAD abc123
detached

worktree /repo/.claude/worktrees/agent-1
HEAD def456
branch refs/heads/impl/123-foo

worktree /repo/.claude/worktrees/agent-2
HEAD ghi789
branch refs/heads/impl/456-bar
locked claude agent agent-2 (pid 4242)
";
        let entries = parse_worktree_list(porcelain);
        assert_eq!(entries.len(), 3);

        assert_eq!(entries[0].path, PathBuf::from("/repo"));
        assert_eq!(entries[0].branch, None);
        assert!(!entries[0].locked);

        assert_eq!(entries[1].path, PathBuf::from("/repo/.claude/worktrees/agent-1"));
        assert_eq!(entries[1].branch.as_deref(), Some("impl/123-foo"));
        assert!(!entries[1].locked);

        assert_eq!(entries[2].path, PathBuf::from("/repo/.claude/worktrees/agent-2"));
        assert_eq!(entries[2].branch.as_deref(), Some("impl/456-bar"));
        assert!(entries[2].locked);
        assert_eq!(entries[2].lock_reason.as_deref(), Some("claude agent agent-2 (pid 4242)"));
    }

    #[test]
    fn parses_lock_with_no_reason() {
        let porcelain = "\
worktree /repo/.claude/worktrees/agent-3
HEAD abc123
branch refs/heads/impl/789-baz
locked
";
        let entries = parse_worktree_list(porcelain);
        assert_eq!(entries.len(), 1);
        assert!(entries[0].locked);
        assert_eq!(entries[0].lock_reason, None);
    }

    // ── is_agent_worktree ────────────────────────────────────────────────

    #[test]
    fn is_agent_worktree_matches_forward_and_back_slash_paths() {
        assert!(is_agent_worktree(Path::new("/repo/.claude/worktrees/agent-1")));
        assert!(is_agent_worktree(Path::new(r"H:\repo\.claude\worktrees\agent-1")));
        assert!(!is_agent_worktree(Path::new("/repo")));
        assert!(!is_agent_worktree(Path::new("/tmp/some-other-worktree")));
    }

    // ── classify: pure branches that don't need a real gh/git call ────────

    #[test]
    fn classify_keeps_root_checkout() -> Result<()> {
        let root = Path::new("/repo");
        let entry = WorktreeEntry {
            path: PathBuf::from("/repo"),
            branch: Some("main".to_string()),
            locked: false,
            lock_reason: None,
        };
        match classify(root, &entry) {
            Verdict::Keep(reason) => {
                assert!(reason.contains("root checkout"));
                Ok(())
            }
            Verdict::Remove => bail!("root checkout must never be classified Remove"),
        }
    }

    #[test]
    fn classify_keeps_locked_worktree_with_reason() -> Result<()> {
        let root = Path::new("/repo");
        let entry = WorktreeEntry {
            path: PathBuf::from("/repo/.claude/worktrees/agent-1"),
            branch: Some("impl/1".to_string()),
            locked: true,
            lock_reason: Some("claude agent agent-1 (pid 99)".to_string()),
        };
        match classify(root, &entry) {
            Verdict::Keep(reason) => {
                assert!(reason.contains("locked"));
                assert!(reason.contains("pid 99"));
                Ok(())
            }
            Verdict::Remove => bail!("locked worktree must never be classified Remove"),
        }
    }

    #[test]
    fn classify_keeps_locked_worktree_without_reason() -> Result<()> {
        let root = Path::new("/repo");
        let entry = WorktreeEntry {
            path: PathBuf::from("/repo/.claude/worktrees/agent-1"),
            branch: Some("impl/1".to_string()),
            locked: true,
            lock_reason: None,
        };
        match classify(root, &entry) {
            Verdict::Keep(reason) => {
                assert_eq!(reason, "locked");
                Ok(())
            }
            Verdict::Remove => bail!("locked worktree must never be classified Remove"),
        }
    }
}
