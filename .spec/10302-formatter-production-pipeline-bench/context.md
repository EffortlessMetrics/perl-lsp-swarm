# Context: #10302 — production-path performance receipt for the native formatter pipeline

## Problem

The nightly benchmark program measures 14 declared Criterion targets across
eight crates (`ci-nightly.yml` `BENCH_TARGETS`), and **none of them touches
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
  `native_document_decision` (:257, one `format_document_typed`) and
  `native_range_decision` (:273, one `format_range_typed`) per LSP request.
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

## Approach chosen (deterministic work-counter instrument + counter-shape canaries; timing stays advisory)

Ranked against the issue's required instrumentation using only in-tree
evidence:

1. **Chosen — versioned `NativePipelineCounters` populated on the real
   classify/format path + Criterion subject benches + plain-test counter
   canaries.** Additive, zero-cost when unset: an optional metrics collector
   hangs off the typed call path (`format_document_typed` /
   `format_range_typed`) recording parse-gate invocations, tokens/nodes
   observed by the gate, lines processed, delimited groups fitted, edits
   derived, replacement bytes, peak depth, and total elapsed under a named
   clock tag — extending `FormatChangeSummary`'s precedent rather than
   inventing a parallel formatter. Benches live in
   `crates/perl-lsp-perltidy/benches/native_pipeline_benchmark.rs`
   over checked-in scaling fixtures (small/medium/large ×
   delimited/statement/opaque/refusal/no-change rows × LF/CRLF/bare-CR ×
   tabs/spaces/width boundaries) whose size ratios expose superlinear
   growth without dangerous allocations. The PR-blocking discriminator is a
   plain deterministic test file asserting (a) single pipeline invocation
   per request at the provider seam, (b) linear-or-bounded counter ratios
   across N/2N/4N scaling steps, (c) refusal/opaque rows stay cost-bounded.
   Nightly enrollment = one `BENCH_TARGETS` entry, so receipts, reports,
   alerts, and baseline comparison keep flowing unmodified.
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

**Anti-masking clause:** no check introduced by this claim may be satisfied
by increasing any timeout, budget constant, size cap, or iteration bound;
envelope movement requires a major bump of the versioned counter schema plus
exact before/after counter receipts. A red canary is repaired by repairing
work, never by relaxing bounds.

## Claim boundary

Measurement-only claim: no formatter algorithm rewrite, no production
telemetry, no execution of Perl, no release/publication surface. Counters
are thin additive hooks on existing functions; behavior when the collector
is unset (all current callers, tests, goldens) is byte-identical.

## Composition

Sibling spec `.spec/10301-formatter-property-fuzz-harness/` shares only
subject vocabulary; file sets are disjoint (this claim: `benches/`,
additive `src/native/` counter plumbing, crate `Cargo.toml` dev-dep,
`.github/workflows/ci-nightly.yml` BENCH_TARGETS entry, canary test file).
Nightly workflow policy comments (#3979) remain authoritative for the
benchmark job.

## Rollback

Single revert: remove the criterion dev-dep, benches dir, counter collector
plumbing (callers never read it), canary test file, and the one
`BENCH_TARGETS` line. Baselines/receipts scripts untouched; no tracked
artifact regeneration.
