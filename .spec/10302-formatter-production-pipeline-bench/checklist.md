# Checklist: #10302 production-path formatter performance receipt

Base pin: `origin/main@e6e956461534b2566c735696f289e0915e2cb189` (2026-08-27).
Composition sibling: #10301 fuzz/property spec
(`.spec/10301-formatter-property-fuzz-harness/`) — disjoint file set.
This PR is docs-only. Every implementation box below remains open, #10302
remains blocked, and no runtime benchmark or receipt proof is delivered here.

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
  `native_range_decision` definition (:262; typed call :273). Reachable
  vector order is `(pipeline_invocations, source_parse_gate_invocations,
  formatted_output_parse_gate_invocations)`: Off and public invalid range
  `0/0/0`, typed literal-preserve refusal `1/0/0`, source refusal `1/1/0`, and
  successful/no-change document or complete-range `1/1/1`. The defensive
  formatted-output-refusal branch is currently unreachable and `NOT_PROVEN`.
- Integrity guards to consume verbatim: stale `target/criterion/` deletion +
  explicit per-target invocation + superset contract over cargo-metadata
  bench-kind targets (#3979), fixture tests
  `benchmarks/scripts/test_extract_criterion.py` /
  `test_benchmark_guards.py`; baselines `benchmarks/baselines/v*.json`;
  comparison/alerts advisory by deliberate policy (#3979/#5282).

Planned surface:

- [ ] `crates/perl-lsp-perltidy/src/native/counters.rs`: additive
      `NativePipelineCounters` (schema v1) + cloneable operation-scoped
      collector handle; `NativeFormatter` gets an optional collector builder
      and forwards it through the typed call path; zero behavior when unset;
      exact
      `pipeline_invocations`, aggregate `parse_gate_invocations`,
      `source_parse_gate_invocations`, and
      `formatted_output_parse_gate_invocations` counters prove successful-path
      one-pipeline/two-parse attribution and disposition-specific early-refusal
      counts
- [ ] `FormattingProvider` gains an optional collector field/builder and
      forwards the same operation handle through both private
      `NativeFormatter::new()` call sites; default/unset construction remains
      byte-identical and zero-effect
- [ ] Each dedicated receipt pass constructs a fresh provider/formatter with a
      fresh collector keyed to exactly one run + subject, snapshots before
      reuse, and shares no collector across concurrent requests. Default live
      providers have no collector
- [ ] Add monotonic `NativePipelineClock`: production `Instant` adapter plus
      deterministic fake clock. Hook the owning production seams for
      `source_parse_elapsed_ns`, `render_elapsed_ns`,
      `formatted_parse_elapsed_ns`, `edit_derivation_elapsed_ns`,
      `classification_elapsed_ns`, and `total_elapsed_ns`. Successful/no-change
      full-path rows require independently attributed positive values; skipped
      refusal stages record explicit `not_executed`. Fixture/mutants reject
      absent, zeroed, copied, or collapsed stage values. Timing stays advisory
- [ ] `crates/perl-lsp-perltidy/Cargo.toml`: additive criterion dev-dep +
      `[[bench]] name = "native_pipeline_benchmark" harness = false`
- [ ] `crates/perl-lsp-perltidy/benches/native_pipeline_benchmark.rs` +
      `benches/support/perf_subjects.rs`: benchmark executable and subject
      loader over the authoritative registry
      and an actual `benchmark_group("native_pipeline")` /
      `bench_function("document_small", ...)` pair producing representative
      Criterion ID `native_pipeline/document_small` (not a direct benchmark
      string containing `/`, which Criterion sanitizes to `_`)
- [ ] Checked-in authoritative
      `crates/perl-lsp-perltidy/benches/native_pipeline_subjects.v1.json`, keyed
      by canonical Criterion ID, covers every required matrix member:
      module/script/test/PSGI/data-processing; compact/multiline;
      delimited/statement/expression/list-operator; comment/trivia/opaque;
      Unicode/tabs/spaces/LF/CRLF/bare-CR;
      no-change/applied/preserved/refused; document/complete-range; bounded
      size/depth/width. `subject_registry_covers_full_issue_matrix` fails for
      any missing category, request target, identity field, or bound; #9327
      remains later exact corpus enrollment
- [ ] Benchmark-only counting global allocator adapted from
      `xtask/src/allocation_tracker.rs` in
      `crates/perl-lsp-perltidy/benches/support/allocation_tracker.rs`:
      serialize each measured operation,
      and run one dedicated receipt pass per registry subject outside
      Criterion's repeated timing iterations. Warm up outside the window,
      reset immediately before `format_*_typed`, snapshot immediately after,
      and serialize later; join Criterion timing separately. Record
      `allocation_count`, `allocated_bytes`, `peak_delta_bytes`, and a
      supported-platform/unavailable tag; unavailable stays `NOT_PROVEN`
- [ ] The benchmark allocator's `unsafe impl GlobalAlloc` and each unsafe
      forwarding operation have site-local `SAFETY:` comments. Add three
      distinct `[[allow]]` entries with stable IDs
      `formatter-native-bench-global-allocator-v1-impl`, `-fn`, and `-block`.
      Each has `kind = "unsafe"`, with `family` and `selector` both set to
      the matching `unsafe_impl`, `unsafe_fn`, or `unsafe_block` value, exact glob
      `crates/perl-lsp-perltidy/benches/support/allocation_tracker.rs`,
      `classification = "reviewed_exception"`, owner `formatter/performance`,
      reason limited to forwarding the GlobalAlloc contract to `System`,
      allocator-test + controlled-mutant evidence, `created = "2026-08-27"`,
      `review_after = "2026-11-27"`, and `expires = "2027-02-27"`
- [ ] Runtime-generated
      `target/criterion/native-pipeline-measurements.v1.json`, distinct from
      the checked-in registry and keyed by canonical Criterion ID, records
      schema/run identity, observed subject/config/engine/environment identity,
      stage/work/edit/depth/invocation counters, allocation measurements, and
      named source-parse/render/formatted-parse/edit-derivation/classification/
      total elapsed fields. It contains exactly one row per
      registry subject from the dedicated serialized receipt pass; Criterion's
      repeated timing samples remain separate and join later by canonical ID
- [ ] `crates/perl-lsp-perltidy/tests/native_pipeline_counters_tests.rs`:
      NPC-001..NPC-006 + NPC-010 pipeline/parse canaries incl. detector sanity
      control and the early-refusal disposition table
- [ ] `crates/perl-lsp-rs-core/tests/native_pipeline_invocation_tests.rs`:
      construct the public provider with a request-local collector, drive
      `format_document_decision` / `format_range_decision`, prove exactly one
      private typed call, and pin Off/invalid-range `0/0/0`, typed-preserve
      `1/0/0`, source-refusal `1/1/0`, and success `1/1/1` vectors; do not claim the currently unreachable
      formatted-output-refusal vector or this adapter property from
      perltidy-only tests
- [ ] `.github/workflows/ci-nightly.yml`: one `BENCH_TARGETS` entry
      `"perl-lsp-perltidy:native_pipeline_benchmark:"`; target identity is
      `perl-lsp-perltidy:native_pipeline_benchmark`, and the trailing
      delimiter encodes only the empty required-feature field
- [ ] In the hosted benchmark step, create/export exactly one
      `NATIVE_PIPELINE_RUN_ID` before the `BENCH_TARGETS` loop; preserve it for
      the formatter benchmark process. Hosted strict extraction passes
      `--subject-registry`, `--measurement-sidecar`, matching
      `--expect-run-id`, and `--expect-id "native_pipeline/document_small"`.
      Structural pins fail if creation moves after the loop, export/visibility
      disappears, or any strict argument diverges
- [ ] Strict nightly extraction includes the exact matching
      `--expect-id "native_pipeline/document_small"`
- [ ] `benchmarks/scripts/test_extract_criterion.py` includes the exact grouped
      Criterion fixture layout
      `native_pipeline/document_small/new/estimates.json`; the representative
      ID proves target execution only, not full-matrix execution
- [ ] Extend `extract-criterion.py` with required `--subject-registry`,
      `--measurement-sidecar`, and `--expect-run-id` inputs. For formatter
      rows, strict mode requires a 1:1 Criterion/registry/runtime-sidecar join;
      extend the receipt/formatter path so every joined field survives.
      Fixtures reject missing, duplicate, stale, unmatched, and schema-
      mismatched rows. NPC-008 remains `NOT_PROVEN` until this path passes
      end-to-end
- [ ] Add `benchmarks/scripts/validate-formatter-receipt.py` and fixtures.
      After `format-results.py ... --receipt` writes the receipt file, validator
      inputs are receipt + registry + runtime sidecar + expected run/ID; fail
      closed unless every formatter row retains run/subject identity,
      work/invocation/edit/depth counters, allocation values/status, and every
      named stage + total elapsed field
- [ ] NPC-007..NPC-009 structural pins inside the same test file

Proof commands:

```bash
cargo fmt -p perl-lsp-perltidy -p perl-lsp-rs-core -- --check
cargo clippy -p perl-lsp-perltidy -p perl-lsp-rs-core --all-targets --locked -- -D warnings
cargo test -p perl-lsp-perltidy --all-targets --locked
cargo test -p perl-lsp-rs-core --all-targets --locked
cargo test -p perl-lsp-perltidy --test native_pipeline_counters_tests --locked -- --test-threads=1
cargo test -p perl-lsp-rs-core --test native_pipeline_invocation_tests --locked
cargo-allow check --mode no-new
export NATIVE_PIPELINE_RUN_ID="${GITHUB_RUN_ID:-local}-$(git rev-parse HEAD)-$(date +%s)"
rm -f target/criterion/native-pipeline-measurements.v1.json
NATIVE_PIPELINE_RUN_ID="$NATIVE_PIPELINE_RUN_ID" \
  cargo bench -p perl-lsp-perltidy --bench native_pipeline_benchmark -- --quick
python3 benchmarks/scripts/test_extract_criterion.py -v
python3 benchmarks/scripts/test_benchmark_guards.py -v
python3 benchmarks/scripts/extract-criterion.py \
  --output benchmarks/results/latest.json \
  --strict \
  --subject-registry crates/perl-lsp-perltidy/benches/native_pipeline_subjects.v1.json \
  --measurement-sidecar target/criterion/native-pipeline-measurements.v1.json \
  --expect-run-id "$NATIVE_PIPELINE_RUN_ID" \
  --expect-id "native_pipeline/document_small"
python3 benchmarks/scripts/format-results.py \
  benchmarks/results/latest.json --receipt \
  > benchmarks/results/native-pipeline-receipt.txt
python3 benchmarks/scripts/validate-formatter-receipt.py \
  --receipt benchmarks/results/native-pipeline-receipt.txt \
  --subject-registry crates/perl-lsp-perltidy/benches/native_pipeline_subjects.v1.json \
  --measurement-sidecar target/criterion/native-pipeline-measurements.v1.json \
  --expect-run-id "$NATIVE_PIPELINE_RUN_ID" \
  --expect-id "native_pipeline/document_small"
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
      Use the serialized benchmark-only counting allocator adapted from
      `xtask/src/allocation_tracker.rs`; require supported-platform evidence
      for allocation count/bytes/peak, site-local `SAFETY:` comments, and the
      narrow owned cargo-allow receipt. The oracle proof must include controlled mutant
      `allocation_oracle_rejects_extra_temporary_copy`: add one temporary
      allocation/copy to the measured production path, demonstrate the
      allocation canary turns red because count/bytes increase, then revert
      the mutant and retain both receipts
- [ ] Keep formatted-output-refusal disposition/counters `NOT_PROVEN` while
      the defensive production branch remains unreachable; do not add a
      synthetic direct-only test and call it production proof
- [ ] Cross-environment timing comparisons remain out of scope permanently
      unless a reviewed policy changes the advisory posture (#3979/#5282)

## EXPLICIT-HUMAN gate

Promoting NPC-003/NPC-004 counter canaries from package-scoped proof to a
required merge-blocking check for all formatter-touching PRs changes
integration posture beyond this lane's authority. Default delivered here:
pipeline/counter canaries block within `-p perl-lsp-perltidy`, while provider
single-invocation/vector canaries block within `-p perl-lsp-rs-core`; making
either required organization-wide is left as an explicit human decision.
