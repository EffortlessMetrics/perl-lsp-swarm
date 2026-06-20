# Implementation Checklist: Arc-wrap ReferenceEdge to eliminate clone waste

## Overview

Wrap `ReferenceEdge` in `Arc<_>` to eliminate N-fold duplication when the same reference maps to multiple target candidates. Storage changes from `Vec<ReferenceEdge>` to `Vec<Arc<ReferenceEdge>>`. All method calls work unchanged via Deref coercion.

**Files changed**: 1 (`crates/perl-workspace/src/semantic/references.rs`)

**Lines affected**: ~15 (struct fields, Arc wrapping, clone operations)

**Compilation order**: No dependencies; can be done in a single step.

## Step 1: Add Arc import and change field types

**File**: `crates/perl-workspace/src/semantic/references.rs`

**What**: Add `use std::sync::Arc;` at the top of the file and change two struct fields.

**Where**:
- Line 14 (after `use std::collections::HashMap;`): Add `use std::sync::Arc;`
- Line 27: Change `references_by_name: HashMap<String, Vec<ReferenceEdge>>,` to `references_by_name: HashMap<String, Vec<Arc<ReferenceEdge>>>,`
- Line 31: Change `references_by_entity: HashMap<EntityId, Vec<ReferenceEdge>>,` to `references_by_entity: HashMap<EntityId, Vec<Arc<ReferenceEdge>>>,`

**Verify command**: `cargo build -p perl-workspace`

---

## Step 2: Wrap ReferenceEdge creation in Arc

**File**: `crates/perl-workspace/src/semantic/references.rs`

**What**: Change the `ReferenceEdge::new(...)` call to `Arc::new(ReferenceEdge::new(...))`.

**Where**: Lines 83-92 (the ReferenceEdge construction)

**Before**:
```rust
let ref_edge = ReferenceEdge::new(
    occ.id,
    occ.anchor_id,
    shard.file_id,
    symbol_key.clone(),
    target_candidates.clone(),
    occ.kind,
    occ.provenance,
    occ.confidence,
);
```

**After**:
```rust
let ref_edge = Arc::new(ReferenceEdge::new(
    occ.id,
    occ.anchor_id,
    shard.file_id,
    symbol_key.clone(),
    target_candidates.clone(),
    occ.kind,
    occ.provenance,
    occ.confidence,
));
```

**Verify command**: `cargo build -p perl-workspace`

---

## Step 3: Update method return types

**File**: `crates/perl-workspace/src/semantic/references.rs`

**What**: Change the return types of `get_by_name` and `get_by_entity` to explicitly include Arc in the slice type.

**Where**: Lines 128 and 133

**Before**:
```rust
pub fn get_by_name(&self, symbol_key: &str) -> &[ReferenceEdge] {
    self.references_by_name.get(symbol_key).map(Vec::as_slice).unwrap_or_default()
}

pub fn get_by_entity(&self, entity_id: EntityId) -> &[ReferenceEdge] {
    self.references_by_entity.get(&entity_id).map(Vec::as_slice).unwrap_or_default()
}
```

**After**:
```rust
pub fn get_by_name(&self, symbol_key: &str) -> &[Arc<ReferenceEdge>] {
    self.references_by_name.get(symbol_key).map(Vec::as_slice).unwrap_or_default()
}

pub fn get_by_entity(&self, entity_id: EntityId) -> &[Arc<ReferenceEdge>] {
    self.references_by_entity.get(&entity_id).map(Vec::as_slice).unwrap_or_default()
}
```

**Note**: Method bodies do not change. The conversion happens automatically because the HashMap values are now `Vec<Arc<ReferenceEdge>>`.

**Verify command**: `cargo build -p perl-workspace`

---

## Step 4: Verify Deref works for mutating remove_file

**File**: `crates/perl-workspace/src/semantic/references.rs`

**What**: Verify that lines 116 and 122 still work unchanged. Arc implements Deref, so `.file_id` dereferences automatically.

**Current code** (lines 115-124):
```rust
for refs in self.references_by_name.values_mut() {
    refs.retain(|r| r.file_id != file_id);
}
self.references_by_name.retain(|_, v| !v.is_empty());

for refs in self.references_by_entity.values_mut() {
    refs.retain(|r| r.file_id != file_id);
}
self.references_by_entity.retain(|_, v| !v.is_empty());
```

**Status**: No changes needed. The closure `|r| r.file_id != file_id` dereferences Arc<ReferenceEdge> transparently to access the field.

**Verify command**: `cargo build -p perl-workspace`

---

## Step 5: Run full test suite

**What**: Verify all tests pass without modification. Existing tests operate on the returned slice via Deref coercion.

**Command**:
```bash
cargo test -p perl-workspace --lib
```

**Expected**: All tests pass, including:
- `add_file_populates_name_index` — queries return Arc<ReferenceEdge>, but Deref makes .symbol_key transparent
- `add_file_populates_entity_index` — same transparency for .occurrence_id
- `remove_file_clears_entries` — works unchanged
- `multiple_edge_targets_produce_multiple_entity_entries` — verifies that the same Arc is stored N times

**Verify command**: `cargo test -p perl-workspace --lib`

---

## Step 6: Verify no downstream compilation errors

**What**: Ensure that all crates that call `ReferenceIndex::get_by_name` or `get_by_entity` still compile. Deref coercion should make Arc<ReferenceEdge> transparent.

**Command**:
```bash
cargo build --workspace
```

**Expected**: Workspace compiles. Callers in LSP providers (perl-lsp, perl-lsp-rs) iterate over the returned slice. Deref coercion handles Arc → ReferenceEdge transparently.

**Verify command**: `cargo build --workspace`

---

## Step 7: Check formatting and clippy

**What**: Run standard code quality gates.

**Commands**:
```bash
cargo xtask fmt
cargo clippy -p perl-workspace --lib
```

**Expected**: No clippy warnings. Code formatted per project style.

**Verify command**: `cargo clippy -p perl-workspace --lib`

---

## Compilation Dependencies

**Order**: Steps 1 → 2 → 3 must be done in sequence (each builds on the previous).
- Step 1 introduces the Arc type and field types
- Step 2 changes the initialization to wrap in Arc
- Step 3 updates return types to match the new field types
- Steps 4–7 verify correctness and quality

Stepping in order ensures the code compiles at each checkpoint.

---

## Verification Summary

| Checkpoint | Command | Expected |
|---|---|---|
| After Step 1 | `cargo build -p perl-workspace` | Compiles (fields renamed) |
| After Step 2 | `cargo build -p perl-workspace` | Compiles (Arc wrapping in place) |
| After Step 3 | `cargo build -p perl-workspace` | Compiles (return types updated) |
| After Step 4 | `cargo build -p perl-workspace` | Compiles (Deref still works) |
| After Step 5 | `cargo test -p perl-workspace --lib` | All tests pass |
| After Step 6 | `cargo build --workspace` | Full workspace compiles |
| After Step 7 | `cargo clippy -p perl-workspace --lib` | No warnings |

