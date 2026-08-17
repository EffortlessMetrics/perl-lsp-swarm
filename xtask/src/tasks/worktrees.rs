//! Compatibility adapter for the historical `cargo xtask worktree-cleanup` command.
//!
//! The default path is now a projection of the canonical typed, read-only
//! worktree inspection provider. The legacy `--force` mutation remains only
//! as a compatibility route until #10263 replaces Boolean-force cleanup with
//! exact-plan application. It is deliberately not exposed by the canonical
//! `worktree-cleanup inspect` binary.

use crate::utils::project_root;
use color_eyre::eyre::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use xtask::worktree_cleanup::{WorktreeActionKind, inspect as inspect_worktrees, render_human};

/// Inspect registered worktrees, or run the legacy explicit mutation path.
///
/// With `force == false`, this function performs no Git, filesystem, config,
/// ref, lock, or worktree mutation. With `force == true`, it preserves the
/// historical explicit cleanup route while warning that exact-plan apply is
/// its successor.
pub fn cleanup(root: Option<PathBuf>, force: bool) -> Result<()> {
    let root = match root {
        Some(root) => root,
        None => project_root()?,
    };

    if !force {
        let plan = inspect_worktrees(&root)?;
        print!("{}", render_human(&plan));
        println!();
        println!(
            "Read-only inspection. Use `cargo run -p xtask --bin worktree-cleanup -- \
             inspect --json` for the canonical typed plan."
        );
        return Ok(());
    }

    run_legacy_force(&root)
}

fn run_legacy_force(root: &Path) -> Result<()> {
    eprintln!(
        "WARNING: `cargo xtask worktree-cleanup --force` is a compatibility-only \
         mutation path. #10263 replaces it with exact-plan application."
    );

    run_prune(root, "before legacy cleanup")?;
    let plan = inspect_worktrees(root)?;
    print!("{}", render_human(&plan));

    let mut selected = 0_u64;
    let mut removed = 0_u64;
    let mut refused = 0_u64;
    for entry in &plan.entries {
        let Some(action) = &entry.proposed_action else {
            continue;
        };
        if !action.targetable || action.kind != WorktreeActionKind::RemoveRegisteredWorktree {
            continue;
        }

        selected += 1;
        println!("Removing legacy candidate: {}", action.target.display());
        let output = Command::new("git")
            .current_dir(root)
            .args(["worktree", "remove"])
            .arg(&action.target)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .wrap_err_with(|| {
                format!("running git worktree remove for {}", action.target.display())
            })?;
        if output.status.success() {
            removed += 1;
        } else {
            refused += 1;
            let stderr = String::from_utf8_lossy(&output.stderr);
            let detail = stderr.trim();
            eprintln!(
                "WARNING: git refused to remove {}; keeping it{}",
                action.target.display(),
                if detail.is_empty() { String::new() } else { format!(": {detail}") }
            );
        }
    }

    run_prune(root, "after legacy cleanup")?;
    println!("Legacy cleanup complete: selected={selected} removed={removed} refused={refused}");
    Ok(())
}

fn run_prune(root: &Path, phase: &str) -> Result<()> {
    let status = Command::new("git")
        .current_dir(root)
        .args(["worktree", "prune"])
        .status()
        .wrap_err_with(|| format!("running git worktree prune {phase}"))?;
    if !status.success() {
        bail!("git worktree prune failed {phase}");
    }
    Ok(())
}
