---
name: orchestrate-work
description: Choose the smallest useful runtime shape for executing a selected public flow or atomic skill, with bounded briefs, one-writer protection, and evidence joining.
---

# Orchestrate work

Use this internal root operation to choose how the selected flow or atomic skill
should run. It is provider-native but semantically equivalent to Claude's
implementation. It is not a public flow, durable lifecycle stage, executor DAG,
repository scheduler, or source of durable state.

The root remains accountable for goal meaning, claim selection, accepted
authority, contradiction resolution, joined evidence, review sufficiency, merge
judgment, and current-main reconciliation.

## Authoritative inputs

Use current `origin/main`, the selected issue/PR and exact candidate identity,
governing repository artifacts, relevant proof/review evidence, and live GitHub
rules/checks. Runtime task lists, agent identity, and prior transcripts are not
authority.

## Runtime shape

Anchor `goal`, `claim`, controlling issue, current flow/skill, and candidate/PR
identity before assigning work. Check current issue/PR state, equivalent current
PRs, and explicit prerequisites. Comments carry material handoffs; they do not
create leases or reservations.

## Focused questions

Ask only questions that can change the selected flow's next decision: semantic
owner, production reachability, external authority, realistic wrong behavior,
candidate/proof mutation, or the review method required by a material finding.

Choose proportionally:

| Work | Normal shape |
| --- | --- |
| Goal interpretation or contradiction resolution | Root |
| Tiny tightly coupled edit | Root/current writer |
| High-output or bounded exploratory evidence | Focused read-only delegate |
| One coherent claim | Whole-flow `deliver-pr` lane |
| Distinct claims | Separate lane owners/worktrees |
| Candidate/proof mutation | One writer |
| Adversarial review | Differentiated oracle, source, or method when useful |
| Unchanged remote wait | No agent; `IN_FLIGHT` |

Select delegation only when expected evidence gain, context preservation,
parallel/tool/environment gain, or recovery value exceeds cold-start, briefing,
duplication, join, coordination, and correlated-failure costs. Tiny work remains
direct; substantive work is normally orchestrated; fan-out stops when another
result cannot change a decision.

## Assignment contract

Use one of these forms:

```text
Take issue #123 through `deliver-pr`.
Run `review-tests` against the current proof.
Map every live production consumer of X.
```

An unknown answer is delegable when the search boundary, authority, stop rule, and
output are bounded. Every brief names the parent flow/skill, exact target,
established facts, authorities, candidate head/claim digest when relevant,
read/write boundary, permitted writer branch/worktree, sufficient result,
falsifiers or negative controls, stop/backward routes, and non-goals.

Keep one writer per candidate. Read-only delegates return evidence, contradictions,
uncertainty, what is and is not established, and a recommended disposition. A
writer returns candidate identity, changed files/behavior, proof, repaired
findings, limitations, GitHub state, and the typed flow result.

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

## Steering and joining

The root may correct, narrow, widen, cancel, retry, replace, or stop executors as
the evidence changes. Join direct evidence rather than counting repeated answers:
compare sources, preserve contradictions, inspect shared premises, reject
unsupported confidence, and route through the invoking skill's next or backward
edge. Runtime transcripts are not durable evidence.

## Skill map

Whole lane → `deliver-pr` or `deliver-goal`.

One transformation → `prepare-issue`, `research-issue`, `review-issue`,
`issue-to-plan`, `research-plan`, `review-plan`, `compile-spec`, `prepare-proof`,
`spec-to-test`, `review-tests`, `build-candidate`, `build-from-proof`,
`improve-test-suite`, `simplify-candidate`, `review-candidate`, `publish-pr`,
`address-review-comments`, `final-challenge`, `review-pr`, `verify-live-ci`, or
`merge-reconcile`, according to the desired result.

This is a routing aid, not a lifecycle compiler. It must not encode a provider,
model, agent count, team topology, live issue/PR/stage, reservation, heartbeat,
or durable executor state.

## Durable boundary

Only the selected issue, plan/spec/policy, candidate, PR, review, check, merge,
or closeout surface is durable. Briefs, retries, transcripts, topology, and
executor state remain runtime-local.

## Actual stop conditions

Stop or return `NOT_PROVEN` for a same-candidate writer collision, unsafe
worktree/destructive action, unestablished identity or authority, contradictory
evidence requiring accountable judgment, or failed instrumentation. An unchanged
remote wait is `IN_FLIGHT`, not a local blocker.

## Return

Return control to the invoking flow after joining evidence. Route changed
authority/scope/claim through `prepare-issue`, weak proof through `prepare-proof`,
candidate mutation through `build-candidate`, fixed formal judgment through
`review-pr`, and unchanged GitHub waits as `IN_FLIGHT`. Preserve an exact
`NOT_PROVEN` boundary when identity, authority, or evidence cannot be established.
