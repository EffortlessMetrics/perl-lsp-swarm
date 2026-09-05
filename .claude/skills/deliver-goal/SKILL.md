---
name: deliver-goal
description: Operate a durable multi-PR outcome from the accountable root through logical claim frames, provider-native SDLC flows, useful GitHub summaries, and current acceptance evidence.
argument-hint: "[umbrella issue or durable outcome]"
---

# Deliver goal

This is the main-thread goal flow. Preserve the verbatim goal source when available; the
current interpretation, constraints, non-goals, acceptance predicates, governing
contracts, current main and evidence, directly linked claims and PRs, explicit
dependencies, merged effects, known limitations, and `NOT_PROVEN` predicates.

The main Claude thread owns goal meaning, claim selection, cross-claim dependencies,
contradictions, runtime-local claim frames, evidence joins, wake events, and final
reconciliation.

A claim frame is logical root state, not normally another agent:

| Field | Meaning |
| --- | --- |
| Claim | one coherent acceptance-and-rollback claim |
| Acceptance predicate | goal condition the claim can satisfy |
| Durable subject | controlling issue, PR, merge, or closeout |
| Current candidate | branch/PR/head and one writer, when mutation exists |
| Current judgment | current typed result or missing judgment |
| External wait | GitHub, prerequisite, platform, or owner condition |
| Wake event | the material event that can change the judgment |

Keep these frames runtime-local and disposable. Reconstruct them from GitHub and
repository artifacts after compaction or replacement. Never commit them, serialize
teammate liveness, or mirror them through labels/comments.

## Phase eligibility

Admission asks whether the host can run a claim. Phase eligibility asks whether this
goal should run it now. They are different questions.

Name the goal's current phase and what it admits. Valuable adjacent findings do not
become runnable merely because they are true. Record deferred work durably and keep the
phase predicate governing selection.

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

Deferring is not discarding. A discovery may change the phase, but change it deliberately
and state why; widening the phase to accommodate work already started makes the
predicate stop governing selection.

## Select and run a claim

Choose one distinct claim that is phase-eligible, still required, actionable, not
already represented by an equivalent current PR, and independently reviewable.

Then focus the main thread on that claim by invoking `deliver-pr` in the same root
context:

```text
select claim frame
→ `deliver-pr`
→ delegate bounded research/build/review programmes as useful
→ join evidence in the main thread
→ update the claim frame
→ return to the goal loop
```

Do **not** create a subordinate orchestrator merely because the claim is substantial.
The main thread retains claim meaning, route selection, evidence joins, review
disposition, and continuation. Researchers, builders, reviewers, subagents, context
forks, Ultracode workflows, or Teams are bounded execution contexts.

Claude may physically nest an agent, fork, workflow, or Team when that specific
execution has an evidence-backed advantage. That does not change logical authority and
must not become a required whole-flow hierarchy.

## Traceable route and useful GitHub boundaries

When another session will need the route and it is not already obvious, add one compact
route declaration to the controlling issue or PR:

```text
Route
- Goal / parent: <umbrella or durable outcome>
- Claim: <one acceptance-and-rollback claim>
- Entry flow: `deliver-pr`
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
and unchanged status local to the main thread.

## Bounded related-PR review orchestration

When directly linked PRs have interacting contracts, authority, or merge order, each PR
still receives its own provider-native review through its claim frame:

```text
`deliver-pr`
→ `finish-pr`
→ `orchestrate-work` for applicable adversarial lenses
→ `review-pr`
→ REVIEW_CURRENT | CHANGES_REQUIRED | NOT_PROVEN |
  BLOCKED_BY_PREREQUISITE | SUPERSEDED_OR_CLOSE
→ when REVIEW_CURRENT, `verify-live-ci`
→ INTEGRATION_READY | PR_IN_FLIGHT | MERGE_BLOCKED | NOT_PROVEN
```

The main thread may synthesize cross-PR contracts and order only after each candidate
has its own review evidence:

| PR | Candidate identity | Current checks | Substantive review result | Integration posture | Explicit prerequisite |
| --- | --- | --- | --- | --- | --- |

Verify complete candidate identity, semantic ownership, status and limitation
propagation, `NOT_PROVEN` visibility, and real repair/merge order.

A green aggregate cannot outrun untrustworthy children. The synthesis is goal-level
judgment, not batch approval or a substitute for per-PR review.

## Loop

```text
reconstruct goal and claim frames
→ select one actionable claim
→ run `deliver-pr` in the main thread against that frame
→ when the claim reaches a GitHub-owned wait, record its wake event once
→ retain the logical frame as IN_FLIGHT; no live agent required
→ advance another independent claim
→ reconcile merged or deliberately closed claims
→ sweep safe worktree residue
→ publish useful goal-level deltas
→ re-evaluate every original acceptance predicate
```

Start a residue sweep with `bash scripts/cleanup-completed-worktrees.sh --dry-run` and
review every disposition against current Git/worktree state. Rerun it without
`--dry-run` only for proven-safe removals; when `scripts/worktree-manager.py` still owns
the slot, release it through the manager with the allocation's owner token instead of
deleting around that local lease.

If another PR lands and a candidate remains valid, do nothing. If an actual conflict,
explicit stack change, or combined-tree failure appears, repair the affected claim and
refresh only affected proof and review.

## Goal completion contract

Return `GOAL_SATISFIED` only when every acceptance predicate is `PASS` or explicitly
`NOT_APPLICABLE`, with current evidence and retained limitations named. Several merged
PRs or an exhausted issue list is not sufficient.

Use `GOAL_PARTIAL` only when progress was deliberately bounded or the durable outcome
was narrowed/superseded. Use `EXTERNAL_BLOCKER` only when every remaining required
claim shares one real external condition or accountable owner decision. Use
`NOT_PROVEN` when the reliable goal boundary or live graph cannot be reconstructed.

## What this establishes

A bounded, resumable goal result whose claims are explicit root-held frames with durable
subjects, current judgments, candidates/writers where applicable, and wake events.

## What this does not establish

A subordinate claim-orchestrator hierarchy, repository scheduler, tracked frontier,
active-goal file, portfolio queue, build-all wave, overlap ledger, agent registry,
comment-per-transition protocol, batch review approval, or merge authority independent
of each candidate's review and live ruleset.
