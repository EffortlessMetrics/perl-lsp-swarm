# `gh` CLI → GitHub MCP tool mapping

**Issue:** [#946](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/946)
**Verified against:** the GitHub MCP server tool surface available in Claude Code
web/remote sessions (2026-07-18).

## Why this exists

The swarm control plane (`.claude/agents/*.md`, `.claude/commands/*.md`) hard-codes
the `gh` CLI for GitHub access. In the **web / remote execution environment**,
`gh` is **absent** — GitHub access is provided **only** through the GitHub MCP
server (`mcp__github__*`). There was no documented `gh`→MCP fallback, so every
GitHub-dependent step had to improvise, risking stale/incorrect GitHub state
(false-green, missed PRs).

This document is the **warn-only fallback reference** (#946's first rollout step):
when a step's `gh …` command can't run because `gh` is missing, use the MCP
equivalent below. It does **not** rewrite any agent/command body — those still say
`gh` and should point here until a later phase makes the merge-blocking steps
tool-agnostic. Detecting `gh` presence with `command -v gh` and branching to MCP is
the intended pattern; local dev (where `gh` is installed) is unaffected.

> **Scope.** This covers the commands the control plane actually uses, weighted
> toward the merge-blocking critical path (`green-ci-check`, `ops-check-queue`,
> `verify-master-green`, `scout-dedup`, `pr-create`, `swarm-status`). It is not an
> exhaustive `gh` manual.

## Mapping

All MCP tools take `owner` + `repo`. `pull_request_read`, `pull_request_review_write`,
`issue_read`, `issue_write`, and `actions_run_trigger` are **method-dispatched** —
the method is given in parentheses.

### Pull requests

| `gh` command | MCP tool (method) | Notes |
|---|---|---|
| `gh pr list` | `mcp__github__list_pull_requests` | Filter by `state`, `head`, `base`. For text/qualifier queries use `search_pull_requests`. |
| `gh pr view <n>` | `mcp__github__pull_request_read` (`get`) | PR metadata, `mergeable_state`, `draft`, head/base SHAs. |
| `gh pr diff <n>` | `mcp__github__pull_request_read` (`get_diff`) | Full unified diff. `get_files` for the changed-file list only. |
| `gh pr checks <n>` | `mcp__github__pull_request_read` (`get_check_runs`) | Individual CI check runs on the head. `get_status` for the combined commit status rollup. |
| `gh pr view <n> --comments` | `mcp__github__pull_request_read` (`get_comments`) | Issue-style comments. `get_reviews` for reviews; `get_review_comments` for inline review **threads** (with `isResolved`/`isOutdated` + thread node IDs). |
| `gh pr create` | `mcp__github__create_pull_request` | Set `draft: true` for a draft. |
| `gh pr edit <n>` | `mcp__github__update_pull_request` | Title/body/base/reviewers. |
| `gh pr ready <n>` | `mcp__github__update_pull_request` (`draft: false`) | No dedicated "ready" tool — flip the draft flag. |
| `gh pr close <n>` | `mcp__github__update_pull_request` (`state: closed`) | |
| `gh pr merge <n>` | `mcp__github__merge_pull_request` | `merge_method`: `merge` / `squash` / `rebase`. |
| `gh pr update-branch <n>` | `mcp__github__update_pull_request_branch` | Update the PR branch from base. |
| `gh pr comment <n>` | `mcp__github__add_issue_comment` | A PR **is** an issue for top-level comments — pass the PR number as `issue_number`. |
| `gh pr review <n>` | `mcp__github__pull_request_review_write` (`create`, `event`: `APPROVE`/`REQUEST_CHANGES`/`COMMENT`) | Omit `event` to open a pending review, then `add_comment_to_pending_review` + `submit_pending`. |
| reply to an inline review comment | `mcp__github__add_reply_to_pull_request_comment` | `commentId` = numeric ID from the `#discussion_r…` anchor. |
| resolve / unresolve a review thread | `mcp__github__resolve_review_thread` / `unresolve_review_thread` (or `pull_request_review_write` `resolve_thread`/`unresolve_thread`) | `threadId` = `PRRT_…` node ID from `pull_request_read` `get_review_comments`. |
| `gh pr checkout <n>` | *(not MCP — use `git`)* | `git fetch origin <branch> && git checkout <branch>`. In worktree flows the branch is already checked out. |

### Issues

| `gh` command | MCP tool (method) | Notes |
|---|---|---|
| `gh issue list` | `mcp__github__list_issues` | Filter by `labels`, `state`. For text/qualifier queries use `search_issues`. |
| `gh issue view <n>` | `mcp__github__issue_read` (`get`) | `get_comments`, `get_labels`, `get_sub_issues`, `get_parent` for the rest. |
| `gh issue comment <n>` | `mcp__github__add_issue_comment` | |
| `gh issue create` | `mcp__github__issue_write` (`create`) | Apply labels via the `labels` array here. |
| `gh issue edit <n>` | `mcp__github__issue_write` (`update`) | Labels, state (`state` + `state_reason`), body, assignees. |
| `gh issue close <n>` | `mcp__github__issue_write` (`update`, `state: closed`, `state_reason`) | `duplicate` closes need `duplicate_of`. |

### CI / Actions

| `gh` command | MCP tool (method) | Notes |
|---|---|---|
| `gh run list` | `mcp__github__actions_list` (`list_workflow_runs`) | Filter by `head_sha`/branch. `list_workflow_jobs` for a run's jobs. |
| `gh run view <id>` | `mcp__github__actions_get` (`get_workflow_run`) | Run details (`get_workflow_job` for one job). `get_job_logs` (a separate tool, with `failed_only` + `tail_lines`) for logs; `get_check_run` (separate tool) for a single check run. |
| `gh run rerun <id>` | `mcp__github__actions_run_trigger` (`rerun_workflow_run`) | `rerun_failed_jobs` to re-run only failed jobs; `cancel_workflow_run` to cancel; `run_workflow` for `workflow_dispatch`. |

### Repo / misc

| `gh` command | MCP tool (method) | Notes |
|---|---|---|
| `gh search issues …` / `gh search prs …` | `mcp__github__search_issues` / `search_pull_requests` | Use the `sort`/`order` params, **not** `sort:` inside the query string. |
| `gh api repos/…/contents/…` | `mcp__github__get_file_contents` | For a file/dir blob. |
| `gh api repos/…/commits/…` | `mcp__github__get_commit` / `list_commits` | |
| `gh repo view` | `mcp__github__search_repositories` / `get_file_contents` | No single "repo view" tool; read what you need. |
| `gh api graphql` (review threads) | `mcp__github__resolve_review_thread` / `pull_request_read` (`get_review_comments`) | The common review-thread GraphQL calls have dedicated tools; there is no general GraphQL passthrough. |

## Gaps — operations with **no** direct MCP equivalent

These are the cases where a `gh`→MCP swap is **not** one-to-one. A step relying on
them must be adapted, not merely translated:

- **`gh label create`** — there is **no** label-creation MCP tool (only
  `get_label` reads one). Labels can be **applied** to an issue/PR via
  `issue_write`'s `labels` array, but a brand-new label must already exist in the
  repo (create it out-of-band, e.g. via `docs/handoff/swarm-pack/setup.sh:367`
  which already guards on `command -v gh`). Steps that assume they can mint a label
  on demand will silently fail in web/MCP sessions.
- **Arbitrary `gh api …`** — there is no general REST/GraphQL passthrough. Only the
  specific endpoints wrapped by a `mcp__github__*` tool are reachable. `gh api`
  calls to unwrapped endpoints have no fallback and must be re-expressed against a
  wrapped tool or dropped.
- **`gh pr checkout` / any local-git operation** — MCP is remote-only; use `git`
  directly (the harness allows `git`).

## How to use this from a control-plane step (warn-only)

Until the merge-blocking steps are made tool-agnostic, follow this pattern rather
than assuming `gh`:

```bash
if command -v gh >/dev/null 2>&1; then
  gh pr view "$PR" --json mergeable,statusCheckRollup   # local dev
else
  echo "::notice::gh absent (web/MCP session) — use mcp__github__pull_request_read (get / get_status)."
  echo "See docs/reference/gh-to-mcp-mapping.md"
fi
```

The MCP tools return structured JSON directly, so the parsing that followed a
`gh … --json …` call usually simplifies (no `jq` needed). Prefer `minimal_output`
and pagination (5–10 items) on `list_*`/`search_*` tools to keep results small.
