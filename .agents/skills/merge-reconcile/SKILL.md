---
name: merge-reconcile
description: Squash-merge one open PR through live GitHub protection with a current-head compare-and-swap, or reconcile an already merged/closed PR, without exact-head review receipt ceremony.
---

# Merge and reconcile

## State branch

Inspect the live PR state first.

- `MERGED` → skip merge and reconcile current `main`.
- `CLOSED_UNMERGED` with a durable close/supersede disposition → reconcile within that disposition.
- `CLOSED_UNMERGED` without a durable disposition → `NOT_PROVEN`.
- `OPEN` → follow the protected merge path below.
- unknown or partial state → `NOT_PROVEN`.

## Protected merge path

For an open PR, verify live GitHub facts:

- PR is ready, not draft;
- required checks are current for the current PR head;
- no unresolved review thread remains;
- no current `CHANGES_REQUESTED` review remains;
- deliberately requested reviewers are not still pending where their review is part of this claim;
- mergeability, conflicts, ruleset, queue, and applicable changelog/support state permit merge.

Do not require a claim digest, `review-run` receipt, current-head human review, or `REVIEW_PROTOCOL_ENFORCE=1` receipt convergence.

Use the current head SHA only as compare-and-swap protection at the instant of merge, for example:

```text
gh pr merge <n> --squash --match-head-commit <current-head-sha>
```

That prevents racing a moving branch. It does not make review validity depend on the SHA.

If the head moves before merge, re-read the live PR. Refresh only proof and review affected by the new commit. A formatting, editorial, generated-receipt, or test-strengthening commit does not trigger a full review by itself. A material semantic, claim, authority, risk, rollback, production-route, conflict, or integration change receives focused review of the affected dimensions.

Do not bypass policy.

## Reconciliation

After merge or evidence-backed deliberate closure:

1. verify the landed/current-main effect where applicable;
2. update or close the controlling issue accurately;
3. keep umbrellas open when only one slice landed;
4. update durable contracts, proof, support claims, and changelog only within the proven boundary;
5. preserve partial or residual work explicitly;
6. safely release branch/worktree residue;
7. expose the next coherent claim.

The squash commit is integration evidence. It does not require reconstructing a fictional exact-head review ceremony.

## Routes

- `RECONCILED` → return to `$deliver-pr` or `$deliver-goal`
- `PARTIAL` → preserve remaining acceptance
- `SUPERSEDED` / `DELIBERATELY_CLOSED` → preserve the durable disposition
- `CANDIDATE_MOVED` → re-read live state and refresh only affected proof/review
- `MERGE_BLOCKED` / `NOT_PROVEN` → preserve the exact blocker or missing evidence
