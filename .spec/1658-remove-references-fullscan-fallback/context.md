# Context: #1658 — perf(lsp): references request spawns full-workspace text scan at request time

## Problem

The `textDocument/references` request handler in `crates/perl-lsp-rs/src/runtime/language/references.rs` performs full-workspace document text scans at request time as a fallback, violating the policy stated in CLAUDE.md that "request-time scans must be avoided."

**User impact:** Typing `ctrl+shift+F12` (find references) in a workspace with 100+ open files blocks the editor for up to 2 seconds while the server scans all document text. This occurs even when the workspace index has already found the references, making the fallback redundant.

**Technical details:**
- **Scan #1 (lines 282–340):** After the index returns results, the handler creates `docs_snapshot: Vec<(String, String)>` by cloning the URI and text of all open documents, then iterates them with regex patterns for both symbol name and package-qualified names.
- **Scan #2 (lines 455–523):** A second scan occurs for qualified-name symbols (e.g., `Package::sub`), again iterating all documents.
- Both scans execute in `IndexAccessMode::Full` (line 221), which runs when the workspace index is **ready**. This is the common case, not a degraded mode.
- The deadline check (2 seconds) is a band-aid, not a fix: the O(N) traversal still starts and blocks the editor thread.

## Why this approach

**Plan-reviewer ratified approach:** Remove the enhanced fallback text scans entirely and rely on the index as the authoritative source for references.

**Justification:**
1. **Index completeness:** The workspace index (`perl-workspace`) uses `index.find_refs()` and `index.find_references()` to find references pre-computed during indexing. These are correct by construction.
2. **Request-time scans violate policy:** CLAUDE.md explicitly forbids request-time scans. The fallback was added to catch "dynamic method calls" and other edge cases, but this is an index completeness problem, not a request-time search problem.
3. **Proper fix is index improvement:** If the index misses references, the fix is to improve the index (e.g., add static analysis for dynamic calls), not to scan at request time. This ensures all features (workspace/symbol, diagnostics, etc.) benefit from the fix, not just references.
4. **Degraded modes are still supported:** The same-file semantic analyzer fallback (lines 590–627) remains intact for `IndexAccessMode::Partial` and `IndexAccessMode::None`. These modes do not perform full-workspace scans.

## Alternatives rejected

- **Deadline-based cap (current approach, rejected):** Keep the 2-second deadline. Reasoning: Still O(N) with unpredictable latency; users experience variable blocking (instant on small workspaces, 2 seconds on large ones). Does not scale.

- **Lazy substring index (deferred, not rejected):** Pre-build a suffix tree or trie during workspace indexing, query at request time without full document cloning. Reasoning: Out of scope for this PR (requires `WorkspaceIndex` API changes). If index gaps are proven post-merge, file a follow-up issue for this optimization.

- **Async references with streaming (out of scope):** Split the request into indexed results (instant) + deferred text results (async). Reasoning: Requires LSP client changes and complicates the protocol. Correct the critical path first (eliminate the fallback).

## Prior art / duplicates

**Related issues in the same performance cluster:**
- #1656: completion O(n) scans — same anti-pattern (request-time scan fallback in completion handler)
- #1652: indexing throughput — prerequisite for validating index completeness
- #1668: workspace/symbol uncapped O(n) scan — parallel performance issue in symbol search

**Existing implementation:** The fallback text-scan pattern appears in:
- `handle_references_inner()` (two scans, lines 282–340 and 455–523) — target of this PR
- `on_references()` legacy handler (line 912: `self.iter_open_buffers()`) — intentional design, separate scope
- Completion handler (referenced in #1656) — separate crate/issue

**Canonical location for references:** After this PR, the canonical references implementation is:
- `handle_references_inner()` for workspace references (index-backed only, same-file fallback)
- `on_references()` for legacy/deprecated behavior (full-workspace scan, documented)

## Links

- **Issue:** #1658
- **Plan-review comment:** [Ratification pass comment](https://github.com/perl-lsp/perl-lsp-swarm/issues/1658#issuecomment-xyz) — all facts verified, approach approved
- **Related issues:** 
  - #1656 (completion O(n) scans) — same pattern, separate crate
  - #1652 (indexing throughput) — prerequisite for validating index completeness
  - #1668 (workspace/symbol uncapped O(n)) — parallel performance issue
  - #1611 (cross-package bare-name false positives) — index correctness issue
  - #1423 (Modern Specification for LSP Language Navigation) — specs for all navigation features
  - #1597 (Fix documents lock re-entry) — partial fix; addressed lock, not the scan
- **CLAUDE.md:** "CLAUDE.md forbids request-time scans" — policy citation
- **Subsystem docs:** docs/reference/SUBSYSTEM_HAZARD_DEFAULTS.md — LSP hazard defaults
- **Spec template:** docs/reference/SPEC_TEMPLATE.md — canonical spec format
- **Performance cluster epic:** #1686 (E6 Navigation theme) — depends on #1665 per ordering

## Decision boundary: Index gaps vs. request-time fallback

This PR **does not fix index completeness gaps**. If the workspace index is missing references (e.g., for dynamic method calls like `$obj->$method_name()`), removing the fallback text scan will expose that gap.

**Mitigation strategy post-merge:**
1. If users report "missing references" in workspace-level queries, file a follow-up issue (#1660 or similar) to improve index completeness.
2. Add a trace log in the issue to guide implementers: "References index returned N results for <symbol>; if this seems incomplete, improve index completeness in `perl-workspace`."
3. Do NOT re-introduce the request-time text-scan fallback; fix the index instead.

**Why index-first is correct:**
- Index improvements benefit all features (workspace/symbol, diagnostics, hover, etc.), not just references.
- Request-time scans are O(N) and unpredictable; index lookups are O(log N) or better.
- The index is the **source of truth** for navigation features; request-time searches undermine that principle.
