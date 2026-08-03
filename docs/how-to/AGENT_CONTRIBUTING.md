# Agent contributing guide

This guide is the short route for an agent entering work in this repository. It
points to the durable repository contracts; it is not a second workflow engine.

## Start from live truth

Begin with the current GitHub issue or pull request, the checked-out repository,
and `origin/main`. Read the issue body as the current claim, then verify its
paths, ownership, acceptance criteria, and proof commands against the code and
current GitHub state. Historical comments and labels are useful receipts, but
they do not override current source or live checks.

The root [AGENTS.md](../../AGENTS.md) is the stable role router. The detailed
method and recovery rules are in [CLAUDE.md](../../CLAUDE.md) and the
[development method](../agents/DEVELOPMENT_METHOD.md).

## Shape one bounded claim

A useful work item has one target seam, a measurable acceptance condition, a
proof path, explicit non-goals, and a cleanup or rollback path. Keep one coherent
claim in a review-forward PR. If adjacent work is discovered, preserve it as a
follow-up issue or update the owning issue; do not bundle it merely because the
files are nearby.

The branch and worktree are the writer boundary. Same-file work in another lane
is not ownership by itself. If a real textual conflict occurs, the later lane
repairs the affected candidate and reruns only the proof and review that the
conflict changed.

## Do not manufacture authority

- Do not use lifecycle labels, agent identities, comments, or a private queue as
  permission to proceed or as proof that a claim is true.
- Do not add a scheduler, reservation system, tracked active goal, baseline, or
  workflow database for ordinary work.
- Do not weaken or remove tests, broaden an allowlist, or invent an API to make a
  check green.
- Do not bundle unrelated fixes or claim that a focused check proves more than it
  actually exercised.
- Treat missing, partial, stale, contradictory, rate-limited, or instrument-
  failed evidence as `NOT_PROVEN`.

Labels may help people navigate history, but live GitHub state, repository
contracts, checks, reviews, and the branch ruleset are the authorities. See
[Live Signals vs Label Signals](../reference/LIVE_SIGNALS_VS_LABELS.md).

## Make proof discriminating

Choose the cheapest proof that can distinguish the intended change from a
realistic wrong implementation. Keep command evidence tied to the candidate:

```text
command and arguments
working directory
candidate or commit identity
exit/termination result
relevant output or artifact reference
claim established and remaining uncertainty
```

For behavior changes, add or strengthen a test at the observable seam. For
documentation or policy changes, verify every link and run the narrow repository
check that governs the edited surface. A passing command is not evidence that a
different command, platform, or hosted environment also passed.

The active gate vocabulary and proof boundaries are in
[Pipeline Gates](../reference/PIPELINE_GATES.md). Formal review is a directed,
falsifying, and verified judgment, not a second reading of the diff or a relay
of someone else's conclusion. The review currentness contract is in
[REVIEW_CURRENTNESS.md](../agents/REVIEW_CURRENTNESS.md).

## Delegate for context economics

Stay direct for a small, tightly coupled claim. Delegate when the evidence-to-
answer compression ratio is high: CI or log triage, repository-wide searches,
dependency/API audits, external-source collection, failure bisection, broad
inventories, or an independently useful proof adversary.

The brief should bound the target, authority, sufficient result, falsifiers,
stop conditions, and non-goals. The child returns concise evidence and
references; the warm root retains decisions, contradictions, and integration.
Delegation is useful when it changes the source, oracle, environment, threat
model, method, or attention surface—not merely because another identity can
repeat the same inspection.

## Finish the lane

Before publication, inspect the diff and run the scoped proof. After publication,
address actionable review findings, check the current candidate state, and merge
only when the repository's live rules permit it. A clean review is a valid result
when the applicable review contract is satisfied.

After a squash merge, sync the relevant view, remove worktrees and branches
created by the lane when safe, remove scratch artifacts, and update the owning
issue with the landed commit, proof, remaining uncertainty, and bounded next
step. Close an issue when it owns no live decision or buildable claim; preserve
the history through issue and PR lineage rather than stale lifecycle text.

For the complete contributor preparation and build profiles, continue through
the [repository CONTRIBUTING.md](../../CONTRIBUTING.md).
