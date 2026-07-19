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
   > **MCP alternative (web/no-gh sessions):** `mcp__github__pull_request_read(method:"get_files", owner, repo, pullNumber:<number>)` — returns the authoritative PR file list with patches. For `baseRefName`: `mcp__github__pull_request_read(method:"get", owner, repo, pullNumber:<number>)` → `baseRefName` field. Repo name is always `EffortlessMetrics/perl-lsp-swarm` in this session; no `gh repo view` needed.
   Before flagging any file as cross-PR contamination, confirm it appears in the
   `pulls/N/files` API response as PR-authored. If it only appears in a
   branch-vs-base diff, it is inherited base state, not scope drift. This
   self-check is mandatory before any SCOPE DRIFT or CONTAMINATION verdict.

2. Read the spec:
   ```bash
   gh pr checkout <number>
   cat .spec/*/acceptance.md 2>/dev/null
   ```
   > **MCP alternative (web/no-gh sessions):** `mcp__github__pull_request_read(method:"get", owner, repo, pullNumber:<number>)` provides PR metadata. Fetch by numeric PR ref to avoid interpolating an untrusted branch name: `git fetch origin "refs/pull/<number>/head:refs/remotes/origin/pr-<number>" && git checkout --detach "refs/remotes/origin/pr-<number>"`.

3. Check each acceptance criterion against the diff — is it implemented?

4. Search for leftover artifacts:
   ```bash
   BASE=$(gh pr view <number> --json baseRefName -q .baseRefName)
   git diff "$(git merge-base "origin/$BASE" HEAD)"..HEAD | grep -iE "TODO|FIXME|HACK|XXX|dbg!|println!|eprintln!|#\[allow"
   ```
   > **MCP alternative (web/no-gh sessions):** Get `baseRefName` via `mcp__github__pull_request_read(method:"get", owner, repo, pullNumber:<number>)`, then use the same git commands.

5. Check commit history coherence:
   ```bash
   BASE=$(gh pr view <number> --json baseRefName -q .baseRefName)
   git log "$(git merge-base "origin/$BASE" HEAD)"..HEAD --oneline
   ```
   > **MCP alternative (web/no-gh sessions):** Get `baseRefName` via `mcp__github__pull_request_read(method:"get", owner, repo, pullNumber:<number>)`, then use the same git command. Or use `mcp__github__pull_request_read(method:"get_commits", owner, repo, pullNumber:<number>)` for the commit list without needing a local checkout.

6. Verify tests still pass (catch late-commit regressions):
   ```bash
   cargo test -p <crate>
   ```

7. Check PR metadata:
   ```bash
   gh pr view <number> --json title,isDraft,labels
   ```
   > **MCP alternative (web/no-gh sessions):** `mcp__github__pull_request_read(method:"get", owner, repo, pullNumber:<number>)` — all of title, isDraft, and labels are in the response.
