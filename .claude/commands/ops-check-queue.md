---
description: Ops step 1 — classify exact-head PR readiness without ceremonial branch refresh
user-invocable: false
---

# Ops Check Queue

Find PRs that are ready to merge or need one precise next transition.

Canonical authorities:

- PR disposition: `docs/specs/PLSP-SPEC-0006-pr-queue-disposition.md`
- authority map: `docs/reference/CONTROL_PLANE_AUTHORITY.md`
- review convergence: `scripts/ci/check-pr-review-convergence`
- required checks: live repository policy, reconciled with `.ci/policies/required-checks.toml`

## Rules

- PR age, inactivity, or commits-behind are observations, not merge or repair
  dispositions.
- Fetch current `main` to inspect semantic interaction. Do not update the PR
  branch merely because it is behind.
- `DIRTY`/`CONFLICTING` means inspect an actual conflict. It does not mean
  automatic rebase.
- GitHub `UNKNOWN` means the mergeability claim is `NOT_PROVEN`. The queue-facing
  result label for that same condition is `UNKNOWN_NOT_PROVEN`; it does not add
  a second semantic state.
- Labels locate candidates and routing work. They are not current-head proof.
- Same-head CI refresh and base integration are separate operations.

## Steps

### 0. Review orphaned `in-build` claims without age-only mutation

List the complete bounded issue population carrying `in-build`, then inspect each
candidate's linked PR, branch/worktree ownership, and salvage state. The explicit
limit avoids the GitHub CLI's default 30-item truncation; if the repository can
exceed 500 matching claims, use the paginated API instead of silently truncating.

```bash
gh issue list --label "in-build" --state open --limit 500 \
  --json number,title,updatedAt
```

> **Connector alternative:** page through all issues with the `in-build` label,
> then inspect linked PR and ownership state. `updatedAt` may select a claim for
> review; it does not authorize label removal.

Classify:

- **Linked open PR or active ownership**: keep the claim.
- **No open PR and no current ownership evidence**: report an orphaned-claim
  candidate; remove the label only after verifying no dirty, unpublished, or
  salvageable work exists.
- **Ambiguous or unavailable ownership state**: `NOT_PROVEN`; do not mutate.

### 1. Capture live PR identity and merge state

```bash
gh pr list --state open --limit 200 \
  --json number,title,headRefOid,baseRefOid,mergeable,mergeStateStatus,isDraft,reviewDecision,labels,updatedAt
```

Record the full head SHA before interpreting checks or reviews.

### 2. Classify mergeability separately from proof

- `MERGEABLE` / `CLEAN`: no textual conflict reported; continue.
- `CONFLICTING` / `DIRTY`: inspect the exact conflict and route through
  `RESOLVE_CONFLICTS` or `REVIEW_SEMANTIC_INTERACTION`.
- `UNKNOWN`: emit `UNKNOWN_NOT_PROVEN`; retry boundedly or report. This is the
  serialized queue representation of a `NOT_PROVEN` mergeability claim.
- `UNSTABLE`: decompose required checks, advisory checks, review, and policy;
  do not treat the summary as a disposition.

A behind-only, conflict-free PR remains eligible for current-head review. Squash
merge without changing its head is allowed only when live rulesets/branch
protection impose no current-integration requirement and any applicable
integration proof is current. A real strict/up-to-date requirement is an
`UPDATE_BASE_REQUIRED` policy reason, not a general age or distance rule.

### 3. Evaluate current-head proof

For each candidate:

1. query `headRefOid` and `statusCheckRollup` together;
2. pin the returned full head SHA;
3. discover the required check set from live policy using the policy procedure
   in `.claude/agents/green-ci.md` or the canonical live collector;
4. classify every rollup entry attributable to that current head, including
   both GitHub CheckRun entries (`name`/`conclusion`) and commit StatusContext
   entries (`context`/`state`);
5. distinguish required success, pending, failed, missing, stale, cancelled,
   skipped/not-applicable, instrument failure, and advisory findings;
6. run the canonical review-convergence check;
7. re-read the head after collection.

```bash
PR_STATE=$(gh pr view <number> --json headRefOid,statusCheckRollup)
HEAD_SHA=$(printf '%s' "$PR_STATE" | jq -r '.headRefOid')
printf '%s' "$PR_STATE" | jq -r --arg head "$HEAD_SHA" '
  .statusCheckRollup[] |
  {
    kind: (.__typename // "unknown"),
    name: (.name // .context),
    state: (.conclusion // .state // .status),
    head_sha: (.headSha // .sha // $head),
    started_at: .startedAt,
    completed_at: .completedAt,
    details_url: (.detailsUrl // .targetUrl)
  }'
scripts/ci/check-pr-review-convergence <number>
```

`statusCheckRollup` is the repository's combined current-head status contract;
querying only `check-runs` would omit legacy/external commit-status contexts.
The emitted `head_sha` preserves an audit trail to the pinned head even when a
rollup entry does not expose its own SHA. For duplicate or failed entries,
inspect the focused underlying run/status only as needed; do not replace the
combined rollup with one status system.

> **Connector alternative:** fetch the PR's combined status rollup or equivalent
> current-head check-run plus commit-status contexts, together with review
> threads and requested reviews. Keep all evidence bound to the captured head
> and re-read it afterward.

Do not use `update-branch`, a merge-main commit, rebase, force-push, or an empty
commit solely to obtain missing proof. Request a same-head rerun/dispatch when
supported; otherwise report `NOT_PROVEN`. A genuine integration requirement is
routed separately.

### 4. Respect routing labels without treating them as proof

A `needs-*` label means a named repair is still requested. A contradictory
`merge-ready` plus `needs-*` projection should be reconciled before merge.

Do not infer semantic supersession or branch freshness from any label.

### 5. Emit one bounded result

Use these queue results:

- **MERGE NOW**: expected head is unchanged; not draft; mergeable; required
  exact-head checks succeed; review convergence succeeds; live policy and any
  applicable integration proof permit squash merge.
- **WAIT**: a named required input is pending.
- **BLOCKED**: a deterministic product, review, policy, or conflict finding
  prevents integration.
- **CONFLICTING**: actual textual conflict requires inspection; no automatic
  resolution strategy is chosen.
- **UNKNOWN_NOT_PROVEN**: GitHub/tool state cannot establish the answer; this is
  the queue-facing form of `NOT_PROVEN` for mergeability.
- **RETURN TO REVIEW**: the head moved after evidence was collected; discard the
  stale authorization and evaluate the new head.
- **ADVISORY**: non-required concern remains visible without becoming a merge
  requirement.

## Output

```text
MERGE NOW: #NNN @ <full-head-sha>
WAIT: #NNN (exact required input)
BLOCKED: #NNN (finding and next action)
CONFLICTING: #NNN (files/seam to inspect)
UNKNOWN_NOT_PROVEN: #NNN (missing state/tool)
RETURN TO REVIEW: #NNN (old head -> new head; stale evidence discarded)
ADVISORY: #NNN (non-required concern)
```
