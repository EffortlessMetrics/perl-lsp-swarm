# Implementation Checklist: Fix rename dedup() only removing consecutive duplicates

## Context
- **Issue**: #1863
- **Crate**: `crates/perl-lsp-rs-core`
- **File**: `crates/perl-lsp-rs-core/src/providers/rename/mod.rs`
- **Problem**: `Vec::dedup()` at lines 196-197 (rename) and 284-285 (scoped_rename) only removes **consecutive** equal elements. When symbols appear in both `symbol_table.symbols` and `symbol_table.references`, the resulting TextEdit objects may not be adjacent after partial sort, leaving duplicates.

## Step 1: Add Ord derive to TextEdit struct
**File**: `crates/perl-lsp-rs-core/src/providers/rename/types.rs`

**Change**: Add `Ord` and `PartialOrd` derives to TextEdit struct (line 8).

**Before**:
```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEdit {
```

**After**:
```rust
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct TextEdit {
```

**Reason**: Enables full sorting by all fields (location, then new_text). With Ord, we can use `.sort()` instead of `.sort_by_key()`, which ensures all identical TextEdit objects become adjacent for proper deduplication.

**Verification**: 
```bash
cargo check -p perl-lsp-rs-core
```

## Step 2: Replace partial sort + dedup in rename() with full sort + dedup
**File**: `crates/perl-lsp-rs-core/src/providers/rename/mod.rs`

**Change**: Lines 196-197, replace:
```rust
edits.sort_by_key(|edit| edit.location.start);
edits.dedup();
```

**With**:
```rust
edits.sort();
edits.dedup();
```

**Reason**: Full sort ensures all identical TextEdit objects (same location.start, location.end, and new_text) are adjacent, so `dedup()` will remove all true duplicates.

**Verification**:
```bash
cargo test -p perl-lsp-rs-core --lib rename::tests
```

## Step 3: Replace partial sort + dedup in scoped_rename() with full sort + dedup
**File**: `crates/perl-lsp-rs-core/src/providers/rename/mod.rs`

**Change**: Lines 284-285, replace:
```rust
edits.sort_by_key(|edit| edit.location.start);
edits.dedup();
```

**With**:
```rust
edits.sort();
edits.dedup();
```

**Reason**: Same as Step 2 — ensures deduplication works correctly for all identical edits.

**Verification**:
```bash
cargo test -p perl-lsp-rs-core --lib rename::tests
```

## Step 4: Add unit test for duplicate edit detection
**File**: `crates/perl-lsp-rs-core/src/providers/rename/mod.rs` (in tests module, around line 412)

**Add new test** (after existing tests):
```rust
#[test]
fn test_rename_no_duplicate_edits_for_shared_locations() {
    // Test that a symbol appearing in both symbol_table.symbols and 
    // symbol_table.references at the same location produces only one edit.
    // 
    // Example: "my $x = $x + 1;" has $x as both a declaration and a reference
    // at nearly the same location (the declaration location and the reference location
    // may differ by sigil handling, but if they map to the same TextEdit, 
    // we should see only one in the output).
    //
    // This test constructs a scenario where a symbol is indexed in both tables
    // and verifies no duplicate edits are returned.
    
    use std::collections::HashMap;
    
    let code = "my $x = $x + 1;\n";
    let mut parser = Parser::new(code);
    let ast = must(parser.parse());
    let provider = RenameProvider::new(&ast, code.to_string());
    
    // Find position of first $x (the declaration)
    let pos = must_some(code.find("$x")) + 1;
    let result = provider.rename(pos, "$y", &RenameOptions::default());
    
    assert!(result.is_valid, "rename should succeed");
    
    // Count edits by location to ensure no duplicates
    let mut location_count: HashMap<(usize, usize), usize> = HashMap::new();
    for edit in &result.edits {
        let key = (edit.location.start, edit.location.end);
        *location_count.entry(key).or_insert(0) += 1;
    }
    
    // Verify all counts are 1 (no duplicates)
    for ((start, end), count) in location_count {
        assert_eq!(
            count, 1,
            "location ({}, {}) has {} edits, expected 1 (no duplicates)",
            start, end, count
        );
    }
}
```

**Reason**: Directly tests the acceptance criterion — that symbols appearing in both tables do not produce duplicate edits. This test will fail before the fix (when vec.dedup() is partial) and pass after.

**Verification**:
```bash
cargo test -p perl-lsp-rs-core --lib rename::tests::test_rename_no_duplicate_edits_for_shared_locations
```

## Step 5: Run full test suite
**Command**:
```bash
cargo test -p perl-lsp-rs-core --lib rename
cargo test -p perl-lsp-rs-core
```

**Expected**: All tests pass, including the new test for duplicate edits.

## Step 6: Verify with clippy and fmt
**Commands**:
```bash
cargo fmt -p perl-lsp-rs-core
cargo clippy -p perl-lsp-rs-core --lib
```

**Expected**: No warnings or errors.

## Compilation Order

1. **Step 1 required before Step 2/3**: TextEdit must have Ord before we can call `.sort()` on a Vec<TextEdit>.
2. **Steps 2 and 3 are independent**: Both replace the same pattern in two methods; can be done in either order.
3. **Step 4 independent**: New test doesn't need to run before others; can be added anytime.
4. **Steps 5-6 are verification**: Run after all changes.

## Notes

- **No external dependencies added**: Ord is a standard trait, TextEdit fields already impl Ord (ByteSpan has Ord, String has Ord).
- **Minimal surface area**: Only 2 lines changed per method, plus 1 derive added.
- **Backward compatible**: Full sort is stricter than partial sort; no code relying on partial-sort-only behavior should break.
- **Performance**: Full sort is negligible cost for typical rename (edits vec is usually <100 items).
