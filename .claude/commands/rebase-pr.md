---
description: Reconcile one PR with current main only after a concrete conflict, semantic, stack, policy, proof, or salvage reason
argument-hint: "<PR number or exact branch name>"
---

# Reconcile PR Base

This command may use rebase, merge-main, GitHub branch update, retargeting, or a
bounded salvage/cherry-pick when review establishes that a base change is
necessary. It must not mutate a branch merely because the PR is old or behind.

Canonical contract: `docs/specs/PLSP-SPEC-0006-pr-queue-disposition.md`.

The slash-command argument is untrusted input. Do not paste it into shell source.

## Entry condition

Before any base mutation, require a reviewed disposition of:

- `RESOLVE_CONFLICTS`;
- `REVIEW_SEMANTIC_INTERACTION`;
- `UPDATE_BASE_REQUIRED` with a concrete reason; or
- `SALVAGE_UNIQUE_DELTA` when the existing branch topology/contamination makes
  in-place repair unsafe or unreviewable.

`REPAIR_EXISTING_BRANCH` is an ordinary one-writer repair disposition, not a
base-mutation authorization. Use the normal repair flow when the branch can be
fixed in place without changing its base. If that repair also needs base
reconciliation, record one of the concrete dispositions above as a separate
reason before entering this command.

Valid base-update reasons include:

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

Do not apply the no-action rule until GitHub has established mergeability. If
`mergeable` or `mergeStateStatus` is `UNKNOWN`, retry boundedly and then return
`NOT_PROVEN` without mutation. If `mergeStateStatus` is `UNSTABLE`, decompose it
into proof/review/policy state; it is neither proof of a conflict nor proof that
no integration action is needed.

Only when mergeability is proven conflict-free and no valid reason is recorded,
stop with `NO_ACTION_REQUIRED` and preserve the existing head/review/check
evidence.

## Steps

### 1. Validate the target and capture repository identity

Reduce the supplied argument to exactly one string `TARGET` without evaluating
it as shell code.

Accept only:

- one decimal pull-request number (`^[0-9]+$`); or
- one exact Git branch name that is non-empty, contains no whitespace, does not
  begin with `-`, and passes `git check-ref-format --branch "$TARGET"`.

Reject empty, multi-word, newline-containing, flag-like (`--repo`), glob, or
otherwise invalid input. Do not use `eval`, word-split the input, or append it to
a shell command string.

```bash
# TARGET is already validated data from the command runtime.
PR_STATE=$(gh pr view "$TARGET" --json \
  number,title,headRefName,headRefOid,baseRefName,baseRefOid,mergeable,mergeStateStatus,isDraft,isCrossRepository,headRepository,headRepositoryOwner)

PR_NUMBER=$(printf '%s' "$PR_STATE" | jq -r '.number')
BRANCH=$(printf '%s' "$PR_STATE" | jq -r '.headRefName')
OLD_HEAD=$(printf '%s' "$PR_STATE" | jq -r '.headRefOid')
MERGEABLE=$(printf '%s' "$PR_STATE" | jq -r '.mergeable')
MERGE_STATE_STATUS=$(printf '%s' "$PR_STATE" | jq -r '.mergeStateStatus')
IS_CROSS_REPOSITORY=$(printf '%s' "$PR_STATE" | jq -r '.isCrossRepository')
HEAD_REPO_NWO=$(printf '%s' "$PR_STATE" | jq -r '.headRepository.nameWithOwner // empty')
HEAD_OWNER=$(printf '%s' "$PR_STATE" | jq -r '.headRepositoryOwner.login // empty')
BASE_REPO_NWO=$(gh repo view --json nameWithOwner --jq .nameWithOwner)
```

When a branch name is supplied, require that GitHub resolves it to exactly one
open PR. If `HEAD_REPO_NWO` is missing (for example, a deleted fork), return
`NOT_PROVEN` or `SALVAGE_REQUIRED`; never assume the head branch lives in the
base repository.

Record:

```yaml
pr:
base_repository:
head_repository:
head_owner:
cross_repository:
branch:
old_head_sha:
base_sha:
mergeable:
merge_state:
reviewed_disposition:
base_update_reason:
```

### 2. Resolve the correct head remote and mutation authority

The PR head repository, not the base repository, owns `refs/heads/$BRANCH`.

```bash
if [ "$IS_CROSS_REPOSITORY" = "true" ]; then
  HEAD_REMOTE_URL="https://github.com/$HEAD_REPO_NWO.git"
else
  HEAD_REMOTE_URL=$(git remote get-url origin)
fi
```

Before any mutation:

- identify the branch owner/current writer and verify no other writer is active;
- verify GitHub's PR head and the head repository's branch both equal `OLD_HEAD`;
- verify maintainers are authorized to push to that head repository/branch;
- record whether history rewrite is permitted;
- inspect dirty, unpushed, or salvageable local work;
- allocate or resume one dedicated isolated worktree pinned to `OLD_HEAD`.

A fork PR without verified push permission cannot be rewritten through the base
repository. Use GitHub's protected update-branch endpoint when applicable,
request the contributor to update it, or use `SALVAGE_UNIQUE_DELTA` with an
explicit replacement owner. Never push a fork's branch name to base `origin`.

The coordination/root checkout must never be switched or used for mutation. For
a same-repository PR, the dedicated worktree may use the exact branch. For a
fork, use a clearly named local reconciliation branch or detached worktree at
`OLD_HEAD`; local branch-name equality is not required, but exact object identity
is.

```bash
COORDINATION_ROOT=$(git rev-parse --show-toplevel)
WORKTREE=<approved-dedicated-worktree-path>
test -d "$WORKTREE"
test "$(git -C "$WORKTREE" rev-parse HEAD)" = "$OLD_HEAD"
test "$(cd "$WORKTREE" && pwd -P)" != "$(cd "$COORDINATION_ROOT" && pwd -P)"
```

If an existing worktree does not equal `OLD_HEAD`, do not reset or rebase it as
if it were the PR head. Preserve unique dirty/unpushed state, then create a
separate worktree pinned to `OLD_HEAD` or return `SALVAGE_REQUIRED`.

### 3. Fetch current objects without changing the PR

```bash
git -C "$WORKTREE" fetch origin main
git -C "$WORKTREE" fetch origin "pull/$PR_NUMBER/head"
test "$(git -C "$WORKTREE" rev-parse HEAD)" = "$OLD_HEAD"
```

The base repository's `refs/pull/$PR_NUMBER/head` gives a read-only exact PR
object even for a fork. Push and remote-head verification still use
`HEAD_REMOTE_URL`.

Inspect actual conflicts, same-seam changes on current `main`, stack deltas,
prerequisite/generated-authority changes, and possible equivalent work. A
finding of no material interaction is a valid reason to leave the head unchanged,
subject to live integration policy.

### 4. Select the smallest correct strategy

| Situation | Preferred action |
| --- | --- |
| Conflict-free; no material interaction or policy requirement | leave head unchanged |
| `REPAIR_EXISTING_BRANCH` without a base reason | return to the normal repair writer |
| Reviewed textual conflict | resolve only the reviewed conflict set in the isolated worktree |
| Semantic conflict | compare models and repair intentionally; do not take one side wholesale |
| Stacked parent squash-merged | preserve the child-only delta; retarget/rebase/cherry-pick only as needed |
| Head branch cannot be rewritten | protected update-branch, contributor handoff, or explicit replacement/salvage |
| Contaminated topology with bounded unique value | `SALVAGE_UNIQUE_DELTA` to a fresh current-main branch |
| Same-head proof merely missing | request non-mutating exact-head proof refresh |

Rebase is one implementation option, not the default doctrine.

### 5. Revalidate the head repository immediately before every mutation

Before each push, force-push, update-branch, retarget, or other branch mutation,
re-read both GitHub's PR head and the head repository branch. Both must equal the
expected pre-mutation SHA.

```bash
verify_expected_remote_head() {
  CURRENT_PR_HEAD=$(gh pr view "$PR_NUMBER" --json headRefOid --jq .headRefOid) || return 2
  CURRENT_REMOTE_HEAD=$(
    git ls-remote --exit-code --heads "$HEAD_REMOTE_URL" "refs/heads/$BRANCH" |
      awk 'NR == 1 { print $1 }'
  ) || return 2

  test "$CURRENT_PR_HEAD" = "$EXPECTED_REMOTE_HEAD" || return 1
  test "$CURRENT_REMOTE_HEAD" = "$EXPECTED_REMOTE_HEAD" || return 1
}
```

Set `EXPECTED_REMOTE_HEAD=$OLD_HEAD` before the first mutation. After an
intentional successful push, update the expected SHA before any later mutation
and revalidate again.

### 6. Perform the chosen mutation only inside the isolated worktree

```bash
cd "$WORKTREE"
test "$(git rev-parse HEAD)" = "$OLD_HEAD"
```

#### Authorized rebase, including reviewed conflict resolution

```bash
if ! git rebase origin/main; then
  if [ "$REVIEWED_DISPOSITION" != "RESOLVE_CONFLICTS" ]; then
    git rebase --abort || {
      echo "SALVAGE_REQUIRED: rebase could not be aborted cleanly" >&2
      exit 2
    }
    test "$(git rev-parse HEAD)" = "$OLD_HEAD" || exit 2
    echo "BLOCKED: an unplanned conflict requires review" >&2
    exit 1
  fi

  # The reviewed conflict plan must name every allowed conflicted path and the
  # semantic resolution. Do not use blanket ours/theirs. At each stop, compare
  # `git diff --name-only --diff-filter=U` with that plan, apply only those
  # decisions, stage them, and continue with `GIT_EDITOR=: git rebase --continue`.
  # If a new/unapproved conflict appears, abort back to OLD_HEAD. If abort fails,
  # preserve the worktree as SALVAGE_REQUIRED and do not publish it.
  resolve_only_reviewed_conflicts_or_abort
fi

NEW_HEAD=$(git rev-parse HEAD)
EXPECTED_REMOTE_HEAD=$OLD_HEAD
verify_expected_remote_head || {
  echo "BLOCKED: remote head moved; do not push" >&2
  exit 1
}
git push \
  --force-with-lease="refs/heads/$BRANCH:$EXPECTED_REMOTE_HEAD" \
  "$HEAD_REMOTE_URL" HEAD:"refs/heads/$BRANCH"
```

`resolve_only_reviewed_conflicts_or_abort` is the bounded human/agent resolution
step, not an executable repository helper. It must fail closed and restore
`OLD_HEAD` when the approved plan is insufficient. Never use naked `--force`.

#### Authorized merge-main

```bash
if ! GIT_MERGE_AUTOEDIT=no git merge --no-ff --no-edit origin/main; then
  git merge --abort || {
    echo "SALVAGE_REQUIRED: merge could not be aborted cleanly" >&2
    exit 2
  }
  test "$(git rev-parse HEAD)" = "$OLD_HEAD" || {
    echo "NOT_PROVEN: merge abort did not restore the pinned head" >&2
    exit 2
  }
  echo "BLOCKED: merge conflict requires a reviewed resolution" >&2
  exit 1
fi

NEW_HEAD=$(git rev-parse HEAD)
EXPECTED_REMOTE_HEAD=$OLD_HEAD
verify_expected_remote_head || {
  echo "BLOCKED: remote head moved; do not push" >&2
  exit 1
}
git push \
  --force-with-lease="refs/heads/$BRANCH:$EXPECTED_REMOTE_HEAD" \
  "$HEAD_REMOTE_URL" HEAD:"refs/heads/$BRANCH"
```

The explicit lease provides compare-and-swap protection even when an ordinary
fast-forward push might otherwise accept an unexpected remote change.

#### Authorized GitHub branch update

```bash
EXPECTED_REMOTE_HEAD=$OLD_HEAD
verify_expected_remote_head || {
  echo "BLOCKED: remote head moved; do not update branch" >&2
  exit 1
}

if ! UPDATE_RESPONSE=$(gh api -X PUT \
  "repos/:owner/:repo/pulls/$PR_NUMBER/update-branch" \
  -f expected_head_sha="$EXPECTED_REMOTE_HEAD"); then
  echo "NOT_PROVEN: GitHub branch update failed" >&2
  exit 2
fi

# A successful request must produce an observable new PR head within a bounded
# reread budget. Continuing with OLD_HEAD would make a failed/no-op mutation look
# successful.
NEW_HEAD=""
for _ in 1 2 3 4 5 6; do
  CANDIDATE_HEAD=$(gh pr view "$PR_NUMBER" --json headRefOid --jq .headRefOid) || exit 2
  if [ "$CANDIDATE_HEAD" != "$OLD_HEAD" ]; then
    NEW_HEAD=$CANDIDATE_HEAD
    break
  fi
  sleep 5
done

test -n "$NEW_HEAD" || {
  printf '%s\n' "$UPDATE_RESPONSE" >&2
  echo "NOT_PROVEN: branch update returned success but no new head was observed" >&2
  exit 2
}
```

#### `SALVAGE_UNIQUE_DELTA`

Preserve the original head, diff, tests, and review evidence. Create a separate
owned worktree and fresh branch from current `origin/main`, apply only the bounded
unique delta, and publish it only after the replacement/supersession relationship
is explicit. Do not force-push or silently replace the original PR branch.

Any retarget or PR metadata mutation also re-reads the current head immediately
before the GitHub write and passes validated values as data fields.

### 7. Prove the published head before reporting success

For in-place mutation, re-read both GitHub and the **head repository** branch and
require them to equal `NEW_HEAD`:

```bash
PUBLISHED_PR_HEAD=$(gh pr view "$PR_NUMBER" --json headRefOid --jq .headRefOid)
PUBLISHED_REMOTE_HEAD=$(
  git ls-remote --exit-code --heads "$HEAD_REMOTE_URL" "refs/heads/$BRANCH" |
    awk 'NR == 1 { print $1 }'
)

test "$PUBLISHED_PR_HEAD" = "$NEW_HEAD" || exit 2
test "$PUBLISHED_REMOTE_HEAD" = "$NEW_HEAD" || exit 2
```

For salvage, verify the new branch separately and verify the original PR head
remains the recorded original head unless a later explicit transition repoints or
supersedes it.

### 8. Verify and report evidence invalidation

```bash
git -C "$WORKTREE" diff --check origin/main...HEAD
```

Run the smallest owning proof required by the changed/resolved seams. Then list
stale exact-head checks, review convergence for changed seams, integration proof,
generated receipts, and Changie/release-note disposition where scope changed.
A successful base operation is not merge readiness.

### 9. Return without switching the coordination checkout

```bash
cd "$COORDINATION_ROOT"
```

Do not run `git checkout -` in the coordination checkout. Retain or remove the
dedicated worktree only through the worktree/cleanup authority after proving no
dirty, unpushed, active, or salvageable state remains.

## Result

```text
### Base Reconciliation Result
- PR: #<number> (<title>)
- Base repository: <owner/name>
- Head repository: <owner/name>
- Branch: <headRefName>
- Dedicated worktree: <path>
- Old head: <full sha>
- New/published head: <full sha or unchanged>
- Disposition: RESOLVE_CONFLICTS / REVIEW_SEMANTIC_INTERACTION / UPDATE_BASE_REQUIRED / SALVAGE_UNIQUE_DELTA
- Concrete reason: <reason>
- Strategy: no action / rebase / merge-main / update / retarget / salvage
- Pre-mutation head verification: <PR and head-repository SHAs>
- Published-head verification: <PR and head-repository SHAs>
- Conflicts and semantic decisions: <details>
- Proof run: <details>
- Evidence invalidated: <details>
- Status: NO_ACTION_REQUIRED / UPDATED / BLOCKED / NOT_PROVEN / SALVAGE_REQUIRED
```
