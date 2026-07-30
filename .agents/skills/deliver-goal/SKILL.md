---
name: deliver-goal
description: Use for an umbrella issue, release outcome, compiler or LSP campaign, or other durable multi-PR end state that should advance through distinct coherent claims.
---

# Deliver goal

## Purpose

Advance a durable outcome through normal issue and PR lanes. Do not create a repository scheduler, tracked current-goal pointer, overlap map, candidate tournament, or build-all-eligible wave.

## Focused reconstruction

Read:

- the selected umbrella or durable outcome;
- governing product, architecture, support, or release contracts;
- current-main behavior and evidence;
- directly linked unresolved claims, PRs, and explicit dependencies;
- recently merged claims that change the remaining goal boundary;
- genuine blockers and human decisions.

Search only far enough to avoid duplicating the same claim or missing an explicit prerequisite. Do not inspect sibling worktrees, touched-file overlap, or nearby implementation details merely because several lanes are active.

## Claim selection

Choose one coherent acceptance-and-rollback claim that is:

- still required by the outcome;
- currently actionable or unblockable now;
- distinct from the other active claims;
- not already represented by an equivalent current PR;
- proportionate and independently reviewable;
- governed by a current issue/plan or capable of entering `$prepare-issue` immediately.

One claim normally has one current candidate. Do not produce competing implementations of the same claim unless comparison itself is required to resolve a material uncertainty; select one candidate before normal publication.

## Lane contract

Each claim lane owns its own issue, branch/worktree, candidate, PR, proof, review repair, and integration cleanup.

Use ordinary issue or PR comments when another lane genuinely needs a fact, for example:

- an explicit prerequisite landed in a materially different shape;
- a governing contract or owner ruling changed;
- one claim superseded another;
- Git or combined-tree proof exposed a real interaction.

Do not maintain cross-lane reservations, overlap reports, liveness state, or implementation surveillance. Let each lane focus on its claim.

If another PR lands and the candidate remains mergeable and semantically valid, do nothing. If Git reports a conflict, an explicit stack changes, or integration proof exposes an interaction, the affected lane rebases or repairs its own candidate and reruns only the affected proof/review.

## Loop

```text
select one distinct coherent claim
→ $deliver-pr
→ if merged or closed, reconcile the umbrella/current-main truth
→ if GitHub owns the next transition, leave that PR in flight
→ select another distinct required claim when useful
→ continue until the goal is satisfied or every remaining claim shares one real blocker
```

A PR waiting on CI, review, or auto-merge is not a goal blocker. Unrelated movement on `main` does not require branch churn.

Selecting a next claim is an internal continuation, not a completion result. Invoke `$deliver-pr` immediately. An `IN_FLIGHT` result from one claim returns to this loop so another distinct claim may advance.

## Terminal results

- `GOAL_SATISFIED`
- `GOAL_PARTIAL` — only when the caller explicitly bounded the requested progress or the durable outcome was deliberately narrowed/superseded; name every residual claim
- `EXTERNAL_BLOCKER`
- `NOT_PROVEN`

Use `EXTERNAL_BLOCKER` only when every remaining required claim depends on the same unresolved external condition or material owner decision. Use `NOT_PROVEN` when the reliable goal boundary or live graph cannot be reconstructed; do not substitute a guessed queue.
