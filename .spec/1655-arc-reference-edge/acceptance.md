# Acceptance Criteria: Arc-wrap ReferenceEdge to eliminate clone waste

## §Behavior

| Input/Condition | Expected Result | Test Name |
|---|---|---|
| Adding file with one reference, one target candidate | ReferenceIndex stores one Arc<ReferenceEdge> in references_by_name and one in references_by_entity | `add_file_populates_name_index` |
| Adding file with one reference, three target candidates | ReferenceIndex stores one Arc<ReferenceEdge> in references_by_name but the *same Arc* three times in references_by_entity (not three clones) | `multiple_edge_targets_produce_multiple_entity_entries` |
| Querying by name | Returns slice of Arc<ReferenceEdge> with identical occurrence_id, anchor_id, symbol_key as original ReferenceEdge | `add_file_populates_name_index` |
| Querying by entity | Returns slice of Arc<ReferenceEdge> with identical target_candidates as original ReferenceEdge | `add_file_populates_entity_index` |
| Removing file | All references from that file are removed from both indexes | `remove_file_clears_entries` |
| Parallel get_by_entity calls on Arc-wrapped references | No data races, no corruption, consistent snapshot semantics | `reference_index_concurrent_access` (new) |

## §Hazards

| Hazard Class | Surface | Impact | Mitigation |
|---|---|---|---|
| **Memory Safety** | Arc<ReferenceEdge> allocation / deallocation | Ref count underflow could cause use-after-free | Use std::sync::Arc (battle-tested, atomic ref counting verified by Rust compiler) |
| **Concurrency** | Multiple threads calling get_by_entity simultaneously | Data race on Arc ref counting | Arc uses AtomicUsize (sync primitive), safe across threads |
| **Performance Regression** | Extra indirection on field access (Arc → ReferenceEdge → field) | 1-2% cache miss risk on lookups | Acceptable tradeoff for N-fold memory savings. Measure with profiler if needed. |
| **API Surface Change** | Changing Vec<ReferenceEdge> to Vec<Arc<ReferenceEdge>> | Existing callers expect direct struct, not pointer | Deref trait makes Arc<T> transparently behave as &T. No API change visible to callers. |
| **Storage Semantics** | Shared Arc across multiple indexes | Multiple indexes hold pointers to same allocation | Correct by design: same ReferenceEdge should be shared, not cloned |
| **Test Coverage** | Existing tests assume non-Arc storage | Tests may fail if snapshot expectations change | Tests operate on Arc<ReferenceEdge> via Deref transparently. No test changes needed. |

## §Contracts

| Contract | Surfaces | Binding |
|---|---|---|
| **ReferenceEdge struct definition** (perl-semantic-facts crate) | `pub struct ReferenceEdge { symbol_key: String, target_candidates: Vec<EntityId>, ... }` | Import Arc<ReferenceEdge> to wrap. No change to struct itself. |
| **ReferenceIndex public API** | `fn get_by_name(&self, key: &str) -> &[ReferenceEdge]` | Return type changes to `&[Arc<ReferenceEdge>]`. Deref coercion makes this transparent to callers. |
| **ReferenceIndex public API** | `fn get_by_entity(&self, id: EntityId) -> &[ReferenceEdge]` | Return type changes to `&[Arc<ReferenceEdge>]`. Deref coercion makes this transparent to callers. |
| **Remove pattern** (lines 115-124) | `refs.retain(\|r\| r.file_id != file_id)` | Works unchanged: Arc<T> dereferences to T, so .file_id still resolves |
| **Rust std::sync::Arc** | AtomicUsize ref counting, Deref impl, Clone trait | Required stdlib behavior for thread-safe shared pointers |

## §API-Shape

### New Types / Changes

| Item | Type | Change | Rationale |
|---|---|---|---|
| `ReferenceIndex::references_by_name` | `HashMap<String, Vec<Arc<ReferenceEdge>>>` | `Vec<ReferenceEdge>` → `Vec<Arc<ReferenceEdge>>` | Enable cheap sharing |
| `ReferenceIndex::references_by_entity` | `HashMap<EntityId, Vec<Arc<ReferenceEdge>>>` | `Vec<ReferenceEdge>` → `Vec<Arc<ReferenceEdge>>` | Enable cheap sharing |
| `ReferenceIndex::get_by_name` return type | `&[Arc<ReferenceEdge>]` | `&[ReferenceEdge]` → `&[Arc<ReferenceEdge>]` | Reflects storage; Deref coercion transparent to callers |
| `ReferenceIndex::get_by_entity` return type | `&[Arc<ReferenceEdge>]` | `&[ReferenceEdge]` → `&[Arc<ReferenceEdge>]` | Reflects storage; Deref coercion transparent to callers |

### Public Callers of Modified API

Search codebase for calls to `get_by_name` and `get_by_entity`:

```bash
rg "\.get_by_name\(|\.get_by_entity\(" --type rust
```

Expected: Low caller count, concentrated in LSP providers that iterate over the result slice. Deref coercion makes Arc<ReferenceEdge> behave identically to ReferenceEdge for iteration and field access.

### Duplication Risk

No name collisions: `Arc` is from std::sync, widely used. No custom Arc type defined locally.

## §Test-Grid

| Test Class | Positive | Negative | Adversarial / Edge Case | State Transition | Test Name | Invariant Verified |
|---|---|---|---|---|---|---|
| **Add file to index** | Add file with 1 reference, 1 target | Add file with empty occurrences | File with unresolved reference (no edge targets, empty target_candidates) | Idle → 1 file indexed | `add_file_populates_name_index` | references_by_name has 1 entry |
| | | Add with 0 edges | Definition occurrence (should skip) | | `definition_occurrences_are_excluded` | Index size unchanged |
| | | | Multiple targets (N candidates) | | `multiple_edge_targets_produce_multiple_entity_entries` | Same Arc is stored N times in entity index |
| **Entity index storage** | 1 ref, 3 targets | No targets | Ambiguous reference (5 candidate implementations) | Idle → indexed | `arc_wrapped_edge_stored_once_per_entity` (new) | Arc ref count = N for N entities |
| | | | | | `arc_not_cloned_in_loop` (new) | Memory diff shows 1 ReferenceEdge allocation, not N |
| **Query by name** | Query matches symbol key | Query misses (wrong key) | Unresolved occurrence (fallback key) | Indexed → queried | `add_file_populates_name_index` | Result dereferences Arc transparently |
| **Query by entity** | Query matches entity_id | Query for non-existent entity | Occurrence with no entity_id | Indexed → queried | `add_file_populates_entity_index` | Result dereferences Arc transparently |
| **Remove file** | Remove indexed file | Remove unindexed file (no-op) | Remove, then re-add (incremental re-index) | Indexed → removed | `remove_file_clears_entries` | Arc ref count drops to 0, allocation freed |
| | | | | | `incremental_reindex_replaces_entries` | Old Arc freed, new Arc allocated |
| **Concurrency** | Parallel get_by_entity calls | | Interleaved add_file + get_by_entity | | `reference_index_concurrent_access` (new) | No data races, Arc ref count stable, results consistent |
| **Deref transparency** | Iterate over returned slice, access .symbol_key on items | | Arc<ReferenceEdge> behaves as &ReferenceEdge | | Existing tests pass unchanged | Deref coercion works, no test changes needed |

## §Blast-Radius

### Files Changed

- `crates/perl-workspace/src/semantic/references.rs` (lines 27-31, 83-100, 128-135)

### Dependent Crates

Search for imports of `ReferenceIndex` or calls to its methods:

```bash
rg "ReferenceIndex|get_by_name|get_by_entity" --type rust crates/ | grep -v "crates/perl-workspace"
```

Expected: Calls are concentrated in `perl-lsp` LSP providers for find-references and workspace-symbol queries. These iterate over the returned slice and dereference fields — **Deref coercion makes Arc<ReferenceEdge> transparent, no changes needed in callers**.

### Internal Boundaries (Must Not Cross)

- **ReferenceEdge struct** (perl-semantic-facts crate): No changes. Arc only wraps for storage.
- **FileFactShard interface** (perl-workspace): No changes.
- **Public API of ReferenceIndex**: Method signatures change (return type includes Arc), but Deref coercion is transparent to callers.

### Downstream Impact

- **LSP find-references command**: Iterates over slice, accesses fields. Works unchanged via Deref.
- **LSP workspace-symbol command**: Same. Works unchanged.
- **Tests**: Existing tests iterate and access fields. Work unchanged via Deref. No test changes needed.

### No Public Breaking Changes

ReferenceIndex is internal to perl-workspace. LSP providers consume public ReferenceIndex API methods (get_by_name, get_by_entity), which still return `&[...]` slices. Deref coercion makes callers unaware of Arc storage.

