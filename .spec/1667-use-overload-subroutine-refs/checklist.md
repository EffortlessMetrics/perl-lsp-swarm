# Implementation Checklist: use overload Subroutine References

Issue: #1667 — Extract use overload subroutine references for find-references tracking

**Approach**: Option B — Special-case overload in symbol extractor (isolated, pragmatic)

Scope: 1 file, ~100-150 LOC added, no AST changes

## Dependency Graph

```
1. Add overload handler to symbol extractor (symbol.rs)
2. Extract subroutine refs from overload args with regex
3. Create SymbolReference entries for each found ref
4. Write failing tests for overload find-references
5. Verify compilation and test pass
```

## Implementation Steps

### Step 1: Add overload handler in symbol.rs

**File**: `crates/perl-semantic-analyzer/src/analysis/symbol.rs`

**What changes**: Add a new method `synthesize_overload_references()` and call it from the `NodeKind::Use` handler for `module == "overload"`.

**Where**: After the existing handlers for "constant", "Class::Tiny", "Readonly", "Const::Fast", "EV" (around line 842).

**Signature**:
```rust
fn synthesize_overload_references(&mut self, args: &[String], location: SourceLocation)
```

**Dependencies**: None — uses existing regex patterns and SymbolReference creation.

**Change order**: Step 1 (no dependencies)

**Verify command**: 
```bash
cargo clippy -p perl-semantic-analyzer --lib
```

---

### Step 2: Implement overload operator → subroutine mapping

**File**: `crates/perl-semantic-analyzer/src/analysis/symbol.rs`

**What changes**: Parse the overload args to find `\&SUBROUTINE` patterns.

**Pattern**: The overload args are stored as strings like `["+", "\\&add", "<<", "\\&lshift", ...]` (operator => reference pairs).

**Logic**:
1. Iterate args in pairs: `args.chunks(2)`
2. For each (operator, reference) pair:
   - Check if reference starts with `\&`
   - Extract the subroutine name after `\&`
   - Create a SymbolReference with `kind: SymbolKind::Subroutine`
3. Add references to the symbol table

**Implementation pattern** (follow existing `synthesize_use_constant_symbols`):
```rust
for chunk in args.chunks(2) {
    if chunk.len() == 2 {
        let ref_str = &chunk[1];
        if let Some(sub_name) = ref_str.strip_prefix("\\&") {
            // Create SymbolReference for sub_name
        }
    }
}
```

**Dependencies**: Step 1

**Change order**: Step 2

**Verify command**:
```bash
cargo clippy -p perl-semantic-analyzer --lib
cargo build -p perl-semantic-analyzer
```

---

### Step 3: Create test for overload references

**File**: `crates/perl-semantic-analyzer/tests/use_overload_subroutine_refs_test.rs` (NEW FILE)

**What changes**: Write a test that verifies SymbolReference entries are created for overload subroutine references.

**Test structure**:
```rust
#[test]
fn test_use_overload_operator_subroutine_reference_extraction() {
    let source = r#"
        package Vector;
        use overload '+' => \&add;
        sub add { ... }
    "#;
    
    // Parse and extract symbols
    let table = extract_symbols(source);
    
    // Verify SymbolReference for 'add' exists with source in Use node
}
```

**Dependencies**: Steps 1-2

**Change order**: Step 3

**Verify command**:
```bash
cargo test -p perl-semantic-analyzer
```

---

### Step 4: Add LSP-level test for find-references

**File**: `crates/perl-lsp-rs/tests/lsp_bdd_workflows.rs` (or new test file)

**What changes**: Add an E2E test that verifies find-references includes the overload declaration.

**Test scenario**:
```gherkin
Scenario: find-references includes use overload subroutine targets
  Given a file with "use overload '+' => \&add; sub add { ... }"
  When I request textDocument/references for symbol 'add'
  Then the response includes the overload declaration location
```

**Dependencies**: Steps 1-3

**Change order**: Step 4

**Verify command**:
```bash
RUST_TEST_THREADS=2 cargo test -p perl-lsp-rs -- --test-threads=2 find_references
```

---

### Step 5: Integration test for rename refactoring

**File**: `crates/perl-lsp-rs/tests/lsp_bdd_workflows.rs` (or test_rename_subroutine_in_overload.rs)

**What changes**: Add a test that verifies renaming a subroutine used in overload correctly updates the overload declaration.

**Test scenario**:
```gherkin
Scenario: rename subroutine used in overload
  Given a file with "use overload '+' => \&add; sub add { ... }"
  When I request textDocument/rename for symbol 'add' to 'add_impl'
  Then the overload declaration is updated to "use overload '+' => \&add_impl"
```

**Dependencies**: Steps 1-4

**Change order**: Step 5 (can run in parallel with Step 4)

**Verify command**:
```bash
RUST_TEST_THREADS=2 cargo test -p perl-lsp-rs -- --test-threads=2 rename
```

---

### Step 6: Verify compilation and full test suite

**File**: N/A (verification step)

**What changes**: None

**Verify command**:
```bash
cargo fmt --all
cargo clippy --workspace
cargo test --workspace --lib
RUST_TEST_THREADS=2 cargo test -p perl-lsp-rs -- --test-threads=2
```

---

## Compilation Order

1. ✓ Step 1 — Add method declaration (no callers yet)
2. ✓ Step 2 — Implement method + call from NodeKind::Use handler
   - Code must compile: symbol extractor references the new method
3. ✓ Step 3 — Add unit test (uses public test helpers)
4. ✓ Step 4 — Add LSP test (depends on symbol extraction working)
5. ✓ Step 5 — Add rename test (depends on symbol references existing)
6. ✓ Step 6 — Full verification (all gates)

**Note**: Steps 4-5 can run in parallel once Step 3 passes.

## Test Files

| Step | Test File | Test Name | Assertion |
|------|-----------|-----------|-----------|
| 3 | `crates/perl-semantic-analyzer/tests/use_overload_subroutine_refs_test.rs` | `test_use_overload_operator_subroutine_reference_extraction` | SymbolReference created for each `\&SUB` |
| 4 | `crates/perl-lsp-rs/tests/lsp_*.rs` | `test_find_references_overload_subroutine` | References include overload decl location |
| 5 | `crates/perl-lsp-rs/tests/lsp_*.rs` | `test_rename_subroutine_in_overload` | Overload decl updated on rename |

---

## Edge Cases to Handle

1. **Single operator without reference**: `use overload '""' => 'stringify'` — skip if not `\&`
2. **Operator with code ref**: `use overload '+' => sub { ... }` — skip anonymous subs
3. **Multiple operators**: `use overload '+' => \&add, '-' => \&sub` — handle all pairs
4. **Fat arrow with spacing**: `use overload '+'=>\&add` vs `use overload '+' => \&add` — both valid, parser normalizes
5. **Qualified names**: `use overload '+' => \&Math::add` — extract `Math::add` as reference

---

## Known Limitations (Document in Context)

- **Option B pragmatism**: Does not fix other pragmas like `use parent` or `use base` (requires Option A refactoring)
- **Pattern fragility**: If parser changes how args are stringified, this regex may break
- **No AST changes**: Does not preserve overload structure for future tools (Option A advantage)

---

## Artifact Cleanup

None — all changes are production code or tests. No temporary debug files.
