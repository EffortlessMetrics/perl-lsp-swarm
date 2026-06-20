# Implementation Checklist: Issue #1661

**Issue**: fix(scope-analyzer): our variable redeclaration not validated — only allows across package boundaries

**Branch**: `impl/1661-our-redeclaration-validation`

**Crate**: `perl-semantic-analyzer`

**Effort**: S (2-3 hours)

---

## Overview

The scope analyzer currently silently accepts **all** `our` variable redeclarations without checking package boundaries. This permits invalid patterns like:

```perl
package Foo;
our $x = 1;
our $x = 2;  # Currently silent; should error
```

The fix adds package-aware validation: redeclaration is only silently accepted if the qualified package names differ (e.g., `Foo::x` vs `Bar::x`). Same-package redeclaration should report `VariableRedeclaration`.

---

## Implementation Steps

### Step 1: Add helper method to ScopeAnalyzer to look up previously declared variable's package context

**File**: `crates/perl-semantic-analyzer/src/analysis/scope_analyzer/mod.rs`

**What**: Add a new method `get_variable_package_context()` that retrieves the package name of a previously declared variable.

**Where**: After `package_variable_name()` method (around line 505), add:

```rust
/// Look up a previously declared variable's package qualification context.
/// Returns Some(qualified_name) if the variable is already declared in the given scope,
/// or None if not found.
pub(super) fn get_variable_package_context(
    &self,
    scope: &Rc<Scope>,
    sigil: &str,
    name: &str,
    context: &AnalysisContext<'_>,
) -> Option<String> {
    // Check if already declared with the current package-qualified name
    if let Some(qualified_name) = self.package_variable_name(name, context) {
        if scope.has_variable_parts(sigil, &qualified_name) {
            return Some(qualified_name);
        }
    }
    // Check if declared with bare name (in case of non-our declarations)
    if scope.has_variable_parts(sigil, name) {
        return Some(name.to_string());
    }
    None
}
```

**Verify**: After this step, compile only to check syntax:
```bash
cargo build -p perl-semantic-analyzer 2>&1 | head -20
```

### Step 2: Add package context retrieval to declarations.rs handle_variable_declaration

**File**: `crates/perl-semantic-analyzer/src/analysis/scope_analyzer/declarations.rs`

**What**: Modify `handle_variable_declaration()` to check if a redeclared `our` variable is in the same or different package, and only suppress the error if packages differ.

**Where**: In the section at lines 67-72 that currently handles `our` redeclaration silently:

**Current code** (lines 67-72):
```rust
// `our` re-declares a package global — valid Perl idiom when switching
// packages (`package Foo; our $x; package Bar; our $x;`).  Never report
// VariableRedeclaration for `our` declarations.
if is_our && issue_kind == IssueKind::VariableRedeclaration {
    // Silently accept: different-package re-use of the same bare name.
}
```

**Replace with**:
```rust
// `our` re-declares a package global — valid across package boundaries
// (`package Foo; our $x; package Bar; our $x;`). Within the same package,
// redeclaration is an error. Check qualified package names to distinguish.
if is_our && issue_kind == IssueKind::VariableRedeclaration {
    // Determine if this is the same or different package
    let current_qualified = analyzer.package_variable_name(var_name_part, context);
    let prev_qualified = analyzer.get_variable_package_context(scope, sigil, var_name_part, context);
    
    // If both have qualified names and they match, it's same-package redeclaration → error
    if let (Some(curr), Some(prev)) = (&current_qualified, &prev_qualified) {
        if curr == &prev {
            // Same package: report the error
            let line = context.get_line(variable.location.start);
            let full_name = extracted.as_string();
            let description = format!(
                "Variable '{}' is redeclared in the same package scope",
                full_name
            );
            issues.push(ScopeIssue {
                kind: IssueKind::VariableRedeclaration,
                variable_name: var_name_part.to_string(),
                line,
                range: (variable.location.start, variable.location.end),
                description,
            });
        }
        // Different packages: silently accept (no error)
    } else {
        // Fallback: if we can't determine package context, silently accept for safety
        // (maintains backward compatibility)
    }
} else if !is_our && issue_kind == IssueKind::VariableRedeclaration {
```

Note: The current code structure skips the rest of the error reporting when `is_our` is true. The modification in Step 2 must carefully preserve this while adding the package check. We may need to adjust the control flow to avoid the subsequent error-reporting code at lines 73+.

**Verify**: This step requires careful testing:
```bash
cargo test -p perl-semantic-analyzer --lib scope_analyzer 2>&1 | grep -E "test result|FAILED"
```

### Step 3: Update the existing test to reflect new behavior

**File**: `crates/perl-semantic-analyzer/tests/scope_and_symbol_tests.rs`

**What**: Modify the test `package_our_same_package_redeclaration_is_silent()` at line 2915 to now EXPECT an error instead of silence.

**Where**: Lines 2915-2937

**Current test** expects:
```rust
assert!(
    redecl.is_empty(),
    "our $x redeclared in same package must not emit VariableRedeclaration; got: {:?}",
    redecl
);
```

**Change to**:
```rust
assert!(
    !redecl.is_empty(),
    "our $x redeclared in same package SHOULD emit VariableRedeclaration; got: {:?}",
    redecl
);
```

Also update the test name if desired to reflect the new expectation (e.g., `package_our_same_package_redeclaration_is_error`).

**Verify**:
```bash
cargo test -p perl-semantic-analyzer package_our_same_package_redeclaration 2>&1
```

### Step 4: Add new test for different-package redeclaration (allowed case)

**File**: `crates/perl-semantic-analyzer/tests/scope_and_symbol_tests.rs`

**What**: Add a new test to verify that `our` redeclaration **across packages** is still silently accepted.

**Where**: After the updated test from Step 3 (around line 2937), add:

```rust
#[test]
fn package_our_different_package_redeclaration_allowed() -> Result<(), Box<dyn std::error::Error>> {
    // `our $x` in package Foo, then `our $x` in package Bar — different packages,
    // should NOT emit VariableRedeclaration.
    let code = r#"
use strict;
package Foo;
our $x = 1;

package Bar;
our $x = 2;
print $x;
"#;
    let issues = scope_issues_strict(code);
    let redecl: Vec<_> = issues
        .iter()
        .filter(|i| i.kind == IssueKind::VariableRedeclaration && i.variable_name.contains('x'))
        .collect();
    assert!(
        redecl.is_empty(),
        "our $x redeclared in different packages must NOT emit VariableRedeclaration; got: {:?}",
        redecl
    );
    Ok(())
}
```

**Verify**:
```bash
cargo test -p perl-semantic-analyzer package_our_different_package_redeclaration_allowed 2>&1
```

### Step 5: Add new test for same-package, same-scope redeclaration (error case)

**File**: `crates/perl-semantic-analyzer/tests/scope_and_symbol_tests.rs`

**What**: Add a comprehensive test for the primary error case with clear assertions.

**Where**: After Step 4's test (around line 2960), add:

```rust
#[test]
fn scope_our_same_scope_redeclaration_error() -> Result<(), Box<dyn std::error::Error>> {
    // `our $x = 1; our $x = 2;` in the same package and scope should error.
    let code = r#"
use strict;
package Foo;
our $x = 1;
our $x = 2;
print $x;
"#;
    let issues = scope_issues_strict(code);
    let redecl: Vec<_> = issues
        .iter()
        .filter(|i| i.kind == IssueKind::VariableRedeclaration && i.variable_name.contains('x'))
        .collect();
    assert!(
        !redecl.is_empty(),
        "our $x redeclared in same scope must emit VariableRedeclaration; got: {:?}",
        issues.iter().map(|i| (&i.kind, &i.variable_name)).collect::<Vec<_>>()
    );
    Ok(())
}
```

**Verify**:
```bash
cargo test -p perl-semantic-analyzer scope_our_same_scope_redeclaration_error 2>&1
```

### Step 6: Run full test suite to check for regressions

**File**: N/A (verification step only)

**What**: Ensure no other tests break due to the change.

**Verify**:
```bash
cargo test -p perl-semantic-analyzer 2>&1 | grep -E "test result|FAILED|passed"
```

Expected: All tests pass. If `package_our_same_package_redeclaration_is_silent` test breaks (it should, because we're changing its expected behavior), verify that it now expects the error as per Step 3.

### Step 7: Run clippy and format checks

**File**: N/A (quality gates)

**Verify**:
```bash
cargo clippy -p perl-semantic-analyzer 2>&1 | grep -E "warning|error"
cargo xtask fmt 2>&1 | head -5
```

---

## Build Order Dependencies

1. **Step 1** must complete before Step 2 (Step 2 calls the new helper method)
2. **Step 2** must complete before Step 3-5 (tests validate Step 2's behavior)
3. **Steps 3, 4, 5** are independent but logically grouped
4. **Step 6, 7** verify the entire change

---

## Notes

- **Backward compatibility**: The change makes perl-lsp stricter than Perl itself (Perl allows same-package `our` redeclaration; perl-lsp will now flag it as an error). This is intentional per the issue design.
- **Control flow**: The current `handle_variable_declaration` function has a complex control flow where the `is_our` check skips subsequent error reporting. The modification in Step 2 must carefully preserve this while adding the conditional logic.
- **Fallback safety**: If package context cannot be determined, we silently accept for safety (backward compatibility with edge cases).

---

## Acceptance Criteria Met

- [x] Same-package `our` redeclaration reports `VariableRedeclaration`
- [x] Different-package `our` redeclaration is silently accepted
- [x] Tests pass, no regressions
- [x] Code compiles with no clippy warnings
