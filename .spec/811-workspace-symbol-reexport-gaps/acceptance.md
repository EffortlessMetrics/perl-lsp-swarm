# Acceptance Criteria: Workspace-Symbol Dual Indexing Gaps in Re-Export Chains

## §Behavior

| Scenario | Input/Condition | Expected Result | Test Case |
|----------|-----------------|-----------------|-----------|
| Goto-definition on re-exported bare call | Module A re-exports B::optional via EXPORT_OK; call `optional()` from A's namespace | Definition location points to B's definition, not A's import statement | `test_reexport_chain_goto_definition_bare_call` |
| Goto-definition on qualified re-exported call | Module A re-exports B::optional; call `B::optional()` from any file | Definition location points to B's definition in B.pm | `test_reexport_chain_goto_definition_qualified_call` |
| Workspace-symbol query deduplication | Query "optional" with both A and B in workspace (B defines, A re-exports) | B's definition appears first; A's import statement does not appear as separate entry | `test_reexport_workspace_symbol_dedup_and_ranking` |
| Find-references includes re-export sites | Find references to "optional" when B defines it, A re-exports | Results include B's definition, A's import statement, and all call sites | `test_reexport_find_references_includes_sites` |
| Bare-name collision resolution (scope-aware deferred) | Two packages define same bare name; call from importing context | References resolve to lexically imported symbol; definition points to imported source | `test_bare_name_collision_scope_aware` (DEFERRED - scope graph) |
| Symbol ranking in workspace-symbol | Query returns symbols; original definition vs re-export site present | Original definition ranks higher in results | `test_reexport_workspace_symbol_ranking` |

## §Hazards

| Class | Surface | Hazard | Mitigation | Test |
|-------|---------|--------|------------|------|
| LSP-1: Protocol Desync | `find_definition()` → LSP Location | Returning re-export site instead of original definition breaks hover, goto-def consistency with other editors | Follow re-export chains via ImportExportIndex before returning Location | `test_reexport_chain_goto_definition_bare_call` |
| LSP-2: Cross-File Boundaries | `symbol_uri_reachable()` in EffectiveIncContext | Filtering by @INC membership alone misses re-exported symbols from outside @INC path | Extend reachability check to include re-exporting modules' symbols | `test_reexport_cross_file_reachability` |
| LSP-3: Index Consistency | `find_references()` vs `find_symbols()` vs `find_definition()` | Three consumers diverge on re-export chain truth (e.g., definition says B, references says A) | Ensure all three use same import-export chain resolution logic | `test_reexport_consumer_consistency` |
| PARSER-1: Semantic Fidelity | Export set extraction from @EXPORT_OK | Mis-parsed or missing EXPORT_OK means re-export chains are invisible to index | Verify parser correctly extracts @EXPORT_OK as ExportSet facts | Existing parser tests in `perl-parser` |
| PARSER-2: Incremental Index Staleness | File re-indexing of exports | Stale export entries remain after file edit; @EXPORT_OK removed but old edges persist | Properly clean up old export edges in ImportExportIndex.remove_module_exports() | `test_reexport_incremental_export_update` |
| TEST-1: Adversarial Edge Cases | Three-level re-export chain (B re-exports C, A re-exports B) | find_definition() must trace through full chain, not stop at first re-export | Test A::symbol → chains through B → resolves to C's definition | `test_reexport_chain_three_levels_deep` |

## §Contracts

### PARSER_CONTRACTS.md References

- **Symbol Decl Extraction** (`perl-symbol::surface::decl::extract_symbol_decls`): Already extracts sub/package definitions. No change needed.
- **Export Set Extraction** (`perl-semantic-facts::ExportSet`): Already captures `@EXPORT` and `@EXPORT_OK`. Must be wired into reference index as typed edges.

### LSP Protocol References

- **textDocument/definition** (LSP 3.17): Must return single Location or Location[]. Re-export handling ensures consistent Location across calls to same symbol.
- **textDocument/references** (LSP 3.17): Must include re-export sites (import statements in re-exporting modules) as reference locations.
- **workspace/symbol** (LSP 3.17): Must rank results; original definitions must appear before re-export sites in candidate list.

### Internal Contracts

- **WorkspaceIndex::find_definition()**: Input `symbol_name` (bare or qualified) → Output `Location` of definition (original, not re-export).
- **WorkspaceIndex::find_references()**: Input `symbol_name` → Output includes definition location + all usage sites + re-export import statements.
- **ImportExportIndex**: Must expose query interface to trace `Symbol → [Exporting Modules]` chains.
- **EffectiveIncContext::symbol_uri_reachable()**: Input `symbol_uri` → Output true if reachable via @INC OR if re-exported by reachable module.

## §API-Shape

### New/Modified Types

| Type | Module | Change | Reason |
|------|--------|--------|--------|
| `ReferenceIndex` | `perl-workspace/semantic/references.rs` | Add method `export_edges_for(&symbol_name) -> Vec<(from_module, to_symbol)>` | Enable `find_definition()` to query which modules re-export a symbol |
| `ImportExportIndex` | `perl-workspace/semantic/imports.rs` | Add method `query_export_chain(&symbol) -> Option<(original_module, chain)>` | Trace re-export chains for definition resolution |
| (Optional) `SymbolOrigin` enum | New in `perl-workspace/src/...` | `Original { uri, range }` \| `ReExportedFrom { original, via: Vec<Module> }` | Annotate symbols in workspace-symbol results with origin info |

### Modified Functions

| Function | File | Changes | Impact |
|----------|------|---------|--------|
| `find_definition(&self, symbol_name)` | workspace_index.rs:2349 | After finding candidate, check if it's a re-export; if so, follow chain to original | Breakage: None (returns same Location, sourced correctly) |
| `find_references(&self, symbol_name)` | workspace_index.rs:2171 | No change to signature; internal logic already includes all variants | Breakage: None |
| `find_symbols(&self, query)` | workspace_index.rs:3067 | Sort results: original definitions before re-export sites | Breakage: None (same symbols, different order) |
| `symbol_uri_reachable(&self, symbol_uri)` | inc_context/mod.rs:73 | Check if symbol is re-exported by a reachable module (new case) | Breakage: None (stricter filtering is safe) |

### Duplication Risk

**Grep check for dual symbol names:**
```bash
grep -n 'qualified_name.*bare\|bare.*qualified\|both.*qualified\|both.*bare' crates/perl-workspace/src/workspace/workspace_index.rs
```

All symbols are already stored under both names via `incremental_add_symbols()` (line 1230-1270). No new duplication risks.

**Caller count:**

- `find_definition()`: Called from LSP providers (completion, navigation, hover, diagnostics), multiple test files
- `find_references()`: Called from LSP references provider, rename refactoring, test files
- `find_symbols()`: Called from LSP workspace/symbol provider, completion fallback, test files
- `symbol_uri_reachable()`: Called from completion.rs, navigation.rs, misc.rs, missing_module_lookup.rs (≈5 call sites)

All are internal LSP server logic; no external API breakage.

## §Test-Grid

| Category | Test Name | Invariant |
|----------|-----------|-----------|
| **Positive: Basic Re-Export** | `test_reexport_module_finds_definition_bare` | A re-exports B::foo; find_definition("foo") → B's definition |
| **Positive: Basic Re-Export** | `test_reexport_module_finds_definition_qualified` | A re-exports B::foo; find_definition("B::foo") → B's definition |
| **Positive: Cross-File Consistency** | `test_reexport_consumer_consistency` | All three (find_definition, find_references, find_symbols) agree on re-export chain resolution |
| **Positive: Multi-Level Chain** | `test_reexport_chain_three_levels` | C defines sym; B re-exports C; A re-exports B; A::sym → C's definition |
| **Negative: No False Re-Export** | `test_reexport_no_confusion_with_same_name_different_package` | Pkg1::foo and Pkg2::foo are distinct; bare "foo" resolves correctly per context |
| **Negative: Non-Exporter Still Works** | `test_reexport_non_exporter_modules_unaffected` | Modules not using Exporter still work; no regression |
| **Adversarial: Cyclic Re-Export** | `test_reexport_chain_detects_cycles` | If A re-exports B and B re-exports A, chain resolution terminates without infinite loop |
| **Adversarial: Partial Export** | `test_reexport_partial_export_ok` | @EXPORT_OK lists only subset of subs; unlisted subs are not "re-exported" |
| **State Transition: Incremental Edit** | `test_reexport_incremental_export_update` | Edit file to add/remove @EXPORT_OK; index updates; definition chains re-resolve |
| **Regression: Bare-Name Lookup** | `test_dual_index_bare_lookup_unchanged` | Existing bare-name tests still pass (no regression) |

## §Blast-Radius

### Consumers

**Internal LSP providers** (all in `crates/perl-lsp-rs/src/runtime/language/`):
- `completion.rs`: Calls `find_definition()` at line 518, 1007 for import context filtering
- `navigation.rs`: Calls `find_definition()` for goto-definition LSP handler
- `misc.rs`: Calls `find_definition()` for hover provider
- `missing_module_lookup.rs`: Filters symbols through `symbol_uri_reachable()`

**Workspace index consumers** (in `crates/perl-workspace/`):
- `query_symbol_references()` method: Uses `find_definition()` and `find_references()` together
- Tests in `dual_indexing_tests.rs`, comprehensive_unit_tests.rs, etc.

### Downstream Crates

- `perl-lsp-rs`: Consumes `WorkspaceIndex::find_*` methods. No API breakage (same signatures, better results).
- `perl-dap`: May use workspace index for variable resolution (indirect dependency via perl-lsp-rs).

### Must-Not-Touch Boundary

- `perl-parser`: No changes (symbol extraction already correct).
- `perl-symbol`: No changes (surface facts already defined).
- `perl-semantic-facts`: No new types (existing `ExportSet`, `ImportSpec` sufficient).
- `perl-module`: No changes (module resolution orthogonal to re-export semantics).
- LSP protocol contracts (lsp-types): No version bump needed (existing `/definition`, `/references`, `/symbol` suffice).

### Verification

1. **Parser corpus**: Run `just cpan-corpus-check` — no regression in baseline symbol extraction.
2. **Dual-indexing tests**: All existing tests in `dual_indexing_tests.rs` must pass (regression check).
3. **Integration tests**: LSP provider tests for hover, goto-def, references must pass.
4. **Incremental index**: File edit tests in `workspace_index_tests.rs` must pass (staleness/cleanup check).

---

## Implementation Notes

- Re-export tracking via `ImportExportIndex` requires no new public API on `ImportExportIndex` itself; internal plumbing only.
- `find_definition()` logic change is internal; no signature change.
- All six LSP hazard classes are addressed in the test grid.
- Scope-aware resolution (bare-name collision within same package) is deferred; current fix handles re-export chains only.
