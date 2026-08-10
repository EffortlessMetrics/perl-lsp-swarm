---
name: deliver-pr
description: Run one coherent claim through its named Claude route using a claim-local lane orchestrator, one writer, useful subagents, and GitHub-native durable handoffs.
argument-hint: "[issue, PR, branch, or claim]"
---

# Deliver PR

This is a lane-root flow for one coherent acceptance-and-rollback claim. Reconstruct
only that lane's issue, governing contract, proof, branch/worktree, candidate, PR,
substantive review, live integration, explicit prerequisites, and closeout state.

The lane root runs the route; it does not merely choose a starting point and then invent
an ad hoc lifecycle. Invoke `orchestrate-work` where useful, follow each selected
skill's normal and material backward routes, keep one candidate writer, and return a
typed lane result to the campaign root.

Mentioning one issue or PR does not make the Claude campaign root a leaf worker. A
campaign root normally delegates a substantial claim as a whole-flow `deliver-pr`
lane. A lane root may perform tiny claim-local work directly when briefing and joining
would cost more than the context it preserves.

Before creating a candidate, check whether an equivalent current PR already implements
the same claim. Do not inspect sibling lanes, touched-file overlap, nearby symbols, or
unrelated worktrees as a routine ownership check.

## Entry route

Enter at the earliest absent or stale useful judgment:

```text
concern, issue, owner, scope, or plan unsettled
→ `prepare-issue`

intent settled, proof absent or weak
→ `prepare-proof`

reviewed proof or implementation candidate needs completion
→ `build-candidate`

publication-ready candidate or existing PR needs convergence
→ `finish-pr`

merged or deliberately closed but unreconciled
→ `merge-reconcile` through `finish-pr`

claim already reconciled
→ return `RECONCILED`
```

Create or link a missing issue where it improves continuity, but do not replay completed
stages performatively.

## Run the route through claim-local orchestration

For each current transition:

1. Invoke the named public flow or atomic skill.
2. Use `orchestrate-work` to choose focused subagents, one writer, context forks,
   review contexts, an Ultracode workflow, or an Agent Team when useful.
3. Require children to consume the named skill when one is supplied.
4. Join compact evidence and contradictions; do not adopt a child verdict as approval.
5. Send accepted mutations through one writer.
6. Publish useful durable facts to the native GitHub surface.
7. Continue through the named next/backward route or return the typed result.

A whole-flow lane may recursively orchestrate within this claim. Leaf task agents may
not select unrelated work or widen into lane ownership unless their brief grants that
authority. Use Agent Teams only when lateral communication changes the result; use
ordinary subagents when independent returns to the lane root are sufficient.

## Candidate and lane contract

One claim normally has one current candidate. One writer mutates this branch/worktree at
a time. Focused readers, reviewers, external oracles, CI evidence agents, and native
subagents may assist without creating rival implementations.

This lane owns its integration work:

- behind-only movement on `main` requires no action;
- an actual Git conflict is resolved in this lane, normally by the later-landing lane;
- an explicit stacked prerequisite is retargeted after the prerequisite lands;
- a combined-tree semantic failure is repaired in the smallest affected candidate;
- only conflict- or interaction-affected proof and review are refreshed.

Use direct issue or PR comments for material cross-lane facts. Do not create
reservations, overlap ledgers, central lane state, or routine sibling-PR surveillance.

## Traceable intended route

When another context will need the route and it is not already obvious, publish one
compact declaration on the controlling issue or PR:

```text
Route
- Goal / parent: <umbrella or durable outcome>
- Claim: <one acceptance-and-rollback claim>
- Entry flow: `deliver-pr`
- Current useful transition: <named skill or external wait>
- Why: <material missing judgment>
- Durable subject: <issue / PR / merged commit>
- Resume when: <material wake event, if any>
```

Update only when the material route changes. This is a resumability aid, not a stage
record, lease, or per-step status protocol.

## Useful GitHub boundaries

Publish a durable issue/PR comment, inline review, submitted review, or finding
disposition when the information:

- changes claim, authority, accepted plan, proof obligation, route, prerequisite,
  support, risk, or rollback meaning;
- is source-backed evidence another context would otherwise rediscover;
- is a localized review finding or evidence-backed disposition;
- records a real external wait and its wake event;
- provides a useful candidate-wide review, integration, merge, or closeout synthesis.

Keep agent identity, topology, liveness, retry order, provisional reasoning, raw logs,
unchanged polls, and routine skill transitions runtime-local. Do not write lane state to
a tracked file.

## Remote-owned waits

When review, CI, queue state, auto-merge, a platform, or another external transition
owns the next action:

- leave the coherent candidate in GitHub;
- record the exact remaining action and wake event once when useful;
- return `IN_FLIGHT` to the caller;
- let `deliver-goal` advance another distinct claim;
- do not poll unchanged state;
- do not refresh the branch for unrelated `main` movement;
- do not call the claim blocked merely because it is in flight.

## What this establishes

One claim follows a traceable provider-native route through issue, proof, candidate,
review, integration, merge, and closeout, with claim-local orchestration and useful
GitHub handoffs.

## What this does not establish

A repository scheduler, tracked active-claim/frontier file, agent registry, competing
candidate set, overlap ledger, comment-per-transition protocol, or merge authorization
independent of `review-pr`, live required checks, mergeability, rulesets, and
unresolved findings.

## Completion

Return `RECONCILED`, `IN_FLIGHT`, `PARTIAL`, `SUPERSEDED`, `BLOCKED`, or
`NOT_PROVEN`, naming what landed or remains, which evidence is current, the durable
issue/PR subject, and the next material route or wake event.
