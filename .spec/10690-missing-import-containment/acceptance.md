# #10690 — Acceptance criteria

| ID | Criterion | Negative fixture / proof |
|----|-----------|--------------------------|
| MISS-001 | Enhanced global route produces no hard-coded import edit | `package App; dumper($x);` → `get_enhanced_refactoring_actions` returns no "Add missing imports" action and no edit whose text inserts `use Data::Dumper;` (core containment test) |
| MISS-002 | PL109 produces no import edit while unrelated PL109 fixes survive | PL109 diagnostic over `dumper` → no "Import '…'" action, no `use Data::Dumper;` edit, quote fixes still present (core + exact-process tests) |
| MISS-003 | `guess_module_for_function` has no production edit/command/completion consumer | Symbol deleted; source scan fails if the name reappears under any `crates/*/src` |
| MISS-004 | Compatibility/original/direct/filtered/resolve routes fail closed | Compat placeholder deleted; exact-process tests inspect the complete returned action set (including edits) for unfiltered, `quickfix`, and `source.fixAll` requests; resolve only reconstructs pragma edits from `data.pragma`, which withdrawn actions never carry |
| MISS-005 | Exact perllsp process returns no affinity-derived edit | `lsp_missing_import_containment_tests.rs`: didOpen package-first `dumper($value)` document, full action-set inspection incl. `newText`; PL109 context-diagnostic request likewise |
| MISS-006 | No enabled empty/no-op action represents withdrawal | Withdrawal is omission; compat "Add missing imports" empty-edit placeholder deleted; scan forbids `create_add_missing_imports_action` |
| MISS-007 | Product claims say nothing about automatic missing-import insertion | Guard asserts root `features.toml` `lsp.code_action` description does not advertise it |
| MISS-008 | Route validator prevents both bypasses and new table reachability | Source-scan guard needles: `guess_module_for_function`, `add_missing_imports`, `find_undefined_functions`, `fix_import_for_bareword_function`, `create_add_missing_imports_action` under `crates/*/src`; `get_known_module_exports` pinned to its single inventoried home; `find_import_insert_position` guarded by invocation-shape detection that exempts only the retained declaration |
| MISS-009 | #790/#8948 remain sole restoration owners | Recorded here and in PR body; withdrawal comments name them |

## Mutation contract

Restoring either route must fail:

- re-dispatching `add_missing_imports` from `get_global_refactorings` (or
  re-creating the symbol anywhere under `crates/*/src`) → scan guard red;
- restoring the PL109 `fix_import_for_bareword_function` arm → MISS-002 red;
- reintroducing the preamble import-insertion helper as production authority →
  invocation-shape guard red (any non-declaration use of
  `find_import_insert_position` under `crates/*/src`).
