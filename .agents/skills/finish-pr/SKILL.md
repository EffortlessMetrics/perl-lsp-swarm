---
name: finish-pr
description: Carry one selected pull request through feedback repair, candidate-bound local proof, substantive review, live integration, squash merge, and reconciliation.
---

# Finish PR

Carry one coherent acceptance-and-rollback candidate through its current GitHub state in
its persistent claim context. Do not replace the lane merely because the work changes
from review to repair, proof, CI diagnosis, or closeout.

Read the selected PR, controlling issue, governing authority, cumulative diff, current
local candidate result, proof and limitations, submitted reviews, inline threads, draft
purpose, required checks, mergeability, rulesets, and explicit prerequisites.

Green CI, zero open threads, textual mergeability, bot output, or the author reading the
diff does not establish substantive review. The operational path combines
`$address-review-comments`, `$build-candidate`, `$prove-before-push`, `$final-challenge`,
`$review-pr`, `$verify-live-ci`, and `$merge-reconcile` as selected by current state.

## Lane-root decisions

The persistent claim lane retains:

- the earliest still-useful route entry;
- finding validity and disposition;
- accepted repair and same-candidate writer boundary;
- which proof/review dimensions a repair changes;
- substantive review sufficiency;
- candidate, base, integration, oracle, instrument, and environment failure ownership;
- remote in-flight versus material blocker classification;
- merge, close, reconciliation, and residual-claim decisions.

Use `$orchestrate-work` for independent evidence that can change one of those decisions:
thread inventory, high-output CI classification, external authority, production-path
tracing, proof challenge, security/compatibility/package/migration/persistence/support
review, or landed-effect verification. Workers return evidence, not merge verdicts.

One writer integrates accepted candidate repairs. A dedicated reviewer may be promoted
in place when the finding is accepted, authority is granted, and no other writer is
mutating the candidate. Keep the review context and worktree rather than cold-starting a
repair agent.

## Procedure

Enter at the earliest useful point:

```text
no PR + coherent candidate
→ `$prove-before-push`
→ `$publish-pr`

draft for a named remote-only/collaboration purpose
→ complete the purpose
→ `$publish-pr`

human, bot, or candidate-owned CI findings
→ `$address-review-comments`
→ one writer repairs through `$build-candidate` where needed
→ commit coherent candidate
→ `$prove-before-push`
→ push/update the PR normally
→ affected `$final-challenge`
→ affected `$review-pr`

no useful current substantive review
→ `$final-challenge`
→ `$orchestrate-work` for missing independent lenses
→ `$review-pr`

`REVIEW_CURRENT`
→ `$verify-live-ci`

`INTEGRATION_READY`
→ `$merge-reconcile`

merged or deliberately closed but unreconciled
→ `$merge-reconcile`
```

A material repair is not reviewable merely because focused tests or formatting pass. It
must have a current `$prove-before-push` result for the committed candidate before the
branch is represented as repaired. `REMOTE_ONLY_PROOF_REQUIRED` may justify a draft
update; `INSTRUMENT_NOT_PROVEN` does not create a ready candidate.

Do not skip directly from “no open findings” to live CI unless a useful cumulative
`REVIEW_CURRENT` judgment exists. Semantic review currentness does not require a new
review solely because the head SHA changed.

## Review orchestration

Before `$review-pr`, reuse current joined adversarial evidence. Invoke only missing,
stale, contradictory, or materially changed lenses, such as:

- `$review-tests` for discrimination and false-green proof;
- `$review-candidate` for implementation, ownership, reachability, complexity, risk, and
  rollback;
- claim-vs-code propositions;
- production-path tracing;
- competent external authority;
- focused security, packaging, migration, persistence, support, or release review.

Brief each lens with an exact proposition and realistic falsifier. Different identity
without a different source, oracle, method, threat model, environment, or attention
surface is not independent corroboration.

The claim lane joins evidence rather than votes, inspects load-bearing seams, and
publishes one cumulative `$review-pr` judgment. If a reviewer became the writer, add a
genuinely different detection surface where substantive merge independence would
otherwise collapse into construction self-review.

## Review-forward repair

Review is cumulative and semantic:

- verify each repair against the finding, changed seam, proof, and claim;
- rerun `$prove-before-push` when the committed candidate materially changes;
- refresh claim, production reachability, authority, compatibility, risk, rollback, or
  proof review only when the repair changes that dimension;
- formatting, editorial cleanup, generated receipt refresh, and stronger tests do not
  automatically invalidate unrelated review;
- an actual conflict or combined-tree repair receives focused proof/review of the
  affected interaction.

Do not require a review receipt tied to every head, merge/rebase `main` for freshness, or
restart a full review merely because another compatible commit was pushed.

## Currentness and Git

One writer mutates this candidate branch/worktree at a time. Commit and push normally
without force.

- behind-only movement on `main` requires no action;
- if a push is rejected, fetch and inspect intervening content;
- compatible remote work is ordinary integration, not `CANDIDATE_MOVED`;
- a real conflict is resolved in this lane;
- an explicit prerequisite is retargeted after it lands;
- a combined-tree interaction is repaired in the smallest affected candidate;
- only affected local proof and review are refreshed.

When GitHub owns the next transition—pending checks, requested review, merge queue, or
armed auto-merge—record the exact pending fact and wake event once when another context
needs it, return `PR_IN_FLIGHT`, and let the wider campaign continue. Do not poll
unchanged state.

## Durable GitHub boundary

Update durable state only when it will survive usefully:

- issue/spec for changed premise, authority, plan, prerequisite, or residual claim;
- PR body for cumulative claim, local candidate result, proof, deviations, risk, and
  limitations;
- inline thread for localized finding and evidence-backed disposition;
- one submitted review for cumulative substantive judgment;
- one remote wait/wake update when needed for resumption;
- issue closeout for landed effect and residual work.

Keep agent topology, role changes, temporary experiments, raw bot/CI logs, repeated
snapshots, retries, and routine skill transitions runtime-local.

## Routes

### Publication and local candidate proof

- `CANDIDATE_READY` → `$prove-before-push`
- `LOCAL_CANDIDATE_PROVEN` → push/update normally; new PR uses `$publish-pr`, existing PR
  continues to `$final-challenge` / `$review-pr`
- `REMOTE_ONLY_PROOF_REQUIRED` → `$publish-pr` or update the existing draft only with a
  named remote-proof condition
- `CANDIDATE_PRODUCT_OR_TEST_FAILURE` / `RIPR_GAP_REQUIRES_REPAIR` /
  `CANDIDATE_NOT_COHERENT` / `WORKTREE_DIRTY` → `$build-candidate`
- `WEAK_OR_CIRCULAR_PROOF` / `PROOF_REVISE` → `$prepare-proof`
- `INSTRUMENT_NOT_PROVEN` / `IDENTITY_NOT_PROVEN` → repair the named
  instrument/identity or preserve `NOT_PROVEN`
- `PR_PUBLISHED_READY` / `PR_RESUMED` → current findings/challenge route
- `DRAFT_FOR_NAMED_REASON` → wait for the named wake event, then repeat `$publish-pr`

### Findings and challenge

- `FINDINGS_REPAIRED_OR_DISPOSITIONED` with no mutation → `$final-challenge`
- `FINDINGS_REPAIRED_OR_DISPOSITIONED` with candidate mutation →
  `$prove-before-push`, then affected `$final-challenge`
- `MUTABLE_FINDINGS_OPEN` → `$build-candidate`
- `PROOF_WEAKENED` → `$prepare-proof`
- `MATERIAL_PREMISE_CHANGED` / `SPLIT_CLAIM` → `$prepare-issue`
- `FOLLOW_UP_ACCEPTED` → create/link the bounded follow-up and continue this claim
- `DISPOSITION_INSTRUMENT_FAILURE` → preserve the finding and return `NOT_PROVEN`

### Substantive review

- `CANDIDATE_READY_FOR_REVIEW` → `$final-challenge`, missing `$orchestrate-work` lenses,
  then `$review-pr`
- `REVIEW_CURRENT` → `$verify-live-ci`
- `CHANGES_REQUIRED` / `REVIEW_FINDINGS_OPEN` → `$address-review-comments`
- `REVIEW_SCOPE_CHANGED` → review affected dimensions; `$prepare-issue` only when claim
  or owner changed
- `BLOCKED_BY_PREREQUISITE` → preserve the exact prerequisite
- `SUPERSEDED_OR_CLOSE` → `$merge-reconcile` for durable closeout
- `NOT_PROVEN` / `REVIEW_NOT_PROVEN` → resolve missing evidence or authority

### Live integration

- `REVIEW_REQUIRED` → `$final-challenge`, missing `$orchestrate-work` lenses, then
  `$review-pr`
- `PRODUCT_OR_TEST_FAILURE` → `$build-candidate`, `$prove-before-push`, affected review
- `PENDING` / `PENDING_REMOTE` / `PR_IN_FLIGHT` → return to `$deliver-pr` /
  `$deliver-goal` with the wake event
- `CONFLICT` / `INTEGRATION_INTERACTION` → repair affected seam, then affected local
  proof and review
- `INSTRUMENT_FAILURE` / `NOT_PROVEN` → preserve missing reliable evidence
- `INTEGRATION_READY` → `$merge-reconcile`

### Merge and closeout

- `RECONCILED` → return the closeout
- `PARTIAL` → preserve remaining acceptance
- `SUPERSEDED` / `DELIBERATELY_CLOSED` → preserve durable disposition
- `MERGE_BLOCKED` → `PR_IN_FLIGHT` for GitHub-owned waits; otherwise preserve the real
  blocker
- `BLOCKED` / `NOT_PROVEN` → preserve the exact blocker or missing evidence
