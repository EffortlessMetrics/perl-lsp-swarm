# Acceptance: #10302 production-path formatter performance receipt

Each row binds one stable proposition to its discriminating executable proof.
Pipeline/parse-counter proof lives in
`crates/perl-lsp-perltidy/tests/native_pipeline_counters_tests.rs` (canaries;
ordinary package CI). Provider single-invocation proof lives in
`crates/perl-lsp-rs-core/tests/native_pipeline_invocation_tests.rs`, driving
public `format_document_decision` / `format_range_decision` with request-local
counters so a duplicate private typed call is observable; it cannot be claimed
by a perltidy-only test. Subject benches live in
`benches/native_pipeline_benchmark.rs`; workflow and receipt contracts use
source-backed structural pins — mirror of the house policy-pin pattern.
The specification was initially delivered by PR #12873. PR #13190 is a bounded
runtime-counter and benchmark-enrollment slice; it does not close #10302. The
allocation oracle, strict runtime-sidecar join/validation, and the remaining
full-matrix evidence stay explicitly `NOT_PROVEN` until their own proof exists.

## PR #13190 implementation boundary

This slice delivers the additive `NativePipelineCounters` instrument, source and
formatted-output parse-gate counters, counter-aware production typed entries,
deterministic subject rows with run identity, a dedicated counted pass before
Criterion timing, and nightly enrollment/upload of the measurement sidecar. It
does not claim allocation count/bytes/peak, strict registry/sidecar validation,
the full #10302 matrix, or release-tier evidence. Those acceptance rows remain
open and must not be inferred from this slice's green package tests.

| Row | Proposition | Proof | Status |
| --- | --- | --- | --- |
| NPC-001 | A versioned `NativePipelineCounters` instrument is production-reachable and operation-scoped. `NativeFormatter` gains an optional collector/builder; `FormattingProvider` gains an optional collector field/builder and forwards the same handle through each private `NativeFormatter::new()` call. Every dedicated receipt pass constructs a fresh provider/formatter and fresh collector keyed to exactly one run + subject, snapshots before reuse, and shares no collector across concurrent requests. Default live providers have no collector; default/unset callers remain zero-effect and all existing goldens stay byte-identical | `unset_collector_leaves_outcomes_byte_identical`; `provider_forwards_same_operation_collector`; `receipt_pass_collectors_are_fresh_and_request_local`; schema const pin (`COUNTER_SCHEMA = v1`) | offline |
| NPC-002 | Stage attribution covers source parse→render→formatted parse→edit derivation→classification with deterministic counters plus named elapsed fields `source_parse_elapsed_ns`, `render_elapsed_ns`, `formatted_parse_elapsed_ns`, `edit_derivation_elapsed_ns`, `classification_elapsed_ns`, and `total_elapsed_ns`. A `NativePipelineClock` monotonic-clock port uses a production `Instant` adapter and deterministic fake clock in tests; hooks live at each owning production seam. Successful/no-change full-path rows require independently attributed positive fields; skipped refusal stages use explicit `not_executed`, never ambiguous zero. Missing, zeroed, copied, or collapsed stage values fail. Work counters include `pipeline_invocations`, aggregate `parse_gate_invocations`, source/output gate invocations, lines, groups, edits, bytes, and depth — nothing is estimated post hoc | `counters_populate_every_stage_from_production_path`; `stage_elapsed_fields_are_independently_attributed`; fake-clock fixture; absent/zero/copy/collapse timing mutants | offline |
| NPC-003 | Vector order is always `(pipeline_invocations, source_parse_gate_invocations, formatted_output_parse_gate_invocations)`. At the public provider seam: provider `Off` and invalid range = `0/0/0` because `format_range_decision` rejects inadmissible ranges before native selection; typed literal-preserve refusal = `1/0/0`; source-parse refusal = `1/1/0`; successful/no-change document and complete-range = `1/1/1`. `FormattingProvider` forwards the same request-local collector handle into exactly one private `format_*_typed` invocation. The formatted-output-refusal branch is currently unreachable, so its vector/outcome stays `NOT_PROVEN` rather than receiving a synthetic direct-proof claim | `crates/perl-lsp-rs-core/tests/native_pipeline_invocation_tests.rs` constructs the public provider with a collector and drives `format_document_decision` / `format_range_decision`: `provider_disposition_vectors_are_exact`, `document_request_invokes_native_pipeline_once`, `range_request_invokes_native_pipeline_once`. Perltidy proof: `successful_document_and_range_parse_source_and_output_once`, `early_refusal_parse_counts_match_disposition` (mutation controls: duplicate either private provider typed call, skip either successful-path gate, parse either side twice, or collapse the refusal table; an exact assertion turns red) | reachable vectors offline; formatted-output refusal `NOT_PROVEN` |
| NPC-004 | The currently observed production-path counters (`gate_nodes_observed`, `lines_processed`, `delimited_groups_fitted`, `edits_derived`, and `replacement_bytes`) stay linear-or-bounded across scaling fixtures at N/2N/4N for every admitted family row. This is a bounded-shape canary for recorded observations, not a proof of algorithmic complexity for uninstrumented fit or structural-comparison work; production operation counters for those activities remain `NOT_PROVEN`. The detector applies only to positive, ordered samples; its absolute slack is capped by each interval's lower sample, so the smallest realistic quadratic series `(1, 4, 16)` is classified as superlinear while linear boundary controls remain bounded. | `scaling_cohort_ratios_stay_within_bounded_envelope`, `detector_flags_known_quadratic_series` (including positive-domain, lower-boundary, and zero-domain controls; loosening either detector bound turns the synthetic-control assertion red via the envelope-version const) | offline |
| NPC-005 | Refusal/opaque/no-change rows are cost-bounded too: unsupported-syntax floods and opaque-heavy subjects cannot bypass detection via cheap refusal paths blowing up elsewhere | `refusal_and_opaque_rows_remain_cost_bounded` | offline |
| NPC-006 | Derived bytes prove output growth only. Real allocation proof is selected: `benches/support/allocation_tracker.rs` adapts the counting global allocator in `xtask/src/allocation_tracker.rs` and the benchmark runs one dedicated serialized receipt pass per registry subject outside Criterion's repeated timing iterations. It warms up before reset and records `allocation_count`, `allocated_bytes`, and `peak_delta_bytes` before sidecar serialization; Criterion timing is joined separately. The unsafe sites require local `SAFETY:` comments and three distinct reviewed cargo-allow entries: IDs `formatter-native-bench-global-allocator-v1-{impl,fn,block}`, each `kind = "unsafe"`, with `family` and `selector` both set to its matching `unsafe_impl`, `unsafe_fn`, or `unsafe_block` value, exact allocator-file glob, `classification = "reviewed_exception"`, owner `formatter/performance`, GlobalAlloc-forwarding reason, allocator/mutant evidence, `created = "2026-08-27"`, `review_after = "2026-11-27"`, and `expires = "2027-02-27"`. Each row carries a supported-platform tag; unavailable/unsupported measurement is `NOT_PROVEN`. Controlled mutant `allocation_oracle_rejects_extra_temporary_copy` must turn the allocation canary red | `derived_output_growth_trips_before_product_envelope`; `allocation_window_excludes_warmup_timing_iterations_and_serialization`; `one_allocation_receipt_row_per_registry_subject`; `cargo-allow check --mode no-new`; supported-run allocation receipt; controlled extra-copy mutant receipt | output offline; allocation `NOT_PROVEN` until supported proof runs |
| NPC-007 | The nightly benchmark job in `.github/workflows/ci-nightly.yml` enrolls target identity `perl-lsp-perltidy:native_pipeline_benchmark`. Offline source inspection confirms that the workflow creates and exports one `NATIVE_PIPELINE_RUN_ID` before the `BENCH_TARGETS` loop, includes the formatter benchmark target, preserves the empty required-feature field in the serialized target entry, and pins the representative Criterion ID `native_pipeline_document/delimited_n32_lf_tabs`. The representative expectation proves only that this target is requested and extracted when the nightly job runs; hosted execution, runtime-sidecar production, and strict registry/sidecar/run-ID joining remain `NOT_PROVEN` in this claim | Workflow structural pins for run-ID creation/export, formatter-benchmark enrollment, target/manifest syntax, and the exact grouped-ID expectation; hosted execution and strict join require a separate supported receipt | offline; hosted execution and strict joining NOT_PROVEN |
| NPC-008 | The authoritative registry covers every #10302 matrix member. A distinct runtime sidecar has exactly one dedicated serialized receipt-pass row per registry key and carries run/subject identity, work/invocation/edit/depth counters, allocation, supported-platform state, and named stage/total timing; Criterion timing joins separately. Strict extraction requires a 1:1 Criterion/registry/sidecar join. Then `format-results.py ... --receipt` writes a receipt file, and `validate-formatter-receipt.py` fails closed unless every formatter row retains run/subject identity, all counters, allocation fields/status, and each stage/total timing field. Missing, duplicate, stale, unmatched, schema-mismatched, or dropped fields fail | Registry completeness; three-way join fixtures; `formatter_receipt_retains_all_joined_evidence`; exact strict-extraction/render/validator commands | NOT_PROVEN |
| NPC-009 | Timing remains evidence, never a required gate: no new wall-clock threshold enters any required check; baseline comparison/alert steps keep their advisory posture unchanged | workflow structural pin asserts `continue-on-error: true` survives verbatim on Compare/alerts steps (mutation control: deleting it turns this red) | offline |
| NPC-010 | Anti-masking ratchet: no `timeout-minutes` or iteration/size/budget constant increases anywhere this claim touches vs base-pin maxima, downward-only | `no_timeout_or_budget_constant_exceeds_base_pin_maxima` (mirror of CRW-006) | offline |

## Mutation controls (must stay red if reintroduced)

- Second provider/native-pipeline invocation, either missing successful-path
  parse-gate pass, any source/formatted-output parse-gate double invocation,
  or universal-two-parse treatment of an early refusal → NPC-003
- Loosening the NPC-004 ratio/slack bounds without a schema change → NPC-004
  (the synthetic detector control makes detector weakening itself observable;
  actual fit/diff-operation complexity remains `NOT_PROVEN`)
- Dropping `native_pipeline_document/delimited_n32_lf_tabs` from the bench or its strict
  `--expect-id` pin → NPC-007
- Adding the controlled temporary allocation/copy
  `allocation_oracle_rejects_extra_temporary_copy` without turning the real
  allocation canary red → NPC-006 remains `NOT_PROVEN`
- Missing registry matrix member; missing/duplicate/stale/unmatched or
  schema-mismatched Criterion/registry/sidecar row; or dropping a joined field
  before the durable result → NPC-008
- Aggregate-average-only reporting hiding a family regression → NPC-008
  (per-subject identity rows are mandatory)
- Promoting timing to a required PR gate → NPC-009
- Raising any timeout/budget/size constant to obtain green → NPC-010
- Benchmarking a helper instead of `format_*_typed` end-to-end → NPC-002
  (stage fields cannot populate without the real pipeline)

## Non-proof residuals (named, not silently dropped)

- Cross-machine timing comparability stays out of scope; unlike-environment
  timings are recorded but never compared (#issue rule preserved).
- Scheduled/release-tier full-matrix environments remain owned by the
  nightly job's existing cadence; this claim guarantees enrollment, not new
  runner provisioning.
- Allocation count and allocated-byte proof are `NOT_PROVEN`. Derived output,
  replacement, or retained-byte counters prove output growth only and may
  never substitute. #10302's allocation requirement stays open until the
  selected serialized counting-allocator window measures count/bytes/peak on
  a supported proof run and kills its controlled extra-copy mutant.
- Formatted-output refusal is a defensive but currently unreachable branch;
  its disposition/counter vector remains `NOT_PROVEN` until production-
  reachable discriminating proof exists.
- #9327 corpus identities and #7140/#7501 product envelopes enroll through
  the registered seams when they land; until then the local cohort and
  schema-v1 envelopes are the authority.
