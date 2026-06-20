# Implementation Checklist: Issue #1849 — Qualified-Name Cursor Position Resolution

## Change Summary

Fix go-to-definition and find-references to correctly resolve cursor position on any component of a qualified name (e.g., `My::Utils::process`), not just the final component. Requires two parallel fixes (navigation.rs + references.rs) and new regression tests.

---

## Step 1: Add Helper Function for Component Lookup (navigation.rs)

**File**: `crates/perl-lsp-rs/src/runtime/language/navigation.rs`

**What**: Add a new helper function `find_component_at_cursor()` that calculates which `::` -separated component contains the cursor position.

**Signature**:
```rust
/// Given a fully-qualified name string and cursor position within that string,
/// determine which `::` -separated component the cursor is in.
/// Returns Some((component_index, component_name)) where component_index is 0-based.
/// Returns None if cursor is exactly on a `::` delimiter or out of bounds.
fn find_component_at_cursor(fqn: &str, cursor_offset: usize) -> Option<(usize, &str)> {
    // Implementation: partition string by `::`, track offsets
}
```

**Dependencies**: None; utility function.

**Verify**: `cargo build -p perl-lsp-rs` (must compile)

---

## Step 2: Update Go-to-Definition Qualified-Name Resolution (navigation.rs)

**File**: `crates/perl-lsp-rs/src/runtime/language/navigation.rs`

**Where**: Lines 1166–1187 (the `// Attempt to resolve fully-qualified symbols` block)

**Current code** (lines 1169–1172):
```rust
let parts: Vec<&str> = m.as_str().split("::").collect();
if parts.len() >= 2 {
    let name = parts.last().copied().unwrap_or("");
    let pkg = parts[..parts.len() - 1].join("::");
```

**New code**:
```rust
let fqn_str = m.as_str();
let parts: Vec<&str> = fqn_str.split("::").collect();
if parts.len() >= 2 {
    // Calculate which component the cursor falls into
    if let Some((component_idx, component_name)) = find_component_at_cursor(fqn_str, cursor_in_text - m.start()) {
        // If cursor is on final component, use existing lookup logic
        if component_idx == parts.len() - 1 {
            let name = component_name;
            let pkg = parts[..parts.len() - 1].join("::");
            if let Some(result) = lookup_workspace_definition(
                self.coordinator(),
                &pkg,
                name,
                Some(uri),
            ) {
                return Ok(Some(result));
            }
        }
        // If cursor is on earlier component, return None (module lookup not yet supported)
        // Fall through to same-file resolution
    }
    // Partial/None: fall through to same-file resolution
}
```

**Dependencies**: Step 1 must be complete (helper function added).

**Verify**: `cargo build -p perl-lsp-rs` (must compile)

---

## Step 3: Update Find-References Qualified-Name Resolution (references.rs)

**File**: `crates/perl-lsp-rs/src/runtime/language/references.rs`

**Where**: Lines 398–429 (the regex-based fallback for qualified symbols)

**Current code** (lines 403–411):
```rust
let parts: Vec<&str> = m.as_str().split("::").collect();
if parts.len() >= 2 {
    let name = parts.last().copied().unwrap_or("").to_string();
    let pkg = parts[..parts.len() - 1].join("::");
```

**New code**:
```rust
let fqn_str = m.as_str();
let parts: Vec<&str> = fqn_str.split("::").collect();
if parts.len() >= 2 {
    // Calculate which component the cursor falls into
    if let Some((component_idx, component_name)) = crate::runtime::language::navigation::find_component_at_cursor(fqn_str, cursor_in_text - m.start()) {
        // If cursor is on final component, use existing lookup logic
        if component_idx == parts.len() - 1 {
            let name = component_name.to_string();
            let pkg = parts[..parts.len() - 1].join("::");
            // [existing reference lookup code continues]
        }
        // If cursor is on earlier component, skip this match
        // Fall through to other resolution strategies
    }
}
```

**Dependencies**: Step 1 must be complete (helper function must be public/exported).

**Note**: The helper function in Step 1 must be made `pub` in navigation.rs so it can be called from references.rs.

**Verify**: `cargo build -p perl-lsp-rs` (must compile)

---

## Step 4: Add Regression Tests (navigation_regression_tests.rs)

**File**: `crates/perl-lsp-rs/tests/navigation_regression_tests.rs`

**Where**: After the existing `test_def_package_qualified_call()` test (around line 382)

**What**: Add new test `test_def_package_qualified_call_cursor_positions()` that covers all three cursor positions:

```rust
#[test]
fn test_def_package_qualified_call_cursor_positions() -> TestResult {
    let doc = concat!(
        "package My::Utils;\n",           // 0
        "sub process { return 'done'; }\n", // 1
        "\n",                             // 2
        "package main;\n",                // 3
        "My::Utils::process();\n",        // 4
    );

    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open_document("file:///qual_cursor.pl", doc)?;

    // Test 1: Cursor on "My" (first component) — should NOT resolve to process()
    let result = harness.request(
        "textDocument/definition",
        json!({
            "textDocument": {"uri": "file:///qual_cursor.pl"},
            "position": {"line": 4, "character": 0}  // 'M' in My
        }),
    ).unwrap_or(json!(null));
    assert!(
        result == json!(null),
        "Cursor on package prefix 'My' must NOT resolve to process() definition, got: {result}"
    );

    // Test 2: Cursor on "Utils" (middle component) — should NOT resolve to process()
    let result = harness.request(
        "textDocument/definition",
        json!({
            "textDocument": {"uri": "file:///qual_cursor.pl"},
            "position": {"line": 4, "character": 3}  // 'U' in Utils
        }),
    ).unwrap_or(json!(null));
    assert!(
        result == json!(null),
        "Cursor on package prefix 'Utils' must NOT resolve to process() definition, got: {result}"
    );

    // Test 3: Cursor on "process" (final component) — MUST resolve to sub definition on line 1
    let result = harness.request(
        "textDocument/definition",
        json!({
            "textDocument": {"uri": "file:///qual_cursor.pl"},
            "position": {"line": 4, "character": 11}  // 'p' in process
        }),
    ).unwrap_or(json!(null));
    if let Some(def_line) = first_location_line(&result) {
        assert_eq!(
            def_line, 1,
            "Cursor on function 'process' must resolve to 'sub process' on line 1, got line {def_line}"
        );
    }

    Ok(())
}
```

**Verify**: `cargo test -p perl-lsp-rs test_def_package_qualified_call_cursor_positions -- --test-threads=1` (must pass)

---

## Step 5: Verify All Tests Pass

**Command**: `RUST_TEST_THREADS=2 cargo test -p perl-lsp-rs -- --test-threads=2`

**Expected**: All navigation and regression tests pass, including the new `test_def_package_qualified_call_cursor_positions()`.

---

## Step 6: Code Quality Checks

**Commands**:
```bash
cargo fmt --all
cargo clippy -p perl-lsp-rs --tests
```

**Expected**: No new warnings or formatting issues.

---

## Step 7: Commit and Push

**Branch**: `impl/1849-qualified-cursor-resolution` (created from origin/main)

**Commit message**:
```
fix(navigation): resolve cursor position correctly on qualified name components (#1849)

When a cursor is positioned on a package prefix in a fully-qualified name
like `My::Utils::process()`, go-to-definition and find-references must
determine which component (package or function) the cursor is actually
on, not unconditionally resolve to the final component.

Changes:
- Add find_component_at_cursor() helper to calculate per-component offsets
- Update go-to-definition qualified-name resolution to branch on component index
- Update find-references qualified-name resolution to branch on component index
- Add regression tests for all three cursor positions

Fixes #1849
```

**Verify**: `git status` (clean), `git log --oneline -3` (commit is visible)

---

## Implementation Notes

### Helper Function Design

The `find_component_at_cursor()` function must:
1. Split the FQN on `::` to get component parts
2. Iterate through parts, tracking cumulative byte offset
3. For each part, check if `cursor_offset` falls within `[start, end]`
4. Return `Some((index, part))` if found, else `None`

Example implementation sketch:
```rust
fn find_component_at_cursor(fqn: &str, cursor_offset: usize) -> Option<(usize, &str)> {
    let mut offset = 0;
    for (idx, part) in fqn.split("::").enumerate() {
        if cursor_offset >= offset && cursor_offset < offset + part.len() {
            return Some((idx, part));
        }
        offset += part.len() + 2; // +2 for "::"
    }
    None
}
```

### Cursor Offset Calculation

In both navigation.rs and references.rs, the cursor offset relative to the match is:
```rust
let cursor_offset_in_match = cursor_in_text - m.start();
```

This offset is then passed to `find_component_at_cursor()`.

### Test Character Positions

For the test string `My::Utils::process()` on line 4:
- `character: 0` → `M` (component 0, `My`)
- `character: 3` → `U` (component 1, `Utils`)
- `character: 11` → `p` (component 2, `process`)

Verify these by counting bytes in the test string.

---

## Summary

| Step | File | Change Type | Compile-check | Test-check |
|------|------|-------------|---|---|
| 1 | navigation.rs | Add helper fn | ✓ | N/A |
| 2 | navigation.rs | Replace lines 1169–1172 | ✓ | ✓ |
| 3 | references.rs | Replace lines 403–411 | ✓ | ✓ |
| 4 | navigation_regression_tests.rs | Add new test | ✓ | ✓ |
| 5 | All | Run full test suite | N/A | ✓ |
| 6 | All | Fmt + Clippy | ✓ | N/A |
| 7 | Git | Commit + Push | N/A | N/A |

All steps compile independently and collectively.
