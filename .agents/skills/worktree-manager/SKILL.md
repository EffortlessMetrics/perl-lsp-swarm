---
name: worktree-manager
description: Inspect task-local setup identity and storage before hook provisioning, or optionally reuse and clean local Git worktrees without creating claim or writer authority.
---

# Worktree manager

Use this optional operation when local worktree reuse or cleanup is materially cheaper than ordinary `git worktree` commands.

Git and GitHub remain authoritative for repository, branch, candidate, PR, and merge state. The helper's `.ops-perl-lsp/worktree-manager/state.json` file is disposable local runtime bookkeeping only. It must not select work, reserve files or semantic surfaces, establish claim ownership, authorize repository lifecycle transitions, or survive as a second work database.

## Rules

- one writer mutates each current candidate branch/worktree at a time;
- allocate only for a named mutation claim; read-only inspection of GitHub or source needs no worktree;
- check host capacity before allocating and wait rather than exceeding it — a saturated host makes local timings, flake rates, and command timeouts untrustworthy, so over-allocation destroys evidence instead of merely slowing work (see `$orchestrate-work` capacity admission);
- a lane that allocates a worktree removes it on completion: lane briefs should carry
  this as a requirement, and a lane ending without releasing must say why in its typed
  return — dead lanes cannot release themselves, so residue is expected, not exceptional;
- before removing or reusing a slot, read its status, untracked files, branch, HEAD, upstream, and lock; check for unpushed or detached commits and unique changes against current default-branch state; preserve a branch or patch for anything ambiguous, and re-read immediately before deletion;
- query actual `git worktree list`, branch state, and GitHub before relying on cached slot metadata;
- an absent, stale, or corrupt helper state file may be rebuilt or discarded;
- slot names and owner labels do not establish issue/claim ownership or lifecycle stage;
- the recorded owner token is only a local cleanup lease: `release` refuses a different owner so one runtime does not delete another runtime's reusable worktree slot;
- supply the same `--owner` value or `WORKTREE_MANAGER_OWNER` used for allocation when releasing; use `--force` only after independently proving the worktree is safe to remove;
- do not require this helper for ordinary worktree creation, reuse, release, or deletion.

## Establish setup identity before provisioning hooks

Before a worktree wrapper, helper allocation, or hook installer, inspect the invoking
checkout and its actual Git common directory. Do not infer the coordination checkout
by removing `/.git`: separate Git directories and linked worktrees need not have that
layout. Record branch, HEAD, origin, OS user, and the worktree list; compare them with
the authorized candidate and mutation owner. A directory name is not root identity.

Select `DEVPLANE`, `CARGO_HOME`, and `TMPDIR` for this task's shell before any Cargo
probe or setup command. For new resources, choose absolute native paths on measured
host-local storage; for an existing producer, retain its admitted settings and private
outputs. An already admitted shared Cargo cache remains usable when its ownership and
capacity permit it. Task-local selection does not require a new cache per command.
For example, in a task subshell, after choosing an existing storage parent:

```bash
(
  set -eu
  : "${SETUP_STORAGE_ROOT:?set an absolute host-local parent after measuring capacity}"
  export DEVPLANE="$SETUP_STORAGE_ROOT/devplane"
  export CARGO_HOME="$SETUP_STORAGE_ROOT/cargo"
  export TMPDIR="$SETUP_STORAGE_ROOT/tmp"
  # Run the observation below, then the separately admitted task commands here.
)
```

This read-only observation uses the current Cargo resolver, including its path and
repository-override refusals. Run it from the intended checkout (nested directories
are supported), with the selected variables exported. It creates no directories,
leases, hooks, or build outputs:

```python
# setup-observation: execute with python3 -B - <<'PY' ... PY
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

sys.dont_write_bytecode = True
for name in ("GIT_DIR", "GIT_WORK_TREE", "GIT_COMMON_DIR"):
    if name in os.environ:
        raise SystemExit(f"unset {name} and invoke from the intended checkout")
for name in ("DEVPLANE", "CARGO_HOME", "TMPDIR"):
    if not os.environ.get(name):
        raise SystemExit(f"select {name} for this task shell first")

def git_line(*args):
    value = subprocess.check_output(["git", *args])
    if not value.endswith(b"\n"):
        raise SystemExit("Git output lacks terminal newline")
    return os.fsdecode(value[:-1])

root = Path(git_line("rev-parse", "--show-toplevel")).resolve()
common = Path(git_line("rev-parse", "--path-format=absolute", "--git-common-dir")).resolve()
spec = importlib.util.spec_from_file_location("setup_cargo", root / "scripts/cargo_admitted.py")
admission = importlib.util.module_from_spec(spec)
spec.loader.exec_module(admission)
worktree, slot, paths = admission.resource_plan(dict(os.environ))
observations = {}
for name, path in {"slot": slot, **paths}.items():
    ancestor = next(p for p in (path, *path.parents) if p.exists())
    if not ancestor.is_dir():
        raise SystemExit(f"resource ancestor is not a directory: {ancestor}")
    usage = shutil.disk_usage(ancestor)
    observations[name] = dict(path=str(path), observed_ancestor=str(ancestor),
                              total_bytes=usage.total, free_bytes=usage.free)
print(json.dumps(dict(root=str(worktree), common_dir=str(common),
    branch=git_line("branch", "--show-current"), head=git_line("rev-parse", "HEAD"),
    origin=git_line("remote", "get-url", "origin"),
    user=subprocess.check_output(["whoami"]).decode().removesuffix("\n"),
    resources=observations), indent=2), flush=True)
subprocess.run(["git", "worktree", "list", "--porcelain", "-z"], check=True)
```

Free-space observations at a shared volume are not additive. Compare all destinations,
available memory/CPU and active builds with the exact workload's admission/budget;
path selection alone proves neither writable capacity nor a build's fit. Follow
[Cargo storage admission](../../../docs/agents/CARGO_STORAGE.md), including the exact
`scripts/cargo-admitted --preflight <command> ...` route before a supported build.
Keep reserve policy, toolchain pins and existing outputs intact; provision missing
resources only after this selection. Report provisioning separately from execution.

An isolated clone has its own common directory and Cargo exclusion domain; the
resolver supports it. This does not make the clone's root a linked worktree or pass
the linked-worktree-only checks in `scripts/agent-preflight.sh` or writer admission.
Use the authorized task checkout and applicable admission, and report any such refusal
as a separate boundary rather than disguising the clone or weakening the check.

Hook setup is a separate mutation decision. Current
`crates/perl-ci-hygiene/src/git_hooks.rs::cmd_install_githooks` enumerates every live
tree, writes each tree's `.githooks`, then sets the shared local relative
`core.hooksPath`. Running from a linked tree does not limit the installer to that tree.
Inspect the common directory and worktree list first; use provisioning only within
the authorized resource scope. A no-shared-installer task reports missing hooks and
continues permitted independent work without claiming hook protection. Inspect active
hooks with `git rev-parse --path-format=absolute --git-path hooks`; do not assume the
active path is `<common-dir>/hooks`.

## Optional commands

```bash
python3 scripts/worktree-manager.py query
python3 scripts/worktree-manager.py allocate --slot issue-2157 --branch issue/2157 --owner "$WORKTREE_MANAGER_OWNER"
python3 scripts/worktree-manager.py release --slot issue-2157 --owner "$WORKTREE_MANAGER_OWNER"
python3 scripts/worktree-manager.py cleanup
```

Use `scripts/cleanup-completed-worktrees.sh` for a one-off prune that does not need reusable-slot bookkeeping. When helper output conflicts with Git, Git wins; repair or discard the helper state and continue.

## Orphan sweep

Lanes killed by usage limits or restarts never release their worktrees. Sweep
periodically and after landing waves:

1. list candidates by branch recency (`git for-each-ref --sort=-committerdate
   refs/heads/` cross-referenced with `git worktree list` and the on-disk slot
   directories);
2. remove a tree only when it holds no state existing elsewhere — no untracked files,
   no unpushed commits, no detached HEAD outside the base — or when the owning lane was
   explicitly abandoned;
3. `bash scripts/cleanup-completed-worktrees.sh --dry-run` approximates this predicate,
   but its landed test accepts any merged PR on the branch *name*, so a reused or
   locally-advanced branch can be planned for removal with its only reference —
   verify the plan against step 2 yourself and re-read each worktree immediately
   before deletion (same rule as slot removal).

The sweep composes with the per-slot safety rules above rather than replacing them.
A tree holding unpushed commits whose lane was not explicitly abandoned is salvage
input, not sweep residue: route it through `$orchestrate-work`'s Salvage revive
survey — push the WIP to a named salvage PR before treating the slot as removable.

Durable tooling for verified orphan reaping and checkout/capacity integrity is owned
by issues #11606 and #3957; this skill carries only the guidance.
