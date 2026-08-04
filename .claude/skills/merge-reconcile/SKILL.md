---
name: merge-reconcile
description: Squash-merge one open PR through live GitHub protection with a current-head compare-and-swap, or reconcile an already merged/closed PR, without exact-head review receipt ceremony.
user-invocable: false
---

# Merge and reconcile

Inspect live PR state first:

- merged → skip merge and reconcile current `main`;
- closed unmerged with a durable close/supersede disposition → reconcile within that disposition;
- closed unmerged without a durable disposition → `NOT_PROVEN`;
- open → follow the protected merge path;
- unknown or partial → `NOT_PROVEN`.

For an open PR, verify live GitHub facts: ready state, current required checks, unresolved review threads, current `CHANGES_REQUESTED` reviews, deliberately requested reviewers still pending where applicable, mergeability/conflicts, ruleset/queue state, and changelog/support disposition.

Do not require a claim digest, `review-run` receipt, current-head human review, or `REVIEW_PROTOCOL_ENFORCE=1` receipt convergence.

Use the current head SHA only as compare-and-swap protection at merge time, for example `gh pr merge <n> --squash --match-head-commit <current-head-sha>`. That prevents racing a moving branch; it does not make review validity depend on the SHA.

If the head moves, re-read live state and refresh only proof/review affected by the new commit. Formatting, editorial, generated-receipt, or test-strengthening commits do not trigger a full review by themselves. Material semantic, claim, authority, risk, rollback, production-route, conflict, or integration changes receive focused review of the affected dimensions.

After merge or evidence-backed closure, verify the landed effect, update controlling issues and durable claims, preserve residual work, clean branch/worktree residue, and expose the next claim.

## Routes

- `RECONCILED` → `deliver-pr` or `deliver-goal`
- `PARTIAL` → preserve remaining acceptance
- `SUPERSEDED` / `DELIBERATELY_CLOSED` → preserve the durable disposition
- `CANDIDATE_MOVED` → re-read live state and refresh only affected proof/review
- `MERGE_BLOCKED` / `NOT_PROVEN` → preserve the exact blocker or missing evidence
