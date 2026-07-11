# Review Convergence — Canonical Authority

**Script:** [`scripts/ci/check-pr-review-convergence`](../../scripts/ci/check-pr-review-convergence)

Every skill or agent that needs to know "has this PR converged on review?"
calls this script. Nothing else re-derives the query. This doc is the
human-readable companion; the script's own header comment is the
authoritative contract — read it first if the two ever disagree.

## Why this exists

Three review rounds on this repo's control-plane fixes (culminating in PR
#3621, which addressed #3598) re-introduced the same defect in five
different files: `pr-ready.md`, `pr-respond.md`, `ops-merge-batch.md`,
`pr-responder.md`, and `ops.md` each computed "is review done?" with their
own hand-copied GraphQL query. The queries drifted from each other every
time one surface was fixed and the others weren't. The fix is not "fix the
query again" — it's "make it impossible to have more than one query."

## The contract

Review convergence is **not** the same question as `reviewDecision`.
`reviewDecision` (APPROVED / CHANGES_REQUESTED / REVIEW_REQUIRED) answers
"what did GitHub compute from review states" — it does not tell you:

- whether a reviewer's approval is stale (predates the current push), or
- whether any review **thread** is still open.

Both are separate, necessary conditions. A PR is only review-converged when
**all** of the following hold, as of the PR's current `headRefOid`:

1. **No pending review requests.** `reviewRequests` is empty — nobody who
   was asked to review has failed to respond at all.
2. **No stale human reviews.** Every non-bot reviewer's *latest* review
   (`latestReviews`, never the full historical `reviews` connection —
   earlier submissions are superseded and don't count) has
   `commit.oid == headRefOid`. A reviewer who approved three commits ago has
   not reviewed the current code. **Bot-authored reviews are excluded from
   this check** (author `__typename == "Bot"` — sourcery-ai, coderabbitai,
   chatgpt-codex-connector, cubic-dev-ai, factory-droid, etc.): they're
   unrequested auto-review apps, never appear in `reviewRequests`, aren't
   required by branch protection, and don't reliably re-run on every push
   (rate limits). A stale bot review is reported as **ADVISORY**, not
   `BLOCK`. This was found by running the script against #3621's own PR:
   blocking on bot staleness meant the PR could never converge again once
   any auto-review app fell behind a push, without a human manually
   re-triggering it — the exact over-block class this script exists to
   eliminate. **Note:** stale bot reviews are now the *only* advisory-only
   (non-blocking) category the script reports — unresolved review threads
   (item 3, below) block regardless of `isOutdated`, so "the same
   treatment as an outdated thread" is no longer an accurate parallel to
   draw; see issue #3679.
3. **No unresolved threads — active or outdated.** Every `reviewThreads`
   node with `isResolved == false` blocks convergence, regardless of
   `isOutdated`. `isOutdated` (the diff hunk a thread commented on no
   longer exists at HEAD) is reported as metadata — it splits the
   unresolved count into `unresolved_active` and `unresolved_outdated` —
   but it is **not** a blocking discriminator. Both categories emit
   `BLOCK`.

   This was previously wrong: an earlier version of this script treated
   `isOutdated == true` unresolved threads as **ADVISORY**, non-blocking,
   reasoning that a thread pointing at code that no longer exists doesn't
   need to gate a machine verdict. That reasoning doesn't match GitHub's
   actual mergeability computation — this repo's active `main`
   branch-protection ruleset enforces `required_conversation_resolution`,
   which GitHub applies to **every** unresolved thread and does not consult
   `isOutdated` at all. PR #3621 proved this directly: it sat `BLOCKED` for
   many review cycles with 0 active threads but 9 outdated-unresolved
   threads; resolving those 9 (no other change) made the merge fire
   immediately. The old behavior meant this script could report
   `converged:true` on a PR GitHub would still refuse to merge — the exact
   "internal verdict disagrees with GitHub mergeability" defect class this
   script exists to eliminate. See issue #3679.
4. **No resolved thread lacking a disposition reply.** `0 unresolved
   threads` is necessary but **not sufficient** — #3647 merged out-of-band,
   while its independent correctness review was still running, with 6 P1
   defects live on main, because a responder silently `resolveReviewThread`'d
   15 threads with **no reply at all**. The ruleset enforces "resolved"; it
   cannot enforce "resolved for a justified reason." This script closes the
   mechanical half of that gap: for every `reviewThreads` node with
   `isResolved == true`, it checks `comments.totalCount`. A thread whose
   `totalCount <= 1` (only the original review comment, nobody — bot,
   human, or the original author — ever replied before it was resolved) is
   the mechanical signature of resolved-to-clear, reported as
   `resolved_without_disposition` and `BLOCK`ed.

   **Heuristic, not intent detection — documented limits:**
   - A thread the *original* reviewer resolves immediately after posting a
     genuine one-line follow-up ("fixed in a1b2c3d") is **not** flagged —
     that reply is the second comment, so `totalCount` is 2. The check is
     "did anyone say anything after the finding," not "did someone else."
   - A low-effort reply ("ok") that isn't a real disposition still passes —
     the script can verify a reply's *existence*, not its *content*. See
     issue #3693 for the `Disposition: fixed|refuted|superseded|follow-up`
     reply-format convention this check assumes reviewers/responders
     follow (content-quality enforcement is a human/reviewer judgment
     call, not a mechanical one).
   - `totalCount` is connection metadata, accurate independent of how many
     comment `nodes` are actually fetched (the script only fetches
     `first: 1`, for the opening commenter's login).
5. **No independent review in flight.** The `needs-deep-review` label is
   treated as a durable, repo-visible blocking marker — while present,
   `independent_review_pending` is `true` and convergence is `BLOCK`ed
   regardless of thread/review state. This makes an in-flight correctness
   review mechanically visible: the other half of the #3647 root cause was
   that the PR merged while its deep review was still running, with that
   review existing only in an orchestrator's task list — not in the repo.

## Pagination

Both `reviewThreads` and `latestReviews` are paginated GraphQL connections.
The script pages through both with `hasNextPage`/`endCursor` until
exhausted — never trust a bare `first: 50` on either field for a PR that
might have more than 50 threads or more than 50 review submissions.

## Usage

```bash
scripts/ci/check-pr-review-convergence <pr-number> [owner/repo]
```

- Exit `0` — converged (review-wise; this does **not** check CI, labels, or
  draft state — those are separate gates).
- Exit `1` — not converged; `BLOCK`/`ADVISORY` reasons on stderr, plus a
  human-readable `FAIL` summary line and a machine-readable JSON object on
  stdout.
- Exit `2` — usage or fetch error (not a verdict either way).

### JSON output shape

```json
{"pr": N, "headRefOid": "...", "converged": true|false,
 "pending_reviewers": [...], "stale_reviews": [...], "stale_bot_reviews": [...],
 "unresolved_active": N, "unresolved_outdated": N, "unresolved_total": N,
 "resolved_threads": N, "resolved_without_disposition": N,
 "independent_review_pending": true|false}
```

`unresolved_active` and `unresolved_outdated` both block convergence;
`unresolved_total` is their sum. `resolved_without_disposition` (count of
resolved threads with `comments.totalCount <= 1` — see item 4 above) and
`independent_review_pending` (the `needs-deep-review` label — item 5 above)
both block convergence too. `stale_bot_reviews` is the only non-blocking
(ADVISORY) count in the object.

### Test seam (no network)

Set `CONVERGENCE_TEST_FIXTURE_DIR=<dir>` (plus an explicit `owner/repo` as
arg 2) to make the script read canned JSON fixture files
(`pr_view.json`, `latestReviews.json`, `reviewThreads.json`) instead of
calling `gh`. This is exercised by
`scripts/tests/test-check-pr-review-convergence.sh` and must never be set
outside of tests.

## Where it's called from

- `/pr-ready` step 3.5 (verify conversation resolution and reviewer
  completion before marking a draft ready)
- `/pr-respond` step 4.5 (before signaling `pr-responded` / requesting
  re-review)
- `/ops-merge-batch` step 2 (fresh green check, immediately before merge)
- `.claude/agents/pr-responder.md` (before treating the PR as ready)
- `.claude/agents/ops.md` (principles section — never merge/auto-merge
  while unconverged)

If you are adding a new surface that needs this check, call the script —
do not write a new GraphQL query for it, even if it looks like "just this
one field."
