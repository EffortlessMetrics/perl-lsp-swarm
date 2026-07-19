---
description: Reconcile one PR with current main only after a concrete conflict, semantic, stack, policy, or proof reason
argument-hint: "<PR number or branch name>"
---

# Reconcile PR Base

This command may use rebase, merge-main, GitHub branch update, retargeting, or a
bounded cherry-pick when review establishes that a base change is necessary. It
must not mutate a branch merely because the PR is old or behind.

Canonical contract: `docs/specs/PLSP-SPEC-0006-pr-queue-disposition.md`.

Context: **$ARGUMENTS**

## Entry condition

Before any mutation, require a reviewed disposition of:

- `RESOLVE_CONFLICTS`; or
- `REVIEW_SEMANTIC_INTERACTION`; or
- `UPDATE_BASE_REQUIRED` with a concrete reason.

Valid reasons include:

- actual textual conflict;
- current `main` changed the same semantic contract and the PR must adapt;
- a stacked prerequisite changed so the child cannot be reviewed/tested
  independently;
- live branch protection or merge-queue policy requires a current integration
  basis;
- meaningful proof cannot be interpreted without the updated prerequisite or
  contract.

These are not sufficient:

- age or inactivity;
- commits-behind;
- unrelated `main` movement;
- non-linear history;
- preference for a clean graph;
- a desire to retrigger CI.

If no valid reason is recorded and the PR is conflict-free, stop with
`NO_ACTION_REQUIRED` and preserve the existing head/review/check evidence.

## Steps

### 1. Resolve exact PR and branch identity

```bash
gh pr view $ARGUMENTS \
  --json number,title,headRefName,headRefOid,baseRefName,baseRefOid,mergeable,mergeStateStatus,isDraft
```

> **MCP alternative:** for a numeric argument, fetch that PR. For a branch-name
> argument, search open PRs by exact head branch, then fetch the numeric result.

Record:

```yaml
pr:
branch:
old_head_sha:
base_sha:
merge_state:
reviewed_disposition:
base_update_reason:
```

### 2. Verify ownership and mutation authority

Before checkout, rebase, or force-push:

- identify the branch owner/current writer;
- verify no other writer is active on the branch;
- verify the remote head still equals `old_head_sha`;
- verify maintainers are authorized to modify the branch;
- record whether history rewrite is permitted;
- inspect dirty, unpushed, or salvageable work in the selected worktree.

If ownership or remote identity is ambiguous, return `BLOCKED` or
`NOT_PROVEN`. Do not force-push.

### 3. Fetch current objects without changing the PR

```bash
git fetch origin main
git fetch origin <branch>
```

Inspect:

- actual textual conflicts;
- current-main changes to the same semantic seam;
- stacked parent/child delta;
- prerequisite and generated-authority changes;
- whether an equivalent implementation already landed.

A finding of no material interaction is a valid reason to stop and leave the
head unchanged.

### 4. Select the smallest correct strategy

| Situation | Preferred action |
| --- | --- |
| Conflict-free; no material interaction | leave head unchanged |
| Simple mechanical textual conflict | resolve with the least destructive branch update compatible with ownership |
| Semantic conflict | compare models and repair intentionally; do not take one side wholesale |
| Stacked parent squash-merged | preserve the child-only delta; retarget/rebase/cherry-pick only as needed |
| Contributor branch cannot be rewritten | merge-main or create a bounded replacement only with explicit ownership/disposition |
| Contaminated topology with small unique delta | `SALVAGE_UNIQUE_DELTA` to a fresh branch |
| Same-head proof merely missing | stop; request non-mutating exact-head proof refresh |

Rebase is one implementation option, not the default doctrine.

### 5. Perform the chosen mutation

For an authorized rebase:

```bash
git checkout <branch>
git rebase origin/main
```

Resolve conflicts according to the reviewed semantic decision. Do not
blindly accept `main` for docs, infrastructure, generated files, schemas, or
`Cargo.lock`; those may be the actual authority changed by the PR.

Push a rewritten head only with an explicit expected SHA:

```bash
git push --force-with-lease=<branch>:<old_head_sha> origin HEAD:<branch>
```

Never use naked `--force`.

For merge-main or GitHub branch update, still record old/new heads and verify the
operation incorporated the intended base without unrelated changes.

### 6. Verify and report evidence invalidation

After mutation:

```bash
NEW_HEAD=$(git rev-parse HEAD)
git diff --check origin/main...HEAD
```

Run the smallest owning proof required by the changed/resolved seams. Then state
which prior evidence is stale:

- exact-head checks;
- review convergence for changed seams;
- integration proof;
- generated receipts;
- Changie/release-note disposition when implementation scope changed.

Do not claim the PR is ready merely because the base operation succeeded.

### 7. Return to the previous safe checkout

```bash
git checkout -
```

Do not switch or mutate the repository's coordination checkout when worktree
policy requires isolated mutation.

## Result

```text
### Base Reconciliation Result
- PR: #<number> (<title>)
- Branch: <headRefName>
- Old head: <full sha>
- New head: <full sha or unchanged>
- Disposition: RESOLVE_CONFLICTS / REVIEW_SEMANTIC_INTERACTION / UPDATE_BASE_REQUIRED
- Concrete reason: <reason>
- Strategy: no action / rebase / merge-main / update / retarget / salvage
- Conflicts and semantic decisions: <details>
- Proof run: <details>
- Evidence invalidated: <details>
- Status: NO_ACTION_REQUIRED / UPDATED / BLOCKED / NOT_PROVEN / SALVAGE_REQUIRED
```
