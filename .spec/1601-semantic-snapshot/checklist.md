# Implementation Checklist: SemanticSnapshot v1 (#1601)

## Overview

Introduce an immutable, generation-numbered `SemanticSnapshot` that the workspace builds **off-thread** and publishes with **one atomic swap** via `parking_lot::RwLock<Arc<SemanticSnapshot>>`. A request captures one `Arc<SemanticSnapshot>` and uses only that generation — never observing torn reads across an update. Legacy `WorkspaceIndex` APIs become thin adapters; no provider cutover in this PR.

**Dependencies:** Must merge after #1600 (file-scoped ids) and #1598 (FileSemanticBundle).

---

## Step 1: Create `crates/perl-workspace/src/workspace/snapshot.rs`

**File:** `crates/perl-workspace/src/workspace/snapshot.rs` (CREATE)

**What to add:** Define `SemanticSnapshot { generation, lifecycle, files, file_ids, references, imports, workspace_roots }` and `SnapshotLifecycle { Building, Degraded, Ready }`. All fields pub; no Clone (callers use `Arc<SemanticSnapshot>`). Implement `Debug` manually (show counts not details). Add `new()` constructor and `is_ready()` convenience method.

**Signatures:**

```rust
pub enum SnapshotLifecycle { Building, Degraded, Ready }

pub struct SemanticSnapshot {
    pub generation: u64,
    pub lifecycle: SnapshotLifecycle,
    pub files: HashMap<String, Arc<FileSemanticBundle>>,
    pub file_ids: HashMap<String, FileId>,
    pub references: ReferenceIndex,
    pub imports: ImportExportIndex,
    pub workspace_roots: Vec<String>,
}

impl SemanticSnapshot {
    pub fn new(generation: u64, lifecycle: SnapshotLifecycle, ...) -> Self
    pub fn is_ready(&self) -> bool
}
```

**Dependencies:** Depends on `FileSemanticBundle` (from #1598); it must be pub-exported from `crates/perl-workspace/src/lib.rs` or be accessible as `crate::FileSemanticBundle`.

**Verify:** `cargo build -p perl-workspace 2>&1 | head -30`

---

## Step 2: Add fields to `WorkspaceIndex` struct (workspace_index.rs)

**File:** `crates/perl-workspace/src/workspace/workspace_index.rs`, struct `WorkspaceIndex` at line ~1165

**What to add:** Three new fields:

```rust
/// Current atomic snapshot (generation-numbered)
current_snapshot: parking_lot::RwLock<Option<Arc<SemanticSnapshot>>>,

/// Open-document overlay: in-memory bundles that override disk snapshot
open_doc_overlay: parking_lot::RwLock<HashMap<String, Arc<FileSemanticBundle>>>,

/// Atomic generation counter (increments on each publish)
generation: std::sync::atomic::AtomicU64,
```

**Location:** Add after existing field declarations (after `workspace_folders` at line ~1188).

**Dependencies:** `SemanticSnapshot` must be imported (`use crate::workspace::snapshot::SemanticSnapshot`).

**Compilation order:** Step 1 must compile first.

**Verify:** `cargo build -p perl-workspace 2>&1 | grep -E "error|warning" | head -10`

---

## Step 3: Initialize new fields in `WorkspaceIndex::new()`

**File:** `crates/perl-workspace/src/workspace/workspace_index.rs`, method `impl WorkspaceIndex { pub fn new() ... }`

**What to add:** Find the `new()` method (around line ~1250 or search for `pub fn new`). Add initialization:

```rust
current_snapshot: parking_lot::RwLock::new(None),
open_doc_overlay: parking_lot::RwLock::new(HashMap::new()),
generation: std::sync::atomic::AtomicU64::new(0),
```

**Location:** Alongside existing field initializations.

**Verify:** `cargo build -p perl-workspace 2>&1 | grep -E "error" | head -5`

---

## Step 4: Add `current_snapshot()` and `publish_snapshot()` methods

**File:** `crates/perl-workspace/src/workspace/workspace_index.rs`, impl block for `WorkspaceIndex`

**What to add:** Two public methods:

```rust
/// Capture current snapshot (if published). Returns None if not yet initialized.
pub fn current_snapshot(&self) -> Option<Arc<SemanticSnapshot>> {
    self.current_snapshot.read().clone()
}

/// Publish a new snapshot atomically, replacing the previous one.
fn publish_snapshot(&self, snapshot: SemanticSnapshot) {
    *self.current_snapshot.write() = Some(Arc::new(snapshot));
}
```

**Location:** Add in the `impl WorkspaceIndex` block, after the existing public accessor methods (around line ~1300-1400).

**Verify:** `cargo build -p perl-workspace 2>&1 | head -20`

---

## Step 5: Add `rebuild_and_publish_snapshot()` method

**File:** `crates/perl-workspace/src/workspace/workspace_index.rs`, impl block for `WorkspaceIndex`

**What to add:** A private method that reads current state and builds a snapshot:

```rust
/// Rebuild snapshot from current index state and publish atomically.
/// Called at the end of index_file() after all fact updates are complete.
fn rebuild_and_publish_snapshot(&self) {
    let gen = self.generation.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
    let files = self.fact_shards.read();
    let refs = self.semantic_reference_index.read();
    let imports = self.semantic_import_export_index.read();
    let roots = self.workspace_folders.read();

    // Build file_ids map from fact_shards
    let file_ids: HashMap<String, FileId> = files
        .iter()
        .map(|(uri, shard)| (uri.clone(), shard.file_id))
        .collect();

    let snapshot = SemanticSnapshot::new(
        gen,
        SnapshotLifecycle::Ready,
        // TODO: populate files from fact_shards (Step 5a)
        HashMap::new(),
        file_ids,
        refs.clone(),
        imports.clone(),
        roots.clone(),
    );

    self.publish_snapshot(snapshot);
}
```

**Location:** Add after `publish_snapshot()` in impl block.

**Note:** This is a draft; the `files` field population will be addressed in Step 5a. The ReferenceIndex and ImportExportIndex may need `.clone()` or equivalent — verify their Clone implementations.

**Verify:** `cargo build -p perl-workspace 2>&1 | grep -E "error|cannot find" | head -10`

---

## Step 5a: Populate `files` field in snapshot (Step 5 refinement)

**File:** `crates/perl-workspace/src/workspace/workspace_index.rs`, in `rebuild_and_publish_snapshot()` method

**What to add:** Extract FileSemanticBundle from FileFactShard or from wherever they are stored. The step 5 method above has a placeholder `HashMap::new()` for the `files` field.

**Key question for red-tdd:** How are FileSemanticBundle instances created and stored? Are they already in the WorkspaceIndex, or must they be built from the FileFactShard facts?

**Verify:** Check how FileFactShard relates to FileSemanticBundle.

---

## Step 6: Wire call to `rebuild_and_publish_snapshot()` in `index_file()`

**File:** `crates/perl-workspace/src/workspace/workspace_index.rs`, in `index_file()` method (around line ~1740)

**What to find:** The `index_file()` method ends with a lock block that updates `semantic_import_export_index` around line `:1777-1782`.

**What to add:** Immediately after that lock block closes, add:

```rust
self.rebuild_and_publish_snapshot();
```

**Location:** Right after line ~1782 (after the import/export lock releases).

**Verify:** `cargo build -p perl-workspace` compiles without errors.

---

## Step 7: Add `update_open_doc_overlay()` method for open-document handling

**File:** `crates/perl-workspace/src/workspace/workspace_index.rs`, impl block

**What to add:** Two helper methods:

```rust
/// Add or update an open document in the overlay.
pub fn set_open_doc_overlay(&self, uri: &str, bundle: Arc<FileSemanticBundle>) {
    let key = DocumentStore::uri_key(&Self::normalize_uri(uri));
    self.open_doc_overlay.write().insert(key, bundle);
}

/// Remove a closed document from the overlay.
pub fn remove_open_doc_overlay(&self, uri: &str) {
    let key = DocumentStore::uri_key(&Self::normalize_uri(uri));
    self.open_doc_overlay.write().remove(&key);
}
```

**Location:** After `rebuild_and_publish_snapshot()` in impl block.

**Verify:** Compiles; no unresolved references.

---

## Step 8: Rewrite `with_semantic_query_context()` to use snapshot (or equivalent)

**File:** `crates/perl-workspace/src/workspace/workspace_index.rs`

**Context:** The plan-review mentions `:2726-2731` as the location of the 3-lock query borrow. Search the current code for the exact location of this pattern (may have changed).

**What to replace:** The current pattern that acquires three separate read locks on fact_shards, semantic_reference_index, and semantic_import_export_index.

**New pattern:**

```rust
pub fn with_semantic_query_context<F, R>(&self, uri: &str, f: F) -> Option<R>
where
    F: FnOnce(FileId, WorkspaceSemanticQueries) -> Option<R>,
{
    let snapshot = self.current_snapshot()?;
    let overlay = self.open_doc_overlay.read();
    let key = DocumentStore::uri_key(&Self::normalize_uri(uri));

    // Check overlay first, then snapshot
    let file_id = overlay
        .get(&key)
        .map(|b| b.file_id)
        .or_else(|| {
            snapshot
                .file_ids
                .get(&key)
                .copied()
        })?;

    // Create queries facade from snapshot (single Arc capture)
    let queries = WorkspaceSemanticQueries::from_snapshot(&snapshot);
    Some(f(file_id, queries))
}
```

**Note:** This assumes `WorkspaceSemanticQueries::from_snapshot()` exists (Step 9).

**Verify:** `cargo build -p perl-workspace 2>&1 | grep -E "error|unresolved" | head -10`

---

## Step 9: Add `WorkspaceSemanticQueries::from_snapshot()` constructor

**File:** `crates/perl-workspace/src/semantic/queries.rs`, impl block for `WorkspaceSemanticQueries<'a>`

**What to add:** A new constructor that takes a borrowed reference to `SemanticSnapshot`:

```rust
/// Create a new `WorkspaceSemanticQueries` from a snapshot.
/// The snapshot must be held for the lifetime 'a.
pub fn from_snapshot(snapshot: &'a SemanticSnapshot) -> Self {
    Self {
        reference_index: &snapshot.references,
        import_export_index: &snapshot.imports,
        fact_shards: /* how to get fact_shards from snapshot? */,
        package_graph: None,  // TODO: populate from snapshot if available
        value_shape_index: None,
    }
}
```

**Key question for red-tdd:** How should `fact_shards` be populated from the snapshot? Does SemanticSnapshot have a method to produce the HashMap<String, FileFactShard> view?

**Location:** After existing constructors (`new()`, `with_package_graph()`, `with_package_graph_and_shapes()`).

**Verify:** Compiles; all field assignments valid.

---

## Step 10: Add doc comment for adapter-bypass audit

**File:** `crates/perl-workspace/src/workspace/workspace_index.rs`

**What to add:** A doc comment at the top of the impl block noting the audit requirement:

```rust
// ADAPTER BYPASS AUDIT (Step 10):
// Every direct read of self.fact_shards, self.semantic_reference_index,
// or self.semantic_import_export_index must either:
// 1. Be replaced with snapshot-based queries via with_semantic_query_context()
// 2. Have a doc comment explaining why it legitimately bypasses atomicity
//    (e.g., diagnostic/admin queries that don't need generation consistency)
//
// Search for: self.fact_shards.read(), self.semantic_reference_index.read(),
//             self.semantic_import_export_index.read()
// Run: grep -n "self\.fact_shards\.read\|self\.semantic_reference_index\.read\|self\.semantic_import_export_index\.read" workspace_index.rs
```

**Then run the grep command and for each match:**
- If the call site is in a provider-facing public method, replace with snapshot-based path
- If the call site is in an internal diagnostic/test method, add a SAFETY doc comment explaining why it's safe to bypass

**Verify:** `grep -n "self\.fact_shards\.read\|self\.semantic_reference_index\.read\|self\.semantic_import_export_index\.read" crates/perl-workspace/src/workspace/workspace_index.rs | wc -l` (count should decrease with each bypass replacement)

---

## Step 11: Re-export `SemanticSnapshot` and `SnapshotLifecycle` in module root

**File:** `crates/perl-workspace/src/workspace/mod.rs`

**What to add:**

```rust
pub use snapshot::{SemanticSnapshot, SnapshotLifecycle};
```

**Location:** In the re-export block at the top of mod.rs.

**Verify:** `cargo build -p perl-workspace` succeeds.

---

## Step 12: Add module declaration

**File:** `crates/perl-workspace/src/workspace/mod.rs`

**What to add:** If not already present, add:

```rust
mod snapshot;
```

**Location:** Near other mod declarations (after `mod document_store;` etc.).

**Verify:** `cargo build -p perl-workspace 2>&1 | grep -E "error" | head -5`

---

## Step 13: Run tests and verify no regressions

**File:** Various test files

**What to verify:**

```bash
cargo test -p perl-workspace --lib 2>&1 | tail -50
cargo clippy -p perl-workspace --lib 2>&1 | grep -E "error|warning" | head -20
cargo xtask fmt --check 2>&1 | grep -E "error|warning" | head -10
```

Expected: All existing tests green; no clippy warnings introduced.

**Verify:** Test output shows "test result: ok" for all test suites.

---

## Step 14: Verify no deadlock in concurrent scenarios (manual inspection)

**File:** `crates/perl-workspace/src/workspace/workspace_index.rs`

**What to check:** The new snapshot acquisition path (Step 8) must not hold any locks while waiting for the snapshot RwLock. The pattern should be:

1. `let snapshot = self.current_snapshot()?;` — acquires snapshot read lock, clones Arc, releases lock
2. `let overlay = self.open_doc_overlay.read();` — acquires overlay read lock (held for duration of callback)
3. Callback runs with snapshot and overlay held

Ensure no nested lock acquisitions that could deadlock with `index_file()`.

**Verify:** Read through the code path; confirm no lock held inside another lock's scope except overlay inside snapshot lifetime (safe because overlay is independent).

---

## Compilation Order Summary

1. Step 1 → 2 → 3 → 4 → 5 (snapshot.rs + WorkspaceIndex struct + init + accessors)
2. Step 5a (populate files field)
3. Step 6 (wire into index_file)
4. Step 7 (overlay helpers)
5. Step 8 (rewrite with_semantic_query_context)
6. Step 9 (WorkspaceSemanticQueries::from_snapshot)
7. Step 10 (audit comment + grepping)
8. Step 11-12 (re-exports)
9. Step 13 (tests)
10. Step 14 (deadlock review)

Each step compiles on its own before proceeding to the next.

---

## Acceptance Tests to Write (Red-TDD)

See `acceptance.md` §Test-Grid for the full test list. Key test names:

1. `torn_read_never_observed_under_concurrent_update` — concurrency test
2. `single_file_update_bumps_generation_atomically` — generation counter
3. `open_doc_overlay_wins_over_disk` — overlay priority
4. `legacy_apis_identical_pre_post_refactor` — regression
5. `query_path_no_deadlock_under_concurrent_index` — deadlock check
6. `no_snapshot_returns_none_not_panic` — graceful degradation
7. `initialization_publishes_snapshot_on_first_index_file` — init lifecycle

---

## Verify Command (Final)

```bash
cargo test -p perl-workspace && cargo clippy -p perl-workspace && cargo xtask fmt
```

Expected: All tests pass, no warnings, formatting clean.
