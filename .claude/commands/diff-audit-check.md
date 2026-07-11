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
   > **MCP alternative (web/no-gh sessions):** skip `gh repo view` — substitute `effortlessmetrics`/`perl-lsp-swarm` directly. For the file list: `mcp__github__pull_request_read(method:"get_files", owner, repo, pullNumber:<number>)`. For base ref: `mcp__github__pull_request_read(method:"get", owner, repo, pullNumber:<number>)` → `.baseRefName`. The `git diff` and `git merge-base` commands are local-only and always available in worktree agents. See [GH_MCP_FALLBACK.md](../../docs/reference/GH_MCP_FALLBACK.md).

   Before flagging any file as cross-PR contamination, confirm it appears in the
   `pulls/N/files` API response as PR-authored. If it only appears in a
   branch-vs-base diff, it is inherited base state, not scope drift. This
   self-check is mandatory before any SCOPE DRIFT or CONTAMINATION verdict.

2. Read the spec:
   ```bash
   gh pr checkout <number>
   cat .spec/*/acceptance.md 2>/dev/null
   ```
   > **MCP alternative (web/no-gh sessions):** `gh pr checkout` has no MCP equivalent — it is a local git operation. In `isolation:worktree` agents the PR branch is already checked out; obtain the head branch name from `mcp__github__pull_request_read(method:"get", owner, repo, pullNumber:<number>)` → `.headRefName`, then `git fetch origin <headRefName> && git checkout <headRefName>` if not already on it.

3. Check each acceptance criterion against the diff — is it implemented?

4. Search for leftover artifacts:
   ```bash
   BASE=$(gh pr view <number> --json baseRefName -q .baseRefName)
   git diff "$(git merge-base "origin/$BASE" HEAD)"..HEAD | grep -iE "TODO|FIXME|HACK|XXX|dbg!|println!|eprintln!|#\[allow"
   ```
   > **MCP alternative (web/no-gh sessions):** for base ref: `mcp__github__pull_request_read(method:"get", owner, repo, pullNumber:<number>)` → `.baseRefName`; the `git diff` and `grep` are local operations.

5. Check commit history coherence:
   ```bash
   BASE=$(gh pr view <number> --json baseRefName -q .baseRefName)
   git log "$(git merge-base "origin/$BASE" HEAD)"..HEAD --oneline
   ```
   > **MCP alternative (web/no-gh sessions):** for base ref same as above; alternatively `mcp__github__pull_request_read(method:"get_commits", owner, repo, pullNumber:<number>)` lists all PR commits without needing local git history.

6. Verify tests still pass (catch late-commit regressions):
   ```bash
   cargo test -p <crate>
   ```

7. Check PR metadata:
   ```bash
   gh pr view <number> --json title,isDraft,labels
   ```
   > **MCP alternative (web/no-gh sessions):** `mcp__github__pull_request_read(method:"get", owner, repo, pullNumber:<number>)` → `.title`, `.isDraft`, `.labels` fields. See [GH_MCP_FALLBACK.md](../../docs/reference/GH_MCP_FALLBACK.md).
