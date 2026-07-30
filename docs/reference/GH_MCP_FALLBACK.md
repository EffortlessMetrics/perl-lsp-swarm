# gh → GitHub MCP Fallback Map

**Scope**: web/remote Claude Code sessions where the `gh` CLI is absent and GitHub access
goes through the GitHub MCP server (`mcp__github__*` tools). Tracks issue #946.

Local sessions may have `gh`; web/remote sessions may not. The repository does not grant or promise shell authorization through `.claude/settings.json`. Detect the available GitHub surface at runtime and use the provider-native connector or MCP equivalent when `gh` is absent.

> **Loading tools**: in web sessions MCP tool schemas are deferred. Load before calling,
> e.g. `ToolSearch("select:mcp__github__pull_request_read,mcp__github__merge_pull_request")`.

> **Branch naming**: the default branch is `main`. Older doctrine text says "master";
> always pass `main` to branch-scoped calls.

## Merge-critical operations

| gh CLI | MCP equivalent | Parity notes |
|---|---|---|
| `gh pr view <n> --json headRefOid,labels,mergeable` | `mcp__github__pull_request_read(method:"get", pullNumber:n)` | Full parity; response carries head SHA, labels, mergeable state, draft flag |
| `gh pr checks <n>` | `mcp__github__pull_request_read(method:"get_check_runs", pullNumber:n)` | Filter check runs by `head_sha` == PR head SHA to avoid stale green |
| `gh pr diff <n>` / `--name-only` | `mcp__github__pull_request_read(method:"get_diff"` / `"get_files", pullNumber:n)` | Full parity |
| `gh pr merge <n> --squash` | `mcp__github__merge_pull_request(pullNumber:n, merge_method:"squash", commit_title, commit_message)` | Full parity |
| `gh pr review <n> --approve` | `mcp__github__pull_request_review_write(method:"create", owner, repo, pullNumber:n, body:"<review>")`, then `mcp__github__pull_request_review_write(method:"submit_pending", owner, repo, pullNumber:n, event:"APPROVE", body:"<review>")` | Create the pending review, then submit it |
| `gh pr review <n> --request-changes` | `mcp__github__pull_request_review_write(method:"create", owner, repo, pullNumber:n, body:"<reason>")`, then `mcp__github__pull_request_review_write(method:"submit_pending", owner, repo, pullNumber:n, event:"REQUEST_CHANGES", body:"<reason>")` | Create the pending review, then submit it |
| `gh pr ready <n>` | `mcp__github__update_pull_request(pullNumber:n, draft:false)` | Full parity |
| `gh pr update-branch <n>` | `mcp__github__update_pull_request_branch(pullNumber:n)` | Full parity; use only for an actual conflict/interaction or current GitHub policy, not behind-only churn |
| `gh pr create --draft` | `mcp__github__create_pull_request(head, base:"main", title, body, draft:true)` | Full parity; draft is an explicit useful exception, not the default publication state |
| `gh pr list --state merged --base main` | `mcp__github__list_pull_requests(state:"closed", base:"main")` then filter `merged_at != null` | Merged-state filter is client-side |
| `gh pr list --label X --state open` | `mcp__github__search_pull_requests(query:"label:X is:open repo:<owner>/<repo>")` | Scope the search query to the repository |
| `gh run list --branch main` | `mcp__github__actions_list(method:"list_workflow_runs", workflow_runs_filter:{branch:"main"})` | Full parity: `status`, `conclusion`, `head_sha`, workflow name per run |
| `gh run list --workflow=<wf>` | `mcp__github__actions_list(method:"list_workflow_runs", resource_id:"<wf file name>")` | Pass workflow file name (e.g. `ci.yml`) as `resource_id` |
| `gh run view <id>` | `mcp__github__actions_get(method:"get_workflow_run", resource_id:"<id>")` | Full parity |
| `gh run view <id> --log-failed` | `mcp__github__get_job_logs(run_id:id, failed_only:true, return_content:true, tail_lines:500)` | Full parity; returns log tails inline |
| `gh run rerun` | `mcp__github__actions_run_trigger` (workflow_dispatch only) | Partial: re-running an existing failed run is not exposed; prefer a same-head native rerun when available rather than mutating the branch merely to trigger checks |

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

## Known gaps

- **Re-running a specific failed workflow run** (`gh run rerun <id>`): not exposed through every connector; use a provider-native rerun when available and otherwise report the limitation rather than manufacturing a new candidate revision.
- **`gh api` free-form calls**: no generic REST escape hatch; if an operation has no mapped tool above, report the limitation rather than improvising.

## Doctrine

- GitHub capability is discovered at runtime. Repository settings do not pre-authorize commands and project hooks do not enforce the development lifecycle.
- Prefer the narrow provider-native operation that preserves exact issue/PR/head identity.
- Never classify GitHub state as unknown without checking the available connector/MCP surface first.
- Do not reproduce a second query or state model when a canonical repository helper already owns the operation.
