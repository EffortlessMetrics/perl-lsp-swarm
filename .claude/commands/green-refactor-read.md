---
description: Green refactor step 1 — read the diff, spec files, and review comments
user-invocable: false
---

# Green Refactor: Read

Understand what was built and what can be simplified.

## Steps

1. Check out the PR branch:
   ```bash
   gh pr checkout <number>
   ```
   > **MCP alternative (web/no-gh sessions):** No direct MCP equivalent for `gh pr checkout`. In a worktree: `git fetch origin pull/<number>/head:<branch> && git checkout <branch>`.

2. Read the full diff:
   ```bash
   git diff origin/master..HEAD
   ```

3. Read the spec files:
   ```bash
   cat .spec/*/checklist.md 2>/dev/null
   cat .spec/*/context.md 2>/dev/null
   ```

4. Read review comments for simplification hints:
   ```bash
   gh pr view <number> --json comments --jq '.comments[].body'
   ```
   > **MCP alternative (web/no-gh sessions):** `mcp__github__pull_request_read(method:"get_comments", pullNumber:<number>)` → all PR review comment bodies.

5. Run tests to confirm current green baseline:
   ```bash
   cargo test -p <crate>
   ```

6. Identify refactoring opportunities:
   - Duplicated code across functions
   - Verbose error handling that could use `?`
   - Deep nesting that could use early returns
   - Non-idiomatic patterns (`.get(0)`, manual loops, unnecessary clones)
   - Dead code (unused imports, variables, functions)
   - Visibility that's wider than needed
