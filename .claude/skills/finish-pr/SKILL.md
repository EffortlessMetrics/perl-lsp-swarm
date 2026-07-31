---
name: finish-pr
description: Carry one selected PR lane through publication, feedback repair, final challenge, formal review, live integration, squash merge, or reconciliation.
argument-hint: "[PR number, branch, or candidate]"
---

# Finish PR

Resolve the selected candidate's exact current head and material claim digest, then inspect its live GitHub state, review threads, formal-review receipts, checks, and mergeability. Do not inspect sibling implementations or treat nearby files/crates as lane ownership.

Enter at the earliest absent or stale useful judgment:

- no PR + ready candidate → `publish-pr`;
- draft whose named purpose remains active → complete that purpose;
- draft whose named purpose is complete → `publish-pr` readiness transition;
- findings or failed candidate proof → `address-review-comments`;
- candidate without current formal review → `final-challenge`, then `review-pr`;
- current review with no substantive findings → `verify-live-ci`;
- merged/closed but unreconciled → `merge-reconcile` closeout-only path;
- already reconciled → return `RECONCILED`.

Candidate or material-claim changes make formal-review evidence stale. Final challenge is a runtime-local pre-review pass, not a second durable stage. If a session resumes before current formal review exists, rerun the bounded challenge and continue directly to `review-pr`. Do not infer review from chat, task state, or teammate identity, and do not review or merge an already merged PR.

One writer mutates this candidate branch/worktree at a time. Focused readers, reviewers, oracles, and subagents may assist without creating rival implementations.

This lane owns its own integration cleanup. Behind-only movement requires no action. If Git reports a conflict, an explicit stack changed, or combined-tree proof exposes a real interaction, rebase or repair this candidate and rerun only affected proof/review. Use a direct issue or PR comment when another lane genuinely needs the result; do not create overlap ledgers or central lane state.

When GitHub owns the next transition—pending checks, requested review, merge queue, or armed auto-merge—record the exact next action once and return `PR_IN_FLIGHT` to `deliver-pr`/`deliver-goal`. Do not poll unchanged state or call the claim blocked merely because it is in flight.

After any candidate or material-claim change, rerun affected supporting evidence, perform the bounded final challenge, and obtain a fresh formal review before merge.

## Child outcome routing

### Publication

- `PR_PUBLISHED_READY` / `PR_RESUMED` → `address-review-comments`
- `DRAFT_FOR_NAMED_REASON` → complete the named remote proof or collaboration, then repeat `publish-pr`
- `DRAFT_REASON_COMPLETE` / `DRAFT` → `publish-pr` to recheck the full threshold and perform the explicit ready transition
- `CANDIDATE_NOT_COHERENT` / `LOCAL_PROOF_STALE` / `WORKTREE_DIRTY` → `build-candidate`
- `DUPLICATE_OR_WRITER_COLLISION` → reuse/resume the equivalent candidate or resolve the actual same-branch/worktree collision
- `IDENTITY_NOT_PROVEN` → establish branch/candidate identity; if reliable identity cannot be restored, return `NOT_PROVEN`

### Feedback and mutable challenge

- `FINDINGS_REPAIRED_OR_DISPOSITIONED` → `final-challenge`
- `MUTABLE_FINDINGS_OPEN` → `build-candidate`, then repeat affected proof and `final-challenge`
- `PROOF_WEAKENED` / `PROOF_REVISE` → `prepare-proof`, then repeat affected candidate passes
- `MATERIAL_PREMISE_CHANGED` → `prepare-issue`
- `SPLIT_CLAIM` → `prepare-issue` to narrow the current claim and preserve the independent residual claim
- `FOLLOW_UP_ACCEPTED` → create or link the bounded follow-up, then continue this PR within its current claim
- `DISPOSITION_INSTRUMENT_FAILURE` → leave the finding unresolved, repair the disposition instrument, and retry; otherwise return `NOT_PROVEN`

### Formal review

- `CANDIDATE_FIXED_FOR_FORMAL_REVIEW` → `review-pr`
- `REVIEW_CURRENT` → `verify-live-ci`
- `REVIEW_FINDINGS_OPEN` → `address-review-comments`
- `REVIEW_NOT_PROVEN` → resolve candidate/claim identity, evidence, or receipt-instrument failure, then repeat `review-pr`; if evidence cannot be restored, return `NOT_PROVEN`
- `CLAIM_REVIEW_STALE` → rerun affected proof, `final-challenge`, then `review-pr`

### Live integration

- `PRODUCT_OR_TEST_FAILURE` → `build-candidate`, then repeat affected proof, `final-challenge`, and `review-pr`
- `PENDING` / `PENDING_REMOTE` / `PR_IN_FLIGHT` → record the exact pending transition once and return control to `deliver-pr` or `deliver-goal`
- `CONFLICT` → resolve this lane's conflict, then `build-candidate` for affected repair/proof followed by `final-challenge` and `review-pr`
- `INTEGRATION_INTERACTION` → repair the smallest affected candidate through `build-candidate`, then rerun affected proof, `final-challenge`, and `review-pr`
- `INSTRUMENT_FAILURE` → identify and repair the failed evidence instrument; if trustworthy evidence cannot be restored, return `NOT_PROVEN`
- `INTEGRATION_READY` → `merge-reconcile`

### Merge and closeout

- `RECONCILED` → return the bounded closeout to the invoking flow
- `PARTIAL` → preserve remaining acceptance and return the residual graph
- `SUPERSEDED` / `DELIBERATELY_CLOSED` → preserve the durable disposition and return the residual graph
- `CANDIDATE_MOVED` / `CLAIM_REVIEW_STALE` → rerun affected proof, `final-challenge`, and `review-pr`
- `MERGE_BLOCKED` → preserve the exact live blocker; return `PR_IN_FLIGHT` when GitHub owns the pending transition, otherwise return `BLOCKED`
- `BLOCKED` / `NOT_PROVEN` → preserve the exact live blocker or missing evidence
