# Session operations

This runbook describes how to resume and close a provider-native engineering lane from repository and GitHub state. It is an operator guide, not a scheduler, reservation system, receipt lifecycle, or workflow database.

## 1. Start from durable state

Use a clean, short-path worktree for the selected claim. Preserve unrelated worktrees and untracked receipts.

```bash
git fetch origin main
git status --short --branch
git worktree list
git rev-parse origin/main
gh pr list --state open --limit 200 --json number,title,headRefOid,baseRefOid,isDraft,mergeStateStatus
```

Use the provider-native GitHub connector when the session lacks a local `gh` surface. Record missing or partial data as `NOT_PROVEN`.

Read the issue body, linked specification, current PR, checks, submitted reviews, and threads before editing. A stale comment or historical count is evidence to reconcile, not current authority.

## 2. Triage one candidate

Record one bounded candidate packet containing its issue, PR, current branch/head identity, draft state, mergeability, open findings, required checks, and known gaps.

```bash
gh pr view <PR> --json number,state,headRefOid,baseRefOid,isDraft,mergeStateStatus,reviews,statusCheckRollup
gh pr checks <PR>
```

Use live PR, thread, review, check, and ruleset facts. Labels may classify area, risk, release, blocker, or requested attention; they do not establish build, review, CI, or merge readiness.

The current head SHA identifies current code and check results. It is not a review-validity token.

## 3. Execute a bounded claim

Use one accountable writer per branch and worktree. Keep the PR to one coherent acceptance-and-rollback candidate. Before state-changing operations, inspect:

```bash
git status --short --branch
git diff --stat
git diff
```

State expected files, proof, non-goals, and claim boundary. Same-file overlap is not ownership. If a later lane has a real conflict, it repairs the conflict and refreshes only affected proof and review.

## 4. Spend context where it compresses evidence

Keep direct, narrow work in the warm root. Delegate when a child can return bounded evidence from a high-volume surface without consuming the root's decision context:

- CI and log triage;
- broad repository or corpus searches;
- dependency and API audits;
- external-source collection;
- failure bisection;
- independent proof adversaries.

The child returns facts, references, contradictions, and uncertainty. The warm root retains decisions and integration.

## 5. Classify failures honestly

| Observation | Disposition |
| --- | --- |
| Source, test, or review defect | repair the bounded claim and rerun affected proof |
| Required check failure | inspect the check's evidence; do not infer a code cause from the name |
| Runner, disk, or hosted-capacity failure | environment/capacity blocked; record `NOT_PROVEN` |
| Billing, quota, or account-plan failure | account intervention required; stop retrying the lane |
| Timeout or no output | `NOT_PROVEN` unless a receipt proves termination and result |
| Rate-limited or partial GitHub data | `NOT_PROVEN`; never treat an empty response as green |
| Behind-only, conflict-free candidate | leave it untouched |

Preserve real exit code, termination class, output references, and candidate identity. A filtered pipeline or later command must not mask an earlier failure.

## 6. Review and finish the lane

Run proof proportional to the changed seam. Review is cumulative and semantic:

- publish useful findings or a concise clean conclusion;
- reply with evidence before resolving a substantive thread;
- after repair, verify the finding, proof, and changed seam;
- revisit broader claim, production path, authority, risk, rollback, or compatibility only when the repair changes them;
- do not restart a full review merely because the commit SHA changed;
- do not post `Review pass (...) at head ... and claim ...` comments.

Before protected squash merge, verify live GitHub facts:

- PR is ready;
- required checks are current;
- no unresolved thread remains;
- no current change request remains;
- any deliberately requested review is complete;
- mergeability, rulesets, queue state, and applicable release/changelog policy permit merge.

Use the current head only as compare-and-swap protection at merge time:

```bash
gh pr merge <PR> --squash --match-head-commit <CURRENT_HEAD>
```

This prevents racing a moving branch; it does not make review currentness depend on the SHA.

After merge, update the issue body with the landed effect, remaining non-goals, and next owner. Remove only lane-created worktrees, branches, and scratch artifacts after confirming they are clean and no longer needed.

## 7. Fresh-session handoff

A new session must recover from GitHub and repository artifacts alone. Leave the issue or PR with:

- repository, branch, candidate/tree and relevant base or landed identity;
- commands, results, environment, and stable versioned evidence/run references;
- original review subjects, directed passes or independent actor reviews, findings,
  dispositions, and remaining uncertainty;
- current writer, next bounded action, and any acknowledged ownership transfer;
- explicit `NOT_PROVEN` gaps, holds and their sources, and pending wake conditions;
- links to governing specs, evidence, and merged PRs;
- claim boundary, non-goals, and useful retained worktree/cache/artifact locations.

Resume the existing candidate when its identity and mutation owner are established.
Preserve useful local changes and evidence under the existing
[worktree protocol](../reference/WORKTREE_PROTOCOL.md). A quiet or replaced session
does not by itself transfer writer ownership; follow the reassignment rule in
`orchestrate-work` ([Codex](../../.agents/skills/orchestrate-work/SKILL.md#a-quiet-worker-is-not-a-result),
[Claude](../../.claude/skills/orchestrate-work/SKILL.md#a-quiet-agent-is-not-a-result)).
Checkpointing or ending a session does not authorize deletion of retained work or
caches.

Keep original run and review identities when carrying evidence forward, and state
why it still applies. A replacement session does not make that evidence a new
execution or invalidate it; refresh only affected evidence under
[review currentness](../agents/REVIEW_CURRENTNESS.md). Resume directed staged review
inline in the owning session's main context; a reset does not require another reviewer,
external handoff, or routine parent payload approval. Preserve any actual independent
actor or GitHub approval requirement. Confirm that the receiving context can read the
needed artifacts before treating the handoff as complete; a
sender-local path alone does not establish recovery.

Do not preserve raw polling logs, head/claim receipt comments, or a second status database. The durable issue, PR, checks, reviews, threads, worktree, and branch state are the handoff.
