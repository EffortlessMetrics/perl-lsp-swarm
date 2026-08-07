---
name: orchestrate-work
description: Compile Claude Code's runtime-local campaign, lane, worker, writer, and review graph for the selected route; join evidence and preserve GitHub as durable state.
user-invocable: false
---

# Orchestrate work

Use this internal Claude operation after selecting a public flow or atomic skill. Run
the selected route, follow its named normal and material backward edges, and use
subagents, long-running lane agents, context forks, Ultracode, or Agent Teams where
they improve evidence, context economy, elapsed time, steering, recovery, or CI cost.

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

Owns one coherent claim. It runs `deliver-pr`, may invoke `orchestrate-work` within that
claim, keeps one candidate writer, joins claim-local evidence, publishes useful GitHub
updates, and returns a typed lane result.

A lane root may perform tiny tightly coupled claim-local work directly. That does not
make campaign-root leaf execution the normal path.

### Worker, writer, and reviewer

- read-only subagents answer one bounded question or consume one named skill;
- one writer mutates the selected candidate branch/worktree;
- reviewers change the source, oracle, method, threat model, environment, or attention
  surface and return evidence rather than approval.

A leaf worker may not widen into lane ownership unless its brief explicitly grants that
authority.

## Run the specified route

```text
campaign outcome
→ `deliver-goal`

one coherent claim
→ `deliver-pr`

bounded transformation
→ named atomic skill

bounded factual uncertainty
→ one focused question inside the invoking skill
```

A whole-flow assignment creates a lane root:

```text
Take issue #123 through `deliver-pr`.
You are the accountable lane root for this claim. Use GitHub and repository artifacts
as durable state, invoke `orchestrate-work` within the claim, keep one candidate
writer, follow normal and material backward routes, and return RECONCILED, IN_FLIGHT,
PARTIAL, SUPERSEDED, BLOCKED, or NOT_PROVEN.
Do not select unrelated claims or change the parent goal.
```

When a child receives a named skill, require it to consume that skill rather than
replace it with an invented lifecycle.

## Runtime shape

| Work | Normal Claude Code shape |
| --- | --- |
| Goal meaning, claim selection, contradictions | Campaign root |
| One substantial claim | Whole-flow `deliver-pr` lane agent |
| Tiny claim-local edit | Lane root or current writer |
| High-output/bounded exploration | Focused read-only subagent |
| Candidate/proof mutation | One writer |
| Substantive review | Lane-root-directed differentiated review/context forks |
| Dynamic claim-local task graph | Ultracode under the lane root |
| Coupled specialists needing lateral communication | Agent Team under the lane root |
| Distinct claims | Separate lane roots and worktrees |
| Unchanged remote wait | No agent; return `IN_FLIGHT` |

Delegate when evidence gain, campaign/lane-context preservation, parallel elapsed-time
gain, changed source/oracle/tool/environment, recovery value, or avoided CI cost
exceeds cold-start, briefing, duplicate research, resource contention, join, and
correlated-failure costs. Stop adding agents when another result cannot change a
decision.

Use ordinary subagents when results return independently. Use Agent Teams only when
lateral communication changes the result. Use Ultracode inside one coherent claim when
tasks become ready dynamically; it does not become repository state or a cross-claim
scheduler.

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
- named provider-native skill when known;
- campaign-root, lane-root, read-only, writer, or reviewer authority;
- candidate branch/worktree for mutation;
- realistic falsifiers and negative controls;
- sufficient return and stable evidence references;
- uncertainty, `NOT_PROVEN`, stop/backward routes, and non-goals.

Do not ask children to rediscover settled facts or return raw transcripts/private
reasoning.

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
├── `review-tests` for proof discrimination/evidence integrity
├── `review-candidate` for implementation, ownership, reachability, complexity,
│   compatibility, risk, and rollback
├── bounded production-path trace when component proof may not reach the live system
├── bounded external oracle when language/protocol/platform/release truth matters
└── focused security/package/migration/persistence/support lens when applicable

join evidence
→ lane root verifies load-bearing seams and contradictions
→ one writer repairs accepted findings through `address-review-comments`
→ lane root publishes cumulative `review-pr`
→ only `REVIEW_CURRENT` enters `verify-live-ci`
```

Do not use a subagent verdict as approval. Different identity without a different
source, oracle, method, threat model, or attention surface is not meaningful
independence.

## Procedure

1. Anchor the durable subject and selected route.
2. Distinguish campaign, lane, writer, worker, and review scopes.
3. Compile the smallest useful runtime graph.
4. Send complete briefs with named skills, falsifiers, returns, and stop conditions.
5. Steer, retry, replace, or cancel while evidence can change a decision.
6. Join graph deltas, inspect load-bearing evidence, and publish only useful durable
   facts at their native GitHub boundary.
7. Continue through the invoking flow's route or return its typed result.

## Stop and routes

Stop or return `NOT_PROVEN` for a same-candidate writer collision, unsafe destructive
action, unestablished identity/authority, unresolved material contradiction, or failed
instrumentation. An unchanged remote wait is `IN_FLIGHT`.

- whole claim → `deliver-pr`
- durable multi-PR goal → `deliver-goal`
- proof challenge → `review-tests` or `prepare-proof`
- candidate challenge → `review-candidate`
- cumulative judgment → `review-pr`
- finding repair → `address-review-comments` with one writer
- current review → `verify-live-ci`
- changed authority/scope/claim → `prepare-issue`
