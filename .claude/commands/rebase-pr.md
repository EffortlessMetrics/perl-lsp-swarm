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
- `SALVAGE_UNIQUE_DELTA` when the existing branch topology or contamination
  makes in-place repair unsafe or unreviewable.

`REPAIR_EXISTING_BRANCH` is an ordinary one-writer repair disposition, not a
base-mutation authorization. Route it to the normal repair flow when the branch
can be fixed in place without changing its base. When that repair also needs
base reconciliation, record one of the concrete dispositions above first.

Valid base-update reasons include:

- an actual textual conflict;
- current `main` changed the same semantic contract and the PR must adapt;
- a stacked prerequisite changed so the child cannot be reviewed or tested
  independently;
- live branch protection or merge-queue policy requires a current integration
  basis;
- meaningful proof cannot be interpreted without the updated prerequisite or
  contract.

Age, inactivity, commits-behind, unrelated `main` movement, non-linear history,
a preference for a clean graph, or a desire to retrigger CI are insufficient.

## 1. Validate the target and capture repository identity

Reduce the supplied argument to exactly one `TARGET` string without evaluating
it as shell code.

Accept only:

- one decimal pull-request number (`^[0-9]+$`); or
- one exact Git branch name that is non-empty, contains no whitespace, does not
  begin with `-`, and passes `git check-ref-format --branch "$TARGET"`.

Reject all other input. Do not use `eval`, word-split the value, or append it to
a shell command string.

```bash
PR_STATE=$(gh pr view "$TARGET" --json \
  number,title,headRefName,headRefOid,baseRefName,baseRefOid,mergeable,mergeStateStatus,isDraft,isCrossRepository,headRepository,headRepositoryOwner)

PR_NUMBER=$(printf '%s' "$PR_STATE" | jq -r '.number')
BRANCH=$(printf '%s' "$PR_STATE" | jq -r '.headRefName')
OLD_HEAD=$(printf '%s' "$PR_STATE" | jq -r '.headRefOid')
BASE_BRANCH=$(printf '%s' "$PR_STATE" | jq -r '.baseRefName')
BASE_SHA=$(printf '%s' "$PR_STATE" | jq -r '.baseRefOid')
MERGEABLE=$(printf '%s' "$PR_STATE" | jq -r '.mergeable')
MERGE_STATE_STATUS=$(printf '%s' "$PR_STATE" | jq -r '.mergeStateStatus')
IS_CROSS_REPOSITORY=$(printf '%s' "$PR_STATE" | jq -r '.isCrossRepository')
HEAD_REPO_NWO=$(printf '%s' "$PR_STATE" | jq -r '.headRepository.nameWithOwner // empty')
HEAD_OWNER=$(printf '%s' "$PR_STATE" | jq -r '.headRepositoryOwner.login // empty')
BASE_REPO_NWO=$(gh repo view --json nameWithOwner --jq .nameWithOwner)
```

When a branch name is supplied, require GitHub to resolve it to exactly one open
PR. If the head repository is missing, return `NOT_PROVEN` or
`SALVAGE_REQUIRED`; never assume the branch lives in the base repository.

If `MERGEABLE` or `MERGE_STATE_STATUS` is `UNKNOWN`, retry boundedly and then
return `NOT_PROVEN`. If the state is `UNSTABLE`, decompose proof, review, and
policy state before deciding. Apply `NO_ACTION_REQUIRED` only after GitHub has
proved the PR conflict-free and no valid base-update reason remains.

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

## 2. Resolve the head remote and prove mutation authority

The PR head repository owns `refs/heads/$BRANCH`.

```bash
if [ "$IS_CROSS_REPOSITORY" = "true" ]; then
  HEAD_REMOTE_URL="https://github.com/$HEAD_REPO_NWO.git"
else
  HEAD_REMOTE_URL=$(git remote get-url origin)
fi
```

Before mutation:

- identify the branch owner/current writer and verify no other writer is active;
- verify maintainers are authorized to push to the head repository/branch;
- record whether history rewrite is permitted;
- inspect dirty, unpushed, or salvageable local work;
- allocate or resume one dedicated isolated worktree pinned to `OLD_HEAD`.

A fork PR without verified push permission cannot be rewritten through base
`origin`. Use GitHub's protected update-branch endpoint when applicable, request
the contributor to update it, or use `SALVAGE_UNIQUE_DELTA` with an explicit
replacement owner. Never push a fork branch name to the base repository.

The coordination checkout must never be switched or used for mutation.

```bash
COORDINATION_ROOT=$(git rev-parse --show-toplevel)
WORKTREE=<approved-dedicated-worktree-path>
test -d "$WORKTREE"
test "$(git -C "$WORKTREE" rev-parse HEAD)" = "$OLD_HEAD"
test "$(cd "$WORKTREE" && pwd -P)" != "$(cd "$COORDINATION_ROOT" && pwd -P)"
```

If an existing worktree differs from `OLD_HEAD`, preserve its unique state and
create a separate pinned worktree or return `SALVAGE_REQUIRED`. Do not reset it
as though it were the PR head.

## 3. Fetch current objects without changing the PR

```bash
git -C "$WORKTREE" fetch origin "$BASE_BRANCH"
git -C "$WORKTREE" fetch origin "pull/$PR_NUMBER/head"
test "$(git -C "$WORKTREE" rev-parse HEAD)" = "$OLD_HEAD"
```

The base repository's `refs/pull/$PR_NUMBER/head` provides a read-only exact PR
object even for a fork. Push and remote-head verification still use
`HEAD_REMOTE_URL`.

Inspect:

- actual textual conflicts;
- current-main changes to the same semantic seam;
- stack parent/child deltas;
- prerequisite and generated-authority changes;
- equivalent landed or open implementations.

A finding of no material interaction is a valid reason to leave the head
unchanged, subject to live integration policy.

## 4. Select the smallest correct strategy

| Situation | Preferred action |
| --- | --- |
| Conflict-free; no material interaction or policy requirement | leave head unchanged |
| `REPAIR_EXISTING_BRANCH` without a base reason | return to the normal repair writer |
| Reviewed textual conflict | use the bounded rebase conflict procedure below |
| Semantic conflict | compare models and update the reviewed conflict plan before mutation |
| Stacked parent squash-merged | preserve the child-only delta; retarget/rebase/cherry-pick only as needed |
| Head branch cannot be rewritten | protected update-branch, contributor handoff, or explicit replacement/salvage |
| Contaminated topology with bounded unique value | `SALVAGE_UNIQUE_DELTA` to a fresh current-main branch |
| Same-head proof merely missing | request non-mutating exact-head proof refresh |

Rebase is one implementation option, not the default doctrine.

## 5. Revalidate the head repository before every remote mutation

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

## 6. Perform the selected mutation in the isolated worktree

```bash
cd "$WORKTREE"
test "$(git rev-parse HEAD)" = "$OLD_HEAD"
```

### Authorized rebase without conflicts

```bash
git rebase "origin/$BASE_BRANCH"
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

### Authorized rebase with a reviewed conflict plan

The reviewed plan must name every expected conflicted path and the semantic
resolution for each. Start the rebase:

```bash
if git rebase "origin/$BASE_BRANCH"; then
  : # no conflict; continue to the push procedure above
else
  git diff --name-only --diff-filter=U | sort -u >.git/reviewed-rebase-unmerged
fi
```

For each rebase stop:

1. compare `.git/reviewed-rebase-unmerged` with the approved conflict-path list;
2. if any path or semantic decision is not approved, run `git rebase --abort`;
3. require abort to restore `OLD_HEAD`; if it does not, preserve the worktree as
   `SALVAGE_REQUIRED` and publish nothing;
4. otherwise apply only the approved resolutions, stage exactly those paths,
   require `git diff --name-only --diff-filter=U` to be empty, and run
   `GIT_EDITOR=: git rebase --continue`;
5. repeat the comparison at every later stop;
6. after completion, run the same expected-head lease and push procedure as the
   conflict-free rebase.

Do not use blanket ours/theirs resolution or an undefined helper. An unexpected
conflict returns to review rather than being guessed through.

### Authorized conflict-free merge-main

Use this strategy only when the reviewed plan expects no textual conflict. A
failure is aborted and returned for a reviewed conflict decision.

```bash
if ! GIT_MERGE_AUTOEDIT=no git merge --no-ff --no-edit "origin/$BASE_BRANCH"; then
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

### Authorized GitHub branch update

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

### `SALVAGE_UNIQUE_DELTA`

Preserve the original head, diff, tests, and review evidence. Create a separate
owned worktree and fresh branch from current `origin/$BASE_BRANCH`, apply only the
bounded unique delta, and publish it only after the replacement/supersession
relationship is explicit. Do not force-push or silently replace the original PR
branch.

Any retarget or metadata mutation also re-reads the current head immediately
before the GitHub write and passes validated values as data fields.

## 7. Prove the published head before reporting success

For in-place mutation, require both GitHub and the head repository branch to
equal `NEW_HEAD`:

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
remains unchanged unless a later explicit transition repoints or supersedes it.

## 8. Verify proof and evidence invalidation

```bash
git -C "$WORKTREE" diff --check "origin/$BASE_BRANCH"...HEAD
```

Run the smallest owning proof required by the resolved seams. List stale
exact-head checks, review convergence for changed seams, integration proof,
generated receipts, and Changie/release-note disposition where scope changed.
A successful base operation is not merge readiness.

## 9. Return without switching the coordination checkout

```bash
cd "$COORDINATION_ROOT"
```

Do not run `git checkout -` in the coordination checkout. Retain or remove the
dedicated worktree only through the cleanup authority after proving no dirty,
unpushed, active, or salvageable state remains.

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
