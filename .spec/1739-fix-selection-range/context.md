# Context: documentSymbol selectionRange Fix

## Problem Statement

The `textDocument/documentSymbol` LSP response violates LSP 3.17 conformance. The `selectionRange` field is incorrectly set to the full symbol location (same as `range`) instead of just the symbol name/identifier span.

**LSP 3.17 Spec Requirement**: `selectionRange` "should be the range that should be selected and revealed when this symbol is being picked, e.g., the name of the function." This allows editors to distinguish between:
- `range` — full symbol body/construct
- `selectionRange` — just the symbol name/identifier

**Current Bug**: Both are identical, spanning the entire symbol construct.

**Example**:
```
Input:   sub foo { my $x = 1; }
Current: range=[0, 24], selectionRange=[0, 24] (both span entire line)
Expected: range=[0, 24], selectionRange=[4, 7] (selectionRange spans only "foo")
```

---

## Root Cause Analysis

### Location of Bug

**File**: `crates/perl-lsp-rs-core/src/providers/document_symbols/mod.rs`  
**Lines**: 150 and 170

Both locations incorrectly call the same `symbol_range()` function for both `range` and `selection_range`:

```rust
// Line 145-152 (source_backed_document_symbol)
Some(DocumentSymbol {
    name: document_symbol_name(symbol),
    detail: document_symbol_detail(symbol),
    kind: document_symbol_kind(symbol),
    range: symbol_range(source, symbol),                 // ✓ correct
    selection_range: symbol_range(source, symbol),       // ✗ BUG: should be symbol_name_range()
    children,
})

// Line 165-172 (source_backed_leaf_symbol)
Some(DocumentSymbol {
    name: document_symbol_name(symbol),
    detail: document_symbol_detail(symbol),
    kind: document_symbol_kind(symbol),
    range: symbol_range(source, symbol),                 // ✓ correct
    selection_range: symbol_range(source, symbol),       // ✗ BUG: should be symbol_name_range()
    children: Vec::new(),
})
```

### Why the Bug Exists

The **source-backed provider** (compiler-derived symbols from parsed AST) does not have name-position information stored in the `Symbol` struct. The `Symbol` struct has:
- `.location` — full span of the symbol construct
- `.name` — the symbol identifier (string, no position info)

The fallback **regex-based provider** (`crates/perl-lsp-rs/src/runtime/language/symbols.rs:220-284`) solves this by capturing the name match position during regex processing:

```rust
// Fallback provider (CORRECT)
if let Some(name_match) = captures.get(1) {
    let name = name_match.as_str().to_string();
    let start_char = byte_to_utf16_col(line, name_match.start());
    let end_char = byte_to_utf16_col(line, name_match.end());
    
    symbols.push(json!({
        "name": name,
        "selectionRange": {
            "start": { "line": line_num, "character": start_char },  // ✓ name position
            "end": { "line": line_num, "character": end_char }       // ✓ name position
        }
    }));
}
```

The source-backed provider **never implemented this**; it just reused `symbol_range()` for both fields.

---

## Design Decisions

### Decision 1: Compute Name Span at Provider Layer (Not Parser Layer)

**Options Considered**:
1. **Store name_start/name_end in Symbol struct** — Requires changes to parser + semantic analyzer
2. **Compute name span at provider layer** — Extract from source text within symbol bounds (chosen)

**Rationale for #2**:
- **Scope**: Parser changes are risky; name extraction is deterministic and cheap
- **Evidence**: Fallback provider proves this works (regex-based name extraction is reliable)
- **Cost**: ~20 lines of code vs. multi-module refactoring
- **Robustness**: Fallback path handles edge cases (name not found → return full range)

### Decision 2: String Search for Name (Not Regex)

**Options Considered**:
1. Regex match within symbol bounds
2. Simple string search (`source_slice.find(&symbol.name)`)

**Rationale for #2**:
- **Simplicity**: Perl identifiers are simple; no special chars
- **Performance**: Single `find()` call vs. regex compile + match
- **Robustness**: Exact name match required; ambiguous matches are rare in well-formed code
- **Precedent**: Fallback provider uses regex for line-level extraction; we use string search for sub-range (orthogonal)

### Decision 3: Fallback to Full Range on Name-Not-Found

**Options Considered**:
1. Panic on name not found
2. Return None (skip symbol)
3. Return full `symbol_range()` (fallback to current behavior)

**Rationale for #3**:
- **Defensive**: Handles unexpected symbol kinds gracefully
- **UX**: Current LSP behavior (selectionRange == range) is incorrect but not broken
- **Safety**: No panics; no symbols mysteriously disappear
- **Audit trail**: Fallback is visible in diff, easy to diagnose if it's triggered

---

## Alternatives Rejected

### 1. Add name_start/name_end to Symbol struct

**Why rejected**:
- Requires changes to `perl-semantic-analyzer` (parser layer)
- Requires updating all symbol extraction code paths
- Scope creep; this issue should be isolated to provider layer
- Parser is high-risk; provider is low-risk

### 2. Ignore the bug (status quo)

**Why rejected**:
- LSP spec violation; editors that rely on `selectionRange` for breadcrumbs/outline will fail
- Fallback provider already implements correctly; source-backed provider is inconsistent
- User impact: outline navigation broken for users with parser enabled

### 3. Use regex to extract name position

**Why rejected**:
- Overkill for simple identifier matching
- Adds regex compilation overhead per symbol
- Identifier-level parsing belongs to semantic analyzer, not provider

---

## Cross-References

### Related Issues

- **#1424** (Modern Specification for Symbolic Intelligence) — Broader semantic intelligence work; this issue is independent and does not block #1424
- **#1695** (selectionRange expansion) — May relate to breadcrumb improvements; verify no overlap
- **#1696** (documentLink) — Different provider surface; no overlap
- **#1691** (POD folding) — Different provider; no overlap
- **#1693** (region folding) — Different provider; no overlap

### Contracts Referenced

- **LSP 3.17 Spec § textDocument/documentSymbol** — Protocol definition
- **PARSER_CONTRACTS.md** — Symbol location semantics (Symbol.location spans entire construct)
- **WireRange conversion** — UTF-16 byte-offset conversion (no changes needed)

### Code Anchors

- `crates/perl-lsp-rs-core/src/providers/document_symbols/mod.rs:149-150` — source-backed provider bug
- `crates/perl-lsp-rs-core/src/providers/document_symbols/mod.rs:169-170` — leaf symbol bug
- `crates/perl-lsp-rs/src/runtime/language/symbols.rs:250-252, 274-276` — fallback provider (correct implementation)
- `crates/perl-semantic-analyzer/src/analysis/symbol.rs:56` — Symbol struct definition

---

## Verification Trail

### Scout Verification ✓

Code anchors confirmed:
```bash
grep -n "selection_range: symbol_range" crates/perl-lsp-rs-core/src/providers/document_symbols/mod.rs
# Output:
# 150:        selection_range: symbol_range(source, symbol),
# 170:        selection_range: symbol_range(source, symbol),
```

Fallback provider confirms correct approach:
```bash
grep -B2 -A2 "selectionRange" crates/perl-lsp-rs/src/runtime/language/symbols.rs | head -15
# Shows name_match.start/end used for selectionRange
```

### Research Verification ✓

- LSP 3.17 spec claims verified (textDocument/documentSymbol protocol)
- Perl language semantics verified (symbol names are always present in constructs)
- Codebase infrastructure verified (Symbol struct, WireRange API)

### Architecture Verification ✓

- Change is isolated to single provider module
- No cross-crate boundary changes
- No public API changes
- Fallback provider proves approach is sound

---

## Risk Assessment

### Low Risk Because:

1. **Isolated change**: Single module (`document_symbols/mod.rs`), two call sites
2. **Fallback behavior**: Name-not-found → return full range (defensive)
3. **No breaking changes**: Response is narrowing `selectionRange` (strictly valid per LSP)
4. **Proven pattern**: Fallback provider already implements correctly
5. **Well-tested**: Integration test validates full roundtrip

### Mitigation:

- Unit tests cover symbol kinds (sub, package, variable, Moose attribute)
- Integration test validates all symbols satisfy `selectionRange ⊆ range` invariant
- Fallback path ensures no panics on edge cases
- Diff review verifies `range` field unchanged

---

## Implementation Notes for Builder

### Build Order

1. Add `symbol_name_range()` helper (no dependencies)
2. Update call site 1 (line 150)
3. Update call site 2 (line 170)
4. Add unit tests
5. Add integration test
6. Verify full workspace

### Common Pitfalls to Avoid

1. **Do NOT modify `range` field** — Keep it calling `symbol_range()` unchanged
2. **Do NOT add fields to Symbol struct** — Would require semantic analyzer changes
3. **Do NOT panic on name-not-found** — Use fallback instead
4. **Do NOT use unwrap() in helper** — Use `.get()` with fallback
5. **Do NOT change UTF-16 conversion** — Reuse `WireRange::from_byte_offsets()` unchanged

### Test Writing Guidance

- Tests should use `assert_eq!(actual, expected)` on byte positions
- Convert byte positions to char positions for readability (show both)
- Use simple, single-symbol test cases for units
- Use multi-symbol Perl source for integration test
- Verify `selectionRange.start_char < range.start_char OR selectionRange starts after keyword`

---

## Appendix: Code Snippets

### Expected Helper Function

```rust
fn symbol_name_range(source: &str, symbol: &Symbol) -> WireRange {
    let source_slice = source
        .get(symbol.location.start..symbol.location.end)
        .unwrap_or("");
    
    if let Some(name_start) = source_slice.find(&symbol.name) {
        let byte_start = symbol.location.start + name_start;
        let byte_end = byte_start + symbol.name.len();
        WireRange::from_byte_offsets(source, byte_start, byte_end)
    } else {
        symbol_range(source, symbol)
    }
}
```

### Example Test Structure

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_symbol_name_range_subroutine() {
        let source = "sub foo { my $x = 1; }";
        let symbol = Symbol {
            name: "foo".to_string(),
            location: SourceLocation { start: 0, end: 22 },
            // ... other fields
        };
        
        let name_range = symbol_name_range(source, &symbol);
        // Verify name_range points to "foo" only (not "sub foo")
    }
}
```

---

## Success Definition

The fix is complete when:

1. ✓ `cargo test -p perl-lsp-rs-core symbol_name_range` passes all unit tests
2. ✓ `cargo test -p perl-lsp-rs-core document_symbols_selection_range` passes integration test
3. ✓ `cargo test -p perl-lsp-rs-core` — all existing tests still pass
4. ✓ `cargo clippy -p perl-lsp-rs-core` — no warnings
5. ✓ `cargo fmt -p perl-lsp-rs-core` — formatted
6. ✓ Manual verification: selectionRange is narrower than range for all symbol kinds
