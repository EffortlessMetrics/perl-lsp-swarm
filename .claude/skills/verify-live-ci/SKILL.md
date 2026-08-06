---
name: verify-live-ci
description: Evaluate one substantively reviewed pull request's live required checks, review threads, draft state, mergeability, and policy without treating CI as review or creating exact-head review churn.
user-invocable: false
---

# Verify live CI

Apply [`docs/agents/PR_REVIEW_STANDARD.md`](../../../docs/agents/PR_REVIEW_STANDARD.md)
and require a useful current substantive review before integration can become ready.

Read one current GitHub snapshot for the selected PR: draft/ready state, cumulative
substantive review result, required checks from live policy, unresolved review
threads, current `CHANGES_REQUESTED` reviews, deliberately requested reviewers still
pending, mergeability, conflicts, queue/ruleset state, explicit prerequisites, and
applicable changelog/support disposition.

Use repository helpers where they report these native facts truthfully. Do not require
`review-run` comments, a material-claim digest, a human review submitted on the latest
commit, or `REVIEW_PROTOCOL_ENFORCE=1` review-receipt convergence.

## Review sufficiency boundary

`verify-live-ci` is an integration-fact skill. It does not perform or infer substantive
review.

- no useful current review for a substantive candidate → `REVIEW_REQUIRED`;
- `CHANGES_REQUIRED` → `REVIEW_FINDINGS_OPEN`;
- `NOT_PROVEN` → preserve the missing review evidence or authority;
- `BLOCKED_BY_PREREQUISITE` → preserve the exact prerequisite;
- `SUPERSEDED_OR_CLOSE` → preserve the durable closeout disposition;
- `REVIEW_CURRENT` → continue evaluating live integration facts.

Green checks, textual mergeability, zero open threads, bot approval, or author
self-certification cannot promote a candidate to `REVIEW_CURRENT`.

The integration result is separate:

```text
INTEGRATION_READY
PR_IN_FLIGHT
MERGE_BLOCKED
NOT_PROVEN
```

Pending checks therefore produce `PR_IN_FLIGHT` while the substantive review remains
current.

## Review currentness

A review is not stale merely because the PR head changed. Evaluate later commits
semantically:

- finding repair → check the affected finding, proof, and seam;
- material claim, production route, authority, risk, rollback, or proof change →
  review the affected dimensions;
- formatting, editorial cleanup, generated receipt refresh, or stronger tests → no
  automatic full-review restart;
- conflict or integration repair → focused review of the repaired seam.

Stale bot or human review timestamps may be reported as context. They do not block by
themselves.

## Live evidence states

Preserve success, failure, pending, not-applicable, cancelled, stale-check-result,
missing, instrument-failure, and not-proven states distinctly. A successful check on
an older candidate is stale check evidence; partial API data that can change the
conclusion is `NOT_PROVEN`.

Classify a failure as candidate-owned, base-owned, integration interaction,
test/oracle defect, instrument failure, environment/capacity, pending, or
`NOT_PROVEN`. Do not widen the PR to absorb unrelated baseline failures, and do not
ignore current-source evidence that directly contradicts the reviewed claim.

Do not update or rebase merely because a conflict-free branch is behind. Resolve
actual conflicts, explicit stack changes, or combined-tree failures in this lane and
rerun only affected proof and review.

## Routes

- `INTEGRATION_READY` → `merge-reconcile`
- `REVIEW_REQUIRED` → `final-challenge`, then `review-pr`
- `DRAFT` → `publish-pr`
- `PENDING` → record the exact pending transition once and return `PR_IN_FLIGHT`
- `PRODUCT_OR_TEST_FAILURE` → `build-candidate`
- `REVIEW_FINDINGS_OPEN` / `CHANGES_REQUIRED` → `address-review-comments`
- `REVIEW_SCOPE_CHANGED` → review the affected dimensions
- `BLOCKED_BY_PREREQUISITE` → preserve the exact prerequisite and return to the
  invoking flow
- `SUPERSEDED_OR_CLOSE` → preserve the closeout disposition through the invoking flow
- `CONFLICT` / `INTEGRATION_INTERACTION` → repair the affected seam and rerun affected
  proof/review
- `INSTRUMENT_FAILURE` / `NOT_PROVEN` → name the missing reliable evidence
