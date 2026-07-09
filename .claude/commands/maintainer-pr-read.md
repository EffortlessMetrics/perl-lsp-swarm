---
description: Maintainer vision (PR) step 1 — read the PR diff, issue spec, and .spec/ files
user-invocable: false
---

# Maintainer PR: Read

Understand what was built and whether it matches the project's direction.

## Steps

1. Read the PR:
   ```bash
   gh pr view <number> --json title,body,labels,files
   gh pr diff <number>
   ```
   > **MCP alternative (web/no-gh sessions):** `mcp__github__pull_request_read(method:"get", pullNumber:<number>)` → `.title`, `.body`, `.labels`, `.files`; `mcp__github__pull_request_read(method:"get_diff", pullNumber:<number>)` → full diff text.

2. Read the linked issue:
   ```bash
   ISSUE=$(gh pr view <number> --json closingIssuesReferences --jq '.closingIssuesReferences[0].number // empty')
   gh issue view "$ISSUE" --json title,body,labels,comments
   ```
   > **MCP alternative (web/no-gh sessions):** `mcp__github__pull_request_read(method:"get", pullNumber:<number>)` → `.closingIssuesReferences[0].number` for the issue number; then `mcp__github__issue_read(method:"get", issue_number:<ISSUE>)` + `mcp__github__issue_read(method:"get_comments", issue_number:<ISSUE>)`.

3. Read .spec/ files if they exist on the branch:
   ```bash
   gh pr checkout <number>
   ls .spec/*/
   cat .spec/*/acceptance.md 2>/dev/null
   cat .spec/*/context.md 2>/dev/null
   ```
   > **MCP alternative (web/no-gh sessions):** No direct MCP equivalent for `gh pr checkout`. In a worktree: `git fetch origin pull/<number>/head:<branch> && git checkout <branch>`, then read spec files with the Read tool.

4. Check the diff scope — which crates changed?
   ```bash
   gh pr diff <number> --stat
   ```
   > **MCP alternative (web/no-gh sessions):** `mcp__github__pull_request_read(method:"get_files", pullNumber:<number>)` → list of changed files with additions/deletions.
