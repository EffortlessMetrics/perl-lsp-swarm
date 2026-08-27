# Context: #10301 — structure-aware property/fuzz harness for formatter safety invariants

## Problem

Formatter safety today rests on focused goldens
(`crates/perl-lsp-perltidy/tests/fixtures/native_formatter/*.pl` +
`.expected.pl`) and the edit-application equivalence test
(`tests/edit_application_equivalence_tests.rs`). Every failure class named by
the issue lives *between* those fixtures, and nothing in the tree generates
subjects there:

- zero property/fuzz surface over the formatter: `fuzz/` (`perl-parser-fuzz`)
  declares 20 targets (substitution_parsing … pod_extraction) and its
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

1. **Chosen — proptest-driven harness + shared invariant core + thin
   cargo-fuzz target.** A checked-in harness module under
   `tests/support/formatter_property_harness/` owns generators, seeds, schema
   versioning, receipts, and the mandatory invariant checker;
   `tests/formatter_property_harness_tests.rs` runs deterministic seeded
   properties in ordinary package CI; one additive fuzz target
   `fuzz/fuzz_targets/perl_tidy_formatter.rs` drives the same invariant core
   from raw bytes mapped onto structured mutations (never executed as Perl).
   The checker consumes only canonical production APIs
   (`format_*_typed`, `apply_edits_exact`) twice per case from fresh
   formatter contexts, and enforces: deterministic outcome, Applied ⇒ exact
   plan equality after independent application + ordered non-overlapping
   target-contained edits, second pass == legitimate NoChange, refusal ⇒
   empty plan + exact reason class, bounded generation, normalized receipts.
   Admitted construct families become a data registry where every family
   variant requires ≥1 generator/mutator disposition — promoting a family
   without registering one fails the suite.
   Matches issue sections Generator model / Mandatory invariant checker /
   Initial property families / Negative controls.
2. Random-byte libFuzzer-only — rejected as primary: measures parse-gate
   rejection, the issue's first named negative control.
3. Perl::Tidy differential oracle — rejected: wrong authority, subprocess
   nondeterminism; banned by the issue.
4. Full cancellation/budget property family now — rejected as gated: the
   invariants are pre-wired but dormant, reporting `not_proven` until #7140
   lands checkpoint inputs; claiming them green today would be fabricated
   coverage.

## Claim boundary

Instrument-only claim: no production formatter algorithm change, no new
admitted syntax, no fixture corpus beyond minimized counterexamples plus the
family registry table. Properties whose oracles do not exist on today's tree
(structural preservation per #8146, protected/trivia hashes per
#7101/#7104/#7111/#7120, cancellation/budget interruption per #7140) are
registered dispositions that fail closed as `not_proven`; they become real
assertions when their owning issues land their in-tree mechanisms.

## Composition

Shares subject-fixture vocabulary with #10302 (.spec/10302-formatter-production-pipeline-bench)
but disjoint surfaces: this claim touches `crates/perl-lsp-perltidy/tests/**`,
its `Cargo.toml` dev-dependencies, and `fuzz/`; #10302 touches `benches/`,
additive counters in `src/native/`, and `.github/workflows/ci-nightly.yml`.
Upstream chain issues (#7101/#7104/#8146/#10237/#10239/#7138/#7140/#9327) are
consumers of this instrument, not modified by it.

## Rollback

Single revert: delete the harness module + tests file + fuzz target entry and
the two added dev-dependency lines; production code is untouched (family
registry data may live entirely under `tests/`). No tracked-file
regeneration, no baseline movement, no workflow change.
