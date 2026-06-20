# Context: Fix ReferenceEdge cloning waste via Arc wrapping

## Problem Statement

The `ReferenceIndex` in `crates/perl-workspace/src/semantic/references.rs` creates a single `ReferenceEdge` struct per occurrence, then clones it multiple times:
1. Once per insertion into `references_by_name` (acceptable — one entry per symbol key)
2. Once per target candidate when inserting into `references_by_entity` (wasteful — N copies for N targets)

For a reference with 5 possible targets, the ReferenceEdge is cloned 5 times into the entity index. Each clone duplicates the `symbol_key: String` and `target_candidates: Vec<EntityId>`, multiplying memory usage by the average target candidate count.

**Scale impact**: 10,000 references × 3 target candidates × ~100B per ReferenceEdge = ~3MB of pure clone redundancy.

## Root Cause Analysis

The `ReferenceEdge` struct contains:
- `symbol_key: String` (allocation)
- `target_candidates: Vec<EntityId>` (allocation)
- Other fields (OccurrenceId, AnchorId, FileId, OccurrenceKind, Provenance, Confidence)

When the same ReferenceEdge is stored in multiple buckets of `references_by_entity`, each bucket holds a full duplicate of the entire struct.

## Proposed Solution: Arc<ReferenceEdge>

Store references as `Arc<ReferenceEdge>` instead of `ReferenceEdge`:
- Arc (Atomic Reference Counted) pointer is cheap to clone (just pointer + atomic increment)
- All existing code that dereferences ReferenceEdge works unchanged via Deref
- Eliminates N-fold duplication where N = average target_candidates

### Key Properties of Arc<T>

1. **Deref trait**: Arc<T> automatically dereferences to T, so `.symbol_key` still works
2. **Clone is cheap**: Clone only increments the reference count (atomic op), doesn't clone the inner T
3. **Thread-safe**: Arc uses AtomicUsize for ref counting (vs Rc which uses Cell<usize>)
4. **Indirection cost**: One extra dereference on lookup (~1-2% cache miss risk on typical hardware)

### Tradeoff Analysis

- **Gain**: Eliminate redundant allocations. For a 1K-file workspace with 8 target candidates per reference on average, saves ~8x memory in the reference index.
- **Cost**: One extra pointer dereference per field access. On modern CPUs with L1/L2 cache, typically <1% performance impact.
- **Verdict**: Favorable tradeoff for workspace scales.

## Alternatives Rejected

### 1. Store Vec<EntityId> separately
**Why rejected**: Would require splitting ReferenceEdge into a fixed part + Vec reference. Complicates the semantic fact model (OccurrenceFact → ReferenceEdge should be 1:1 even with multiple targets).

### 2. Normalize target_candidates per entity
**Why rejected**: Would split a single OccurrenceFact into multiple ReferenceEdges (one per target). Breaks the invariant that a single Reference carries the complete set of candidates.

### 3. Use Rc instead of Arc
**Why rejected**: perl-lsp queries references from multiple threads. Rc (non-atomic) is not thread-safe. Arc is required.

## Verification Strategy

1. **Behavior preservation**: All queries return identical results (API doesn't change, just storage)
2. **Unit tests**: Existing tests in references.rs must pass without modification (Deref handles Arc transparency)
3. **Memory profile**: Large-scale test should show reduction in B/ref
4. **Concurrency**: Parallel get_by_entity calls must not race or corrupt

## Affected Code Paths

**File**: `crates/perl-workspace/src/semantic/references.rs`

- **Lines 27-31**: struct field type changes (Vec<ReferenceEdge> → Vec<Arc<ReferenceEdge>>)
- **Line 83-92**: Wrap ReferenceEdge::new() in Arc::new()
- **Line 95**: Arc clone (cheap)
- **Line 99**: Arc clone in loop (cheap)
- **Lines 128-135**: Query methods return &[Arc<ReferenceEdge>] (Deref still works)

## No Breaking Changes

- Public API (methods) unchanged: `get_by_name`, `get_by_entity` still return slices
- Existing code that calls these methods still dereferences Arc<ReferenceEdge> transparently
- Tests do not need modification (Deref trait)
- No changes to dependent crates (API is internal to ReferenceIndex)

## Risk Assessment

**Blast radius**: One file, internal API only. No downstream crate imports ReferenceIndex's storage details.

**Hazards to cover**:
- Memory safety: Arc is well-tested Rust stdlib, no manual memory management risk
- Concurrency: Arc is thread-safe with atomic ref counting
- Performance: Extra indirection is measurable but acceptable tradeoff

## Prior Art / Learning

Arc wrapping for deduplication is a standard Rust pattern. See:
- Rust stdlib Arc documentation: https://doc.rust-lang.org/std/sync/struct.Arc.html
- Idiom: Use Arc when the same allocation is shared across multiple containers
