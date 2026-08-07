---
name: deliver-goal
description: Operate a durable multi-PR outcome through a runtime-local frontier of claim lanes, provider-native routes, useful GitHub handoffs, and final goal reconciliation.
---

# Deliver goal

This is the Codex campaign-root flow. Preserve the verbatim goal source, current
interpretation, constraints, non-goals, acceptance predicates, governing contracts,
current main/evidence, directly required claims, explicit dependencies, merged effects,
known limitations, and `NOT_PROVEN` predicates.

Do not turn the campaign root into a general implementation worker. Its job is to
select, brief, steer, join, merge-judge, and reconcile claim lanes while protecting the
context needed for the whole outcome.

## Reconstruct the bounded goal graph

Read only the graph projection needed by this goal:

- umbrella/goal issue and current synthesis;
- directly required unresolved claims and their issues/PRs;
- explicit prerequisites and owner decisions;
- recently merged effects that change acceptance;
- current submitted reviews, integration posture, and remote waits;
- shared blocker or contradiction that affects several claims.

Do not scan/score the entire backlog, inspect sibling worktrees, or infer ownership
from touched files.

For a durable campaign, maintain one useful current synthesis on the umbrella issue.
For a session-sized goal, runtime context is sufficient. Do not create tracked
frontier, active-goal, queue, stage, or lane-state files.

## Runtime-local frontier

Maintain a compact in-context frontier:

| Claim | Goal predicate | Lane root/context | Durable subject | Current judgment | Next material action | External wait | Wake event |
| --- | --- | --- | --- | --- | --- | --- | --- |

Reconstruct it after compaction/replacement from current GitHub and repository facts.
Do not post frontier rows or agent liveness to GitHub.

A wake event is the next fact that can change a lane decision. Examples: a worker or
reviewer returns; a material finding appears; a required check concludes; a candidate
changes materially; a prerequisite lands; a real conflict/interaction appears; or the
PR merges/closes.

If a lane is waiting on an unchanged remote event, leave it in GitHub and work another
claim. Do not poll.

## Select and run claim routes

Choose a bounded set of independent, actionable, goal-required claims. For each
selected claim, write the active route and then run it:

```text
`$deliver-goal`
→ `$deliver-pr`(#123)
→ `$orchestrate-work`
→ claim-local writer/review workers as useful
→ `$finish-pr`
→ lane result
```

A selected claim is not progress by itself. Invoke `$deliver-pr` or delegate the whole
flow immediately.

Use a whole-flow lane when a coherent claim needs sustained local memory:

```text
Take issue #123 through `$deliver-pr`.
You are the lane root for this claim only. Use `$orchestrate-work` within it, keep one
candidate writer, follow normal and material backward routes, and return
`RECONCILED`, `IN_FLIGHT`, `PARTIAL`, `SUPERSEDED`, `BLOCKED`, or `NOT_PROVEN` with
compact evidence.
```

The campaign root retains goal interpretation, claim selection, cross-claim decisions,
contradiction resolution, merge judgment, and final reconciliation.

## Claim selection

Select by judgment, not a durable score:

- release/blocker criticality;
- dependency-unlocking value;
- information gain;
- candidate/readiness state;
- local proof and hosted CI cost;
- available worktree/agent/build capacity;
- likelihood another result changes a decision.

Different claims may touch the same files or crates. One claim normally has one current
candidate and one writer. Coordinate only for duplicate claims, explicit prerequisites,
same-candidate writers, actual conflicts, destructive shared runtime state, or proven
combined-tree interactions.

## Useful GitHub boundaries

Write only reusable campaign or lane facts:

- corrected goal interpretation or acceptance predicate;
- current umbrella synthesis/plan;
- claim prerequisite, supersession, or actual interaction;
- PR claim/proof/limitation change;
- submitted review, inline finding, or evidence-backed disposition;
- named remote wait when another operator needs the handoff;
- merge/closure effect and remaining goal predicates.

Do not write frontier snapshots, assignments, liveness, skill-completion messages,
polling updates, transcripts, or unchanged summaries.

## Bounded related-PR review orchestration

When a bounded related PR set has interacting contracts, each PR still receives its
own `$deliver-pr`/`$finish-pr`/`$review-pr` route. After individual review, the campaign
root may synthesize:

| PR | Candidate/artifact identity | Substantive review result | Integration posture | Explicit prerequisite | Cross-PR contract |
| --- | --- | --- | --- | --- | --- |

Verify parent/child schema and validator agreement, complete candidate/artifact-set
identity, semantic owner, limitation/`NOT_PROVEN` propagation, whether fan-in loads
child evidence, and actual repair/merge order.

This is not batch approval or a portfolio queue.

## Loop

```text
reconstruct goal graph and runtime frontier
→ select independent actionable claims
→ run/delegate each selected `$deliver-pr` route
→ join compact lane results and update only useful GitHub facts
→ leave remote-owned waits `IN_FLIGHT`
→ reconcile merged/closed claims into the goal predicates
→ revisit lanes only on their wake event
→ continue until all predicates pass or every remaining claim shares one real blocker
```

Behind-only `main` movement requires no action. An actual conflict, explicit stack
change, or combined-tree failure is repaired by the affected lane with affected proof
and review only.

## Goal completion

Return `GOAL_SATISFIED` only when every acceptance predicate is `PASS` or explicitly
`NOT_APPLICABLE`, with current evidence and retained limitations named.

Return `GOAL_PARTIAL` only when the caller bounded progress or the durable outcome was
deliberately narrowed/superseded. Preserve failed and `NOT_PROVEN` predicates.

Use `EXTERNAL_BLOCKER` only when every remaining required claim shares one unresolved
external condition or material owner decision. Use `NOT_PROVEN` when the reliable goal
boundary or live graph cannot be reconstructed.

## What this does not establish

No repository scheduler, tracked frontier, active-goal pointer, portfolio database,
agent roster, liveness comments, overlap map, batch approval, or automatic merge
authority.