---
name: orchestrate-work
description: Compile the smallest useful root-to-programme runtime shape for the selected route; keep claim orchestration in the main Claude thread, join evidence, and preserve GitHub as durable state.
user-invocable: false
---

# Orchestrate work

Use this internal Claude operation after selecting a public flow or atomic skill. Run
the selected route, follow its named normal and material backward edges, and decide
which work stays in the accountable main thread and which bounded programmes should run
in subagents, agent profiles, context forks, Ultracode, or Teams.

This is not a public stage, subordinate-orchestrator factory, durable executor DAG,
scheduler, tracked frontier, or source of transaction state.

## Logical authority

The main Claude thread owns orchestration across both goal and claim scopes:

- goal meaning and acceptance predicates;
- logical claim frames and claim selection;
- cross-claim dependencies and contradictions;
- current route and next/backward transition for each claim;
- joined evidence and review sufficiency;
- candidate writer allocation;
- finding disposition, integration judgment, and reconciliation;
- remote waits and wake events.

A claim/lane is a logical root-held frame. It does not normally create another
orchestrator agent.

Bounded execution contexts are:

- **researcher / read-only subagent** — source mapping, archaeology, external oracle,
  CI/log classification, bounded inventory;
- **builder / writer** — one candidate or proof mutation programme, one worktree;
- **reviewer** — one fixed subject and differentiated review programme;
- **context fork** — inherited context when useful, without creating independent
  evidence merely by inheritance;
- **Agent Team** — only when lateral communication changes the result;
- **Ultracode** — bounded dynamic execution when tasks become ready during one
  programme;
- **direct main-thread work** — tiny or tightly coupled judgment where delegation
  changes no evidence surface.

Nested agents, forks, Teams, and dynamic workflows are physical execution techniques.
They do not transfer logical claim orchestration or create a required recursive
hierarchy.

## Run the specified route

```text
durable goal
→ `deliver-goal`

one coherent claim frame
→ `deliver-pr`

bounded transformation
→ named atomic skill

bounded factual uncertainty
→ focused read-only question inside the invoking skill
```

When a child receives a named skill, require it to consume that skill rather than
replace it with an invented lifecycle.

## Runtime shape

| Work | Normal Claude shape |
| --- | --- |
| Goal meaning, claim selection, contradictions | Main thread |
| One substantial claim | Root-held claim frame running `deliver-pr` |
| Tiny tightly coupled work | Main thread/current writer |
| High-output or bounded exploration | Focused read-only subagent/researcher |
| Candidate/proof mutation | One builder/writer programme |
| Substantive review | One or more differentiated reviewer/fork programmes |
| Dynamic bounded task graph | Ultracode when useful |
| Coupled specialists needing lateral communication | Agent Team when useful |
| Distinct claims | Distinct root-held frames; separate writers/worktrees where needed |
| Unchanged remote wait | No live agent; claim frame is `IN_FLIGHT` |

Delegate when expected evidence gain, context preservation, parallel elapsed-time gain,
changed source/oracle/tool/environment, recovery value, or avoided CI cost exceeds
cold-start, briefing, duplicate research, resource contention, join, and
correlated-failure costs. Stop adding agents when another result cannot change a
decision.

## Programme continuity

The unit of delegation is a coherent **programme**, not necessarily one atomic skill.
Keep one context when the same subject/artifact understanding remains load-bearing:

```text
researcher: source map → external truth → issue-currency check
builder: proof → implementation → hardening → simplification → affected repair
reviewer: claim/proof/candidate lenses over one fixed subject
```

Atomic skills change attention and method. They do not automatically require a new
agent. Conversely, use a fresh context when a different source, oracle, threat model,
tool boundary, or independence property is the reason for delegation.

## Capacity admission

Size execution to the host, not to the amount of work available. Saturation destroys
evidence: once builds contend, local timings, flake rates, and command timeouts stop
being trustworthy.

Consume current local admission before dispatching a writer. Do not dispatch new
build-heavy mutation when writer/build capacity is exhausted, the workspace-wide Cargo
token is held, or disk/process/worktree state is `NOT_PROVEN`.

Capacity limits are host profiles, not repository invariants. Cap what consumes the
host—builds, worktrees, locks, disk—not an arbitrary number of cheap read-only agents.

Concurrent writers are not bounded by a global count. Separate specified claims may use
separate writers/worktrees; one candidate still has exactly one writer.

Keep these quantities distinct:

```text
logical WIP    root-held claim frames
mutation WIP   active writers and candidate worktrees
compute WIP    live build/test process groups and shared Cargo tokens
storage WIP    disk floor, target/cache footprint, safe reclaim state
```

A claim waiting on GitHub may hold no local resource and needs no live representative.

## A quiet agent is not a result

Every dispatched programme owes a typed return or leaves its dimension visibly
`NOT_PROVEN`. Silence does not transfer ownership and does not make a claim available to
a second writer.

When an agent goes quiet, inspect the durable artifact and local mutation state:

```text
typed result returned        → join it
artifact proves work landed  → synthesize a bounded return from the artifact
stated wait still current    → preserve IN_FLIGHT; no polling
no artifact and no return    → FAILED_NO_RETURN; affected dimension NOT_PROVEN
```

Before reassignment, establish whether the prior writer can resume, whether its process
has stopped, and whether uncommitted/unpushed work exists. Salvage useful work before
replacing a writer. Establishing those facts alone does not transfer mutation authority:
reassign only after the prior writer is provably unable to resume, or after an
acknowledged handoff has stopped or revoked it, so two writers never mutate the same
candidate concurrently.

## Root-held claim frames

The main thread may keep this runtime-local table:

```text
claim
acceptance predicate
durable issue / PR / merge subject
current candidate / writer
current judgment
external wait
next wake event
```

Reconstruct it from GitHub/repository artifacts after compaction or replacement. Never
commit it, post agent liveness, or turn it into a portfolio database. Revisit an
in-flight claim only when its wake event occurs.

## Assignment contract

Every brief names:

- parent flow/skill and exact durable subject;
- accepted authority and established facts;
- one bounded question or mutation boundary;
- named provider-native skill when known;
- execution authority: read-only | writer | reviewer;
- candidate branch/worktree for mutation;
- realistic falsifiers and negative controls;
- sufficient return and stable evidence references;
- uncertainty, `NOT_PROVEN`, stop/backward routes, and non-goals.

Do not grant a child generic claim-orchestration authority. If an experimental
whole-flow worker, nested agent, context fork, or Team is useful, bind it to the named
claim and route while the main thread retains decision/join authority.

**Separate the brief's stable part from its observed part.** The claim, acceptance
criteria, non-goals, and authorities are stable. Head SHAs, check results, mergeability,
and counts go stale faster than an executor can act on them.

Carry volatile values as an observation basis, not instructions:

```text
Observed as of <sha>: <PR state, head, then-discovered required-policy set and results>.
Re-derive live protection, rulesets, contexts, and results before mutating.
If materially different, return PREMISE_CHANGED, CANDIDATE_MOVED, or SUPERSEDED rather
than proceeding against stale state.
```

Discover required policy rather than naming a remembered set. Write any instruction
that names a specific PR, branch, or SHA as conditional.

## Graph-delta returns

Read-only workers return subject identity, conclusion, direct and contradictory
evidence, authority and searched scope, what is/is not established, the affected
claim/proof/authority edge, recommended route, `NOT_PROVEN` boundary, and overflow
references.

Writers return candidate identity, behavior/seams changed, proof run/not run, repaired
findings, limitations, current GitHub state, and typed result. Reviewers return
localized findings with severity, affected dimension, evidence, realistic falsifier,
uncertainty, and suggested disposition.

The main thread joins graph deltas rather than votes. Repeated claims from one source
are not independent corroboration. Preserve contradictions until direct evidence
resolves them.

Track dispatched review dimensions explicitly. A lens that fails to return leaves that
dimension `NOT_PROVEN`, not examined-and-clean.

## Useful GitHub publication filter

Publish when information changes claim, authority, plan, proof obligation, route,
prerequisite, support, risk, or rollback meaning; prevents useful source-backed evidence
from being rediscovered; records a localized finding or supported disposition; records
a real external wait/wake event; or provides a useful cumulative review, merged effect,
or goal synthesis.

Keep agent identity, topology, liveness, retries, temporary task state, provisional
reasoning, raw logs already referenced elsewhere, unchanged polling, and routine skill
transitions runtime-local.

## PR review orchestration

```text
root-held claim frame
├── claim-vs-code proposition checks
├── `review-tests` for proof discrimination/evidence integrity
├── `review-candidate` for implementation, ownership, reachability, complexity,
│   compatibility, risk, and rollback
├── bounded production-path trace when component proof may not reach the live system
├── bounded external oracle when language/protocol/platform/release truth matters
└── focused security/package/migration/persistence/support lens when applicable

join evidence in main thread
→ main thread verifies load-bearing seams and contradictions
→ one writer repairs accepted findings through `address-review-comments`
→ main thread publishes cumulative `review-pr`
→ only `REVIEW_CURRENT` enters `verify-live-ci`
```

Do not use a subagent verdict as approval. Different identity without a different
source, oracle, method, threat model, environment, or attention surface is not
meaningful independence.

## Procedure

1. Anchor the durable subject, root-held claim frame, and selected route.
2. Identify unresolved judgments and the one mutation owner.
3. Compile the smallest useful **flat root-to-programme** execution shape.
4. Send complete briefs with named skills, falsifiers, returns, and stop conditions.
5. Steer, retry, replace, or cancel while evidence can change a decision.
6. Join graph deltas in the main thread, inspect load-bearing evidence, and publish only
   useful durable facts at their native GitHub boundary.
7. Continue through the invoking flow's route or update the claim frame with its typed
   result/wake event.

## What this establishes

A provider-native root orchestration result with root-held claim frames, bounded
programme delegation, one candidate writer, contradiction-aware evidence joins, named
backward routes, and a useful GitHub publication boundary.

## What this does not establish

A subordinate claim orchestrator, persistent executor graph, scheduler, tracked
frontier, lane lease, fixed agent count, provider/model mandate, automatic truth from
worker agreement, substantive review by itself, integration readiness, or merge
authorization.

## Stop and routes

Stop or return `NOT_PROVEN` for a same-candidate writer collision, unsafe destructive
action, unestablished identity/authority, unresolved material contradiction, or failed
instrumentation. An unchanged remote wait is `IN_FLIGHT`.

- current claim → `deliver-pr`
- durable multi-PR goal → `deliver-goal`
- proof challenge → `review-tests` or `prepare-proof`
- candidate challenge → `review-candidate`
- cumulative judgment → `review-pr`
- finding repair → `address-review-comments` with one writer
- current review → `verify-live-ci`
- changed authority/scope/claim → `prepare-issue`
