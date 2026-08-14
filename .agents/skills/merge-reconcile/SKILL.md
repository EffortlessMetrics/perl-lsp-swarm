---
name: merge-reconcile
description: Require current substantive review and live integration before squash merge, then reconcile the landed or deliberately closed claim without exact-head ceremony.
---

# Merge and reconcile

This is the irreversible edge for one claim. It may be invoked directly, so it must
reconstruct or consume both predecessor judgments rather than assuming another skill
already ran.

No tracked review receipt, state file, claim digest, private task record, or agent
identity is authority. Use current GitHub reviews, inline threads and dispositions,
checks/rulesets/mergeability, the candidate claim, and repository evidence.

## State branch

Inspect the live PR state first.

- `MERGED` → skip merge and reconcile current `main`.
- `CLOSED_UNMERGED` with an evidence-backed close/supersede disposition → reconcile
  within that disposition.
- `CLOSED_UNMERGED` without a durable disposition → `NOT_PROVEN`.
- `OPEN` → establish substantive review, then live integration, then protected merge.
- unknown or partial state → `NOT_PROVEN`.

## Review predecessor

For an open PR, establish the current provider-native substantive result before reading
integration green as permission to merge.

Read:

- cumulative submitted reviews and useful clean conclusions;
- localized inline findings;
- evidence-backed `fixed`, `refuted`, `superseded`, or `follow-up` dispositions;
- current candidate claim, production route, proof, limitations, and material changes;
- whether later commits changed a reviewed semantic dimension.

Classify:

```text
REVIEW_CURRENT
CHANGES_REQUIRED
NOT_PROVEN
BLOCKED_BY_PREREQUISITE
SUPERSEDED_OR_CLOSE
REVIEW_REQUIRED
```

Rules:

- green checks, `mergeable: true`, zero open threads, bot approval, or author
  self-certification cannot create `REVIEW_CURRENT`;
- a resolved thread without a visible evidence-backed disposition does not establish
  convergence;
- a useful clean review is valid;
- later formatting, editorial, generated-receipt, or stronger-test commits do not stale
  unrelated review dimensions;
- material claim, production-route, authority, proof, risk, rollback, compatibility,
  conflict, or integration changes require focused affected review;
- no exact-head review comment or claim hash is required.

Routes:

- `REVIEW_REQUIRED` → `$finish-pr` / `$final-challenge` / `$orchestrate-work` /
  `$review-pr`;
- `CHANGES_REQUIRED` → `$address-review-comments` with one writer, affected proof, and
  affected re-review;
- `NOT_PROVEN` → preserve the missing or contradictory evidence;
- `BLOCKED_BY_PREREQUISITE` → preserve the exact prerequisite and wake event;
- `SUPERSEDED_OR_CLOSE` → reconcile the evidence-backed disposition;
- only `REVIEW_CURRENT` may continue to live integration.

## Integration predecessor

After `REVIEW_CURRENT`, invoke or reconstruct `$verify-live-ci` for the current
candidate and classify:

```text
INTEGRATION_READY
PR_IN_FLIGHT
MERGE_BLOCKED
NOT_PROVEN
```

Verify live GitHub facts:

- PR is ready, not draft;
- required checks are current for the candidate;
- no unresolved substantive thread remains;
- no current `CHANGES_REQUESTED` review remains;
- deliberately requested reviewers are not still pending where their judgment is part
  of this claim;
- mergeability, conflicts, ruleset, queue, and applicable changelog/support state permit
  merge.

A pending check leaves `REVIEW_CURRENT` intact and returns `PR_IN_FLIGHT`. Do not poll
unchanged state. A missing, skipped, stale, cancelled, or instrument-failed result is
not success.

Only this conjunction authorizes the ordinary protected merge path:

```text
REVIEW_CURRENT
AND
INTEGRATION_READY
→ protected squash merge
```

## Protected merge

Use the current head SHA only as compare-and-swap protection at the instant of merge:

```text
gh pr merge <n> --squash --match-head-commit <current-head-sha>
```

That prevents racing a moving branch. It does not make review currentness depend on the
SHA.

If the head moves before merge, re-read the candidate. Refresh only proof, review, and
integration dimensions affected by the new commit. Never use administrative bypass to
discover what is failing or to outrun unresolved review/integration evidence.

## Reconciliation

After merge or evidence-backed deliberate closure:

1. verify the landed/current-main effect where applicable;
2. update or close the controlling issue accurately;
3. keep umbrella goals open when only one predicate landed;
4. update durable contracts, proof, support claims, and changelog only within the
   proven boundary;
5. preserve partial or residual work explicitly;
6. release the claim's worktree;
7. expose the next coherent claim to `$deliver-goal`.

Release on **every** terminal outcome — merged, superseded, deliberately closed,
or **abandoned** — not only the merge path. For an abandoned lane, return the
typed `ABANDONED`/`EXTERNAL_BLOCKER` result to the campaign root and release the
worktree from the allocator or campaign root on that return, same as merge closeout. A cap bounds how many worktrees exist at once;
nothing bounds residue, and most accumulation is finished work whose content already
lives on the remote, each copy still holding a multi-gigabyte `target/`.

Release belongs to whoever allocated the worktree, not to whoever finished the work in
it. A writer cannot remove the directory it stands in, so a lane ending inside its own
worktree leaves it behind by construction. The lane root or campaign root releases on the
typed return.

Keep a worktree only when it holds state existing nowhere else — uncommitted changes,
unpushed commits, or a detached HEAD outside the base. An open PR is not such a state: a
fully pushed branch is restored with one `git worktree add`, and the branch, PR, and
review all survive removal. `bash scripts/cleanup-completed-worktrees.sh --dry-run`
applies that predicate across every worktree.

Post a closeout only when the landed effect, residual claim, support boundary, or next
route is useful. Do not persist runtime topology, task state, or merge-check polling.

## Supersession carries its corrections

When one candidate replaces another, the replacement inherits the superseded candidate's
findings, dispositions, and corrections. Carry them forward before closing:

- corrections to claims the replacement still makes, including anything the superseded
  body stated inaccurately and later fixed;
- accepted findings not yet repaired, with the evidence behind each disposition;
- limitations and `NOT_PROVEN` boundaries still applying to the replacement;
- revalidate every carried finding against the replacement head before preserving a `fixed`, `accepted`, or `NOT_PROVEN` disposition.

A correction that dies with a superseded PR is worse than one never made: the inaccurate
claim reaches `main` through the replacement, and the record shows a reviewed candidate,
so nothing downstream has reason to look again. Where the replacement's claim differs,
state which corrections no longer apply and why rather than dropping them silently.

## Results and routes

- `RECONCILED` → `$deliver-pr` or `$deliver-goal`
- `PARTIAL` → preserve remaining acceptance
- `SUPERSEDED` / `DELIBERATELY_CLOSED` → preserve the durable disposition
- `REVIEW_REQUIRED` → `$finish-pr` / `$review-pr`
- `CHANGES_REQUIRED` → `$address-review-comments`
- `PR_IN_FLIGHT` → return to `$deliver-goal` with the wake event
- `CANDIDATE_MOVED` → refresh only affected proof/review/integration
- `MERGE_BLOCKED` / `NOT_PROVEN` → preserve the exact blocker or missing evidence
