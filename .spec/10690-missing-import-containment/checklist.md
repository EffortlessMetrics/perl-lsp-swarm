# Checklist: #10690 — missing-import containment

Mutations that would restore hard-coded missing-import edits — each must fail
the named guard:

| Mutation | Guard that must fail |
| --- | --- |
| Re-introduce `guess_module_for_function` (or a renamed clone) in any production `src/` path | `no_production_route_references_the_withdrawn_affinity_routes` global symbol ban + every spelling falsifier |
| Restore the enhanced dispatch `import_management::add_missing_imports(...)` in `enhanced/mod.rs` | symbol ban (`add_missing_imports`, `find_undefined_functions`) + `enhanced_provider_never_offers_missing_import_actions` |
| Re-add the PL109 → import extension arm in `diagnostic_routes.rs` | symbol ban (`fix_import_for_bareword_function`) + provider/process "no import action for mapped spellings" assertions |
| Authorize an edit from spelling→module affinity alone (e.g. `dumper` → `use Data::Dumper;`) | per-spelling inertness tests (all ten table entries) |
| Fire on an identically named local sub or already-imported module | collision tests (`sub basename`, pre-imported JSON/Data::Dumper) |
| Stand in refusal with an enabled empty edit / no-op rewrite / disabled stub carrying data | stand-in rejection helpers (provider + process) |
| Insert at byte zero of a multi-package file or into the wrong package | insertion-geometry falsifiers (no returned edit inserts a table-module directive anywhere) |
| Bypass via client-supplied context diagnostics, filtered request, forged resolve, or fabricated command | exact-process stdio negative cases |
| Restore the edit under a compatibility provider | compat placeholder ban (`create_add_missing_imports_action`) + route inventory re-check |

## Verification commands

```bash
cargo fmt -p perl-lsp-rs-core -p perl-lsp-rs -p perllsp -p perl-parser -- --check
cargo clippy -p perl-lsp-rs-core -p perl-lsp-rs -p perllsp --all-targets --locked -- -D warnings
cargo test -p perl-lsp-rs-core --all-targets --locked code_action
cargo test -p perl-lsp-rs --all-targets --locked code_action
cargo test -p perl-lsp-rs-core --test missing_import_withdrawal_containment_tests --locked
cargo test -p perl-lsp-rs-core --test auto_import_quickfix_bdd --locked
cargo test -p perllsp --test lsp_missing_import_withdrawal_process --locked
cargo xtask check-test-wiring
cargo xtask check-support-claims
cargo xtask check-provider-confidence-matrix
git diff --check
```

## Observed verification results (this candidate)

- fmt: clean for all four touched packages.
- `perl-lsp-rs-core` code_action filter: 185 passed, 0 failed. `perl-lsp-rs`
  code_action filter: 145+ passed, 0 failed (one `lsp_3_17_code_actions_tests`
  failure under full-suite shared-host contention reproduced neither in isolation
  nor on binary rerun — environmental).
- New containment suite: 6/6; rewritten BDD suite: 8/8; exact-process fixture:
  4/4 over real `perllsp --stdio`.
- Collateral controls on shared seams: `organize_imports_containment_tests`
  3/3, `pl700_withdrawal_containment_tests` 7/7, `quick_fix_new_codes_bdd`
  17/17 — sibling containments intact.
- Shift-left RED proof recorded against unmodified main@b38568b08: BDD 5/8
  failing (`Import 'Data::Dumper'`, `Import 'File::Basename'`, `Import 'JSON'`,
  `Add missing imports` all returned), containment 5/6 failing including the
  source-scan guard, process fixture 3/4 failing (unfiltered, filtered,
  minimal-client import edits returned over stdio).
- `cargo xtask check-architecture` does not exist on the current xtask CLI
  (same finding as the #11079 packet); the route/architecture guard is the
  in-tree source-scan test plus behavioral falsifiers.
- `cargo xtask check-test-wiring`: exits 1 with 59 unwired test files, none of
  them added or touched by this diff (pre-existing ledger debt).
- `cargo xtask check-support-claims` and
  `cargo xtask check-provider-confidence-matrix`: pass (exit 0).
- `cargo clippy -D warnings` on stable 1.95.0 reports ~186 pre-existing errors
  across untouched files (native-critic/references/lifecycle tests); zero
  findings in any file this diff touches. Repository-wide lint gate ownership
  remains with hosted CI.

## Advertisement verification

Searched current head for surfaces advertising automatic missing-import
insertion: `features.toml` QuickFix description lists variable/pragma/
deprecated-pattern fixes only; capability snapshots, provider contracts, status
tables, VS Code contributions, command reference, and LSP feature overview carry
no add-missing-import claim. The single advertising surface found is
`docs/project/PERL_LSP_VISION.md` ("Add missing import" code action bullet,
whose organize-imports sibling line was already withdrawn by #8305); this PR
rewrites both lines truthfully. This file records that check.

## Shift-left discipline

The behavioral falsifiers (rewritten `auto_import_quickfix_bdd.rs`), the
containment suite with its source-scan guard
(`missing_import_withdrawal_containment_tests.rs`), and the exact-process
fixture were added first and proven failing against unmodified main before the
production deletion landed. The process fixture belongs to the shift-left set
because the defect is production routing, not provider-unit behavior alone.

## Residual observations (out of this claim's scope)

- `perl-parser/src/ide/lsp_compat/code_actions.rs` still pushes enabled
  empty-edit placeholders titled "Remove unused imports" and "Sort imports"
  (SOURCE_ORGANIZE_IMPORTS). These are organize-imports surfaces owned by the
  #8305/#10696 lane, not affinity authority; left untouched. The
  "Add missing imports" sibling placeholder IS this claim's advertisement and is
  removed.
- `perl-parser/src/refactor/import_optimizer.rs` retains its own
  missing-import consolidation inside the unreachable-from-server
  `ImportOptimizer`; organizer territory (#10696).
