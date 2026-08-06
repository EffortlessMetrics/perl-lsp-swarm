---
name: finish-pr
description: Carry one selected pull request through publication, feedback repair, substantive acceptance review, live integration, squash merge, and reconciliation without exact-head review churn.
argument-hint: "[PR number, branch, or candidate]"
---

# Finish PR

Read the selected PR, controlling issue, cumulative diff, submitted reviews, inline
threads, current substantive merge posture, required checks, draft state,
mergeability, and explicit prerequisites. Do not inspect sibling implementations or
treat nearby files/crates as lane ownership.

Apply [`docs/agents/PR_REVIEW_STANDARD.md`](../../../docs/agents/PR_REVIEW_STANDARD.md)
for substantive review and
[`docs/agents/REVIEW_CURRENTNESS.md`](../../../docs/agents/REVIEW_CURRENTNESS.md)
for later repairs.

Enter at the earliest useful point:

- no PR + ready candidate → `publish-pr`;
- draft with a real remote-proof or collaboration purpose → complete that purpose;
- substantive findings or failed candidate proof → `address-review-comments`;
- no useful current substantive review → `final-challenge`, then `review-pr`;
- current `READY_FOR_INTEGRATION` review with no open substantive findings →
  `verify-live-ci`;
- merged/closed but unreconciled → `merge-reconcile`;
- already reconciled → `RECONCILED`.

A clean bot summary, green CI, zero open threads, textual mergeability, task
completion, or author/self-review statement does not establish a useful substantive
review. `verify-live-ci` evaluates integration facts; it cannot substitute for
`review-pr`.

Do not compute a claim digest, require a review receipt tied to the current head, or
restart a full `deep` review merely because another commit was pushed.

## Review-forward repair

Review is cumulative and semantic. Verify repairs against the finding, proof, and seam
they change. Revisit broader claim, authority, production reachability, risk,
rollback, compatibility, or proof questions only when the repair materially changes
them. Formatting, editorial cleanup, generated receipt refresh, and stronger tests do
not automatically invalidate prior review. Conflict or integration repair receives
focused review of the repaired seam.

After accepted repair:

```text
rerun affected proof
→ final-challenge on changed semantic subjects
→ review-pr on affected findings/dimensions
→ update the cumulative merge posture
→ verify-live-ci only when posture is READY_FOR_INTEGRATION
```

One writer mutates this candidate branch/worktree at a time. Behind-only movement
requires no action. Resolve actual conflicts, explicit stack changes, or combined-tree
interactions in this lane and rerun only affected proof and review.

When GitHub owns the next transition—pending checks, requested review, merge queue, or
armed auto-merge—record the pending fact once and return `PR_IN_FLIGHT`. Do not poll
unchanged state or call the wider goal blocked.

## Child outcome routing

### Publication

- `PR_PUBLISHED_READY` / `PR_RESUMED` → `address-review-comments`
- `DRAFT_FOR_NAMED_REASON` → complete the named purpose, then repeat `publish-pr`
- `DRAFT_REASON_COMPLETE` / `DRAFT` → `publish-pr`
- `CANDIDATE_NOT_COHERENT` / `LOCAL_PROOF_STALE` / `WORKTREE_DIRTY` →
  `build-candidate`
- `DUPLICATE_OR_WRITER_COLLISION` → reuse the equivalent candidate or resolve the
  actual same-branch/worktree collision
- `IDENTITY_NOT_PROVEN` → establish branch/candidate identity or return `NOT_PROVEN`

### Findings and challenge

- `FINDINGS_REPAIRED_OR_DISPOSITIONED` → rerun affected proof, then
  `final-challenge`
- `MUTABLE_FINDINGS_OPEN` → `build-candidate`
- `PROOF_WEAKENED` / `PROOF_REVISE` → `prepare-proof`
- `MATERIAL_PREMISE_CHANGED` / `SPLIT_CLAIM` → `prepare-issue`
- `FOLLOW_UP_ACCEPTED` → create or link the bounded follow-up, then continue
- `DISPOSITION_INSTRUMENT_FAILURE` → preserve the unresolved finding and repair the
  instrument or return `NOT_PROVEN`

### Review

- `CANDIDATE_READY_FOR_REVIEW` / `REVIEW_REQUIRED` → `review-pr`
- `READY_FOR_INTEGRATION` / `REVIEW_CURRENT` → `verify-live-ci`
- `CHANGES_REQUIRED` / `REVIEW_FINDINGS_OPEN` → `address-review-comments`
- `REVIEW_SCOPE_CHANGED` → review the affected dimensions; route backward only if
  the claim or owner changed
- `IN_FLIGHT` → return `PR_IN_FLIGHT` with the named GitHub-owned transition
- `BLOCKED_BY_PREREQUISITE` → preserve the exact prerequisite and return to the
  invoking flow
- `SUPERSEDED_OR_CLOSE` → `merge-reconcile` for durable closeout
- `NOT_PROVEN` / `REVIEW_NOT_PROVEN` → resolve missing evidence or authority

### Live integration

- `REVIEW_REQUIRED` → `final-challenge`, then `review-pr`
- `PRODUCT_OR_TEST_FAILURE` → `build-candidate`, then repeat affected proof and review
- `PENDING` / `PENDING_REMOTE` / `PR_IN_FLIGHT` → return control to `deliver-pr` or
  `deliver-goal`
- `CONFLICT` / `INTEGRATION_INTERACTION` → repair the affected seam, then rerun
  affected proof and review
- `INSTRUMENT_FAILURE` / `NOT_PROVEN` → name the missing reliable evidence
- `INTEGRATION_READY` → `merge-reconcile`

### Merge and closeout

- `RECONCILED` → return the closeout
- `PARTIAL` → preserve remaining acceptance
- `SUPERSEDED` / `DELIBERATELY_CLOSED` → preserve the durable disposition
- `CANDIDATE_MOVED` → re-read the live PR; refresh only evidence/review affected by
  the new commit
- `MERGE_BLOCKED` → return `PR_IN_FLIGHT` for GitHub-owned waits, otherwise preserve
  the real blocker
- `BLOCKED` / `NOT_PROVEN` → preserve the exact blocker or missing evidence
