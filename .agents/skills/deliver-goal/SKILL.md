---
name: deliver-goal
description: Advance an umbrella issue, release outcome, compiler/LSP campaign, or durable multi-PR end state through distinct coherent claims and Codex-native PR review.
---

# Deliver goal

Read the verbatim goal source when available; the current interpretation, constraints,
non-goals, and acceptance predicates; the selected umbrella or durable outcome;
governing contracts; current main and evidence; directly linked unresolved claims and
PRs; explicit dependencies; recently merged claims; known limitations and
`NOT_PROVEN` predicates; and real blockers.

For a durable campaign, keep one current synthesis on the umbrella issue. For a
session-sized goal, runtime context is sufficient. Do not scan or score the whole
backlog, inspect sibling worktrees for overlap, or mutate a tracked active-goal
pointer.

Choose one distinct coherent acceptance-and-rollback claim that remains required,
actionable, not already represented by an equivalent current PR, and independently
reviewable. One claim normally has one candidate and one integrating writer.

Each lane owns its issue, candidate branch/worktree, proof, provider-native
substantive review, review repair, integration cleanup, and merge conflict. Use a
direct issue or PR comment only when another lane genuinely needs a material fact.

## Bounded related-PR review orchestration

When the selected goal directly links a bounded set of PRs whose contracts, authority,
or merge order interact, the Codex root reviews the train through the native flow
rather than one shared standard or batch-status judgment.

For each PR:

```text
`$deliver-pr`
→ `$finish-pr`
→ `$orchestrate-work` for applicable adversarial lenses
→ `$review-pr`
→ REVIEW_CURRENT | CHANGES_REQUIRED | NOT_PROVEN |
  BLOCKED_BY_PREREQUISITE | SUPERSEDED_OR_CLOSE
→ when REVIEW_CURRENT, `$verify-live-ci`
→ INTEGRATION_READY | PR_IN_FLIGHT | MERGE_BLOCKED | NOT_PROVEN
```

Each PR keeps its own submitted review and claim boundary. The goal root may then
synthesize:

| PR | Candidate identity | Hosted/current checks | Substantive review result | Integration posture | Explicit prerequisite |
| --- | --- | --- | --- | --- | --- |

Verify cross-PR contracts that an individual diff cannot settle alone:

- parent/child schema and validator agreement;
- complete candidate and artifact-set identity;
- semantic owner and authority boundaries;
- status and limitation propagation;
- whether a fan-in loads and validates child evidence rather than accepting copied
  summaries;
- whether `NOT_PROVEN` child state remains visible;
- actual stack, prerequisite, repair, and merge order.

A green parent or aggregate cannot outrun untrustworthy child contracts. The synthesis
is goal-level judgment—not batch approval, a substitute for each PR's `$review-pr`, or
merge authorization independent of live policy. Do not repeat still-current reviews
merely to populate the table.

Use `$orchestrate-work` to fan out bounded read-only contract checks when useful. The
root joins the evidence, resolves contradictions, and names the repair and merge
order.

## Loop and routes

```text
select one distinct claim
→ `$deliver-pr`
→ if the PR reaches a GitHub-owned wait, retain it as IN_FLIGHT
→ advance another distinct actionable claim
→ reconcile merged or deliberately closed claims
→ when bounded PR contracts interact, run the related-PR review orchestration
→ re-evaluate the original goal's acceptance predicates
→ continue until the predicates pass or every remaining claim shares one real blocker
```

If another PR lands and a candidate remains valid, do nothing. If an actual conflict,
explicit stack change, or combined-tree failure appears, the affected lane repairs its
own candidate and refreshes only affected proof and review. Behind-only movement does
not require churn.

Selecting a claim is not a completion result: invoke `$deliver-pr` immediately. An
`IN_FLIGHT` result returns to this loop so another distinct claim may advance.

## Goal completion contract

Return `GOAL_SATISFIED` only when every acceptance predicate is `PASS` or explicitly
`NOT_APPLICABLE`, with current evidence and retained limitations named. An exhausted
issue list, no newly found work, or several merged PRs is not sufficient.

Use `GOAL_PARTIAL` only when the caller bounded progress or the durable outcome was
deliberately narrowed or superseded. Preserve failed, limited, and `NOT_PROVEN`
predicates rather than rewriting the goal around what landed.

## What this establishes

Bounded advancement of one durable outcome through distinct coherent claims, with
provider-native per-PR review, current integration posture, cross-PR contract and
merge-order synthesis where needed, and every residual required claim stated.

## What this does not establish

A repository scheduler, tracked active-goal pointer, portfolio queue, build-all wave,
overlap ledger, batch review approval, or merge authorization independent of each
candidate's review and live ruleset.

## Terminal results

Return `GOAL_SATISFIED`, `GOAL_PARTIAL`, `EXTERNAL_BLOCKER`, or `NOT_PROVEN`.

Use `EXTERNAL_BLOCKER` only when every remaining required claim shares one unresolved
external condition or material owner decision. Use `NOT_PROVEN` when the reliable goal
boundary or live graph cannot be reconstructed.