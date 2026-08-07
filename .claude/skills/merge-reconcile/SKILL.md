---
name: merge-reconcile
description: Enforce current substantive review plus current integration posture before expected-head squash merge, then reconcile the landed or deliberately closed claim.
user-invocable: false
---

# Merge and reconcile

This is Claude's irreversible integration backstop. Normal entry is:

```text
`finish-pr`
→ `review-pr`
→ `REVIEW_CURRENT`
→ `verify-live-ci`
→ `INTEGRATION_READY`
→ `merge-reconcile`
```

Direct invocation must reconstruct or receive both predecessor judgments. Never assume
they happened merely because the PR is green, mergeable, thread-clean, or previously
visited by another agent.

## State branch

Read live PR state first.

- merged → skip merge and reconcile current `main`;
- closed unmerged with a useful durable close/supersede disposition → reconcile it;
- closed unmerged without a durable disposition → `NOT_PROVEN`;
- open → establish predecessor judgments below;
- unknown/partial → `NOT_PROVEN`.

## Predecessor judgments

### Substantive review

Establish one useful semantically current result from the provider-native `review-pr`
flow and durable GitHub evidence:

```text
REVIEW_CURRENT
CHANGES_REQUIRED
NOT_PROVEN
BLOCKED_BY_PREREQUISITE
SUPERSEDED_OR_CLOSE
```

Read the candidate claim, submitted reviews, inline findings, finding dispositions,
affected proof, and material later changes. Thread resolution, bot approval, green
checks, mergeability, or author self-certification do not create `REVIEW_CURRENT`.

Do not require an exact-head review receipt. Review currentness follows material claim,
production-route, authority, proof, compatibility, risk, rollback, conflict, and
integration dimensions.

### Live integration

Only after review is `REVIEW_CURRENT`, establish the current `verify-live-ci` result:

```text
INTEGRATION_READY
PR_IN_FLIGHT
MERGE_BLOCKED
NOT_PROVEN
```

Use current required policy, current-head checks, unresolved substantive threads and
change requests, requested reviews that are part of the claim, draft purpose,
mergeability/conflicts, ruleset/queue state, explicit prerequisites, and applicable
changelog/support/release evidence.

## Decision table

The following routes are mandatory:

```text
review missing or not reconstructable
→ REVIEW_REQUIRED
→ `finish-pr` / `review-pr`

`CHANGES_REQUIRED`
→ `address-review-comments`
→ no merge

review `NOT_PROVEN`
→ preserve the missing or contradictory evidence
→ no merge

`BLOCKED_BY_PREREQUISITE`
→ preserve the exact prerequisite
→ no merge

`SUPERSEDED_OR_CLOSE`
→ durable closeout only
→ no merge

`REVIEW_CURRENT` + `PR_IN_FLIGHT`
→ return `PR_IN_FLIGHT`

`REVIEW_CURRENT` + `MERGE_BLOCKED`
→ preserve the concrete blocker

`REVIEW_CURRENT` + integration `NOT_PROVEN`
→ preserve the missing reliable integration evidence
→ no merge

`REVIEW_CURRENT` + `INTEGRATION_READY`
→ protected merge
```

No other combination reaches merge.

## Protected merge path

For `REVIEW_CURRENT` + `INTEGRATION_READY`, re-read the live PR immediately before the
irreversible transition and verify:

- PR remains ready, not draft;
- required checks and integration evidence still apply to the current candidate;
- no unresolved substantive finding or current `CHANGES_REQUESTED` review remains;
- mergeability, ruleset, queue, prerequisites, and release/changelog posture permit
  merge;
- the current head matches the expected candidate.

Use the current head SHA only as compare-and-swap protection, for example:

```text
gh pr merge <n> --squash --match-head-commit <current-head-sha>
```

If the head moved, re-read the candidate and refresh only proof/review/integration
materially affected by that commit. Do not bypass policy.

## Semantic currentness

- formatting, editorial cleanup, generated-receipt refresh, or stronger tests do not
  force broad review unless meaning changed;
- finding repair refreshes the finding, proof, and changed seam;
- material claim, production-route, authority, compatibility, security, persistence,
  packaging, migration, support, release, or rollback change refreshes affected
  review;
- actual conflict or combined-tree repair refreshes the affected interaction;
- unrelated base movement with a conflict-free candidate causes no review churn.

## Useful GitHub boundary

Read useful GitHub evidence; do not create tracked review/merge state. Write only:

- evidence-backed finding disposition that is still missing;
- useful blocker or prerequisite handoff another operator needs;
- final merge/closure effect, proof boundary, and residual claim.

Do not post stage completion, liveness, polling, exact-head ceremony, or duplicate
unchanged summaries.

## Reconciliation

After protected merge or evidence-backed deliberate closure:

1. verify the landed/current-main effect;
2. update/close the controlling issue accurately;
3. keep umbrellas open when only one slice landed;
4. update contracts, proof, support claims, and changelog only within the proven
   boundary;
5. preserve partial/residual work;
6. safely release branch/worktree residue;
7. expose the next coherent claim.

## Routes

- `REVIEW_REQUIRED` → `finish-pr` / `review-pr`
- `CHANGES_REQUIRED` → `address-review-comments`
- `PR_IN_FLIGHT` → return to `deliver-pr` / `deliver-goal`
- `MERGE_BLOCKED` / `NOT_PROVEN` → preserve exact blocker or missing evidence
- `CANDIDATE_MOVED` → re-read and refresh only affected dimensions
- `RECONCILED` / `PARTIAL` / `SUPERSEDED` / `DELIBERATELY_CLOSED` → return durable
  closeout

No central review database, exact-head receipt protocol, stage file, lifecycle label,
or reviewer identity is required.