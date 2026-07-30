---
name: finish-pr
description: Use when one publication-ready candidate or existing pull request needs state-aware GitHub publication, feedback repair, final challenge, formal review, live integration proof, squash merge, or reconciliation.
---

# Finish PR

## Purpose

Carry one coherent acceptance-and-rollback candidate from its actual current GitHub state through convergence and current-main reconciliation.

This flow owns only the selected PR lane. It does not inspect sibling implementations, reserve overlapping files or semantic surfaces, or coordinate other candidates.

## State inspection and entry

Before invoking a child skill:

1. resolve the exact PR head and normalized material claim/review-index digest;
2. inspect live PR state, review threads, formal-review receipts, checks, and mergeability;
3. enter at the earliest absent or stale useful judgment.

```text
no PR + publication-ready candidate
  → publish-pr

draft PR whose named remote/collaboration purpose is still active
  → remain draft and complete that purpose

draft PR whose named purpose is complete
  → publish-pr readiness transition

ready PR with substantive findings or failed candidate proof
  → address-review-comments

candidate without a current formal review
  → final-challenge
  → review-pr

current formal review + no unresolved substantive findings
  → verify-live-ci

merged or deliberately closed but unreconciled
  → merge-reconcile closeout-only path

already reconciled
  → return RECONCILED
```

A candidate or material-claim change makes formal-review evidence stale. Final challenge is a runtime-local pre-review pass, not a second durable stage. If a session resumes before current formal review exists, rerun the bounded challenge and continue directly to `review-pr`. Do not infer formal review from conversation, task state, or agent identity. Do not review or merge an already merged/closed PR.

## Candidate boundary

One writer applies accepted repairs to this candidate branch/worktree at a time. Read-only finding verification, CI classification, and differentiated review may assist without becoming rival candidates.

This lane owns its actual integration work:

- behind-only movement on `main` requires no action;
- a Git conflict is resolved in this lane's worktree;
- an explicit stacked prerequisite is rebased or retargeted after it lands;
- a combined-tree interaction is repaired in the smallest affected candidate;
- only affected proof/review is refreshed.

Use a direct issue or PR comment when a prerequisite, ruling, supersession, or real integration finding materially affects another lane. No overlap map, reservation system, or sibling-lane monitoring is part of this flow.

## Remote-owned waits

When the candidate is coherent and GitHub owns the next transition—pending checks, requested review, merge queue, or armed auto-merge—do not poll unchanged state or keep the root trapped in this flow.

Record the exact remaining action once and return `PR_IN_FLIGHT` to the invoking `deliver-pr`/`deliver-goal` flow so another distinct claim may proceed.

## Procedure

Follow the state-selected child skill and its local routes. After any candidate or material-claim change, rerun affected supporting proof/review, perform the bounded final challenge, and obtain a fresh formal-review record before merge.

## What this establishes

A merged or deliberately closed claim with durable GitHub evidence and reconciled remaining work, or one coherent candidate explicitly left in flight under GitHub authority.

## What this does not establish

A broader umbrella goal is complete unless current-main reconciliation proves it.

## Valid exits

- `PR_PUBLISHED_READY` / `PR_RESUMED` → `$address-review-comments`
- `DRAFT_FOR_NAMED_REASON` / `DRAFT_REASON_COMPLETE` → `$publish-pr` for the explicit readiness transition
- `FINDINGS_REPAIRED_OR_DISPOSITIONED` → `$final-challenge`
- `CANDIDATE_FIXED_FOR_FORMAL_REVIEW` → `$review-pr`
- `REVIEW_CURRENT` → `$verify-live-ci`
- `REVIEW_FINDINGS_OPEN` → `$address-review-comments`
- `REVIEW_NOT_PROVEN` → resolve the named candidate/claim identity, evidence, or receipt-instrument failure, then repeat `$review-pr`; if reliable evidence cannot be restored, return `NOT_PROVEN`
- `CLAIM_REVIEW_STALE` → `$final-challenge`, then `$review-pr`
- `PRODUCT_OR_TEST_FAILURE` → `$build-candidate`, then repeat affected proof, final challenge, and formal review
- `PENDING_REMOTE` / `PR_IN_FLIGHT` → return control to `$deliver-pr` or `$deliver-goal`
- `INTEGRATION_READY` → `$merge-reconcile`
- `RECONCILED` → return the bounded closeout to the invoking flow
- `PARTIAL` / `SUPERSEDED` → reconcile honestly and return the residual graph to the invoking flow
- `BLOCKED` / `NOT_PROVEN` → preserve the exact live blocker or missing evidence
