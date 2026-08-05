---
name: orchestrate-work
description: Choose the smallest useful runtime shape for executing a selected public flow or atomic skill, with bounded briefs, one-writer protection, and evidence joining.
user-invocable: false
---

# Orchestrate work

Use this internal root operation to choose how the selected flow or atomic skill should run. It is not a public flow, durable lifecycle stage, executor graph, repository scheduler, or source of durable state.

The root keeps goal meaning, claim selection, authority, contradiction resolution, joined evidence, review sufficiency, merge judgment, and current-main reconciliation. Execution may be delegated; accountability may not be.

## Authoritative inputs

Use current `origin/main`, the selected issue/PR and candidate identity, governing repository artifacts, relevant proof/review evidence, and live GitHub rules/checks. Runtime task lists, teammate identity, and prior transcripts are not authority.

## Runtime shape

Anchor goal, claim, controlling issue, current flow/skill, and candidate/PR identity. Check current issue/PR state, equivalent current PRs, and explicit prerequisites.

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

Delegate only when expected evidence gain, context preservation, tool/environment difference, elapsed-time gain, or recovery value exceeds cold-start, briefing, duplication, join, coordination, and correlated-failure costs. Stop adding agents when another result cannot change a decision.

## Assignment contract

Use a public flow, atomic skill, or bounded question:

```text
Take issue #123 through `deliver-pr`.
Run `review-tests` against the current proof.
Map every live production consumer of X.
```

Every brief names the parent flow/skill, exact target, established facts, authorities, candidate/PR identity and reviewed semantic seams where relevant, read/write boundary, permitted writer branch/worktree, sufficient result, falsifiers or negative controls, stop/backward routes, and non-goals.

Do not include a claim digest or request a `review-run` receipt. Review is semantic rather than exact-head.

Keep one writer per candidate. Read-only delegates return evidence, contradictions, uncertainty, what is and is not established, and a recommended disposition. Writers return candidate identity, changed files/behavior, proof, repaired findings, limitations, GitHub state, and the typed flow result.

## Recommended procedure

1. Anchor the durable subject and candidate identity.
2. Identify unresolved judgments and choose the smallest useful shape.
3. Send a complete brief with one-writer and stop conditions.
4. Steer, retry, replace, or cancel only while returned evidence can change the decision.
5. Join evidence, preserve contradictions, update the owned durable artifact, and return through the invoking flow.

## Review currentness

A later commit does not invalidate review merely because the SHA changed. Revisit only findings, proof, claims, production paths, authority, or risk dimensions materially changed by later work. Formatting, editorial cleanup, generated receipt refresh, or stronger tests do not trigger a full review by themselves.

## Durable boundary

Only the selected issue, plan/spec/policy, candidate, PR, review, check, merge, or closeout surface is durable. Briefs, retries, transcripts, topology, and executor state remain runtime-local.

## Actual stop conditions

Stop or return `NOT_PROVEN` for a same-candidate writer collision, unsafe worktree/destructive action, unestablished identity or authority, contradictory evidence requiring accountable judgment, or failed instrumentation. An unchanged remote wait is `IN_FLIGHT`, not a local blocker.

## Return

Return control to the invoking flow after joining evidence. Route changed authority/scope/claim through `prepare-issue`, weak proof through `prepare-proof`, candidate mutation through `build-candidate`, review through `review-pr`, and unchanged GitHub waits as `IN_FLIGHT`.
