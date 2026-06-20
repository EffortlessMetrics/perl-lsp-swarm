# Acceptance Criteria: Fix rename dedup() only removing consecutive duplicates

## §Behavior

| Input / Condition | Expected Result | Test Name |
|---|---|---|
| Symbol in both `symbol_table.symbols` and `symbol_table.references` at same location (e.g., `my $x = $x + 1`) | Exactly one TextEdit per unique location in output; no duplicates | `test_rename_no_duplicate_edits_for_shared_locations` |
| Multiple references to same symbol, no duplicates in either table | All references renamed; correct count of edits | `test_rename_variable` (existing) |
| Scoped rename on symbol with shadowing | Correct edits for declaration scope; no duplicates across scope boundaries | `test_scoped_rename_simple_variable` (existing) |
| Scoped rename with nested scopes | Deep scope traversal succeeds; no duplicate edits for same location across scopes | `test_collect_descendant_scopes_deep_nesting_correctness` (existing) |
| Rename with comments/strings options enabled | Additional edits from text search merged with symbol table edits; no duplicates | (covered by new test if edits overlap) |

## §Hazards

| Hazard Class | Surface | Description | Mitigation |
|---|---|---|---|
| **Logic (LOGIC-1)** | `RenameProvider::rename()` L196-197, `scoped_rename()` L284-285 | Partial sort + `dedup()` misses non-consecutive duplicates. Symptoms: LSP client applies rename twice at same location, corrupting output | Full sort ensures all equal edits become adjacent; `dedup()` then removes all true duplicates. Test verifies no location appears twice. |
| **Type Safety (TYPE-1)** | `TextEdit` struct (types.rs L8) | Without `Ord` derive, cannot call `.sort()` without custom comparator. Adding derive is safe: both `ByteSpan` and `String` impl `Ord`; comparison is by location then new_text | Derive `PartialOrd, Ord` on TextEdit; verify compile and clippy clean |
| **Scope Contamination (SCOPE-1)** | `scoped_rename()` L284-285 | Same dedup bug affects scoped renames; symbol shadowing can cause duplicates across scope boundaries if edits at same location exist in both tables | Apply same fix to both `rename()` and `scoped_rename()`; test scoped rename with shadowing |
| **Boundary Condition (BOUNDARY-1)** | Empty edits vec, single edit, many identical edits | `dedup()` on empty vec is no-op (correct). Single edit: no duplicates possible (correct). Many identical: full sort handles all cases | Test includes "symbol in both tables" which creates realistic duplication scenario |
| **Regression (REG-1)** | Existing rename tests | Full sort is stricter than partial sort (more deterministic). Any code relying on "unsorted except by start" will not break; final order is just more deterministic | Run full test suite (`cargo test -p perl-lsp-rs-core`); all existing tests must pass |
| **Compatibility (COMPAT-1)** | Callers of `rename()` and `scoped_rename()` | Return type `RenameResult` and edits vec structure unchanged; only the deduplication logic improves. No API breakage | No breaking changes; improvement is internal |

## §Contracts

**Parser Contracts** (from PARSER_CONTRACTS.md):
- Symbol table extraction: `perl_semantic_analyzer::symbol::{SymbolTable, SymbolExtractor}` — responsible for populating both `symbols` and `references` tables. This fix assumes both tables exist and may overlap; no parser contract changes required.
- Node location (SourceLocation): `perl_parser_core::SourceLocation` — provides start/end byte offsets. This fix relies on location equality for deduplication; no contract changes.

**LSP Protocol** (textDocument/rename):
- Input: `position` (byte offset), `newName` (string), options
- Output: `WorkspaceEdit` with edits array (TextEdit[] per file)
- Expectation: All locations within an edit list are unique (no client-side duplication)
- This fix ensures LSP response complies by removing duplicate edits before returning to client

**Semantic Contracts**:
- `adjust_location_for_sigil()` (apply.rs): Adjusts location based on symbol kind (scalar/$, array/@, hash/%). This can map two different source locations to the same TextEdit (e.g., both $x references adjusted the same way). Our fix handles this correctly by deduplicating the result.

## §API-Shape

**New API surface**: None. TextEdit struct gains `Ord` and `PartialOrd` derives (automatic, zero-cost).

**Modified API surface**: 
- `TextEdit` (types.rs L8): Adds `Ord, PartialOrd` derives. This is a pure addition (no removals, no behavior change from caller perspective). Existing code using `TextEdit` is unaffected.

**Breaking changes**: None.

**Dup-risk grep** (potential name collisions):
- `TextEdit`: Defined in `crates/perl-lsp-rs-core/src/providers/rename/types.rs`. Re-exported via `crates/perl-lsp-rs-core/src/lib.rs`. Unique within crate; no naming conflicts.
- `rename()`, `scoped_rename()`: Methods on `RenameProvider`; unique within crate.

**Caller count**:
- `rename()`: Called by LSP handler `textDocument/rename` (crates/perl-lsp-rs/src/handlers/rename.rs or equivalent); used in ~3-5 tests
- `scoped_rename()`: Similar; used in ~5-10 tests
- Both are stable public API, used by LSP server and tests

## §Test-Grid

| Scenario | Positive / Negative / Adversarial | Test Name | Invariant Checked |
|---|---|---|---|
| Symbol in both tables at same location | Positive | `test_rename_no_duplicate_edits_for_shared_locations` | No location appears more than once in edits vec |
| Simple single-name rename (3 references) | Positive (regression) | `test_rename_variable` (existing) | Exactly 3 edits, all renamed correctly |
| Function rename (declaration + call) | Positive (regression) | `test_rename_function` (existing) | 2+ edits, all correct |
| Scoped rename with shadowing (outer scope) | Positive (regression) | `test_scoped_rename_shadowed_outer` (existing) | Outer declaration renamed, inner left alone |
| Scoped rename with shadowing (inner scope) | Positive (regression) | `test_scoped_rename_shadowed_inner` (existing) | Inner declaration renamed, outer left alone |
| Deep nesting (50 levels) | Positive (regression) | `test_collect_descendant_scopes_deep_nesting_correctness` (existing) | All references in deep scope renamed, no crash |
| Cycle guard (self-referential scope) | Adversarial | `test_collect_descendant_scopes_cycle_guard` (existing) | Termination guaranteed, cycle detected |
| Linear chain (1000 scopes) | Performance (regression) | `test_collect_descendant_scopes_linear_chain_performance` (existing) | Completes in <10ms (O(n), not O(n²)) |
| Empty edits (no symbol found) | Negative | `test_scoped_rename_no_symbol_at_position` (existing) | Error returned, empty edits vec |
| Invalid new name | Negative | `test_validate_new_name`, `test_scoped_rename_validates_new_name` (existing) | Validation error returned |

## §Blast-Radius

**Consumers** (direct users of `rename()` / `scoped_rename()`):
- LSP handler: `textDocument/rename` protocol handler (crates/perl-lsp-rs/src/handlers/ or similar)
- Tests in `crates/perl-lsp-rs-core/src/providers/rename/mod.rs` (will all pass)

**Downstream crates** (depend on perl-lsp-rs-core):
- `crates/perl-lsp-rs` (LSP server) — uses RenameProvider; will benefit from fix (duplicates removed, correct behavior)
- Tests in `crates/perl-lsp-rs/tests/` (if any LSP-level rename tests) — may see different edits count, but duplicates removed is correct

**Must-not-touch boundaries**:
- `perl_semantic_analyzer::symbol::{SymbolTable, SymbolExtractor}` — we do not modify symbol table generation
- `perl_parser_core` — we do not modify parser or location tracking
- LSP protocol layer — we fix the data structure (RenameResult) but do not change the protocol message format
- No side effects: function signatures and return types unchanged

**Integration risks**: Very low. The fix is internal logic (sort/dedup). No breaking API changes. All existing tests pass. Callers of `rename()` see the same interface, just correct output (duplicates removed).

