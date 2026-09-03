---
name: merge-reconcile
description: Require current substantive review and live integration before squash merge, then reconcile the landed or deliberately closed root-held claim without exact-head ceremony.
user-invocable: false
---

# Merge and reconcile

This is the irreversible edge for one root-held claim frame. It may be invoked directly,
so it must reconstruct or consume both predecessor judgments rather than assuming another
skill already ran.

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

For an open PR, establish the current Claude-native substantive result before reading
integration green as permission to merge.

Read cumulative submitted reviews and useful clean conclusions, localized findings and
dispositions, the current candidate claim/production route/proof/limitations, and any
material changes since the useful review.

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

- `REVIEW_REQUIRED` → `finish-pr` / `final-challenge` / `orchestrate-work` /
  `review-pr`;
- `CHANGES_REQUIRED` → `address-review-comments` with one writer, affected proof, and
  affected re-review;
- `NOT_PROVEN` → preserve the missing or contradictory evidence;
- `BLOCKED_BY_PREREQUISITE` → preserve the exact prerequisite and wake event;
- `SUPERSEDED_OR_CLOSE` → reconcile the evidence-backed disposition;
- only `REVIEW_CURRENT` may continue to live integration.

The main Claude thread owns this cumulative judgment for the claim frame. A bounded
review context does not acquire merge authority.

## Integration predecessor

After `REVIEW_CURRENT`, invoke or reconstruct `verify-live-ci` for the current candidate
and classify:

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

When the required union is still pending, arm auto-merge with the current head SHA:

```text
gh pr merge <n> --auto --squash --match-head-commit <current-head-sha>
```

The command request is not evidence that GitHub persisted the transition. Immediately
re-read the live PR. Return `PR_IN_FLIGHT` only after a fresh GitHub read confirms a
non-null `autoMergeRequest` for the same PR, the unchanged current head, and the squash
method. If the request is absent, the head moved, or the method differs, return
`MERGE_BLOCKED` or `NOT_PROVEN` with the observed state; do not report auto-merge as
armed.

A confirmed auto-merge request leaves `REVIEW_CURRENT` intact and returns
`PR_IN_FLIGHT`. The read-back and current-head compare-and-swap prevent a command success
or stale branch observation from stranding the claim behind a transition GitHub never
accepted. That prevents racing a moving branch. It does not make review currentness
depend on the SHA.

If the head moves before merge, re-read the candidate. Refresh only proof, review, and
integration dimensions affected by the new commit. Never use administrative bypass to
discover what is failing or to outrun unresolved review/integration evidence.

If an armed auto-merge has not fired, one manual probe merge through the REST endpoint
(`gh api -X PUT repos/{owner}/{repo}/pulls/<n>/merge -f merge_method=squash -f
sha=<current-head-sha>`) is the sanctioned no-polling probe only after the required
union is green on the head SHA, or after an explicit evidence-backed waiver is recorded
on the PR/issue naming every unmet requirement. A waiver recorded merely to save
wall-clock is not a waiver.

The **main/accountable root** records any such waiver, owns the compare-and-swap
transition, and either merges or arms auto-merge. A bounded writer/reviewer/subagent does
not inherit this authority merely because it handled the claim's candidate.

For automation-authored PRs whose `pull_request` runs remain `action_required`, green
`workflow_dispatch` runs on the same head do not substitute for required PR contexts.
Use an actual trusted approval/identity path or preserve integration as `NOT_PROVEN`.

## Reconciliation

After merge or evidence-backed deliberate closure:

1. verify the landed/current-main effect where applicable;
2. update or close the controlling issue accurately;
3. keep umbrella goals open when only one predicate landed;
4. update durable contracts, proof, support claims, and changelog only within the
   proven boundary;
5. preserve partial or residual work explicitly;
6. release the claim's worktree through the allocator/main thread that owns it;
7. update the root-held claim frame and expose the next coherent claim to `deliver-goal`.

Release local mutation resources on every terminal outcome—merged, superseded,
deliberately closed, or abandoned—not only the merge path. An abandoned claim reports
its typed `ABANDONED`/`EXTERNAL_BLOCKER`/`NOT_PROVEN` result to the main thread; there is
no subordinate orchestrator that must remain alive to represent it.

Release belongs to whoever allocated/controls the worktree, not to the writer standing
inside it. Keep a worktree only when it holds state that exists nowhere else:
uncommitted changes, unpushed commits, or detached useful state outside the base. A
fully pushed open PR is reconstructable with `git worktree add`.

Use `bash scripts/cleanup-completed-worktrees.sh --dry-run` to classify safe residue
before applying cleanup. Do not persist runtime topology, task state, or merge-check
polling as closeout evidence.

## Supersession carries its corrections

When one candidate replaces another, the replacement inherits the superseded candidate's
findings, dispositions, and corrections. Carry forward:

- corrections to claims the replacement still makes;
- accepted findings not yet repaired, with the evidence behind each disposition;
- limitations and `NOT_PROVEN` boundaries still applying to the replacement;
- revalidated dispositions against the replacement candidate.

Where the replacement's claim differs, state which corrections no longer apply and why
rather than dropping them silently.

## Results and routes

- `RECONCILED` → `deliver-pr` or `deliver-goal`
- `PARTIAL` → preserve remaining acceptance in the root-held claim frame
- `SUPERSEDED` / `DELIBERATELY_CLOSED` → preserve the durable disposition
- `REVIEW_REQUIRED` → `finish-pr` / `review-pr`
- `CHANGES_REQUIRED` → `address-review-comments`
- `PR_IN_FLIGHT` → update the claim frame with the wake event and return to `deliver-goal`
- `CANDIDATE_MOVED` → refresh only affected proof/review/integration
- `MERGE_BLOCKED` / `NOT_PROVEN` → preserve the exact blocker or missing evidence
