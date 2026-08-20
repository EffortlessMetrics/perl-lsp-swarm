# Historical Incremental Parsing Benchmark Report

## Status

This document is retained as **historical, non-authoritative evidence** for experimental incremental algorithms previously exposed through `perl-incremental-parsing`.

It must not be used to claim current production reuse, avoided parser work, editor latency, or supported fallback rates. Several historical mechanisms measured token retention, cache matches, cloning, or analysis after a full parse under the shared word “incremental.” Those are different work regimes.

The current authority chain is:

- canonical implementation: `perl_parser::incremental`;
- strategy and work receipts: #7072;
- correctness comparison: #7045 and #2327;
- regime-specific benchmarks: #7099;
- unsupported-claim cleanup: #7081.

## Retained benchmark target

`benches/incremental_parsing_benchmarks.rs` remains temporarily available for migration. The checked `behavior_disposition.json` classifies it as `historical_mixed_incremental` with disposition `quarantine_and_rewrite`.

Before any result from that target can become current evidence, each benchmark must be rewritten into one explicit regime:

```text
cold_fresh_parse
warm_full_parse
full_incremental_fallback
checkpoint_to_eof
checkpoint_to_exact_token_sync
bounded_ast_leaf_patch
analytical_similarity_only
validation_oracle_only
```

Every sample must bind the implementation, commit, toolchain, configuration, fixture, edit class, actual parser invocations, fallback reason, and production-versus-oracle work. A regime that is not implemented is reported as `not_implemented`; another algorithm may not stand in for it.

## Historical expectations are not current claims

Prior versions of this document listed expected percentages, universal millisecond targets, cache-efficiency thresholds, and analogies to other parsers. Those values were hypotheses, not same-strategy receipts from the shipping path. They have been removed from the current report surface.

Historical Git revisions retain the original planning document when provenance research is needed. Current README, package metadata, badges, scorecards, or release claims must cite the future #7099 report rather than reconstructing those numbers.

## Running the retained comparison target

```bash
cargo bench -p perl-incremental-parsing
```

Running it proves only that the historical comparison target executes. It does not promote its metrics or names. Review the target against #7072/#7099 before interpreting results.

## Migration rule

Unique correctness cases move to their canonical owners before duplicate compatibility tests or benchmark helpers are removed. Performance-only assertions that cannot distinguish full parsing, comparison, cloning, validation, and actual retained work are rewritten or retired rather than preserved for test-count continuity.
