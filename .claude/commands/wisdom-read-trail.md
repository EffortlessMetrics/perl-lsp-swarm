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

   > **MCP alternative (web/no-gh sessions):** `mcp__github__issue_read(method:"get", issue_number:N)` for the body; `mcp__github__issue_read(method:"get_comments", issue_number:N)` for the comments

2. Read the plan-reviewer's comment (if any):
   ```bash
   gh issue view <number> --json comments --jq '.comments[] | select(contains("Plan Review"))'
   ```

   > **MCP alternative (web/no-gh sessions):** `mcp__github__issue_read(method:"get_comments", issue_number:N)` then filter for comments containing "Plan Review" in agent code

3. Read the PR description and diff:
   ```bash
   gh pr view <pr-number> --json body --jq '.body'
   gh pr diff <pr-number>
   ```

   > **MCP alternatives (web/no-gh sessions):**
   > - PR body: `mcp__github__pull_request_read(method:"get", pullNumber:N)` → `.body` field
   > - PR diff: `mcp__github__pull_request_read(method:"get_diff", pullNumber:N)`

4. Read review comments:
   ```bash
   gh api repos/{owner}/{repo}/pulls/<pr-number>/comments --jq '.[].body'
   gh pr view <pr-number> --json reviews --jq '.reviews[].body'
   ```

   > **MCP alternatives (web/no-gh sessions):**
   > - Inline review comments: `mcp__github__pull_request_read(method:"get_review_comments", pullNumber:N)` — each thread includes body, path, line
   > - Review summaries: `mcp__github__pull_request_read(method:"get_reviews", pullNumber:N)` → `.body` per review

5. Read the merged code:
   ```bash
   gh pr view <pr-number> --json mergeCommit --jq '.mergeCommit.oid'
   git show <commit> --stat
   ```

   > **MCP alternative (web/no-gh sessions):** `mcp__github__pull_request_read(method:"get", pullNumber:N)` → `mergeCommit.oid` if present; `mcp__github__pull_request_read(method:"get_commits", pullNumber:N)` for the commit list

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
