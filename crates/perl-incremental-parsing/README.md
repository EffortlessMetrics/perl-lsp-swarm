# perl-incremental-parsing

This unpublished crate is a **compatibility adapter** for the canonical incremental parser API in `perl-parser`.

## Authority

`perl_parser::incremental::{Edit, IncrementalState, apply_edits}` owns implementation behavior, correctness tests, work receipts, and production eligibility. This package does not define a second parser, recovery contract, equivalence denominator, or performance claim.

The checked [`behavior_disposition.json`](behavior_disposition.json) inventory classifies every remaining compatibility test, benchmark, and governed document. New test or benchmark targets fail the compatibility authority contract until they receive an explicit migration or retention disposition.

## Migration

New code should depend on `perl-parser` directly:

```rust
use perl_parser::incremental::{Edit, IncrementalState, apply_edits};
```

Existing imports remain available while the compatibility boundary is retired deliberately:

```rust
use perl_incremental_parsing::incremental::{Edit, IncrementalState, apply_edits};
```

The re-export path resolves to the same canonical Rust types and functions; focused downstream tests prove that identity.

## Historical benchmarks

The benchmark code and reports in this package exercise mixed historical and experimental mechanisms. They are retained temporarily for evidence migration and are **not current shipping-performance authority**. Current regime-specific benchmark work is tracked by #7099 after truthful work receipts in #7072.

## License

Licensed under either [MIT](../../LICENSE-MIT) or [Apache-2.0](../../LICENSE-APACHE) at your option.
