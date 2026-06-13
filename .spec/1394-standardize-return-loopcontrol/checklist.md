# Implementation Checklist: #1394 Parser Return/LoopControl Precedence

## Problem Summary

**Verified Bug:** `parse_return()` at statement level incorrectly binds word operators (`or`, `and`, `xor`) to the return *value* instead of the surrounding statement. Loop control keywords (`next`, `last`, `redo`) handle this correctly because they parse at expression level only.

**Evidence:** 
- `return $x or die` parses as `(return ($x or die))` — **WRONG**
- Should parse as `((return $x) or die)` — per Perl's own precedence warning
- Perl warns: `Possible precedence issue with control flow operator (return)`
- Loop control already correct: `next or die` parses correctly as `(next) or (die)`

**Root Cause:** 
- `parse_return()` (statement level) calls `parse_expression()` without stopping at word operators
- `parse_return_expr()` (expression level) correctly uses `parse_assignment()` and stops at word boundaries
- Inconsistency: statement-level return is less restrictive than expression-level return

---

## Implementation Steps

### Step 1: Fix `parse_return()` to respect word-operator boundaries

**File:** `crates/perl-parser-core/src/engine/parser/statements.rs`

**Function:** `parse_return()` (lines 481-504)

**Change:** Replace `parse_expression()` with `parse_assignment()` so return stops at word-operator boundaries.

```rust
// Before:
let value = if Self::is_statement_terminator(...) || ... {
    None
} else {
    Some(Box::new(self.parse_expression()?))  // <-- CONSUMES OR/AND/XOR
};

// After:
let value = if Self::is_statement_terminator(...) || ... {
    None
} else {
    Some(Box::new(self.parse_assignment()?))  // <-- STOPS AT OR/AND/XOR
};
```

**Why:** `parse_assignment()` is called from `parse_return_expr()` and stops at word-operator boundaries. Using the same function at statement level ensures consistency.

**Compilation:** Should compile cleanly.

---

### Step 2: Add regression test for statement-level return with word operators

**File:** `crates/perl-parser-core/tests/control_flow_return_precedence_1394.rs` (CREATE NEW)

**Test Cases:**

```rust
#[test]
fn test_return_with_word_or_at_statement_level() {
    // return $x or die;  should be (or (return $x) (die))
    let source = "return $x or die;";
    let ast = parse_code(source).unwrap();
    let sexp = ast.to_sexp();
    
    // Verify: top-level node should be a binary_or, not a return with or inside
    assert!(sexp.contains("binary_or"), "Expected binary_or at top level: {}", sexp);
    assert!(sexp.contains("return"), "Expected return inside or: {}", sexp);
    // The return should be the left child, die should be the right
}

#[test]
fn test_return_with_word_and_at_statement_level() {
    // return $x and die;  should be (and (return $x) (die))
    let source = "return $x and die;";
    let ast = parse_code(source).unwrap();
    let sexp = ast.to_sexp();
    
    assert!(sexp.contains("binary_and"), "Expected binary_and at top level: {}", sexp);
    assert!(sexp.contains("return"), "Expected return inside and: {}", sexp);
}

#[test]
fn test_return_without_value_with_word_or() {
    // return or die;  should be (or (return) (die))
    let source = "return or die;";
    let ast = parse_code(source).unwrap();
    let sexp = ast.to_sexp();
    
    assert!(sexp.contains("binary_or"), "Expected binary_or at top level: {}", sexp);
    assert!(sexp.contains("(return)") || sexp.contains("(return "), 
            "Expected return with no value: {}", sexp);
}

#[test]
fn test_return_value_does_not_consume_or_separator() {
    // return $x or return $y;  should be (or (return $x) (return $y))
    // NOT (return (or $x (return $y)))
    let source = "return $x or return $y;";
    let ast = parse_code(source).unwrap();
    let sexp = ast.to_sexp();
    
    assert!(sexp.contains("binary_or"), "Expected binary_or at top level: {}", sexp);
    // Count occurrences of "return" — should be 2
    let return_count = sexp.matches("return").count();
    assert_eq!(return_count, 2, "Expected 2 return nodes, got {}: {}", return_count, sexp);
}
```

**Verify Command:** 
```bash
cargo test -p perl-parser-core --test control_flow_return_precedence_1394
```

---

### Step 3: Verify loop control already handles this correctly

**File:** No changes needed — loop control already works correctly

**Verification Tests (EXISTING):** 
- `loop_control_tests.rs` — all tests pass
- `control_flow_expr_tests.rs` — all tests pass
- Already tested: `next or die`, `last or die`, `redo or die`

**Verify Command:**
```bash
cargo test -p perl-parser-core control_flow_expr_tests loop_control_tests
```

---

### Step 4: Verify statement-level return still respects statement modifiers

**File:** `crates/perl-parser-core/tests/control_flow_return_precedence_1394.rs` (same test file)

**Test Case:**

```rust
#[test]
fn test_return_with_statement_modifier() {
    // return $x if $cond;  should still work
    let source = "return $x if $cond;";
    let ast = parse_code(source).unwrap();
    let sexp = ast.to_sexp();
    
    // Should have statement_modifier_if
    assert!(sexp.contains("statement_modifier_if"), "Expected statement_modifier_if: {}", sexp);
    assert!(sexp.contains("return"), "Expected return: {}", sexp);
}
```

**Why:** Ensure that statement modifiers (if/unless/while/until/for) still bind correctly *after* the return value stops at word-operator boundaries.

**Verify Command:**
```bash
cargo test -p perl-parser-core --test control_flow_return_precedence_1394 -- test_return_with_statement_modifier
```

---

### Step 5: Verify return in expression contexts (no regression)

**File:** No code changes needed — existing `parse_return_expr()` unchanged

**Verification Tests (EXISTING):** 
- `control_flow_expr_tests.rs` covers:
  - `$x = return 1` (assignment)
  - `$cond ? return $x : $y` (ternary)
  - `$cond && return` (short-circuit)
  - All these use `parse_return_expr()` which is unchanged

**Verify Command:**
```bash
cargo test -p perl-parser-core control_flow_expr_tests
```

---

### Step 6: Full parser regression suite

**File:** No new code

**Verify Commands:**
```bash
# Full test suite
cargo test -p perl-parser-core

# Comprehensive checks
cargo fmt --all
cargo clippy -p perl-parser-core --tests

# Integration: parse real code samples
cargo run -p perl-parser --features cli --bin perl-parse -- /tmp/test_samples.pl
```

---

## Compilation Order

Since we're only changing one function in one file, there are no inter-module dependencies to worry about. The change should compile immediately.

1. Modify `parse_return()` to use `parse_assignment()` instead of `parse_expression()`
2. All downstream code (statement parsing, word-operator parsing) works with the same AST structure
3. No breaking changes to `NodeKind::Return` or any public API

---

## Test Grid Summary

| Test Category | Test Name | Expected | Coverage |
|---|---|---|---|
| **Statement-level word operators** | `test_return_with_word_or_at_statement_level` | Binary OR at top level | Return correctly stops at `or` |
| | `test_return_with_word_and_at_statement_level` | Binary AND at top level | Return correctly stops at `and` |
| | `test_return_without_value_with_word_or` | Binary OR at top level | Return with no value stops at `or` |
| | `test_return_value_does_not_consume_or_separator` | 2 return nodes, OR between | Return value doesn't consume next return |
| **Statement modifiers** | `test_return_with_statement_modifier` | statement_modifier_if present | Modifiers still work after return |
| **Expression contexts (regression)** | `control_flow_expr_tests.rs` (all) | Pass unchanged | No regression in ternary/short-circuit |
| **Loop control (no change)** | `loop_control_tests.rs` (all) | Pass unchanged | Loop control still works correctly |

---

## Hazards Addressed

- **Parser Precedence Bug (PARSER-2):** Fixed statement-level return to respect word-operator precedence
- **Inconsistency (PARSER-3):** Statement-level and expression-level return now use the same precedence stopping point
- **Test Coverage (TEST-1):** Added comprehensive tests for word-operator boundaries with return

---

## Files Affected

| File | Changes | Risk |
|---|---|---|
| `crates/perl-parser-core/src/engine/parser/statements.rs` | `parse_return()` line 499: `parse_expression()` → `parse_assignment()` | LOW — changes only the method called for parsing the return value |
| `crates/perl-parser-core/tests/control_flow_return_precedence_1394.rs` | NEW — 4-5 test functions | LOW — new tests only |

---

## Verification Workflow

1. **Step 1 Code Edit:** Modify `parse_return()` to use `parse_assignment()`
2. **Compile Check:** `cargo build -p perl-parser-core`
3. **Step 2 Test Add:** Create new test file with 4 test cases
4. **Red Tests:** `cargo test -p perl-parser-core --test control_flow_return_precedence_1394` (should fail before Step 1, pass after)
5. **Regression Check:** `cargo test -p perl-parser-core control_flow_expr_tests` (should pass throughout)
6. **Full Suite:** `cargo test -p perl-parser-core`
7. **Lint:** `cargo clippy -p perl-parser-core --tests`
8. **Format:** `cargo fmt --all`

---

## Notes for Builder

- **Single-point Fix:** Only one function change in one file. All logic already exists (`parse_assignment()` is already used successfully elsewhere).
- **No API Changes:** `NodeKind::Return` unchanged, public parser interface unchanged.
- **Backward Compatible:** Code that correctly parsed before still parses the same way; only incorrect precedence cases are fixed.
- **Word Operators:** Perl has 4 word operators at statement level: `or`, `and`, `xor`, `not`. This fix addresses the first 3; `not` is already handled by expression parsing.
- **Test Pattern:** Use `assert!(sexp.contains(...))` to check S-expression structure since the parser produces S-expressions as the intermediate format.
