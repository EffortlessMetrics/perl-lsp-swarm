---
name: deliver-pr
description: Carry one coherent claim through a traceable Claude lane route, claim-local orchestration, one writer, provider-native review, integration, and reconciliation.
argument-hint: "[issue, PR, branch, or claim]"
---

# Deliver PR

This is the claim-lane flow. Carry one acceptance-and-rollback claim through one
current candidate. Reconstruct only the selected issue/contract, semantic owner and
consumers, proof, branch/worktree, PR, findings, substantive review, integration facts,
explicit prerequisites, and closeout state.

The main Claude thread remains accountable unless it explicitly delegates the whole
flow. A whole-flow delegate becomes the lane root for this claim and may invoke
`orchestrate-work` within the claim.

## Trace and run the route

At entry, state the shortest current route and then execute it:

```text
`deliver-pr`(#123)
→ `prepare-issue` | `prepare-proof` | `build-candidate` | `finish-pr`
→ material backward routes as evidence requires
→ lane result
```

When delegating the whole lane:

```text
parent: `deliver-goal`
route: `deliver-pr`(#123) → `orchestrate-work` → current named skill
scope: this claim only
writer: one admitted candidate writer
return: RECONCILED | IN_FLIGHT | PARTIAL | SUPERSEDED | BLOCKED | NOT_PROVEN
```

Do not replace the selected skill route with a hand-written lifecycle recipe.

## Entry routing

```text
concern, owner, scope, or plan unsettled
→ `prepare-issue`

intent settled; proof absent or weak
→ `prepare-proof`

reviewed proof or implementation candidate needs completion
→ `build-candidate`

publication-ready candidate or existing PR needs convergence
→ `finish-pr`

merged or deliberately closed but unreconciled
→ `finish-pr` → `merge-reconcile`

claim already reconciled
→ `RECONCILED`
```

Existing coherent work enters midstream. Do not replay completed judgments for
ceremony.

## Lane orchestration

The lane root uses `orchestrate-work` to keep disposable work outside its claim-control
context:

- source/authority and production-consumer mapping;
- external oracle research;
- proof design and realistic wrong-implementation challenge;
- candidate implementation and focused repair;
- test hardening and simplification;
- CI/log classification;
- differentiated substantive review.

One writer mutates the candidate. Read-only subagents and reviewers may run
concurrently. A child may recursively orchestrate only when the brief explicitly
grants claim-local orchestration authority.

Use Ultracode when a dynamic task graph emerges inside this claim. Use an Agent Team
only when lateral communication among claim-local agents changes the result.

The lane root retains claim meaning, accepted authority, contradictions, evidence
joins, candidate sufficiency, review sufficiency, integration judgment, and closeout.

## Candidate and concurrency contract

Before creating a candidate, check whether an equivalent current PR already implements
the same claim and whether explicit prerequisites exist. Do not routinely inspect
sibling worktrees, touched-file overlap, or nearby symbols as ownership.

Different claims may overlap files/crates. This lane coordinates only a duplicate
claim, same-candidate writer collision, explicit stack, destructive shared runtime
state, actual conflict, or demonstrated combined-tree interaction.

- behind-only `main` movement → no action;
- actual Git conflict → lane-local repair and affected proof/review;
- explicit prerequisite lands/changes → retarget affected lane;
- combined-tree failure → repair smallest affected candidate;
- unrelated main movement → no rebase/review churn.

## Useful GitHub boundaries

Update the controlling issue or PR only when the result is reusable:

- corrected premise, scope, owner, plan, or proof obligation;
- prerequisite/supersession/actual interaction;
- candidate claim, proof, limitation, or deviation;
- inline review finding or cumulative submitted review;
- evidence-backed disposition;
- named remote-owned wait needed for handoff;
- merge/closure effect and residual work.

Do not post route progress, skill completion, agent liveness, polling, raw logs, or
unchanged repeated summaries.

## Remote-owned waits

When review, checks, queue state, auto-merge, or another external transition owns the
next action:

1. leave the coherent candidate in GitHub;
2. record the exact pending fact only when another operator needs the handoff;
3. identify the wake event;
4. return `IN_FLIGHT`;
5. do not poll unchanged state.

The campaign root may advance another distinct claim.

## Return packet

Return compact graph deltas:

```text
claim and candidate identity
current route/result
behavior/evidence established
proof and review currentness
contradictions or limitations
GitHub durable updates made
external wait and wake event
remaining acceptance or next named route
```

## Completion

Return `RECONCILED`, `IN_FLIGHT`, `PARTIAL`, `SUPERSEDED`, `BLOCKED`, or
`NOT_PROVEN`.

This skill does not create a scheduler, tracked lane state, overlap ledger, executor
DAG, or merge authority independent of `review-pr`, `verify-live-ci`, branch
protection, rulesets, and expected-head merge safety.