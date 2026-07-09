---
description: Wisdom step 1 — read the full issue→PR→merge trail
user-invocable: false
---

# Wisdom: Read Trail

Read everything that happened in this change's lifecycle.

## Steps

1. Read the issue (the scout's investigation):
   ```bash
   gh issue view <number> --json body,comments --jq '{body: .body, comments: [.comments[].body]}'
   ```
   > **MCP alternative (web/no-gh sessions):** `mcp__github__issue_read(method:"get", issue_number:<number>)` for body; `mcp__github__issue_read(method:"get_comments", issue_number:<number>)` for comments.

2. Read the plan-reviewer's comment (if any):
   ```bash
   gh issue view <number> --json comments --jq '.comments[] | select(contains("Plan Review"))'
   ```
   > **MCP alternative (web/no-gh sessions):** `mcp__github__issue_read(method:"get_comments", issue_number:<number>)` → filter in agent code for comments containing "Plan Review".

3. Read the PR description and diff:
   ```bash
   gh pr view <pr-number> --json body --jq '.body'
   gh pr diff <pr-number>
   ```
   > **MCP alternative (web/no-gh sessions):** `mcp__github__pull_request_read(method:"get", pullNumber:<pr-number>)` → `.body` for description; `mcp__github__pull_request_read(method:"get_diff", pullNumber:<pr-number>)` for the diff.

4. Read review comments:
   ```bash
   gh api repos/{owner}/{repo}/pulls/<pr-number>/comments --jq '.[].body'
   gh pr view <pr-number> --json reviews --jq '.reviews[].body'
   ```
   > **MCP alternative (web/no-gh sessions):** `mcp__github__pull_request_read(method:"get_review_comments", pullNumber:<pr-number>)` for inline diff comments; `mcp__github__pull_request_read(method:"get_reviews", pullNumber:<pr-number>)` for review summaries.

5. Read the merged code:
   ```bash
   gh pr view <pr-number> --json mergeCommit --jq '.mergeCommit.oid'
   git show <commit> --stat
   ```
   > **MCP alternative (web/no-gh sessions):** `mcp__github__pull_request_read(method:"get", pullNumber:<pr-number>)` → `.mergeCommit.oid`; then `mcp__github__get_commit(owner, repo, sha:<commit>)` for the commit stats.

## Output

Record in your task:
```
Issue: #NNN — scout's analysis
Plan review: <what was refined>
Builder: <what was built, what was noted>
Reviewer: <what was caught, what was fixed forward>
Reviewer-deep: <what edge cases, what follow-ups>
Final state: <what merged>
```
