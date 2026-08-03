# Session operations

This runbook describes how to resume and close a provider-native engineering
lane from repository and GitHub state. It is an operator guide, not a scheduler,
reservation system, or workflow database.

## 1. Start from durable state

Use a clean, short-path worktree for the selected claim. Preserve unrelated
worktrees and untracked receipts.

```bash
git fetch origin main
git status --short --branch
git worktree list
git rev-parse origin/main
gh pr list --state open --limit 200 --json number,title,headRefOid,baseRefOid,isDraft,mergeStateStatus
```

Use the configured provider-native GitHub connector when the session does not
have a local `gh` surface. When a local shell is available, the commands above
are the CLI equivalent; detect the available surface before running them and
record a missing or partial surface as `NOT_PROVEN`.

Read the issue body, linked specification, current PR, checks, reviews, and
receipts before editing. A stale comment or historical count is evidence to
reconcile, not current authority.

## 2. Triage one candidate

Record one candidate packet containing its issue, PR, head and base SHAs, draft
state, mergeability, review/thread state, required checks, and known gaps.

```bash
gh pr view <PR> --json number,state,headRefOid,baseRefOid,isDraft,mergeStateStatus,reviews,statusCheckRollup
gh pr checks <PR>
```

`mergeStateStatus` is the live mergeability field used by this runbook; do not
infer readiness from the deprecated nullable `mergeable` field. Use the live PR,
review, thread, check, and ruleset facts for lifecycle decisions.
Labels may provide stable classification such as area, risk, release, blocker,
or requested attention; they do not establish assignment, review quality, build
readiness, CI truth, or merge readiness.

## 3. Execute a bounded claim

Use one accountable writer per branch and worktree. Keep the PR to one coherent
acceptance-and-rollback candidate. Before a state-changing operation, inspect:

```bash
git status --short --branch
git diff --stat
git diff
```

State the expected files, proof commands, non-goals, and claim boundary in the
issue or PR. Same-file overlap is not ownership. If a later lane has a real
textual conflict, it repairs the conflict and refreshes only affected proof and
review.

## 4. Spend context where it compresses evidence

Keep direct, narrow work in the warm root. Delegate when a child can return
bounded evidence from a high-volume surface without consuming the root's
decision context:

- CI and log triage;
- broad repository or corpus searches;
- dependency and API audits;
- external-source collection;
- failure bisection;
- independent proof adversaries.

The child returns commands, facts, references, and uncertainty. The warm root
retains decisions, contradictions, and integration. Delegation is not required
merely because attention moved between workflow stages.

## 5. Classify failures honestly

| Observation | Disposition |
| --- | --- |
| Source, test, or review defect | repair the bounded claim and rerun affected proof |
| Required check failure | inspect the check's own evidence; do not infer a code cause from the name |
| Runner, disk, or hosted-capacity failure | environment/capacity blocked; record `NOT_PROVEN` and name the affected execution surface |
| Billing, quota, or account-plan failure | account intervention required; record `NOT_PROVEN` and stop retrying the lane |
| Timeout or no output | `NOT_PROVEN` unless a receipt proves termination and result |
| Rate-limited or partial GitHub data | `NOT_PROVEN`; never treat an empty response as green |
| Behind-only, conflict-free candidate | leave it untouched until a material decision requires refresh |

Preserve the real exit code, termination class, stdout/stderr references, and
candidate identity for local commands. A filtered pipeline or a later successful
command must not mask an earlier failure.

## 6. Finish and clean the lane

Run proof proportional to the changed seam. At minimum, use the repository's
format and diff checks plus the affected package or policy proof. For a PR, run
the repository-owned convergence checker with enforcement before protected
squash merge, then verify the current review receipt and hosted checks against
the current head:

```bash
REVIEW_PROTOCOL_ENFORCE=1 scripts/ci/check-pr-review-convergence <PR> <OWNER>/<REPO>
```

The checker result is `NOT_PROVEN` when review or instrument data is incomplete;
it is never an empty-green substitute.

After merge, update the issue body with the final receipt, remaining non-goals,
and next owner. Remove only worktrees, branches, and scratch artifacts created by
the lane after confirming they are clean and no longer needed.

## 7. Fresh-session handoff

A new session must be able to recover from GitHub and repository artifacts alone.
Leave the issue or PR with:

- the exact observation SHA (the candidate head or base commit against which a
  packet/proof was captured), or the post-merge squash SHA when the lane is
  complete;
- commands and proof results;
- current owner and next bounded action;
- explicit `NOT_PROVEN` gaps;
- links to the governing spec, receipt, review, and merged PR;
- the claim boundary and non-goals.

Do not preserve raw polling logs or create a second status database. The durable
issue, PR, checks, reviews, receipts, worktree, and branch state are the handoff.
