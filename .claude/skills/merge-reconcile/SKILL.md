---
name: merge-reconcile
description: Expected-head squash merge for an open PR, or evidence-backed closeout for an already merged/closed PR, followed by current-main reconciliation.
user-invocable: false
---

# Merge and reconcile

Inspect live PR state first:

- merged → skip merge and reconcile current main;
- closed unmerged with an existing durable close/supersede disposition → reconcile only within that recorded disposition;
- closed unmerged without a durable disposition → `NOT_PROVEN`; do not invent intent, close the controlling issue, or release residual ownership;
- open → resolve the current visible material claim digest with `scripts/reviews/claim-digest --pr <n> [--repo owner/repo]`, then verify the reviewed head, `REVIEW_PROTOCOL_ENFORCE=1 scripts/ci/check-pr-review-convergence <n> [owner/repo]`, live checks, ready state, mergeability, conflict state, and applicable changelog/support disposition against that same candidate;
- unknown/partial → `NOT_PROVEN`.

For an open PR, squash-merge through current protection with expected-head compare-and-swap, for example `gh pr merge <n> --squash --match-head-commit <reviewed-head-sha>`. If the candidate head or material claim moved, rerun affected proof, final challenge, and formal review.

Then update controlling and umbrella issues, durable claims within proof, residual work, and branch/worktree cleanup.

## Routes

- `RECONCILED` → `deliver-pr` or `deliver-goal`
- `PARTIAL` → preserve remaining acceptance
- `SUPERSEDED` / `DELIBERATELY_CLOSED` → preserve the existing durable disposition
- `CANDIDATE_MOVED` / `CLAIM_REVIEW_STALE` → rerun affected proof, `final-challenge`, and `review-pr`
- `MERGE_BLOCKED` / `NOT_PROVEN` → preserve the exact blocker or missing evidence
