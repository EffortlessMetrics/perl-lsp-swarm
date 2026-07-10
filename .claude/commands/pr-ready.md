---
description: Mark a reviewed draft PR as ready for CI
argument-hint: "<PR number>"
---

# Mark PR Ready

Mark a reviewed draft PR as ready for merge. This triggers CI. Context: **$ARGUMENTS**

## Steps

### 1. Parse PR number

Extract the PR number from $ARGUMENTS. If not provided, list open draft PRs:
```bash
gh pr list --state open --draft --json number,title,headRefName --template '{{range .}}#{{.number}} {{.title}} ({{.headRefName}}){{"\n"}}{{end}}'
```

### 2. Verify PR exists and is a draft

```bash
gh pr view $NUMBER --json isDraft,title,state
```

If the PR is not a draft, report: "PR #N is already marked ready" and stop.
If the PR is not open, report the current state and stop.

### 3. Verify review coverage

```bash
gh pr view $NUMBER --json labels,files
```

Policy:
- If the PR has the `deep-reviewed` label, proceed.
- If it does not have `deep-reviewed`, it may proceed **only** when every changed file is docs-only (`docs/**` or doc-text files such as `.md`, `.mdx`, `.txt`, `.rst`, `.adoc`).
- Otherwise: **STOP.** Report: "PR #$NUMBER cannot be marked ready — missing `deep-reviewed` on a non-docs PR. Route to reviewer-deep first."

Optionally validate receipt freshness when `deep-reviewed` is present to ensure the deep review covers the current HEAD:
```
/label-receipt-validate pr $NUMBER deep-reviewed
```

### 3.5 Verify conversation resolution and reviewer completion

`reviewDecision` alone doesn't prove either condition below — it says nothing
about thread resolution, and a review can predate the current push. Check both:

```bash
gh pr view $NUMBER --json reviewRequests,latestReviews,headRefOid,reviewDecision
```

A reviewer only counts as finished if their review's `commit.oid` equals
`headRefOid` — only the latest review per reviewer counts (earlier reviews are
superseded). Then check every review thread is resolved (paginate if `hasNextPage` is true — see
`/pr-respond` step 4.5 for the query and pagination guidance):

```bash
gh api graphql -f pr=$NUMBER -f query='query($pr:Int!) { repository(owner:"OWNER", name:"REPO") { pullRequest(number:$pr) { reviewThreads(first: 50) { nodes { id isResolved path comments(first:1) { nodes { body } } } pageInfo { hasNextPage endCursor } } } } }'
```

**Never enable or retain auto-merge, and never mark a PR ready for merge
pickup, while any requested reviewer has not yet finished on the current HEAD
SHA or any substantive review thread remains unresolved.** Resolve threads for
a reason (fixed/refuted/superseded/accepted-with-follow-up), not
performatively — main mechanically requires conversation resolution before
merge. If `reviewRequests` is non-empty, a review's `commit.oid` is stale
relative to `headRefOid`, or any thread node has `isResolved: false`, **STOP**
and report which reviewer or thread is still pending instead of proceeding.

### 4. Mark ready and signal merge-readiness

```bash
gh pr ready $NUMBER
```

Apply the `merge-ready` label with verification (see `/label-apply-verified`):
```
/label-apply-verified pr $NUMBER "merge-ready"
```

The `merge-ready` label signals the ops agent that this PR has passed review and is cleared for merge pickup.

### 5. Write version-bound receipt

Record the label binding against the current HEAD SHA so the orchestrator can detect staleness:

```
/label-receipt-write pr $NUMBER merge-ready pr-ready
```

### 6. Report

Output: "PR #$NUMBER marked ready -- CI will trigger. Labeled merge-ready for ops pickup."

Include the PR URL for convenience:
```bash
gh pr view $NUMBER --json url --template '{{.url}}'
```
