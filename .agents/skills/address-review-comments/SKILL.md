---
name: address-review-comments
description: Verify and disposition every substantive human, bot, and candidate-owned CI finding, routing candidate mutations through local proof before re-review.
---

# Address review comments

Read the current PR, controlling issue, governing contract, cumulative candidate,
current local candidate result, submitted reviews, inline threads, and relevant CI
evidence.

For each substantive finding choose one supported lowercase class:

```text
fixed
refuted
superseded
follow-up
```

## Lane-root decisions

The persistent claim lane retains:

- whether a finding is valid, stale, refuted, superseded, duplicate, or a bounded
  follow-up;
- whether it is candidate-, base-, integration-, oracle-, instrument-, or
  environment-owned;
- whether it changes claim, owner, proof, compatibility, risk, rollback, or support;
- the accepted repair and same-candidate writer;
- which local proof and substantive review dimensions become stale.

Use focused workers only for evidence that can change those judgments: complete finding
inventory, source/external-authority verification, high-output CI classification,
reproduction, production-path tracing, proof/oracle challenge, or detecting a weakened
test/ratchet/support claim disguised as a fix. Workers do not resolve threads or
authorize repair.

One writer integrates accepted candidate repairs. A reviewer that found the defect may
be promoted in place when the repair is bounded, authority is granted, and no other
writer is mutating the candidate. Keep that context and worktree rather than paying a
cold start.

## Procedure

1. Enumerate current review threads with:

   ```bash
   scripts/reviews/threads <pr> [owner/repo] [--unresolved-only] [--json]
   ```

   This is the sanctioned source of `<threadId>`. `scripts/reviews/state` is aggregate
   only; do not hand-roll review-thread GraphQL.
2. Verify each finding against current source, governing authority, observed behavior,
   and candidate-bound proof. Do not patch comments literally.
3. Refute findings already answered by the semantic currentness contract. A
   conflict-free candidate is not defective because it is behind `main`, its head SHA
   changed, or a check ran on an older SHA. Revalidate an unchanged affected seam when
   necessary; do not dismiss a genuine older-head failure merely because unrelated work
   landed. Base attribution requires an equivalent gate and matching failure signature
   at the candidate's merge base.
4. Classify each accepted candidate-owned finding:
   - implementation/test construction needed → `$build-candidate` inside this context;
   - proof itself weak/circular → `$prepare-proof` before repair is accepted;
   - material claim/owner change → `$prepare-issue`;
   - bounded follow-up outside this claim → create/link the follow-up when authorized.
5. Batch accepted mutations through the one writer. Preserve unaffected work and the
   current claim boundary.
6. If the candidate changed, commit one coherent candidate and run
   `$prove-before-push`. Do not push or describe the repair as complete from formatting,
   diff hygiene, or an isolated passing test alone.
7. Push the proven candidate normally without force when authorized. If the push is
   rejected, inspect intervening content and integrate only a real conflict or compatible
   work; do not convert head movement into `CANDIDATE_MOVED`.
8. Compose each canonical reply with:

   ```text
   Disposition: <fixed|refuted|superseded|follow-up>
   Evidence: <claim-bounded evidence summary>
   ```

   Pass the complete text through `scripts/reviews/disposition` with the PR, thread ID,
   lowercase class, and required class-specific evidence (`--commit`, `--argument`,
   `--superseded-by`, or `--issue`). Let the helper append its marker, post the reply,
   and only then resolve the thread.
9. Re-run the thread enumerator and confirm no substantive finding was silently dropped.
10. Return a result that distinguishes candidate mutation from disposition-only work.

Do not call raw thread-resolution APIs, resolve performatively, or use labels/personas as
evidence.

## Join and return packet

The join is complete only when every substantive finding has one supported visible
disposition, accepted candidate mutations have a current `$prove-before-push` result,
material premise changes have returned to the proper earlier route, and unresolved
contradictions remain explicit.

Return:

- candidate/base/head and claim identity;
- complete substantive finding set and ownership classification;
- dispositions and their evidence/commit/follow-up identity;
- whether the candidate changed;
- local candidate result, proof run/not run, and affected review dimensions;
- unresolved contradictions, limitations, and exact next skill;
- cleanup of lane-created resources no longer needed.

## GitHub boundary

Localized findings and dispositions belong in their inline threads. Cross-cutting
finding classes, material premise changes, bounded follow-ups, or candidate-wide proof
changes may update the PR/issue synthesis. Keep worker topology, raw logs, duplicate
findings, temporary reproduction output, retries, and routine transitions runtime-local.

## Result classes and routes

- `FINDINGS_DISPOSITIONED_NO_CANDIDATE_CHANGE` → `$final-challenge` for the still-current
  candidate, then affected `$review-pr`
- `FINDINGS_REPAIRED_LOCAL_CANDIDATE_PROVEN` → push/update the PR, then affected
  `$final-challenge` and `$review-pr`
- `FINDINGS_REPAIRED_REMOTE_ONLY_PROOF_REQUIRED` → update/preserve the named draft remote
  boundary, then resume when its wake event occurs
- `CANDIDATE_PRODUCT_OR_TEST_FAILURE` / `RIPR_GAP_REQUIRES_REPAIR` →
  `$build-candidate` / `$improve-test-suite`, then repeat `$prove-before-push`
- `PROOF_WEAKENED` / `WEAK_OR_CIRCULAR_PROOF` → `$prepare-proof`
- `MATERIAL_PREMISE_CHANGED` / `SPLIT_CLAIM` / `RETURN_TO_ISSUE` → `$prepare-issue`
- `FOLLOW_UP_ACCEPTED` → create/link the bounded follow-up and continue this PR within
  its claim
- `INSTRUMENT_NOT_PROVEN` / `DISPOSITION_INSTRUMENT_FAILURE` / `BLOCKED` /
  `NOT_PROVEN` → preserve the unresolved finding or missing reliable evidence
