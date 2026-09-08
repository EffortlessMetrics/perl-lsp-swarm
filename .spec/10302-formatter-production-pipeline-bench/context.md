# Context: #10302 — production-path performance receipt for the native formatter pipeline

## Problem

The current candidate's nightly benchmark program enumerates 15 declared
Criterion targets across the workspace (`ci-nightly.yml` `BENCH_TARGETS`),
including `perl-lsp-perltidy:native_pipeline_benchmark:`. The earlier base
comparison had 14 targets and no formatter bench; that is historical baseline
context, not the candidate's current workflow state. The production
native path — source → parse gate
(`NativeFormatter::validate_clean_parse`, `Parser::parse_with_recovery`) →
safe-subset line/delimited rendering → edit derivation → typed
classification/evidence (`classify_native_result`) — therefore has no
reusable performance envelope, no stage attribution, and no guard against
any of the issue's seven named failure shapes (repeated parsing, quadratic
group fitting, allocation blowup, evidence dominating render, family-specific
variance, adapter double-invocation, helper-only benchmarking). Installed
first-useful journeys record end-to-end latency only and cannot localize a
regression.

## Governing evidence (2026-08-28, origin/main@a9664af790888333efbe50a042fa060f3cc2d171; historical candidate pin f709f1d19f2c0c0c1ac844040d732d50914c1252)

The historical candidate pin above is superseded by later documentation-only
commits. Current candidate identity is live PR state and must be re-derived
before using this context as exact-head evidence.

- Production seams to pin invocation counts against:
  `crates/perl-lsp-rs-core/src/providers/formatting/formatting.rs`
  `native_document_decision` definition (:249; one
  `format_document_typed` call at :257) and `native_range_decision`
  definition (:262; one `format_range_typed` call at :273) per native LSP
  request. The provider-facing counted subset is proven in
  `crates/perl-lsp-perltidy/tests/native_pipeline_counters_tests.rs` by
  `document_request_parses_exactly_once` and
  `range_request_parses_exactly_once`, which drive the public decision methods.
  The complete provider vector matrix is `NOT_PROVEN`; no dedicated
  `perl-lsp-rs-core` invocation-test artifact exists in this candidate. Vector order is always
  `(pipeline_invocations, source_parse_gate_invocations,
  formatted_output_parse_gate_invocations)`: Off and public invalid range
  `0/0/0`; typed literal-preserve refusal `1/0/0`; source-parse refusal
  `1/1/0`; successful/no-change document
  and complete-range `1/1/1`. The formatted-output-refusal branch at
  `implementation.rs:235-249` is currently unreachable, so its disposition
  and counters remain `NOT_PROVEN` rather than receiving synthetic proof.
- Bench infrastructure already exists and must be consumed, not forked:
  `.github/workflows/ci-nightly.yml` benchmark job deletes stale
  `target/criterion/` before running (#3979 integrity guards), invokes each
  *declared* target explicitly from `BENCH_TARGETS`, whose header contract
  requires the list to stay a superset of every bench-kind cargo-metadata
  target — an omitted new target is silent coverage loss;
  fixture-guard tests (`benchmarks/scripts/test_extract_criterion.py`,
  `test_benchmark_guards.py`) protect that chain; receipts/baselines/alerts
  flow through `benchmarks/scripts/{extract-criterion,format-results,
  compare.sh,alert}.py|sh` with baselines at `benchmarks/baselines/v*.json`.
- House timing policy is already correct and must be preserved verbatim:
  baseline comparison runs `continue-on-error: true` because shared-runner
  wall-clock variance would false-positive; integrity and regression policy
  are deliberately separate (#3979/#5282). Tight PR wall-clock gates are a
  named negative control in the issue.
- Instrumentation must come from the production path itself; today nothing
  exposes stage work. Nearest existing deterministic shape surfaces:
  paren-depth tracking (`implementation.rs:606`),
  `FormatChangeSummary`
  {edit_count, source_bytes_changed, rendered_bytes_changed, changed_lines}
  and stable digests in `FormatIdentity` — all already machine-comparable.
- Honest dependency boundary: representative corpus identities (#9327) are
  OPEN with zero tree references, and output/allocation envelopes (#7140/
  #7501) are OPEN; current subjects remain self-contained in the benchmark
  source, with a future subject-manifest extension seam for #9327, and counter envelopes must
  carry their own versioned schema until #7140 codifies product limits.
  Derived output/replacement/retained bytes can prove output growth but are
  not an allocation oracle. The selected future route is a serialized
  benchmark-only counting global allocator adapted from
  `xtask/src/allocation_tracker.rs`; allocation count/bytes/peak remain
  `NOT_PROVEN` until that seam produces a supported proof run and kills its
  controlled extra-copy mutant.

## Approach chosen (operation counters + serialized allocation window + counter-shape canaries; timing stays advisory)

Ranked against the issue's required instrumentation using only in-tree
evidence:

1. **Chosen — versioned `NativePipelineCounters` populated on the real
   classify/format path + Criterion subject benches + plain-test counter
   canaries.** Additive, zero-cost when unset: `NativeFormatter` gains an
   optional operation-scoped collector/builder. `FormattingProvider` gains an
   optional collector field/builder and forwards the same handle through each
   private `NativeFormatter::new()` call. Every dedicated receipt pass creates
   a fresh provider/formatter and collector keyed to one run + subject,
   snapshots before reuse, and never shares it across concurrent requests;
   default live providers carry no collector. The typed call path
   (`format_document_typed` / `format_range_typed`) records the pipeline invocation
   plus distinctly attributed source and formatted-output parse-gate
   invocations, tokens/nodes observed by each gate, lines processed,
   delimited groups fitted, edits derived, output/replacement bytes, peak
   depth, and one aggregate advisory `elapsed` field under `Instant`.
   The proposed `NativePipelineClock`, deterministic fake clock, named
   per-stage elapsed fields, and independent stage attribution are not present
   in the candidate and remain `NOT_PROVEN`; aggregate elapsed must not be
   presented as those fields. Timing remains advisory — extending
   `FormatChangeSummary`'s precedent rather than inventing a parallel
   formatter. In the fixed `(pipeline_invocations,
   source_parse_gate_invocations, formatted_output_parse_gate_invocations)`
   order, reachable provider-seam vectors are Off and invalid range
   `0/0/0`, typed literal-preserve `1/0/0`, source refusal
   `1/1/0`, and successful/no-change document and complete-range `1/1/1`;
   canaries reject a duplicated private typed call, a skipped successful gate,
   or a collapsed refusal table. Formatted-output refusal remains
   `NOT_PROVEN` because its defensive branch is currently unreachable.
   Provider-facing counted tests live in
   `crates/perl-lsp-perltidy/tests/native_pipeline_counters_tests.rs` as
   `document_request_parses_exactly_once` and
   `range_request_parses_exactly_once`; a dedicated `perl-lsp-rs-core`
   invocation-test artifact and the complete vector matrix remain
   `NOT_PROVEN`. Benches live in
   `crates/perl-lsp-perltidy/benches/native_pipeline_benchmark.rs`
   over the current procedural `bench_rows()` subjects. A versioned subject
   manifest is not present in this candidate and remains `NOT_PROVEN`.
   Its completeness test covers every named #10302 member:
   module/script/test/PSGI/data-processing; compact/multiline;
   delimited/statement/expression/list-operator; comment/trivia/opaque;
   Unicode/tabs/spaces/LF/CRLF/bare-CR; no-change/applied/preserved/refused;
   document/complete-range; bounded size/depth/width. Those bounded scaling
   rows expose superlinear growth in the counters this instrument actually
   records without dangerous allocations. They do not count each fit or
   structural-comparison operation, so NPC-004 deliberately does not claim
   to detect a quadratic implementation of either uninstrumented activity;
   production operation counters and a corresponding quadratic mutation
   remain `NOT_PROVEN` follow-up work. The PR-blocking discriminators are
   plain deterministic test files asserting (a) single pipeline invocation
   per request at the provider seam, (b) linear-or-bounded counter ratios
   across N/2N/4N scaling steps, (c) refusal/opaque rows stay cost-bounded.
   Nightly enrollment adds one `BENCH_TARGETS` entry, taking the declared
   inventory to 15 targets across ten crates. Existing timing reports, alerts,
   and baseline comparison keep their policy; exact subject identity requires
   the explicit receipt-schema extension below.
   Matches issue sections Required instrumentation / Benchmark subjects /
   Performance model / CI and cadence.
2. Wall-clock PR gates or raising job `timeout-minutes` to absorb a slow
   run — rejected and forbidden: masks real regressions behind noise or
   headroom; violates the anti-masking clause below and house policy.
3. Helper microbenchmark (format_simple_line alone) standing in for the
   pipeline — rejected: the issue's named failure shape; benches must drive
   `format_*_typed` end-to-end including classification/evidence.
4. Wait-for-#9327-corpus-first — rejected as primary: blocks the instrument
   on an open issue while bounded local subjects already discriminate shape;
   subject tables carry exact digests so #9327 identities can enroll without
   schema movement.

The representative formatter result is pinned as the emitted Criterion ID
`native_pipeline_document/delimited_n32_lf_tabs`, from the authoritative
`BENCH_GROUP = "native_pipeline_document"` and the `delimited_n32_lf_tabs`
subject name. Nightly strict extraction must pass the same
`--expect-id "native_pipeline_document/delimited_n32_lf_tabs"`; running the
target without producing that result is not a successful receipt. An
extractor fixture pins the grouped Criterion layout
`native_pipeline_document/delimited_n32_lf_tabs/new/estimates.json`. This
representative ID proves target execution only; it does not prove the full
subject matrix ran. The benchmark source and workflow are authoritative for
this group/name pair; the source-truth documents use this emitted ID rather
than an illustrative group/name pair.
The serialized `BENCH_TARGETS` entry is
`perl-lsp-perltidy:native_pipeline_benchmark:`: its target identity is
`perl-lsp-perltidy:native_pipeline_benchmark`, and the trailing delimiter
exists solely to encode an empty required-feature field.

Hosted wiring creates and exports exactly one `NATIVE_PIPELINE_RUN_ID` before
the `BENCH_TARGETS` loop, so the formatter benchmark and later extractor share
one run identity. The benchmark currently uploads its runtime measurement
sidecar and the representative Criterion result, but does not perform a strict
versioned-manifest/sidecar join. A future strict extractor would consume that
manifest, sidecar, matching `--expect-run-id`, and formatter
`--expect-id "native_pipeline_document/delimited_n32_lf_tabs"`. Structural pins
cover creation order, export/benchmark visibility, and every currently wired
strict argument; moving creation after the loop or dropping/diverging an
argument fails.

Full per-subject fixture/config receipt identity is not present today:
`extract-criterion.py`
keeps Criterion timing plus global Git SHA/dirty state, OS, and Rust version,
and `format-results.py` does not recover fixture digest, config fingerprint,
or engine. NPC-008 is therefore `NOT_PROVEN`. The selected implementation
provides a runtime-generated `target/criterion/native-pipeline-measurements.v1.json`
sidecar from procedural `bench_rows()` subjects, but does not provide the
versioned `native_pipeline_subjects.v1.json` manifest or a strict
manifest/sidecar join. Those are future #10302 work, not delivered behavior.
The future strict-join contract would carry schema/run and observed identity,
work/edit/depth/invocation counters, allocation measurements and
supported-platform state, plus named source-parse/render/formatted-parse/
edit-derivation/classification/total elapsed fields. It would require exactly
one row per manifest subject, produced by a dedicated serialized receipt pass
outside Criterion's repeated timing iterations; Criterion timing would stay
separate and join by canonical ID. Future strict extraction would take both
paths and an expected run ID, require a 1:1 Criterion/manifest/sidecar join,
and fail missing, duplicate, stale, unmatched, or schema-mismatched rows
before a future formatter-results receipt path preserves every field. The
planned `format-results.py latest.json --receipt` and
`validate-formatter-receipt.py` steps are not present in this candidate and
remain `NOT_PROVEN`.

A future allocation implementation is intended to use a separate durable seam
inside the benchmark executable:
`crates/perl-lsp-perltidy/benches/support/allocation_tracker.rs`, adapting the
counting global allocator in `xtask/src/allocation_tracker.rs`.
That future implementation would serialize one dedicated receipt pass per
manifest subject outside Criterion's repeated timing iterations; warm-up would
occur before reset; the allocator window would begin immediately before the
production `format_*_typed`
call and would be snapshotted immediately after; sidecar serialization would
occur outside it. Criterion timing would be joined separately. The future
sidecar would record `allocation_count`, `allocated_bytes`, and
`peak_delta_bytes` with a supported-platform tag. Unsupported/unavailable
measurement stays
`NOT_PROVEN`. Because the benchmark allocator requires `unsafe impl
GlobalAlloc`, unsafe trait methods, and forwarding blocks, the implementation
must place `SAFETY:` comments at each site and add three distinct cargo-allow
entries: IDs `formatter-native-bench-global-allocator-v1-{impl,fn,block}`.
Each has `kind = "unsafe"`; `family` and `selector` both set to the matching
`unsafe_impl`, `unsafe_fn`, or `unsafe_block` value; exact allocator-file glob;
`classification = "reviewed_exception"`; owner `formatter/performance`;
reason limited to forwarding the GlobalAlloc contract to `System`;
allocator-test and mutant evidence; `created = "2026-08-27"`;
`review_after = "2026-11-27"`; `expires = "2027-02-27"`.
`cargo-allow check --mode no-new` is required proof. The controlled mutant
`allocation_oracle_rejects_extra_temporary_copy` keeps one extra temporary
allocation/copy live inside that window and must turn the allocation canary
red before being reverted.

**Anti-masking clause:** no check introduced by this claim may be satisfied
by increasing any timeout, budget constant, size cap, or iteration bound;
envelope movement requires a major bump of the versioned counter schema plus
exact before/after counter receipts. A red canary is repaired by repairing
work, never by relaxing bounds.

## Claim boundary

PR #13190 is a bounded runtime-counter and benchmark-enrollment candidate;
#10302 remains open/blocked. The candidate delivers operation-scoped counters,
aggregate advisory elapsed, subject identity, and benchmark enrollment, but does
not prove per-stage timing, the complete provider vector matrix, allocation
count/bytes/peak, strict manifest/sidecar validation, the full #10302 matrix, or
release-tier evidence. Those boundaries remain `NOT_PROVEN`. There is no
formatter algorithm rewrite, Perl execution, or release/publication surface;
unset collectors remain byte-identical for current callers and goldens.
Derived-byte proof is limited to output growth and never claims allocation
count/bytes/peak.

## Composition

Sibling spec `.spec/10301-formatter-property-fuzz-harness/` shares only
subject vocabulary; file sets are disjoint (this claim: `benches/`,
additive `src/native/` counter plumbing, crate `Cargo.toml` dev-dep,
provider-facing counted proof in `crates/perl-lsp-perltidy/tests/native_pipeline_counters_tests.rs`,
provider collector forwarding, future subject-manifest/sidecar schema,
benchmark-only allocation tracker,
extractor/formatter receipt-schema fixtures and three-way join,
`.github/workflows/ci-nightly.yml` BENCH_TARGETS and strict-ID entries, canary
test file).
Nightly workflow policy comments (#3979) remain authoritative for the
benchmark job.

## Rollback

Single revert: remove the criterion dev-dep, benches dir, counter collector
plumbing (callers never read it), provider/canary tests, procedural subject
identity, future manifest, runtime-sidecar writer, benchmark-only allocation tracker, and
fail-closed receipt-schema join, and the BENCH_TARGETS/strict-ID entries.
Baselines remain untouched; no tracked artifact regeneration.
