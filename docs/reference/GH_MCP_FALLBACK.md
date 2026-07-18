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
| list review threads | `mcp__github__pull_request_read(method:"get_review_comments", pullNumber:n)` | Returns threads with `isResolved`/`isOutdated` flags, the `PRRT_…` thread node ID, and inline comments (each with its `#discussion_r…` numeric ID) |
| reply to a review comment | `mcp__github__add_reply_to_pull_request_comment(pullNumber:n, commentId, body)` | `commentId` is the **numeric** ID from the `#discussion_r…` anchor, **not** the `PRRT_…` thread node ID |
| resolve / unresolve a review thread | `mcp__github__resolve_review_thread(threadId)` / `mcp__github__unresolve_review_thread(threadId)` | `threadId` = `PRRT_…` node ID from `get_review_comments`. **Merge gate**: `main` requires 0 unresolved threads — resolve each with a disposition reply first (see review-convergence doctrine) |
| `gh pr review <n> --approve` / `--request-changes` | `mcp__github__pull_request_review_write(method:"create", event:"APPROVE", pullNumber:n, body)` | Use `event:"REQUEST_CHANGES"` or `event:"COMMENT"` for other review types; omit `event` to open a pending review, then `add_comment_to_pending_review` + `submit_pending` |
| `gh pr ready <n>` | `mcp__github__update_pull_request(pullNumber:n, draft:false)` | Full parity |
| `gh pr update-branch <n>` | `mcp__github__update_pull_request_branch(pullNumber:n)` | Full parity |
| `gh pr create --draft` | `mcp__github__create_pull_request(head, base:"main", title, body, draft:true)` | Full parity |
| `gh pr list --state merged --base main` | `mcp__github__list_pull_requests(state:"closed", base:"main")` then filter `merged_at != null` | Merged-state filter is client-side |
| `gh run list --branch main` | `mcp__github__actions_list(method:"list_workflow_runs", workflow_runs_filter:{branch:"main"})` | Full parity: `status`, `conclusion`, `head_sha`, workflow name per run. **Previously mis-documented as "no MCP equivalent"** — corrected 2026-07 |
| `gh run list --workflow=<wf>` | `mcp__github__actions_list(method:"list_workflow_runs", resource_id:"<wf file name>")` | Pass workflow file name (e.g. `ci.yml`) as `resource_id` |
| `gh run view <id>` | `mcp__github__actions_get(method:"get_workflow_run", resource_id:"<id>")` | Full parity |
| `gh run view <id> --log-failed` | `mcp__github__get_job_logs(run_id:id, failed_only:true, return_content:true, tail_lines:500)` | Full parity; returns log tails inline. **Previously mis-documented as unavailable** — corrected 2026-07 |
| `gh run rerun <id>` | `mcp__github__actions_run_trigger(method:"rerun_workflow_run", run_id:id)` | Full parity. `method:"rerun_failed_jobs"` re-runs only failed jobs; `"cancel_workflow_run"` cancels; `"run_workflow"` fires a `workflow_dispatch`. **Previously mis-documented as "not exposed (workflow_dispatch only)"** — corrected 2026-07 after verifying the `actions_run_trigger` method enum |

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

## Known gaps (true no-equivalents)

- **`gh api` free-form calls**: no generic REST escape hatch; if an operation has no
  mapped tool above, report the limitation rather than improvising.
- **Creating a new label** (`gh label create`): the upstream github-mcp-server has a
  `label_write(method:"create")` tool, but it is **not exposed in this deployment's
  tool surface** — only `get_label` (read) and applying existing labels via
  `issue_write(labels:[...])` are available here. Verify with
  `ToolSearch("select:mcp__github__label_write")`; if it returns nothing (as of
  2026-07 it does), a brand-new label must be created out-of-band (e.g.
  `docs/handoff/swarm-pack/setup.sh`, which guards on `command -v gh`).
- **Local git operations** (`gh pr checkout`, etc.): MCP is remote-only; use `git`
  directly (the harness allows `git`).

## Doctrine

- Inline `> **MCP alternative**` notes at call sites are the enforcement surface
  (per [enforcement-over-doctrine](../concepts/enforcement-over-doctrine.md)); this map is
  the index. When adding a new `gh` call to a control-plane file, add the inline note and,
  if the mapping is new, a row here.
- Never classify GitHub state as UNKNOWN in an MCP session without first checking this
  map — the 2026-07 correction above exists because two "unavailable" claims caused
  agents to skip master-green verification that was in fact fully supported.
