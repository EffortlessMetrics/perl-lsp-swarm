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

> **MCP alternative (web/no-gh sessions):** `mcp__github__pull_request_read(method:"get", pullNumber:<number>)` for title/body/labels/files; `mcp__github__pull_request_read(method:"get_diff", pullNumber:<number>)` for full diff

2. Read the linked issue:
   ```bash
   ISSUE=$(gh pr view <number> --json closingIssuesReferences --jq '.closingIssuesReferences[0].number // empty')
   gh issue view "$ISSUE" --json title,body,labels,comments
   ```

> **MCP alternative (web/no-gh sessions):** `mcp__github__pull_request_read(method:"get", pullNumber:<number>)` — extract `closingIssuesReferences[0].number`; then `mcp__github__issue_read(method:"get", issue_number:<ISSUE>)` for body/labels and `mcp__github__issue_read(method:"get_comments", issue_number:<ISSUE>)` for comments

3. Read .spec/ files if they exist on the branch:
   ```bash
   gh pr checkout <number>
   ls .spec/*/
   cat .spec/*/acceptance.md 2>/dev/null
   cat .spec/*/context.md 2>/dev/null
   ```

> **MCP alternative (web/no-gh sessions):** `mcp__github__pull_request_read(method:"get", pullNumber:<number>)` to get the branch name; then `mcp__github__get_file_contents(path:".spec/...")` to read spec files directly from the branch

4. Check the diff scope — which crates changed?
   ```bash
   gh pr diff <number> --stat
   ```

> **MCP alternative (web/no-gh sessions):** `mcp__github__pull_request_read(method:"get_files", pullNumber:<number>)` — lists changed files with additions/deletions
