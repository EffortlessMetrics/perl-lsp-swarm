# #10690 — Missing-import containment context

Train slot ICC-00B (immediate containment). Parent programme #8277; replacement
owners #790/#8948. Evidence pin: current `origin/main` at branch time.

## Ruling

Hard-coded function→module affinity (or PL109 presentation) != candidate identity
!= import edit authorization. Every hard-coded missing-import edit is withdrawn
from every production surface until the exact candidate planner (#790/#8948)
lands.

## Current affinity routes (re-inventoried on this branch)

1. **Route A — enhanced global action**
   `EnhancedCodeActionsProvider::get_global_refactorings`
   → `providers/code_actions/enhanced/import_management.rs::add_missing_imports`
   → `find_undefined_functions` (HashSet scan + table-based suppression)
   → `import_management::guess_module_for_function` (7-entry hard-coded table)
   → `TextEditHelpers::find_import_insert_position` (package-blind preamble line
   scan — inserts before a leading `package`, i.e. into `main`)
   → one enabled `use <module>;` insertion edit titled "Add missing imports".

2. **Route B — PL109 diagnostic quick fix**
   `DiagnosticCode::UnquotedBareword` arm in
   `providers/code_actions/diagnostic_routes.rs`
   → `quick_fixes::fix_import_for_bareword_function`
   → `guess_module_for_function` + `import_block_end` (line-oriented preamble
   scan) → enabled "Import '<module>'" edit.

3. **Parser-compat placeholder** (`perl-parser/src/ide/lsp_compat/code_actions.rs`):
   `provide_import_actions` pushed an *enabled* "Add missing imports" action with
   an empty default `WorkspaceEdit`. Not wired into the perllsp server (proven by
   caller search), but it is exactly the enabled no-op the ruling forbids and a
   re-wiring hazard; deleted.

## Table disposition

`guess_module_for_function` had no non-authoritative consumer (completion uses
the separate #11158 authority). Deleted outright per the issue's preferred order.
The sibling organize-imports sorter residue (`collect_imports`/`sort_imports`/
`find_imports_range`) remains owned by #8305's containment guard, not this claim.

## Withdrawn product claim

Automatic missing-import insertion is not offered on any surface: direct,
filtered (`quickfix`, `source.fixAll` aggregate), resolve, command, enhanced,
original, parser-compat, completion. Root `features.toml`'s `lsp.code_action`
row never advertised it and must not start to. Capability snapshots pin the
advertised kinds.

## Preserved behavior

PL109 keeps its independently justified fixes: quote with single quotes, quote
with double quotes, declare-as-filehandle for uppercase barewords. All other
diagnostic/action families are untouched. Generic `quickfix` capability stays.

## Restoration

Only #790/#8948 may restore exact unresolved-subject selection, exporter proof,
and package-aware insertion planning.
