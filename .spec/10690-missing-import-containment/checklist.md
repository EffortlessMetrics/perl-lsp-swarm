# #10690 — Checklist

| Acceptance | Source anchor (post-change state) | Negative fixture | Command | Review lens |
|------------|-----------------------------------|------------------|---------|-------------|
| MISS-001 | `enhanced/mod.rs` `get_global_refactorings` withdrawal comment | package-first `dumper($x)` enhanced request | `cargo test -p perl-lsp-rs-core --all-targets --locked missing_import_containment` | Route |
| MISS-002 | `diagnostic_routes.rs` PL109 arm keeps only `fix_bareword` | PL109 diag over `dumper` | same | Collateral (quote/filehandle survive) |
| MISS-003/008 | deleted: table, both routes, `find_undefined_functions`, `find_import_insert_position`, compat placeholder | reintroduce any needle under `crates/*/src` | same (`no_production_route_references_the_withdrawn_import_authority`) | Architecture recurrence |
| MISS-004/005 | `crates/perl-lsp-rs/tests/lsp_missing_import_containment_tests.rs` | unfiltered + `quickfix` + `source.fixAll` exact-process requests; full edit inspection | `cargo test -p perl-lsp-rs --test lsp_missing_import_containment_tests --locked` | Exact-process routing |
| MISS-006 | withdrawal = omission; no enabled empty action anywhere for affinity candidates | compat placeholder deletion | core containment tests | Claim honesty |
| MISS-007 | root `features.toml` `lsp.code_action` description | drift test asserting no automatic missing-import advertisement | same as MISS-001 | Claims/docs |

## Dispositions

- `guess_module_for_function` — **deleted** (no non-authoritative consumer).
- `add_missing_imports` + `find_undefined_functions` (+ unit tests) — **deleted**.
- `enhanced::import_management` module — **deleted** with its mod declaration.
- `quick_fixes::fix_import_for_bareword_function` + `import_block_end` — **deleted**.
- `TextEditHelpers::find_import_insert_position` — **deleted** (only consumer was
  Route A); pragma insertion helper retained.
- Parser-compat "Add missing imports" empty-edit placeholder — **deleted**.
- Organize-imports sorter residue (`collect_imports`/`sort_imports`/
  `find_imports_range`) — untouched; owned by #8305's guard.
- Completion auto-import — separate #11158 authority; verified non-reuse of this
  table.

## Known mutations that must stay red

1. Re-adding `guess_module_for_function` or either route symbol under
   `crates/*/src`.
2. Restoring the PL109 import arm (MISS-002 fails).
3. Advertising automatic missing-import insertion in the feature catalog row.
