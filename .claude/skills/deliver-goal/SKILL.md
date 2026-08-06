---
name: deliver-goal
description: Advance an umbrella issue, release outcome, compiler/LSP campaign, or durable multi-PR end state through distinct coherent claims.
argument-hint: "[umbrella issue or durable outcome]"
---

# Deliver goal

Read the verbatim goal source when available; the current interpretation,
constraints, non-goals, and acceptance predicates; the selected umbrella or durable
outcome; governing contracts; current main and evidence; directly linked unresolved
claims and PRs; explicit dependencies; recently merged claims; known limitations and
`NOT_PROVEN` predicates; and real blockers.

For a durable campaign, keep one current synthesis on the umbrella issue. For a
session-sized goal, runtime context is sufficient. Do not scan or score the whole
backlog, inspect sibling worktrees for overlap, or mutate a tracked active-goal
pointer.

Choose one distinct coherent acceptance-and-rollback claim that is still required,
actionable, not already represented by an equivalent current PR, and independently
reviewable. One claim normally has one current candidate; do not manufacture
competing implementations of the same claim.

Each lane owns its own issue, branch/worktree, PR, proof, substantive review, review
repair, and merge conflicts. Let it focus on the claim. Use a direct issue or PR
comment only when another lane genuinely needs a fact: a prerequisite changed, a
governing ruling changed, a claim was superseded, or Git/integration proof exposed a
real interaction.

## Bounded related-PR review

When the selected goal directly links a bounded PR set whose contracts or merge order
interact, apply
[`docs/agents/PR_REVIEW_STANDARD.md`](../../../docs/agents/PR_REVIEW_STANDARD.md).
Do not scan unrelated open PRs or create a portfolio queue.

Each PR must first receive its own substantive `review-pr` conclusion. The goal root
may then synthesize:

| PR | Candidate identity | Hosted/current checks | Substantive review result | Integration posture | Explicit prerequisite |
| --- | --- | --- | --- | --- | --- |

Verify schema, identity, authority, status, limitation propagation, artifact-set, and
fan-in contracts across the bounded set. A parent or aggregate cannot outrun
untrustworthy child evidence merely because its own synthetic fixture is green. Name
the correct repair and merge order.

The synthesis is goal-level judgment, not batch approval, a substitute for per-PR
review, or authorization independent of each PR's live ruleset and evidence. Do not
repeat still-current PR reviews merely to populate the table.

## Routes

```text
select one distinct claim
→ `deliver-pr`
→ reconcile after merge or deliberate closure
→ re-evaluate the original goal's acceptance predicates
→ while a PR waits on CI/review/auto-merge, advance another distinct claim
→ when related PR contracts interact, synthesize each substantive review result,
  current integration posture, and dependency order without replacing individual reviews
→ continue until the predicates pass or every remaining claim shares one real blocker
```

If another PR lands and this candidate remains valid, do nothing. If an actual
conflict or integration failure appears, the affected lane rebases or repairs its own
candidate and refreshes only affected proof/review. Behind-only movement does not
require churn.

Selecting a claim is not a completion result: invoke `deliver-pr` immediately. When
one claim returns `IN_FLIGHT`, resume this loop and advance another distinct
actionable claim.

## Goal completion contract

Return `GOAL_SATISFIED` only when every acceptance predicate is `PASS` or explicitly
`NOT_APPLICABLE`, with current evidence and retained limitations named. An exhausted
issue list, no newly found work, or several merged PRs is not sufficient by itself.

Use `GOAL_PARTIAL` only when the caller bounded the requested progress or the durable
outcome was deliberately narrowed or superseded. Preserve failed, limited, and
`NOT_PROVEN` predicates rather than rewriting the goal around what happened to land.

## What this establishes

Bounded advancement of one selected durable outcome through distinct coherent claims,
with the goal source, current interpretation, acceptance state, merged results,
in-flight work, related-PR review/integration/dependency synthesis where applicable,
and every residual required claim stated when the loop terminates.

## What this does not establish

A repository scheduler, tracked active-goal pointer, portfolio queue,
build-all-eligible wave, overlap ledger, batch review approval, or merge authorization
independent of each candidate's live ruleset and evidence.

## Terminal results

Return `GOAL_SATISFIED`, `GOAL_PARTIAL`, `EXTERNAL_BLOCKER`, or `NOT_PROVEN`.

Use `EXTERNAL_BLOCKER` only when every remaining required claim shares the same
unresolved dependency or material owner decision. Use `NOT_PROVEN` when the reliable
goal boundary or live graph cannot be reconstructed.
