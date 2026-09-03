---
name: finish-pr
description: Carry one selected pull request through publication, feedback repair, Codex-native substantive review, live integration, squash merge, and reconciliation.
---

# Finish PR

Carry one coherent acceptance-and-rollback candidate through its current GitHub state.
The Codex root remains the accountable orchestrator for this root-held claim frame. Do
not inspect sibling implementations or treat nearby files, crates, branches, or
worktrees as ownership.

Read the selected PR, controlling issue, governing authority, cumulative diff, proof
and limitations, submitted reviews, inline threads, current substantive review result,
required checks, draft purpose, mergeability, and explicit prerequisites.

A shared method document, green CI, zero open threads, bot output, textual
mergeability, or the author reading the diff does not establish substantive review.
Codex's operational path is this skill together with `$orchestrate-work`,
`$final-challenge`, `$review-pr`, and `$verify-live-ci`.

## Orchestration affordances

### Root decisions

The accountable root retains:

- the earliest still-useful entry point in the PR route;
- whether a finding is valid, stale, refuted, superseded, or a bounded follow-up;
- whether resolving a valid finding belongs in this candidate;
- whether the candidate or claim changed and which proof/review dimensions became stale;
- substantive review sufficiency and cumulative candidate judgment;
- candidate/base/integration failure ownership;
- whether a remote state is in flight, blocked, or not proven;
- the protected merge and current-main reconciliation decision.

### Delegable read-only work

Use focused workers where useful for:

- complete review-thread and submitted-review inventory;
- source-backed verification of human/bot findings;
- high-output CI log and artifact classification;
- live required-check/ruleset discovery;
- production-path, external-oracle, proof, candidate, security, package, migration,
  persistence, support, or release review;
- merged-effect and residual-claim verification after integration.

Each worker receives the exact PR/candidate, accepted claim, current GitHub snapshot,
named `$skill` where applicable, one bounded question, falsifiers, sufficient evidence,
uncertainty, and non-goals. Workers return graph deltas, not merge verdicts.

### Mutation owner and join

One writer integrates accepted candidate repairs. Read-only reviewers and CI evidence
workers do not mutate the candidate.

The accountable root joins current findings/dispositions, candidate and claim mutation,
stale proof/review dimensions, cumulative review, live integration facts,
contradictions, and limitations into one typed route result. Repeated bot findings or
several workers reading one artifact are not independent evidence.

### Return packet

Return PR/candidate identity, claim and non-goals, current finding dispositions,
`candidate_changed`, `claim_changed`, `stale_review_dimensions`, proof/review dimensions
current or stale, substantive review result, integration posture, exact remote wait and
wake event, limitations/`NOT_PROVEN`, merge/closeout result, and next route.

## Procedure

Enter at the earliest useful point:

```text
no PR + publication-ready candidate
→ `$publish-pr`

draft with a real remote-proof, collaboration, or protected-experiment purpose
→ complete that purpose
→ `$publish-pr`

substantive human/bot/CI findings or failed candidate proof
→ `$address-review-comments`
→ one joined repair wave
→ affected proof only when the candidate changed or review dimensions became stale

candidate is mutable and no useful current substantive review exists
→ `$final-challenge`
→ `$orchestrate-work` for the applicable review subgraph
→ `$review-pr`

`CHANGES_REQUIRED`
→ `$address-review-comments`
→ one writer publishes the joined repair wave once when candidate bytes must change
→ affected proof/challenge/review only for changed or stale dimensions

no candidate or claim change + no stale review dimension
→ preserve current proof/review
→ continue at the earliest genuinely missing judgment

`REVIEW_CURRENT`
→ stabilize the reviewed candidate head for required CI
→ `$verify-live-ci`

`INTEGRATION_READY`
→ `$merge-reconcile`

merged or deliberately closed but unreconciled
→ `$merge-reconcile`
```

Do not skip directly from “no open findings” to live CI unless a useful cumulative
`REVIEW_CURRENT` judgment exists. The review must be evidence-backed and semantically
current; it need not be repeated merely because the head SHA or thread state changed.

## Codex-native review handoff

Before `$review-pr`, the root uses `$orchestrate-work` to select only review lenses
that can change the decision. It may delegate `$review-tests`, `$review-candidate`, a
production-path trace, an external oracle, or a focused
security/package/migration/persistence/support question to read-only native subagents.
Each child receives the exact candidate, controlling claim, established facts,
authority, `$skill`, falsifiers, read-only boundary, sufficient evidence, and
non-goals.

The root joins evidence rather than votes, inspects the load-bearing seams, and
publishes one cumulative `$review-pr` judgment. Reviewers do not authorize merge. One
integrating writer repairs accepted findings.

## Review-forward repair

Review is cumulative and semantic:

- verify each repair against the finding, proof, and seam it changes;
- revisit claim, production reachability, authority, compatibility, risk, rollback, or
  proof only when the repair materially changes that dimension;
- a supported no-change disposition changes thread state, not candidate meaning;
- formatting, editorial cleanup, generated receipt refresh, and stronger tests do not
  automatically invalidate prior review;
- actual conflict or combined-tree repair receives focused review of the affected
  interaction.

Do not compute a claim digest, require a review receipt tied to the current head, or
restart a full deep review merely because another commit was pushed or a comment was
resolved.

## Repair waves and head stabilization

Do not publish one commit per comment. `$address-review-comments` first pins the current
observation basis, decides finding validity separately from current-candidate admission,
joins the review wave, and promotes only repeated mechanisms to bounded failure classes.
One writer integrates admitted source/proof changes, runs affected proof and—when a
class was promoted—its class-level falsifier, rereads the complete governing semantic
units and dependent claims, and publishes one candidate update.

The repair-wave packet must state `candidate_changed`, `claim_changed`, and
`stale_review_dimensions`. `candidate_changed` means a semantic candidate change
(source behavior, proof obligations, or claim-bearing contracts); byte-only updates
such as formatting, generated-receipt refreshes, comment-only edits, and additive test
strengthening leave it false. When the candidate or claim changed semantically, or
review dimensions became stale, refresh only those proof/challenge/review dimensions.
When `candidate_changed=false`, `claim_changed=false`, and `stale_review_dimensions` is
empty, preserve current proof and review conclusions; do not create an empty repair
commit or manufacture another final-challenge cycle.

If the repair wave introduces a new checker, registry, parser, abstraction, or
substantial proof surface, run `$simplify-candidate` before final challenge. Reconcile
the PR body to the candidate's current claim, proof, limitations, and remaining work;
do not retain an ever-growing diary of superseded intermediate defects.

After the affected final challenge and substantive review return `REVIEW_CURRENT`, keep
the reviewed candidate head stable while required checks evaluate it. Reopen mutation
only for a current candidate defect, material claim contradiction, candidate-owned
required-check failure, actual merge conflict, demonstrated combined-tree interaction,
or the bounded fresh merge-tree re-evaluation selected by `$verify-live-ci` under the
currentness contract. `$verify-live-ci` remains the sole action owner for that exception;
this skill does not prescribe an empty commit independently.

Do not mutate the stabilized head for duplicate corroboration, wording preference,
reviewer quota or availability, optional stronger proof that cannot change the current
verdict, or behind-only movement on `main`. A new material finding opens one new joined
repair wave and the repaired candidate must earn `REVIEW_CURRENT` again before it is
stabilized for integration.

## Candidate and integration boundary

One writer mutates this candidate branch/worktree at a time. Read-only research,
review, CI classification, and oracle work may assist.

- behind-only movement on `main` requires no action;
- a real Git conflict is resolved in this claim;
- an explicit squash-merge stack is reconciled against the actual landed parent and its
  child-only delta rather than merely changing the PR base;
- a combined-tree interaction is repaired in the smallest affected candidate;
- only affected proof and review are refreshed.

When GitHub owns the next transition—pending checks, requested review, merge queue, or
armed auto-merge—record the exact pending fact and wake event once when useful and
return `PR_IN_FLIGHT`. Do not poll unchanged state or call the wider goal blocked. A
remote integration wait does not make a still-current substantive review stale.

An armed auto-merge that appears stalled is usually waiting on the slowest required
context, not broken: `ripr+ New Gap Gate` is the tail of the required union. The
manual probe merge mechanism, its compare-and-swap SHA guard, and the waiver bar are
single-sourced in `$merge-reconcile` — follow that skill's text rather than a second
copy here.

## Useful GitHub boundary

Publish candidate-wide route/proof/limitation changes in the PR body or a compact PR
comment; localized findings inline; finding dispositions in replies before resolution;
one cumulative submitted review; one material remote wait/wake update when another
context needs it; and landed-effect/residual-claim closeout on the issue.

Keep worker topology, task progress, raw bot/CI logs, repeated check snapshots, retries,
unchanged polls, and routine route transitions runtime-local. Link stable artifacts
rather than copying them.

## Routes

### Publication

- `PR_PUBLISHED_READY` / `PR_RESUMED` → `$address-review-comments`
- `DRAFT_FOR_NAMED_REASON` → complete the reason, then `$publish-pr`
- `CANDIDATE_NOT_COHERENT` / `LOCAL_PROOF_STALE` / `WORKTREE_DIRTY` →
  `$build-candidate`
- `DUPLICATE_OR_WRITER_COLLISION` → reuse the equivalent candidate or resolve the
  actual same-branch/worktree collision
- `IDENTITY_NOT_PROVEN` → establish candidate identity or return `NOT_PROVEN`

### Findings and challenge

- `FINDINGS_REPAIRED_OR_DISPOSITIONED` with candidate/claim change or non-empty
  `stale_review_dimensions` → affected proof, then `$final-challenge`
- `FINDINGS_REPAIRED_OR_DISPOSITIONED` with no candidate/claim change and empty
  `stale_review_dimensions` → preserve current proof/review and continue at the earliest
  genuinely missing judgment
- `MUTABLE_FINDINGS_OPEN` → one joined repair wave through `$address-review-comments`
- `PROOF_WEAKENED` / `PROOF_REVISE` → `$prepare-proof`
- `MATERIAL_PREMISE_CHANGED` / `SPLIT_CLAIM` → `$prepare-issue`
- `FOLLOW_UP_ACCEPTED` → create or link the bounded follow-up and continue
- `DISPOSITION_INSTRUMENT_FAILURE` / `NOT_PROVEN` → preserve the missing review,
  bounded denominator, authority, instrument, or proof

### Substantive review

- `CANDIDATE_READY_FOR_REVIEW` → `$final-challenge`, `$orchestrate-work`, then
  `$review-pr`
- `REVIEW_CURRENT` → stabilize the candidate head, then `$verify-live-ci`
- `CHANGES_REQUIRED` / `REVIEW_FINDINGS_OPEN` → `$address-review-comments`
- `REVIEW_SCOPE_CHANGED` → review the affected dimensions; use `$prepare-issue` only
  when claim or owner changed
- `BLOCKED_BY_PREREQUISITE` → preserve the exact prerequisite in the invoking flow
- `SUPERSEDED_OR_CLOSE` → `$merge-reconcile` for durable closeout
- `NOT_PROVEN` / `REVIEW_NOT_PROVEN` → resolve missing evidence or authority

### Live integration

- `REVIEW_REQUIRED` → `$final-challenge`, `$orchestrate-work`, then `$review-pr`
- `PRODUCT_OR_TEST_FAILURE` → `$build-candidate`, then affected proof and review
- `PENDING` / `PENDING_REMOTE` / `PR_IN_FLIGHT` → return to `$deliver-pr` or
  `$deliver-goal`
- `CONFLICT` / `INTEGRATION_INTERACTION` → repair the affected seam, then affected
  proof and review
- `INSTRUMENT_FAILURE` / `NOT_PROVEN` → preserve the missing reliable evidence
- `INTEGRATION_READY` → `$merge-reconcile`

### Merge and closeout

- `RECONCILED` → return the closeout
- `PARTIAL` → preserve remaining acceptance
- `SUPERSEDED` / `DELIBERATELY_CLOSED` → preserve the durable disposition
- `CANDIDATE_MOVED` → re-read live state and refresh only affected proof/review
- `MERGE_BLOCKED` → return `PR_IN_FLIGHT` for GitHub-owned waits; otherwise preserve
  the real blocker
- `BLOCKED` / `NOT_PROVEN` → preserve the exact blocker or missing evidence
