# Acceptance: #10302 production-path formatter performance receipt

Each row binds one stable proposition to its discriminating executable proof.
Pipeline and provider-facing parse-counter proof lives in
`crates/perl-lsp-perltidy/tests/native_pipeline_counters_tests.rs` (canaries;
ordinary package CI), including `document_request_parses_exactly_once` and
`range_request_parses_exactly_once`. The stronger complete provider vector
matrix is not separately proven in this slice. Subject benches live in
`benches/native_pipeline_benchmark.rs`; workflow and receipt contracts use
source-backed structural pins — mirror of the house policy-pin pattern.
The specification was initially delivered by PR #12873. PR #13190 is a bounded
runtime-counter and benchmark-enrollment slice; it does not close #10302. The
allocation oracle, strict runtime-sidecar join/validation, and the remaining
full-matrix evidence stay explicitly `NOT_PROVEN` until their own proof exists.

## PR #13190 implementation boundary

This slice delivers the additive `NativePipelineCounters` instrument, source and
formatted-output parse-gate counters, counter-aware production typed entries,
one aggregate advisory elapsed field, deterministic subject rows with run
identity, a dedicated counted pass before Criterion timing, and nightly
enrollment/upload of the measurement sidecar. It
does not claim allocation count/bytes/peak, strict registry/sidecar validation,
the full #10302 matrix, or release-tier evidence. Those acceptance rows remain
open and must not be inferred from this slice's green package tests.

| Row | Proposition | Proof | Status |
| --- | --- | --- | --- |
| NPC-001 | A versioned `NativePipelineCounters` instrument is production-reachable and operation-scoped. `NativeFormatter` gains an optional collector/builder; `FormattingProvider` gains an optional collector field/builder and forwards the same handle through each private `NativeFormatter::new()` call. Every dedicated receipt pass constructs a fresh provider/formatter and fresh collector keyed to exactly one run + subject, snapshots before reuse, and shares no collector across concurrent requests. Default live providers have no collector; default/unset callers remain zero-effect and all existing goldens stay byte-identical | `unset_collector_leaves_outcomes_byte_identical`; `nested_counter_scope_populates_supplied_and_outer_snapshots`; `receipt_identity_rows_include_production_counter_snapshot`; schema const pin (`COUNTER_SCHEMA = v1`) | offline |
| NPC-002 | The production instrument records deterministic work counters for pipeline invocation, source/output parse gates, observed nodes, lines, fitted groups, edits, replacement bytes, depth, and one aggregate advisory `elapsed` duration measured with `Instant`. Named per-stage fields (`source_parse_elapsed_ns`, `render_elapsed_ns`, `formatted_parse_elapsed_ns`, `edit_derivation_elapsed_ns`, `classification_elapsed_ns`, `total_elapsed_ns`), a `NativePipelineClock` port, deterministic fake clock, and independent stage attribution are not implemented or proven in this slice and remain `NOT_PROVEN`; aggregate elapsed must not be presented as those fields | `counters_populate_every_stage_from_production_path`; `elapsed_measurement_wraps_classification_for_document_and_range`; no per-stage timing or fake-clock proof exists | work counters and aggregate elapsed offline; per-stage timing NOT_PROVEN |
| NPC-003 | The counted provider-facing document and range methods preserve one pipeline invocation and distinguish source/output parse-gate counts for successful and source-parse-refusal requests. The complete public vector matrix for provider `Off`, invalid range, typed literal-preserve refusal, and every successful disposition is not separately proven here; the formatted-output-refusal branch is unreachable and remains `NOT_PROVEN` | `crates/perl-lsp-perltidy/tests/native_pipeline_counters_tests.rs`: `document_request_parses_exactly_once`, `range_request_parses_exactly_once` (these drive the public provider methods); the broader vector matrix and a dedicated `perl-lsp-rs-core` provider test artifact are absent | reachable document/range subset offline; complete vector matrix NOT_PROVEN |
| NPC-004 | The currently observed production-path counters (`gate_nodes_observed`, `lines_processed`, `layout_groups_fitted`, `edits_derived`, and `replacement_bytes`) stay linear-or-bounded across scaling fixtures at N/2N/4N for every admitted family row. This is a bounded-shape canary for recorded observations, not a proof of algorithmic complexity for uninstrumented fit or structural-comparison work; production operation counters for those activities remain `NOT_PROVEN`. The detector applies only to positive, ordered samples; its absolute slack is capped by each interval's lower sample, so the smallest realistic quadratic series `(1, 4, 16)` is classified as superlinear while linear boundary controls remain bounded. | `scaling_cohort_ratios_stay_within_bounded_envelope`, `detector_flags_known_quadratic_series` (including positive-domain, lower-boundary, and zero-domain controls; loosening either detector bound turns the synthetic-control assertion red via the envelope-version const) | offline |
| NPC-005 | Refusal/opaque/no-change rows are cost-bounded too: unsupported-syntax floods and opaque-heavy subjects cannot bypass detection via cheap refusal paths blowing up elsewhere | `refusal_and_opaque_rows_remain_cost_bounded` | offline |
| NPC-006 | Derived bytes prove output growth only. A future #10302 allocation contract would adapt the counting global allocator in `xtask/src/allocation_tracker.rs` and run one serialized receipt pass per future manifest subject outside Criterion's repeated timing iterations. The allocator oracle, sidecar, and controlled-mutant evidence remain `NOT_PROVEN`. | Future allocation-window, receipt, cargo-allow, and controlled-mutant proofs | output offline; allocation `NOT_PROVEN` |
| NPC-007 | The nightly benchmark job in `.github/workflows/ci-nightly.yml` enrolls target identity `perl-lsp-perltidy:native_pipeline_benchmark`. Offline source inspection confirms that the workflow creates and exports one `NATIVE_PIPELINE_RUN_ID` before the `BENCH_TARGETS` loop, includes the formatter benchmark target, preserves the empty required-feature field in the serialized target entry, and pins the representative Criterion ID `native_pipeline_document/delimited_n32_lf_tabs`. The representative expectation proves only that this target is requested and extracted when the nightly job runs; hosted execution, runtime-sidecar production, and strict registry/sidecar/run-ID joining remain `NOT_PROVEN` in this claim | Workflow structural pins for run-ID creation/export, formatter-benchmark enrollment, target/manifest syntax, and the exact grouped-ID expectation; hosted execution and strict join require a separate supported receipt | offline; hosted execution and strict joining NOT_PROVEN |
| NPC-008 | Future #10302 contract, not delivered by PR #13190: a versioned subject manifest (the proposed JSON path is not present) would cover every matrix member, and a distinct runtime sidecar would have exactly one serialized receipt-pass row per manifest key with strict Criterion/manifest/sidecar joining. The current candidate has neither the manifest nor the runtime sidecar/validator path; those remain `NOT_PROVEN`. | Future manifest completeness, three-way join fixtures, receipt retention, and strict extraction/validation commands | NOT_PROVEN |
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
