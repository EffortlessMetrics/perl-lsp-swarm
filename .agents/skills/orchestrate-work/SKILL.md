---
name: orchestrate-work
description: Compile Codex's runtime-local campaign, lane, worker, writer, and review graph for the selected route while preserving GitHub as durable state.
---

# Orchestrate work

Use this internal Codex operation after a public flow or atomic skill has been selected.
The orchestrator does not merely choose a topology: it runs the selected route, follows
its named normal and material backward edges, and uses subagents where they improve
evidence, context economy, elapsed time, steering, recovery, or CI cost.

This skill is not a public lifecycle stage, durable executor DAG, repository scheduler,
tracked frontier, or source of transaction state.

## Scope-relative roots

`Root` is scope-relative. Distinguish these contexts before dispatching work.

### Campaign root

Owns one durable multi-claim outcome:

- original goal and current interpretation;
- acceptance predicates and required claims;
- cross-lane dependencies and contradictions;
- runtime-local frontier and wake events;
- goal-level evidence joins, exceptions, and reconciliation.

The campaign root normally orchestrates. Leaf implementation, broad archaeology, raw
log analysis, repetitive proof, and review exploration should leave this context unless
the work is so small that briefing and joining clearly cost more than the permanent
context pollution.

### Lane root

Owns one coherent acceptance-and-rollback claim. It runs `$deliver-pr`, may invoke
`$orchestrate-work` inside that claim, keeps one candidate writer, joins claim-local
evidence, publishes useful GitHub updates, and returns a typed lane result to its
campaign root.

A lane root may directly perform tiny tightly coupled claim-local work. That does not
make campaign-root leaf execution the normal path.

### Worker, writer, and reviewer contexts

- read-only workers answer one bounded question or consume one named `$skill`;
- one writer mutates the selected candidate branch/worktree;
- reviewers change the source, oracle, method, threat model, environment, or attention
  surface and return evidence rather than approval.

A leaf worker may not widen into a claim orchestrator unless its brief explicitly grants
lane-root authority.

## Authoritative inputs

Use current `origin/main`, the selected issue/PR and candidate identity, governing
repository artifacts, relevant proof and review evidence, and live GitHub state.
Runtime task lists, frontier tables, subagent identity, worktrees, retries, and prior
transcripts are not authority and must not be written to tracked state files.

## Run the specified route

Anchor the goal, claim, controlling issue, current flow/skill, candidate or PR identity,
and current missing judgment.

Then run the named route:

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

A whole-flow assignment is a lane-root assignment, not a writer prompt:

```text
Take issue #123 through `$deliver-pr`.
You are the accountable lane root for this claim. Use GitHub and repository artifacts
as durable state, invoke `$orchestrate-work` within the claim, keep one candidate
writer, follow the public flow's normal and material backward routes, and return
RECONCILED, IN_FLIGHT, PARTIAL, SUPERSEDED, BLOCKED, or NOT_PROVEN.
Do not select unrelated claims or alter the parent goal.
```

When a child is given a named `$skill`, require it to consume that skill rather than
replace it with an invented lifecycle recipe.

## Runtime shape

Choose proportionally:

| Work | Normal Codex shape |
| --- | --- |
| Goal interpretation, claim selection, contradiction resolution | Campaign root |
| One coherent substantial claim | Whole-flow `$deliver-pr` lane root |
| Tiny claim-local edit | Lane root or current writer |
| High-output or bounded exploratory evidence | Focused read-only worker/explorer |
| Candidate/proof mutation | One integrating writer |
| Substantive review | Lane-root-directed differentiated review subgraph |
| Distinct claims | Separate lane roots and worktrees |
| Unchanged remote wait | No agent; return `IN_FLIGHT` |

Substantive work is normally orchestrated, but maximal fan-out is not sophistication.
Delegate when evidence gain, root-context preservation, parallel elapsed-time gain,
changed source/oracle/tool/environment, recovery value, or avoided CI cost exceeds
cold-start, briefing, duplicate research, resource contention, join, and correlated-
failure costs. Stop adding agents when another result cannot change a decision.

## Runtime-local frontier

A campaign root may keep a compact in-memory frontier:

```text
claim
acceptance predicate
lane context
durable issue / PR / merge subject
current judgment
external wait
next wake event
```

Reconstruct it from GitHub and repository artifacts after compaction or replacement.
Do not commit it, write it to a tracked file, post agent liveness, or turn it into a
portfolio database. Revisit an in-flight lane only when its wake event occurs.

## Assignment contract

Every brief names:

- parent flow or skill;
- exact issue, PR, candidate, and branch/worktree identity;
- established facts and accepted authority;
- one bounded question or mutation boundary;
- the provider-native `$skill` the child must consume when known;
- campaign-root, lane-root, read-only, writer, or reviewer authority;
- realistic falsifiers or negative controls;
- sufficient output and stable evidence references;
- uncertainty and `NOT_PROVEN` conditions to preserve;
- stop/backward routes and non-goals.

Do not ask a child to rediscover settled facts. Do not request raw transcripts, private
reasoning, claim digests, or review-run receipts.

## Graph-delta returns

Read-only workers return:

```text
subject identity
conclusion
direct evidence
contradictory evidence
authority and searched scope
what this establishes
what this does not establish
affected claim / proof / authority edge
recommended route
NOT_PROVEN boundary
overflow references
```

Writers return candidate identity, changed behavior and seams, proof executed, proof
not executed, repaired findings, limitations, current GitHub state, and the typed flow
result. Reviewers return localized findings with severity, affected claim dimension,
evidence, realistic falsifier, uncertainty, and suggested disposition.

The orchestrator joins graph deltas rather than votes. Repeated claims sourced from the
same evidence are not independent corroboration. Preserve contradictions until direct
evidence resolves them.

## Useful GitHub publication filter

GitHub receives durable work facts, not runtime supervision.

Post or update GitHub when information:

- changes the claim, authority, accepted plan, proof obligation, route, prerequisite,
  support boundary, risk, or rollback meaning;
- is a reusable source-backed finding another lane or later context would otherwise
  rediscover;
- is a localized review finding that belongs inline;
- dispositions a finding with current evidence;
- records a real external wait and the event that should resume the lane;
- provides a useful cumulative review, merged-effect, or goal synthesis.

Keep information runtime-local when it is agent identity, topology, liveness, retry
order, provisional reasoning, raw logs already referenced elsewhere, unchanged polling,
or a routine transition already implied by the selected route.

Use the native surface that owns the fact:

- issue comment/body: durable research, ruling, dependency, accepted plan, route change,
  or goal synthesis;
- PR body/comment: candidate-wide route, proof, limitation, or integration synthesis;
- inline review: localized finding;
- review reply: evidence-backed disposition before resolution;
- submitted review: cumulative candidate judgment;
- issue closeout: landed effect and residual claims.

## Traceable route

When another context will benefit and the route is not already obvious, publish one
compact route declaration on the controlling issue or PR:

```text
Route
- Goal / parent: <issue or durable outcome>
- Claim: <one acceptance-and-rollback claim>
- Entry flow: <public flow>
- Current useful transition: <named skill or external wait>
- Why: <material missing judgment>
- Durable subject: <issue / PR / merged commit>
- Resume when: <material wake event, if any>
```

Update it only when the material route changes. Do not post one comment per skill,
agent, poll, or head SHA. The route declaration is a resumability aid, not stage or
lifecycle authority.

## PR review orchestration

When invoked from `$finish-pr` or `$review-pr`, build a bounded review subgraph around
the actual claim and risk—not a fixed reviewer roster.

```text
lane root
├── `$review-tests` when proof discrimination or evidence integrity is material
├── `$review-candidate` when implementation, ownership, reachability, complexity,
│   compatibility, risk, or rollback is material
├── bounded production-path trace when component proof may not reach the live system
├── bounded external oracle when language/protocol/platform/release truth matters
└── focused security/package/migration/persistence/support lens when applicable

joined evidence
→ lane root verifies load-bearing seams and contradictions
→ one writer repairs accepted findings through `$address-review-comments`
→ lane root performs and publishes cumulative `$review-pr`
→ only `REVIEW_CURRENT` enters `$verify-live-ci`
```

Do not use a subagent verdict as approval. Different identity without a different
source, oracle, method, threat model, or attention surface is not meaningful
independence.

## Recommended procedure

1. Anchor the durable subject and selected route.
2. Distinguish campaign, lane, writer, worker, and review scopes.
3. Identify unresolved judgments and compile the smallest useful runtime graph.
4. Send complete briefs with named skills, authority, falsifiers, return packets, and
   stop conditions.
5. Steer, retry, replace, or cancel while returned evidence can still change a
   decision.
6. Join graph deltas, verify load-bearing evidence, and publish only useful durable
   facts at their native GitHub boundary.
7. Continue through the invoking flow's named route or return the exact typed result.

## Review currentness

A later commit does not invalidate review merely because the SHA changed. Revisit only
findings, proof, claims, production paths, authority, compatibility, risk, rollback, or
integration dimensions materially changed by later work.

## Actual stop conditions

Stop or return `NOT_PROVEN` for a same-candidate writer collision, unsafe destructive
action, unestablished identity or authority, contradictory evidence requiring an
accountable product decision, or failed instrumentation. An unchanged remote wait is
`IN_FLIGHT`, not a local blocker.

## Routes

- whole claim lane → `$deliver-pr`
- durable multi-PR outcome → `$deliver-goal`
- proof challenge → `$review-tests` or `$prepare-proof`
- candidate challenge → `$review-candidate`
- cumulative PR judgment → `$review-pr`
- accepted finding repair → `$address-review-comments` with one writer
- current substantive review → `$verify-live-ci`
- changed authority, scope, or claim → `$prepare-issue`
- unchanged GitHub wait → return `IN_FLIGHT`
- missing reliable identity, authority, or evidence → return `NOT_PROVEN`
