---
name: worktree-manager
description: Optionally reuse or clean local Git worktrees without creating claim, writer, lane, or repository-state authority.
---

# Worktree manager

Use this optional operation when local worktree reuse or cleanup is materially cheaper than ordinary `git worktree` commands.

Git and GitHub remain authoritative for repository, branch, candidate, PR, and merge state. The helper's `.ops-perl-lsp/worktree-manager/state.json` file is disposable local runtime bookkeeping only. It must not select work, reserve files or semantic surfaces, establish claim ownership, authorize repository lifecycle transitions, or survive as a second work database.

## Rules

- one writer mutates each current candidate branch/worktree at a time;
- allocate only for a named mutation claim; read-only inspection of GitHub or source needs no worktree;
- check host capacity before allocating and wait rather than exceeding it — a saturated host makes local timings, flake rates, and command timeouts untrustworthy, so over-allocation destroys evidence instead of merely slowing work (see `$orchestrate-work` capacity admission);
- before removing or reusing a slot, read its status, untracked files, branch, HEAD, upstream, and lock; check for unpushed or detached commits and unique changes against current default-branch state; preserve a branch or patch for anything ambiguous, and re-read immediately before deletion;
- query actual `git worktree list`, branch state, and GitHub before relying on cached slot metadata;
- an absent, stale, or corrupt helper state file may be rebuilt or discarded;
- slot names and owner labels do not establish issue/claim ownership or lifecycle stage;
- the recorded owner token is only a local cleanup lease: `release` refuses a different owner so one runtime does not delete another runtime's reusable worktree slot;
- supply the same `--owner` value or `WORKTREE_MANAGER_OWNER` used for allocation when releasing; use `--force` only after independently proving the worktree is safe to remove;
- do not require this helper for ordinary worktree creation, reuse, release, or deletion.

## Optional commands

```bash
python3 scripts/worktree-manager.py query
python3 scripts/worktree-manager.py allocate --slot issue-2157 --branch issue/2157 --owner "$WORKTREE_MANAGER_OWNER"
python3 scripts/worktree-manager.py release --slot issue-2157 --owner "$WORKTREE_MANAGER_OWNER"
python3 scripts/worktree-manager.py cleanup
```

Use `scripts/cleanup-completed-worktrees.sh` for a one-off prune that does not need reusable-slot bookkeeping. When helper output conflicts with Git, Git wins; repair or discard the helper state and continue.
