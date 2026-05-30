---
description: Scout step 1 — check if this finding is already tracked
user-invocable: false
---

# Scout Dedup Check

Before investigating, verify this isn't already covered.

> **Tooling note:** Use the GitHub **MCP** tools for all issue/PR search — this swarm runs in
> environments where the `gh` CLI is unavailable (e.g. Claude Code on the web). Load them once per
> session with:
> `ToolSearch` → `select:mcp__github__search_issues,mcp__github__search_pull_requests,mcp__github__list_issues`

## Steps

1. **Check the local blockers ledger** for known active issues in this area:
   ```bash
   python3 -c "
   import yaml, sys
   data = yaml.safe_load(open('.ci/blockers.yaml'))
   topic = sys.argv[1].lower() if len(sys.argv) > 1 else ''
   for section in ['parser_blockers', 'lsp_limitations', 'ci_blockers']:
       for entry in data.get(section, []):
           if any(topic in str(v).lower() for v in entry.values()):
               print(f\"KNOWN: {entry['id']} -> issue {entry.get('issue','none')} ({entry.get('status','?')})\")
   " "<your topic keywords>" 2>/dev/null || echo "(blockers.yaml not yet available)"
   ```
   ⚠️ `blockers.yaml` is **manually maintained and may be stale** (counts are from its `last_verified`
   date, *system corpus only*). Treat a match as a lead to verify against `.ci/parser-corpus-baseline.json`
   (system) and `.ci/cpan-corpus-baseline.json` (CPAN) — not as ground truth. `status: fixed` entries
   with stale counts are common; don't report a fixed/clean bucket as live work.

2. **Search open issues** via MCP — you MUST pass `owner` and `repo`:

   `mcp__github__search_issues` with `owner: "effortlessmetrics"`, `repo: "perl-lsp-swarm"`,
   `query: "<topic keywords>"`.

   - ⚠️ **A query with no `owner`/`repo` returns `total_count: 0`.** That is the tool's behavior, **not**
     "no results found." If you ever get `0` on a broad term, you almost certainly dropped the scope
     params — sanity-check by first searching a term you *know* exists (a crate or feature name) before
     trusting any zero result.

3. **Search open PRs**: `mcp__github__search_pull_requests` with the same `owner`/`repo` + `query`.

4. **Search recently closed/merged** (it might already be done): `mcp__github__search_issues` with
   `owner`/`repo` and `query: "<topic keywords> is:closed"`. If a broad search still looks thin, fall
   back to `mcp__github__list_issues` (`owner`/`repo`, `state: CLOSED`, `orderBy: UPDATED_AT`,
   `direction: DESC`) and scan recent titles.

## Decision

- **Duplicate found**: mark this step completed, note the existing issue/PR number, STOP scouting.
  Report: "Already tracked as #NNN."
- **Related but different**: note the related issue and continue. Dedup by **failure mode / source
  surface / user-visible behavior / intended fix** — *never* by shared file, theme, helper, or base
  commit alone. Two issues touching the same file are **not** duplicates if they have different failure
  modes or different fixes.
- **No match**: continue investigating.
