---
name: verify-live-ci
description: Evaluate one pull request's live required checks, review threads, draft state, mergeability, and policy without exact-head review receipts, branch churn, or sibling-lane surveillance.
user-invocable: false
---

# Verify live CI

Read one current GitHub snapshot for the selected PR: draft/ready state, required checks from live policy, unresolved review threads, current `CHANGES_REQUESTED` reviews, deliberately requested reviewers still pending, mergeability, conflicts, queue/ruleset state, and applicable changelog/support disposition.

Use repository helpers where they report these native facts truthfully. Do not require `review-run` comments, a material-claim digest, a human review submitted on the latest commit, or `REVIEW_PROTOCOL_ENFORCE=1` review-receipt convergence.

Review is not stale merely because the PR head changed. Evaluate later commits semantically:

- finding repair → check the affected finding, proof, and seam;
- material claim, production route, authority, risk, rollback, or proof change → review the affected dimensions;
- formatting, editorial cleanup, generated receipt refresh, or stronger tests → no automatic full-review restart;
- conflict or integration repair → focused review of the repaired seam.

Stale bot or human review timestamps may be reported as context. They do not block by themselves.

Preserve success, failure, pending, not-applicable, cancelled, stale-check-result, missing, instrument-failure, and not-proven states distinctly. A successful check on an older candidate is stale check evidence; partial API data that can change the conclusion is `NOT_PROVEN`.

Do not update or rebase merely because a conflict-free branch is behind. Resolve actual conflicts, explicit stack changes, or combined-tree failures in this lane and rerun only affected proof/review.

## Routes

- `INTEGRATION_READY` → `merge-reconcile`
- `DRAFT` → `publish-pr`
- `PENDING` → record the exact pending transition once and return `PR_IN_FLIGHT`
- `PRODUCT_OR_TEST_FAILURE` → `build-candidate`
- `REVIEW_FINDINGS_OPEN` → `address-review-comments`
- `REVIEW_SCOPE_CHANGED` → review the affected dimensions
- `CONFLICT` / `INTEGRATION_INTERACTION` → repair the affected seam and rerun affected proof/review
- `INSTRUMENT_FAILURE` / `NOT_PROVEN` → name the missing reliable evidence
