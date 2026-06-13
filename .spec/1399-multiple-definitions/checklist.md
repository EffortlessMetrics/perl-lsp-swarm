# Implementation Checklist: Support Multiple Definitions for the Same Symbol

## Overview

This checklist guides the builder through promoting the internal `definition_candidates()` API to public and adding support for returning multiple definitions in LSP goto-definition responses. The change is scoped to:

1. **WorkspaceIndex public API** (add 2 new methods, update 2 existing)
2. **LSP navigation handlers** (update 2-3 call sites to handle Vec<Location>)
3. **Tests** (4-5 new tests covering multiple definition scenarios)

All changes are localized to:
- `crates/perl-workspace/src/workspace/workspace_index.rs` (public API changes)
- `crates/perl-lsp-rs/src/runtime/language/navigation.rs` (LSP handler updates)
- `crates/perl-workspace/tests/definition_ambiguity_regression_tests.rs` (new/updated tests)

Compilation order is linear; each step should compile independently.

---

## Step 1: Add `find_definitions()` Public Method (Workspace Index)

**File:** `crates/perl-workspace/src/workspace/workspace_index.rs`

**What changes:**
- Add new public method `pub fn find_definitions(&self, symbol_name: &str) -> Vec<Location>`
- This is the public promotion of the internal `definition_candidates()` method
- Signature should mirror `definition_candidates()` but with public visibility

**Where:**
- After line 2367 (after the existing `definition_candidates()`)
- Keep `definition_candidates()` as `pub(crate)` for backward compatibility with internal callers

**Implementation:**
```rust
/// Find all definitions for a symbol name (bare or fully-qualified).
///
/// Returns a vector of all known locations where this symbol is defined.
/// For a bare name like "foo", returns all packages where "foo" is defined.
/// For a qualified name like "Package::foo", returns the location for that specific symbol.
///
/// The order of results is determined by the insertion order into the internal index
/// and may vary across workspace reloads. Use sorted_definitions() if deterministic
/// ordering is required.
///
/// Returns an empty vector if the symbol is not found.
///
/// # Examples
///
/// ```
/// let index = WorkspaceIndex::new();
/// // ... index some files ...
/// let defs = index.find_definitions("Package::foo");
/// for location in defs {
///     println!("Found at: {}", location.uri);
/// }
/// ```
pub fn find_definitions(&self, symbol_name: &str) -> Vec<Location> {
    self.definition_candidates(symbol_name)
}
```

**Verify:**
```bash
cd /path/to/workspace
cargo build -p perl-workspace --lib
cargo test -p perl-workspace --lib definition
```

---

## Step 2: Add `find_defs()` Public Method for SymbolKey (Workspace Index)

**File:** `crates/perl-workspace/src/workspace/workspace_index.rs`

**What changes:**
- Add new public method `pub fn find_defs(&self, key: &SymbolKey) -> Vec<Location>`
- This is the structured-key parallel to `find_definitions()` (bare/qualified name lookup)
- Mirrors `find_def()` but returns Vec instead of Option

**Where:**
- After the existing `find_def()` method (around line 3473)
- Maintain symmetry: `find_def()` returns Option, `find_defs()` returns Vec

**Implementation:**
```rust
/// Find all definitions for a structured symbol key.
///
/// This is the batch version of `find_def()`. Returns all candidate definitions
/// for the given symbol key, useful for exploring all overloads or redefinitions
/// of a symbol in the workspace.
///
/// Returns an empty vector if no definitions are found.
///
/// # Examples
///
/// ```
/// let key = SymbolKey {
///     pkg: "MyPackage".to_string(),
///     name: "method".to_string(),
///     kind: SymKind::Sub,
///     sigil: None,
/// };
/// let defs = index.find_defs(&key);
/// assert!(!defs.is_empty(), "method should be defined");
/// ```
pub fn find_defs(&self, key: &SymbolKey) -> Vec<Location> {
    let symbols = self.symbols.read();
    // Try exact structured key first
    let struct_key = format!("{}::{}", key.pkg, key.name);
    if let Some(candidates) = symbols.get(struct_key.as_str()) {
        return candidates.iter().map(|c| c.location.clone()).collect();
    }
    // Fallback: try bare name
    if let Some(candidates) = symbols.get(key.name.as_str()) {
        return candidates.iter().map(|c| c.location.clone()).collect();
    }
    Vec::new()
}
```

**Verify:**
```bash
cargo build -p perl-workspace --lib
cargo test -p perl-workspace --lib def
```

---

## Step 3: Update `find_definition()` to Delegate to `find_definitions()`

**File:** `crates/perl-workspace/src/workspace/workspace_index.rs`

**What changes:**
- Update the implementation of `find_definition()` to call `find_definitions().first()`
- Signature unchanged (backward compatible)
- This ensures consistency: `find_definition()` returns first, `find_definitions()` returns all

**Where:**
- Line 2349, in the body of `find_definition()`

**Current code (line 2349-2365):**
```rust
pub fn find_definition(&self, symbol_name: &str) -> Option<Location> {
    if let Some(location) = self.definition_candidates(symbol_name).into_iter().next() {
        return Some(location);
    }
    // ... fallback logic ...
}
```

**New code:**
```rust
pub fn find_definition(&self, symbol_name: &str) -> Option<Location> {
    if let Some(location) = self.find_definitions(symbol_name).into_iter().next() {
        return Some(location);
    }
    // ... fallback logic (unchanged) ...
}
```

**Why:** Ensures `find_definition()` always returns the same location as `find_definitions().first()`. Reduces code duplication.

**Verify:**
```bash
cargo build -p perl-workspace --lib
cargo test -p perl-workspace definition_ambiguity_regression_tests
```

---

## Step 4: Update `find_def()` to Delegate to `find_defs()`

**File:** `crates/perl-workspace/src/workspace/workspace_index.rs`

**What changes:**
- Update the implementation of `find_def()` to call `find_defs().first()`
- Signature unchanged (backward compatible)

**Where:**
- Line 3473, in the body of `find_def()`

**Current code (around 3473-3510):**
```rust
pub fn find_def(&self, key: &SymbolKey) -> Option<Location> {
    // ... implementation ...
}
```

**New code:**
```rust
pub fn find_def(&self, key: &SymbolKey) -> Option<Location> {
    self.find_defs(key).into_iter().next()
}
```

**Verify:**
```bash
cargo build -p perl-workspace --lib
cargo test -p perl-workspace --lib
```

---

## Step 5: Update LSP Navigation Handler to Support Multiple Definitions

**File:** `crates/perl-lsp-rs/src/runtime/language/navigation.rs`

**What changes:**
- Modify `find_symbol_key_definition_location()` (line 534) to return `Vec<Location>`
- Update its call sites to handle Vec instead of Option

**Where:**
- Line 534 function signature: Change return type from `Option<Location>` to `Vec<Location>`
- Lines 534-567: Update function body

**Current code (534-567):**
```rust
fn find_symbol_key_definition_location(
    workspace_index: &crate::workspace_index::WorkspaceIndex,
    symbol_key: &crate::workspace_index::SymbolKey,
) -> Option<crate::workspace_index::Location> {
    // ... returns Option<Location> ...
}
```

**New code:**
```rust
fn find_symbol_key_definition_location(
    workspace_index: &crate::workspace_index::WorkspaceIndex,
    symbol_key: &crate::workspace_index::SymbolKey,
) -> Vec<crate::workspace_index::Location> {
    // ... collect all definitions and return Vec ...
}
```

**Detailed implementation:**
1. Replace `find_def()` with `find_defs()` to get all candidates
2. Replace `find_definition()` with `find_definitions()` to get all candidates
3. Combine results from both searches into a single Vec
4. Return empty Vec if no definitions found

**Verify:**
```bash
cargo build -p perl-lsp-rs --lib
cargo test -p perl-lsp-rs --lib 2>&1 | head -50
```

---

## Step 6: Update Call Site: `handle_definition_inner()` (Primary Handler)

**File:** `crates/perl-lsp-rs/src/runtime/language/navigation.rs`

**What changes:**
- Line 1412: Update call to `find_symbol_key_definition_location()` to handle Vec<Location>
- Build LSP response array from all definitions

**Where:**
- Line 1412-1425 (inside `handle_definition_inner()`)

**Current code:**
```rust
if let Some(def_location) = find_symbol_key_definition_location(
    workspace_index,
    &workspace_symbol_key,
) {
    // Convert single location to LSP array
    if let Some(lsp_location) =
        crate::workspace_index::lsp_adapter::to_lsp_location(&def_location,)
    {
        return Ok(Some(json!([lsp_location])));
    }
}
```

**New code:**
```rust
let def_locations = find_symbol_key_definition_location(
    workspace_index,
    &workspace_symbol_key,
);
if !def_locations.is_empty() {
    let lsp_locations: Vec<_> = def_locations
        .iter()
        .filter_map(|loc| crate::workspace_index::lsp_adapter::to_lsp_location(loc))
        .collect();
    if !lsp_locations.is_empty() {
        return Ok(Some(json!(lsp_locations)));
    }
}
```

**Verify:**
```bash
cargo build -p perl-lsp-rs --lib
cargo test -p perl-lsp-rs --lib definition 2>&1 | grep -E "test_|passed|failed"
```

---

## Step 7: Update Call Site: `find_workspace_definition_location()` (Helper)

**File:** `crates/perl-lsp-rs/src/runtime/language/navigation.rs`

**What changes:**
- Line 569-600: Update `find_workspace_definition_location()` to return `Vec<Location>`
- Update all call sites of `find_workspace_definition_location()` to handle Vec

**Where:**
- Line 569 function signature
- Lines 581-600 function body
- Call sites: lines 1174, 1200, 1225, 1261, 1289 (update these to handle Vec)

**Current code (line 569-600):**
```rust
fn find_workspace_definition_location(
    workspace_index: &crate::workspace_index::WorkspaceIndex,
    pkg: &str,
    name: &str,
    doc_uri: Option<&str>,
) -> Option<crate::workspace_index::Location> {
    // ... returns single location ...
}
```

**New code:**
```rust
fn find_workspace_definition_location(
    workspace_index: &crate::workspace_index::WorkspaceIndex,
    pkg: &str,
    name: &str,
    doc_uri: Option<&str>,
) -> Vec<crate::workspace_index::Location> {
    // ... collect and return all definitions ...
}
```

**Call site pattern (e.g., line 1174):**
```rust
// Old:
if let Some(result) = lookup_workspace_definition(...) { return Ok(Some(result)); }

// New:
let result = lookup_workspace_definition(...);
if !result.is_empty() { return Ok(Some(json!(result))); }
```

**Verify:**
```bash
cargo build -p perl-lsp-rs --lib
cargo clippy -p perl-lsp-rs --lib 2>&1 | grep "error\|warning" | head -20
```

---

## Step 8: Add Tests to Verify Multiple Definitions Behavior

**File:** `crates/perl-workspace/tests/definition_ambiguity_regression_tests.rs`

**What changes:**
- Add new test `test_find_definitions_returns_all_candidates()`
- Update existing test comment at line 24 (remove the TODO about "future candidate API")
- Add test for LSP response array format

**Where:**
- After line 151 (end of existing tests)

**New tests to add:**

```rust
#[test]
fn find_definitions_returns_all_candidates() -> Result<(), Box<dyn std::error::Error>> {
    let index = WorkspaceIndex::new();
    let alpha_uri = file_url("/workspace/lib/Alpha.pm")?;
    let beta_uri = file_url("/workspace/lib/Beta.pm")?;

    index.index_file(alpha_uri.clone(), "package Alpha;\nsub same { 1 }\n".to_string())?;
    index.index_file(beta_uri.clone(), "package Beta;\nsub same { 1 }\n".to_string())?;

    let definitions = index.find_definitions("same");
    assert_eq!(definitions.len(), 2, "should find both definitions");
    let uris: Vec<_> = definitions.iter().map(|d| d.uri.as_str()).collect();
    assert!(uris.contains(&"file:///workspace/lib/Alpha.pm"));
    assert!(uris.contains(&"file:///workspace/lib/Beta.pm"));
    Ok(())
}

#[test]
fn find_definitions_single_returns_array() -> Result<(), Box<dyn std::error::Error>> {
    let index = WorkspaceIndex::new();
    let uri = file_url("/workspace/lib/Single.pm")?;

    index.index_file(uri.clone(), "package Single;\nsub only_one { 1 }\n".to_string())?;

    let definitions = index.find_definitions("only_one");
    assert_eq!(definitions.len(), 1);
    assert_eq!(definitions[0].uri, "file:///workspace/lib/Single.pm");
    Ok(())
}

#[test]
fn find_definitions_nonexistent_returns_empty() -> Result<(), Box<dyn std::error::Error>> {
    let index = WorkspaceIndex::new();
    let uri = file_url("/workspace/lib/Empty.pm")?;

    index.index_file(uri, "package Empty;\n".to_string())?;

    let definitions = index.find_definitions("nonexistent");
    assert!(definitions.is_empty());
    Ok(())
}

#[test]
fn find_definition_delegates_to_find_definitions() -> Result<(), Box<dyn std::error::Error>> {
    let index = WorkspaceIndex::new();
    let uri = file_url("/workspace/lib/Test.pm")?;

    index.index_file(uri, "package Test;\nsub foo { 1 }\n".to_string())?;

    let single = index.find_definition("Test::foo");
    let all = index.find_definitions("Test::foo");
    
    assert!(single.is_some());
    assert_eq!(all.len(), 1);
    assert_eq!(single.unwrap().uri, all[0].uri);
    Ok(())
}

#[test]
fn find_defs_returns_all_for_symbol_key() -> Result<(), Box<dyn std::error::Error>> {
    let index = WorkspaceIndex::new();
    let file_a = file_url("/workspace/lib/FileA.pm")?;
    let file_b = file_url("/workspace/lib/FileB.pm")?;

    // Two files both declaring same package and method
    index.index_file(file_a, "package MyPkg;\nsub method { 1 }\n".to_string())?;
    index.index_file(file_b, "package MyPkg;\nsub method { 2 }\n".to_string())?;

    let key = crate::workspace::workspace_index::SymbolKey {
        pkg: "MyPkg".to_string(),
        name: "method".to_string(),
        kind: crate::workspace::workspace_index::SymKind::Sub,
        sigil: None,
    };

    let defs = index.find_defs(&key);
    assert_eq!(defs.len(), 2, "should find definitions in both files");
    Ok(())
}
```

Also update the existing test at line 9-27 to verify that `find_definitions()` returns all:

```rust
#[test]
fn same_bare_sub_name_in_two_packages_is_deterministic() -> Result<(), Box<dyn std::error::Error>> {
    let index = WorkspaceIndex::new();
    let alpha_uri = file_url("/workspace/lib/Alpha.pm")?;
    let beta_uri = file_url("/workspace/lib/Beta.pm")?;

    index.index_file(alpha_uri, "package Alpha;\nsub collide { 1 }\n".to_string())?;
    index.index_file(beta_uri, "package Beta;\nsub collide { 1 }\n".to_string())?;

    // Old behavior: find_definition returns one
    let first = index.find_definition("collide").ok_or("definition should resolve")?;
    let second = index.find_definition("collide").ok_or("definition should resolve")?;
    assert_eq!(first.uri, second.uri, "find_definition should be deterministic");

    // New behavior: find_definitions returns all
    let all_defs = index.find_definitions("collide");
    assert_eq!(all_defs.len(), 2, "find_definitions should expose both candidates");
    let uris: Vec<_> = all_defs.iter().map(|d| d.uri.as_str()).collect();
    assert!(uris.contains(&"file:///workspace/lib/Alpha.pm"));
    assert!(uris.contains(&"file:///workspace/lib/Beta.pm"));
    Ok(())
}
```

**Verify:**
```bash
cargo test -p perl-workspace definition_ambiguity_regression_tests -- --test-threads=1
cargo test -p perl-workspace definition
```

---

## Step 9: Verify Workspace Tests Pass

**Command:**
```bash
cd /path/to/workspace
cargo test -p perl-workspace --lib 2>&1 | tail -20
```

**Expected:** All tests pass, including new definition tests.

---

## Step 10: Verify LSP Tests Pass

**Command:**
```bash
cargo test -p perl-lsp-rs --lib 2>&1 | tail -20
```

**Expected:** All tests pass; no regressions in navigation/definition tests.

---

## Step 11: Format and Lint

**Command:**
```bash
cargo xtask fmt
cargo clippy -p perl-workspace --lib
cargo clippy -p perl-lsp-rs --lib
```

**Expected:** No clippy warnings or fmt changes needed.

---

## Step 12: Full Workspace Test

**Command:**
```bash
cargo test --workspace --lib 2>&1 | grep -E "test result:|FAILED" | head -20
```

**Expected:** All tests pass; no FAILED lines.

---

## Summary of Changes by File

| File | Change | Lines | Type |
|------|--------|-------|------|
| `crates/perl-workspace/src/workspace/workspace_index.rs` | Add `find_definitions()` public method | +20 | New API |
| `crates/perl-workspace/src/workspace/workspace_index.rs` | Add `find_defs()` public method | +25 | New API |
| `crates/perl-workspace/src/workspace/workspace_index.rs` | Update `find_definition()` body | 3 | Update |
| `crates/perl-workspace/src/workspace/workspace_index.rs` | Update `find_def()` body | 1 | Update |
| `crates/perl-lsp-rs/src/runtime/language/navigation.rs` | Update `find_symbol_key_definition_location()` | ~30 | Update |
| `crates/perl-lsp-rs/src/runtime/language/navigation.rs` | Update `find_workspace_definition_location()` | ~30 | Update |
| `crates/perl-lsp-rs/src/runtime/language/navigation.rs` | Update `handle_definition_inner()` call sites | ~5 | Update |
| `crates/perl-workspace/tests/definition_ambiguity_regression_tests.rs` | Update existing test | 10 | Update |
| `crates/perl-workspace/tests/definition_ambiguity_regression_tests.rs` | Add 5 new tests | +130 | New Tests |

**Total lines added:** ~250, mostly tests and comments.

---

## Notes for Builder

1. **Compilation order**: Follow steps 1-4 first (WorkspaceIndex API additions), which are independent. Then steps 5-7 (LSP handler updates) can be done in any order, as they depend only on the new WorkspaceIndex API.

2. **Testing strategy**: 
   - After step 4: workspace tests should pass
   - After step 7: LSP tests should pass
   - Step 8: Add new tests that verify the new behavior
   - Step 9-10: Comprehensive verification

3. **Backward compatibility**: All existing public APIs (`find_definition()`, `find_def()`) remain unchanged in signature. Only their implementation changes to delegate to the new Vec-returning methods. Callers experience zero breaking changes.

4. **LSP behavior**: The LSP protocol already allows `Location | Location[]` for definition response, so returning an array is valid. Clients that only show the first item will still work; clients that show all will get enhanced experience.

5. **Follow-up work**: Per context.md and issue description, deterministic ordering (sort by file modification time or priority list) is deferred to a follow-up issue. For now, insertion order is preserved and documented.
