---
name: worktree-manager
description: Optionally reuse or clean local Git worktrees without creating claim, writer, lane, or repository-state authority.
user-invocable: true
---

# Worktree manager

Use this optional operation when local worktree reuse or cleanup is materially cheaper than ordinary `git worktree` commands.

Git and GitHub remain authoritative for repository, branch, candidate, PR, and merge state. The helper's `.agent-worktrees/.worktree-manager/state.json` file is disposable local runtime bookkeeping only. It must not select work, reserve files or semantic surfaces, establish claim ownership, authorize repository lifecycle transitions, or survive as a second work database.

## Rules

- one writer mutates each current candidate branch/worktree at a time;
- query actual `git worktree list`, branch state, and GitHub before relying on cached slot metadata;
- an absent, stale, or corrupt helper state file may be rebuilt or discarded;
- numeric slots and owner tokens do not establish issue/claim ownership or lifecycle stage;
- the recorded owner token is only a local cleanup lease: `release` refuses a missing or different owner so one runtime does not delete another runtime's reusable worktree slot;
- supply the same explicit `--owner` value used for allocation when releasing; use `--force` only after independently proving the worktree is safe to remove;
- do not require this helper for ordinary worktree creation, reuse, release, or deletion.

## Optional commands

```bash
python3 scripts/worktree-manager.py list
python3 scripts/worktree-manager.py allocate \
  --slot 1 \
  --kind issue \
  --id 2157 \
  --slug compiler-recovery \
  --owner lane-a
python3 scripts/worktree-manager.py release --slot 1 --owner lane-a
```

`allocate` creates the canonical `agent/<kind>-<id>-<slug>` branch by default. Use `--use-existing-branch` only when that exact canonical branch already exists and is not checked out elsewhere.

Use `scripts/cleanup-completed-worktrees.sh` for a one-off prune that does not need reusable-slot bookkeeping. When helper output conflicts with Git, Git wins; repair or discard the helper state and continue.
