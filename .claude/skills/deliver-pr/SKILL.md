---
name: deliver-pr
description: Carry one issue, PR, branch, candidate, or coherent acceptance-and-rollback claim from its current state through reconciliation.
argument-hint: "[issue, PR, branch, or claim]"
---

# Deliver PR

Carry one claim through one current candidate. Reconstruct only that lane's issue, contract, proof, branch/worktree, PR, review, check, merge, explicit prerequisites, and closeout state.

Before creating a candidate, check whether an equivalent current PR already implements the same claim. Do not inspect sibling lanes, touched-file overlap, nearby symbols, or unrelated worktrees as a routine ownership check.

Route to the earliest absent or stale useful flow:

- unsettled concern/plan → `prepare-issue`;
- settled intent with weak proof → `prepare-proof`;
- reviewed proof or incomplete candidate → `build-candidate`;
- publication-ready candidate or existing PR → `finish-pr`;
- merged but unreconciled work → `merge-reconcile` through `finish-pr`;
- claim already reconciled → return `RECONCILED`.

Existing coherent work enters midstream. One claim normally has one candidate, and one writer mutates that branch/worktree at a time. Focused readers, reviewers, oracles, and subagents may assist without creating rival implementations.

This lane owns its own integration cleanup. Behind-only movement requires no action. When Git reports a conflict, an explicit stack changes, or integration proof exposes a real interaction, rebase or repair this candidate and rerun only affected proof/review. Use a direct issue or PR comment when another lane genuinely needs the result; do not create overlap ledgers or central lane state.

When CI, review, or auto-merge owns the next transition, leave the candidate in GitHub, record the exact next action once, and return `IN_FLIGHT` to the caller. If invoked from `deliver-goal`, the outer loop may then advance another distinct claim. Do not poll unchanged state or call the claim blocked merely because it is in flight.

## What this establishes

A bounded route for one coherent claim through its current issue, proof, candidate, pull request, integration, and closeout state, returning reconciled truth or an exact bounded result.

## What this does not establish

A repository scheduler, tracked active-claim pointer, competing candidate set, overlap ledger, or merge authorization independent of the live ruleset, required checks, and current review evidence.

## Completion

Return `RECONCILED`, `IN_FLIGHT`, `PARTIAL`, `SUPERSEDED`, `BLOCKED`, or `NOT_PROVEN`, with current evidence and the issue or PR that exposes remaining work.
