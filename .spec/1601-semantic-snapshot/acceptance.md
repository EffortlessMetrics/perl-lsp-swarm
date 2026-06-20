# Acceptance Specification: SemanticSnapshot v1 (#1601)

## §Behavior

Introductory table: input condition → expected result. Rows from issue #1601 acceptance grid.

| # | Input / Condition | Expected Result | Notes |
|---|---|---|---|
| 1 | Reader thread captures `Arc<SemanticSnapshot>` while writer publishes new generation | All facts (file IDs, references, imports) read by reader belong to single generation; no split state from generation N (shards) mixed with N+1 (imports) | Concurrency safety invariant. Fixes the torn-read window at `:1769`/`:1777` where reader sees new shard + old imports |
| 2 | File A indexed with content X; `generation` = G1. Same file re-indexed with content Y | `generation` increments to G2 (= G1 + 1); all facts in snapshot carry generation G2; no facts from G1 visible after publish | Atomicity: one file change = one generation bump + one snapshot swap |
| 3 | File opened in editor (in-memory text bundle); disk snapshot has older version of same file | Query returns facts from open-document bundle; disk snapshot facts not visible until `close_document` is called | Open-doc overlay takes priority over disk snapshot without rebuilding entire snapshot |
| 4 | Run `find_definition()`, `find_references()`, `find_symbols()` on corpus from before refactor | Results match pre-refactor golden baseline stored in test constants | Adapter layer is behavior-preserving; legacy APIs unchanged |
| 5 | Read thread calls `with_semantic_query_context()` to query facts | Single `Arc<SemanticSnapshot>` captured at entry; no 3-lock borrow like old `:2726-2731`; method returns in < 1ms without lock contention | Eliminates 3-simultaneous-lock query borrow on (fact_shards, reference_index, import_export_index) |
| 6 | Workspace in `Building` lifecycle state; query issued | Query returns `None` (no panic, no hang); no blocking on builder thread | Graceful degradation: unavailable snapshot → empty result, not exception |
| 7 | Workspace initialized, first `index_file()` completes | `current_snapshot()` returns `Some` with `generation == 1`, `lifecycle == Ready` | Initialization publish happens automatically on first file index |

---

## §Hazards

Risk analysis by hazard class. Seeded from SUBSYSTEM_HAZARD_DEFAULTS.md (LSP-2, LSP-3, LSP-4 applicable for workspace-level atomicity).

### LSP-2: Concurrent Protocol Reads See Torn State

| Hazard | Surface | Severity | Mitigation | Test |
|--------|---------|----------|-----------|------|
| Reader thread observes generation N for file_ids but generation N+1 for imports during snapshot publish window | `WorkspaceIndex::with_semantic_query_context()` `:2726-2731` query path | CRITICAL | Single `Arc<SemanticSnapshot>` capture at method entry; release both read locks before callback returns. `ArcSwap` or `RwLock<Arc<>>` ensures atomic swap on publish side. | `torn_read_never_observed_under_concurrent_update`: spawn 8 readers + 1 writer doing 20 rapid `index_file()` calls; each reader captures generation + asserts all read facts carry same generation |

### LSP-3: Lock Contention Degrades Tail Latency

| Hazard | Surface | Severity | Mitigation | Test |
|--------|---------|----------|-----------|------|
| `parking_lot::RwLock<Arc<SemanticSnapshot>>` contention under N concurrent LSP clients (reader hold time = Arc clone = nanoseconds; write happens once per file change) | `WorkspaceIndex::current_snapshot()` reader acquisition on every query | MEDIUM | RwLock reader is non-blocking; write side holds lock only during Arc swap (~nanoseconds). 10 concurrent readers + 1 writer should stay < 5ms total latency for 100 queries. | `query_path_no_deadlock_under_concurrent_index`: 4 threads alternating `index_file()` + `with_semantic_query_context()` calls; complete in < 5s without deadlock |

### LSP-4: Adapter Layer Indirection Cascades

| Hazard | Surface | Severity | Mitigation | Test |
|--------|---------|----------|-----------|------|
| Legacy `find_definition()` adapters through snapshot cause latency spike for high-volume keystroke-driven symbol queries | Everywhere `find_definition()` is called from LSP providers | LOW | Arc cloning is O(1); snapshot query path is equivalent to old single-lock path. Measurement: < 5ms per query on 1000+ symbol workspaces. | Regression test `legacy_apis_identical_pre_post_refactor`: identical symbol lookup latencies pre/post refactor on benchmark corpus |

### LSP-5: Open-Document Overlay Invalidation Race

| Hazard | Surface | Severity | Mitigation | Test |
|--------|---------|----------|-----------|------|
| Concurrent `open_document()` + `index_file()` (disk sync) → overlay stale while disk snapshot rebuilds; reader sees split state | `WorkspaceIndex::set_open_doc_overlay()` + `rebuild_and_publish_snapshot()` | MEDIUM | Overlay is checked BEFORE snapshot each query; overlay invalidation is explicit (`remove_open_doc_overlay()` on close). Snapshot rebuild does NOT invalidate overlay; they are independent. | `open_doc_overlay_wins_over_disk`: index file A with content X (entity Foo); open doc with content Y (entity Bar); query must return Bar; no stale Foo |

### LSP-6: Generation Counter Wraparound

| Hazard | Surface | Severity | Mitigation | Test |
|--------|---------|----------|-----------|------|
| `AtomicU64` generation wraps at u64::MAX (~600 years at 1 change/sec/file for 500-file workspace); stale snapshot comparison becomes ambiguous | `WorkspaceIndex::generation` counter | LOW | Wraparound is practically impossible (u64 counter, typical workspace 1-10 updates/min). No client-side caching of generation numbers that would fail on wraparound (snapshot captured per request, not cached). | `generation_wraparound_behavior_at_u64_max`: set counter to u64::MAX - 1; publish 2 snapshots; assert generation increments correctly (wraps to 0) and no correctness issue arises |

### Cross-Subsystem: Parser Integration

| Hazard | Surface | Severity | Mitigation | Test |
|--------|---------|----------|-----------|------|
| N/A — Parser is upstream of WorkspaceIndex; parser changes do not affect snapshot atomicity | N/A | N/A | Snapshot captures already-parsed facts; does not depend on parser version. | N/A |

### Cross-Subsystem: DAP Integration

| Hazard | Surface | Severity | Mitigation | Test |
|--------|---------|----------|-----------|------|
| N/A — DAP does not use WorkspaceIndex directly; debug session state is separate | N/A | N/A | SemanticSnapshot is LSP-only concern. | N/A |

---

## §Contracts

Parser and protocol contracts touched by this change.

### PARSER_CONTRACTS.md References

| Contract | Section | How #1601 Touches It | Impact |
|----------|---------|---------|--------|
| **EntityFact shape** | `docs/reference/PARSER_CONTRACTS.md` §2.1 | SemanticSnapshot immutably captures EntityFact collections; shape is unchanged | None — snapshot is transparent to fact structure |
| **FileSemanticBundle composition** | `docs/reference/PARSER_CONTRACTS.md` §2.2 (defined in #1598) | SemanticSnapshot.files field is `HashMap<String, Arc<FileSemanticBundle>>`; requires #1598's bundle definition | Depends on #1598 `merge-ready` |
| **Atomicity invariant** | `docs/reference/PARSER_CONTRACTS.md` §1.6 (from #1599 umbrella) | FIXES broken contract: publication WAS non-atomic at `:1769`/`:1777`; now guaranteed atomic via single Arc swap | Implements the snapshot invariant from #1599 |
| **ReferenceIndex shape** | `docs/reference/PARSER_CONTRACTS.md` §3.1 | Snapshot captures `ReferenceIndex` by value (or Arc); immutable snapshot captures immutable index | No shape change; snapshot design depends on ReferenceIndex being cloneable |
| **ImportExportIndex shape** | `docs/reference/PARSER_CONTRACTS.md` §3.2 | Snapshot captures `ImportExportIndex` by value (or Arc) | No shape change; depends on ImportExportIndex being cloneable |

### LSP Protocol Contracts

| Behavior | Required By | How Satisfied |
|-----------|---|---|
| **find-definition response is deterministic per workspace state** | LSP spec §6.8 (textDocument/definition) | Snapshot is immutable and generation-numbered; same generation always returns same results |
| **find-references response is complete within one workspace state** | LSP spec §6.19 (textDocument/references) | All references queried from single snapshot generation; no split state |
| **workspace/symbol response is stable** | LSP spec §6.15 (workspace/symbol) | Symbol index is part of snapshot; stable across query |

---

## §API-Shape

New types, exported APIs, and ID-space changes.

### New Public Types

| Type | Module | Purpose | Shape |
|------|--------|---------|-------|
| `SemanticSnapshot` | `crate::workspace::snapshot` | Immutable generation-numbered workspace snapshot | `pub struct { generation: u64, lifecycle: SnapshotLifecycle, files: HashMap<String, Arc<FileSemanticBundle>>, file_ids: HashMap<String, FileId>, references: ReferenceIndex, imports: ImportExportIndex, workspace_roots: Vec<String> }` |
| `SnapshotLifecycle` | `crate::workspace::snapshot` | Lifecycle enum for snapshot (Building/Degraded/Ready) | `pub enum { Building, Degraded, Ready }` |

### New Public Methods

| Method | Receiver | Signature | Purpose |
|--------|----------|-----------|---------|
| `current_snapshot()` | `&WorkspaceIndex` | `pub fn current_snapshot(&self) -> Option<Arc<SemanticSnapshot>>` | Capture current snapshot; returns None if not yet published |
| `is_ready()` | `&SemanticSnapshot` | `pub fn is_ready(&self) -> bool` | Convenience: check if lifecycle == Ready |

### Modified Public Methods

| Method | Changes | Compat Impact |
|--------|---------|---|
| `with_semantic_query_context()` (if exists) | Now uses snapshot instead of 3-lock borrow | Behavior-preserving; signature unchanged |

### New Constructor (WorkspaceSemanticQueries)

| Method | Signature | Purpose |
|--------|-----------|---------|
| `from_snapshot()` | `pub fn from_snapshot(snapshot: &'a SemanticSnapshot) -> Self` | Create queries facade from snapshot; used by with_semantic_query_context() |

### ID-Space Changes

None — EntityId and FileId are unchanged. No new ID spaces introduced.

### Dup-Risk Grep

Files that export SemanticSnapshot or SnapshotLifecycle — verify no duplicates:

```bash
grep -r "pub struct SemanticSnapshot\|pub enum SnapshotLifecycle" crates/
```

Expected result: only `crates/perl-workspace/src/workspace/snapshot.rs` (1 match each).

### Caller Count (Snapshot Capture)

| Function | Pre-refactor callers | Post-refactor callers | Notes |
|----------|---|---|---|
| `current_snapshot()` | N/A (new) | Every `with_semantic_query_context()` call | Central capture point; high volume |
| `publish_snapshot()` | N/A (new) | `rebuild_and_publish_snapshot()` only (1 call site in index_file) | Bottleneck for atomicity; low frequency |

---

## §Test-Grid

Acceptance test rows: input condition → test name → invariant verified.

| # | Condition | Test Name | File | Invariant |
|---|-----------|-----------|------|-----------|
| 1 | 8 readers, 1 writer doing 20 rapid `index_file()` calls with random delays | `torn_read_never_observed_under_concurrent_update` | `crates/perl-workspace/tests/integration_semantic_snapshot.rs` | No reader ever sees (shards from generation N) + (imports from generation N+1); generation consistency enforced |
| 2 | Single file A indexed twice with different content | `single_file_update_bumps_generation_atomically` | `crates/perl-workspace/tests/integration_semantic_snapshot.rs` | Generation increments G1 → G2; both snapshots available; G2 snapshot shows new content for A |
| 3 | File indexed with content X; then opened in editor with content Y | `open_doc_overlay_wins_over_disk` | `crates/perl-workspace/tests/integration_semantic_snapshot.rs` | Query returns entity from Y (open doc), not X (disk); overlay priority verified |
| 4 | Run `find_definition()`, `find_references()`, `find_symbols()` on 3-file golden corpus (pre-refactor baseline stored in test) | `legacy_apis_identical_pre_post_refactor` | `crates/perl-workspace/tests/integration_semantic_snapshot.rs` | Result count, order, location URIs match golden baseline exactly (no regressions) |
| 5 | 4 threads: 2 call `with_semantic_query_context()` + 2 call `index_file()` in random interleaved order for 1000 iterations | `query_path_no_deadlock_under_concurrent_index` | `crates/perl-workspace/tests/integration_semantic_snapshot.rs` | All 1000 iterations complete in < 5s; no deadlock; no panic; results valid |
| 6 | Fresh `WorkspaceIndex::new()`; no `index_file()` called yet; call `with_semantic_query_context()` | `no_snapshot_returns_none_not_panic` | `crates/perl-workspace/tests/integration_semantic_snapshot.rs` | Returns `None` without panic, hang, or unwrap; graceful None result |
| 7 | Fresh `WorkspaceIndex`; call `index_file()` once | `initialization_publishes_snapshot_on_first_index_file` | `crates/perl-workspace/tests/integration_semantic_snapshot.rs` | `current_snapshot()` returns `Some`; snapshot.generation == 1; snapshot.lifecycle == Ready |
| 8 (CI) | `cargo test -p perl-workspace --lib && cargo clippy -p perl-workspace && cargo xtask fmt` | All tests green + lint clean | CI gate | All new + existing tests pass; no warnings; fmt clean |

---

## §Blast-Radius

Change scope and consumers.

### Directly Modified Files

| File | Lines | Type | Impact |
|------|-------|------|--------|
| `crates/perl-workspace/src/workspace/snapshot.rs` | +150 (new) | New module | Defines SemanticSnapshot + SnapshotLifecycle |
| `crates/perl-workspace/src/workspace/workspace_index.rs` | ~+40 (fields), ~+50 (methods) | Struct + impl | Add 3 fields; wire rebuild_and_publish_snapshot() call; rewrite with_semantic_query_context() to use snapshot |
| `crates/perl-workspace/src/workspace/mod.rs` | +1 (mod decl), +2 (re-exports) | Module root | Add `mod snapshot;` and re-export types |
| `crates/perl-workspace/src/semantic/queries.rs` | +20 | impl WorkspaceSemanticQueries | Add `from_snapshot()` constructor |

### Indirect Consumers

| Crate | How Affected | Required Change |
|-------|---|---|
| `crates/perl-lsp-rs/` (LSP server) | Calls `find_definition()` / `find_references()` / `find_symbols()` on WorkspaceIndex | **None** — these are adapters; behavior-preserving |
| `crates/perl-lsp-*` providers | Call semantic query methods on WorkspaceIndex | **None** — adapter layer unchanged |
| `crates/perl-dap/` (debug adapter) | Does not use WorkspaceIndex; unaffected | **None** |
| `crates/perl-parser/` | Unaffected | **None** |
| **Test suites** | Existing tests call `find_definition()` etc. | **None** — backward compatible |

### Must-Not-Touch Boundary

- **Do NOT remove** the six existing `RwLock` fields from `WorkspaceIndex` in this PR:
  - `files: Arc<RwLock<HashMap<String, FileIndex>>>`
  - `symbols: Arc<RwLock<HashMap<String, Vec<DefinitionCandidate>>>>`
  - `global_references: Arc<RwLock<HashMap<String, Vec<Location>>>>`
  - `fact_shards: Arc<RwLock<HashMap<String, FileFactShard>>>`
  - `semantic_reference_index: Arc<RwLock<ReferenceIndex>>`
  - `semantic_import_export_index: Arc<RwLock<ImportExportIndex>>`

  **Reason:** They remain the authoritative write targets; `rebuild_and_publish_snapshot()` reads from them. Their removal is Tranche 4 work (provider cutover).

- **Do NOT change provider API** in `crates/perl-lsp-*` modules. This PR is snapshot substrate only.

- **Do NOT depend on FileSemanticBundle behavior** beyond what #1598 specifies. The bundle is built by #1598; this PR consumes it.

### Structural Blocker Status

**Label: `structural-blocker`** — This PR (Tranche 3 substrate) blocks Tranche 4 provider cutover (#1604+). Do not merge Tranche 4 work until this issue is complete and `merge-ready`.

### Dependency Gates

| Issue | Required Status | Reason |
|-------|---|---|
| #1600 | Must be merged before red-tdd starts | Snapshot depends on file-scoped EntityIds from #1600 |
| #1598 | Must be merged before builder starts implementation | FileSemanticBundle is defined in #1598; required in snapshot.rs |
| #1599 | Should be merged before or with this PR | #1599 is the umbrella contract for atomicity; #1601 implements it |

---

## Summary

**What changes:** WorkspaceIndex gains an atomic `RwLock<Arc<SemanticSnapshot>>` that captures all semantic facts (file IDs, references, imports) at one point in time. A request captures one `Arc<SemanticSnapshot>` and uses only that generation — no torn reads across updates.

**What stays the same:** Legacy public APIs (`find_definition()`, `find_references()`, `find_symbols()`) are behavior-preserving adapters over the snapshot. Providers are not modified this PR.

**What's fixed:** The non-atomic publication window (`:1769` → `:1782`) where readers saw new shards with stale import visibility. The 3-simultaneous-lock query borrow (`:2726-2731`) is eliminated.

**What's new:** `SemanticSnapshot` type, open-document overlay, generation counter, `current_snapshot()` accessor, `from_snapshot()` constructor for queries.

**Risk:** MEDIUM. Core pattern (RwLock<Arc<>>) is proven. Main risk: adapter-bypass (legacy APIs still directly reading old locks). Step 10 audit (grep + manual inspection) is mandatory.
