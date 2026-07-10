# Editor Intelligence Scorecard

Tracks correctness and latency for the LSP editor-intelligence features: hover, completion, goto-definition, document symbols, and rename.

Auto-generated detail with latency tables: [editor_ux.md](editor_ux.md)  
Gold corpus fixtures: `test_corpus/gold/` (33 fixture dirs)  
Test harness: `crates/perl-lsp-rs/tests/editor_intelligence_scorecard.rs`  
Run command: `RUST_TEST_THREADS=2 cargo test -p perl-lsp-rs --test editor_intelligence_scorecard`

Last measured: `2026-04-26T01:27:25Z` · Scenarios: 9 of 22 declared (41% fixture coverage)

## Correctness

| Feature | Metric | Result |
|---|---|---:|
| Completion | Top-1 accuracy | 100.00% |
| Completion | Top-5 accuracy | 100.00% |
| Cross-file | Navigation success | 100.00% |
| Goto-definition | Exact-hit rate | 100.00% |
| Diagnostics | Correctness | 100.00% |
| Hover | Correctness | 100.00% |
| Rename | Success rate | 100.00% |
| Document symbols | Correctness | 100.00% |

## Latency (ms, warm path)

| Request class | p50 | p95 |
|---|---:|---:|
| completion | 27 | 35 |
| definition | 36 | 44 |
| diagnostics | 53 | 66 |
| document_symbols | 20 | 28 |
| hover | 24 | 31 |
| workspace_symbols | 58 | 70 |

Ratchet policy: floor metrics may improve or stay flat; regressions fail `cargo xtask ux-scorecard --ratchet-check`. Baselines in `.ci/metrics/baselines/editor_ux.json`.

## Gold Corpus Coverage

| Fixture area | Fixtures |
|---|---|
| Hover | hover_scalar_var, hover_array_var, hover_hash_var, hover_builtin_func, hover_package, hover_sub_def, hover_use_constant, hover_imported_func |
| Completion | completion_method_arrow, completion_lexical_var, completion_builtin, completion_package_colon |
| Goto-definition | goto_inherited_method, goto_local_sub, goto_oop_method |
| Diagnostics | strict_diagnostics (via UX scenarios) |
| Document symbols | document_symbols_core (via UX scenarios) |
| Rename | rename_workflow_core (via UX scenarios) |

## Infrastructure

The scorecard reuses the existing LSP test harness (`LspServer`, `send_request`, `drain_until_quiet`). Sidecar JSON files (`expected_hover.json`, `expected_goto.json`, `expected_completion.json`) alongside each `test_corpus/gold/<fixture>/fixture.pl` drive the assertions. The loader lives in `crates/perl-corpus/src/gold.rs`.

## Open Work

- Fixture coverage: 9/22 declared scenarios measured (41%). Adding fixtures for inherited-method resolution, cross-file rename, OOP completion context, and Moose/Moo-specific completions will raise coverage.
- `sort_text` is not yet wired in completion.rs — Top-1 accuracy reflects BFS traversal order, not scored ranking.
- Module resolution blockers (issue #7570) affect 11 `@INC` conformance scenarios; tracked in `editor_ux.json`.
