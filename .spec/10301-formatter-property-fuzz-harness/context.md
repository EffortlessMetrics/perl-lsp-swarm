# Context: #10301 — structure-aware property/fuzz harness for formatter safety invariants

## Problem

Formatter safety today rests on focused goldens
(`crates/perl-lsp-perltidy/tests/fixtures/native_formatter/*.pl` +
`.expected.pl`) and the edit-application equivalence test
(`tests/edit_application_equivalence_tests.rs`). Every failure class named by
the issue lives *between* those fixtures, and nothing in the tree generates
subjects there:

- zero property/fuzz surface over the formatter: `fuzz/` (`perl-parser-fuzz`)
  declares 21 targets (substitution_parsing … pod_extraction) and its
  `Cargo.toml` does not even depend on `perl-lsp-perltidy`;
- `crates/perl-lsp-perltidy/Cargo.toml` dev-deps are only
  `perl-tdd-support`, `serde_json`, `tempfile` — no `proptest`, no generator;
- random-byte fuzzing is structurally useless here: the parse gate
  (`NativeFormatter::validate_clean_parse`,
  `src/native/implementation.rs:56-96`) refuses non-clean-parse input via
  `native.format.parse_error` / `native.format.parse_incomplete` /
  `native.format.literal_preserve_region`, so raw bytes would mostly measure
  parser rejection. External Perl::Tidy differential output is not a native
  correctness oracle either (subprocess adapter path only).

## Governing evidence (2026-08-27, origin/main@e6e956461534b2566c735696f289e0915e2cb189)

This pin is a research receipt, not an implementation authority. The implementer
must rerun the count, dependency, API, and source-location checks against the
implementation branch's merge base before writing code. Any mismatch or missing
source leaves the affected proposition `NOT_PROVEN` until this bundle is updated.

The canonical consumer APIs a harness must bind already exist and are typed:

- `PerlFormatter::{format_document, format_range} -> FormatResult`
  {`formatted`, `edits: Vec<TextEdit>`, `changed`, `diagnostics`}
  (`src/native/result.rs:129-138`);
- `NativeFormatter::{format_document_typed, format_range_typed}`
  (`src/native/outcome.rs:216-243`) → `FormatOutcome` with exhaustive
  `FormatDisposition` {Applied, NoChange, Refused, FailedOrNotProven},
  stable `FormatReasonCode`, `FormatIdentity`
  {content_digest, config_fingerprint, actual_engine}, `FormatChangeSummary`,
  and `FormatSafetyEvidence` {parse_before, parse_after,
  literal_preservation, utf8, line_endings};
- an independent byte-edit applicator exists:
  `apply_edits_exact` + `EditSpec`/`PositionEncoding`
  (`src/native/edit_application.rs:31-144`) — the "independent application ==
  rendered bytes" invariant can be checked without reusing production edit
  derivation;
- refusal taxonomy constants are stable string codes
  (`PARSE_ERROR_CODE`, `PARSE_INCOMPLETE_CODE`, `UNSAFE_RANGE_CODE`,
  `PARSE_PRESERVATION_CODE`, `LITERAL_PRESERVE_CODE`);
- `proptest = "1.11.0"` is already a workspace dependency (root
  `Cargo.toml:364`) with an in-repo minimization precedent
  (`crates/perl-lexer/tests/lexer_robustness_tests.proptest-regressions`);
- `FormatConfig` exposes mutation axes: mode
  (Native/Compat/ExternalLegacy/Off), keyword spacing, brace placement,
  trailing commas, final-newline policy; range targets exercise
  `UnsafeRange`/widening paths.

Honest capability boundary on today's tree (gates what this harness may claim):

- cooperative cancellation / resource checkpoints **do not exist** (#7140
  OPEN): no format entry point takes a cancel/budget input; the only
  budget-adjacent boundary is parser `terminated_early()` →
  `parse_incomplete` refusal (`implementation.rs:69-79`);
- trivia/opaque byte geometry (#7101/#7104) and structural preservation
  beyond parse success (#8146) are OPEN — protected-region hashing beyond
  whole-literal refusal (`literal_preserve_region`) is not consumable yet;
- the representative breadth corpus (#9327) is OPEN with zero tree
  references, so generator families must come from the formatter's own
  admitted safe-subset registry, not from #9327 identities.

## Approach chosen (deterministic seedable property harness inside the crate's tests tree)

Ranked against the issue's generator-model requirements using only in-tree
evidence:

1. **Chosen — proptest-driven structure-aware property/fuzz harness inside the
   crate's integration-test tree.** One checker, admitted-family registry,
   strategies, and receipt types live under
   `tests/support/formatter_property_harness/`; the integration test owns only
   orchestration and acceptance/mutation tests. The checker consumes only
   canonical production APIs
   (`format_*_typed`, `apply_edits_exact`) twice per case from fresh
   formatter contexts, and enforces: deterministic outcome, Applied ⇒ exact
   plan equality after independent application + ordered non-overlapping
   target-contained edits, second pass == legitimate NoChange, refusal ⇒
   empty plan + exact reason class, bounded generation, normalized receipts.
   The checker is fallible and returns typed case identity on failure; the
   support module and integration test add no panic-family,
   unchecked-indexing, or unsafe exceptions.
   Admitted construct families become a data registry where every family
   variant requires ≥1 generator/mutator disposition — promoting a family
   without registering one fails the suite.
   Matches issue sections Generator model / Mandatory invariant checker /
   Initial property families / Negative controls.
2. Cargo-fuzz adapter — rejected for this claim. The current libFuzzer target
   contract can keep or reject a non-crashing input, but a logical invariant
   `Err` becomes a finding only through a panic/abort crash bridge. That would
   add the exact panic-shaped exception this repository's test policy forbids;
   silently rejecting the `Err` would make the invariant false-green. The issue
   requires a structure-aware property/fuzz harness, not cargo-fuzz specifically,
   and proptest supplies generation, shrinking, deterministic replay, and all
   three bounded execution tiers without that exception.
3. Random-byte libFuzzer-only — rejected: measures parse-gate rejection, the
   issue's first named negative control.
4. Perl::Tidy differential oracle — rejected: wrong authority, subprocess
   nondeterminism; banned by the issue.
5. Full cancellation/budget property family now — rejected as gated: the
   invariants are pre-wired but dormant, reporting `not_proven` until #7140
   lands checkpoint inputs; claiming them green today would be fabricated
   coverage.

## Seed, shrink, replay, and CI-economics contract

- The focused profile runs one deterministic exemplar for every admitted
  family plus exactly 64 generated cases selected by a programmatic fixed
  `FPH_SEED`, on one test thread. Seed, generator schema, admitted
  families, source digest, target, configuration fingerprint, and invariant
  outcomes are part of the normalized receipt; the same seed/profile must
  regenerate the same ordered cases and receipt.
- Proptest shrink persistence uses
  `tests/formatter_property_harness_tests.proptest-regressions`. Every persisted
  counterexample is paired with a readable named Rust regression test; the
  replay identity binds its minimized input, generator schema, seed, source and
  configuration digests, target, invariant, and focused-test name. The ordered
  admitted-family exemplars plus checked-in minimized entries form the
  canonical corpus. Every profile receipt binds a corpus schema/version and
  digest, so corpus addition, removal, or reordering cannot masquerade as a
  replay of the same input set.
- Focused runs every-family exemplars plus 64 generated cases with at most
  1,024 shrink iterations. Scheduled runs 16 fixed seeds × 256 cases with at
  most 4,096 shrink iterations. Release runs 64 fixed seeds × 256 cases with
  at most 4,096 shrink iterations and records the supplied exact candidate,
  profile, and schema. These are work-count limits, not product latency claims.
  Schema `formatter_property_harness.v1` is written atomically at
  `target/formatter-property-harness/<profile>/receipt.json` with stable row
  ordering and no timestamp or absolute path. Scheduled and release consumers
  retain it; release consumers validate the required caller-supplied candidate
  binding rather than asking the harness to spawn Git.
  Before either is enabled, its owner records a measured LEM projection and
  routing rationale. Missing, timed-out, crashed, stale-schema, or
  instrument-failed evidence is `NOT_PROVEN`, never pass.

## Claim boundary

Instrument-only claim: no production formatter algorithm change, no new
admitted syntax, no fixture corpus beyond minimized counterexamples plus the
family registry table. Properties whose oracles do not exist on today's tree
(structural preservation per #8146, protected/trivia hashes per
#7101/#7104/#7111/#7120, cancellation/budget interruption per #7140) are
registered dispositions that fail closed as `not_proven`; they become real
assertions when their owning issues land their in-tree mechanisms.
The harness support module and test remain panic-free, safe Rust and fallible;
there is no cargo-allow or test carveout.

## Composition

Shares subject-fixture vocabulary with #10302 (.spec/10302-formatter-production-pipeline-bench)
but disjoint surfaces: this claim touches only perltidy test support/tests, its
proptest dev-dependency, and the lockfile if dependency wiring changes it;
#10302 touches `benches/`,
additive counters in `src/native/`, and `.github/workflows/ci-nightly.yml`.
Upstream chain issues (#7101/#7104/#8146/#10237/#10239/#7138/#7140/#9327) are
consumers of this instrument, not modified by it.

## Rollback

Single revert: delete the test support/tests and persisted regression entries,
remove the proptest dev-dependency, and restore `Cargo.lock` if it changed. The
product API and default production behavior remain untouched. No baseline or
workflow change.
