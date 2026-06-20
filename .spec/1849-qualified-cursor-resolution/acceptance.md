# Acceptance Criteria: Issue #1849 — Qualified-Name Cursor Position Resolution

## §Behavior

| Input | Condition | Expected Result |
|-------|-----------|-----------------|
| Cursor on package prefix `My` in `My::Utils::process()` | go-to-definition request | Return `null` (module definitions not yet supported) |
| Cursor on middle package `Utils` in `My::Utils::process()` | go-to-definition request | Return `null` (module definitions not yet supported) |
| Cursor on final component `process` in `My::Utils::process()` | go-to-definition request | Navigate to `sub process` definition inside `My::Utils` package |
| Cursor on package prefix `My` in `My::Utils::process()` | find-references request | Return empty list (module references not yet supported) |
| Cursor on middle package `Utils` in `My::Utils::process()` | find-references request | Return empty list (module references not yet supported) |
| Cursor on final component `process` in `My::Utils::process()` | find-references request | Return all call sites of `process()` in `My::Utils` package |
| Cursor exactly on `::` delimiter between components | go-to-definition request | Return `null` (not a valid symbol position) |
| Single-component unqualified call `process()` | go-to-definition request | Continue to work as before (no regression) |
| Qualified name in string interpolation `"$obj->$pkg::method()"` | go-to-definition request | Handle gracefully (return `null` or best-effort result) |

---

## §Hazards

| Hazard Class | Surface | Scenario | Mitigation | Test Name |
|---|---|---|---|---|
| **LSP-1: Off-by-one character positions** | `find_component_at_cursor()` offset calculation | Cursor at byte boundary between two components or within multi-byte UTF-8 sequence | Comprehensive unit tests of boundary cases; test with 1-byte, 2-byte, and 4-byte UTF-8 characters | `test_cursor_position_boundaries`, `test_utf8_qualified_names` |
| **LSP-2: Regex match span interpretation** | navigation.rs & references.rs cursor position check | Cursor position `cursor_in_text` is relative to window start, not line start; off-by-one in match span calculation | Verify `cursor_offset_in_match = cursor_in_text - m.start()` is correct; test with various text window sizes | `test_text_window_offset_calculation` |
| **LSP-3: Incomplete component branch coverage** | All three code paths in component-aware branching | Earlier component path returns `null` instead of querying workspace index; later enhancement may break if not structured for future module lookup | Design return path to be extensible (not hard-coded to only return null); document as "module lookup deferred to future PR" | `test_earlier_component_returns_null` |
| **LSP-4: Regex pattern mismatch** | `get_fqn_regex()` in navigation.rs and `get_qualified_name_regex()` in references.rs | Regex definition does not match all valid Perl qualified names; may skip certain valid patterns | Verify both regexes match the same patterns; add adversarial tests for edge-case module names | `test_regex_pattern_coverage` |
| **LSP-5: Off-by-one in test string positions** | `test_def_package_qualified_call_cursor_positions()` | Character positions in test assertions do not align with actual string layout | Count characters manually and verify with string indexing; add comments documenting the positions | `test_string_position_alignment` |
| **LSP-6: Scope and state pollution** | References caching and document state | Find-references uses cached regexes and workspace index; changes to one document affect another | Ensure tests use isolated documents and fresh harness instances; verify no cross-document state bleed | `test_isolated_document_state` |

---

## §Contracts

### LSP Protocol

- **Request**: `textDocument/definition` (LSP 3.17 §textDocument_definition)
  - Input: `TextDocumentPositionParams` (uri, position with line + character)
  - Output: `Location | Location[] | null`
  - Contract touched: The position-to-definition resolution now correctly handles multi-component qualified names

- **Request**: `textDocument/references` (LSP 3.17 §textDocument_references)
  - Input: `ReferenceParams` (uri, position, includeDeclaration)
  - Output: `Location[] | null`
  - Contract touched: The position-to-references resolution now correctly branches on component position

### Parser Contracts

- **PARSER_CONTRACTS.md**: Qualified Name Classification
  - Perl qualified names (`Package::Name`) are matched by regex post-parse, not during parse
  - The regex pattern used must handle arbitrary nesting depth (`A::B::C::D::...`)
  - Regex must not match partial constructs like `::trailing` or `leading::`
  - No AST node changes required; fix operates on regex matches only

### Module Resolution (references.rs)

- **Symbol lookup**: `lookup_workspace_definition()` and `index.find_refs()`
  - These functions are called only when cursor is on the final component (behavior unchanged)
  - No modifications to symbol key structure or index query interface

---

## §API-Shape

### New Public API

| Item | Type | Visibility | Purpose |
|---|---|---|---|
| `find_component_at_cursor()` | Function | pub (in navigation module) | Calculate which `::` -separated component contains the cursor position |

### Signature

```rust
/// Given a fully-qualified name string and cursor byte position within that string,
/// determine which `::` -separated component the cursor is in.
///
/// # Arguments
/// * `fqn` - Fully-qualified name string (e.g. "My::Utils::process")
/// * `cursor_offset` - Cursor byte position relative to fqn start (e.g. 0 for 'M', 3 for 'U')
///
/// # Returns
/// * `Some((component_index, component_name))` if cursor is within a component
///   where component_index is 0-based (0 = first part before first ::)
/// * `None` if cursor is exactly on a :: delimiter, past string end, or invalid
pub fn find_component_at_cursor(fqn: &str, cursor_offset: usize) -> Option<(usize, &str)>
```

### No Changes to Public API

- `goto_definition_workspace()` — input/output contracts unchanged
- `goto_references()` — input/output contracts unchanged
- All changes are internal (conditional branching on component index)

### Dup-Risk (Grep for reuse opportunities)

```bash
grep -r "split.*::" crates/perl-lsp-rs/src --include="*.rs"
# Result: Many locations split qualified names (completion.rs, workspace_rename.rs, etc.)
# Risk: If similar off-by-one bugs exist elsewhere, they should also be fixed
# Mitigation: This PR focuses only on navigation/references; other locations can be addressed in follow-up PRs
```

### Caller Count (Estimator)

- **Direct callers of find_component_at_cursor()**: 2 (navigation.rs + references.rs)
- **Indirect callers (go-to-definition users)**: Unbounded (LSP client calls)
- **Indirect callers (find-references users)**: Unbounded (LSP client calls)

---

## §Test-Grid

### Positive Cases

| Input | Expectation | Test Name | Invariant |
|---|---|---|---|
| Cursor on `process` in `My::Utils::process()` with `sub process` defined in `My::Utils` | Navigate to line with `sub process` definition | `test_def_package_qualified_call_cursor_positions` (case 3) | Final component always resolves to function definition (existing behavior) |
| Cursor on `bar` in `Foo::bar()` with `sub bar` defined in `Foo` package | Navigate to `sub bar` definition in `Foo` package | Existing test `test_def_package_qualified_call` | No regression; existing behavior preserved |
| Unqualified call `process()` in same package | Navigate to `sub process` declaration | Existing test suite | Single-component names work as before (no regression) |
| Find-references on `process` in `My::Utils::process()` | Return all call sites of `process` in `My::Utils` package | New test `test_refs_qualified_name_final_component` | Final component find-refs behaves as before |

### Negative Cases

| Input | Expectation | Test Name | Invariant |
|---|---|---|---|
| Cursor on `My` in `My::Utils::process()` | Return `null` (no module definition support) | `test_def_package_qualified_call_cursor_positions` (case 1) | Earlier component must NOT resolve to final component function |
| Cursor on `Utils` in `My::Utils::process()` | Return `null` (no module definition support) | `test_def_package_qualified_call_cursor_positions` (case 2) | Middle component must NOT resolve to final component function |
| Cursor exactly on `::` delimiter | Return `null` | `test_cursor_on_delimiter` | Delimiter position must not crash or return incorrect result |
| Find-references on `My` in `My::Utils::process()` | Return empty list | New test `test_refs_qualified_name_earlier_component` | Earlier component find-refs must NOT find function call sites |
| Malformed qualified name `Foo:::bar` (triple colon) | Handled gracefully (return null or best effort) | `test_malformed_qualified_name` | Parser should not crash on unusual input |

### Adversarial Cases

| Input | Expectation | Test Name | Invariant |
|---|---|---|---|
| Very deep nesting `A::B::C::D::E::F::G::process()`, cursor on `C` | Return `null` | `test_deep_nesting_middle_component` | Component calculation must work for arbitrary depth |
| Qualified name with non-ASCII module names (if Perl allows) `Модуль::func()` | Handle correctly with UTF-8 aware offset | `test_utf8_module_names` | Byte offset calculation must handle multi-byte characters |
| Qualified name followed immediately by `::` in code `Foo::Bar::baz() :: next_statement` | Correctly identify match boundary and not include the trailing `::`| `test_match_boundary_precision` | Regex match span must not extend past the qualified name |
| Cursor at end of qualified name string `My::Utils::process()`, character after last `)`| Return `null` (out of bounds) | `test_cursor_out_of_bounds` | Must not panic or return incorrect component |
| Qualified name inside a string literal `"My::Utils::process"` | Either handle correctly or return `null` (not critical) | `test_qualified_name_in_string_context` | Must handle context gracefully (string content is not code) |

### State Transition Cases

| Before State | Transition | After State | Test Name | Invariant |
|---|---|---|---|---|
| Multiple documents open, first document has `My::Utils::process()` | Cursor moves from first to second document | State is isolated; query on second document unaffected by first | `test_multi_document_state_isolation` | Find-references must not bleed state between documents |
| Document with unqualified call `process()` | Edit to add package prefix: `Foo::process()` | Go-to-definition on edited call resolves correctly | `test_edit_to_qualified_name` | Component tracking must adapt to changed source |
| Workspace index contains outdated symbol entries | Query go-to-definition on qualified name | Returns result from current document or latest index | `test_stale_index_fallback` | Must not return stale definition from old index snapshot |

---

## §Blast-Radius

### Consumers

| Consumer | How affected | Risk level | Mitigations |
|---|---|---|---|
| LSP clients (VSCode, Vim, Emacs, etc.) via textDocument/definition request | Users will now see correct navigation when clicking on package prefixes (previously incorrect) | Low — strictly an improvement; no new failures expected | Regression tests ensure final component behavior unchanged |
| LSP clients via textDocument/references request | Users will see correct reference results when querying package prefixes | Low — strictly an improvement; no new failures expected | Regression tests ensure final component behavior unchanged |
| IDE integration tests depending on definition results | Tests that hardcoded assumptions about "cursor on qualified name always resolves to final component" may now fail | Medium — test updates required | Identify all such tests and update assertions |

### Downstream Crates

| Crate | How affected | Risk level | Mitigations |
|---|---|---|---|
| perl-lsp-rs (binary) | Internal change only; no public API change | Low | All consumers of go-to-definition and find-references use LSP protocol (no internal API change) |
| perl-lsp-rs-core (if it exists) | May provide shared utilities; check if `find_component_at_cursor()` belongs there | Low | If extracted, re-export from navigation.rs for compatibility |
| Test infrastructure (LspHarness, etc.) | No changes required | None | Existing harness works with new behavior |

### Must-Not-Touch Boundaries

| Boundary | Why off-limits | Verification |
|---|---|---|
| **Parser AST structure** | This fix operates entirely on regex matches post-parse; no AST changes | Grep for new/modified AST node types should return empty |
| **Workspace index schema** | No new symbol keys or index structures; only conditional branching | Grep for `SymbolKey` modifications should return empty |
| **LSP protocol definitions** | No new request types or response shapes; only behavior change | Inspect generated protocol types (`lsp_types.rs`); no diff |
| **Module resolution algorithm** | Final-component lookups unchanged; earlier components return null (not queried) | Existing workspace_index queries same number of times (only earlier component branch skips) |

---

## Summary

This fix strictly improves cursor position resolution in qualified names without expanding the LSP protocol, changing the parser, or modifying the workspace index. The primary risk is off-by-one errors in the new component-offset calculation; mitigated by comprehensive boundary-case tests. Final-component behavior is identical to current code, ensuring no regression on existing tests.

All test additions use standard regression test patterns and isolated document harnesses. No changes to shared state, caching, or module-level initialization.
