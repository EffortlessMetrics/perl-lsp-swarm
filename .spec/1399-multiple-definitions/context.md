# Context: Support Multiple Definitions for the Same Symbol

## Problem Statement

The `WorkspaceIndex` currently exposes only a single definition for symbols via the public `find_definition()` API, even though it internally stores a `Vec<DefinitionCandidate>` for each symbol name. This means Go-to-Definition in the LSP server returns only the first (non-deterministically) found definition when a symbol is redefined across multiple files or packages.

In Perl, it's common to have the same symbol (e.g., `sub foo`) defined in multiple files:
- Multiple files contributing to the same package
- Symbol redefinition across different modules
- Testing fixtures with duplicated package/function names

The current implementation returns whichever candidate the hash table happens to yield first, leading to non-deterministic navigation results.

## Current State Verification

### Infrastructure Already Present

1. **Internal storage** (workspace_index.rs:1169): `symbols: Arc<RwLock<HashMap<String, Vec<DefinitionCandidate>>>>`
   - Already stores multiple candidates per symbol

2. **Internal API** (workspace_index.rs:2367): `pub(crate) fn definition_candidates()` 
   - Returns `Vec<Location>` but is crate-internal only

3. **Public API** (workspace_index.rs:2349): `pub fn find_definition()`
   - Returns `Option<Location>` (single)
   - Calls `.into_iter().next()` on `definition_candidates()`, discarding the rest

4. **LSP handler** (navigation.rs:1388, 1423, etc.):
   - Already returns `json!(result)` where result is a `Vec<Value>`
   - LSP spec for `textDocument/definition` response allows `Location | Location[]`

### Test TODO

File: `crates/perl-workspace/tests/definition_ambiguity_regression_tests.rs:24`
```
"current implementation returns a single winner; future candidate API should expose both"
```

This explicitly documents the desired behavior as a future enhancement.

## Design Decisions

### 1. Make `definition_candidates` Public

Instead of creating a new method, promote `definition_candidates()` from `pub(crate)` to `pub` and rename to `find_definitions()` for API symmetry with `find_definition()`.

**Rationale:**
- Aligns with Rust's naming convention (plural for Vec return)
- Provides access to all candidates without changing internal storage
- Backward compatible (existing `find_definition()` can delegate to `find_definitions().first()`)

### 2. Add `find_defs` for SymbolKey

Create a companion method `pub fn find_defs(&self, key: &SymbolKey) -> Vec<Location>` parallel to existing `find_def()` which returns a single location.

**Rationale:**
- Maintains symmetry with the `find_definition()` / `find_definitions()` pair
- SymbolKey-based lookups are used in semantic navigation (exact symbols)
- Allows LSP handlers to request all definitions for a given symbol key

### 3. Update LSP Handler to Use Multiple Definitions

Modify `find_symbol_key_definition_location()` to return `Vec<Location>` and update call sites to handle all definitions.

**Rationale:**
- LSP spec supports multiple locations in response (§5.1.3)
- Allows editors to show all known definitions, not just first
- Preserves first-definition fallback for UI that only shows one

### 4. Sorting/Ordering

The existing `DefinitionCandidate` order is preserved. Per the issue description, "if multiple definitions exist, the 'latest' one (based on file-system modification time or a predefined priority list) is sorted first." However, this is deferred as optimization and marked as a follow-up issue. The current spec focuses on making all definitions available; sorting is left for a future priority pass.

## Alternatives Rejected

1. **Create entirely new internal storage structure** — Rejected because the current `Vec<DefinitionCandidate>` already does what we need; promoting it to public is simpler.

2. **Return a custom struct with metadata per definition** — Rejected because the LSP Location type is sufficient; metadata can be added in a follow-up.

3. **Cache sorted definitions** — Rejected as premature optimization; deterministic ordering is tracked as a follow-up issue.

## Related Issues / Prior Art

- **LSP Spec (§5.1)**: `textDocument/definition` response allows `Location | Location[]`
- **Parser Contracts**: None (workspace indexing is language-agnostic)
- **Existing tests**: `definition_ambiguity_regression_tests.rs` covers the case of multiple definitions but explicitly only tests single return for now

## Change Scope

This is a **medium-sized change**:
- Affects workspace indexing public API (2 new public methods)
- Affects LSP navigation handlers (3-4 call sites)
- No parser changes
- No DAP changes
- Test additions: 3-4 tests covering multiple definitions scenarios

The core semantic logic remains unchanged; this is purely about exposing existing internal capability.
