# Acceptance: #10301 formatter property/fuzz harness

Each row binds one stable proposition to its discriminating executable proof.
Proof lives in `crates/perl-lsp-perltidy/tests/formatter_property_harness_tests.rs`
(generators and test-only orchestration) plus the feature-gated shared invariant
core at `crates/perl-lsp-perltidy/src/formatter_property_harness/`, consumed by
both the package tests and `fuzz/fuzz_targets/perl_tidy_formatter.rs`.
The package tests and fuzz path both enable the named
`formatter-property-harness` feature; the fuzz target is an adapter, not a
second checker. No test-only `tests/support/` module is the sharing boundary.

| Row | Proposition | Proof | Status |
| --- | --- | --- | --- |
| FPH-001 | Every admitted construct family is a registry variant and every variant carries at least one generator/mutator disposition; promoting a family without a disposition fails the suite | `every_admitted_family_has_a_registered_disposition` (mutation control: deleting any single registry disposition turns this red) | offline |
| FPH-002 | Two runs of the same seed/case through fresh formatter contexts produce identical disposition, reason, identity (content digest + config fingerprint), change summary, and edit-plan digest | `two_fresh_runs_are_identical_typed_outcomes` | offline |
| FPH-003 | For every `Applied` outcome, the complete plan applied via independent `apply_edits_exact` equals the rendered bytes exactly, edits are ordered, pairwise non-overlapping, and contained in the requested target or its exactly recorded widening | `applied_plan_independently_applies_to_rendered_bytes`, `applied_edits_are_ordered_nonoverlapping_and_target_contained` | offline |
| FPH-004 | Second pass on the formatted output from a fresh context is a legitimate `NoChange` (`AlreadyFormatted`) with zero edits — idempotence | `second_pass_is_legitimate_nochange` | offline |
| FPH-005 | `Refused`/`FailedOrNotProven` outcomes never carry an applied or partial plan: `edits` is empty and the reason class is one of the stable refusal codes; deliberately invalid/recovered generated source maps only to typed refusals | `refusals_carry_no_plan_and_exact_reason_class` | offline |
| FPH-006 | Line-ending conventions are preserved across LF/CRLF/bare-CR/mixed variants (`FormatSafetyEvidence.line_endings == Preserved` whenever input parses) and every emitted UTF-16 range is valid for the exact subject geometry (#8048 rules) | `line_endings_and_utf16_geometry_survive_variants` | offline |
| FPH-007 | Generation is bounded and receipted: case record carries generator schema/version, seed, source digest, target, profile fingerprint, admitted families; identical inputs produce an identical normalized receipt; no wall-clock assertion exists in the checker | `generated_case_receipt_is_deterministic_and_bounded` | offline |
| FPH-008 | Dormant invariant slots fail closed: cancellation/budget interruption, structural preservation beyond parse success, and protected-region hash families exist as registered dispositions that report `not_proven` on today's tree instead of passing vacuously | `dormant_invariants_report_not_proven_until_dependencies_land` (flip: they turn into real assertions when #7140/#7101/#7104/#8146 mechanisms land) | offline |
| FPH-009 | The harness never reuses production edit application or oracle substitution: it must not reference `PerlTidyFormatter`, subprocess adapters, or apply its expected bytes using the producer's own derivation path | `harness_module_does_not_reference_external_oracle` (source-text pin over the harness module, house policy-pin pattern) | offline |
| FPH-010 | A cargo-fuzz target drives the same feature-gated invariant core from structured byte mutations and is declared in `fuzz/Cargo.toml` (adding the missing `perl-lsp-perltidy` path dependency with `formatter-property-harness` enabled); minimized crashes land as committed focused regressions under the crate's regression-file convention | manifest/source structural pins in `fph_policy_pins` require the shared `src/formatter_property_harness/` boundary, feature wiring in both consumers, and no duplicate checker; minimization is demonstrated by one checked-in `.proptest-regressions` entry wire format compatible with `crates/perl-lexer/tests/lexer_robustness_tests.proptest-regressions` | offline |

## Mutation controls (must stay red if reintroduced)

- Reuse of the production edit applicator for expected-byte construction → FPH-009
- Any partial plan escaping a refusal/failure/cancellation-shaped outcome → FPH-005
- Overlapping or target-escaping edits silently normalized → FPH-003
- Second-pass instability accepted as `Applied` → FPH-004
- New promoted family without generator disposition → FPH-001
- Non-deterministic seed/order dependence changing normalized receipts → FPH-007
- Random-byte rejection-dominant generation replacing structured subjects → FPH-001/FPH-007
- Generated Perl executed anywhere in the harness → FPH-009 (source-text pin bans process spawning)
- Wall-clock thresholds standing in for bounded work → FPH-007

## Non-proof residuals (named, not silently dropped)

- Scheduled-tier seed/time budgets and release-tier receipt consumption by
  #7147/#9749 stay governed by those issues' consumers; this claim ships the
  deterministic PR/focused tier plus the wired-but-dormant schedule hooks.
- Real statistical depth per family and crash-cluster triage workflow are
  runtime operations, not unit proof.
- Trivia/opaque hashing (#7111/#7120), structural-preservation oracle
  (#8146), and cancellation checkpoints (#7140) convert FPH-008 dormancies;
  landing them is owned by those issues, not this spec.
