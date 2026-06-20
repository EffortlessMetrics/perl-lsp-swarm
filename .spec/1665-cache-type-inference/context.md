# Issue 1665: Cache TypeInferenceEngine to Avoid Redundant Re-derivation

## Problem Statement

Hover and completion requests create a fresh `TypeInferenceEngine` on **every request**, even when the document version is unchanged. The semantic analyzer is already cached (`semantic_analyzer_cache` in `LspServer`), but type inference is computed fresh each time.

For a 500+ line file, each hover incurs:
1. `TypeInferenceEngine::new()` — allocates fresh builtins map
2. `.infer(ast)` — traverses entire AST
3. Redundant `ClassModelBuilder::new().build(ast)` — SemanticAnalyzer already has cached class_models

**Evidence**: 
- `hover.rs:314-315` — fresh engine on every hover
- `completion.rs:759-760` — fresh engine on every completion
- Both ignore `.infer()` errors and only build the environment

## Proposed Solution

Memoize `TypeInferenceEngine` per document using the same pattern as `semantic_analyzer_cache`:

1. Add `type_inference_engine_cache` to `LspServer` struct (similar to `semantic_analyzer_cache`)
   - Key: `(uri_normalized, content_hash)` like existing SemanticAnalyzer cache
   - Value: `Arc<TypeInferenceEngine>` (mutable interior)
   - LRU eviction at 50 entries

2. Create `get_or_build_type_engine()` method in `LspServer`
   - Mirrors `get_or_build_analyzer()` pattern
   - Reuses same content hash

3. Replace hover's `TypeInferenceEngine::new()` call with cached version
4. Replace completion's `TypeInferenceEngine::new()` call with cached version

## Alternatives Considered (from issue)

1. **Lazy type computation per variable**: Rejects because completion still needs all types
2. **Cache only in completion**: Rejects because hover is user-facing and higher-latency path
3. **Embed in SemanticAnalyzer**: Deferred — requires refactoring TypeInferenceEngine to accept pre-built class models; memoization at LspServer level is proven and simpler

## Key Design Decisions

**Why this approach wins:**
- Proven pattern (works for SemanticAnalyzer)
- Cache is transparent to callers (no API changes to TypeInferenceEngine)
- Automatic invalidation via content hash (no TTL management)
- Minimal diff (~50 lines as per issue)

**Cache coherency:**
- Use same content hash mechanism as `semantic_analyzer_cache`
- Clear cache on `didChange` via document version bump (existing mechanism)
- LRU eviction at 50 entries matches analyzer cache strategy

**Mutability:**
- `TypeInferenceEngine` has `&mut self` on `.infer()`, so cache must hold mutable interior
- Use `Cell<Option<...>>` or `RefCell<...>` for interior mutability, OR
- **Preferred**: Make `.infer()` method work with `&self` where possible, or
- Store pre-computed result (the inferred environment) rather than the engine itself

## Related Issues

- #1656 — completion latency O(n) + O(n²) (separate redundant-rebuild issue)
- #1652 — indexing throughput (workspace scale cluster)
- #1374 — incremental AST reuse
- #1373 — latency budgets

**Build order:** Can proceed independently; no dependencies on #1656 or #1652.

## Acceptance Criteria

From issue body:
- Two hovers on same variable in unchanged file reuse cached engine
- Cache is invalidated when document version changes
- Type labels for hover match before and after fix (regression test)

## Contract Impact

**PARSER_CONTRACTS.md references:** None affected; this is LSP-layer caching only.

**LSP protocol impact:** None; cache is transparent to protocol.

**API surface changes:** 
- Add `type_inference_engine_cache` field to `LspServer`
- Add `get_or_build_type_engine()` method
- No public API changes to TypeInferenceEngine

## Hazard Seeding

This is a **low-complexity memoization** (size/S, ~50 lines). The change is local to hover/completion request path and does not touch parser or semantic analyzer. 

**Subsystem**: LSP request handlers (hover, completion)  
**Risk**: Low — memoization is proven in SemanticAnalyzer, cache coherency is tied to document version (automatic), no new error paths

**Hazards to cover in acceptance.md:**
- LSP-3: Cache coherency (invalidation on document version change)
- LSP-4: Null/missing cache entry handling
- Cross-system: Type inference correctness matches non-cached path (regression test)
