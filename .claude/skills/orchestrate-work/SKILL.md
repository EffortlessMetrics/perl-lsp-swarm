---
name: orchestrate-work
description: Choose the smallest useful runtime shape for executing a selected public flow or atomic skill, with bounded briefs, one-writer protection, and evidence joining.
user-invocable: false
---

# Orchestrate work

## Purpose

Convert the current public flow or atomic skill into the smallest useful runtime
shape. The root keeps goal meaning, claim selection, accepted authority,
contradiction resolution, joined evidence, review sufficiency, merge judgment,
and current-main reconciliation. Execution may be delegated; accountability may
not be.

This is an internal root operation, not a seventh public flow, durable lifecycle
stage, executor graph, or repository state machine. Its outputs are runtime-local.

## Use when

- a substantive flow has several unresolved judgments;
- raw evidence volume is much larger than the decision-relevant result;
- independent read-only questions can proceed concurrently;
- a whole coherent claim should be carried by one lane owner;
- a different source, oracle, tool, environment, or review method can change the
  evidence;
- the root's campaign context should be protected from disposable output.

## Do not use when

- the work is tiny and tightly coupled;
- briefing and joining cost more than direct execution;
- the root is resolving a material product decision or contradiction;
- the purpose is merely to demonstrate subagent use;
- the flow is waiting on unchanged GitHub state.

## Authoritative inputs

Use current `origin/main`, the selected issue/PR and its exact candidate state,
governing repository artifacts, relevant proof and review evidence, and the live
GitHub rules/checks applicable to the claim. Runtime task lists, agent identity,
and prior transcripts are not authority.

## Anchor the durable subject

Resolve before assigning work:

```text
goal
claim
controlling issue
current public flow
current atomic skill, if any
candidate / PR and exact head, if one exists
```

Begin from the selected work, not from available agent names. Before creating a
candidate, check current issue/PR state, equivalent current PRs, and explicit
prerequisites. A comment can carry a material handoff; it is not a lease,
reservation, or proof that a claim remains current.

## Focused questions

Ask only questions that can change the selected flow's next decision:

```text
Which source owns this fact?
What production path reaches it?
What external authority or oracle is required?
What realistic wrong implementation should fail?
What candidate or proof needs mutation?
What finding requires a changed method or backward route?
```

## Select the work shape

First identify the unresolved judgments, then choose proportionally:

| Work class | Normal executor |
| --- | --- |
| Goal interpretation, claim selection, contradiction resolution | Root |
| Small tightly coupled edit | Root or current writer |
| High-output evidence gathering | Focused read-only agent |
| Bounded repository or external exploration | Focused read-only agent |
| Proof mutation | One proof/candidate writer |
| Candidate implementation | One candidate writer |
| Adversarial test or candidate challenge | Focused reviewer/oracle or changed method |
| Complete coherent claim under a broader campaign | One whole-flow `deliver-pr` lane |
| Distinct independent claims | Separate lane owners and worktrees |
| Unchanged remote wait | No agent; return `IN_FLIGHT` |

Delegation economics are an engineering judgment:

```text
expected evidence gain
+ root-context preservation
+ elapsed-time or tool/environment gain
+ recovery or replacement value
- cold-start and briefing cost
- duplicated research and join cost
- coordination and correlated-failure risk
```

Stop adding agents when the next result would not change a decision. Do not
encode a provider, model, agent count, team topology, or required identity.

## Recommended procedure

1. Anchor the durable subject and exact candidate identity.
2. Identify unresolved judgments and classify the work shape.
3. Select direct execution, a whole-flow lane, an atomic-skill assignment, or a
   bounded question using delegation economics.
4. Send a complete brief with one-writer and stop conditions.
5. Steer, retry, replace, or cancel only while returned evidence can change the
   decision.
6. Join evidence, preserve contradictions, update the owned durable artifact,
   and return through the invoking flow.

## Assignment forms

### Whole-flow assignment

Use a public flow when one delegate can own a coherent lane:

```text
Take issue #123 through `deliver-pr`.
Use GitHub as durable state and return the typed lane result.
```

The lane still has one candidate and one mutating writer. It follows the flow's
normal and material backward routes.

### Atomic-skill assignment

Use one named transformation for a bounded judgment:

```text
Run `review-tests` against the current proof for issue #123.
Return observed execution, falsifiers, limitations, and the proof disposition.
```

### Bounded question

Use a focused question when the result supplies evidence to the current skill but
is not itself a complete transformation:

```text
Map every live production consumer of WorkspaceRootAuthority.
```

An unknown conclusion is delegable. An unbounded search space, authority, stop
rule, or output contract is not ready to delegate. Exploratory work is valid when
its search boundary is explicit:

```text
Search the named crates, package guidance, and linked issue/PR. Return candidate
owners, direct source locations, contradictions, unaccounted routes, and the
evidence needed to close each gap. Stop when every live route is accounted for or
a named evidence gap prevents closure.
```

## Delegation brief

Every brief carries:

```text
parent flow or skill
target and exact identity
question or desired transformation
established facts not to rediscover
authoritative inputs
candidate head / claim digest where relevant
read-only or write authority
writer branch/worktree where mutation is allowed
sufficient returned result
falsifiers or negative controls
stop and backward-route conditions
explicit non-goals
```

Do not ask a read-only delegate to mutate the candidate. Do not allow two writers
to touch the same candidate branch or worktree. A whole-flow writer returns its
candidate identity and proof; a read-only delegate returns evidence rather than a
verdict detached from evidence.

## Steer and join

The root may correct a misdirected question, provide newly established facts,
narrow or widen a bounded search, cancel obsolete work, retry or replace a failed
executor, change a read-only investigation to no further action, or stop further
delegation when the decision is already determined. Steering never transfers root
accountability or creates a durable topology.

Join evidence, not votes:

1. compare direct evidence and preserve contradictions;
2. inspect load-bearing sources and shared premises;
3. reject unsupported confidence and repeated assumptions;
4. decide what changes the durable artifact;
5. route through the invoking skill's next or material backward edge.

A read-only return packet contains:

```text
Conclusion
Direct evidence
Contradictory evidence
Uncertainty
What this establishes
What this does not establish
Recommended disposition or next skill
```

A writer returns candidate identity, changed behavior/files, proof executed,
findings repaired, known limitations, current GitHub state, and the typed flow
result. Raw transcripts are not the normal join surface.

## Skill-selection map

Whole lane → `deliver-pr` or `deliver-goal`.

One artifact transformation → the named atomic skill:

| Desired result | Skill |
| --- | --- |
| Settle problem, owner, scope, or plan | `prepare-issue`, `research-issue`, `review-issue`, `issue-to-plan`, `research-plan`, `review-plan` |
| Create a durable cross-PR contract | `compile-spec` |
| Establish or challenge proof | `prepare-proof`, `spec-to-test`, `review-tests` |
| Build, harden, simplify, or challenge a candidate | `build-candidate`, `build-from-proof`, `improve-test-suite`, `simplify-candidate`, `review-candidate` |
| Publish, repair, formally review, or challenge a PR | `publish-pr`, `address-review-comments`, `final-challenge`, `review-pr` |
| Verify live integration and reconcile | `verify-live-ci`, `merge-reconcile` |

The map is a routing aid, not a lifecycle compiler. References must remain real
provider-native skills and the invoking public flow remains responsible for the
next transition.

## GitHub and repository boundary

Read the selected issue/PR, governing repository artifacts, exact candidate
identity, proof, reviews, threads, checks, and current mergeability as relevant.
Update only the durable surface owned by the invoking flow or selected writer.
Use direct issue/PR comments for material prerequisites, rulings, supersession,
or actual integration findings. Do not create an issue-comment lease, file
reservation, executor database, heartbeat, topology receipt, or active-agent
registry.

Durable updates are the selected issue, plan/spec/policy, candidate, PR, review,
check, merge, or closeout surface. Briefs, retries, transcripts, topology, and
executor state remain runtime-local.

## What this establishes

A proportional, bounded runtime assignment and a joined evidence packet for the
selected flow or claim, with one writer per current candidate and an explicit
return route.

## What this does not establish

It does not establish that a delegate's conclusion is correct without evidence,
that review is sufficient, that a PR is mergeable, or that a broader goal is
complete. It does not create durable orchestration authority.

## Routes

- complete whole-flow or atomic assignment → return to the invoking flow;
- evidence changes authority, scope, proof, or claim → use that flow's backward
  route, normally `prepare-issue` or `prepare-proof`;
- candidate mutation → keep one writer and return through `build-candidate`;
- formal review subject → `review-pr` owns fixed-candidate currentness;
- unchanged external wait → return `IN_FLIGHT` to `deliver-pr` or `deliver-goal`;
- unavailable, contradictory, or insufficient evidence → return the exact
  `NOT_PROVEN` boundary.

## Actual stop conditions

Stop or return `NOT_PROVEN` only for a same-candidate writer collision, unsafe
worktree or destructive action, unestablished identity/authority, contradictory
evidence that needs accountable judgment, or an instrument failure that prevents
an honest result. A remote-owned wait is `IN_FLIGHT`, not a local blocker.
