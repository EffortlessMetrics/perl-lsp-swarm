# LSP round-trip receipt versions

`lsp_roundtrip.json` is an advisory measurement from an actual benchmark run,
not a performance gate. Its `schema_version` determines the units of every lane:

| Version | Percentile fields | Unit |
| --- | --- | --- |
| 1 | `p50_ms`, `p95_ms`, `p99_ms` | Whole milliseconds, truncated before aggregation |
| 2 | `p50_us`, `p95_us`, `p99_us` | Whole microseconds, truncated before aggregation |

Version 2 is emitted by
`crates/perl-lsp-rs/benches/lsp_stdio_roundtrip_benchmark.rs`. It applies uniformly
to initialize, didOpen-to-diagnostics, and every warm request lane. Sample counts,
metadata, and the rounded-index percentile convention are unchanged. Printed
custom summaries use `us`. Criterion measures its groups independently; its
`target/criterion` timing history is not this custom receipt format.

Consumers must dispatch on `schema_version`, select the corresponding field
suffix, and reject unsupported versions rather than treating missing fields as
zero. For comparisons in microseconds, version 1 integers can be multiplied by
1,000, but this only rescales the old truncated observations: it cannot recover
sub-millisecond precision. Keep the version/provenance visible when comparing.
No in-repository reader or standalone schema for this custom receipt is present;
other metric receipts with `_ms` fields retain their own contracts.

The committed version 1 receipt (measured 2026-10-04 at commit `23ba886442`) is
historical advisory data. Its zeroes do not establish zero latency. It is retained
unchanged; a version 2 replacement requires a real resource-admitted benchmark
execution against the server binary, with genuine runtime/commit provenance.
Do not rename its fields, relabel its version, or synthesize new measurements.

The deterministic contract tests in
`crates/perl-lsp-rs/tests/lsp_roundtrip_receipt.rs` use synthetic durations and the
same pure conversion, summary, and serialization code as the benchmark. They do
not measure wallclock performance or regenerate the historical receipt.
