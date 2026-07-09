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
   (MCP sessions: use `mcp__github__pull_request_read(method:"get_diff", owner, repo, pullNumber:<number>)` for the full authored diff, or `method:"get_files"` for file list — but prefer the `git diff` approach below for contamination analysis.)
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
> **MCP alternative (web/no-gh sessions):** hard-code `effortlessmetrics/perl-lsp-swarm` — no MCP call needed; the repo identity is fixed. | `mcp__github__pull_request_read(method:"get_files", owner, repo, pullNumber:<number>)` — full parity. | `mcp__github__pull_request_read(method:"get", owner, repo, pullNumber:<number>)` → full PR object with isDraft, mergeable, mergeStateStatus, labels, headRefOid, reviewDecision fields.
   Before flagging any file as cross-PR contamination, confirm it appears in the
   `pulls/N/files` API response as PR-authored. If it only appears in a
   branch-vs-base diff, it is inherited base state, not scope drift. This
   self-check is mandatory before any SCOPE DRIFT or CONTAMINATION verdict.

2. Read the spec:
   ```bash
   gh pr checkout <number>
   cat .spec/*/acceptance.md 2>/dev/null
   ```
> **MCP alternative (web/no-gh sessions):** `gh pr checkout` has no direct MCP equivalent. In worktrees: `git fetch origin pull/<N>/head:<branch> && git checkout <branch>` instead.

3. Check each acceptance criterion against the diff — is it implemented?

4. Search for leftover artifacts:
   ```bash
   BASE=$(gh pr view <number> --json baseRefName -q .baseRefName)
   git diff "$(git merge-base "origin/$BASE" HEAD)"..HEAD | grep -iE "TODO|FIXME|HACK|XXX|dbg!|println!|eprintln!|#\[allow"
   ```
> **MCP alternative (web/no-gh sessions):** `mcp__github__pull_request_read(method:"get", owner, repo, pullNumber:<number>)` → full PR object with isDraft, mergeable, mergeStateStatus, labels, headRefOid, reviewDecision fields.

5. Check commit history coherence:
   ```bash
   BASE=$(gh pr view <number> --json baseRefName -q .baseRefName)
   git log "$(git merge-base "origin/$BASE" HEAD)"..HEAD --oneline
   ```
> **MCP alternative (web/no-gh sessions):** `mcp__github__pull_request_read(method:"get", owner, repo, pullNumber:<number>)` → full PR object with isDraft, mergeable, mergeStateStatus, labels, headRefOid, reviewDecision fields.

6. Verify tests still pass (catch late-commit regressions):
   ```bash
   cargo test -p <crate>
   ```

7. Check PR metadata:
   ```bash
   gh pr view <number> --json title,isDraft,labels
   ```
> **MCP alternative (web/no-gh sessions):** `mcp__github__pull_request_read(method:"get", owner, repo, pullNumber:<number>)` → full PR object with isDraft, mergeable, mergeStateStatus, labels, headRefOid, reviewDecision fields.
