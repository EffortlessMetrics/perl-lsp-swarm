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
| NPC-001 | A versioned `NativePipelineCounters` instrument exists on the production typed path and is zero-effect when unset: default callers and all existing goldens behave byte-identically | `unset_collector_leaves_outcomes_byte_identical`; schema const pin (`COUNTER_SCHEMA = v1`) | offline |
| NPC-002 | Stage attribution covers source→source parse gate→render→formatted-output parse gate→edit derivation→evidence classification with deterministic integer counters (`pipeline_invocations`, aggregate `parse_gate_invocations`, `source_parse_gate_invocations`, `formatted_output_parse_gate_invocations`, lines processed, delimited groups fitted, edits derived, replacement/output bytes, peak depth) — no field may be "estimated" post hoc | `counters_populate_every_stage_from_production_path` | offline |
| NPC-003 | Each provider document/range request that selects the native formatter invokes `format_*_typed` exactly once. A successful/no-change native pipeline records `pipeline_invocations == 1`, `source_parse_gate_invocations == 1`, `formatted_output_parse_gate_invocations == 1`, and aggregate `parse_gate_invocations == 2`. Early refusals are disposition-specific: disabled/invalid-range/literal-preserve-before-parse paths record zero parse gates; a source-parse refusal records source == 1 and formatted-output == 0; a formatted-output refusal records one of each. No universal two-parse assertion may be applied to early refusals | `crates/perl-lsp-rs-core/tests/native_pipeline_invocation_tests.rs` drives public `format_document_decision` / `format_range_decision` with shared counters: `document_request_invokes_native_pipeline_once`, `range_request_invokes_native_pipeline_once`. Perltidy proof: `successful_document_and_range_parse_source_and_output_once`, `early_refusal_parse_counts_match_disposition` (mutation controls: add a second private provider `format_*_typed` call, skip either successful-path gate, parse either side twice, or collapse the refusal table to a universal count; the corresponding exact assertion turns red) | offline |
| NPC-004 | Counter shapes stay linear-or-bounded across scaling fixtures at N/2N/4N for every admitted family row; the detector itself is proven by flagging a known-quadratic synthetic series as superlinear | `scaling_cohort_ratios_stay_within_bounded_envelope`, `detector_flags_known_quadratic_series` (mutation control: loosening any ratio bound turns its own row red via the envelope-version const) | offline |
| NPC-005 | Refusal/opaque/no-change rows are cost-bounded too: unsupported-syntax floods and opaque-heavy subjects cannot bypass detection via cheap refusal paths blowing up elsewhere | `refusal_and_opaque_rows_remain_cost_bounded` | offline |
| NPC-006 | Derived-byte counters detect output growth before product envelopes: output/replacement/retained bytes may bound emitted-result growth only. They are not allocation evidence and must never satisfy #10302's allocation-count/allocated-byte requirement, which remains open until a supported proof run uses a real allocation oracle (#7140/#7501 alignment also stays open upstream) | `derived_output_growth_trips_before_product_envelope`; structural pin rejects labeling any derived-byte field or result as allocation proof | offline |
| NPC-007 | The nightly benchmark job enrolls target identity `perl-lsp-perltidy:native_pipeline_benchmark`. Its serialized `BENCH_TARGETS` entry is `perl-lsp-perltidy:native_pipeline_benchmark:`, where the trailing delimiter represents only the empty required-feature field. The bench exposes grouped Criterion ID `native_pipeline/document_small`, and strict extraction includes the matching `--expect-id "native_pipeline/document_small"`. This proves that target produced one representative result, not that its full subject matrix ran | policy pin test reading `.github/workflows/ci-nightly.yml` + manifest `[[bench]]` declaration pin + extractor fixture for the exact grouped on-disk layout + exact representative-ID/strict-extraction pin | offline |
| NPC-008 | Every formatter subject must carry content digest, config fingerprint, engine, and toolchain/environment identity in the durable receipt. Current `extract-criterion.py` preserves only Criterion timing plus global SHA/dirty/OS/Rust, so this is `NOT_PROVEN`. The selected implementation path is a checked-in subject-identity sidecar keyed by canonical Criterion ID, joined fail-closed by an extended extractor into the result schema and rendered/preserved by the formatter-results path; missing, duplicate, stale, or unmatched formatter identities fail strict extraction | Sidecar-schema fixtures + `subject_identity_sidecar_join_is_fail_closed` + end-to-end extractor/formatter fixture asserting every formatter row retains the joined identity and current environment tag | NOT_PROVEN |
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
- Missing/duplicate/stale/unmatched subject sidecar identity, or dropping a
  joined field before the durable result → NPC-008
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
  never substitute. #10302's allocation requirement stays open until a real
  allocation oracle measures count/bytes on a supported proof run.
- #9327 corpus identities and #7140/#7501 product envelopes enroll through
  the registered seams when they land; until then the local cohort and
  schema-v1 envelopes are the authority.
