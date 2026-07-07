---
description: Diff auditor step 1 — review the complete PR diff for coherence and cleanliness
user-invocable: false
---

# Diff Audit: Check

Review the cumulative diff from all agents.

## Steps

1. Get the PR file list and authored diff through GitHub API metadata.
   Do not use `gh pr diff`: it shows branch-vs-current-base state and can
   produce false contamination claims on stale-base PRs.
   ```bash
   # Authoritative PR file list (PR-authored only):
   REPO=$(gh repo view --json nameWithOwner -q .nameWithOwner)
   gh api repos/$REPO/pulls/<number>/files --jq '.[].filename'
   gh api repos/$REPO/pulls/<number>/files --jq '.[] | {filename, patch: (.patch // "(binary)")}'

   # Full authored diff, only what the PR added relative to its base merge point:
   BASE=$(gh pr view <number> --json baseRefName -q .baseRefName)
   git fetch origin "$BASE"
   git diff "$(git merge-base "origin/$BASE" HEAD)"..HEAD --stat
   git diff "$(git merge-base "origin/$BASE" HEAD)"..HEAD
   ```

   > **MCP alternative (web/no-gh sessions):**
   > - File list: `mcp__github__pull_request_read(method:"get_files", pullNumber:N)` — returns `filename` and `patch` per file; repo is always `effortlessmetrics/perl-lsp-swarm` (no `gh repo view` needed)
   > - Base ref: `mcp__github__pull_request_read(method:"get", pullNumber:N)` → `.baseRefName`
   > - Authored diff: use `git diff` locally after `git fetch origin "$BASE"` (no MCP equivalent for git operations)

   Before flagging any file as cross-PR contamination, confirm it appears in the
   `pulls/N/files` API response as PR-authored. If it only appears in a
   branch-vs-base diff, it is inherited base state, not scope drift. This
   self-check is mandatory before any SCOPE DRIFT or CONTAMINATION verdict.

2. Read the spec:
   ```bash
   gh pr checkout <number>
   cat .spec/*/acceptance.md 2>/dev/null
   ```

   > **MCP alternative (web/no-gh sessions):** `gh pr checkout` has no MCP equivalent — work in a worktree with the PR branch already checked out; `.spec/` files are local and readable with the Read tool

3. Check each acceptance criterion against the diff — is it implemented?

4. Search for leftover artifacts:
   ```bash
   BASE=$(gh pr view <number> --json baseRefName -q .baseRefName)
   git diff "$(git merge-base "origin/$BASE" HEAD)"..HEAD | grep -iE "TODO|FIXME|HACK|XXX|dbg!|println!|eprintln!|#\[allow"
   ```

   > **MCP alternative (web/no-gh sessions):** `mcp__github__pull_request_read(method:"get", pullNumber:N)` → `.baseRefName`; grep artifact patterns in the `.patch` field of each file from `get_files`

5. Check commit history coherence:
   ```bash
   BASE=$(gh pr view <number> --json baseRefName -q .baseRefName)
   git log "$(git merge-base "origin/$BASE" HEAD)"..HEAD --oneline
   ```

   > **MCP alternative (web/no-gh sessions):** `mcp__github__pull_request_read(method:"get_commits", pullNumber:N)` — returns commit messages and SHAs in order

6. Verify tests still pass (catch late-commit regressions):
   ```bash
   cargo test -p <crate>
   ```

7. Check PR metadata:
   ```bash
   gh pr view <number> --json title,isDraft,labels
   ```

   > **MCP alternative (web/no-gh sessions):** `mcp__github__pull_request_read(method:"get", pullNumber:N)` — includes `title`, `draft`, and `labels` fields
