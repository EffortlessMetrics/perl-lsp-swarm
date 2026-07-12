## Current state (verified against origin/main, HEAD 25eaca807)

**Status: NOT IMPLEMENTED — greenfield feature**

- `crates/perl-parser-core/src/hir/model.rs` — `CompileEnvironment` struct **exists** (pub struct at line 1436)
- `crates/perl-parser-core/src/hir/lower.rs` — `lower_ast()` function **exists** (line 29) with single-pass per-file lowering
- `crates/perl-parser-core/src/hir/cache.rs` — **does NOT exist**
- `CompileStateKey` struct — **does NOT exist** anywhere in codebase
- `CompileCache` container — **does NOT exist** anywhere in codebase
- COMPILE_EFFECT_MODEL_VERSION constant — **exists** at `crates/perl-parser-core/src/hir/model.rs:262`
- RuntimeState caches — verified: `pod_cache`, `semantic_tokens_cache`, `module_scan_cache` (3+ existing caches with inconsistent invalidation)

## Claim check

**Issue premise:** Gate 4 (staged compilation) requires a compiler-owned cache layer with full keys, eviction policy, pressure counters, and lifecycle management. ✓ **CONFIRMED**

- **Stop rule verified:** `docs/project/COMPILER_CAPABILITY_STATUS.md` — "Do not add retained compiler caches without owner, key, cap, eviction, pressure counter, cleanup event, and regression test." ✓
- **Gate 4 epic verified:** issue #2076 lists "Compiler caches (full key + eviction)" as a required Gate 4 deliverable. ✓
- **Cache key design sound:** `{file_path, source_hash, pragma_snapshot_hash, inc_root_hash, model_version}` captures all invalidation dimensions (pragma, @INC, deps, schema). ✓
- **COMPILE_EFFECT_MODEL_VERSION already conformance-guarded:** test at `crates/perl-parser-core/tests/compile_state_layers_spec_alignment.rs:111-137` exists and fails if version changes without spec update. ✓

**No factual inaccuracies found in issue body.** Design is high-quality and load-bearing for multi-file compilation.

## Scope & plan

**This is SIZE/LARGE, not SIZE/SMALL:**
- New `cache.rs` module in `parser-core` with `CompileStateKey`, `CompileCacheEntry`, `CompileCache` types
- LRU eviction + memory pressure management
- Integration with `RuntimeState` (dependency-graph aware invalidation)
- Cross-crate plumbing into `document_access` / LSP lifecycle (open/change/close)
- 6+ regression tests covering cache hit, invalidation, dependency edges, eviction, pragma sensitivity, cleanup

**Architectural consideration — caches must reconcile:**
RuntimeState already holds 3+ caches (pod_cache, semantic_tokens_cache, module_scan_cache). Adding a 5th cache without unified eviction strategy risks inconsistent invalidation. The PR should either:
- (A) Unify keying/eviction across all 5 caches in a follow-up PR, OR
- (B) Justify why CompileCache stays separate with documented rationale

**Bootstrap dependency:** The `inc_root_hash` and `dependency_edges` fields in the cache key require facts from static-require / module-graph work (concurrent Gate-4 scope). Cache keying should degrade gracefully when the module graph is incomplete (file-local caching until facts arrive).

## Triage verdict

**BUILDER-READY / NEEDS-SPEC** — The design is sound and non-goals are clear. Issue is ready for a plan-review → implementation split. Recommend:

1. **Plan-review** validates cache-unification strategy and bootstrap degradation approach
2. **Split into 3 sequenced PRs:**
   - PR1: `CompileStateKey` in `model.rs` + key field on `CompileEnvironment` (independently testable)
   - PR2: `CompileCache` container with LRU eviction in new `cache.rs` (integrates with keying)
   - PR3: `RuntimeState` wiring + LSP lifecycle integration (integrates container)
3. Sequence under Gate-4 epic; mark as blocking for static-require correctness
4. Confirm `COMPILE_EFFECT_MODEL_VERSION` constant is reused (not redefined) to auto-invalidate on schema changes

## Side findings

None. Issue is self-contained and does not uncover upstream defects.
