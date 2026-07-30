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

## Routes

- `PR_PUBLISHED_READY` / `PR_RESUMED` → `address-review-comments`
- `DRAFT_FOR_NAMED_REASON` / `DRAFT_REASON_COMPLETE` → `publish-pr` for the explicit readiness transition
- `FINDINGS_REPAIRED_OR_DISPOSITIONED` → `final-challenge`
- `CANDIDATE_FIXED_FOR_FORMAL_REVIEW` → `review-pr`
- `REVIEW_CURRENT` → `verify-live-ci`
- `REVIEW_FINDINGS_OPEN` → `address-review-comments`
- `REVIEW_NOT_PROVEN` → resolve candidate/claim identity, evidence, or receipt-instrument failure, then repeat `review-pr`; if evidence cannot be restored, return `NOT_PROVEN`
- `CLAIM_REVIEW_STALE` → `final-challenge`, then `review-pr`
- `PRODUCT_OR_TEST_FAILURE` → `build-candidate`, then repeat affected proof, final challenge, and formal review
- `PENDING_REMOTE` / `PR_IN_FLIGHT` → return control to `deliver-pr` or `deliver-goal`
- `INTEGRATION_READY` → `merge-reconcile`
- `RECONCILED` → return the bounded closeout to the invoking flow
- `PARTIAL` / `SUPERSEDED` → reconcile and return the residual graph
- `BLOCKED` / `NOT_PROVEN` → preserve the exact live blocker or missing evidence
