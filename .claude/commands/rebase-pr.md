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

Before any mutation, require a reviewed disposition of:

- `RESOLVE_CONFLICTS`;
- `REVIEW_SEMANTIC_INTERACTION`;
- `UPDATE_BASE_REQUIRED` with a concrete reason; or
- `SALVAGE_UNIQUE_DELTA` when the existing branch topology/contamination makes
  in-place repair unsafe or unreviewable.

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

### 1. Validate the target before invoking GitHub

The command runtime/agent must first reduce the supplied argument to exactly one
string `TARGET` without evaluating it as shell code.

Accept only:

- one decimal pull-request number (`^[0-9]+$`); or
- one exact Git branch name that:
  - is non-empty;
  - contains no whitespace;
  - does not begin with `-`;
  - passes `git check-ref-format --branch "$TARGET"`.

Reject empty, multi-word, newline-containing, flag-like (`--repo`), glob, or
otherwise invalid input. Do not invoke `eval`, word-split the input, or append it
to a shell command string.

After validation, pass it as one quoted argument:

```bash
# TARGET is already validated data from the command runtime.
PR_STATE=$(gh pr view "$TARGET" \
  --json number,title,headRefName,headRefOid,baseRefName,baseRefOid,mergeable,mergeStateStatus,isDraft)
PR_NUMBER=$(printf '%s' "$PR_STATE" | jq -r '.number')
BRANCH=$(printf '%s' "$PR_STATE" | jq -r '.headRefName')
OLD_HEAD=$(printf '%s' "$PR_STATE" | jq -r '.headRefOid')
MERGEABLE=$(printf '%s' "$PR_STATE" | jq -r '.mergeable')
MERGE_STATE_STATUS=$(printf '%s' "$PR_STATE" | jq -r '.mergeStateStatus')
```

When a branch name is supplied, require that GitHub resolves it to exactly one
open PR. Apply the `UNKNOWN`/`UNSTABLE` fail-closed rule above before choosing
`NO_ACTION_REQUIRED` or mutating.

Record:

```yaml
pr:
branch:
old_head_sha:
base_sha:
mergeable:
merge_state:
reviewed_disposition:
base_update_reason:
```

### 2. Verify ownership, pinned-head worktree isolation, and mutation authority

Before checkout, rebase, or any remote mutation:

- identify the branch owner/current writer;
- verify no other writer is active on the branch;
- verify the remote head equals `old_head_sha`;
- verify maintainers are authorized to modify the branch;
- record whether history rewrite is permitted;
- inspect dirty, unpushed, or salvageable work;
- allocate or resume one **dedicated isolated worktree** for this PR branch;
- require that the worktree HEAD equals the pinned GitHub `OLD_HEAD`, not merely
  that its local branch name matches.

The coordination/root checkout must never be switched or used for mutation. The
selected worktree must be a different path, map to the expected branch/head, and
be owned by this operation. If a dedicated worktree cannot be established or
ownership is ambiguous, return `BLOCKED` or `NOT_PROVEN`.

Example verification after allocation through the repository worktree manager:

```bash
COORDINATION_ROOT=$(git rev-parse --show-toplevel)
WORKTREE=<approved-dedicated-worktree-path>
test -d "$WORKTREE"
test "$(git -C "$WORKTREE" rev-parse --abbrev-ref HEAD)" = "$BRANCH"
test "$(git -C "$WORKTREE" rev-parse HEAD)" = "$OLD_HEAD"
test "$(cd "$WORKTREE" && pwd -P)" != "$(cd "$COORDINATION_ROOT" && pwd -P)"
```

If an existing local branch/worktree does not equal `OLD_HEAD`, do not rebase or
reset it as if it were the PR head. First preserve any unique dirty/unpushed
state, then create a separate dedicated worktree pinned to `OLD_HEAD` or return
`SALVAGE_REQUIRED`.

Do not create a second worktree for a branch already checked out elsewhere.
Resume the proven owner or stop.

### 3. Fetch current objects without changing the PR

From the dedicated worktree:

```bash
git -C "$WORKTREE" fetch origin main
git -C "$WORKTREE" fetch origin "$BRANCH"
```

After fetch, re-check that the worktree remains at `OLD_HEAD`; fetching the
remote must not silently substitute a newer local branch head.

Inspect:

- actual textual conflicts;
- current-main changes to the same semantic seam;
- stacked parent/child delta;
- prerequisite and generated-authority changes;
- whether an equivalent implementation already landed.

A finding of no material interaction is a valid reason to stop and leave the
head unchanged, subject to live integration policy.

### 4. Select the smallest correct strategy

| Situation | Preferred action |
| --- | --- |
| Conflict-free; no material interaction or policy requirement | leave head unchanged |
| Simple mechanical textual conflict | resolve with the least destructive branch update compatible with ownership |
| Semantic conflict | compare models and repair intentionally; do not take one side wholesale |
| Stacked parent squash-merged | preserve the child-only delta; retarget/rebase/cherry-pick only as needed |
| Contributor branch cannot be rewritten | merge-main or create a bounded replacement only with explicit ownership/disposition |
| Contaminated topology with a bounded unique delta | `SALVAGE_UNIQUE_DELTA`: preserve it on a fresh current-main branch; do not rewrite the original merely for cleanliness |
| Same-head proof merely missing | stop; request non-mutating exact-head proof refresh |

Rebase is one implementation option, not the default doctrine.

### 5. Revalidate the remote head immediately before every mutation

The initial ownership read is not enough. A writer may push while inspection or
local conflict resolution is in progress.

Before **each** push, force-push, GitHub branch update, retarget, or other remote
branch mutation:

1. re-read the PR head from GitHub;
2. re-read `refs/heads/$BRANCH` from the remote;
3. require both to equal the expected pre-mutation head;
4. abort with `BLOCKED`/`NOT_PROVEN` on missing or mismatched state.

```bash
verify_expected_remote_head() {
  CURRENT_PR_HEAD=$(gh pr view "$PR_NUMBER" --json headRefOid --jq .headRefOid) || return 2
  CURRENT_REMOTE_HEAD=$(
    git ls-remote --exit-code --heads origin "refs/heads/$BRANCH" |
      awk 'NR == 1 { print $1 }'
  ) || return 2

  test "$CURRENT_PR_HEAD" = "$EXPECTED_REMOTE_HEAD" || return 1
  test "$CURRENT_REMOTE_HEAD" = "$EXPECTED_REMOTE_HEAD" || return 1
}
```

Set `EXPECTED_REMOTE_HEAD=$OLD_HEAD` before the first mutation. After a successful
push that intentionally continues the same operation, update the expected head
to the newly published SHA before any later mutation and revalidate again.

### 6. Perform the chosen mutation only inside the isolated worktree

Enter the dedicated worktree without checking out branches in the coordination
checkout:

```bash
cd "$WORKTREE"
test "$(git rev-parse --abbrev-ref HEAD)" = "$BRANCH"
test "$(git rev-parse HEAD)" = "$OLD_HEAD"
```

#### Authorized rebase

```bash
if ! git rebase origin/main; then
  # Resolve only when the reviewed semantic decision is sufficient. Otherwise
  # restore the pinned pre-rebase state before returning BLOCKED.
  if ! git rebase --abort; then
    echo "SALVAGE_REQUIRED: rebase could not be aborted cleanly; preserve this worktree" >&2
    exit 2
  fi
  test "$(git rev-parse HEAD)" = "$OLD_HEAD" || {
    echo "NOT_PROVEN: abort did not restore the pinned head" >&2
    exit 2
  }
  echo "BLOCKED: conflict requires a new reviewed resolution" >&2
  exit 1
fi

NEW_HEAD=$(git rev-parse HEAD)
EXPECTED_REMOTE_HEAD=$OLD_HEAD
verify_expected_remote_head || {
  echo "BLOCKED: remote head moved; do not force-push" >&2
  exit 1
}
git push \
  --force-with-lease="refs/heads/$BRANCH:$EXPECTED_REMOTE_HEAD" \
  origin HEAD:"refs/heads/$BRANCH"
```

Never use naked `--force`. If conflict work is intentionally preserved instead
of aborted, mark the dedicated worktree `SALVAGE_REQUIRED`, record its exact
rebase state, and do not report the PR as updated.

#### Authorized merge-main

```bash
git merge --no-ff origin/main
NEW_HEAD=$(git rev-parse HEAD)
EXPECTED_REMOTE_HEAD=$OLD_HEAD
verify_expected_remote_head || {
  echo "BLOCKED: remote head moved; do not push" >&2
  exit 1
}
git push origin HEAD:"refs/heads/$BRANCH"
```

The normal push provides a second server-side non-fast-forward guard if the
remote moves after the explicit re-read.

#### Authorized GitHub branch update

Use the endpoint's compare-and-swap field rather than a blind update:

```bash
EXPECTED_REMOTE_HEAD=$OLD_HEAD
verify_expected_remote_head || {
  echo "BLOCKED: remote head moved; do not update branch" >&2
  exit 1
}
gh api -X PUT "repos/:owner/:repo/pulls/$PR_NUMBER/update-branch" \
  -f expected_head_sha="$EXPECTED_REMOTE_HEAD"
NEW_HEAD=$(gh pr view "$PR_NUMBER" --json headRefOid --jq .headRefOid)
```

#### `SALVAGE_UNIQUE_DELTA`

- preserve the original head, diff, tests, and review evidence;
- create a separate dedicated worktree and fresh branch from current
  `origin/main`;
- apply only the bounded unique delta;
- do not force-push or silently replace the original PR branch;
- publish/repoint only after the new owner and supersession relationship are
  explicit;
- report both original and salvage heads.

Any retarget or PR metadata mutation also re-reads the current head immediately
before the GitHub write and passes validated values as quoted/data fields.

### 7. Prove the published head before reporting success

A successful local operation or push command is not enough. Re-read both GitHub
and the remote branch and require them to equal the resulting `NEW_HEAD`:

```bash
PUBLISHED_PR_HEAD=$(gh pr view "$PR_NUMBER" --json headRefOid --jq .headRefOid)
PUBLISHED_REMOTE_HEAD=$(
  git ls-remote --exit-code --heads origin "refs/heads/$BRANCH" |
    awk 'NR == 1 { print $1 }'
)

test "$PUBLISHED_PR_HEAD" = "$NEW_HEAD" || {
  echo "NOT_PROVEN: PR head does not equal the resulting head" >&2
  exit 2
}
test "$PUBLISHED_REMOTE_HEAD" = "$NEW_HEAD" || {
  echo "NOT_PROVEN: remote branch does not equal the resulting head" >&2
  exit 2
}
```

For `SALVAGE_UNIQUE_DELTA`, verify the new salvage branch separately and verify
that the original PR head remains the recorded original head unless an explicit
later transition repoints or supersedes it.

### 8. Verify and report evidence invalidation

```bash
git -C "$WORKTREE" diff --check origin/main...HEAD
```

Run the smallest owning proof required by the changed/resolved seams. Then state
which prior evidence is stale:

- exact-head checks;
- review convergence for changed seams;
- integration proof;
- generated receipts;
- Changie/release-note disposition when implementation scope changed.

Do not claim the PR is ready merely because the base operation succeeded.

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
- Branch: <headRefName>
- Dedicated worktree: <path>
- Old head: <full sha>
- New/published head: <full sha or unchanged>
- Disposition: RESOLVE_CONFLICTS / REVIEW_SEMANTIC_INTERACTION / UPDATE_BASE_REQUIRED / SALVAGE_UNIQUE_DELTA
- Concrete reason: <reason>
- Strategy: no action / rebase / merge-main / update / retarget / salvage
- Remote-head revalidation: <expected and observed heads before mutation>
- Published-head verification: <PR and remote heads after mutation>
- Conflicts and semantic decisions: <details>
- Proof run: <details>
- Evidence invalidated: <details>
- Status: NO_ACTION_REQUIRED / UPDATED / BLOCKED / NOT_PROVEN / SALVAGE_REQUIRED
```
