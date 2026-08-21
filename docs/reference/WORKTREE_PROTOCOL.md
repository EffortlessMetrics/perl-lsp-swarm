# Worktree and branch-mutation protocol

Status: current operational reference
Owner: perl-lsp maintainers
Maintainer contract: [MAINTAINER_AGENT_DOCTRINE.md](MAINTAINER_AGENT_DOCTRINE.md)
Agent contribution route: [AGENT_CONTRIBUTING.md](../how-to/AGENT_CONTRIBUTING.md)

Git worktrees provide filesystem isolation for concurrent write claims. They do not
create semantic ownership, reservations, review authority, or permission to rewrite a
branch.

## Core boundary

One coherent candidate has one mutation owner at a time:

```text
one claim
→ one current candidate
→ one branch/worktree
→ one mutation owner
→ one pull request
```

Use one worktree per genuine concurrent write claim, not per lifecycle pass. The same
candidate can move through planning, implementation, repair, review, and reconciliation
without being rebuilt in a fresh worktree merely to demonstrate process.

Different claims may use different worktrees concurrently, including same-file work.
Coordinate only equivalent candidates, an explicit stack or prerequisite, destructive
shared runtime state, one branch with multiple writers, a real Git conflict, or a
demonstrated combined-tree interaction.

## Coordination checkout

The main checkout is a coordination surface, not an edit surface. Keep it on the current
default branch, `main`, and use it for fetches, worktree creation, and read-only source
comparison.

Do not:

- edit files in the coordination checkout;
- switch it to a feature branch for agent work;
- run `git stash` anywhere in the shared repository;
- create a local branch named `origin/main`;
- remove `.git/worktrees/` entries manually;
- run destructive cleanup while work ownership or salvage state is unknown.

`git stash` is shared across all worktrees. Use scoped `git restore` for discarded work
or a branch-local WIP commit for work that must survive.

## Create a worktree

From the coordination checkout:

```bash
git fetch origin main
git worktree add -b fix/<issue>-<slug> \
  .worktrees/<short-slot> \
  origin/main
```

For an existing candidate branch:

```bash
git worktree add .worktrees/<short-slot> <branch>
```

`/.worktrees/` is ignored in `.gitignore`, so a linked checkout created there never
appears as untracked content in the coordination checkout. A worktree root that is not
ignored breaks the clean-checkout requirement above and makes accidental staging
possible.

Keep paths short, especially on Windows. Enable repository long-path support where
needed:

```bash
git config core.longpaths true
```

Before editing:

```bash
git status --short --branch
git rev-parse --show-toplevel
git rev-parse --git-dir
git rev-parse --git-common-dir
bash scripts/agent-preflight.sh
```

The worktree must be on a named candidate branch, not `main` and not detached HEAD.

## Cargo target directories

Cargo's ordinary default target directory is `<worktree>/target`, which is already
isolated per worktree. Do not export a persistent shell-level `CARGO_TARGET_DIR`; a stale
profile export can redirect every worktree into another candidate's build output.

The repository's `scripts/cargo-safe` and `just agent-*` commands are a deliberate
exception: they set a process-local devplane cache with disk and build-lock controls.
That bounded wrapper does not justify a persistent shell-profile export.

Use either:

```bash
# Native per-worktree target directory
cargo test -p <package> --all-targets --locked

# Repository-managed bounded devplane
just agent-test
```

Do not mix the two accidentally through an inherited environment variable.

## Ownership before mutation

A branch name or local worktree does not prove mutation authority. Before any push,
force-push, update, retarget, or conflict rewrite:

- identify the current candidate and pull request;
- identify the current mutation owner;
- verify no other writer is active on the branch;
- pin the expected remote head SHA;
- inspect dirty, untracked, and unpushed state;
- establish permission to modify the remote branch;
- name the concrete purpose of the mutation.

For fork pull requests, verify which repository owns the head branch and whether
maintainers may push. Never push a fork branch name into the base repository by
assumption.

## Behind-only movement

Behind-only movement requires no action.

```text
candidate is conflict-free
+ unrelated commits land on main
→ leave the candidate head unchanged
```

Fetch and inspect current `main` to determine whether the same semantic seam,
prerequisite, generated authority, or accepted behavior changed. A no-material-interaction
finding is sufficient to preserve the existing branch.

Do not rebase, merge-main, update the branch, create an empty commit, or force-push
solely because:

- the candidate is old;
- the branch is many commits behind;
- unrelated files changed on `main`;
- a cleaner graph would look nicer;
- a required status needs another run;
- a previous rebase did or did not happen.

## Rebase and other integration work

Rebase is ordinary integration work. Its main accepted use is resolving an actual merge
conflict in the candidate lane. It is also available when refreshing the base materially
simplifies current owned work or reduces a concrete integration risk.

There is no mechanical one-rebase limit. Distinct integration work may justify more
than one rebase. Repeated rebases solely to chase `main` or replay CI are churn.

Select the smallest correct strategy:

| Situation | Candidate action |
| --- | --- |
| Conflict-free; no material interaction or policy requirement | leave the branch unchanged |
| Focused source defect without base interaction | repair the existing candidate |
| Reviewed textual conflict | rebase or merge-main with an explicit conflict plan |
| Same-seam semantic interaction | compare models before editing |
| Parent of an explicit stack squash-merged | preserve the child-only delta; retarget, rebase, cherry-pick, or reconstruct only as needed |
| Head branch cannot be rewritten safely | contributor handoff, permitted branch update, or explicit salvage/replacement |
| Contaminated topology with bounded unique value | salvage the unique delta to a fresh owned candidate |
| Missing or transient same-head CI | rerun the same head where supported; do not mutate source |

An unexpected conflict returns to review. Do not guess through it with blanket
ours/theirs resolution.

## Safe push patterns

Ordinary push:

```bash
git push -u origin HEAD
```

Explicit destination when the local branch name differs:

```bash
git push origin HEAD:refs/heads/<remote-branch>
```

Authorized history rewrite:

```bash
git push \
  --force-with-lease="refs/heads/<branch>:<expected-old-sha>" \
  origin HEAD:refs/heads/<branch>
```

Naked `--force` is prohibited. Immediately before the push, re-read both the PR head
and remote branch head. Both must equal the expected old SHA. If either moved, stop and
reconcile ownership rather than overwriting the new work.

After a successful mutation:

- record old and new head identities;
- verify the PR and remote branch expose the new head;
- rerun only proof and review affected by the change;
- treat required live statuses as pending until GitHub records them for the new head;
- preserve earlier semantic evidence for subjects the mutation could not affect.

## Conflict repair in an isolated worktree

Before repair:

```bash
git fetch origin main
git status --short --branch
git rev-parse HEAD
```

For a reviewed rebase:

```bash
git rebase origin/main
```

If an unexpected conflict appears:

```bash
git rebase --abort
```

Require abort to restore the pinned old head. If it does not, preserve the worktree and
return `SALVAGE_REQUIRED`; publish nothing until state is understood.

### Cargo.lock conflict repair

Conflict repair preserves the accepted `Cargo.lock` and first validates locked metadata
without mutation. The accepted lock remains byte-identical unless an explicit branch
admission is made for a manifest-required lock change. The typed routing is:

| Result | Meaning |
| --- | --- |
| `accepted_lock_preserved` | Locked metadata is compatible; keep the accepted lock. |
| `lock_conflict_requires_admission` | A conflict exists; stop and obtain dependency admission. |
| `manifest_requires_lock_change` | The manifest requires a lock change; refuse conflict repair until admitted. |
| `branch_admission_preserved` | A separately admitted branch operation remains outside conflict repair. |
| `historical_text` | Archive or historical guidance is not an active command surface. |
| `controlled_isolated_generation` | Extracted-package smoke may generate a lock only in its isolated temporary package. |
| `not_proven` | Dynamic or unowned construction has no proven production reachability. |

Active conflict repair must not use `cargo generate-lockfile`, bare cargo update, or
delete/recreate Cargo.lock. Targeted dependency guidance and release/version refresh
remain separate branch-admission operations; they do not authorize conflict repair.
There is currently no owned git/Cargo lock-conflict helper seam. This contract therefore
stops at the validator and fixture oracle and is an adoption case for depguard #22;
it does not invent a dependency-admission service.

The validator's fixture anchors include the negative active examples, the positive
isolated smoke and release controls, and a dynamically constructed command. The latter
is explicitly `not_proven`, not accepted by token matching. A compatible accepted lock
is checked byte-for-byte, while a manifest-required lock change is refused without
mutating the accepted worktree.

The following are not active conflict-repair instructions: `cargo generate-lockfile`
in `scripts/ci/check_perl_lsp_rs_core_package.py` operates on an extracted package,
and `just bump-version` belongs to the release/version-refresh scope.

The validator emits deterministic output with:

```text
python3 scripts/ci/test_validate_cargo_lock_conflict_policy.py
python3 scripts/ci/validate_cargo_lock_conflict_policy.py --repo-root .
```

On Windows, interactive rebase/editor flows can be unreliable in linked worktrees. Use
non-interactive commands with an explicit plan, or reconstruct the bounded delta on a
fresh branch when that is safer. The choice is driven by the candidate and conflict, not
by a universal rebase ban.

## Worktree-local proof

Before push, inspect the actual candidate:

```bash
git status --short --branch
git diff --check
git diff origin/main...HEAD
```

Run focused proof for the changed seam, then broader affected proof when selected by
risk or repository policy. Do not infer that a stale target directory, another
worktree's binary, or a local command proves the hosted integration result.

## Multi-box operation

Local slot tools may prevent two processes on one machine from choosing the same path.
They are runtime aids, not durable ownership authority.

Cross-machine ownership belongs in the current GitHub issue, PR, branch, and explicit
maintainer handoff. Do not create a tracked lease database, file reservation map, or
persistent agent-liveness record for ordinary work.

When ownership is ambiguous:

- stop branch mutation;
- preserve local state;
- inspect the live PR and remote head;
- publish one useful ownership question or handoff when another context needs it;
- continue another independent claim rather than polling.

## Cleanup and salvage

Cleanup is destructive and therefore evidence-gated. After merge, closure, or
supersession:

1. verify the landed or retained result;
2. inspect the worktree for dirty and untracked files;
3. inspect commits not reachable from the retained branch;
4. preserve any useful unpushed delta or evidence;
5. confirm the worktree and branch belong to the completed lane;
6. remove through Git's own commands.

```bash
git worktree remove .worktrees/<short-slot>
git branch -D <local-branch>
git worktree prune
```

Use `--force` on worktree removal only after proving no source, receipt, or scratch state
needs preservation. Never delete `.git/worktrees/` manually.

A squash merge does not preserve feature-branch ancestry on `main`. Do not use ancestry
alone to decide that a branch contains no unique value.

## Builder checklist

Before editing:

- [ ] Current issue/claim and candidate are identified.
- [ ] One mutation owner is established.
- [ ] Worktree is isolated, named, and not on `main`.
- [ ] Coordination checkout is clean and remains on `main`.
- [ ] `CARGO_TARGET_DIR` is not inherited from a persistent shell profile.
- [ ] Dirty, untracked, and salvageable state has been inspected.

Before branch mutation:

- [ ] Expected remote head is pinned and revalidated.
- [ ] Concrete repair or integration purpose is recorded.
- [ ] Affected proof and review are known.
- [ ] Force-push, when necessary, uses an explicit lease.
- [ ] Fork/head-repository ownership is correct.

After completion:

- [ ] Landed or retained effect is verified.
- [ ] Residual work has an owner.
- [ ] No useful dirty or unpushed state remains.
- [ ] Only lane-owned worktree, branch, and scratch are removed.
