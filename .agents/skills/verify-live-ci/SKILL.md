---
name: verify-live-ci
description: Explicit atomic skill for evaluating one current PR candidate's live required checks, review/thread convergence, material-claim currentness, draft state, mergeability, and policy without branch-freshness churn or sibling-lane surveillance.
---

# Verify live CI

Resolve the exact current PR head and base. Reject `INTEGRATION_READY` while the PR remains draft.

## Canonical review and claim convergence

Run the repository-owned composite checker in enforced protocol mode rather than reconstructing review state or invoking its internal claim checker independently:

```text
REVIEW_PROTOCOL_ENFORCE=1 scripts/ci/check-pr-review-convergence <pr-number> [owner/repo]
```

The public checker owns complete review/thread/receipt/disposition convergence and proves that the completed formal-review receipt matches both the same current candidate head and the current visible normalized material PR claim/review index. Enforced mode makes running-review receipts, stale receipt heads, missing disposition markers, and required independent verification real blockers rather than advisory metadata.

A checker, API, pagination, digest, candidate-snapshot, or instrument failure is `NOT_PROVEN`, never an empty green result.

## Live machine and merge evidence

Read current GitHub state for this selected PR:

- required checks discovered from live repository policy;
- each applicable check's conclusion and evaluated candidate;
- draft/ready state and the draft's named purpose/completion condition;
- mergeability, ruleset, queue, and changelog/support state where applicable.

Do not inspect sibling PR implementations, touched-file overlap, or neighbouring worktrees. An interaction exists only when Git reports a conflict, an explicit prerequisite changed, or actual merge-group/synthetic integration proof failed.

Distinguish:

```text
success
failure
pending
not_applicable
cancelled
stale
missing
instrument_failure
not_proven
```

A successful older-candidate result is stale, not green. Missing or partial API data that can change the conclusion is `NOT_PROVEN`.

## Squash-merge currentness

Do not update, rebase, merge `main`, rerun formal review, or replay all checks merely because a conflict-free branch is behind.

- candidate or material claim change → rerun affected supporting evidence, final challenge, and fresh formal review;
- actual merge conflict → this lane resolves it, then reruns affected evidence/review;
- explicit stack or failed integration proof → targeted repair in the affected lane;
- unrelated `main` movement → no action.

## Routes

- `INTEGRATION_READY` → `$merge-reconcile`
- `DRAFT` → `$publish-pr` to evaluate the named draft purpose and perform the explicit ready transition when complete
- `PENDING` → record the exact pending transition once and return `PR_IN_FLIGHT` to the invoking flow
- `PRODUCT_OR_TEST_FAILURE` → repair through `$build-candidate`
- `REVIEW_FINDINGS_OPEN` → `$address-review-comments`
- `CLAIM_REVIEW_STALE` → `$final-challenge`, then `$review-pr`
- `CONFLICT` → resolve this lane's conflict and rerun affected proof/review
- `INTEGRATION_INTERACTION` → repair the smallest affected candidate and rerun affected proof/review
- `INSTRUMENT_FAILURE` / `NOT_PROVEN` → name the missing reliable evidence
