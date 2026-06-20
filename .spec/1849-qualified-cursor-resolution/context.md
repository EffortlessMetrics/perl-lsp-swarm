# Context: Issue #1849 — Navigation: Fix Qualified-Name Cursor Position Resolution

## Problem Statement

Go-to-definition and find-references features currently resolve incorrectly when the cursor is positioned on a package prefix in a fully-qualified name (e.g., `My::Utils::process()`). The code unconditionally uses the final component regardless of where the cursor is actually positioned within the qualified name.

**Example:**
- Cursor on `My` → should navigate to module `My` or return null, but currently navigates to `process()`
- Cursor on `Utils` → should navigate to `My::Utils` or return null, but currently navigates to `process()`
- Cursor on `process` → correctly navigates to the function definition

## Root Cause

After regex-matching a fully-qualified symbol like `Package::Sub` against the line text, the code correctly identifies the match span `[m.start(), m.end()]`. However, it fails to partition that span into per-component boundaries:
- Bytes 0–6 → `Package`
- Bytes 7–8 → `::`
- Bytes 9–11 → `Sub`

Without this partitioning, the code cannot determine which component the cursor falls into and unconditionally defaults to the final component via `parts.last()`.

## Affected Locations

1. **crates/perl-lsp-rs/src/runtime/language/navigation.rs:1169–1172**
   - Function: `goto_definition_workspace` (within the definition resolution handler)
   - Issue: Always uses `parts.last()` when processing qualified names

2. **crates/perl-lsp-rs/src/runtime/language/references.rs:403–411**
   - Function: In the find-references fallback handler
   - Issue: Identical logic — always uses `parts.last()`

3. **crates/perl-lsp-rs/tests/navigation_regression_tests.rs:345–368**
   - Test: `test_def_package_qualified_call()`
   - Issue: Only tests cursor position on the final component (`character: 9` on `bar`)
   - Missing: Tests for cursor positions on package prefixes

## Design Approach

The fix requires calculating which `::` -separated component contains the cursor position and branching accordingly:

1. **Component offset calculation**: After splitting on `::`, track the byte offset of each component within the match span
2. **Cursor component lookup**: Determine which component index the cursor falls into
3. **Component-aware branching**:
   - Final component (e.g., `process`) → function/method lookup (existing behavior)
   - Earlier components (e.g., `My`, `Utils`) → either module lookup or return null
   - Exactly on `::` delimiter → return null

4. **Test coverage**: Add regression tests for all three cursor positions to prevent future regressions

## Alternatives Considered

1. **Split and search for nearest `::` to cursor** — simple but less robust to varying whitespace or macro-generated code
2. **Use tree-sitter to find the precise node** — more robust but requires parser interaction; the regex approach is already in place
3. **Return module definitions for earlier components** — possible future enhancement, but for now returning null is safer

**Decision**: Use per-component offset calculation. This is surgical, maintains the existing regex-based approach, and requires no parser integration.

## Test Strategy

Create a new test `test_def_package_qualified_call_cursor_positions` that systematically tests all three cursor positions:

```perl
package My::Utils;
sub process { return 'done'; }

package main;
My::Utils::process();
```

For the call on line 4, test three cursor positions:
- `character: 0` (on `My`) — expect null or module definition
- `character: 3` (on `Utils`) — expect null or `My::Utils` module definition
- `character: 12` (on `process`) — expect function definition (existing behavior)

## Links

- **Issue**: #1849
- **Crate**: perl-lsp-rs
- **Features**: Navigation (go-to-definition, find-references)
- **Protocol**: LSP textDocument/definition, textDocument/references
- **Related**: Parser symbol tracking, workspace index queries
