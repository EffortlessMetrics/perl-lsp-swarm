---
description: Ops step 2 — squash-merge up to 3 reviewed PRs using expected-head authorization
user-invocable: false
---

# Ops Merge Batch

Merge up to three candidates from `/ops-check-queue`. The batch limit is an
operational throttle, not a reason to update every branch after `main` moves.

Canonical authorities:

- PR disposition: `docs/specs/PLSP-SPEC-0006-pr-queue-disposition.md`
- authority map: `docs/reference/CONTROL_PLANE_AUTHORITY.md`
- review convergence: `scripts/ci/check-pr-review-convergence`
- required checks and merge permission: live GitHub policy

## Steps

### 1. Choose candidates and order only real dependencies

Respect explicit stacks, same-authority overlap, generated files, public API
collisions, and known integration dependencies. Do not order or update a PR only
because it is older or further behind.

A prior merge moving `main` does not invalidate an unchanged PR-head review. It
may invalidate a separate integration receipt when that receipt was bound to the
old base.

### 2. Capture one live readiness packet per PR

Immediately before each merge, capture:

```bash
gh pr view <number> \
  --json isDraft,mergeable,mergeStateStatus,labels,headRefOid,baseRefOid,reviewRequests,reviewDecision
gh api repos/:owner/:repo/commits/<head-sha>/check-runs --paginate
scripts/ci/check-pr-review-convergence <number>
```

> **MCP alternative:** fetch the PR, exact-head check runs, review threads, and
> requested reviews through the canonical connector map. Keep the packet bound
> to the captured full head SHA.

Required at merge time:

- PR is not draft;
- GitHub reports no actual textual conflict;
- required checks discovered from live policy succeed on the exact head;
- review convergence succeeds for the exact head;
- no active repair request contradicts readiness;
- Changie/release-note disposition and other live policy inputs are satisfied;
- applicable integration proof is current when the risk/policy trigger requires
  it;
- the PR head remains unchanged after evidence collection.

Labels are navigation/projected state, not proof. Advisory checks remain visible
but do not become required merely because they are present or red.

`UNKNOWN` is `NOT_PROVEN`. `UNSTABLE` must be decomposed into required,
advisory, review, policy, or platform state. `DIRTY`/`CONFLICTING` is an actual
conflict to inspect, not an automatic rebase instruction.

### 3. Run the repository pre-merge guard

```bash
just pre-merge-check <number>
# or: bash scripts/pre-merge-check.sh <number>
```

Treat its result according to the current repository contract. A local tool or
instrument failure is `NOT_PROVEN`; it is not permission to bypass protected
GitHub evidence.

### 4. Prepare a useful squash commit message

```bash
gh pr view <number> --json title,body
```

Use `<PR title> (#<number>)` as the subject and a concise explanation of what
changed and why as the body.

### 5. Re-read and squash-merge the expected head

```bash
EXPECTED_HEAD=$(gh pr view <number> --json headRefOid --jq .headRefOid)
# Re-evaluate readiness for EXPECTED_HEAD, then:
gh api -X PUT repos/:owner/:repo/pulls/<number>/merge \
  -f merge_method=squash \
  -f sha="$EXPECTED_HEAD" \
  -f commit_title="<title> (#<number>)" \
  -f commit_message="<summary>"
```

> **MCP alternative:** call the merge endpoint with `merge_method: "squash"`
> and `expected_head_sha: <full sha>`.

If the head moved, the merge must fail closed and return to current-head review.
Never silently merge the replacement head.

Do not use `--admin`, protection bypass, or naked force operations.

### 6. Verify and reconcile

After a successful merge:

1. verify the PR reports merged and capture the merge SHA;
2. fetch/inspect current `main` and verify the expected result landed;
3. hand exact head/merge identity to the reconciliation path;
4. update the controlling issue/spec/proof/Changie state accurately;
5. remove only labels, branches, and worktrees whose ownership and cleanup
   safety are proven;
6. preserve salvage or ambiguous work.

A squash merge means the feature branch's commit ancestry is not the mainline
history. Do not use commit ancestry alone to decide that a worktree or unique
branch delta is safe to delete.

### 7. Handle non-ready results precisely

- `CONFLICTING` → inspect mechanical versus semantic conflict; select a reviewed
  resolution strategy.
- `UNKNOWN_NOT_PROVEN` → retry boundedly or report missing state.
- required check pending → wait for that named input.
- required check failed → classify product/test/instrument/policy failure.
- current-head proof missing/stale → request same-head rerun/dispatch; do not
  mutate the branch merely to trigger CI.
- head moved → `RETURN_TO_REVIEW`.
- draft → remain in review.
- active `needs-*` → route the named repair.
- advisory failure only → record it and apply the declared advisory policy.
- integration basis stale → refresh integration proof; do not automatically
  change the PR head.

### 8. Verify main health at bounded checkpoints

After a high-risk parser/lexer/public-API merge, or after the batch, verify the
latest `main` run for the merged SHA. Halt when `main` has a real required
failure. Distinguish an instrument/capacity failure from a product regression
before changing source.

## Output

```text
Merged: #NNN @ <expected-head> -> <merge-sha>
Skipped: #NNN (result class, exact blocker, next action)
Main verification: <sha> <result>
Reconciliation: complete / partial / salvage-required / not-proven
```
