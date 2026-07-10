# Editor Intelligence Scorecard

Measures whether each LSP editor feature **returns the right thing** in realistic
workflows. The harness drives the LSP server against a gold corpus of annotated
Perl fixtures and asserts correctness on every request type (hover, goto-definition,
completion, document symbols, diagnostics, and rename).

Full machine-generated metrics (correctness + latency p50/p95 with ratchet baselines):
→ [`docs/project/status/editor_ux.md`](editor_ux.md)

## Correctness snapshot

Values from the last `cargo xtask ux-scorecard` run. All floors are enforced by
`cargo xtask ux-scorecard --ratchet-check`.

| Metric | Value |
|---|---:|
| Hover correctness | 100.00% |
| Completion top-1 relevance | 100.00% |
| Completion top-5 relevance | 100.00% |
| Goto-definition exact hit | 100.00% |
| Document symbols correctness | 100.00% |
| Diagnostics correctness | 100.00% |
| Rename success | 100.00% |
| Cross-file success | 100.00% |

## Latency snapshot (p50 / p95 ms)

| Request class | p50 ms | p95 ms |
|---|---:|---:|
| hover | 24 | 31 |
| completion | 27 | 35 |
| goto-definition | 36 | 44 |
| document symbols | 20 | 28 |
| diagnostics | 53 | 66 |
| workspace symbols | 58 | 70 |

## Gold corpus

Fixtures live in [`test_corpus/gold/`](../../../test_corpus/gold/). Each fixture
directory contains:

| File | Purpose |
|---|---|
| `fixture.pl` | Perl source file under test |
| `expected_hover.json` | Hover correctness assertions (present in 8 fixtures) |
| `expected_goto.json` | Goto-definition exact-hit assertions (present in 3 fixtures) |
| `expected_completion.json` | Completion relevance assertions (present in 4 fixtures) |
| `expected_symbols.json` | Document symbol structure assertions (present in 2 fixtures) |
| `expected.json` | Diagnostics assertions (present in 11 fixtures) |

Total: **33 fixture directories**, **9 of 22 declared scenarios** measured (41% coverage).
Scenario expansion is tracked by [#1426](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/1426).

## Test harness

| Component | Location |
|---|---|
| Integration tests | `crates/perl-lsp-rs/tests/editor_intelligence_scorecard.rs` |
| Fixture loader | `crates/perl-corpus/src/gold.rs` |
| UX scorecard aggregator | `crates/perl-lsp-ux-tests/src/scorecard.rs` |
| xtask emitter | `xtask/src/tasks/ux_scorecard.rs` |
| Ratchet baseline | `.ci/metrics/baselines/editor_ux.json` |

Run the scorecard:

```bash
RUST_TEST_THREADS=2 cargo test -p perl-lsp-rs --test editor_intelligence_scorecard -- --nocapture
cargo xtask ux-scorecard
cargo xtask ux-scorecard --ratchet-check
```

## Assertion kinds

Each `expected_*.json` sidecar uses typed assertion `kind` values. The harness
runs every assertion and fails CI on the first failure; pass rate is printed to
stdout under `--nocapture`.

| Kind | Meaning |
|---|---|
| `hover_non_null` | Hover must return non-empty content |
| `hover_contains` | Hover content must include `needle` |
| `hover_absent` | Hover content must NOT include `needle` |
| `hover_null` | Hover must return null (no hover at this position) |
| `goto_non_null` | Goto-definition must return a location |
| `goto_line` | Goto-definition must resolve to `expected_line` |
| `goto_null` | Goto-definition must return null (intentional non-resolution) |
| `completion_non_empty` | Completion list must not be empty |
| `completion_present` | `expected_label` must appear in results |
| `completion_top1` | `expected_label` must be first |
| `completion_top5` | `expected_label` must be in top 5 |
| `completion_noise_absent` | `forbidden_label` must not appear |
| `symbols_non_empty` | Document symbols response must not be empty |
| `symbols_count_at_least` | Response must have at least `min` symbols |
| `symbols_contains_name` | `expected_name` must appear in results |

## Ratchet policy

Regression-only ratchet: correctness floors may improve or stay flat; any
statistically meaningful regression causes `cargo xtask ux-scorecard --ratchet-check`
to fail with a receipt. This is enforced as a required CI check.
