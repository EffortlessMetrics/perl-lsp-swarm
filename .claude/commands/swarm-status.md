---
description: Show current swarm state — open PRs, issues, metrics summary, queue depth
argument-hint: "[--full]"
---

# Swarm Status

Aggregate current swarm state. Context: **$ARGUMENTS**

## Quick View

```bash
echo "=== Open PRs ==="
gh pr list --state open --json number,title,labels --limit 30

echo "=== Discovered Issues ==="
gh issue list --label "swarm-discovered" --state open --limit 20

echo "=== Architectural Decisions Needed ==="
gh issue list --label "swarm-architectural" --state open

echo "=== Recent Merges (last 24h) ==="
gh pr list --state merged --limit 20 --json number,title,mergedAt

echo "=== Queue Depth ==="
gh issue list --label "builder-ready" --state open --json number --jq length
gh issue list --label "needs-plan-review" --state open --json number --jq length
gh pr list --label "merge-ready" --state open --json number --jq length

echo "=== Metrics Dashboard (last 24h) ==="
cargo xtask swarm-summary .ops-perl-lsp --since 24h --limit 10
cargo xtask swarm-summary .ops-perl-lsp --since 24h --limit 10 --format json
```

> **MCP alternative (web/no-gh sessions):**
> - Open PRs: `mcp__github__list_pull_requests(owner, repo, state:"open", perPage:30)`
> - Discovered issues: `mcp__github__list_issues(owner, repo, labels:["swarm-discovered"], state:"OPEN", perPage:20)`
> - Architectural issues: `mcp__github__list_issues(owner, repo, labels:["swarm-architectural"], state:"OPEN")`
> - Recent merges: `mcp__github__list_pull_requests(owner, repo, state:"closed")` then filter for non-null `mergedAt` within last 24h; exclude PRs closed without merging
> - Queue depth (builder-ready): `mcp__github__list_issues(owner, repo, labels:["builder-ready"], state:"OPEN")`
> - Queue depth (needs-plan-review): `mcp__github__list_issues(owner, repo, labels:["needs-plan-review"], state:"OPEN")`
> - merge-ready PRs: `mcp__github__search_pull_requests(query:"label:merge-ready is:open repo:effortlessmetrics/perl-lsp-swarm")`
>
> See [docs/reference/GH_MCP_FALLBACK.md] for the full mapping.

## Full View (`--full`)

Also includes:
```bash
echo "=== Metrics Dashboard (last 7d) ==="
cargo xtask swarm-summary .ops-perl-lsp --since 7d --limit 25
cargo xtask swarm-summary .ops-perl-lsp --since 7d --limit 25 --format json

echo "=== Recent Issues Filed ==="
gh issue list --label "swarm-discovered" --state open --limit 20 --json number,title

echo "=== Worktrees ==="
git worktree list
```
> **MCP alternative (web/no-gh sessions):** Recent discovered issues: `mcp__github__list_issues(owner, repo, labels:["swarm-discovered"], state:"OPEN", perPage:20)` — returns number and title fields. Worktrees: `git worktree list` works unchanged.
