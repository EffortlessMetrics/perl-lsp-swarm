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
gh pr list --state open --limit 50 --json number,title,headRefOid,baseRefOid,isDraft,mergeStateStatus
```

Read the issue body, linked specification, current PR, checks, reviews, and
receipts before editing. A stale comment or historical count is evidence to
reconcile, not current authority.

## 2. Triage one candidate

Record one candidate packet containing its issue, PR, head and base SHAs, draft
state, mergeability, review/thread state, required checks, and known gaps.

```bash
gh pr view <PR> --json number,state,headRefOid,baseRefOid,isDraft,mergeable,mergeStateStatus,reviews,statusCheckRollup
gh pr checks <PR>
```

Use the live PR, review, thread, check, and ruleset facts for lifecycle decisions.
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
| Runner, disk, billing, or capacity failure | environment/capacity blocked; record `NOT_PROVEN` |
| Timeout or no output | `NOT_PROVEN` unless a receipt proves termination and result |
| Rate-limited or partial GitHub data | `NOT_PROVEN`; never treat an empty response as green |
| Behind-only, conflict-free candidate | leave it untouched until a material decision requires refresh |

Preserve the real exit code, termination class, stdout/stderr references, and
candidate identity for local commands. A filtered pipeline or a later successful
command must not mask an earlier failure.

## 6. Finish and clean the lane

Run proof proportional to the changed seam. At minimum, use the repository's
format and diff checks plus the affected package or policy proof. For a PR, verify
the current review receipt and hosted checks against the current head before
protected squash merge.

After merge, update the issue body with the final receipt, remaining non-goals,
and next owner. Remove only worktrees, branches, and scratch artifacts created by
the lane after confirming they are clean and no longer needed.

## 7. Fresh-session handoff

A new session must be able to recover from GitHub and repository artifacts alone.
Leave the issue or PR with:

- the exact observation or merge SHA;
- commands and proof results;
- current owner and next bounded action;
- explicit `NOT_PROVEN` gaps;
- links to the governing spec, receipt, review, and merged PR;
- the claim boundary and non-goals.

Do not preserve raw polling logs or create a second status database. The durable
issue, PR, checks, reviews, receipts, worktree, and branch state are the handoff.
