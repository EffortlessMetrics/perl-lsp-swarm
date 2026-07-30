---
name: merge-reconcile
description: Explicit atomic skill for expected-head squash merge when the PR is open, or evidence-backed closeout when it is already merged/closed, followed by current-main reconciliation.
---

# Merge and reconcile

## State branch

Inspect the live PR state first.

- `MERGED` → skip merge and begin current-main reconciliation.
- `CLOSED_UNMERGED` with an existing durable close/supersede disposition → skip merge and reconcile only within that recorded disposition.
- `CLOSED_UNMERGED` without a durable disposition → `NOT_PROVEN`; do not invent deliberate closure, close the controlling issue, or release residual ownership.
- `OPEN` → follow the protected merge path below.
- unknown or partial state → `NOT_PROVEN`.

## Protected merge path

Before merging an open PR, verify:

- the full expected reviewed candidate SHA;
- the current visible normalized material claim/review-index digest from `scripts/reviews/claim-digest --pr <n> [--repo owner/repo]`;
- `REVIEW_PROTOCOL_ENFORCE=1 scripts/ci/check-pr-review-convergence <n> [owner/repo]` passes for that same candidate and claim;
- live required checks and mergeability;
- PR is ready, not draft;
- no unresolved actual conflict;
- changelog, support, release, or migration disposition where applicable.

Squash-merge through current repository protection using expected-head compare-and-swap semantics, for example:

```text
gh pr merge <n> --squash --match-head-commit <reviewed-head-sha>
```

Do not bypass policy. If the candidate head or material claim moved, do not merge; rerun affected proof, final challenge, and formal review.

## Reconciliation

After merge or evidence-backed deliberate closure:

1. verify the landed/current-main effect where applicable;
2. update or close the controlling issue accurately;
3. keep umbrellas open when only one slice landed;
4. update durable contracts, proof, support claims, and changelog only within the proven boundary;
5. preserve partial or residual work explicitly;
6. safely release branch/worktree ownership and residue;
7. expose the next coherent claim.

The future squash commit is integration evidence; it does not retroactively replace the candidate and material claim reviewed before merge.

## Routes

- `RECONCILED` → return to `$deliver-pr` or `$deliver-goal`
- `PARTIAL` → preserve remaining acceptance and return to the owning flow
- `SUPERSEDED` / `DELIBERATELY_CLOSED` → preserve the existing durable disposition and remaining graph
- `CANDIDATE_MOVED` / `CLAIM_REVIEW_STALE` → rerun affected proof, `$final-challenge`, and `$review-pr`
- `MERGE_BLOCKED` / `NOT_PROVEN` → preserve the exact current blocker or missing evidence
