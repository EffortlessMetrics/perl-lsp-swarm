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
- `UNKNOWN` means `NOT_PROVEN`; retry boundedly or report it.
- Labels locate candidates and routing work. They are not current-head proof.
- Same-head CI refresh and base integration are separate operations.

## Steps

### 0. Review orphaned `in-build` claims without age-only mutation

List issues carrying `in-build`, then inspect each candidate's linked PR,
branch/worktree ownership, and salvage state.

```bash
gh issue list --label "in-build" --state open --json number,title,updatedAt
```

> **MCP alternative (web/no-gh sessions):** list issues with the `in-build`
> label, then inspect linked PR and ownership state. `updatedAt` may select a
> claim for review; it does not authorize label removal.

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
- `UNKNOWN`: `UNKNOWN_NOT_PROVEN`; retry boundedly or report.
- `UNSTABLE`: decompose required checks, advisory checks, review, and policy;
  do not treat the summary as a disposition.

A behind-only, conflict-free PR remains eligible for current-head review and
squash merge without changing its head.

### 3. Evaluate current-head proof

For each candidate:

1. pin `headRefOid`;
2. discover the required check set from live policy;
3. read check runs attributable to that exact head;
4. distinguish required success, pending, failed, missing, stale, cancelled,
   skipped/not-applicable, instrument failure, and advisory findings;
5. run the canonical review-convergence check;
6. re-read the head after collection.

```bash
HEAD_SHA=$(gh pr view <number> --json headRefOid --jq .headRefOid)
gh api repos/:owner/:repo/commits/$HEAD_SHA/check-runs --paginate \
  --jq '.check_runs[] | {name,status,conclusion,head_sha,details_url}'
scripts/ci/check-pr-review-convergence <number>
```

> **MCP alternative (web/no-gh sessions):** fetch the PR, exact-head check
> runs, review threads, and requested reviews through the canonical connector
> mappings. Keep all evidence bound to the captured head.

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
- **UNKNOWN_NOT_PROVEN**: GitHub/tool state cannot establish the answer.
- **RETURN TO REVIEW**: the head moved after evidence was collected.
- **ADVISORY**: non-required concern remains visible without becoming a merge
  requirement.

## Output

```text
Merge candidates: #NNN @ <full-head-sha>
Waiting: #NNN (exact required input)
Blocked: #NNN (finding and next action)
Conflicting: #NNN (files/seam to inspect)
Not proven: #NNN (missing state/tool)
Advisory: #NNN (non-required concern)
```
