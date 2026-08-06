---
name: deliver-pr
description: Carry one issue, PR, branch, candidate, or coherent acceptance-and-rollback claim from its current state through Codex-native review and reconciliation.
---

# Deliver PR

Carry one claim through one current candidate. Reconstruct only that lane's issue,
contract, proof, branch/worktree, PR, substantive review, live integration, explicit
prerequisites, and closeout state.

Mentioning one issue or PR does not make the Codex root a disposable bounded child. The
root remains accountable unless it explicitly delegates the whole flow.

Before creating a candidate, check whether an equivalent current PR already implements
the same claim. Do not inspect sibling lanes, touched-file overlap, nearby symbols, or
unrelated worktrees as a routine ownership check.

Existing coherent work enters midstream. One claim normally has one candidate, and one
writer mutates that branch/worktree at a time. Focused readers, reviewers, oracles, and
native subagents may assist without creating rival implementations.

An existing or publication-ready PR always enters `$finish-pr`. `$deliver-pr` does not
infer readiness from green CI, zero threads, mergeability, or author self-review;
`$finish-pr` must establish the provider-native `$review-pr` result before
`$verify-live-ci` can make an integration decision.

## Entry routing

```text
concern, issue, or plan unsettled
→ `$prepare-issue`

intent settled, proof absent or weak
→ `$prepare-proof`

reviewed proof or implementation candidate needs completion
→ `$build-candidate`

publication-ready candidate or existing PR needs convergence
→ `$finish-pr`

merged or deliberately closed but unreconciled
→ `$merge-reconcile` through `$finish-pr`

claim already reconciled
→ return `RECONCILED`
```

Create or link a missing issue where it improves continuity, but do not replay
completed stages performatively.

## Candidate and lane contract

One claim normally has one current candidate. Do not create rival branches for the
same implementation merely to manufacture parallelism.

One writer mutates this candidate branch/worktree at a time. Read-only research,
review, and oracle work may assist without becoming competing candidates.

This lane owns its integration work:

- behind-only movement on `main` requires no action;
- an actual Git conflict is resolved in this lane, normally by the later-landing lane;
- an explicit stacked prerequisite is retargeted after the prerequisite lands;
- a combined-tree semantic failure is repaired in the smallest affected candidate;
- only conflict- or interaction-affected proof and review are refreshed.

Use the controlling issue or PR discussion for material cross-lane facts. A direct
comment is normally enough. Do not create reservations, overlap ledgers, central lane
state, or routine sibling-PR surveillance.

## Remote-owned waits

When review, CI, queue state, auto-merge, or another external transition owns the next
action:

- leave the coherent candidate in GitHub;
- record the exact remaining action once;
- return `IN_FLIGHT` to the caller;
- if the caller is `$deliver-goal`, it may advance another distinct claim;
- do not call the claim blocked merely because it is in flight;
- do not poll unchanged state or refresh the branch for unrelated `main` movement.

## What this establishes

A bounded route for one coherent claim through its current issue, proof, candidate,
Codex-native substantive review, live integration, and closeout state.

## What this does not establish

A repository scheduler, tracked active-claim pointer, competing candidate set, overlap
ledger, or merge authorization independent of `$review-pr`, live required checks,
mergeability, rulesets, and unresolved findings.

## Completion

Return `RECONCILED`, `IN_FLIGHT`, `PARTIAL`, `SUPERSEDED`, `BLOCKED`, or
`NOT_PROVEN`, naming what landed or remains, what evidence is current, and which issue
or PR exposes the next action.