# Acceptance: #10302 production-path formatter performance receipt

Each row binds one stable proposition to its discriminating executable proof.
Pipeline/parse-counter proof lives in
`crates/perl-lsp-perltidy/tests/native_pipeline_counters_tests.rs` (canaries;
ordinary package CI). Provider single-invocation proof lives in
`crates/perl-lsp-rs-core/tests/native_pipeline_invocation_tests.rs`, driving
public `format_document_decision` / `format_range_decision` with shared
counters so a duplicate private typed call is observable; it cannot be claimed
by a perltidy-only test. Subject benches live in
`benches/native_pipeline_benchmark.rs`; workflow and receipt contracts use
source-backed structural pins — mirror of the house policy-pin pattern.

| Row | Proposition | Proof | Status |
| --- | --- | --- | --- |
| NPC-001 | A versioned `NativePipelineCounters` instrument is production-reachable and operation-scoped. `NativeFormatter` gains an optional collector/builder; `FormattingProvider` gains an optional collector field/builder and forwards the same handle through each private `NativeFormatter::new()` call. Default/unset callers remain zero-effect and all existing goldens stay byte-identical | `unset_collector_leaves_outcomes_byte_identical`; `provider_forwards_same_operation_collector`; schema const pin (`COUNTER_SCHEMA = v1`) | offline |
| NPC-002 | Stage attribution covers source→source parse gate→render→formatted-output parse gate→edit derivation→evidence classification with deterministic integer counters (`pipeline_invocations`, aggregate `parse_gate_invocations`, `source_parse_gate_invocations`, `formatted_output_parse_gate_invocations`, lines processed, delimited groups fitted, edits derived, replacement/output bytes, peak depth) — no field may be "estimated" post hoc | `counters_populate_every_stage_from_production_path` | offline |
| NPC-003 | Vector order is always `(pipeline_invocations, source_parse_gate_invocations, formatted_output_parse_gate_invocations)`. At the provider seam: provider `Off` = `0/0/0`; invalid range and typed literal-preserve refusal = `1/0/0`; source-parse refusal = `1/1/0`; successful/no-change document and complete-range = `1/1/1`. `FormattingProvider` forwards one shared operation collector into exactly one private `format_*_typed` invocation. The formatted-output-refusal branch is currently unreachable, so its vector/outcome stays `NOT_PROVEN` rather than receiving a synthetic direct-proof claim | `crates/perl-lsp-rs-core/tests/native_pipeline_invocation_tests.rs` constructs the public provider with a collector and drives `format_document_decision` / `format_range_decision`: `provider_disposition_vectors_are_exact`, `document_request_invokes_native_pipeline_once`, `range_request_invokes_native_pipeline_once`. Perltidy proof: `successful_document_and_range_parse_source_and_output_once`, `early_refusal_parse_counts_match_disposition` (mutation controls: duplicate either private provider typed call, skip either successful-path gate, parse either side twice, or collapse the refusal table; an exact assertion turns red) | reachable vectors offline; formatted-output refusal `NOT_PROVEN` |
| NPC-004 | Counter shapes stay linear-or-bounded across scaling fixtures at N/2N/4N for every admitted family row; the detector itself is proven by flagging a known-quadratic synthetic series as superlinear | `scaling_cohort_ratios_stay_within_bounded_envelope`, `detector_flags_known_quadratic_series` (mutation control: loosening any ratio bound turns its own row red via the envelope-version const) | offline |
| NPC-005 | Refusal/opaque/no-change rows are cost-bounded too: unsupported-syntax floods and opaque-heavy subjects cannot bypass detection via cheap refusal paths blowing up elsewhere | `refusal_and_opaque_rows_remain_cost_bounded` | offline |
| NPC-006 | Derived bytes prove output growth only. Real allocation proof is selected: `benches/support/allocation_tracker.rs` adapts the counting global allocator in `xtask/src/allocation_tracker.rs` and the benchmark runs one dedicated serialized receipt pass per registry subject outside Criterion's repeated timing iterations. It warms up before reset and records `allocation_count`, `allocated_bytes`, and `peak_delta_bytes` before sidecar serialization; Criterion timing is joined separately. The `unsafe impl GlobalAlloc`, unsafe trait methods, and forwarding blocks require site-local `SAFETY:` comments plus narrow owned cargo-allow receipts in family `formatter-native-bench-global-allocator-v1`: exact file glob with one selector each for `unsafe_impl`, `unsafe_fn`, and `unsafe_block`; owner `formatter/performance`; GlobalAlloc-forwarding reason; allocator/mutant evidence; created `2026-08-27`; review-after `2026-11-27`. Each row carries a supported-platform tag; unavailable/unsupported measurement is `NOT_PROVEN`, never a derived-byte substitute. Controlled mutant `allocation_oracle_rejects_extra_temporary_copy` adds one live temporary allocation/copy inside the measured production window and must turn the allocation canary red | `derived_output_growth_trips_before_product_envelope`; `allocation_window_excludes_warmup_timing_iterations_and_serialization`; `one_allocation_receipt_row_per_registry_subject`; `cargo-allow check --mode no-new`; supported-run allocation receipt; controlled extra-copy mutant receipt | output offline; allocation `NOT_PROVEN` until supported proof runs |
| NPC-007 | The nightly benchmark job enrolls target identity `perl-lsp-perltidy:native_pipeline_benchmark`. Its serialized `BENCH_TARGETS` entry is `perl-lsp-perltidy:native_pipeline_benchmark:`, where the trailing delimiter represents only the empty required-feature field. The bench exposes grouped Criterion ID `native_pipeline/document_small`, and strict extraction includes the matching `--expect-id "native_pipeline/document_small"`. This proves that target produced one representative result, not that its full subject matrix ran | policy pin test reading `.github/workflows/ci-nightly.yml` + manifest `[[bench]]` declaration pin + extractor fixture for the exact grouped on-disk layout + exact representative-ID/strict-extraction pin | offline |
| NPC-008 | A checked-in authoritative subject registry keyed by canonical Criterion ID covers every #10302 matrix member: module/script/test/PSGI/data-processing; compact/multiline; delimited/statement/expression/list-operator; comment/trivia/opaque; Unicode/tabs/spaces/LF/CRLF/bare-CR; no-change/applied/preserved/refused; document/complete-range; bounded size/depth/width. A distinct runtime-generated measurement sidecar has exactly one dedicated serialized receipt-pass row per registry key (outside Criterion's repeated timing iterations) and records schema/run/observed identity, stage/work/edit/depth/invocation counters, allocation measurements, supported-platform state, and receipt-pass stage/total timing. Criterion timing remains separate and is joined by canonical ID. Strict extraction requires a 1:1 Criterion/registry/sidecar join and rejects missing, duplicate, stale, unmatched, or schema-mismatched rows before the formatter-results path preserves them | `subject_registry_covers_full_issue_matrix`; `one_measurement_row_per_registry_subject`; sidecar-schema fixtures; `criterion_registry_sidecar_join_is_one_to_one_and_fail_closed`; end-to-end strict extractor/formatter fixture | NOT_PROVEN |
| NPC-009 | Timing remains evidence, never a required gate: no new wall-clock threshold enters any required check; baseline comparison/alert steps keep their advisory posture unchanged | workflow structural pin asserts `continue-on-error: true` survives verbatim on Compare/alerts steps (mutation control: deleting it turns this red) | offline |
| NPC-010 | Anti-masking ratchet: no `timeout-minutes` or iteration/size/budget constant increases anywhere this claim touches vs base-pin maxima, downward-only | `no_timeout_or_budget_constant_exceeds_base_pin_maxima` (mirror of CRW-006) | offline |

## Mutation controls (must stay red if reintroduced)

- Second provider/native-pipeline invocation, either missing successful-path
  parse-gate pass, any source/formatted-output parse-gate double invocation,
  or universal-two-parse treatment of an early refusal → NPC-003
- Deliberately quadratic fit/diff mutation slipping past scaling rows → NPC-004
  (the detector sanity control makes detector weakening itself observable)
- Dropping `native_pipeline/document_small` from the bench or its strict
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
