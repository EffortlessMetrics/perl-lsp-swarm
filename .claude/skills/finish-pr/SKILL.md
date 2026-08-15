---
name: finish-pr
description: Carry one selected pull request through publication, feedback repair, Claude-native substantive review, live integration, squash merge, and reconciliation.
argument-hint: "[PR number, branch, or candidate]"
---

# Finish PR

Carry one coherent acceptance-and-rollback candidate through its current GitHub state.
The main Claude thread remains the accountable lane owner. Do not inspect sibling
implementations or treat nearby files, crates, branches, or worktrees as ownership.

Read the selected PR, controlling issue, governing authority, cumulative diff, proof
and limitations, submitted reviews, inline threads, current substantive review result,
required checks, draft purpose, mergeability, and explicit prerequisites.

A shared method document, green CI, zero open threads, bot output, textual
mergeability, or the author reading the diff does not establish substantive review.
Claude's operational path is this skill together with `orchestrate-work`,
`final-challenge`, `review-pr`, and `verify-live-ci`.

## Orchestration affordances

### Lane-root decisions

The lane root retains the earliest useful route entry, finding validity/disposition,
which repairs materially change proof/review dimensions, substantive review sufficiency,
candidate/base/integration failure ownership, remote in-flight/blocking classification,
protected merge, and current-main reconciliation.

### Useful subagent and workflow work

Use focused subagents, context forks, an Ultracode workflow, or an Agent Team only where
useful for:

- complete review-thread and submitted-review inventory;
- source-backed finding verification;
- high-output CI log/artifact classification;
- live required-check/ruleset discovery;
- production-path, external-oracle, proof, candidate, security, package, migration,
  persistence, support, or release review;
- merged-effect and residual-claim verification.

Each child receives the exact PR/candidate, accepted claim, current GitHub snapshot,
named skill where applicable, one bounded question, falsifiers, sufficient evidence,
uncertainty, and non-goals. Children return graph deltas, not merge verdicts.

### Mutation owner and join

One writer integrates accepted candidate repairs. Read-only reviewers and CI evidence
agents do not mutate the candidate.

The lane root joins current findings/dispositions, affected proof, cumulative review,
live integration facts, contradictions, and limitations into one typed route result.
Repeated bot findings or several agents reading one artifact are not independent
evidence.

### Return packet

Return PR/candidate identity, claim/non-goals, current finding dispositions,
proof/review dimensions current or stale, substantive review result, integration
posture, exact remote wait/wake event, limitations/`NOT_PROVEN`, merge/closeout result,
and next route.

## Procedure

Enter at the earliest useful point:

```text
no PR + publication-ready candidate
→ `publish-pr`

draft with a real remote-proof, collaboration, or protected-experiment purpose
→ complete that purpose
→ `publish-pr`

substantive human/bot/CI findings or failed candidate proof
→ `address-review-comments`
→ rerun affected proof

candidate is mutable and no useful current substantive review exists
→ `final-challenge`
→ `orchestrate-work` for the applicable review subgraph
→ `review-pr`

`CHANGES_REQUIRED`
→ `address-review-comments`
→ one writer repairs
→ affected proof
→ affected `final-challenge`
→ affected `review-pr`

`REVIEW_CURRENT`
→ `verify-live-ci`

`INTEGRATION_READY`
→ `merge-reconcile`

merged or deliberately closed but unreconciled
→ `merge-reconcile`
```

Do not skip directly from “no open findings” to live CI unless a useful cumulative
`REVIEW_CURRENT` judgment exists. The review must be evidence-backed and semantically
current; it need not be repeated merely because the head SHA changed.

## Claude-native review handoff

Before `review-pr`, the lane root uses `orchestrate-work` to select only review lenses
that can change the decision. It may delegate `review-tests`, `review-candidate`, a
production-path trace, an external oracle, or a focused
security/package/migration/persistence/support question to read-only subagents or
context forks. Use Agent Teams only when lateral communication changes the result.

Each child receives the exact candidate, controlling claim, established facts,
authority, named skill, falsifiers, read-only boundary, sufficient evidence, and
non-goals.

The lane root joins evidence rather than votes, inspects load-bearing seams, and
publishes one cumulative `review-pr` judgment. Reviewers do not authorize merge. One
writer repairs accepted findings.

## Review-forward repair

Review is cumulative and semantic:

- verify each repair against the finding, proof, and seam it changes;
- revisit claim, production reachability, authority, compatibility, risk, rollback, or
  proof only when the repair materially changes that dimension;
- formatting, editorial cleanup, generated receipt refresh, and stronger tests do not
  automatically invalidate prior review;
- actual conflict or combined-tree repair receives focused review of the affected
  interaction.

Do not compute a claim digest, require a review receipt tied to the current head, or
restart a full deep review merely because another commit was pushed.

## Candidate and integration boundary

One writer mutates this candidate branch/worktree at a time. Read-only research,
review, CI classification, and oracle work may assist.

- behind-only movement on `main` requires no action;
- a real Git conflict is resolved in this lane;
- an explicit stack is retargeted after its prerequisite lands;
- a combined-tree interaction is repaired in the smallest affected candidate;
- only affected proof and review are refreshed.

When GitHub owns the next transition—pending checks, requested review, merge queue, or
armed auto-merge—record the exact pending fact and wake event once when useful and
return `PR_IN_FLIGHT`. Do not poll unchanged state or call the wider goal blocked. A
remote integration wait does not make a still-current substantive review stale.

## Useful GitHub boundary

Publish candidate-wide route/proof/limitation changes in the PR body or a compact PR
comment; localized findings inline; finding dispositions in replies before resolution;
one cumulative submitted review; one material remote wait/wake update when another
context needs it; and landed-effect/residual-claim closeout on the issue.

Keep subagent/Team topology, task progress, raw bot/CI logs, repeated check snapshots,
retries, unchanged polls, and routine route transitions runtime-local. Link stable
artifacts rather than copying them.

## Routes

### Publication

- `PR_PUBLISHED_READY` / `PR_RESUMED` → `address-review-comments`
- `DRAFT_FOR_NAMED_REASON` → complete the reason, then `publish-pr`
- `CANDIDATE_NOT_COHERENT` / `LOCAL_PROOF_STALE` / `WORKTREE_DIRTY` →
  `build-candidate`
- `DUPLICATE_OR_WRITER_COLLISION` → reuse the equivalent candidate or resolve the
  actual same-branch/worktree collision
- `IDENTITY_NOT_PROVEN` → establish candidate identity or return `NOT_PROVEN`

### Findings and challenge

- `FINDINGS_REPAIRED_OR_DISPOSITIONED` → affected proof, then `final-challenge`
- `MUTABLE_FINDINGS_OPEN` → `build-candidate`
- `PROOF_WEAKENED` / `PROOF_REVISE` → `prepare-proof`
- `MATERIAL_PREMISE_CHANGED` / `SPLIT_CLAIM` → `prepare-issue`
- `FOLLOW_UP_ACCEPTED` → create or link the bounded follow-up and continue
- `DISPOSITION_INSTRUMENT_FAILURE` → preserve the finding and return `NOT_PROVEN`

### Substantive review

- `CANDIDATE_READY_FOR_REVIEW` → `final-challenge`, `orchestrate-work`, then
  `review-pr`
- `REVIEW_CURRENT` → `verify-live-ci`
- `CHANGES_REQUIRED` / `REVIEW_FINDINGS_OPEN` → `address-review-comments`
- `REVIEW_SCOPE_CHANGED` → review the affected dimensions; use `prepare-issue` only
  when claim or owner changed
- `BLOCKED_BY_PREREQUISITE` → preserve the exact prerequisite in the invoking flow
- `SUPERSEDED_OR_CLOSE` → `merge-reconcile` for durable closeout
- `NOT_PROVEN` / `REVIEW_NOT_PROVEN` → resolve missing evidence or authority

### Live integration

- `REVIEW_REQUIRED` → `final-challenge`, `orchestrate-work`, then `review-pr`
- `PRODUCT_OR_TEST_FAILURE` → `build-candidate`, then affected proof and review
- `PENDING` / `PENDING_REMOTE` / `PR_IN_FLIGHT` → return to `deliver-pr` or
  `deliver-goal`
- `CONFLICT` / `INTEGRATION_INTERACTION` → repair the affected seam, then affected
  proof and review
- `INSTRUMENT_FAILURE` / `NOT_PROVEN` → preserve the missing reliable evidence
- `INTEGRATION_READY` → `merge-reconcile`

### Merge and closeout

- `RECONCILED` → return the closeout
- `PARTIAL` → preserve remaining acceptance
- `SUPERSEDED` / `DELIBERATELY_CLOSED` → preserve the durable disposition
- `CANDIDATE_MOVED` → re-read live state and refresh only affected proof/review
- `MERGE_BLOCKED` → return `PR_IN_FLIGHT` for GitHub-owned waits; otherwise preserve
  the real blocker
- `BLOCKED` / `NOT_PROVEN` → preserve the exact blocker or missing evidence
