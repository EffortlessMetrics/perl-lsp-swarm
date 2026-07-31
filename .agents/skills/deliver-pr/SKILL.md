---
name: deliver-pr
description: Use for one issue, pull request, branch, implementation candidate, or coherent acceptance-and-rollback claim that should be carried from its current state through reconciliation.
---

# Deliver PR

## Purpose

Carry one coherent claim through one current candidate. Reconstruct only that claim's relevant GitHub and repository state, enter at the earliest absent or stale useful judgment, and continue through reconciliation.

Mentioning one issue or PR does not make the root a disposable bounded child. The root remains accountable for the selected claim unless it explicitly delegates the whole flow.

## Focused reconstruction

Read:

- the controlling issue and current synthesis/plan;
- the governing specification, ADR, policy, or invariant where applicable;
- current proof and known limitations;
- this lane's branch/worktree and current writer;
- the existing PR, candidate, reviews, threads, checks, mergeability, and closeout state;
- explicit prerequisites and current-main behavior material to this claim.

Before creating a candidate, check whether an equivalent current PR already implements the same claim. Do not inspect sibling lanes, file overlap, nearby symbols, or unrelated worktrees as a routine ownership check.

## Entry routing

```text
concern, issue, or plan unsettled
  → $prepare-issue

intent settled, proof absent or weak
  → $prepare-proof

reviewed proof or implementation candidate needs completion
  → $build-candidate

publication-ready candidate or existing PR needs convergence
  → $finish-pr

merged or deliberately closed but unreconciled
  → $merge-reconcile through $finish-pr

claim already reconciled
  → return `RECONCILED`
```

Existing coherent work enters midstream. Create or link a missing issue where it improves continuity, but do not replay completed stages performatively.

## Candidate and lane contract

One claim normally has one current candidate. Do not create rival branches for the same implementation merely to manufacture parallelism.

One writer mutates this candidate branch/worktree at a time. Read-only research, review, and oracle work may assist without becoming competing candidates.

This lane owns its own integration work:

- behind-only movement on `main` requires no action;
- an actual Git conflict is resolved in this lane's worktree, normally by the later-landing lane;
- an explicit stacked prerequisite is rebased or retargeted after the prerequisite lands;
- a combined-tree semantic failure is repaired in the smallest affected candidate;
- only conflict- or interaction-affected proof and review are refreshed.

Use the controlling issue or PR discussion for material cross-lane facts. A direct comment is normally enough. Do not create reservations, overlap ledgers, central lane state, or routine sibling-PR surveillance.

## Remote-owned waits

When CI, review, auto-merge, or another external transition owns the next action:

- leave the coherent candidate in GitHub;
- record the exact remaining action once;
- return `IN_FLIGHT` to the caller;
- if the caller is `$deliver-goal`, it may advance another distinct claim;
- do not call the claim blocked merely because it is in flight;
- do not poll unchanged state or refresh the branch for unrelated `main` movement.

## Completion

Return one bounded result:

- `RECONCILED`
- `IN_FLIGHT`
- `PARTIAL`
- `SUPERSEDED`
- `BLOCKED`
- `NOT_PROVEN`

Name what landed or remains in flight, what evidence is current, and which issue or PR exposes the next action.
