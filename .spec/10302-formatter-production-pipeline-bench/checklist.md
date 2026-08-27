# Checklist: #10302 production-path formatter performance receipt

Base pin: `origin/main@e6e956461534b2566c735696f289e0915e2cb189` (2026-08-27).
Composition sibling: #10301 fuzz/property spec
(`.spec/10301-formatter-property-fuzz-harness/`) — disjoint file set.

Red-first receipts-of-record (2026-08-27, pure reads on the base pin):

- Zero formatter bench surface: `BENCH_TARGETS` in
  `.github/workflows/ci-nightly.yml` enumerates exactly 14 targets
  (perl-workspace, perl-token, perl-symbol, perl-pragma, perl-parser ×5,
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
  `native_document_decision` (:257) / `native_range_decision` (:273).
- Integrity guards to consume verbatim: stale `target/criterion/` deletion +
  explicit per-target invocation + superset contract over cargo-metadata
  bench-kind targets (#3979), fixture tests
  `benchmarks/scripts/test_extract_criterion.py` /
  `test_benchmark_guards.py`; baselines `benchmarks/baselines/v*.json`;
  comparison/alerts advisory by deliberate policy (#3979/#5282).

Planned surface:

- [ ] `crates/perl-lsp-perltidy/src/native/counters.rs`: additive
      `NativePipelineCounters` (schema v1) + optional collector plumbed
      through the typed call path only; zero behavior when unset
- [ ] `crates/perl-lsp-perltidy/Cargo.toml`: additive criterion dev-dep +
      `[[bench]] name = "native_pipeline_benchmark" harness = false`
- [ ] `crates/perl-lsp-perltidy/benches/native_pipeline_benchmark.rs` +
      `benches/support/perf_subjects.rs`: checked-in scaling cohort
      (small/medium/large × delimited/statement/opaque/refusal/no-change ×
      LF/CRLF/bare-CR × tabs/spaces/width) with exact subject identity
- [ ] `crates/perl-lsp-perltidy/tests/native_pipeline_counters_tests.rs`:
      NPC-001..NPC-006 + NPC-010 canaries incl. detector sanity control
- [ ] `.github/workflows/ci-nightly.yml`: one `BENCH_TARGETS` entry
      `"perl-lsp-perltidy:native_pipeline_benchmark:"`
- [ ] NPC-007..NPC-009 structural pins inside the same test file

Proof commands:

```bash
cargo fmt -p perl-lsp-perltidy -- --check
cargo clippy -p perl-lsp-perltidy --all-targets --locked -- -D warnings
cargo test -p perl-lsp-perltidy --all-targets --locked
cargo test -p perl-lsp-perltidy --test native_pipeline_counters_tests -- --test-threads=1
cargo bench -p perl-lsp-perltidy --bench native_pipeline_benchmark -- --quick
```

Open residuals (owned by upstream issues, not silently dropped):

- [ ] Enroll #9327 representative corpus identities through the subject
      registry seam when that corpus lands
- [ ] Align derived-output/allocation envelopes with #7140/#7501 product
      bounds when they codify limits (schema-major bump + before/after
      receipts)
- [ ] Cross-environment timing comparisons remain out of scope permanently
      unless a reviewed policy changes the advisory posture (#3979/#5282)

## EXPLICIT-HUMAN gate

Promoting NPC-003/NPC-004 counter canaries from package-scoped proof to a
required merge-blocking check for all formatter-touching PRs changes
integration posture beyond this lane's authority. Default delivered here:
they block within `-p perl-lsp-perltidy` proof runs; making them required
organization-wide is left as an explicit human decision.
