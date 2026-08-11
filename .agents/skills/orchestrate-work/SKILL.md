---
name: orchestrate-work
description: Compile Codex's runtime-local campaign, lane, worker, writer, and review graph for the selected route; join evidence and preserve GitHub as durable state.
---

# Orchestrate work

Use this internal Codex operation after selecting a public flow or atomic skill. Run the
selected route, follow its named normal and material backward edges, and use workers or
lane roots where they improve evidence, context economy, elapsed time, steering,
recovery, or CI cost.

This is not a public stage, durable executor DAG, scheduler, tracked frontier, or source
of transaction state.

## Scope hierarchy

### Campaign root

Owns the durable goal, acceptance predicates, claim selection, cross-lane dependencies,
contradictions, runtime-local frontier, joined evidence, exceptions, and goal
reconciliation.

The campaign root normally orchestrates. Leaf implementation, broad archaeology, raw
logs, repetitive proof, and review exploration should leave this context unless direct
inspection of one load-bearing seam is itself the campaign judgment.

### Lane root

Owns one coherent claim. It runs `$deliver-pr`, may invoke `$orchestrate-work` within
that claim, keeps one candidate writer, joins claim-local evidence, publishes useful
GitHub updates, and returns a typed lane result.

A lane root may perform tiny tightly coupled claim-local work directly. That does not
make campaign-root leaf execution the normal path.

### Worker, writer, and reviewer

- read-only workers answer one bounded question or consume one named `$skill`;
- one writer mutates the selected candidate branch/worktree;
- reviewers change the source, oracle, method, threat model, environment, or attention
  surface and return evidence rather than approval.

A leaf worker may not widen into lane ownership unless its brief explicitly grants that
authority.

## Run the specified route

```text
campaign outcome
→ `$deliver-goal`

one coherent claim
→ `$deliver-pr`

bounded transformation
→ named atomic `$skill`

bounded factual uncertainty
→ one focused question inside the invoking skill
```

A whole-flow assignment creates a lane root:

```text
Take issue #123 through `$deliver-pr`.
You are the accountable lane root for this claim. Use GitHub and repository artifacts
as durable state, invoke `$orchestrate-work` within the claim, keep one candidate
writer, follow normal and material backward routes, and return RECONCILED, IN_FLIGHT,
PARTIAL, SUPERSEDED, BLOCKED, or NOT_PROVEN.
Do not select unrelated claims or change the parent goal.
```

When a child receives a named `$skill`, require it to consume that skill rather than
replace it with an invented lifecycle.

## Runtime shape

| Work | Normal Codex shape |
| --- | --- |
| Goal meaning, claim selection, contradictions | Campaign root |
| One substantial claim | Whole-flow `$deliver-pr` lane root |
| Tiny claim-local edit | Lane root or current writer |
| High-output/bounded exploration | Focused read-only worker or explorer |
| Candidate/proof mutation | One writer |
| Substantive review | Lane-root-directed differentiated review subgraph |
| Distinct claims | Separate lane roots and worktrees |
| Unchanged remote wait | No agent; return `IN_FLIGHT` |

Delegate when evidence gain, campaign/lane-context preservation, parallel elapsed-time
gain, changed source/oracle/tool/environment, recovery value, or avoided CI cost
exceeds cold-start, briefing, duplicate research, resource contention, join, and
correlated-failure costs. Stop adding agents when another result cannot change a
decision.

## Capacity admission

Size the runtime graph to the host, not to the work available. Saturation destroys
evidence rather than only delaying it: once builds contend, local timings, flake rates,
and command timeouts stop being trustworthy, and the root begins dispatching diagnostic
agents into ambiguity it produced itself.

Consume the current local admission result before dispatching a writer. Do not dispatch
when writer capacity is exhausted, heavy-build capacity is exhausted, the workspace-wide
Cargo token is held, or disk/process/worktree state is `NOT_PROVEN`.

Capacity limits are a host profile, not a repository invariant. A workstation, a laptop,
a remote builder, and a read-only review context have different envelopes. Until #3957
provides an admission command, apply the profile recorded in local configuration; the
initial single-workstation profile is one build-heavy writer and one workspace-wide
build.

Cap what consumes the host, which is builds — not how many agents exist. A read-only
worker reading GitHub and source holds no worktree, no build, and no locks, so its limit
is the attention available to steer it. Rationing cheap workers while build-heavy work
runs unbounded caps the wrong thing.

Concurrent writers are likewise not bounded by a count. Two writers on two claims is safe
when both claims are specified and disjoint, and unsafe when they are vague, because
vague claims overlap and overlapping writers produce rework rather than parallelism. The
precondition for a second writer is a specification, not a slot.

Count what the host carries, not what was dispatched. These quantities come apart:

```text
logical WIP    active campaign and lane contexts
mutation WIP   active writers and candidate worktrees
compute WIP    live build/test process groups and workspace-wide Cargo tokens
storage WIP    disk floor, target/cache footprint, safe reclaim state
```

A lane that has returned while its process group still drains is `STOPPING`. Its build
token is not released until the process tree exits and its locks are gone. A claim
waiting on GitHub may hold no local resource at all.

- never launch a replacement for the same claim from silence. An independent claim may
  proceed when the campaign phase permits it, no equivalent candidate owns it, admission
  returns `ADMIT`, and the waiting lane has released the resources the new lane needs;
- read-only inspection of GitHub or source requires no worktree; allocate one only for a
  named mutation claim. Ordinary `git worktree` and an optional `$worktree-manager` slot
  are both valid routes consuming the same admission result; the helper is a cleanup
  lease, not the capacity authority;
- when admission fails, wait; declining to dispatch is a valid orchestration action;
- a local timing or flake-rate measurement taken under saturation is `NOT_PROVEN` and
  must be reported as such rather than as a number.

## A quiet agent is not a result

Only a typed return ends a lane. An idle signal, a terminated process, an exhausted
budget, or prolonged silence says nothing about the claim.

When a lane goes quiet, inspect the artifact rather than the agent — PR state, branch
head, worktree status, live checks:

```text
typed result returned        → join it
artifact shows the work done → synthesize the typed result from the artifact, record it
                               as synthesized, then release the lane
stated wait still current    → leave it; an unchanged remote wait is IN_FLIGHT
no artifact and no return    → FAILED_NO_RETURN; claim state is NOT_PROVEN
```

`FAILED_NO_RETURN` is not a finding of abandonment. Silence establishes nothing about
whether the worker is dead, the process group has stopped, the worktree is clean, a
remote head moved, or uncommitted work exists. Read the task handle, process group,
branch, worktree, remote head, and durable subject, then choose salvage, wait, stop, or
explicit reassignment. Reassignment follows those checks and is never the default
consequence of silence; treating quiet as an unowned claim is what puts two writers on
one candidate.

Silence is also not spare capacity and not completion. A lane holding a current wait
condition is not stalled, and re-tasking it discards work in flight.

## Runtime-local frontier

A campaign root may keep this in memory only:

```text
claim
acceptance predicate
lane context
durable issue / PR / merge subject
current judgment
external wait
next wake event
```

Reconstruct it from GitHub/repository artifacts after replacement. Never commit it,
write it to a tracked file, post agent liveness, or turn it into a portfolio database.
Revisit an in-flight lane only when its wake event occurs.

## Assignment contract

Every brief names:

- parent flow/skill and exact durable subject;
- accepted authority and established facts;
- one bounded question or mutation boundary;
- named provider-native `$skill` when known;
- campaign-root, lane-root, read-only, writer, or reviewer authority;
- candidate branch/worktree for mutation;
- realistic falsifiers and negative controls;
- sufficient return and stable evidence references;
- uncertainty, `NOT_PROVEN`, stop/backward routes, and non-goals.

Do not ask children to rediscover settled facts or return raw transcripts/private
reasoning.

Separate the brief's stable part from its observed part. Claim, acceptance, non-goals,
and authorities are stable. Head SHAs, check results, mergeability, and counts go stale
faster than a child can act on them, and an instruction resting on stale state is
unexecutable rather than merely inaccurate.

Do not delete the volatile state — the child needs it to see which premise moved. Carry
it as an observation basis with an entry condition:

```text
Observed as of <sha>: <pr state, head, the then-discovered required-policy set and its
results>.
Re-derive live protection, rulesets, contexts, and results before mutating.
If materially different, return PREMISE_CHANGED, CANDIDATE_MOVED, or SUPERSEDED instead
of proceeding.
```

Discover a required-policy set rather than naming a remembered one. Classic branch
protection and rulesets are independent and additive, so a brief asserting a fixed count
of required checks states exactly the kind of premise this section exists to prevent.

Express any instruction naming a specific PR, branch, or SHA conditionally, so a child
that finds the world changed has a defined action instead of a contradiction.

## Graph-delta returns

Read-only workers return subject identity, conclusion, direct and contradictory
evidence, authority and searched scope, what is/is not established, the affected
claim/proof/authority edge, recommended route, `NOT_PROVEN` boundary, and overflow
references.

Writers return candidate identity, behavior/seams changed, proof run/not run, repaired
findings, limitations, current GitHub state, and typed result. Reviewers return
localized findings with severity, affected dimension, evidence, realistic falsifier,
uncertainty, and suggested disposition.

The root must join evidence as graph deltas rather than votes. Repeated claims from one
source are not independent corroboration. Preserve contradictions until direct evidence
resolves them.

Every dispatched agent owes a typed return. Track what was dispatched: a lens that dies
— exhausted budget, killed process, tooling failure — leaves its dimension `NOT_PROVEN`,
not examined-and-clean. An unnoticed absent return is indistinguishable from a clean
one, which is the failure the review method exists to prevent.

Remembering a dead lens does not by itself make merge refusal reliable. Carry the
dispatch list into the review join as an explicit dimension ledger, so the cumulative
result rests on enumerated dimensions rather than on whichever reviews returned:

```text
claim-vs-code     REVIEWED
proof             REVIEWED
shutdown safety   NOT_PROVEN   (lens dispatched, no return)
external oracle   NOT_APPLICABLE
```

Wiring that ledger into the convergence predicate governing merge is tracked separately
under #3693. This skill requires only that the dispatch be recorded and the absence be
visible to the join.

## Useful GitHub publication filter

Publish when information changes claim, authority, plan, proof obligation, route,
prerequisite, support, risk, or rollback meaning; prevents useful source-backed evidence
from being rediscovered; records a localized finding or supported disposition; records
a real external wait/wake event; or provides a useful cumulative review, merged effect,
or goal synthesis.

Keep agent identity, topology, liveness, retries, temporary task state, provisional
reasoning, raw logs already referenced elsewhere, unchanged polling, and routine skill
transitions runtime-local.

Use issues for durable research/rulings/plans/dependencies/goal synthesis; PR bodies or
comments for candidate-wide route/proof/limitation summaries; inline review for
localized findings; review replies for dispositions; submitted reviews for cumulative
judgment; and issue closeout for landed effects and residual claims.

## Traceable route

When another context will benefit and the route is not obvious, publish one compact
issue/PR declaration:

```text
Route
- Goal / parent: <issue or durable outcome>
- Claim: <one acceptance-and-rollback claim>
- Entry flow: <public flow>
- Current useful transition: <named skill or external wait>
- Why: <material missing judgment>
- Durable subject: <issue / PR / merged commit>
- Resume when: <wake event, if any>
```

Update only when the material route changes. It is a resumability aid, not stage,
lifecycle, or lease authority.

## PR review orchestration

```text
lane root
├── claim-vs-code: each property the PR body asserts, verified against the diff
├── `$review-tests` for proof discrimination/evidence integrity
├── `$review-candidate` for implementation, ownership, reachability, complexity,
│   compatibility, risk, and rollback
├── bounded production-path trace when component proof may not reach the live system
├── bounded external oracle when language/protocol/platform/release truth matters
└── focused security/package/migration/persistence/support lens when applicable

join evidence
→ lane root verifies load-bearing seams and contradictions
→ one writer repairs accepted findings through `$address-review-comments`
→ lane root publishes cumulative `$review-pr`
→ only `REVIEW_CURRENT` enters `$verify-live-ci`
```

Do not use a subagent verdict as approval. Different identity without a different
source, oracle, method, threat model, or attention surface is not meaningful
independence.

Brief each lens to falsify a named claim rather than assess it. Each returns the angles
it attempted with outcomes, refuted ones included, so the join can distinguish coverage
from agreement.

## Procedure

1. Anchor the durable subject and selected route.
2. Distinguish campaign, lane, writer, worker, and review scopes.
3. Compile the smallest useful runtime graph.
4. Send complete briefs with named skills, falsifiers, returns, and stop conditions.
5. Steer, retry, replace, or cancel while evidence can change a decision.
6. Join graph deltas, inspect load-bearing evidence, and publish only useful durable
   facts at their native GitHub boundary.
7. Continue through the invoking flow's route or return its typed result.

## What this establishes

A claim-local, provider-native runtime route with explicit campaign/lane/worker scopes,
complete child briefs, one candidate writer, bounded parallelism, contradiction-aware
evidence joins, named backward routes, and a useful GitHub publication boundary. The
invoking flow can continue from a typed result without importing raw worker context.

## What this does not establish

A persistent executor graph, scheduler, tracked frontier, lane lease, fixed agent count,
provider/model mandate, automatic truth from worker agreement, substantive review by
itself, integration readiness, or merge authorization.

## Stop and routes

Stop or return `NOT_PROVEN` for a same-candidate writer collision, unsafe destructive
action, unestablished identity/authority, unresolved material contradiction, or failed
instrumentation. An unchanged remote wait is `IN_FLIGHT`.

- whole claim → `$deliver-pr`
- durable multi-PR goal → `$deliver-goal`
- proof challenge → `$review-tests` or `$prepare-proof`
- candidate challenge → `$review-candidate`
- cumulative judgment → `$review-pr`
- finding repair → `$address-review-comments` with one writer
- current review → `$verify-live-ci`
- changed authority/scope/claim → `$prepare-issue`
