# Implementation Checklist: Issue #955

## Summary
Add regression tests for `find_dependents` with `@ISA`-based inheritance patterns (`our @ISA`, bare `@ISA`, `push @ISA`) to close the coverage gap. The implementation in `package_graph_extractor.rs` already supports these patterns; tests verify the gap is test-only or surface a latent bug in indexing.

## Change Order

### Step 1: Add `test_find_dependents_via_our_isa` test
**File**: `crates/perl-workspace/tests/comprehensive_unit_tests.rs` (line 2150, after existing tests)

**What**: New test function that checks `WorkspaceIndex::find_dependents` correctly registers inheritance when declared via `our @ISA = qw(...)`.

**Test form**:
```rust
#[test]
fn test_find_dependents_via_our_isa() -> Result<(), Box<dyn std::error::Error>> {
    let index = WorkspaceIndex::new();
    let uri = file_url("/child.pm")?;
    index.index_file(uri, "package Child;\nour @ISA = qw(Base::Class);\n1;\n".to_string())?;
    
    let dependents = index.find_dependents("Base::Class");
    assert!(!dependents.is_empty(), "our @ISA = qw(Base::Class) should register Base::Class as a dependency");
    Ok(())
}
```

**Dependencies**: None (reuses existing test infrastructure).

**Verify command**: `cargo test -p perl-workspace test_find_dependents_via_our_isa -- --nocapture --test-threads=1 2>&1 | grep -E "test.*our_isa|passed|FAILED"`

---

### Step 2: Add `test_find_dependents_via_bare_isa` test
**File**: `crates/perl-workspace/tests/comprehensive_unit_tests.rs` (line 2150+N, after Step 1)

**What**: New test function that checks inheritance declared via bare `@ISA = (...)` (without `our` qualifier).

**Test form**:
```rust
#[test]
fn test_find_dependents_via_bare_isa() -> Result<(), Box<dyn std::error::Error>> {
    let index = WorkspaceIndex::new();
    let uri = file_url("/derived.pm")?;
    index.index_file(uri, "package Derived;\n@ISA = ('My::Root');\n1;\n".to_string())?;
    
    let dependents = index.find_dependents("My::Root");
    assert!(!dependents.is_empty(), "bare @ISA = (...) should register My::Root as a dependency");
    Ok(())
}
```

**Dependencies**: None; runs after Step 1.

**Verify command**: `cargo test -p perl-workspace test_find_dependents_via_bare_isa -- --nocapture --test-threads=1 2>&1 | grep -E "test.*bare_isa|passed|FAILED"`

---

### Step 3: Add `test_find_dependents_via_push_isa` test
**File**: `crates/perl-workspace/tests/comprehensive_unit_tests.rs` (line 2150+2N, after Step 2)

**What**: New test function that checks inheritance declared via `push @ISA, 'Base'` (common idiom for adding to existing inheritance list).

**Test form**:
```rust
#[test]
fn test_find_dependents_via_push_isa() -> Result<(), Box<dyn std::error::Error>> {
    let index = WorkspaceIndex::new();
    let uri = file_url("/extended.pm")?;
    index.index_file(uri, "package Extended;\npush @ISA, 'Base::Extended';\n1;\n".to_string())?;
    
    let dependents = index.find_dependents("Base::Extended");
    assert!(!dependents.is_empty(), "push @ISA, '...' should register the base as a dependency");
    Ok(())
}
```

**Dependencies**: None; runs after Step 2.

**Verify command**: `cargo test -p perl-workspace test_find_dependents_via_push_isa -- --nocapture --test-threads=1 2>&1 | grep -E "test.*push_isa|passed|FAILED"`

---

### Step 4: Run full workspace test suite
**Verify command**: `cargo test -p perl-workspace --lib 2>&1 | tail -20`

**Expected**: All three new tests pass; no regression in existing tests.

---

## File Paths (verified to exist)
- `crates/perl-workspace/tests/comprehensive_unit_tests.rs` ✓ (2150 lines, exists)
- `crates/perl-semantic-analyzer/src/analysis/package_graph_extractor.rs` ✓ (implementation verified: lines 104-173 support `@ISA` forms)

---

## Notes for Builder

1. All tests follow the existing pattern from `test_find_dependents_via_use_parent` (line 2108) and `test_find_dependents_via_use_base` (line 2124).

2. The `file_url()` helper is defined at line 28 of the same test file.

3. Each test instantiates a new `WorkspaceIndex`, indexes a single file with one of the three `@ISA` forms, and asserts `find_dependents()` finds the parent class.

4. If any test fails, the failure indicates a latent bug in the `@ISA` indexing logic in `package_graph_extractor.rs`, which should be escalated to the parser-fix / semantic-analyzer teams.

5. If all tests pass, the coverage gap is closed — confirm by grepping for `@ISA` tests after merge.
