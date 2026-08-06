---
name: orchestrate-work
description: Choose Claude Code's smallest useful runtime shape for a selected flow, including bounded PR-review subgraphs, one-writer protection, and evidence joining.
user-invocable: false
---

# Orchestrate work

Use this internal Claude root operation to choose how the selected public flow or
atomic skill should run. It is not a public lifecycle stage, durable executor graph,
repository scheduler, or source of transaction state.

The main Claude thread keeps goal meaning, claim selection, authority, contradiction
resolution, joined evidence, review sufficiency, merge judgment, and current-main
reconciliation. Execution may be delegated; accountability may not be.

## Authoritative inputs

Use current `origin/main`, the selected issue/PR and candidate identity, governing
repository artifacts, relevant proof and review evidence, and live GitHub state.
Runtime task lists, subagent identity, Teams state, worktrees, and prior transcripts
are not authority.

## Runtime shape

Anchor the goal, claim, controlling issue, current flow/skill, and candidate or PR
identity before dispatching work.

Choose proportionally:

| Work | Normal Claude Code shape |
| --- | --- |
| Goal interpretation, claim selection, contradiction resolution | Main thread |
| Tiny tightly coupled edit | Main thread or current writer |
| High-output or bounded exploratory evidence | Focused read-only subagent |
| One coherent claim | Whole-flow `deliver-pr` lane |
| Distinct claims | Separate lane owners and worktrees |
| Candidate/proof mutation | One integrating writer |
| Substantive PR review | Root-directed differentiated review subgraph |
| Unchanged remote wait | No agent; return `IN_FLIGHT` |

Substantive work is normally orchestrated, but maximal fan-out is not sophistication.
Delegate when a different source, oracle, environment, threat model, tool, or attention
surface; evidence compression; root-context preservation; elapsed-time gain; or
recovery value exceeds cold-start, briefing, duplication, join, and correlated-failure
costs. Stop adding agents when another result cannot change a decision.

## Assignment contract

Use a public flow, atomic skill, or bounded question:

```text
Take issue #123 through `deliver-pr`.
Run `review-tests` against the current proof and return only evidence and falsifiers.
Trace the live caller path from textDocument/rename to the changed semantic owner.
```

Every brief names:

- parent flow or skill;
- exact issue, PR, candidate, and branch/worktree identity;
- established facts and accepted authority;
- one bounded question or mutation boundary;
- the provider-native skill the child should consume;
- read-only versus writer status;
- realistic falsifiers or negative controls;
- sufficient output and evidence references;
- uncertainty and `NOT_PROVEN` conditions to preserve;
- stop/backward routes and non-goals.

Do not ask a child to rediscover facts already established. Do not include a claim
digest or request a review-run receipt.

Keep one writer per candidate. Read-only subagents return evidence, contradictions,
uncertainty, what is and is not established, and recommended findings. Writers return
candidate identity, changed behavior, proof, repaired findings, limitations, and the
typed flow result.

## PR review orchestration

When invoked from `finish-pr` or `review-pr`, build a bounded review subgraph around
the actual claim and risk—not a fixed reviewer roster.

```text
main Claude thread
├── `review-tests` when proof discrimination or evidence integrity is material
├── `review-candidate` when implementation, ownership, reachability, complexity,
│   compatibility, risk, or rollback is material
├── bounded production-path trace when component proof may not reach the live system
├── bounded external oracle when language/protocol/platform/release truth matters
└── focused security/package/migration/persistence/support lens when applicable

joined evidence
→ main thread verifies load-bearing seams and contradictions
→ one writer repairs accepted findings through `address-review-comments`
→ main thread performs and publishes cumulative `review-pr`
→ only `REVIEW_CURRENT` enters `verify-live-ci`
```

Do not use a subagent verdict as approval. Do not count votes. Do not let the writer's
construction context be the only detection surface supporting a substantive merge.
Different identity without a different source, oracle, method, threat model, or
attention surface is not meaningful independence.

Example focused briefs:

```text
Consume `review-tests` for PR #123. Read only. Determine whether the current tests
fail against the historical defect for the intended reason, whether the negative and
stale directions are represented, and whether the schema and executable validator
accept the same documents. Return findings with file/line evidence and name anything
NOT_PROVEN. Do not edit or post a GitHub review.
```

```text
Consume `review-candidate` for PR #123. Read only. Trace the real production caller to
the changed code, identify the semantic owner and duplicate-authority risk, and test
the PR claim against its rollback boundary. Return only evidence-backed findings or a
clean conclusion with residual risk. Do not edit or authorize merge.
```

The root must join evidence from these returns with direct inspection of the cumulative
PR and publish one useful GitHub review. If findings require mutation, hand them to one
writer; after repair, rerun affected proof and only the review dimensions changed by
the repair.

## Recommended procedure

1. Anchor the durable subject and candidate identity.
2. Identify unresolved judgments and choose the smallest useful execution shape.
3. Send complete briefs with provider-native skills, one-writer status, falsifiers,
   and stop conditions.
4. Steer, retry, replace, or cancel only while returned evidence can change the
   decision.
5. Join direct evidence, preserve contradictions, reject unsupported confidence, and
   update the owned durable artifact.
6. Return through the invoking flow with the exact result or `NOT_PROVEN` boundary.

## Review currentness

A later commit does not invalidate review merely because the SHA changed. Revisit only
findings, proof, claims, production paths, authority, compatibility, risk, rollback, or
integration dimensions materially changed by later work. Formatting, editorial
cleanup, generated receipt refresh, and stronger tests do not trigger a full review by
themselves.

## Durable boundary

Only the selected issue, plan/spec/policy, candidate, PR, submitted review, check,
merge, or closeout surface is durable. Briefs, retries, transcripts, topology, and
executor state remain runtime-local.

## Actual stop conditions

Stop or return `NOT_PROVEN` for a same-candidate writer collision, unsafe destructive
action, unestablished identity or authority, contradictory evidence requiring an
accountable product decision, or failed instrumentation. An unchanged remote wait is
`IN_FLIGHT`, not a local blocker.

## Routes

- whole claim lane → `deliver-pr`
- durable multi-PR outcome → `deliver-goal`
- proof challenge → `review-tests` or `prepare-proof`
- candidate challenge → `review-candidate`
- cumulative PR judgment → `review-pr`
- accepted finding repair → `address-review-comments` with one writer
- current substantive review → `verify-live-ci`
- changed authority, scope, or claim → `prepare-issue`
- unchanged GitHub wait → return `IN_FLIGHT`
- missing reliable identity, authority, or evidence → return `NOT_PROVEN`