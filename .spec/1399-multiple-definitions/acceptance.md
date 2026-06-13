# Acceptance Criteria: Support Multiple Definitions for the Same Symbol

## §Behavior

| Condition | Input | Expected Result |
|-----------|-------|-----------------|
| Single definition | Query symbol with only one definition in workspace | Return array with one location |
| Multiple definitions in different packages | Query `foo` with definitions in `PackageA::foo` and `PackageB::foo` | Return array with both locations (order preserved from internal storage) |
| Multiple definitions in same package, different files | Two files both declare `package X; sub foo` | Return array with both locations |
| Bare symbol lookup (unqualified) | Query bare name "foo" with multiple candidates | Return all candidates where "foo" is defined |
| Qualified symbol lookup | Query "Package::foo" with one definition | Return array with single location for that package |
| No definition exists | Query non-existent symbol | Return empty array |
| Deterministic on repeated calls | Query same symbol twice | Return locations in same order both times |
| LSP goto definition request | LSP client requests `textDocument/definition` with cursor on symbol with multiple defs | Return Location array with all candidates |

## §Hazards

### WORKSPACE-1: Index Consistency on Multi-Definition Scenarios
**Surface:** `WorkspaceIndex::definition_candidates()` / `find_defs()` public API
**Risk:** Race condition if definitions are added/removed concurrently across file edits while LSP is returning results
**Mitigation:** The `Vec<DefinitionCandidate>` is behind `Arc<RwLock<>>`, so snapshot is atomic at read time. Test with file edit mid-iteration.
**Adversarial test:** Index file A, query, start returning, edit file A (remove definition), verify LSP client sees consistent snapshot or proper invalidation.

### WORKSPACE-2: Ordering Stability for Identical Symbols
**Surface:** `DefinitionCandidate` order in Vec (currently insertion order from hash iteration)
**Risk:** Order non-determinism from HashMap iteration causing flaky tests or inconsistent editor behavior
**Mitigation:** Document that order is insertion order (non-guaranteed), sort by (uri, line, column) in public APIs for determinism. Per context.md, sorting is deferred; for now, document as "order undefined, use sort if needed."
**Adversarial test:** Index same symbol in different file order, verify first call and second call return same candidate first. If non-deterministic, flag as known limitation for follow-up issue.

### WORKSPACE-3: LSP Client Handling of Multiple Locations
**Surface:** `navigation.rs` LSP handler returning `json!(Vec<Location>)` instead of single `json!(location)`
**Risk:** LSP client may not expect array in response; some editors only show first or may error
**Mitigation:** LSP spec (§5.1) explicitly allows `Location | Location[]` for definition response. Verify vscode-extension can handle array. Test with mock LSP client.
**Adversarial test:** Send `textDocument/definition` with multiple definitions, verify mock client (and real vscode if available) handles array response correctly.

### WORKSPACE-4: Cache Invalidation on File Edits
**Surface:** `WorkspaceIndex::definition_candidates()` returns `Vec<Location>` clones
**Risk:** Stale locations if a file is edited after query but before result is used
**Mitigation:** Each Location contains a URI; navigator should reload text at that URI when user jumps to definition. Existing pattern (per navigation.rs) already handles this. Test: edit file after definition query but before jump.
**Adversarial test:** Query definitions, modify file (remove sub), jump to definition (old location), verify graceful handling (LSP shows empty file or previous version, doesn't crash).

### WORKSPACE-5: Backward Compatibility
**Surface:** `WorkspaceIndex::find_definition()` (existing public API, now delegates to `find_definitions().first()`)
**Risk:** Breaking change if code expects single Location instead of Option<Location>
**Mitigation:** `find_definition()` returns `Option<Location>` (unchanged signature); just delegates to `find_definitions().first()`. Old call sites work unchanged.
**Adversarial test:** Ensure all call sites of `find_definition()` (hover, rename, etc.) still work with new implementation.

### WORKSPACE-6: Query Performance with Large Candidate Sets
**Surface:** `find_definitions()` returns cloned Vec of potentially many Location objects
**Risk:** Perf degradation if a symbol has 100+ definitions and we clone all of them for LSP response
**Mitigation:** Locations are small (URI + Range); cloning is acceptable. If perf becomes issue, defer to return `&[Location]` references in follow-up. For now, document expected behavior with many candidates.
**Adversarial test:** Create workspace with 100+ redefinitions of same symbol, measure query time (should be <1ms per spec target).

## §Contracts

### Parser Contracts
- **N/A** — This change does not modify parsing behavior. No parser invariants affected.

### LSP Protocol Contracts
- **textDocument/definition request/response** (LSP §5.1.3)
  - Response type: `Location | Location[]`
  - Surface: `crates/perl-lsp-rs/src/runtime/language/navigation.rs::handle_definition_inner()`
  - Current: Returns `json!([single_location])`
  - After: Returns `json!(vec_of_locations)` where vec may have 1+ elements
  - No protocol change; response type was always allowed to be array

### Workspace Index Contracts
- **`WorkspaceIndex::find_definition()` (existing)**
  - Signature: `pub fn find_definition(&self, symbol_name: &str) -> Option<Location>`
  - Behavior: Returns first definition (backward compatible)
  - New: Delegates to `find_definitions().first()`

- **`WorkspaceIndex::find_definitions()` (new, renamed from internal `definition_candidates()`)**
  - Signature: `pub fn find_definitions(&self, symbol_name: &str) -> Vec<Location>`
  - Behavior: Returns all known definitions for a symbol (insertion order)
  - Visibility: Public (promoted from `pub(crate)`)

- **`WorkspaceIndex::find_defs()` (new)**
  - Signature: `pub fn find_defs(&self, key: &SymbolKey) -> Vec<Location>`
  - Behavior: Returns all definitions for a SymbolKey (parallel to `find_def()`)
  - Visibility: Public

## §API-Shape

### New Public Types/Functions
| Item | Path | Signature | Purpose |
|------|------|-----------|---------|
| `find_definitions()` | `WorkspaceIndex::find_definitions()` | `pub fn(&str) -> Vec<Location>` | Get all definitions for a symbol name (bare or qualified) |
| `find_defs()` | `WorkspaceIndex::find_defs()` | `pub fn(&SymbolKey) -> Vec<Location>` | Get all definitions for a structured symbol key |

### Modified Public Functions
| Item | Path | Old Signature | New Signature | Note |
|------|------|---------------|---------------|------|
| `find_definition()` | `WorkspaceIndex::find_definition()` | `pub fn(&str) -> Option<Location>` | `pub fn(&str) -> Option<Location>` | Unchanged; now delegates to `find_definitions().first()` |
| `find_def()` | `WorkspaceIndex::find_def()` | `pub fn(&SymbolKey) -> Option<Location>` | `pub fn(&SymbolKey) -> Option<Location>` | Unchanged; now delegates to `find_defs().first()` |

### Duplicate-Risk Grep
```bash
# Grep for symbol methods that may need updates
grep -rn "find_definition\|find_def\|definition_candidates" crates/perl-lsp-rs/src/runtime/language/ --include="*.rs"

# Check for return type changes needed in handlers
grep -rn "Option<.*Location>\|Some(.*location" crates/perl-lsp-rs/src/runtime/language/navigation.rs
```

### Caller Count
- `find_definition()`: 10+ callers (hover, navigation, etc.) - all remain compatible (Option<Location> unchanged)
- New `find_definitions()`: Estimated 3 new callers (primary definition handler, possibly hover/completion enhancements)
- New `find_defs()`: Estimated 2-3 new callers (semantic navigation, inheritance lookups)

## §Test-Grid

### Positive Tests (Behavior Should Work)

| Test Name | Scenario | Input | Expected Output | Invariant |
|-----------|----------|-------|-----------------|-----------|
| `test_single_definition_returns_single_location` | One definition in workspace | Query "Foo::bar" with one `package Foo; sub bar` | Array with one Location | Result count = 1 |
| `test_multiple_definitions_bare_name` | Same bare name in different packages | Index `package A; sub foo` and `package B; sub foo`, query "foo" | Array with 2+ Locations | All returned locations define "foo" |
| `test_multiple_definitions_same_package_different_files` | Two files for same package | Index two `package X; sub run` in different URIs, query "X::run" | Array with 2 Locations | Both URIs in result, same package |
| `test_qualified_lookup_single_match` | Qualified name with one definition | Query "Specific::Package::method" with one definition | Array with one Location | Unambiguous match |
| `test_no_definition_returns_empty` | Non-existent symbol | Query "nonexistent::symbol" | Empty array `[]` | No false positives |
| `test_find_definitions_deterministic_order` | Multiple calls same symbol | Query same symbol twice | Same order both times | Consistent ordering |
| `test_lsp_definition_returns_array` | LSP handler with multiple defs | LSP `textDocument/definition` with 2+ defs | `json!([loc1, loc2, ...])` | Response is valid JSON array |

### Negative Tests (Should Fail/Return Empty)

| Test Name | Scenario | Input | Expected Output | Invariant |
|-----------|----------|-------|-----------------|-----------|
| `test_misspelled_symbol_returns_empty` | Typo in symbol | Query "foo" when only "foo_bar" defined | Empty array | No partial matches |
| `test_after_file_removal_old_defs_gone` | File with definition removed | Add definition, remove file, query | Only remaining definitions | Stale URIs cleaned up |
| `test_undefined_package_returns_empty` | Package with no defs | Query "Nonexistent::method" | Empty array | No false results |

### Adversarial Tests (Edge Cases / Race Conditions)

| Test Name | Scenario | Input | Expected Output | Invariant |
|-----------|----------|-------|-----------------|-----------|
| `test_definitions_with_concurrent_edit` | LSP query mid-edit | Index A, start returning, edit A (add def), finish returning | Consistent snapshot from first read | No interleaving corruption |
| `test_many_definitions_performance` | Perf: 100+ identical symbols | Index 100 redefinitions, query all | All 100+ returned in <1ms | Meets perf spec |
| `test_order_stable_across_file_indexing_order` | Order independence | Index in order [A, B, C], then query; reindex in [C, B, A], query | Same ordering both times if deterministic | Consistent across index rebuild |
| `test_find_definitions_vs_find_definition_consistency` | Backward compat check | Query via old `find_definition()` vs new `find_definitions()` | `find_definition().next() == find_definitions().first()` | Old API still works |

## §Blast-Radius

### Consumers (Direct Callers)

1. **LSP Navigation Handler** (`crates/perl-lsp-rs/src/runtime/language/navigation.rs`)
   - Call site: `find_symbol_key_definition_location()` (line 534) and `find_workspace_definition_location()` (line 569)
   - Change: These functions should now return `Vec<Location>` instead of `Option<Location>`, allowing handlers to collect all candidates
   - Impact: Medium — affects goto-definition, hover (if hover shows definition), rename-prep

2. **Hover Provider** (`crates/perl-lsp-rs/src/runtime/language/hover.rs`)
   - Call site: `workspace_index.find_definition(package_name)` (line 226 area)
   - Change: Can optionally use `find_definitions()` to show all definitions in hover info (optional enhancement)
   - Impact: Low — backward compatible if unchanged, enhanced if updated

3. **Call Hierarchy Provider** (`crates/perl-lsp-rs/src/call_hierarchy_provider/mod.rs`)
   - Call site: `index.find_definition(&candidate)` 
   - Change: Can use `find_definitions()` to walk all call targets
   - Impact: Low — call hierarchy already walks multiple call sites, so multiple definitions align with pattern

4. **Workspace Tests**
   - Call sites: `definition_ambiguity_regression_tests.rs` and new tests
   - Change: Tests now verify multiple definitions are returned
   - Impact: Test-only, non-shipping

### Downstream Crates (Transitive Consumers)

- **perl-lsp-rs**: Imports `WorkspaceIndex` from `perl-workspace`. Only affected if it uses `definition_candidates()` (which was private before). Current callers use `find_definition()` (unchanged).
- **perl-lsp** (binary): Depends on perl-lsp-rs. No direct impact.
- **perl-dap**: Does not currently use `find_definition()`. No impact.

### Must-Not-Touch Boundary

- **Parser** (`crates/perl-parser`, `crates/perl-lexer`): No changes. Parser already identifies all definitions; the change is purely about indexing/retrieval.
- **DAP** (`crates/perl-dap*`): No changes. DAP does not use WorkspaceIndex for definition lookups.
- **Module Resolution** (`crates/perl-module-*`): No changes. These provide path resolution for `use`/`require`; not affected by definition lookup.
- **LSP Protocol Surface**: No breaking changes. LSP spec already allows arrays in definition response.

### Risk Summary

- **Scope expansion risk**: LOW — All call sites are in navigation.rs; focused change.
- **Backward compatibility risk**: LOW — Existing APIs unchanged in signature; new APIs are additive.
- **Cross-subsystem impact**: LOW — Workspace indexing is isolated; minimal cascade to parser/DAP/module-resolution.
