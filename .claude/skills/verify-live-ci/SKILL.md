---
name: verify-live-ci
description: Evaluate one current PR candidate's live checks, review/thread convergence, material-claim currentness, draft state, mergeability, and policy without branch churn or sibling-lane surveillance.
user-invocable: false
---

# Verify live CI

Resolve the selected PR's current head/base and reject integration readiness while draft.

Run the canonical composite checker in enforced protocol mode:

```text
REVIEW_PROTOCOL_ENFORCE=1 scripts/ci/check-pr-review-convergence <pr-number> [owner/repo]
```

Do not invoke the internal claim-currentness helper as a parallel authority. The public convergence command owns review/thread/receipt/disposition state and proves material-claim currentness against the same current candidate snapshot. Enforced mode makes running-review receipts, stale receipt heads, missing disposition markers, and required independent verification blocking rather than advisory. Failure, candidate movement, or partial data is `NOT_PROVEN`.

Discover required checks from live policy and bind evidence to the candidate it evaluated. Preserve failure, pending, stale, missing, cancelled, instrument-failure, and not-proven states distinctly. Read the draft's named purpose and completion condition as part of live state.

Do not inspect sibling PR implementations, touched-file overlap, or neighbouring worktrees. A conflict-free branch is not defective merely because `main` advanced. An interaction exists only when Git reports a conflict, an explicit prerequisite changed, or actual merge-group/synthetic integration proof failed.

This lane resolves its own actual conflict or integration repair and reruns only affected evidence/review. Unrelated `main` movement requires no action.

## Routes

- `INTEGRATION_READY` → `merge-reconcile`
- `DRAFT` → `publish-pr` to evaluate the named purpose and perform the explicit ready transition when complete
- `PENDING` → record the exact pending transition once and return `PR_IN_FLIGHT` to the invoking flow
- `PRODUCT_OR_TEST_FAILURE` → `build-candidate`
- `REVIEW_FINDINGS_OPEN` → `address-review-comments`
- `CLAIM_REVIEW_STALE` → `final-challenge`, then `review-pr`
- `CONFLICT` → resolve this lane's conflict and rerun affected evidence/review
- `INTEGRATION_INTERACTION` → repair the smallest affected candidate and rerun affected evidence/review
- `INSTRUMENT_FAILURE` / `NOT_PROVEN` → name the missing evidence
