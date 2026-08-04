---
name: finish-pr
description: Carry one selected pull request through publication, feedback repair, proportional review, live integration, squash merge, and reconciliation without exact-head review churn.
---

# Finish PR

## Purpose

Carry one coherent acceptance-and-rollback candidate from its current GitHub state through merge or durable closeout.

This flow owns only the selected PR lane. It does not inspect sibling implementations, reserve files or semantic surfaces, or coordinate other candidates.

## State inspection

Read the live PR, controlling issue, cumulative diff, submitted reviews, inline threads, required checks, draft state, mergeability, and explicit prerequisites.

Enter at the earliest useful point:

```text
no PR + publication-ready candidate
  → publish-pr

draft with a real remote-proof or collaboration purpose
  → complete that purpose

substantive findings or failed candidate proof
  → address-review-comments

candidate needing proportional review
  → final-challenge
  → review-pr

no open substantive findings
  → verify-live-ci

merged or deliberately closed but unreconciled
  → merge-reconcile

already reconciled
  → RECONCILED
```

Do not compute a claim digest, require a review receipt tied to the current head, or restart a full `deep` review merely because another commit was pushed.

## Review-forward repair

Review is cumulative and semantic:

- verify each repair against the finding, proof, and seam it changes;
- revisit broader claim, authority, risk, rollback, or production-path questions only when the repair materially changes them;
- formatting, editorial cleanup, generated receipt refresh, and stronger tests do not automatically invalidate prior review;
- conflict or integration repair receives focused review of the repaired seam.

A later SHA is new code evidence, not automatic proof that every prior judgment is stale.

## Candidate and integration boundary

One writer mutates this candidate branch/worktree at a time. Read-only review, CI classification, and specialist evidence may assist.

- behind-only movement on `main` requires no action;
- a real Git conflict is resolved in this lane;
- an explicit stack is retargeted after its prerequisite lands;
- a combined-tree interaction is repaired in the smallest affected candidate;
- only affected proof and review are refreshed.

## Remote-owned waits

When GitHub owns the next transition—pending checks, requested review, merge queue, or armed auto-merge—record the exact pending fact once and return `PR_IN_FLIGHT`. Do not poll unchanged state or call the wider goal blocked.

## Child outcome routing

### Publication

- `PR_PUBLISHED_READY` / `PR_RESUMED` → `$address-review-comments`
- `DRAFT_FOR_NAMED_REASON` → complete the named purpose, then repeat `$publish-pr`
- `DRAFT_REASON_COMPLETE` / `DRAFT` → `$publish-pr`
- `CANDIDATE_NOT_COHERENT` / `LOCAL_PROOF_STALE` / `WORKTREE_DIRTY` → `$build-candidate`
- `DUPLICATE_OR_WRITER_COLLISION` → reuse the equivalent candidate or resolve the actual same-branch/worktree collision
- `IDENTITY_NOT_PROVEN` → establish branch/candidate identity or return `NOT_PROVEN`

### Findings and challenge

- `FINDINGS_REPAIRED_OR_DISPOSITIONED` → rerun affected proof, then `$final-challenge`
- `MUTABLE_FINDINGS_OPEN` → `$build-candidate`
- `PROOF_WEAKENED` / `PROOF_REVISE` → `$prepare-proof`
- `MATERIAL_PREMISE_CHANGED` / `SPLIT_CLAIM` → `$prepare-issue`
- `FOLLOW_UP_ACCEPTED` → create or link the bounded follow-up, then continue this PR
- `DISPOSITION_INSTRUMENT_FAILURE` → preserve the unresolved finding and repair the instrument or return `NOT_PROVEN`

### Review

- `CANDIDATE_READY_FOR_REVIEW` → `$review-pr`
- `REVIEW_CURRENT` → `$verify-live-ci`
- `REVIEW_FINDINGS_OPEN` → `$address-review-comments`
- `REVIEW_SCOPE_CHANGED` → review the affected dimensions; route backward only if the claim or owner changed
- `REVIEW_NOT_PROVEN` → resolve the missing evidence or authority

### Live integration

- `PRODUCT_OR_TEST_FAILURE` → `$build-candidate`, then repeat affected proof and review
- `PENDING` / `PENDING_REMOTE` / `PR_IN_FLIGHT` → return control to `$deliver-pr` or `$deliver-goal`
- `CONFLICT` / `INTEGRATION_INTERACTION` → repair the affected seam, then rerun affected proof and review
- `INSTRUMENT_FAILURE` / `NOT_PROVEN` → name the missing reliable evidence
- `INTEGRATION_READY` → `$merge-reconcile`

### Merge and closeout

- `RECONCILED` → return the closeout to the invoking flow
- `PARTIAL` → preserve remaining acceptance and return the residual graph
- `SUPERSEDED` / `DELIBERATELY_CLOSED` → preserve the durable disposition
- `CANDIDATE_MOVED` → re-read the live PR; refresh only evidence/review affected by the new commit
- `MERGE_BLOCKED` → return `PR_IN_FLIGHT` for GitHub-owned waits, otherwise preserve the real blocker
- `BLOCKED` / `NOT_PROVEN` → preserve the exact blocker or missing evidence
