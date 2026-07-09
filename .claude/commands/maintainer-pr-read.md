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
> **MCP alternative (web/no-gh sessions):** `mcp__github__pull_request_read(method:"get", owner, repo, pullNumber:<number>)` → full PR object with isDraft, mergeable, mergeStateStatus, labels, headRefOid, reviewDecision fields. | `mcp__github__pull_request_read(method:"get_diff", owner, repo, pullNumber:<number>)` — full unified diff.

2. Read the linked issue:
   ```bash
   ISSUE=$(gh pr view <number> --json closingIssuesReferences --jq '.closingIssuesReferences[0].number // empty')
   gh issue view "$ISSUE" --json title,body,labels,comments
   ```
> **MCP alternative (web/no-gh sessions):** `mcp__github__pull_request_read(method:"get", owner, repo, pullNumber:<number>)` → full PR object with isDraft, mergeable, mergeStateStatus, labels, headRefOid, reviewDecision fields. | `mcp__github__issue_read(method:"get", owner, repo, issue_number:<number>)` — full parity.

3. Read .spec/ files if they exist on the branch:
   ```bash
   gh pr checkout <number>
   ls .spec/*/
   cat .spec/*/acceptance.md 2>/dev/null
   cat .spec/*/context.md 2>/dev/null
   ```
> **MCP alternative (web/no-gh sessions):** `gh pr checkout` has no direct MCP equivalent. In worktrees: `git fetch origin pull/<N>/head:<branch> && git checkout <branch>` instead.

4. Check the diff scope — which crates changed?
   ```bash
   gh pr diff <number> --stat
   ```
> **MCP alternative (web/no-gh sessions):** `mcp__github__pull_request_read(method:"get_files", owner, repo, pullNumber:<number>)` — returns list of changed files with additions/deletions.
