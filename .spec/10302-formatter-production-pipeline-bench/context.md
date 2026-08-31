# Context: #10302 — production-path performance receipt for the native formatter pipeline

## Problem

The nightly benchmark program measures 14 declared Criterion targets across
nine crates (`ci-nightly.yml` `BENCH_TARGETS`), and **none of them touches
the formatter**: `crates/perl-lsp-perltidy` has no `[[bench]]` target, no
criterion dev-dependency, and its sources contain zero counters/metrics/
stats structures (tree-wide grep on the base pin: no hits). The production
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

## Governing evidence (2026-08-27, origin/main@e6e956461534b2566c735696f289e0915e2cb189)

- Production seams to pin invocation counts against:
  `crates/perl-lsp-rs-core/src/providers/formatting/formatting.rs`
  `native_document_decision` definition (:249; one
  `format_document_typed` call at :257) and `native_range_decision`
  definition (:262; one `format_range_typed` call at :273) per native LSP
  request. Provider single-invocation proof lives in
  `crates/perl-lsp-rs-core/tests/native_pipeline_invocation_tests.rs`, driving
  public `format_document_decision` / `format_range_decision` with one
  request-local collector handle so a duplicate private typed call is
  observable; a perltidy-only
  test cannot establish adapter call count. Vector order is always
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
  #7501) are OPEN; subjects must be self-contained checked-in fixtures now
  with a registered extension seam for #9327, and counter envelopes must
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
   depth, and elapsed fields under monotonic `NativePipelineClock`. Its
   production adapter uses `Instant`; tests use a deterministic fake clock.
   Owning source-parse, render, formatted-parse, edit-derivation,
   classification, and total seams record `source_parse_elapsed_ns`,
   `render_elapsed_ns`, `formatted_parse_elapsed_ns`,
   `edit_derivation_elapsed_ns`, `classification_elapsed_ns`, and
   `total_elapsed_ns` independently. Successful/no-change full-path rows
   require positive independently attributed fields; skipped refusal stages
   use explicit `not_executed`. Fake-clock fixtures and mutants reject absent,
   zeroed, copied, or collapsed values. Timing remains advisory — extending
   `FormatChangeSummary`'s precedent rather than inventing a parallel
   formatter. In the fixed `(pipeline_invocations,
   source_parse_gate_invocations, formatted_output_parse_gate_invocations)`
   order, reachable provider-seam vectors are Off and invalid range
   `0/0/0`, typed literal-preserve `1/0/0`, source refusal
   `1/1/0`, and successful/no-change document and complete-range `1/1/1`;
   canaries reject a duplicated private typed call, a skipped successful gate,
   or a collapsed refusal table. Formatted-output refusal remains
   `NOT_PROVEN` because its defensive branch is currently unreachable.
   Provider tests live in
   `crates/perl-lsp-rs-core/tests/native_pipeline_invocation_tests.rs` and
   drive the public decision methods with a request-local collector; the perltidy
   canaries own only pipeline-internal attribution. Benches live in
   `crates/perl-lsp-perltidy/benches/native_pipeline_benchmark.rs`
   over a checked-in authoritative registry keyed by canonical Criterion ID.
   Its completeness test covers every named #10302 member:
   module/script/test/PSGI/data-processing; compact/multiline;
   delimited/statement/expression/list-operator; comment/trivia/opaque;
   Unicode/tabs/spaces/LF/CRLF/bare-CR; no-change/applied/preserved/refused;
   document/complete-range; bounded size/depth/width. Those bounded scaling
   rows expose superlinear growth without dangerous allocations. The
   PR-blocking discriminators are
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

The representative formatter result is pinned as Criterion ID
`native_pipeline/document_small`. Nightly strict extraction must pass the
same `--expect-id "native_pipeline/document_small"`; running the target
without producing that result is not a successful receipt. An extractor
fixture pins the grouped Criterion layout
`native_pipeline/document_small/new/estimates.json`. This representative ID
proves target execution only; it does not prove the full subject matrix ran.
The bench must create `benchmark_group("native_pipeline")` and call that
group's `bench_function("document_small", ...)`; a direct benchmark string
containing `/` is sanitized to `_` and cannot establish the grouped pin.
The serialized `BENCH_TARGETS` entry is
`perl-lsp-perltidy:native_pipeline_benchmark:`: its target identity is
`perl-lsp-perltidy:native_pipeline_benchmark`, and the trailing delimiter
exists solely to encode an empty required-feature field.

Hosted wiring creates and exports exactly one `NATIVE_PIPELINE_RUN_ID` before
the `BENCH_TARGETS` loop, so the formatter benchmark and later extractor share
one run identity. Hosted strict extraction supplies the authoritative subject
registry, runtime measurement sidecar, matching `--expect-run-id`, and
formatter `--expect-id "native_pipeline/document_small"`. Structural pins
cover creation order, export/benchmark visibility, and every strict argument;
moving creation after the loop or dropping/diverging an argument fails.

Per-subject receipt identity is not present today: `extract-criterion.py`
keeps Criterion timing plus global Git SHA/dirty state, OS, and Rust version,
and `format-results.py` does not recover fixture digest, config fingerprint,
or engine. NPC-008 is therefore `NOT_PROVEN`. The selected implementation has
two distinct artifacts keyed by canonical Criterion ID: a checked-in
authoritative `native_pipeline_subjects.v1.json` registry, and a runtime-
generated `target/criterion/native-pipeline-measurements.v1.json` sidecar.
The sidecar carries schema/run and observed identity, work/edit/depth/
invocation counters, allocation measurements and supported-platform state,
and named source-parse/render/formatted-parse/edit-derivation/classification/
total elapsed fields. It has exactly one row per registry
subject, produced by a dedicated serialized receipt pass outside Criterion's
repeated timing iterations; Criterion timing stays separate and joins by
canonical ID. Strict extraction takes both paths and an expected run ID,
requires a 1:1 Criterion/registry/sidecar join, and fails missing,
duplicate, stale, unmatched, or schema-mismatched rows before the formatter-
results path preserves every field. After strict extraction,
`format-results.py latest.json --receipt` writes a receipt file and the planned
`validate-formatter-receipt.py` checks receipt + registry + sidecar + expected
run/ID fail-closed, requiring every formatter row to retain identity,
counters, allocation/status, and all stage/total timing fields.

Allocation uses a separate durable seam inside the benchmark executable:
`crates/perl-lsp-perltidy/benches/support/allocation_tracker.rs`, adapting the
counting global allocator in `xtask/src/allocation_tracker.rs`.
One dedicated receipt pass per registry subject is serialized outside
Criterion's repeated timing iterations; warm-up occurs before reset; the
allocator window begins immediately before the production `format_*_typed`
call and is snapshotted immediately after; sidecar serialization occurs
outside it. Criterion timing is joined separately. The
sidecar records `allocation_count`, `allocated_bytes`, and `peak_delta_bytes`
with a supported-platform tag. Unsupported/unavailable measurement stays
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

This PR is a docs-only contract repair. #10302 remains open/blocked; none of
the planned benchmark, counter, allocator, receipt, or hosted-workflow runtime
surfaces is delivered or proven here. The future implementation is a
measurement-only claim: no formatter algorithm rewrite, no production
telemetry, no execution of Perl, no release/publication surface. Counters
are operation-scoped additive hooks on the native formatter and provider;
behavior when the optional collector is unset (all current callers, tests,
goldens) is byte-identical. The counting allocator exists only in the benchmark
executable. Derived-byte proof is limited to output growth and never claims
allocation count/bytes/peak.

## Composition

Sibling spec `.spec/10301-formatter-property-fuzz-harness/` shares only
subject vocabulary; file sets are disjoint (this claim: `benches/`,
additive `src/native/` counter plumbing, crate `Cargo.toml` dev-dep,
provider proof in `crates/perl-lsp-rs-core/tests/native_pipeline_invocation_tests.rs`,
provider collector forwarding, checked-in subject registry, runtime
measurement sidecar schema, benchmark-only allocation tracker,
extractor/formatter receipt-schema fixtures and three-way join,
`.github/workflows/ci-nightly.yml` BENCH_TARGETS and strict-ID entries, canary
test file).
Nightly workflow policy comments (#3979) remain authoritative for the
benchmark job.

## Rollback

Single revert: remove the criterion dev-dep, benches dir, counter collector
plumbing (callers never read it), provider/canary tests, subject-identity
registry, runtime-sidecar writer, benchmark-only allocation tracker, and
fail-closed receipt-schema join, and the BENCH_TARGETS/strict-ID entries.
Baselines remain untouched; no tracked artifact regeneration.
