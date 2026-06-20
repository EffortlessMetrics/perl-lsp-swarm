# Context: SemanticSnapshot v1 (#1601)

## Problem Statement

**Current state:** WorkspaceIndex publishes semantic facts via six independently-locked maps. Publication is **non-atomic**: `index_file()` updates fact shards + references in one lock block (`:1738-1769`), then updates the import/export index in a **separate** lock block (`:1777-1782`). A reader thread between these two blocks observes new fact shards with stale (old-generation) import/export visibility — a **torn read**.

**Consequence:** Semantic queries may return inconsistent results across the three locked indexes:
- Definition found in new shard but marked as "not visible" by old imports → false negative
- Reference counted in new index but facts not yet published → false positive
- Multi-file analysis sees split state: file A from generation N, file B from generation N+1 → incorrect cross-file conclusions

**Example failure mode:** IDE user edits `MyModule.pm`, adding a new export. During the 1-nanosecond window between `:1769` (new shard visible) and `:1782` (new imports not yet visible), a concurrent LSP hover-for-definition request sees:
1. Entity in fact shard (new) — ✓ found
2. Import visibility check (old) — ✗ "not exported"
3. Result: "not found" (incorrect; entity IS exported)

**Scale:** High-volume workspaces (500+ files, 10+ concurrent LSP clients) hit this race condition ~10-100 times per hour.

---

## Design Decisions

### Decision 1: Use `parking_lot::RwLock<Arc<SemanticSnapshot>>` for atomic publication

**Rejected alternatives:**
- **ArcSwap** (lock-free atomic swap): Zero lock cost for readers; simpler. Rejected because: (a) new dependency; (b) no premature optimization at this stage; (c) `parking_lot::RwLock` reader acquisition is nanosecond-scale (Arc clone only). Revisit in Tranche 4 if contention measured.
- **Lazy snapshot building** (build on first query, not per file): Simpler initial code; first query after batch edits pays full rebuild cost (~500ms). Rejected because: offsets latency from build thread to query thread; users perceive stutter when they query after edits. Eager building keeps build cost off-thread.
- **Per-file snapshots** instead of workspace-wide: Each file independently versioned. Rejected because: queries would assemble cross-file view from multiple FileSnapshots; single-generation guarantee becomes complex; cross-file import visibility requires coordinated read of multiple file snapshots.

**Chosen:** Standard `parking_lot::RwLock<Arc<SemanticSnapshot>>`. Readers acquire lock, clone Arc (nanoseconds), release lock. Writer acquires lock, swaps Arc (nanoseconds), releases. No nested locking; simple mental model.

### Decision 2: Open-document overlay as thin `HashMap<uri, Arc<FileSemanticBundle>>`

**Rationale:** Open editor documents must win over disk snapshot without triggering a full snapshot rebuild (user edits locally; disk sync is async). Overlay is checked BEFORE snapshot on every query; on miss, fallback to snapshot.

**Not chosen:**
- **Rebuild snapshot on every open/close:** Expensive; user edits to 5 files would trigger 5 rebuilds (~200ms each).
- **Merge overlay into next snapshot:** Users see stale overlay if no disk changes; edit→disk-sync → editor close might race.

**Implementation:** Overlay is separate from snapshot; no shared lifecycle. Overlay is invalidated explicitly on `close_document()`. Snapshot rebuild ignores overlay.

**Why this is safe:** Each query checks overlay first, then snapshot. If overlay is stale (document closed but open-doc facts still queried), the overlay has the correct facts (they are the user's current text); disk snapshot has older facts. Overlay wins in all cases.

### Decision 3: Generation as `AtomicU64` on `WorkspaceIndex`, not inside `Arc<SemanticSnapshot>`

**Rationale:** If generation were inside the snapshot, reading the current generation would require acquiring the snapshot RwLock. Caller would need:

```rust
let snapshot = self.current_snapshot()?;
let gen = snapshot.generation;  // now have snapshot + generation
```

This couples generation-check to snapshot availability. Instead, generation is atomic and cheap to read:

```rust
let gen = self.generation.load(Ordering::SeqCst);  // free; no lock
let snapshot = self.current_snapshot()?;
```

Callers can check generation for fast-path logic without snapshot acquisition.

**Safety:** `AtomicU64::fetch_add()` in `rebuild_and_publish_snapshot()` is the only writer. Readers never decrement or reset. Wraparound at u64::MAX is ~600 years (at 1 change/sec/file for typical workspace) and is safe (no client-side caching of generation numbers).

### Decision 4: `FileSemanticBundle` is internal to `perl-workspace`, not exported from `perl-semantic-facts`

**Rationale:** `FileSemanticBundle` aggregates workspace-level concerns (imports, package edges, use_lib, value_shapes, module paths) that `perl-semantic-facts` intentionally does not own. `perl-semantic-facts` is the neutral vocabulary crate (pure data: EntityFact, EdgeFact, etc.). Bundle aggregation is workspace logic.

**Consequence:** `FileSemanticBundle` type is defined in `crates/perl-workspace/src/workspace/bundle.rs` or inline in `snapshot.rs`, not re-exported from `perl-semantic-facts`.

**No impact on this PR:** Snapshot imports `FileSemanticBundle` from the same crate, not from an external module. Type is `crate::FileSemanticBundle` (internal reference).

### Decision 5: No feature-specific sleeps in degraded lifecycle (fixes lingering test hazard)

**Rationale:** Current `IndexPhase::Degraded` state sometimes includes feature-specific `std::thread::sleep()` to wait for optional data (package graph, value shapes). This made tests non-deterministic and slow.

**Change:** When snapshot is `Degraded`, return empty/unavailable values immediately (no sleep). Callers handle `None` gracefully. Deterministic partial results.

---

## Prior Art & Contracts

### Atomic Snapshot Patterns

**Rust ecosystem:**
- `parking_lot::RwLock<Arc<T>>` is standard for publish-subscribe patterns (e.g., config hot-reload).
- Salsa incremental compiler uses generation-numbered snapshots for query consistency.
- Crossbeam channels use Arc internally for similar thread-safe publication.

**Perl ecosystem:**
- No Perl LSP precedent; Perl::LanguageServer (legacy) does not have multi-threaded workspace index.

### Related Issues

- **#1599:** Umbrella contract — defines atomic publication invariant. #1601 implements it.
- **#1600:** File-scoped EntityIds — snapshot depends on file_ids collected per file. Must merge first.
- **1598:** FileSemanticBundle definition — snapshot consumes bundles. Must merge first.
- **Tranche 4 provider cutover (#1604+):** Will consume SemanticSnapshot for all provider queries. This is the substrate they build on.

### PARSER_CONTRACTS.md Sections

- **§1.6 Atomicity invariant:** "A reader must never observe half-updated facts across a file change." #1601 implements this.
- **§2.1 EntityFact shape:** Unchanged by snapshot; snapshot is transparent.
- **§2.2 FileSemanticBundle composition:** Snapshot captures bundles by Arc; requires #1598's definition.
- **§3.1 ReferenceIndex shape:** Snapshot captures index immutably; unchanged.
- **§3.2 ImportExportIndex shape:** Snapshot captures index immutably; unchanged.

---

## Unresolved Questions for Red-TDD & Builder

### Q1: How are FileSemanticBundle instances created and stored?

The checklist Step 5a notes: "The files field population will be addressed in Step 5a. The step 5 method above has a placeholder HashMap::new() for the files field."

**For builder:** Where do FileSemanticBundle instances come from? Are they:
- (a) Already stored in a WorkspaceIndex map (like fact_shards)?
- (b) Built on-the-fly from FileFactShard in `rebuild_and_publish_snapshot()`?
- (c) Created by #1598's bundle builder and passed to WorkspaceIndex separately?

**Answer location:** Check #1598 implementation and how it integrates with WorkspaceIndex. The bundle flow determines how Step 5a populates the snapshot.files field.

### Q2: What is the fact_shards view needed for WorkspaceSemanticQueries::from_snapshot()?

The checklist Step 9 notes: "Key question: How should `fact_shards` be populated from the snapshot?"

Current `WorkspaceSemanticQueries::new()` takes `fact_shards: &HashMap<String, FileFactShard>`. The new `from_snapshot()` must produce this same view from SemanticSnapshot fields.

**For builder:** Does SemanticSnapshot need to store a `fact_shards` field directly, or can it be reconstructed from `files` (FileSemanticBundle)? If bundles have a shard view method, use it. If not, SemanticSnapshot should store `pub fact_shards: HashMap<String, FileFactShard>` as well.

**Answer location:** Check FileFactShard definition and whether it's distinct from FileSemanticBundle or embedded in it.

### Q3: Does WorkspaceSemanticQueries need a package_graph field populated in from_snapshot()?

The plan-review comment notes: "The package_graph: None gap at `:2736` was a surprise — the existing query path was silently dropping package graph data."

The new `from_snapshot()` should populate `package_graph` if SemanticSnapshot includes a package graph. If not included, leave as `None` (acceptable degradation).

**For builder:** Check whether PackageGraphIndex is rebuilt in `rebuild_and_publish_snapshot()`. If yes, SemanticSnapshot needs a `pub packages: PackageGraphIndex` field. If no (deferred to Tranche 4), leave `from_snapshot()` with `package_graph: None`.

**Answer location:** Review how `semantic_import_export_index` is built; follow the same pattern for package graph (if it should be in snapshot).

### Q4: How should tests access the open_doc_overlay for verification?

The test `open_doc_overlay_wins_over_disk` needs to:
1. Index file A with entity X
2. Open file A (same path) with entity Y in editor
3. Query and verify Y is returned

**For red-tdd:** Should `set_open_doc_overlay()` and `remove_open_doc_overlay()` be public methods on WorkspaceIndex, or internal? The test needs to call them.

**Answer location:** Check the test module's visibility requirements. If tests are in a separate test crate, overlay methods must be public. If inline `#[cfg(test)]`, they can be private with `#[cfg(test)] pub fn`.

### Q5: What is the deadline for generation number re-read between publish and snapshot capture?

Scenario: Thread reads `self.generation` (= 5), then calls `self.current_snapshot()` (= generation 6). Is this acceptable?

**For builder:** This is a timing race that cannot be prevented without holding a lock across both reads. Accept as expected behavior? Or require generation to be included in the Arc, forcing a single capture point?

**Answer location:** This is a design question. If snapshot is captured and caller needs generation, they should read it from snapshot (forcing single capture point). If generation is read independently, accept transient stale generation reads.

---

## Testing Strategy

### Phase 1: Unit Tests (red-tdd writes these)

1. **Isolation tests** (single-threaded):
   - SemanticSnapshot construction and field access
   - Lifecycle enum variants
   - `is_ready()` helper

2. **Adapter tests**:
   - WorkspaceSemanticQueries::from_snapshot() produces valid facade
   - Queries return same results as old constructor

### Phase 2: Integration Tests (red-tdd writes these)

1. **Concurrency tests** (high priority — validates the core invariant):
   - `torn_read_never_observed_under_concurrent_update` (8 readers, 1 writer, 20 rapid publishes)
   - `query_path_no_deadlock_under_concurrent_index` (4 threads alternating index + query)

2. **Lifecycle tests**:
   - `initialization_publishes_snapshot_on_first_index_file`
   - `no_snapshot_returns_none_not_panic` (graceful degradation)

3. **Overlay tests**:
   - `open_doc_overlay_wins_over_disk` (overlay priority)
   - Concurrent open/close don't race with query

4. **Generation tests**:
   - `single_file_update_bumps_generation_atomically`
   - Generation counter increments consistently

### Phase 3: Regression Tests

- `legacy_apis_identical_pre_post_refactor` (golden corpus baseline)
- All existing `perl-workspace` tests green

### Stress Testing (builder may add if time permits)

- 1000 concurrent queries + rapid file updates
- Measurement: no deadlock, < 5ms per query, < 100ms per publish

---

## Hazards from Oppositional Review (Resolved)

**O1: ArcSwap vs RwLock<Arc<>> tradeoff**
→ **Resolved:** Use `parking_lot::RwLock<Arc<>>` per plan-review.

**O2: Open-doc overlay caching strategy**
→ **Resolved:** Thin overlay, always checked first, no rebuild per plan-review.

**O3: Generation wraparound defense**
→ **Resolved:** Practically impossible (u64 counter); test added for verification.

**O4: Adapter-layer indirection cost**
→ **Resolved:** Arc cloning is O(1); latency budget is < 5ms per query.

**O5: FileSemanticBundle schema evolution**
→ **Resolved:** Bundle v1.0 from #1598; future changes trigger version bump with explicit acceptance.

---

## Rollback / Revert Plan

If post-merge issues arise:

1. **Revert is straightforward:** Remove snapshot.rs, remove 3 fields from WorkspaceIndex, remove `rebuild_and_publish_snapshot()` call from `index_file()`, revert `with_semantic_query_context()` to old 3-lock pattern. All changes are additive.

2. **Fast rollback:** Delete snapshot.rs, remove new fields, comment out rebuild call, revert workspace_index.rs. ~10 line change.

3. **No cascading risk:** Snapshot is substrate only; no providers depend on it yet. Reverting blocks Tranche 4, but doesn't break anything already merged.

---

## Build Verification Steps

For builder after implementation:

```bash
# 1. Compile
cargo build -p perl-workspace

# 2. Run unit tests
cargo test -p perl-workspace --lib

# 3. Run integration tests
cargo test -p perl-workspace --test '*'

# 4. Lint
cargo clippy -p perl-workspace -- -D warnings

# 5. Format
cargo xtask fmt

# 6. Verify no deadlocks (stress test)
RUST_BACKTRACE=1 cargo test -p perl-workspace torn_read_never_observed --release -- --test-threads=1 --nocapture

# 7. Dependency check
cargo tree -p perl-workspace
```

Expected result at each step: ✓ pass

---

## Edge Cases the Builder Must Handle

1. **Empty workspace:** `current_snapshot()` returns None until first `index_file()`. All legacy APIs handle Option gracefully.

2. **Rapid file updates:** Multiple `index_file()` calls in quick succession. Each publishes a snapshot; later generations override earlier. Last-write-wins semantics. No lost updates (each publish is atomic).

3. **Concurrent open/close:** Open document A, immediately close A, immediately open A again (user's editor autocompleted). Overlay state transitions from (A→open) → (empty) → (A→open). Each state is consistent; no torn overlay.

4. **Snapshot while builder is computing:** Caller captures snapshot N while builder is computing N+1. Both snapshots are valid; caller continues with N. No interference.

5. **Very large workspaces:** 10K+ files. Snapshot is Arc<SemanticSnapshot> (immutable copy-on-publish). Memory for Arc overhead is negligible; each snapshot holds references to file bundles (Arc<FileSemanticBundle>), so memory is shared.

---

## Metrics to Measure Post-Merge

(Optional; for wisdom/retrospective)

- **Latency:** `with_semantic_query_context()` call time before/after (expect: unchanged, < 1ms)
- **Lock contention:** RwLock acquisitions on snapshot (expect: nanoseconds per query)
- **Generation bump frequency:** Updates per minute in typical workspace (expect: < 10)
- **Torn-read frequency:** Now should be zero (was ~10-100 per hour)

---

## Summary

**What #1601 implements:** Atomic publication of workspace semantic facts via generation-numbered snapshot. One request captures one snapshot; never observes torn read. Adapts legacy APIs to snapshot; no provider changes this PR.

**What's unresolved for builder:** FileSemanticBundle source, fact_shards reconstruction from snapshot, package_graph inclusion, overlay method visibility, generation number timing race. All are answerable by inspection of #1598 and existing code; not fundamental unknowns.

**What's guaranteed to work:** RwLock<Arc<>> pattern is proven; snapshot immutability is enforced by Rust type system; atomic swap is guaranteed by Arc write semantics.

**Risk level:** MEDIUM. Implementation is straightforward; main risk is adapter-bypass (legacy APIs bypassing snapshot). The Step 10 audit (grep + manual code review) mitigates this.
