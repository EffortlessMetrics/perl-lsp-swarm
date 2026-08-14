---
name: worktree-manager
description: Optionally reuse or clean local Git worktrees without creating claim, writer, lane, or repository-state authority.
user-invocable: true
---

# Worktree manager

Use this optional operation when the repository's existing reusable-slot helper is materially cheaper than ordinary `git worktree` commands.

Git and GitHub remain authoritative for repository, branch, candidate, PR, review, and merge state. The helper's `.ops-perl-lsp/worktree-manager/state.json` file and sibling managed-worktree pool are disposable local runtime bookkeeping. They must not select work, reserve files or semantic surfaces, establish claim ownership, authorize lifecycle transitions, or survive as a second work database.

## Rules

- one writer mutates each current candidate branch/worktree at a time;
- allocate only for a named mutation claim; read-only inspection of GitHub or source needs no worktree;
- check host capacity before allocating, and wait rather than allocating past it — a saturated host makes local timings, flake rates, and command timeouts untrustworthy, so over-allocating destroys evidence rather than just slowing work (see `orchestrate-work` capacity admission);
- before removing or reusing a slot, read its status, untracked files, branch, HEAD, upstream, and lock; check for unpushed or detached commits and for unique changes against current default-branch state; preserve a branch or patch for anything ambiguous, and re-read immediately before deletion;
- inspect actual Git branch/worktree state and current GitHub state before relying on cached slot metadata;
- an absent, stale, or corrupt helper state file may be repaired or discarded;
- a slot name or owner label is a local reuse/cleanup hint, not issue ownership or lifecycle state;
- pass an explicit stable `--owner` value when allocating and releasing a slot so accidental cross-runtime release is visible;
- use `--force` only after independently proving the worktree contains no unsalvaged work;
- do not require this helper for ordinary worktree creation, reuse, release, or deletion.

## Optional commands

```bash
python3 scripts/worktree-manager.py query
python3 scripts/worktree-manager.py allocate \
  --slot issue-2157 \
  --branch issue/2157-compiler-recovery \
  --owner lane-a
python3 scripts/worktree-manager.py release --slot issue-2157 --owner lane-a
python3 scripts/worktree-manager.py cleanup
```

`allocate` checks whether the requested branch already exists on `origin` and fails closed if it cannot verify the remote state. A genuinely new branch is based on freshly fetched default-branch state. The managed pool lives outside the tracked repository by default, under `<repo-parent>/<repo-name>-worktrees/`.

Use `scripts/cleanup-completed-worktrees.sh` for a one-off prune that does not need reusable-slot bookkeeping. When helper output conflicts with Git or GitHub, Git and GitHub win; repair or discard the helper state and continue.
