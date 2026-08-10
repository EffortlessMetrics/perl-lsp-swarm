---
name: deliver-goal
description: Operate a durable multi-PR outcome through campaign-root orchestration, claim-local deliver-pr lanes, useful GitHub summaries, and current acceptance evidence.
---

# Deliver goal

This is a campaign-root flow. Preserve the verbatim goal source when available; the
current interpretation, constraints, non-goals, and acceptance predicates; governing
contracts; current main and evidence; directly linked claims and PRs; explicit
dependencies; merged effects; known limitations; and `NOT_PROVEN` predicates.

The campaign root owns goal meaning, claim selection, cross-lane dependencies,
contradictions, frontier decisions, goal-level evidence joins, and final reconciliation.
It normally does not perform claim-local leaf implementation, broad repository
archaeology, raw log analysis, or repetitive proof. Route those into claim-local lanes
or focused workers unless direct inspection of one load-bearing seam is itself the
campaign judgment.

For a durable campaign, keep one useful current synthesis on the umbrella issue. For a
session-sized goal, runtime context may be sufficient. Do not scan or score the whole
backlog, inspect sibling worktrees for overlap, mutate a tracked active-goal pointer,
or write the runtime frontier to a file.

## Reconstruct the runtime-local frontier

Maintain only the bounded claims required by this goal:

| Field | Meaning |
| --- | --- |
| Claim | one coherent acceptance-and-rollback claim |
| Acceptance predicate | goal condition the claim can satisfy |
| Lane context | active/resumable claim-local orchestrator, if any |
| Durable subject | controlling issue, PR, merge, or closeout |
| Current judgment | current typed lane result or missing judgment |
| External wait | GitHub, prerequisite, platform, or owner condition |
| Wake event | the material event that can change the judgment |

This frontier is runtime-local and disposable. Reconstruct it from GitHub and repository
artifacts after compaction or provider replacement. Never commit it, serialize agent
liveness, or mirror it through labels/comments.

Revisit an in-flight lane only when its wake event occurs. Unchanged checks, reviews,
queue state, or base movement do not justify polling or route churn.

## Phase eligibility

Admission asks whether the host can run a claim. Phase eligibility asks whether this
campaign should run it now. They are different questions, and a campaign can fail on
either.

Name the goal's current phase and what it admits. The failure this prevents is not
picking bad work — it is that every valuable adjacent finding becomes runnable while the
phase predicate stays unsatisfied, so the campaign accumulates good PRs and never
converges on its own acceptance criterion:

```text
phase: honest-main

eligible
  classify current main failures
  repair current main failures
  repair the instruments needed to classify them
  bounded read-only evidence supporting those decisions

defer
  ordinary backlog
  unrelated hardening
  product opportunities
  release work the phase does not require
```

Deferring is not discarding. Record a deferred finding as a durable issue and move on;
the cost of losing it is what makes adjacent work feel urgent.

A discovery may change the phase, but change it deliberately and state why. Widening the
phase to accommodate work already started is how the predicate quietly stops governing.

## Select and run a claim route

Choose one distinct claim that is phase-eligible, still required, actionable, not
already represented by an equivalent current PR, and independently reviewable.

For substantial work, delegate the whole claim as a lane-root route:

```text
Take issue #123 through `$deliver-pr`.
You are the accountable lane root for this claim. Use GitHub and repository artifacts
as durable state, invoke `$orchestrate-work` within the claim, keep one candidate
writer, follow every normal and material backward route, and return RECONCILED,
IN_FLIGHT, PARTIAL, SUPERSEDED, BLOCKED, or NOT_PROVEN.
Do not select unrelated claims or change the parent goal.
```

The lane root chooses claim-local task, writer, proof, and review agents as useful.
The campaign root does not hand-script a substitute lifecycle.

For a tiny claim where another leaf worker would cost more than it saves, keep the work
inside a claim-local lane context and let that lane root or current writer execute it
directly. Do not convert the campaign root into the leaf worker merely because the
patch is small.

## Traceable route and useful GitHub boundaries

When another context will need the route and it is not already obvious, add one compact
route declaration to the controlling issue or PR:

```text
Route
- Goal / parent: <umbrella or durable outcome>
- Claim: <one acceptance-and-rollback claim>
- Entry flow: `$deliver-pr`
- Current useful transition: <named skill or external wait>
- Why: <material missing judgment>
- Durable subject: <issue / PR / merged commit>
- Resume when: <wake event, if any>
```

Update it only when the material route changes. Do not post one comment per skill,
agent, poll, or head SHA.

Publish goal-level information only when it changes or usefully summarizes accepted
goal interpretation/predicates, claim selection, prerequisite/supersession,
cross-PR contracts, shared blockers/wake events, merged effects, residual claims, or
completion judgment. Keep runtime topology, task lists, liveness, retries, raw logs,
and unchanged status local to the campaign root.

## Bounded related-PR review orchestration

When directly linked PRs have interacting contracts, authority, or merge order, each PR
keeps its own lane and provider-native review:

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

The campaign root may synthesize:

| PR | Candidate identity | Current checks | Substantive review result | Integration posture | Explicit prerequisite |
| --- | --- | --- | --- | --- | --- |

Verify parent/child schema and validator agreement, complete candidate/artifact identity,
semantic ownership, status and limitation propagation, fan-in evidence loading,
`NOT_PROVEN` visibility, and real repair/merge order.

A green aggregate cannot outrun untrustworthy children. The synthesis is goal-level
judgment, not batch approval or a substitute for per-PR review.

## Loop

```text
reconstruct goal and runtime-local frontier
→ select one actionable claim
→ run `$deliver-pr` through a lane root
→ when the lane reaches a durable GitHub-owned wait, record the wake event once
→ retain the lane as IN_FLIGHT
→ advance another independent claim
→ reconcile merged or deliberately closed claims
→ sweep worktree residue
→ publish useful goal-level deltas
→ re-evaluate every original acceptance predicate
```

Sweep each pass, not at wind-down. Per-claim release misses whatever ended without
reconciling — superseded, abandoned, or a lane that died mid-flight — so residue
accumulates even when every individual release worked. Deferring the sweep to the end of
a campaign runs it exactly when local evidence is least trustworthy and the context that
knew what each worktree was for is gone.

Run `bash scripts/cleanup-completed-worktrees.sh --dry-run` to report disposition; it
keeps anything holding uncommitted changes, unpushed commits, detached work outside the
base, or a worktree-manager owner. Review the output, then run the same command without
`--dry-run` to apply safe removals, or release owned slots via
`worktree-manager.py release` when the allocator still holds the lease.

If another PR lands and a candidate remains valid, do nothing. If an actual conflict,
explicit stack change, or combined-tree failure appears, the affected lane repairs its
own candidate and refreshes only affected proof and review.

## Goal completion contract

Return `GOAL_SATISFIED` only when every acceptance predicate is `PASS` or explicitly
`NOT_APPLICABLE`, with current evidence and retained limitations named. Several merged
PRs or an exhausted issue list is not sufficient.

Use `GOAL_PARTIAL` only when progress was deliberately bounded or the durable outcome
was narrowed/superseded. Use `EXTERNAL_BLOCKER` only when every remaining required
claim shares one real external condition or accountable owner decision. Use
`NOT_PROVEN` when the reliable goal boundary or live graph cannot be reconstructed.

## What this establishes

A bounded, resumable goal-level orchestration result: required claims and acceptance
predicates are explicit; each active claim has a durable subject, current judgment, and
wake event; interacting PR contracts and merge order are synthesized only after each
candidate receives its own provider-native review; merged effects and every residual
required claim are reconciled against the original goal.

## What this does not establish

A repository scheduler, tracked frontier, active-goal file, portfolio queue, build-all
wave, overlap ledger, agent registry, comment-per-transition protocol, batch review
approval, or merge authority independent of each candidate's review and live ruleset.
