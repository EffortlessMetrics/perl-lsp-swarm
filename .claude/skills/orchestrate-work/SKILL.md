---
name: orchestrate-work
description: Compile Claude Code's runtime-local campaign, persistent claim-lane, and focused evidence-worker graph for the selected route; join evidence and preserve GitHub as durable state.
user-invocable: false
---

# Orchestrate work

Use this internal Claude operation after selecting a public flow or atomic skill. Run the
selected route and follow its named forward and backward edges. Use persistent claim
lanes for substantive independent work and focused subagents for bounded evidence.

This is not a public stage, durable executor DAG, scheduler, tracked frontier, lease
system, or source of transaction state.

## Context hierarchy

### Campaign root

Owns the durable goal, acceptance predicates, claim selection, cross-lane dependencies,
contradictions, proof debt, runtime-local frontier, joined evidence, exceptions, and
goal reconciliation.

The campaign root orchestrates by default. It selects PRs/claims, dispatches persistent
lanes, joins results, controls compute admission, resolves dependency and supersession
questions, and performs merge/close/park decisions. It should not become the routine
first reviewer, implementer, proof runner, CI diagnostician, or cleanup worker.

A failed child dispatch does not automatically turn the campaign root into the missing
worker. Reclaim completed lanes, join available evidence, route another decision, or
dispatch another useful claim first.

### Persistent claim lane

Owns one coherent acceptance-and-rollback claim, normally one issue or PR. It runs
`deliver-pr` and remains the same context while the route moves through review, repair,
proof, review refresh, live CI, and closeout.

The lane is not a stage-specific reviewer, writer, proof runner, or finisher. Those are
activities selected by skills inside the lane. Do not close and replace the lane merely
because `review-pr` returned `CHANGES_REQUIRED`, `prepare-proof` returned
`PROOF_READY`, or `verify-live-ci` returned a candidate-owned failure.

A claim lane may mutate its candidate when:

- its current skill result supports the repair;
- the repair remains inside the accepted claim and non-goals;
- its brief grants mutation/publication authority;
- no other writer is mutating the same candidate.

It keeps one candidate writer at a time, joins claim-local evidence, publishes useful
GitHub updates, and returns a typed `deliver-pr` result only at a real wait, blocker,
terminal disposition, or external-action boundary.

### Focused worker or review lens

Answers one bounded question or consumes one named skill. It may change source, oracle,
method, threat model, environment, or attention surface. It returns evidence,
falsifiers, contradictions, uncertainty, and references to its invoking lane.

Focused workers do not become claim owners, authorize merge, create rival candidates,
or force the durable lane to discard context. A child that creates a worktree or process
group owns cleanup of those resources.

Use ordinary subagents when independent results return to the lane root. Use Agent Teams
only when lateral communication changes the result. Use Ultracode inside one coherent
claim when tasks become ready dynamically; it is not repository state or a cross-claim
scheduler.

## Normal runtime shapes

| Work | Normal Claude Code shape |
| --- | --- |
| Goal meaning, claim selection, contradictions, integration | Campaign root |
| One PR or coherent claim | Persistent `deliver-pr` lane agent |
| Broad PR campaign | Several disjoint PR lanes, usually beginning in review |
| Missing review dimension | Focused read-only subagent/context fork inside the PR lane |
| Candidate mutation | The same PR lane acting as its one writer |
| Focused proof | The same PR lane when admitted, or a bounded evidence context when another environment materially helps |
| CI log/artifact classification | Focused read-only subagent returning to the PR lane |
| Coupled specialists needing lateral communication | Agent Team under one claim lane |
| Dynamic claim-local task graph | Ultracode under one claim lane |
| Unchanged remote wait | Lane returns `IN_FLIGHT`; campaign advances another claim |
| Explicit cleanup/admission question | Bounded campaign-ops context |

For a large PR queue, roughly five or six disjoint PR lanes may be useful when runtime
capacity supports them. That is a default review fan-out, not a topology, quota, or
occupancy target. Each lane may continue into repair and proof without being replaced.

## Lane continuity

Treat skill results as route transitions inside the same lane:

```text
`review-pr`: CHANGES_REQUIRED
→ same lane `address-review-comments` / `build-candidate`
→ same lane affected proof
→ same lane affected `final-challenge` / `review-pr`

`review-pr`: REVIEW_CURRENT
→ same lane `verify-live-ci`

`verify-live-ci`: PRODUCT_OR_TEST_FAILURE
→ same lane `build-candidate`
→ same lane affected proof and review

`verify-live-ci`: INTEGRATION_READY
→ same lane `merge-reconcile` when authorized
```

Do not create a review agent, then a repair agent, then a proof agent for the same PR.
That destroys loaded context and turns ordinary skill routing into orchestration work.
Use a new subagent only when an independent evidence surface can change the decision.

When a lane returns `IN_FLIGHT`, preserve its wake event. Resume the same thread when the
runtime retains it. If it cannot be resumed, reconstruct from GitHub and repository
artifacts without creating a second candidate.

## Continuous campaign flow

Treat lane results as a stream, not a batch barrier:

```text
dispatch useful disjoint PR lanes
→ each lane follows `deliver-pr`
→ join each typed result as it arrives
→ merge, close, park, record a blocker, or resume on its next skill
→ refill only when another independent claim is useful
```

Do not wait for every lane before acting on a completed result. Do not retain stale,
duplicate, completed, closed, cancelled, or missing handles to preserve a count. A lane
changing from review to repair remains one live lane.

## Capacity admission

Cap what consumes the host, not how many contexts exist.

```text
logical WIP    active campaign and claim contexts
review WIP     active read-only questions and evidence lenses
mutation WIP   candidates currently being edited
compute WIP    live build/test process groups and shared Cargo tokens
storage WIP    disk floor, target/cache footprint, safe reclaim state
proof debt     published or local behavioral changes still missing affected proof/review
```

Read-only GitHub/source review can be broad. Writers and heavy proof require bounded,
disjoint claims plus host admission. When proof debt grows, stop starting more mutation
and keep remaining capacity read-only until proof and merge closure catch up.

A lane that returned while its process group drains is `STOPPING`; its compute resource
is not released until the process tree exits. A PR waiting on GitHub may hold no local
resource at all.

When build admission fails, continue cheap review, evidence joining, and integration
decisions. A timing or flake measurement taken under saturation is `NOT_PROVEN`.

## Assignment contract

A persistent PR-lane brief normally needs only:

- exact PR/issue and coherent claim;
- accepted authority and non-goals when not obvious from the durable subject;
- mutation and publication authority;
- merge/close/issue-creation authority;
- worktree permission and local proof budget;
- known prerequisite, material finding, or hosted wake event.

The lane invokes `deliver-pr`; the skills own procedure and next-step routing. Do not
restate the full review, repair, proof, CI, and cleanup manuals in every brief.

A focused worker brief names:

- parent flow/skill and exact durable subject;
- one bounded question;
- accepted facts and authority;
- named skill when known;
- read/write boundary;
- realistic falsifiers and sufficient return;
- stop conditions, uncertainty, and non-goals;
- cleanup responsibility for resources it creates.

Head SHAs, check results, mergeability, and counts are observations, not leases or
default stop conditions. Re-read volatile state only when the next decision depends on
it: before a destructive action or merge, after a rejected push, or when intervening
content may materially conflict with or supersede the task.

## A quiet lane is not a result

Only a typed return or durable artifact establishes lane state.

```text
typed result returned        → join it
artifact shows work complete → synthesize the result, mark it synthesized, release lane
stated wait still current    → preserve wake event; continue independent campaign work
no artifact and no return    → FAILED_NO_RETURN; claim remains NOT_PROVEN
```

Silence is not spare capacity, abandonment, completion, or permission to start a second
writer. Inspect only the minimum artifact/process/worktree state needed for salvage,
wait, stop, or explicit reassignment.

## Runtime-local frontier

The campaign root may keep this in memory:

```text
claim / PR
acceptance predicate
lane handle
current skill and judgment
candidate/worktree when mutating
proof debt
external wait
next wake event
```

Reconstruct it from GitHub and repository artifacts after compaction or replacement.
Never commit it, post agent liveness, or turn it into a portfolio database.

## Evidence joins and returns

Join graph deltas rather than votes. Repeated claims from one source are not independent
corroboration. Preserve contradictions until direct evidence resolves them.

Focused workers return subject, conclusion, direct and contradictory evidence, searched
scope, affected claim/proof/authority edge, realistic falsifier, recommendation,
uncertainty, and `NOT_PROVEN` boundary.

Persistent PR lanes return candidate identity, skills traversed, changes made, proof
run/not run, findings and dispositions, substantive review result, live integration
posture, exact next skill or wake event, limitations, typed `deliver-pr` result, and
verified cleanup of lane-created resources no longer needed.

Every dispatched evidence lens owes a visible result. A dead lens leaves its dimension
`NOT_PROVEN`, not examined-and-clean.

## Worktree and process cleanup

Each context owns resources it creates.

- Read-only workers avoid worktrees unless checkout-local inspection or proof requires
  one.
- A persistent PR lane may keep one worktree across review, repair, and proof; do not
  delete it at every skill boundary.
- The lane removes its worktree after retained work is safely published or abandoned and
  no near-term same-lane transition needs the cache.
- A child that cannot clean up reports exact path, process, lock, reason, and verified
  state without bypassing execution policy.
- Preserve shared targets/caches, locked or ambiguous worktrees, and resources owned by
  another agent or tool.
- Broad cleanup runs only when storage blocks work or the campaign is closing.

Cleanup is not a standing review lane and should not displace actionable repository
work.

## Useful GitHub publication

Publish only when information changes claim, authority, plan, proof obligation, route,
prerequisite, support, risk, rollback, or closeout meaning; prevents source-backed
evidence from being rediscovered; records a localized finding/disposition; records a
real external wait/wake event; or provides a useful cumulative review or merged effect.

Keep agent identity, topology, liveness, retries, ordinary skill transitions, temporary
state, provisional reasoning, raw logs, and unchanged polling runtime-local.

## PR review orchestration

Inside one persistent PR lane:

```text
lane root
├── claim-vs-code propositions
├── `review-tests` for proof discrimination
├── `review-candidate` for implementation/reachability/complexity/risk
├── production-path trace when needed
├── external oracle when needed
└── focused security/package/migration/persistence/support lens when needed

join evidence
→ lane root publishes cumulative `review-pr`
→ same lane repairs accepted findings
→ same lane runs affected proof/review
→ same lane evaluates live CI and closeout
```

Different identity without a different source, oracle, method, threat model, environment,
or attention surface is not meaningful independence. Brief lenses to falsify named
propositions, not to say whether the PR looks good.

## Procedure

1. Anchor the durable subject and selected route.
2. Distinguish campaign, persistent claim-lane, and focused-worker scopes.
3. Dispatch the smallest useful graph: broad claim contexts, narrow mutation, scarce
   compute.
4. Send compact claim-lane briefs and complete focused-worker briefs.
5. Join each result as it arrives and let the same lane follow its next skill.
6. Steer, resume, retry, replace, or cancel only while evidence can change a decision.
7. Inspect load-bearing evidence and publish only durable facts at their native GitHub
   boundary.
8. Continue through the invoking flow or return its typed result.

## What this establishes

Persistent claim-local execution with context reuse across skill transitions, focused
evidence workers, one writer per candidate, compute-aware proof, contradiction-aware
joins, agent-owned cleanup, and useful GitHub handoffs.

## What this does not establish

A persistent scheduler, tracked frontier, stage-agent taxonomy, fixed agent count,
provider/model mandate, automatic truth from worker agreement, substantive review by
itself, integration readiness, or merge authorization.

## Stop and routes

Stop or return `NOT_PROVEN` for a same-candidate writer collision, unsafe destructive
action, unestablished identity/authority, unresolved material contradiction, or failed
instrumentation that blocks the claim. An unchanged remote wait, failed dispatch, head
SHA change, or unavailable cleanup operation does not stop independent campaign work.

- whole claim / one PR → persistent `deliver-pr` lane
- durable multi-PR goal → `deliver-goal`
- proof challenge → `review-tests` or `prepare-proof` inside the current lane
- candidate challenge → `review-candidate` inside the current lane
- cumulative judgment → `review-pr` inside the current lane
- finding repair → `address-review-comments` / `build-candidate` inside the current lane
- current review → `verify-live-ci` inside the current lane
- integration-ready / closeout → `merge-reconcile` inside the current lane when authorized
- changed authority/scope/claim → `prepare-issue`; split to a new lane only when the durable claim actually splits
