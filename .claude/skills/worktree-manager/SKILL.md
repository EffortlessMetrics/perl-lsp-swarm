---
name: worktree-manager
description: Optionally reuse or clean local Git worktrees without creating claim, writer, lane, or repository-state authority.
user-invocable: true
---

# Worktree manager

Use this optional operation when local worktree reuse or cleanup is materially cheaper than ordinary `git worktree` commands.

Git and GitHub remain authoritative for repository, branch, worktree, candidate, PR, and merge state. The helper's `.ops-perl-lsp/worktree-manager/state.json` file is disposable local runtime bookkeeping only. It must not select work, reserve files or semantic surfaces, prove writer ownership, block a valid candidate, or survive as a repository lifecycle database.

## Rules

- one writer mutates each current candidate branch/worktree at a time;
- distinct claim lanes use ordinary optimistic Git concurrency, even when their eventual integration may conflict;
- query actual `git worktree list`, branch state, and GitHub before relying on cached slot metadata;
- an absent, stale, or corrupt helper state file may be rebuilt or discarded;
- do not infer claim ownership or lifecycle stage from slot names or owner labels;
- do not require this helper for ordinary worktree creation, reuse, release, or deletion.

## Optional commands

```bash
python3 scripts/worktree-manager.py query
python3 scripts/worktree-manager.py allocate --slot issue-2157 --branch issue/2157
python3 scripts/worktree-manager.py release --slot issue-2157
python3 scripts/worktree-manager.py cleanup
```

Use `scripts/cleanup-completed-worktrees.sh` for a one-off prune that does not need reusable-slot bookkeeping. When helper output conflicts with Git, Git wins; repair or discard the helper state and continue.
