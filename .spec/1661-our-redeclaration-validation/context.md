# Context: Issue #1661

**Title**: fix(scope-analyzer): our variable redeclaration not validated — only allows across package boundaries

**Issue**: #1661

**Date**: 2026-06-20

---

## Problem Statement

The scope analyzer in perl-semantic-analyzer silently accepts **all** `our` variable redeclarations without checking if they cross package boundaries. This is incorrect behavior because:

1. **Perl idiom**: `our $x` is valid to declare across package boundaries (e.g., `package Foo; our $x; package Bar; our $x;`). This reuses the same bare name in different package-qualified namespaces.

2. **Same-package error**: Redeclaring the same `our` variable **within the same package scope** should be flagged as an error, not silently accepted.

**Current code** (lines 67-72 of `declarations.rs`):
```rust
// `our` re-declares a package global — valid Perl idiom when switching
// packages (`package Foo; our $x; package Bar; our $x;`).  Never report
// VariableRedeclaration for `our` declarations.
if is_our && issue_kind == IssueKind::VariableRedeclaration {
    // Silently accept: different-package re-use of the same bare name.
}
```

The comment documents the intent (different packages only), but the code does not enforce it.

---

## Key Decisions

### Decision 1: Perl's Actual Behavior vs. Linting Intent

**Fact discovered**: Perl itself **allows** same-package `our` redeclaration without error:
```perl
use strict;
package Foo;
our $x = 1;
our $x = 2;  # Perl: OK (silently ignored; re-imports same global)
```

**Issue intent**: perl-lsp should be **stricter** than Perl for linting purposes (catch redundant/likely-mistake redeclarations).

**Decision**: Implement per issue design: same-package redeclaration reports error. This makes perl-lsp a linter, not a Perl-compatible validator.

**Rationale**: Linters are intentionally stricter than interpreters to catch common mistakes. This is similar to how eslint is stricter than JavaScript engines.

### Decision 2: Package Context Tracking

**Available mechanism**: The code already uses `package_variable_name()` to generate qualified names (`Foo::x` vs `Bar::x`). This method reads the current package from `AnalysisContext.current_package`.

**Decision**: Use qualified-name comparison to distinguish same-package from different-package redeclaration. If previous declaration's qualified name matches current qualified name, it's same-package; else different-package.

**Rationale**: Avoids state-machine complexity; leverages existing infrastructure.

### Decision 3: When to Report the Error

**Question**: Should we report error at first redeclaration, second, or both?

**Answer**: Report at second and subsequent. Once a variable is declared, any redeclaration in the same scope is an error. This aligns with existing `VariableRedeclaration` behavior for `my`.

---

## Alternatives Considered

### Alt 1: Track Package in Variable struct (REJECTED)

**Idea**: Add `package: String` field to `Variable` struct to store the package name at declaration time.

**Pros**:
- More explicit; easier to trace
- Cleaner logic

**Cons**:
- Overlaps with #1654 variable metadata work (state variable semantic tracking)
- Sequencing conflict: #1654 may need to land first or merge this work
- Introduces struct schema change mid-flight

**Decision**: REJECTED in favor of using existing qualified-name infrastructure.

### Alt 2: Leave as-is (status quo) (REJECTED)

**Idea**: No code change; silently accept all `our` redeclarations (current behavior).

**Pros**:
- Zero implementation cost
- Matches Perl's behavior exactly

**Cons**:
- Misses real errors: redundant/likely-mistake redeclarations
- Violates issue design intent: stricter linting

**Decision**: REJECTED; issue explicitly requests validation.

### Alt 3: Always report our redeclaration, no special case (REJECTED)

**Idea**: Remove the `is_our` check entirely; report `VariableRedeclaration` for any `our` redeclaration regardless of package.

**Pros**:
- Simplest code change (delete 5 lines)

**Cons**:
- Breaks valid Perl idiom: declaring same variable in multiple packages
- Likely high false-positive rate in real code

**Decision**: REJECTED; must preserve cross-package idiom.

---

## Related Issues

| Issue | Relationship | Notes |
|-------|--------------|-------|
| #1654 | Parent concern: variable declaration semantics | Broader work on `state` variable initialization-once semantics. May eventually consolidate variable metadata tracking. #1661 is independent; sequencing is fine. |
| #1659 | Related: state variable scope binding | Different variable kind (`state`), different issue. No dependency. |
| #1664 | Related: variable declaration validation | Mentioned in issue comments as part of broader variable validation work. No direct code dependency. |

---

## Perl Semantic Verification

**Research-verifier finding**: Perl's perldoc does not explicitly state whether same-scope `our` redeclaration is an error or allowed.

**Empirical verification** (2026-06-20):
```bash
$ perl -e 'use strict; package Foo; our $x = 1; our $x = 2; print "OK\n";'
OK
```

**Conclusion**: Perl **does allow** same-scope, same-package `our` redeclaration. It is silently a no-op (re-imports the same package global).

**perl-lsp design intent**: Stricter than Perl to catch likely mistakes. This is a valid linting stance and is documented in the issue.

---

## Implementation Approach

### High-level strategy

1. Add `get_variable_package_context()` helper to retrieve previous declaration's package context.
2. When `VariableRedeclaration` is detected for `our`, compare qualified names.
3. If same package, report error. If different package, silently accept.
4. Update test expectations to match new behavior.

### Why this approach

- **Minimal scope**: Only changes redeclaration handling; leaves other scope validation untouched.
- **Leverages existing code**: Uses `package_variable_name()` and `has_variable_parts()` already in codebase.
- **Preserves backwards compat**: Fallback to silence if package context can't be determined (safety valve).
- **Testable**: Clear test matrix with positive, negative, boundary, and state-transition cases.

---

## Testing Strategy

### Test Coverage

| Test Category | Count | Purpose |
|---|---|---|
| Same-package redeclaration error | 2 | Initialized and uninitialized `our` in same package |
| Different-package redeclaration allowed | 1 | `our` redeclared across `package` statements |
| Nested scope handling | 2 | Separate blocks in same package vs. same block with redeclaration |
| Mixed variable kinds | 1 | `our` then `my` (shadowing, not redeclaration) |
| Package switching | 1 | Multiple package changes; each package maintains separate variables |
| Regression: `my` redeclaration unchanged | 1 | Ensure `my` behavior is unaffected |

Total: **8 test cases** covering positive, negative, boundary, and state-transition scenarios.

### Test Execution

Red-TDD builder will:
1. Add failing tests to establish expected behavior
2. Verify tests fail on current main (baseline)
3. Commit red tests to impl branch
4. Builder implements fix to make tests pass
5. Green-TDD builder adds edge case and regression tests

---

## Notes for Red-TDD Builder

- The existing test `package_our_same_package_redeclaration_is_silent()` at line 2915 documents the OLD (incorrect) behavior. This test should be inverted: expect error instead of silence.
- Add at least one test for the "different packages" case to ensure we don't break the valid idiom.
- Consider testing with and without `use strict;` to verify package context is tracked correctly in both modes.
- Package context is maintained in `AnalysisContext.current_package`; verify this is correctly updated when `package` statements are encountered.

---

## Notes for Builder

- The modification in `declarations.rs` must preserve the control flow that skips subsequent error-reporting code. The current structure has an `if is_our { ... } else { ... }` pattern. The new logic should emit the error inside the `if is_our` branch, then skip the subsequent code.
- Method `get_variable_package_context()` will be called during the `VariableRedeclaration` detection phase, which is the right time (after `declare_variable_parts_in_context()` returns the issue kind).
- The scope passed to the lookup should be the **current scope** (where redeclaration was detected), not a parent scope.
- Fallback to silent acceptance if package context cannot be determined (for robustness with edge cases).

---

## Verification Checklist for Red-TDD

- [ ] Tests compile without warnings
- [ ] `cargo test -p perl-semantic-analyzer scope_our_same_scope_redeclaration_error` passes (once implemented)
- [ ] `cargo test -p perl-semantic-analyzer package_our_different_package_redeclaration_allowed` passes (once implemented)
- [ ] `cargo test -p perl-semantic-analyzer scope_my_redeclaration_same_scope_error` passes (unchanged behavior for `my`)
- [ ] Full `perl-semantic-analyzer` test suite passes with no regressions
- [ ] No clippy warnings introduced

---

## Links and References

| Document | Purpose |
|----------|---------|
| [PARSER_CONTRACTS.md](../../docs/reference/PARSER_CONTRACTS.md) | Parser-layer semantic contracts (not affected by this change) |
| [docs/concepts/](../../docs/concepts/) | Portable patterns and design decisions |
| [Issue #1661](https://github.com/Perl-Critic/perl-lsp/issues/1661) | Original issue with scout analysis and research verification |
| [Issue #1654](https://github.com/Perl-Critic/perl-lsp/issues/1654) | Related: state variable semantics |
| perldoc [perlmod](https://perldoc.perl.org/perlmod) | Perl module documentation (confirms `our` creates package-qualified variables) |
| perldoc [strict](https://perldoc.perl.org/strict) | Perl strict pragma documentation |
