---
name: deliver-pr
description: Carry one coherent claim through its named Claude route in the accountable main thread, using one writer, bounded research/review programmes, and GitHub-native durable handoffs.
argument-hint: "[issue, PR, branch, or claim]"
---

# Deliver PR

This is the main-thread claim flow for one coherent acceptance-and-rollback claim.
Reconstruct only the selected claim frame: controlling issue, governing contract,
proof, branch/worktree, candidate, PR, substantive review, live integration, explicit
prerequisites, and closeout state.

The main Claude thread owns the claim meaning, current route, evidence joins, finding
dispositions, integration judgment, and continuation. It invokes `orchestrate-work`
where useful, follows each selected skill's normal and material backward routes, keeps
one candidate writer, and updates the root-held claim frame.

A substantial claim does **not** normally create another orchestrator. Bounded agents,
context forks, or workflows may execute research, build, proof, or review programmes;
the main thread remains accountable for orchestration and synthesis.

Before creating a candidate, check whether an equivalent current PR already implements
the same claim. Do not inspect sibling claim implementation details, touched-file
overlap, nearby symbols, or unrelated worktrees as a routine ownership check.

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

## Run the route from the main thread

For each current transition:

1. Invoke the named public flow or atomic skill.
2. Use `orchestrate-work` to decide what remains direct in the main thread and what
   becomes a bounded researcher, builder/writer, reviewer, subagent, context fork,
   Ultracode workflow, or Agent Team.
3. Require children to consume the named skill when one is supplied.
4. Join compact evidence and contradictions in the main thread; do not adopt a child
   verdict as approval.
5. Send accepted mutation through one writer.
6. Publish useful durable facts to the native GitHub surface.
7. Continue through the named next/backward route or update the claim frame with the
   typed result/wake event.

A child programme may span several ordered atomic skills when the same subject and
artifact context remain load-bearing. Do not respawn once per skill merely to imitate a
pipeline stage.

Use Agent Teams only when lateral communication changes the result. Use context forks
for inherited context when helpful, but do not treat inheritance as independence. Use
Ultracode when dynamic readiness materially improves one bounded execution. These are
physical execution choices, not subordinate orchestration authority.

Recursive orchestration is optional provider mechanics only. A whole-flow worker,
nested agent, dynamic workflow, or team remains a bounded execution context and does
not become the repository's logical claim owner or create a required sub-hierarchy.

## Candidate and claim contract

One claim normally has one current candidate. One writer mutates this branch/worktree at
a time. Focused readers, reviewers, external oracles, CI evidence agents, and native
subagents may assist without creating rival implementations.

The selected claim owns its integration work:

- behind-only movement on `main` requires no action;
- an actual Git conflict is repaired in the affected claim;
- an explicit stacked prerequisite is retargeted after the prerequisite lands;
- a combined-tree semantic failure is repaired in the smallest affected candidate;
- only conflict- or interaction-affected proof and review are refreshed.

Use direct issue or PR comments for material cross-claim facts. Do not create
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
unchanged polls, and routine skill transitions runtime-local. Do not write claim-frame
state to a tracked file.

## Remote-owned waits

When review, CI, queue state, auto-merge, a platform, or another external transition
owns the next action:

- leave the coherent candidate in GitHub;
- record the exact remaining action and wake event once when useful;
- mark this root-held claim frame `IN_FLIGHT`;
- let `deliver-goal` select another distinct claim;
- do not keep an idle agent alive merely to represent the wait;
- do not poll unchanged state;
- do not refresh the branch for unrelated `main` movement;
- do not call the claim blocked merely because it is in flight.

## What this establishes

One root-held claim follows a traceable provider-native route through issue, proof,
candidate, review, integration, merge, and closeout, with bounded programme delegation
and useful GitHub handoffs.

## What this does not establish

A subordinate claim orchestrator, repository scheduler, tracked active-claim/frontier
file, agent registry, competing candidate set, overlap ledger, comment-per-transition
protocol, or merge authorization independent of `review-pr`, live required checks,
mergeability, rulesets, and unresolved findings.

## Completion

Update the root-held claim frame with `RECONCILED`, `IN_FLIGHT`, `PARTIAL`,
`SUPERSEDED`, `BLOCKED`, or `NOT_PROVEN`, naming what landed or remains, which evidence
is current, the durable issue/PR subject, and the next material route or wake event.
Return to `deliver-goal` or the caller when the current claim has no immediately useful
root work.
