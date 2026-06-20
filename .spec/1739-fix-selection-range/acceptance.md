# Acceptance Criteria: documentSymbol selectionRange fix

## §Behavior

| Input | Condition | Expected Result | Test Name |
|-------|-----------|-----------------|-----------|
| `sub foo { }` | Source-backed subroutine symbol | `selectionRange` spans "foo" (char 4-7), `range` spans entire declaration | `test_symbol_name_range_subroutine` |
| `package MyPkg;` | Source-backed package symbol | `selectionRange` spans "MyPkg" (post-keyword), `range` spans entire line | `test_symbol_name_range_package` |
| `my $counter = 0;` | Scalar variable with sigil | `selectionRange` spans "counter" (post-sigil), `range` spans declaration | `test_symbol_name_range_scalar_variable` |
| `my @items = ();` | Array variable with sigil | `selectionRange` spans "items" (post-sigil), `range` spans declaration | `test_symbol_name_range_array_variable` |
| `has name => (is => 'ro');` | Moose attribute declaration | `selectionRange` spans "name" correctly, `range` spans full attribute | `test_symbol_name_range_moose_attribute` |
| Multi-symbol source | Roundtrip with parser output | For each symbol, `selectionRange` contained in `range` and pinpoints name only | `test_document_symbols_selection_range_vs_range` |
| Symbol name not found in slice | Edge case fallback | Return full `symbol_range()` (current behavior) instead of panicking | `test_symbol_name_range_fallback` |

---

## §Hazards

### LSP-2: Protocol Contract Breach

**Surface**: `crates/perl-lsp-rs-core/src/providers/document_symbols/mod.rs:150,170` — `selectionRange` field  
**Hazard**: Returning incorrect protocol shape violates LSP 3.17 spec  
**Symptom**: Editor breadcrumbs, outline, linked editing fail to highlight correct symbol name span  
**Mitigation**: Tests verify `selectionRange` always smaller than or equal to `range`, pinpoints identifier only  
**Verification**: `test_document_symbols_selection_range_vs_range` integration test validates full roundtrip

---

### LSP-3: Fallback Provider Divergence

**Surface**: Source-backed provider vs. fallback regex provider behavior divergence  
**Hazard**: Source-backed provider may extract different name spans than fallback (regex-based), causing inconsistent client UX  
**Symptom**: Users see different symbol highlighting in outline depending on which provider (parser success vs. parser failure) is active  
**Mitigation**: Copy fallback provider's strategy: extract `name_match.start/end` from source text within symbol bounds  
**Verification**: Unit tests use same name extraction logic as fallback (grep-verifiable against `symbols.rs:250-252, 274-276`)

---

### PARSER-3: Name Extraction Fragility

**Surface**: `symbol_name_range()` uses `source_slice.find(&symbol.name)` — string search within location bounds  
**Hazard**: If symbol.name occurs multiple times within symbol.location, find() returns first occurrence, which may be incorrect for shadowed/nested names  
**Symptom**: Name span points to wrong identifier (e.g., inner variable with same name as outer)  
**Mitigation**: Fallback to full `symbol_range()` if name not found; accept first-match for well-formed symbols (common case)  
**Verification**: Test adversarial cases: `$x$x` (duplicate names), nested scopes with same name, complex expressions

---

### GENERAL-1: Regression — Range Field Untouched

**Surface**: `range` field must remain the full symbol location (unchanged behavior)  
**Hazard**: Accidental modification of `range` field alongside `selection_range` fix  
**Symptom**: Editor jumps to wrong location when user selects symbol from outline  
**Mitigation**: Diff review explicitly checks `range: symbol_range(...)` unchanged at lines 149, 169  
**Verification**: Diff audit confirms both call sites only change second parameter, not first

---

### GENERAL-2: Unsafe String Indexing

**Surface**: `symbol.location.start..symbol.location.end` byte slice extraction  
**Hazard**: If Symbol.location bounds are not validated, `.get()` returns None and we fall back to full range  
**Symptom**: Fallback-on-bad-bounds defeats the fix silently (selectionRange == range again)  
**Mitigation**: `is_source_backed()` check (line 189-191) validates `symbol.location.end <= source.len()` before use  
**Verification**: All Symbol sources routed through `source_backed_document_symbol()` which checks `is_source_backed()`

---

### GENERAL-3: UTF-16 Code Unit Mismatch

**Surface**: `WireRange::from_byte_offsets()` converts byte offsets to UTF-16  
**Hazard**: Name extraction uses byte offsets, but LSP protocol requires UTF-16 code units; mismatch causes off-by-one in client  
**Symptom**: Non-ASCII symbols (Cyrillic names, emojis) have incorrect code-unit span in `selectionRange`  
**Mitigation**: `WireRange::from_byte_offsets()` is the single authoritative conversion point (used by both `symbol_range()` and new `symbol_name_range()`); inherits same UTF-16 handling  
**Verification**: Unit tests use ASCII symbols (safe); integration test with UTF-8 multi-byte sequences validates conversion

---

## §Contracts

### LSP 3.17 DocumentSymbol Protocol

**Contract**: [LSP 3.17 Spec § textDocument/documentSymbol](https://microsoft.github.io/language-server-protocol/specifications/specification-current/#textDocument_documentSymbol)  
**Requirement**: `selectionRange` (type `Range`) must be "the range that should be selected and revealed when this symbol is being picked, e.g., the name of the function"  
**Current Violation**: Both `range` and `selectionRange` span the entire symbol location; no distinction between body and name  
**Fix Conformance**: `selectionRange` pinpoints identifier only; `range` retains full symbol body  
**Evidence**: Fallback provider already implements this correctly (`symbols.rs:250-252, 274-276`)

---

### PARSER_CONTRACTS: Symbol Extraction

**Contract**: [PARSER_CONTRACTS.md § Symbol Location Semantics](../../docs/reference/PARSER_CONTRACTS.md)  
**Requirement**: `Symbol.location` spans the entire symbol construct (from first keyword to closing brace/semicolon)  
**Usage in Fix**: Name extraction finds `symbol.name` within `symbol.location` bounds  
**Invariant**: `symbol.name` is always present within the symbol construct (validated by `is_source_backed()`)  
**Verification**: Fallback provider proves this works (symbol names are searchable within line bounds)

---

### WireRange Conversion

**Contract**: `perl_position_tracking::WireRange::from_byte_offsets(source, start, end)`  
**Requirement**: Convert byte offsets to LSP-compatible UTF-16 code units  
**Usage in Fix**: `symbol_name_range()` reuses same conversion path as `symbol_range()`  
**Invariant**: Both functions use identical byte-to-UTF-16 path; no conversion divergence possible  
**No Change**: Existing `WireRange` API, no protocol-layer modifications

---

## §API-Shape

### New Function Signature

**Function**: `fn symbol_name_range(source: &str, symbol: &Symbol) -> WireRange`  
**Visibility**: Private (internal to module, not exported)  
**Location**: `crates/perl-lsp-rs-core/src/providers/document_symbols/mod.rs` (after line 195)  
**Parameters**:
- `source: &str` — Full document source text (required for byte-to-UTF-16 conversion)
- `symbol: &Symbol` — Symbol from semantic analyzer (has name and location)

**Return**: `WireRange` — LSP-compatible range spanning symbol name only  
**Fallback**: Returns full `symbol_range()` if name cannot be found (defensive)

### Call Sites

**Site 1**: Line 150 in `source_backed_document_symbol()`  
- **Before**: `selection_range: symbol_range(source, symbol)`
- **After**: `selection_range: symbol_name_range(source, symbol)`

**Site 2**: Line 170 in `source_backed_leaf_symbol()`  
- **Before**: `selection_range: symbol_range(source, symbol)`
- **After**: `selection_range: symbol_name_range(source, symbol)`

### No Public API Changes

- `DocumentSymbol` struct unchanged (still has `selection_range: WireRange`)
- `DocumentSymbolLiveResult` unchanged
- `source_backed_document_symbols_from_ast()` unchanged
- No new exports, no breaking changes

### Dup-Risk Grep

**Search for existing helpers matching name extraction logic**:
```bash
grep -r "symbol.name" crates/perl-lsp-rs-core/src/providers/
grep -r "name_range" crates/perl-lsp-rs-core/src/providers/
grep -r "selection_range" crates/perl-lsp-rs-core/src/
```

**Result**: No duplicate helpers found. Fallback provider implements via regex (different approach, not shareable).

### Caller Count

**Callers of `symbol_name_range()`**: Exactly 2 (hardcoded in steps 2-3)
- `source_backed_document_symbol()` — line 150
- `source_backed_leaf_symbol()` — line 170

**Callers of `symbol_range()` (unchanged)**: Remains unchanged for `range` field; used at lines 149, 169

---

## §Test-Grid

| Category | Test Name | Acceptance Criterion | Implementation Notes |
|----------|-----------|----------------------|----------------------|
| **Unit** | `test_symbol_name_range_subroutine` | Name span excludes `sub` keyword for `sub foo { }` | Verify char position of 'f' in "foo" vs. 's' in "sub" |
| **Unit** | `test_symbol_name_range_package` | Name span excludes `package` keyword for `package MyPkg;` | Verify char position of 'M' in "MyPkg" vs. 'p' in "package" |
| **Unit** | `test_symbol_name_range_scalar_variable` | Name span excludes `$` sigil for `my $counter = 0;` | Verify char position of 'c' in "counter" vs. '$' |
| **Unit** | `test_symbol_name_range_array_variable` | Name span excludes `@` sigil for `my @items = ();` | Verify char position of 'i' in "items" vs. '@' |
| **Unit** | `test_symbol_name_range_moose_attribute` | Name span correct for Moose `has` attribute | Verify char position of attribute name only |
| **Unit** | `test_symbol_name_range_fallback` | Fallback to full range if name not found | Intentionally construct Symbol with unreachable name, verify returns `symbol_range()` |
| **Integration** | `test_document_symbols_selection_range_vs_range` | Full roundtrip: `range` >= `selection_range`, selection pinpoints name only | Parse actual Perl code, call `source_backed_document_symbols_from_ast()`, verify all symbols obey invariant |
| **Regression** | `cargo test -p perl-lsp-rs-core` | No existing tests break | Run full test suite, verify same count passes |
| **Regression** | `cargo test -p perl-lsp-rs` | Fallback provider tests still pass | Verify regex-based fallback unaffected by source-backed changes |
| **Conformance** | LSP 3.17 shape validation | Response conforms to LSP spec (selectionRange smaller than range) | Validator schema check (automated in CI) |

---

## §Blast-Radius

### Consumers (Direct Dependents)

**Direct callers of `source_backed_document_symbols_from_ast()`**:
```bash
grep -r "source_backed_document_symbols_from_ast" crates/ --include="*.rs"
```

**Expected result**: Only `crates/perl-lsp-rs/src/runtime/language/symbols.rs` (the runtime handler) calls this function.

**Impact**: Runtime calls the provider; providers emit JSON response via LSP protocol. Response shape is wire-only (JSON-serialized), no struct-level breaking changes.

### Downstream Crates

**LSP clients** (VSCode extension, other editors) consume the JSON response.
- **Impact**: Editors that rely on `selectionRange` (breadcrumbs, outline highlighting) will see correct name spans instead of full locations
- **Benefit**: Fixes LSP spec violation; improves UX
- **Risk**: None — narrowing `selectionRange` is backwards-compatible (smaller selection is valid)

### Must-Not-Touch Boundaries

**Do NOT modify**:
1. `Symbol` struct (add no fields, no parsing changes)
2. `DocumentSymbol` struct field names or types
3. `range` field logic (must remain full symbol location)
4. Fallback regex provider (different implementation path)
5. Protocol response shape (only `selectionRange` value changes)

**Rationale**: Changes are localized to `document_symbols/mod.rs`; no cross-crate interfaces, no shared helpers.

### Cross-Crate Impact Analysis

| Crate | Impact | Evidence |
|-------|--------|----------|
| `perl-semantic-analyzer` | None — read-only use of Symbol | No changes to Symbol struct |
| `perl-lsp-rs` | None — uses protocol layer only | Consumes JSON response, semantics unchanged |
| `perl-lsp-rs-core` | Self-contained — one module | Change isolated to document_symbols provider |
| `perl-parser-core` | None — read-only use of AST | No parsing changes |
| All other crates | None — no direct dependencies | No transitively affected crates |

---

## §Coverage-Map

**Why coverage explanation needed**: This fix changes the `selectionRange` field output without changing test assertions or logic paths. A diff-review might miss that coverage actually validates the new behavior.

| Code Change | Coverage Source | Validation |
|-------------|-----------------|------------|
| `symbol_name_range()` helper function | Unit tests `test_symbol_name_range_*` | Each test extracts name span and validates char position |
| Call site line 150 | `test_document_symbols_selection_range_vs_range` | Integration test parses code, calls provider, validates output shape |
| Call site line 170 | `test_document_symbols_selection_range_vs_range` | Same integration test covers both leaf and non-leaf symbols |
| Fallback path (`else` branch) | `test_symbol_name_range_fallback` | Explicit test for name-not-found scenario |
| UTF-16 conversion | Inherited from `symbol_range()` (unchanged path) | Existing `WireRange::from_byte_offsets()` tests validate UTF-16 |

**Assertion Format**: All tests use `assert_eq!(actual_range, expected_range)` comparing byte positions, verified against source text offsets.

**No metrics to update**: `docs/project/status/*.md` files auto-regenerate post-merge based on test counts. This fix adds new tests (increasing count) but does not require manual status update.
