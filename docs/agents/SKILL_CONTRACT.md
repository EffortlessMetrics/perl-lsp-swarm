# Skill contract

Skills are small, self-navigating artifact transformations. Public flows group related
SDLC concerns; atomic skills supply just-in-time methods. They make the next useful
judgment and route clear without turning the repository into a runtime workflow engine,
tracked frontier, or executor hierarchy.

## Provider-native operational authority

Shared docs define method and structural expectations. They are not substitutes for the
instructions a running provider consumes.

```text
Claude Code operational skills
→ .claude/skills/*

Codex operational skills
→ .agents/skills/*
```

A substantive skill must be operationally complete in both provider implementations.
Provider mechanics may differ; the semantic transformation and route boundary must
remain aligned.

## Root orchestration and scope-relative execution

The main/root provider thread is the accountable orchestrator. It owns:

```text
goal meaning and acceptance predicates
logical claim frames and claim selection
cross-claim dependencies and contradictions
current route / backward route per claim
joined evidence and review sufficiency
candidate writer allocation
finding disposition and integration judgment
remote waits / wake events
reconciliation and continuation
```

A claim/lane is a **logical root-held frame**, not normally an agent identity. The root
may focus on one claim through `deliver-pr`, switch to another when the first is
`IN_FLIGHT`, and reconstruct frames from GitHub/repository state after replacement.

Bounded execution contexts consume work from the root:

```text
researcher / read-only worker
→ bounded source, external-truth, CI/log, archaeology, or inventory programme

builder / writer
→ one candidate/proof mutation programme and one worktree

reviewer
→ one fixed subject and differentiated review programme

root direct
→ tiny/tightly-coupled judgment where delegation changes no evidence surface
```

A provider may use context forks, built-in workers, nested agents, dynamic workflows, or
Teams when a specific task benefits. Those are physical execution choices, not logical
claim-orchestration authority. No skill may require recursive orchestration, a
subordinate orchestrator role, or a particular physical topology.

## Required skill shape

Every public flow and substantive atomic skill must expose the applicable semantics
below. This is an affordance vocabulary for the root and maintenance-time validators,
not a requirement for identical headings.

```text
Purpose
Use when / do not use when
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
What this establishes / does not establish
Valid exits and routes
Actual stop conditions
```

A skill may combine fields under provider-native headings. It must still make the
root-retained decision, mutation owner, delegation surface, join/return boundary,
durable/runtime split, and establishment/nonclaim boundary clear.

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

Public flows are natural root entrypoints. Atomic skills may execute directly in the
root or inside bounded researcher, writer, or reviewer programmes. Adding, renaming, or
removing a skill is a control-plane change and must update both provider
implementations and route validation.

The internal `orchestrate-work` operation is root-facing runtime compilation guidance,
not a public stage. It decides what stays direct and what becomes a bounded programme,
while the invoking public flow retains route ownership.

It must not encode a subordinate orchestrator requirement, durable executor DAG,
tracked frontier, lease, reservation, liveness state, provider, model, agent count,
team topology, or comment-per-transition protocol.

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
  → return to deliver-goal so another distinct claim frame may proceed;
    a deliver-pr invoked standalone returns PR_IN_FLIGHT to its caller,
    since an external system may own the next transition

ALREADY_SATISFIED
  → return to deliver-pr for reconciliation

NOT_APPLICABLE
  → return to the public flow and select the next applicable skill

BLOCKED / NOT_PROVEN
  → name the exact dependency, authority, instrument, or evidence gap
```

Selecting a route is not completion. The root invokes the selected skill and follows its
named next or material backward edge. Do not mix stage identifiers, agent identities,
label names, guessed command names, and callable skills in one exit vocabulary.

## Applicability

The normal path runs every applicable pass. A pass is not applicable only when:

- its subject genuinely does not exist;
- current evidence already establishes the same judgment;
- the change is proportionally mechanical and has no corresponding decision;
- the flow entered after that judgment and replay would add no value.

A missed earlier pass causes forward repair, not retrospective punishment.

## Programme rule

A bounded agent may execute several ordered atomic skills when one context remains
useful:

```text
one context
+ one artifact set
+ one bounded purpose
+ several ordered skills
→ one return packet
```

Do not fork per skill merely because the skill name changed. Fork when a changed source,
oracle, threat model, tool/sandbox, independence property, or context boundary makes a
new execution context useful.

Typical programmes:

- researcher: source ownership → external truth → issue/currentness evidence;
- builder: proof → implementation → hardening → simplification → affected repair;
- reviewer: issue/plan/proof/candidate lenses over one fixed subject.

The root selects the programme and owns the join. Skills own the procedure.

## Candidate and claim rule

A substantive skill operates on one selected root-held claim frame and its current
candidate.

- one claim normally has one current candidate;
- one writer mutates that candidate branch/worktree at a time;
- focused research, oracle, proof, review, and CI evidence work may assist;
- helpers do not inspect sibling claim implementation details or touched-file overlap as
  routine ownership checks;
- before creating a candidate, check for an equivalent current PR and explicit
  prerequisites;
- the affected claim owns its conflict resolution and affected re-proof/re-review;
- leaf agents consume the named skill when one is supplied;
- a substantial claim does not require another orchestrator identity.

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
execution authority: read-only | writer | reviewer
falsifiers / negative controls
sufficient return and stable evidence references
NOT_PROVEN and stop/backward conditions
non-goals
```

A brief does not grant generic claim-orchestration authority. An experimental whole-flow
worker may be bounded to a claim and route, but the root retains selection, joining,
disposition, and continuation.

Read-only returns preserve direct and contradictory evidence, searched scope, authority,
what is/is not established, the affected claim/proof/authority edge, recommended route,
`NOT_PROVEN` boundary, and overflow references. Writers return candidate identity,
behavior/seams changed, proof run/not run, findings repaired, limitations, current
GitHub state, and the typed result.

The root joins graph deltas rather than votes. Repeated claims from one source are not
independent corroboration. Private reasoning and raw transcripts are never required.

## GitHub publication boundary

Publish when information:

- changes claim, authority, accepted plan, proof obligation, route, prerequisite,
  support, risk, or rollback meaning;
- is reusable source-backed evidence another context would otherwise rediscover;
- is a localized review finding or supported disposition;
- records a real external wait and wake event;
- provides a useful cumulative review, merged effect, or goal synthesis.

Keep runtime-local:

- root claim-frame ordering and current attention;
- agent identity, topology, liveness, retry order, and task state;
- provisional reasoning that changes no durable decision;
- raw logs already available by stable reference;
- unchanged polling and routine route transitions.

Use issues for durable research/plan/ruling/dependency/goal synthesis, PR bodies or
comments for candidate-wide route/proof/limitation summaries, inline review for
localized findings, review replies for dispositions, submitted reviews for cumulative
judgment, and issue closeout for landed effect and residual work.

Labels may classify area, kind, risk, release, or requested attention. They must not
prove route, build, review, CI, response, or merge completion.

## Review-bearing provider contract

Both providers implement the full review flow directly from the root:

```text
root router
→ provider-native orchestrate-work
→ provider-native finish-pr
→ provider-native review-pr
→ provider-native verify-live-ci
```

The provider-local skills must establish:

- `orchestrate-work` selects a flat root-to-programme execution shape with one-writer
  protection, bounded briefs, graph-delta joining, and useful GitHub publication
  filtering;
- `finish-pr` routes substantive candidates without useful current review through
  `final-challenge`, `orchestrate-work`, and `review-pr` before integration;
- `review-pr` reconstructs candidate/evidence, traces production reachability,
  challenges proof/evidence, verifies external/semantic truth, checks authority,
  complexity, risk, and rollback, and publishes findings or a useful clean conclusion;
- `verify-live-ci` reads integration facts only after `REVIEW_CURRENT`;
- accepted repair refreshes affected proof and review dimensions without exact-head
  ceremony;
- `deliver-goal` synthesizes related PRs only after each candidate has its own review;
- `deliver-pr` runs one root-held claim frame and sends accepted mutation through one
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

These are cumulative judgments and flow results, not labels, exact-head receipts,
automatic approvals, or merge authorization independent of live policy.

## Structural validation

Maintenance-time validation may check:

- metadata and route targets;
- provider semantic coverage;
- no-proof, midstream, in-flight, repair, and backward routes;
- root-held claim-frame wording and absence of required subordinate orchestration;
- root-retained decisions and one mutation owner where mutation is applicable;
- delegable read-only work or an explicit reason the skill remains local;
- programme continuity versus useful context separation;
- useful parallelism/communication boundary;
- join predicate and graph-delta/return packet;
- useful GitHub publication versus runtime-local state;
- external wait and wake-event semantics for wait-bearing flows;
- explicit establishment and nonclaim boundaries;
- provider roots directly name their operational flows;
- `orchestrate-work` runs the specified route without requiring a nested orchestrator;
- `finish-pr` cannot bypass substantive review;
- `review-pr` routes current review to `verify-live-ci`;
- `verify-live-ci` cannot infer review from integration facts;
- substantive review and integration posture remain distinct;
- absence of tracked frontier, liveness, agent registry, comment-per-transition, and
  retired orchestration metadata.

Validation should test semantic markers and route relations appropriate to each skill
class, not require identical headings or prose across Claude and Codex.

It must not inspect a live issue/PR stage, select agents, judge actual review sufficiency
from a phrase gate, infer neighbouring-claim overlap, authorize mutation, or run between
ordinary skill transitions.
