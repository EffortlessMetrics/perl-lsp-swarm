# Implementation Checklist: fix(lsp) documentSymbol selectionRange must span symbol name, not full location

## Overview
Fix LSP 3.17 spec violation in document-symbol provider where `selectionRange` incorrectly spans the full symbol location instead of just the symbol name/identifier.

**Branch**: `impl/1739-fix-selection-range`  
**Crate**: `perl-lsp-rs-core`  
**File**: `crates/perl-lsp-rs-core/src/providers/document_symbols/mod.rs`  
**Size**: S (one helper function + two call-site updates)

---

## Step 1: Add helper function `symbol_name_range()`

**File**: `crates/perl-lsp-rs-core/src/providers/document_symbols/mod.rs`  
**Location**: After `symbol_range()` function (after line 195)  
**What**: Add new internal function to extract the name span from a symbol

```rust
/// Extract the byte range of the symbol name within its source location.
///
/// For subroutines: returns span of name only, excluding `sub` keyword
/// For packages: returns span of name only, excluding `package` keyword
/// For variables: returns span of name only, excluding sigil ($, @, %)
/// For Moose `has` attributes: returns span of the attribute name
///
/// # Arguments
/// * `source` - The full source text
/// * `symbol` - The symbol whose name span should be extracted
///
/// # Returns
/// `WireRange` spanning just the symbol identifier (post-keyword, post-sigil)
fn symbol_name_range(source: &str, symbol: &Symbol) -> WireRange {
    let source_slice = source
        .get(symbol.location.start..symbol.location.end)
        .unwrap_or("");
    
    // Extract name span relative to symbol location
    if let Some(name_start) = source_slice.find(&symbol.name) {
        let byte_start = symbol.location.start + name_start;
        let byte_end = byte_start + symbol.name.len();
        WireRange::from_byte_offsets(source, byte_start, byte_end)
    } else {
        // Fallback: return the full symbol range if name cannot be found
        symbol_range(source, symbol)
    }
}
```

**Dependencies**: None — function uses existing `symbol_range()` and `symbol` data  
**Verify**: 
```bash
cargo build -p perl-lsp-rs-core
```

---

## Step 2: Update `source_backed_document_symbol()` to use `symbol_name_range()` for selection_range

**File**: `crates/perl-lsp-rs-core/src/providers/document_symbols/mod.rs`  
**Location**: Line 150  
**Change**: Replace second `symbol_range(source, symbol)` with `symbol_name_range(source, symbol)`

**Before**:
```rust
    Some(DocumentSymbol {
        name: document_symbol_name(symbol),
        detail: document_symbol_detail(symbol),
        kind: document_symbol_kind(symbol),
        range: symbol_range(source, symbol),
        selection_range: symbol_range(source, symbol),  // ← FIX THIS
        children,
    })
```

**After**:
```rust
    Some(DocumentSymbol {
        name: document_symbol_name(symbol),
        detail: document_symbol_detail(symbol),
        kind: document_symbol_kind(symbol),
        range: symbol_range(source, symbol),
        selection_range: symbol_name_range(source, symbol),  // ← FIXED
        children,
    })
```

**Dependencies**: Requires Step 1 (helper function must exist)  
**Verify**: 
```bash
cargo build -p perl-lsp-rs-core
```

---

## Step 3: Update `source_backed_leaf_symbol()` to use `symbol_name_range()` for selection_range

**File**: `crates/perl-lsp-rs-core/src/providers/document_symbols/mod.rs`  
**Location**: Line 170  
**Change**: Replace second `symbol_range(source, symbol)` with `symbol_name_range(source, symbol)`

**Before**:
```rust
    Some(DocumentSymbol {
        name: document_symbol_name(symbol),
        detail: document_symbol_detail(symbol),
        kind: document_symbol_kind(symbol),
        range: symbol_range(source, symbol),
        selection_range: symbol_range(source, symbol),  // ← FIX THIS
        children: Vec::new(),
    })
```

**After**:
```rust
    Some(DocumentSymbol {
        name: document_symbol_name(symbol),
        detail: document_symbol_detail(symbol),
        kind: document_symbol_kind(symbol),
        range: symbol_range(source, symbol),
        selection_range: symbol_name_range(source, symbol),  // ← FIXED
        children: Vec::new(),
    })
```

**Dependencies**: Requires Step 1 (helper function must exist)  
**Verify**: 
```bash
cargo build -p perl-lsp-rs-core
```

---

## Step 4: Write unit tests for `symbol_name_range()` helper

**File**: `crates/perl-lsp-rs-core/src/providers/document_symbols/mod.rs`  
**Location**: Add inline `#[cfg(test)]` module at end of file (after line 250+)  
**What**: Test the helper function with various symbol kinds

Create a `#[cfg(test)]` module with tests covering:

1. **test_symbol_name_range_subroutine** — Verify name span excludes `sub` keyword
   - Input: `"sub foo { ... }"`
   - Expected: `selectionRange` starts at 'f' in "foo", not at 's' in "sub"

2. **test_symbol_name_range_package** — Verify name span excludes `package` keyword
   - Input: `"package MyPkg; ..."`
   - Expected: `selectionRange` starts at 'M' in "MyPkg", not at 'p' in "package"

3. **test_symbol_name_range_scalar_variable** — Verify name span excludes sigil
   - Input: `"my $counter = 0;"`
   - Expected: `selectionRange` starts at 'c' in "counter", not at '$'

4. **test_symbol_name_range_array_variable** — Verify name span excludes sigil
   - Input: `"my @items = ();"`
   - Expected: `selectionRange` starts at 'i' in "items", not at '@'

5. **test_symbol_name_range_moose_attribute** — Verify Moose `has` attribute correct
   - Input: `"has name => (is => 'ro');"`
   - Expected: `selectionRange` spans the attribute name correctly

**Verify**:
```bash
cargo test -p perl-lsp-rs-core symbol_name_range -- --nocapture
```

---

## Step 5: Write integration test — roundtrip with actual document symbols

**File**: `crates/perl-lsp-rs-core/src/providers/document_symbols/mod.rs` (same test module)  
**What**: Integration test verifying the fix end-to-end

Create test **test_document_symbols_selection_range_vs_range**:
- Parse actual Perl code with multiple symbol kinds
- Call `source_backed_document_symbols_from_ast()`
- Verify that for EACH symbol:
  - `range` spans the full definition (start line/char to end line/char)
  - `selection_range` is strictly smaller and spans only the name/identifier
  - `selection_range.start <= range.start` OR starts after keywords/sigils
  - `selection_range.end <= range.end`

Example assertions:
```rust
// For subroutine "sub foo { ... }"
assert!(doc_symbol.range.start_character < doc_symbol.selection_range.start_character);  // range starts at "sub", selection starts at "foo"
assert!(doc_symbol.selection_range.end_character <= doc_symbol.range.end_character);      // selection is contained in range
```

**Verify**:
```bash
cargo test -p perl-lsp-rs-core document_symbols_selection_range -- --nocapture
```

---

## Step 6: Verify no regression with workspace tests

**Verify all crate tests still pass**:
```bash
cargo test -p perl-lsp-rs-core
```

**Verify against fallback provider semantics** (compare correctness with `crates/perl-lsp-rs/src/runtime/language/symbols.rs`):
```bash
RUST_TEST_THREADS=2 cargo test -p perl-lsp-rs -- --test-threads=2 document_symbol
```

**Lint and format**:
```bash
cargo fmt -p perl-lsp-rs-core
cargo clippy -p perl-lsp-rs-core
```

---

## Compilation Order

1. **Step 1**: Add helper function (no dependencies, must compile)
2. **Step 2-3**: Update call sites (depend on Step 1, must compile)
3. **Step 4-5**: Add tests (depend on Steps 1-3, must pass)
4. **Step 6**: Verify workspace (all tests pass, no clippy warnings)

Each step must compile and the crate must be in a valid state.

---

## Success Criteria

- All unit tests in `symbol_name_range` test module pass
- Integration test verifies roundtrip correctness
- Workspace tests pass
- No clippy warnings
- Code follows `CLAUDE.md` standards (no unwrap/expect/panic in production code)

---

## Rollback Plan

If the helper function cannot reliably extract the name span, fallback to the full `symbol_range()` (current behavior). The fallback is built into the helper:
```rust
} else {
    symbol_range(source, symbol)  // fallback
}
```

This ensures robustness — if name extraction fails for a symbol kind, we return the full range (current LSP behavior) rather than panicking.
