# Skill contract

Skills are small, self-navigating artifact transformations. They make the next useful
judgment and route clear without turning the repository into a runtime workflow engine,
tracked frontier, or cross-lane coordinator.

## Provider-native operational authority

Shared docs define method and structural expectations. They are not substitutes for
the instructions a running provider consumes.

```text
Claude Code operational skills
→ .claude/skills/*

Codex operational skills
→ .agents/skills/*
```

A substantive skill must be operationally complete in both provider implementations.
It may link shared context, but it cannot require the agent to reconstruct its actual
procedure from one provider-neutral authority document.

## Scope-relative execution

Skills may be consumed by different runtime scopes:

```text
campaign root
→ durable goal, acceptance predicates, bounded frontier, cross-lane decisions,
  evidence joins, and goal reconciliation

lane root
→ one coherent claim, its named deliver-pr route, claim-local orchestration,
  one candidate writer, review sufficiency, integration, and closeout

worker / writer / reviewer
→ one bounded skill or question, one mutation surface where applicable,
  compact evidence and a typed return
```

Campaign-root leaf execution is exceptional. Lane-root direct execution remains valid
for tiny claim-local work where briefing and joining cost more than the context saved.
A whole-flow lane may invoke `orchestrate-work` within its claim. A leaf worker may not
expand into lane ownership unless its brief explicitly grants that authority.

## Required skill shape

Every public flow and substantive atomic skill must expose the applicable semantics
below. This is an affordance vocabulary for orchestrators and validators, not a demand
that every skill use these exact headings or include inapplicable fields.

```text
Purpose
Use when
Do not use when
Authoritative inputs
Root-retained decisions
Delegable read-only questions
Mutation owner
Useful parallelism / communication requirement
Recommended procedure
Join predicate
Required return packet
GitHub reads and useful durable updates
What stays runtime-local
External wait / wake event where applicable
What this establishes
What this does not establish
Valid exits and routes
Actual stop conditions
```

A skill may combine fields under provider-native headings such as scope hierarchy,
assignment contract, graph-delta returns, runtime boundary, or goal-completion
contract. It must still make the relevant decision owner, mutation owner, delegation
surface, join/return boundary, durable/runtime split, and establishment/nonclaim
boundary unambiguous enough for `orchestrate-work` and maintenance-time validation.

## Canonical skill vocabulary

The public flows are:

```text
deliver-goal
deliver-pr
prepare-issue
prepare-proof
build-candidate
finish-pr
```

The atomic transformation catalog is:

```text
# Issue and plan
find-or-create-issue
research-issue
review-issue
issue-to-plan
research-plan
review-plan
compile-spec

# Proof, build, and hardening
spec-to-test
review-tests
build-from-proof
improve-test-suite
simplify-candidate
review-candidate

# Publication, review, and integration
publish-pr
address-review-comments
final-challenge
review-pr
verify-live-ci
merge-reconcile
```

Public flows are natural campaign/lane entrypoints. Atomic skills normally execute in
worker, writer, reviewer, or lane-root contexts. Adding, renaming, or removing a skill
is a control-plane change and must update both provider implementations and route
validation.

The internal `orchestrate-work` operation is root-facing runtime compilation guidance,
not a public stage. It selects claim lanes, bounded questions, atomic-skill assignments,
one candidate writer, differentiated review contexts, or independent claim lanes. It
must run the route selected by the invoking flow and return to that flow.

It must not encode a durable executor DAG, tracked frontier, lease, reservation,
liveness state, provider, model, agent count, team topology, or comment-per-transition
protocol.

## Local route grammar

Use direct callable provider-local skill names in route sections.

```text
PLAN_READY
  → prepare-proof

PROOF_READY
  → build-candidate

CANDIDATE_READY
  → finish-pr

MATERIAL_PREMISE_CHANGED
  → prepare-issue

WEAK_PROOF
  → review-tests

REVIEW_FINDINGS_OPEN
  → address-review-comments

REVIEW_REQUIRED
  → final-challenge, orchestrate-work, then review-pr

REVIEW_CURRENT
  → verify-live-ci

PR_IN_FLIGHT
  → return to deliver-goal so another distinct claim may proceed

ALREADY_SATISFIED
  → return to deliver-pr for reconciliation

NOT_APPLICABLE
  → return to the public flow and select the next applicable skill

BLOCKED / NOT_PROVEN
  → name the exact dependency, authority, instrument, or evidence gap
```

Selecting a route is not completion. The orchestrator invokes the selected skill and
follows its named next or material backward edge. Do not mix stage identifiers, agent
identities, label names, guessed command names, and callable skills in one exit
vocabulary.

## Applicability

The normal path runs every applicable pass. A pass is not applicable only when:

- its subject genuinely does not exist;
- current evidence already establishes the same judgment;
- the change is proportionally mechanical and has no corresponding decision;
- the flow entered after that judgment and replay would add no value.

A missed earlier pass causes forward repair, not retrospective punishment.

## Candidate and lane rule

A substantive skill operates on one selected claim and its current candidate.

- one claim normally has one current candidate;
- one writer mutates that candidate branch/worktree at a time;
- focused research, oracle, proof, review, and CI evidence work may assist;
- helpers do not inspect sibling lanes or touched-file overlap as a routine ownership
  check;
- before creating a candidate, check only for an equivalent current PR and explicit
  prerequisites;
- the affected lane owns its conflict resolution and affected re-proof/re-review;
- whole-flow lane agents execute `deliver-pr` and may orchestrate within the claim;
- leaf agents consume the named skill when one is supplied.

Do not add orchestration metadata, executor DAGs, lane reservations, candidate
frontiers, or persistent liveness state to skills or tracked files.

## Brief and return contract

A bounded brief should identify:

```text
parent flow/skill
exact durable subject and candidate identity
established facts and accepted authority
one question or mutation boundary
named provider-native skill when known
runtime authority: campaign root | lane root | read-only | writer | reviewer
falsifiers / negative controls
sufficient return and stable evidence references
NOT_PROVEN and stop/backward conditions
non-goals
```

Read-only returns should preserve direct and contradictory evidence, searched scope,
authority, what is/is not established, the affected claim/proof/authority edge,
recommended route, `NOT_PROVEN` boundary, and overflow references. Writers return
candidate identity, behavior/seams changed, proof run/not run, findings repaired,
limitations, current GitHub state, and the typed flow result.

The root joins graph deltas rather than votes. Repeated claims from one source are not
independent corroboration. Private reasoning and raw transcripts are never required.

## GitHub publication boundary

State which native GitHub surfaces the skill reads and may update, and which surfaces
must not be treated as authority.

Publish when information:

- changes claim, authority, accepted plan, proof obligation, route, prerequisite,
  support, risk, or rollback meaning;
- is reusable source-backed evidence another context would otherwise rediscover;
- is a localized review finding or supported disposition;
- records a real external wait and wake event;
- provides a useful cumulative review, merged effect, or goal synthesis.

Keep runtime-local:

- agent identity, topology, liveness, retry order, task state, and frontier;
- provisional reasoning that changes no durable decision;
- raw logs already available by stable reference;
- unchanged polling and routine route transitions.

Use issues for durable research/plan/ruling/dependency/goal synthesis, PR bodies or
comments for candidate-wide route/proof/limitation summaries, inline review for
localized findings, review replies for dispositions, submitted reviews for cumulative
judgment, and issue closeout for landed effect and residual work.

When route traceability is useful and not already obvious, one compact issue/PR route
declaration may state the parent goal, claim, entry flow, current named transition,
reason, durable subject, and wake event. Update it only when the material route changes.
It is not stage authority.

Labels may classify area, kind, risk, release, or requested attention. They must not
prove route, build, review, CI, response, or merge completion.

## Review-bearing provider contract

Both providers implement the full review flow directly:

```text
root router
→ provider-native orchestrate-work
→ provider-native finish-pr
→ provider-native review-pr
→ provider-native verify-live-ci
```

The provider-local skills must establish:

- `orchestrate-work` contains hierarchical scopes, PR review subgraphs, bounded briefs,
  one-writer protection, graph-delta joining, and useful GitHub publication filtering;
- `finish-pr` routes substantive candidates without useful current review through
  `final-challenge`, `orchestrate-work`, and `review-pr` before integration;
- `review-pr` reconstructs candidate/evidence, traces production reachability,
  challenges proof/evidence, verifies external/semantic truth, checks authority,
  complexity, risk, and rollback, and publishes findings or a useful clean conclusion;
- `verify-live-ci` reads integration facts only after `REVIEW_CURRENT`;
- accepted repair refreshes affected proof and review dimensions without exact-head
  ceremony;
- `deliver-goal` synthesizes related PRs only after each candidate has its own review;
- `deliver-pr` runs the selected named route and sends accepted mutation through one
  writer.

Substantive review results are:

```text
REVIEW_CURRENT
CHANGES_REQUIRED
NOT_PROVEN
BLOCKED_BY_PREREQUISITE
SUPERSEDED_OR_CLOSE
```

Live integration postures are:

```text
INTEGRATION_READY
PR_IN_FLIGHT
MERGE_BLOCKED
NOT_PROVEN
```

These are cumulative judgments and flow results, not labels, claim digests, exact-head
receipts, automatic approvals, or merge authorization independent of live policy.

## Structural validation

Maintenance-time validation may check:

- metadata and route targets;
- provider semantic coverage;
- no-proof, midstream, in-flight, repair, backward, and whole-flow-lane routes;
- campaign-root, lane-root, worker, reviewer, and candidate-writer wording;
- root-retained decisions and one mutation owner where mutation is applicable;
- delegable read-only work or an explicit reason the skill remains local;
- useful parallelism/communication boundary;
- join predicate and graph-delta/return packet;
- useful GitHub publication versus runtime-local state;
- external wait and wake-event semantics for wait-bearing flows;
- explicit establishment and nonclaim boundaries;
- provider roots directly name their operational flows;
- `orchestrate-work` runs the specified route and contains the provider-local review
  subgraph;
- `finish-pr` cannot bypass substantive review;
- `review-pr` routes current review to `verify-live-ci`;
- `verify-live-ci` cannot infer review from integration facts;
- substantive review and integration posture remain distinct;
- useful GitHub updates are distinct from runtime state;
- route trace is bounded and non-authoritative;
- absence of tracked frontier, liveness, agent registry, comment-per-transition, and
  retired orchestration metadata.

Validation should test semantic markers and route relations appropriate to each skill
class, not require identical headings or prose across Claude and Codex.

It must not inspect a live issue/PR stage, select agents, judge actual review sufficiency
from a phrase gate, infer neighbouring-lane overlap, authorize mutation, or run between
ordinary skill transitions.
