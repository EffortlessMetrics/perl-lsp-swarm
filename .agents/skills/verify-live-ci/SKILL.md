---
name: verify-live-ci
description: Evaluate one substantively reviewed PR's live checks, threads, draft state, mergeability, and policy without treating CI as review or creating exact-head churn.
---

# Verify live CI

This is Codex's live-integration fact skill. It does not perform, infer, or replace the
substantive review owned by `$review-pr`.

Read one current GitHub snapshot for the selected PR:

- cumulative substantive review result;
- draft/ready state and any still-valid draft purpose;
- required checks discovered from live policy and relevant advisory checks;
- unresolved review threads and current `CHANGES_REQUESTED` reviews;
- deliberately requested reviewers still pending;
- mergeability, conflicts, queue/ruleset state, and explicit prerequisites;
- applicable changelog, support, release, or publication disposition.

Use repository helpers where they report these facts truthfully. Do not require a
review-run comment, claim digest, review submitted on the latest SHA solely because the
SHA changed, or review-receipt convergence.

## Review sufficiency boundary

```text
no useful current substantive review
→ REVIEW_REQUIRED
→ `$review-pr`

CHANGES_REQUIRED
→ `$address-review-comments`

NOT_PROVEN
→ preserve missing review evidence or authority

BLOCKED_BY_PREREQUISITE
→ preserve the exact prerequisite

SUPERSEDED_OR_CLOSE
→ preserve durable closeout

REVIEW_CURRENT
→ evaluate live integration facts
```

Green checks, textual mergeability, zero open threads, bot approval, or author
self-certification cannot promote a candidate to `REVIEW_CURRENT`.

## Integration postures

Use one separate integration result:

```text
INTEGRATION_READY
PR_IN_FLIGHT
MERGE_BLOCKED
NOT_PROVEN
```

- `INTEGRATION_READY` means current GitHub protection and integration facts permit the
  irreversible transition.
- `PR_IN_FLIGHT` means GitHub owns a named pending transition such as required checks,
  requested review, queue state, or armed auto-merge.
- `MERGE_BLOCKED` means a concrete conflict, failed required check, unresolved
  substantive thread/change request, ruleset failure, or explicit prerequisite blocks
  merge.
- `NOT_PROVEN` means API data, check identity, required-policy discovery, or another
  integration instrument is missing or unreliable.

A pending check therefore leaves the substantive review current while integration is
`PR_IN_FLIGHT`.

## Live evidence classification

Preserve success, failure, pending, not-applicable, cancelled, stale-check-result,
missing, instrument-failure, and not-proven states distinctly. A successful check on
an older candidate is stale evidence, not current green.

Classify failures as candidate-owned, base-owned, integration interaction,
test/oracle defect, instrument failure, environment/capacity, pending, or
`NOT_PROVEN`. Do not widen the PR to absorb unrelated baseline failures, and do not
ignore current-source evidence that directly contradicts the reviewed claim.

## Semantic currentness

A review is not stale merely because the PR head changed:

- finding repair → check the affected finding, proof, and seam;
- material claim, production route, authority, proof, compatibility, risk, or rollback
  change → return to `$review-pr` for affected dimensions;
- formatting, editorial cleanup, generated receipt refresh, or stronger tests → no
  automatic full-review restart;
- conflict or combined-tree repair → focused proof and review of the affected seam.

Do not update, rebase, merge `main`, or replay all proof merely because a conflict-free
branch is behind.

## Routes

- `REVIEW_REQUIRED` → `$review-pr`
- `REVIEW_FINDINGS_OPEN` / `CHANGES_REQUIRED` → `$address-review-comments`
- `REVIEW_SCOPE_CHANGED` → `$review-pr` for affected dimensions
- `DRAFT` → `$publish-pr`
- `PENDING` / `PR_IN_FLIGHT` → return the exact pending transition to `$finish-pr` or
  `$deliver-goal`
- `PRODUCT_OR_TEST_FAILURE` → `$build-candidate`, then affected proof and review
- `CONFLICT` / `INTEGRATION_INTERACTION` → repair the affected seam, then affected
  proof and `$review-pr`
- `BLOCKED_BY_PREREQUISITE` / `MERGE_BLOCKED` → preserve the exact blocker
- `INSTRUMENT_FAILURE` / `NOT_PROVEN` → name the missing reliable evidence
- `INTEGRATION_READY` → `$merge-reconcile`