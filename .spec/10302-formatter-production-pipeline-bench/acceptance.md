# Acceptance: #10302 production-path formatter performance receipt

Each row binds one stable proposition to its discriminating executable proof.
Proof lives in `crates/perl-lsp-perltidy/tests/native_pipeline_counters_tests.rs`
(canaries; ordinary package CI), `benches/native_pipeline_benchmark.rs`
(subject benches), and `fph-free` structural pins — mirror of the house
policy-pin pattern.

| Row | Proposition | Proof | Status |
| --- | --- | --- | --- |
| NPC-001 | A versioned `NativePipelineCounters` instrument exists on the production typed path and is zero-effect when unset: default callers and all existing goldens behave byte-identically | `unset_collector_leaves_outcomes_byte_identical`; schema const pin (`COUNTER_SCHEMA = v1`) | offline |
| NPC-002 | Stage attribution covers source→parse gate→render→edit derivation→evidence classification with deterministic integer counters (gate invocations, lines processed, delimited groups fitted, edits derived, replacement bytes, peak depth) — no field may be "estimated" post hoc | `counters_populate_every_stage_from_production_path` | offline |
| NPC-003 | One LSP formatting request invokes the native pipeline exactly once, at both provider seams: `native_document_decision` and `native_range_decision` record gate_invocations == 1 per call; no adapter double-invoke passes green | `document_request_parses_exactly_once`, `range_request_parses_exactly_once` (mutation control: a second accidental `format_*_typed` call in either decision fn turns these red) | offline |
| NPC-004 | Counter shapes stay linear-or-bounded across scaling fixtures at N/2N/4N for every admitted family row; the detector itself is proven by flagging a known-quadratic synthetic series as superlinear | `scaling_cohort_ratios_stay_within_bounded_envelope`, `detector_flags_known_quadratic_series` (mutation control: loosening any ratio bound turns its own row red via the envelope-version const) | offline |
| NPC-005 | Refusal/opaque/no-change rows are cost-bounded too: unsupported-syntax floods and opaque-heavy subjects cannot bypass detection via cheap refusal paths blowing up elsewhere | `refusal_and_opaque_rows_remain_cost_bounded` | offline |
| NPC-006 | Allocation/output growth is detected before product envelopes: derived-bytes-per-source-byte and retained-output caps ride the versioned schema so regression trips before user-visible growth (#7140/#7501 alignment stays open upstream) | `derived_output_growth_trips_before_product_envelope` | offline |
| NPC-007 | The nightly benchmark job enrolls the new target: `BENCH_TARGETS` contains `perl-lsp-perltidy:native_pipeline_benchmark`, keeping the declared-superset contract over bench-kind cargo-metadata targets | policy pin test reading `.github/workflows/ci-nightly.yml` + manifest `[[bench]]` declaration pin | offline |
| NPC-008 | Bench subjects carry exact identity: every subject fixture pins content digest, config fingerprint, engine, and toolchain/environment tag in the emitted results JSON consumed unmodified by existing extract-criterion/format-results scripts | `subject_identity_is_recorded_for_receipt_consumption` (on-disk layout matches the Criterion extraction guards) | offline |
| NPC-009 | Timing remains evidence, never a required gate: no new wall-clock threshold enters any required check; baseline comparison/alert steps keep their advisory posture unchanged | workflow structural pin asserts `continue-on-error: true` survives verbatim on Compare/alerts steps (mutation control: deleting it turns this red) | offline |
| NPC-010 | Anti-masking ratchet: no `timeout-minutes` or iteration/size/budget constant increases anywhere this claim touches vs base-pin maxima, downward-only | `no_timeout_or_budget_constant_exceeds_base_pin_maxima` (mirror of CRW-006) | offline |

## Mutation controls (must stay red if reintroduced)

- Second parse/render invocation in an adapter path → NPC-003
- Deliberately quadratic fit/diff mutation slipping past scaling rows → NPC-004
  (the detector sanity control makes detector weakening itself observable)
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
- Formal allocation counting rides the selected stable instrument; if
  toolchain support is unavailable on some runner, that dimension degrades
  to derived-byte counters honestly labeled, never fabricated.
- #9327 corpus identities and #7140/#7501 product envelopes enroll through
  the registered seams when they land; until then the local cohort and
  schema-v1 envelopes are the authority.
