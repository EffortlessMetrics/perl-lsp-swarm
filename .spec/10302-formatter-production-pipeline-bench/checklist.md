# Checklist: #10302 production-path formatter performance receipt

Base pin: `origin/main@e6e956461534b2566c735696f289e0915e2cb189` (2026-08-27).
Composition sibling: #10301 fuzz/property spec
(`.spec/10301-formatter-property-fuzz-harness/`) — disjoint file set.

Red-first receipts-of-record (2026-08-27, pure reads on the base pin):

- Zero formatter bench surface: `BENCH_TARGETS` in
  `.github/workflows/ci-nightly.yml` enumerates exactly 14 targets
  (perl-workspace, perl-token, perl-symbol, perl-pragma, perl-parser ×6,
  perl-lsp-rs rope_performance_benchmark, perl-lexer, perl-dap,
  perl-incremental-parsing); `crates/perl-lsp-perltidy/Cargo.toml` has no
  criterion dev-dep and no `[[bench]]`.
- Zero instrumentation on the pipeline: tree-wide grep for
  counters/metrics/stats over `crates/perl-lsp-perltidy/**` returns nothing;
  nearest deterministic surfaces are `FormatChangeSummary`,
  `FormatIdentity` digests, and depth tracking at
  `implementation.rs:606`.
- Production seams that must stay single-invocation:
  `perl-lsp-rs-core/src/providers/formatting/formatting.rs`
  `native_document_decision` definition (:249; typed call :257) /
  `native_range_decision` definition (:262; typed call :273). A successful
  request invokes the provider/native pipeline once but the validation parse
  gate twice: once for source and once for formatted output.
- Integrity guards to consume verbatim: stale `target/criterion/` deletion +
  explicit per-target invocation + superset contract over cargo-metadata
  bench-kind targets (#3979), fixture tests
  `benchmarks/scripts/test_extract_criterion.py` /
  `test_benchmark_guards.py`; baselines `benchmarks/baselines/v*.json`;
  comparison/alerts advisory by deliberate policy (#3979/#5282).

Planned surface:

- [ ] `crates/perl-lsp-perltidy/src/native/counters.rs`: additive
      `NativePipelineCounters` (schema v1) + optional collector plumbed
      through the typed call path only; zero behavior when unset; exact
      `pipeline_invocations`, aggregate `parse_gate_invocations`,
      `source_parse_gate_invocations`, and
      `formatted_output_parse_gate_invocations` counters prove successful-path
      one-pipeline/two-parse attribution and disposition-specific early-refusal
      counts (zero before parse; source == 1/output == 0 on source refusal;
      source == 1/output == 1 on formatted-output refusal)
- [ ] `crates/perl-lsp-perltidy/Cargo.toml`: additive criterion dev-dep +
      `[[bench]] name = "native_pipeline_benchmark" harness = false`
- [ ] `crates/perl-lsp-perltidy/benches/native_pipeline_benchmark.rs` +
      `benches/support/perf_subjects.rs`: checked-in scaling cohort
      (small/medium/large × delimited/statement/opaque/refusal/no-change ×
      LF/CRLF/bare-CR × tabs/spaces/width) with exact subject identity
      and an actual `benchmark_group("native_pipeline")` /
      `bench_function("document_small", ...)` pair producing representative
      Criterion ID `native_pipeline/document_small` (not a direct benchmark
      string containing `/`, which Criterion sanitizes to `_`)
- [ ] `crates/perl-lsp-perltidy/tests/native_pipeline_counters_tests.rs`:
      NPC-001..NPC-006 + NPC-010 pipeline/parse canaries incl. detector sanity
      control and the early-refusal disposition table
- [ ] `crates/perl-lsp-rs-core/tests/native_pipeline_invocation_tests.rs`:
      drive public `format_document_decision` / `format_range_decision` with
      shared counters and prove exactly one private `format_document_typed` /
      `format_range_typed` call per request; do not claim this adapter property
      from perltidy-only tests
- [ ] `.github/workflows/ci-nightly.yml`: one `BENCH_TARGETS` entry
      `"perl-lsp-perltidy:native_pipeline_benchmark:"`; target identity is
      `perl-lsp-perltidy:native_pipeline_benchmark`, and the trailing
      delimiter encodes only the empty required-feature field
- [ ] Strict nightly extraction includes the exact matching
      `--expect-id "native_pipeline/document_small"`
- [ ] `benchmarks/scripts/test_extract_criterion.py` includes the exact grouped
      Criterion fixture layout
      `native_pipeline/document_small/new/estimates.json`; the representative
      ID proves target execution only, not full-matrix execution
- [ ] Select and implement the receipt-identity path: a checked-in subject
      identity sidecar keyed by canonical Criterion ID plus a fail-closed join
      in `extract-criterion.py`; extend the receipt/formatter path so every
      formatter row preserves content digest, config fingerprint, engine, and
      the current toolchain/environment tag. Fixtures reject missing,
      duplicate, stale, and unmatched identities. NPC-008 remains
      `NOT_PROVEN` until this executable path passes end-to-end
- [ ] NPC-007..NPC-009 structural pins inside the same test file

Proof commands:

```bash
cargo fmt -p perl-lsp-perltidy -- --check
cargo clippy -p perl-lsp-perltidy --all-targets --locked -- -D warnings
cargo test -p perl-lsp-perltidy --all-targets --locked
cargo test -p perl-lsp-perltidy --test native_pipeline_counters_tests -- --test-threads=1
cargo test -p perl-lsp-rs-core --test native_pipeline_invocation_tests --locked
cargo bench -p perl-lsp-perltidy --bench native_pipeline_benchmark -- --quick
python3 benchmarks/scripts/test_extract_criterion.py -v
python3 benchmarks/scripts/test_benchmark_guards.py -v
```

Open residuals (owned by upstream issues, not silently dropped):

- [ ] Enroll #9327 representative corpus identities through the subject
      registry seam when that corpus lands
- [ ] Align derived-output envelopes with #7140/#7501 product bounds when they
      codify limits (schema-major bump + before/after receipts); allocation
      remains a separate real-oracle requirement
- [ ] Keep #10302's allocation-count/allocated-byte requirement open and
      `NOT_PROVEN` until a real allocation oracle measures both on a supported
      proof run; derived output/replacement/retained bytes never substitute.
      The oracle proof must include controlled mutant
      `allocation_oracle_rejects_extra_temporary_copy`: add one temporary
      allocation/copy to the measured production path, demonstrate the
      allocation canary turns red because count/bytes increase, then revert
      the mutant and retain both receipts
- [ ] Cross-environment timing comparisons remain out of scope permanently
      unless a reviewed policy changes the advisory posture (#3979/#5282)

## EXPLICIT-HUMAN gate

Promoting NPC-003/NPC-004 counter canaries from package-scoped proof to a
required merge-blocking check for all formatter-touching PRs changes
integration posture beyond this lane's authority. Default delivered here:
they block within `-p perl-lsp-perltidy` proof runs; making them required
organization-wide is left as an explicit human decision.
