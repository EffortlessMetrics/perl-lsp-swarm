# gh → GitHub MCP Fallback Map

**Scope**: web/remote Claude Code sessions where the `gh` CLI is absent and GitHub access
goes through the GitHub MCP server (`mcp__github__*` tools). Tracks issue #946.

Local sessions have `gh` (and `.claude/settings.json` grants `Bash(gh *)`); web/remote
sessions do **not**. Detect at runtime with `command -v gh` — if absent, use the MCP
column below. Control-plane files carry inline `> **MCP alternative**` notes at each
merge-critical `gh` call site; this page is the consolidated map and the place to fix
first when tool parity changes.

> **Loading tools**: in web sessions MCP tool schemas are deferred. Load before calling,
> e.g. `ToolSearch("select:mcp__github__pull_request_read,mcp__github__merge_pull_request")`.

> **Branch naming**: the default branch is `main`. Older doctrine text says "master";
> always pass `main` to branch-scoped calls (see AGENT_CATALOG "Base ref is `origin/main`").

## Merge-critical operations

| gh CLI | MCP equivalent | Parity notes |
|---|---|---|
| `gh pr view <n> --json headRefOid,labels,mergeable` | `mcp__github__pull_request_read(method:"get", pullNumber:n)` | Full parity; response carries head SHA, labels, mergeable state, draft flag |
| `gh pr checks <n>` | `mcp__github__pull_request_read(method:"get_check_runs", pullNumber:n)` | Filter check runs by `head_sha` == PR head SHA to avoid stale green |
| `gh pr diff <n>` / `--name-only` | `mcp__github__pull_request_read(method:"get_diff"` / `"get_files", pullNumber:n)` | Full parity |
| `gh pr merge <n> --squash` | `mcp__github__merge_pull_request(pullNumber:n, merge_method:"squash", commit_title, commit_message)` | Full parity |
| `gh pr ready <n>` | `mcp__github__update_pull_request(pullNumber:n, draft:false)` | Full parity |
| `gh pr update-branch <n>` | `mcp__github__update_pull_request_branch(pullNumber:n)` | Full parity |
| `gh pr create --draft` | `mcp__github__create_pull_request(head, base:"main", title, body, draft:true)` | Full parity |
| `gh pr list --state merged --base main` | `mcp__github__list_pull_requests(state:"closed", base:"main")` then filter `merged_at != null` | Merged-state filter is client-side |
| `gh run list --branch main` | `mcp__github__actions_list(method:"list_workflow_runs", workflow_runs_filter:{branch:"main"})` | Full parity: `status`, `conclusion`, `head_sha`, workflow name per run. **Previously mis-documented as "no MCP equivalent"** — corrected 2026-07 |
| `gh run list --workflow=<wf>` | `mcp__github__actions_list(method:"list_workflow_runs", resource_id:"<wf file name>")` | Pass workflow file name (e.g. `ci.yml`) as `resource_id` |
| `gh run view <id>` | `mcp__github__actions_get(method:"get_workflow_run", resource_id:"<id>")` | Full parity |
| `gh run view <id> --log-failed` | `mcp__github__get_job_logs(run_id:id, failed_only:true, return_content:true, tail_lines:500)` | Full parity; returns log tails inline. **Previously mis-documented as unavailable** — corrected 2026-07 |
| `gh run rerun` | `mcp__github__actions_run_trigger` (workflow_dispatch only) | Partial: re-running an existing failed run is not exposed; push an empty-diff commit or use `update_pull_request_branch` to retrigger |

## Issue / triage operations

| gh CLI | MCP equivalent | Parity notes |
|---|---|---|
| `gh issue view <n> --comments` | `mcp__github__issue_read(method:"get"` / `"get_comments")` | Full parity |
| `gh issue list --label X` | `mcp__github__list_issues(labels:["X"], state:"OPEN")` | Full parity |
| `gh issue list --search "..."` | `mcp__github__search_issues(query:"repo:<owner>/<repo> ...")` | Scope the query with `repo:` |
| `gh issue edit <n> --add-label X` | `mcp__github__issue_write(method:"update", labels:[...])` | **Labels are replaced, not appended** — read current labels first, then write the union |
| `gh issue comment <n>` | `mcp__github__add_issue_comment(issue_number:n, body)` | Full parity |
| `gh issue close <n>` | `mcp__github__issue_write(method:"update", state:"closed", state_reason)` | Always set `state_reason` |
| `gh api repos/.../commits/<sha>/check-runs` | `mcp__github__list_commits` / `mcp__github__get_commit` + `pull_request_read(method:"get_check_runs")` | Commit-scoped check-run listing goes through the PR when one exists |
| `gh search code` | `mcp__github__search_code(query:"repo:<owner>/<repo> ...")` | Stay inside session repo scope |

## PR / issue operations (label and state queries)

| gh CLI | MCP equivalent | Parity notes |
|---|---|---|
| `gh pr list --state open --label X` | `mcp__github__search_issues(query:"repo:<owner>/<repo> label:X is:pr is:open")` | GitHub's unified search covers PRs; use `is:pr` to scope. `mcp__github__list_pull_requests` has no label-filter param. |
| `gh pr list --state merged --limit N` | `mcp__github__list_pull_requests(state:"closed", perPage:N)` then filter `merged_at != null` | Merged state is client-side; see also the Merge-critical row for base-filtered variant |
| `gh pr list --state open --limit N` | `mcp__github__list_pull_requests(state:"open", perPage:N)` | Full parity when no label filter needed |
| `gh pr list --state open --json number \| jq length` | `mcp__github__list_pull_requests(state:"open", perPage:100)` → count `.length` | Pagination needed for >100 open PRs (rare in practice) |
| `gh issue list --label X --state open` | `mcp__github__list_issues(labels:["X"], state:"OPEN", perPage:50)` | Full parity; use `perPage:100` for large queues |
| `gh issue list --label X --state open --json number \| jq length` | `mcp__github__list_issues(labels:["X"], state:"OPEN", perPage:100)` → count `.length` | Full parity |
| `gh issue view <n> --json updatedAt` | `mcp__github__issue_read(method:"get", issue_number:n)` → `.updatedAt` | Full parity; also returns all other fields in one call |
| `gh pr checks <N>` | `mcp__github__pull_request_read(method:"get_check_runs", pullNumber:N)` | Full parity — already in Merge-critical table above |
| `gh repo view --json nameWithOwner` | Not needed in web sessions: owner/repo are always `effortlessmetrics`/`perl-lsp-swarm` — substitute directly. | Local sessions: `gh repo view` works. Web sessions: substitute the literal strings. |

## Comment operations

| gh CLI | MCP equivalent | Parity notes |
|---|---|---|
| `gh pr comment <n> --body "..."` | `mcp__github__add_issue_comment(issue_number:n, body:"...")` | GitHub treats PR comments as issue comments; full parity |
| `gh issue comment <n> --body "..."` | `mcp__github__add_issue_comment(issue_number:n, body:"...")` | Already in Issue/triage table above |
| `gh api repos/.../issues/comments/$ID --method PATCH` | **Known gap** — editing an existing comment has no direct MCP tool. Workaround: delete the old comment and create a new one (via `mcp__github__add_issue_comment`), or accept the limitation and post a follow-up comment instead. | |

## Local-only operations (no MCP equivalent)

| gh CLI | MCP equivalent | Notes |
|---|---|---|
| `gh pr checkout <n>` | No MCP equivalent — this is a local git operation. | In `isolation:worktree` agents the PR branch is already checked out. In non-worktree web sessions, use `git fetch origin <branch> && git checkout <branch>` if the remote branch name is known from `mcp__github__pull_request_read(method:"get")` → `.headRefName`. |
| `gh run rerun <id>` | No direct equivalent — already listed in Known gaps below | Push an empty-diff commit or `update_pull_request_branch` to retrigger |

## Known gaps (true no-equivalents)

- **Re-running a specific failed workflow run** (`gh run rerun <id>`): not exposed;
  `actions_run_trigger` only fires `workflow_dispatch`-enabled workflows. Workaround:
  `update_pull_request_branch` (if base moved) or push a rebase/empty commit.
- **`gh api` free-form calls**: no generic REST escape hatch; if an operation has no
  mapped tool above, report the limitation rather than improvising.

## Doctrine

- Inline `> **MCP alternative**` notes at call sites are the enforcement surface
  (per [enforcement-over-doctrine](../concepts/enforcement-over-doctrine.md)); this map is
  the index. When adding a new `gh` call to a control-plane file, add the inline note and,
  if the mapping is new, a row here.
- Never classify GitHub state as UNKNOWN in an MCP session without first checking this
  map — the 2026-07 correction above exists because two "unavailable" claims caused
  agents to skip master-green verification that was in fact fully supported.
