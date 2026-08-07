---
name: orchestrate-work
description: Compile and operate Claude Code's smallest useful campaign- or claim-local executor route while keeping runtime state ephemeral and GitHub writes evidence-bearing.
user-invocable: false
---

# Orchestrate work

Use this internal Claude operation after a public flow or atomic skill has been
selected. It compiles an ephemeral executor route from the durable
goal/claim/evidence graph. It is not a lifecycle stage, tracked frontier, executor
database, scheduler, lease system, or GitHub liveness protocol.

## Scope-relative roots

Identify the current scope before dispatching anything.

### Campaign root

The main Claude thread owns:

- verbatim goal and current interpretation;
- acceptance predicates and required claims;
- claim selection and cross-claim prerequisites;
- material contradictions and owner decisions;
- compact evidence joins;
- merge judgment and goal reconciliation.

Campaign-root leaf execution is exceptional. It may inspect one decisive seam or make
an orchestration/product decision directly, but broad source archaeology, log reading,
proof construction, implementation, repair, and specialist review normally leave this
context.

### Lane root

A whole-flow Claude agent may own one coherent acceptance-and-rollback claim through
`deliver-pr`:

- controlling issue and governing contract;
- semantic owner and live consumers;
- proof obligations;
- candidate, writer, review, and integration state;
- claim-level decisions and closeout.

A lane root may invoke `orchestrate-work` recursively within its claim. It may execute
tiny claim-local work directly when briefing and joining would cost more than the
context, but substantial leaf work normally goes to subagents or a dynamic workflow.

### Worker, writer, and reviewer contexts

- focused subagents answer one bounded read-only question;
- one writer mutates the current candidate;
- reviewers change the detection surface and return findings/evidence;
- a whole-flow `deliver-pr` delegate is a lane root, not merely a builder.

A leaf subagent may create further children only when its brief explicitly grants
claim-local orchestration authority. No child may select unrelated claims or expand
the parent goal.

## Authoritative inputs

Use current `origin/main`, the selected issue/PR/candidate, accepted contracts, current
proof/review, and live GitHub facts. Runtime teammate identity, liveness, task lists,
frontier, worktree map, retries, and transcripts are not authority.

## Run the selected route

Do not invent an ad-hoc lifecycle recipe when provider-native skills already define
the route. State the route in the active context and then invoke/operate it.

Examples:

```text
`deliver-goal`
→ `deliver-pr`(#123)
→ `orchestrate-work`
→ lane root delegates writer: `build-candidate`
→ lane root delegates reviewer: `review-tests`
→ lane root joins and runs `finish-pr`
```

```text
`finish-pr`(#456)
→ `orchestrate-work`
→ proof reviewer: `review-tests`
→ production-route reviewer: bounded question
→ candidate reviewer: `review-candidate`
→ lane root joins and runs `review-pr`
```

Every child brief carries:

- parent route and selected public flow/atomic skill;
- exact issue, PR, candidate, branch/worktree, and basis where relevant;
- established facts and accepted authority;
- one bounded question or mutation boundary;
- read-only, writer, reviewer, or lane-root status;
- one-writer identity for candidate mutation;
- realistic falsifiers or negative controls;
- sufficient return and stable evidence references;
- material backward routes, stop conditions, and non-goals.

Do not ask a child to rediscover facts already established. Do not replace a named
skill with a hand-written pseudo-flow.

## Runtime-local frontier

For campaign work, maintain a bounded in-context frontier:

| Claim | Goal predicate | Lane context | Durable subject | Current judgment | Next material action | External wait | Wake event |
| --- | --- | --- | --- | --- | --- | --- | --- |

The frontier is a working projection, not durable state. Reconstruct it after context
replacement from the umbrella/goal issue, directly linked issues and PRs, submitted
reviews, checks, merges, and repository evidence.

Do not commit it, comment it as lane status, or mirror it into labels, dashboards,
Teams tasks, memory, or agent-state files.

A wake event is the next fact capable of changing a lane decision, such as:

- writer/reviewer result arrives;
- material GitHub finding appears;
- required check concludes;
- candidate head changes materially;
- actual conflict or combined-tree interaction appears;
- prerequisite lands or changes;
- PR merges or closes.

Do not poll unchanged remote state. Return `IN_FLIGHT` and advance another claim when
useful.

## Topology selection

Choose the smallest route that improves the decision:

| Work | Normal Claude shape |
| --- | --- |
| Goal interpretation, claim selection, contradiction resolution | Campaign root |
| One coherent substantial claim | Whole-flow `deliver-pr` lane root |
| Tiny claim-local mechanical change | Lane root or current writer |
| High-output evidence, logs, corpus/repository sweep | Focused read-only subagent |
| Proof or candidate mutation | One writer |
| External oracle or production-path trace | Focused read-only subagent |
| Substantive review | Differentiated reviewer subgraph joined by lane root |
| Distinct independent claims | Separate lane roots/worktrees |
| Dynamically discovered claim-local task graph | Ultracode inside the lane |
| Sustained lateral coordination | Agent Team only when communication matters |
| Unchanged remote wait | No subagent; `IN_FLIGHT` |

Dispatch when expected evidence gain, root-context preservation, dependency-unlocking
value, elapsed-time gain, changed detection surface, or CI-cost avoidance exceeds
cold-start, briefing, duplicated research, resource contention, join, and correlated
failure costs.

Stop adding agents when another result cannot change a decision.

## Graph-delta returns

A read-only subagent returns:

```text
subject and basis
conclusion
direct evidence and authority
scope searched
contradiction or uncertainty
what this establishes
what this does not establish
affected claim/proof/authority edge
recommended route
NOT_PROVEN boundary
stable overflow references
```

A writer also returns:

```text
candidate identity
behavior and seams changed
proof executed
proof deliberately not executed
findings repaired
limitations
current GitHub state
typed flow result
```

A reviewer returns findings with severity, affected claim dimension, evidence,
realistic falsifier, uncertainty, and suggested disposition. It does not return
`mergeable` or approval authority.

The lane or campaign root must join evidence rather than votes. Repeated conclusions
derived from one source remain one evidence path. Builder self-report remains author
evidence. Preserve concrete contradictions until settled through evidence or an
accountable product decision.

## Useful GitHub boundary

Write to GitHub only when the returned information will help later work:

- issue correction, governing decision, synthesis, plan, or dependency;
- material prerequisite/supersession/interaction handoff;
- PR claim/proof/limitation/deviation update;
- inline finding or cumulative submitted review;
- evidence-backed disposition;
- remote-owned wait another operator must know;
- merge/closure result and residual claim.

Do not post agent assignments, liveness, frontier rows, stage/skill completion,
heartbeat/polling comments, raw logs, transcripts, or unchanged repeated summaries.
The route stays in runtime context; useful conclusions become GitHub artifacts.

## PR review orchestration

When invoked from `finish-pr` or `review-pr`, select applicable independent questions:

```text
lane root
├── `review-tests` for discrimination/evidence integrity
├── `review-candidate` for implementation/authority/reachability/risk
├── production-path trace from a real caller
├── competent external oracle
└── focused security/package/migration/persistence/support review

returns
→ lane root verifies load-bearing seams and contradictions
→ one writer repairs accepted findings
→ affected proof/review reruns
→ lane root publishes cumulative `review-pr`
```

Use a fresh context fork or focused reviewer when it changes the detection surface.
Use Teams only when reviewers or builders must communicate during the work. Different
agent identity is not independence by itself; change the source, oracle, method,
environment, threat model, or attention surface.

## Recommended procedure

1. Identify campaign-root or lane-root scope.
2. Anchor the durable goal/claim/candidate and current public flow.
3. Write the compact runtime route.
4. Identify unresolved judgments, resource constraints, and wake events.
5. Dispatch the smallest useful subagent/lane graph with complete briefs.
6. Steer, cancel, retry, or replace while another result can change a decision.
7. Join graph deltas, verify load-bearing evidence, and choose the next named route.
8. Update GitHub only at a useful durable boundary.
9. Return through the invoking skill with the typed result, `IN_FLIGHT`, or
   `NOT_PROVEN`.

## Routes

- durable multi-PR outcome → `deliver-goal`
- whole claim lane → `deliver-pr`
- unsettled claim/authority → `prepare-issue`
- proof creation or revision → `prepare-proof`
- candidate mutation → `build-candidate`
- proof challenge → `review-tests`
- candidate challenge → `review-candidate`
- PR convergence → `finish-pr`
- cumulative judgment → `review-pr`
- accepted finding repair → `address-review-comments` with one writer
- current review → `verify-live-ci`
- unchanged remote wait → return `IN_FLIGHT`
- missing identity/authority/evidence → return `NOT_PROVEN`

## Hard stops

Stop for same-candidate writer collision, destructive loss risk, unestablished durable
identity/authority, unsafe irreversible action, material contradiction requiring an
accountable owner decision, or failed evidence instrumentation. Do not stop merely
because a lane is waiting remotely or because another independent claim touches the
same files.