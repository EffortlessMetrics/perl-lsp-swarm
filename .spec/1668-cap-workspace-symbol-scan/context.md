# Context: #1668 — perf(workspace-symbol): cap O(n) search_source_symbols scan

## Problem

The workspace/symbol request handler performs an **uncapped full-workspace symbol scan**, collecting and cloning **all matching symbols** before applying the result cap. For large workspaces (1000+ files, 10k+ symbols), a common query like 'new' or 'get' can match 500+ symbols. This incurs **O(n*m) work** (n=symbols, m=query string length via contains check), with the cap applied only after the full scan completes.

**User impact:**
- Typing 'workspace/symbol' request in a large CPAN-like workspace (10k+ files) blocks the editor for 100-500ms while the full scan completes
- Responsive LSP servers should return initial results within 50ms
- Large monorepos with 50k+ symbols exhibit noticeable latency on every symbol search

**Evidence:**
- `crates/perl-lsp-rs/src/runtime/workspace.rs:290-296` — search functions called without cap, results cloned, cap applied after
- `crates/perl-workspace/src/workspace/workspace_index.rs:2917-2936` — search_source_symbols returns ALL matches, no early exit
- `crates/perl-workspace/src/workspace/workspace_index.rs:2943-3001` — search_generated_workspace_symbols same uncapped behavior

## Why this approach

The **early-exit cap parameter** approach wins because:

1. **Low-cost incremental fix**: Add cap parameter to two functions + early-exit logic (~10-15 lines per function)
2. **Immediate latency relief**: Stops scanning once cap is reached, eliminating O(n) cloning cost
3. **Minimal API surface change**: Only two public function signatures change; no new types or protocol changes
4. **No cache invalidation complexity**: Unlike request-scoped caching, no state to maintain across calls
5. **Aligns with LSP semantics**: Cap is already part of the LSP spec; applying it at search boundary matches protocol intent

## Alternatives rejected

- **Request-scoped cache** (cache last N requests by query string hash):
  - Rejected because: Adds cache invalidation complexity; only helps if user types same prefix twice; insufficient for initial symptom relief
  
- **Trie-based prefix index** (index symbols in a trie for O(k log n) prefix queries):
  - Rejected because: Significant refactor to WorkspaceIndex; out of scope for this fix but should be filed as follow-up epic
  - Note: Issue #1686 mentions trie index as future optimization

- **Post-scan deduplication optimization** (fix O(n²) Vec::remove in completion):
  - Rejected because: This is a separate issue (#1656); both can proceed in parallel but target different code paths
  - Note: Completion handler also calls search_source_symbols but has different postprocessing concerns

## Prior art / duplicates

No existing implementation found. The workspace/symbol handler (workspace.rs:290-296) is the canonical implementation of this pattern. The completion handler (per #1656) has a similar O(n) scan problem but different postprocessing (deduplication vs capping).

**Related issues in the cluster:**
- #1656 (completion latency O(n) scan + O(n²) dedup) — separate code path, same root cause class
- #1652 (initial indexing throughput) — different phase (index build vs query)
- #1514 (workspace/symbol determinism) — multi-root ordering, not performance
- #1665 (TypeInferenceEngine re-derivation) — navigation engine, not symbol search

**Follow-up work (not blocking):**
- #1686 (epic: trie-based prefix index) — future optimization for 100k+ symbol workspaces

## Links

- **Issue:** #1668
- **Plan-review comment:** https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/1668#issuecomment-4757222779 (ratification summary)
- **Scout comments:** Issue #1668, comments by EffortlessSteven (scout finding, scout linking)
- **Related:**
  - #1656 (completion dedup O(n²) → O(n))
  - #1652 (initial indexing throughput)
  - #1514 (workspace/symbol determinism)
  - #1665 (TypeInferenceEngine re-derivation — E6 Navigation dependency)
  - #1686 (epic: navigation responsiveness — mentions trie index follow-up)
- **Subsystem:** perl-workspace (symbol indexing), perl-lsp-rs (LSP handler)
- **Hazard defaults:** docs/reference/SUBSYSTEM_HAZARD_DEFAULTS.md — LSP (LSP-1 through LSP-4)
- **Performance pattern:** Shift-left ladder — move cap enforcement earlier in the pipeline (search boundary vs post-collection)

## Dependency order

**Blocks:** None explicitly, but E6 Navigation theme (epic #1686) lists this as prerequisite for trie-index follow-up.

**Blocked by:** #1653 (string part search optimization) — must fix O(n) regex search before tackling workspace-symbol scanning. This is a sequential dependency noted in the issue comments.

**Independent from:** #1656 (completion dedup) — both target symbol search but different call sites and postprocessing.

## Test strategy (for red TDD builder)

The acceptance.md §Test-Grid defines 14 test cases across positive/negative/adversarial/performance categories. Key adversarial tests:

1. **Off-by-one boundary tests** — cap=200 with 199 and 201 matches
2. **Large workspace latency** — 10k symbol index with cap=200 must complete <5ms
3. **State transition coherence** — cap applied consistently during Building→Ready→Degraded transitions
4. **Combined source + generated** — ensure combined search respects single cap, not per-source cap

The builder should write these tests **before** implementing the feature (red-TDD discipline), then implement to make them green.
