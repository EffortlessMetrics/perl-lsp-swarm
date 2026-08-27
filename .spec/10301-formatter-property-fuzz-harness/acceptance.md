# Acceptance: #10301 formatter property/fuzz harness

Each row binds one stable proposition to its discriminating executable proof.
Proof lives in `crates/perl-lsp-perltidy/tests/formatter_property_harness_tests.rs`
with one test-only invariant core under
`crates/perl-lsp-perltidy/tests/support/formatter_property_harness/`.
Proptest supplies deterministic structure-aware generation, shrinking, and
replay for focused, scheduled, and release profiles; no public feature, product
module, extra crate, or cargo-fuzz adapter is part of the claim. Every
first-party entry point is fallible and carries typed case identity without new
panic-family, unchecked-indexing, or unsafe exceptions.

| Row | Proposition | Proof | Status |
| --- | --- | --- | --- |
| FPH-001 | Every admitted construct family is a registry variant and every variant carries at least one generator/mutator disposition; promoting a family without a disposition fails the suite | `every_admitted_family_has_a_registered_disposition` (mutation control: deleting any single registry disposition turns this red) | offline |
| FPH-002 | Two runs of the same seed/case through fresh formatter contexts produce identical disposition, reason, identity (content digest + config fingerprint), change summary, and edit-plan digest | `two_fresh_runs_are_identical_typed_outcomes` | offline |
| FPH-003 | For every `Applied` outcome, the complete plan applied via independent `apply_edits_exact` equals the rendered bytes exactly, edits are ordered, pairwise non-overlapping, and contained in the requested target or its exactly recorded widening | `applied_plan_independently_applies_to_rendered_bytes`, `applied_edits_are_ordered_nonoverlapping_and_target_contained` | offline |
| FPH-004 | Second pass on the formatted output from a fresh context is a legitimate `NoChange` (`AlreadyFormatted`) with zero edits — idempotence | `second_pass_is_legitimate_nochange` | offline |
| FPH-005 | `Refused`/`FailedOrNotProven` outcomes never carry an applied or partial plan: `edits` is empty and the reason class is one of the stable refusal codes; deliberately invalid/recovered generated source maps only to typed refusals | `refusals_carry_no_plan_and_exact_reason_class` | offline |
| FPH-006 | Under `FinalNewline::Preserve`, LF/CRLF/bare-CR/mixed conventions are preserved. Under `Insert` or `Trim`, only the requested terminal-newline policy delta is permitted and evidence reports `ChangedByFormatter`; the unaffected body convention remains unchanged. Every emitted UTF-16 range is valid for the exact subject geometry (#8048 rules) | `line_endings_and_utf16_geometry_survive_variants` (negative control: treating `Insert` on terminal CRLF as `Preserved` turns the test red) | offline |
| FPH-007 | Generation is bounded and receipted: the focused profile runs one deterministic exemplar for every admitted family plus exactly 64 generated cases from a programmatic fixed seed and one test thread; each case record carries generator schema/version, seed, source digest, target, profile fingerprint, admitted families, and replay identity. `FPH_SEED=<u64>` reproduces the same normalized cases and receipt; no wall-clock assertion exists in the checker | `generated_case_receipt_is_deterministic_bounded_and_replayable` | offline |
| FPH-008 | Dormant invariant slots fail closed: cancellation/budget interruption, structural preservation beyond parse success, and protected-region hash families exist as registered dispositions that report `not_proven` on today's tree instead of passing vacuously | `dormant_invariants_report_not_proven_until_dependencies_land` (flip: they turn into real assertions when #7140/#7101/#7104/#8146 mechanisms land) | offline |
| FPH-009 | The harness never reuses production edit application or substitutes an external oracle: the checker, strategies, orchestration, and policy test must not reference `PerlTidyFormatter`, spawn a process, execute generated Perl, use the producer's own expected-byte derivation, or add panic-family/unchecked-indexing/unsafe exceptions | `fph_policy_pins` in `tests/formatter_property_harness_tests.rs` scans the complete planned harness surface for `unwrap`, `expect`, panic/assert/todo/unimplemented/unreachable/debug macros, unchecked indexing/slicing, and `unsafe`, in addition to the oracle/process bans; repository panic/unsafe and cargo-allow no-new ratchets provide independent backstops | offline |
| FPH-010 | Every counterexample shrinks through the same proptest strategy into both its persisted replay identity and a readable focused regression test before or with the fix; focused, scheduled, and release profiles use distinct bounded seed/case/shrink budgets and normalized receipt schemas | `persisted_counterexample_replays_and_matches_focused_regression`, `execution_profiles_are_distinct_bounded_and_receipted`; mutation controls remove either the persistence row or focused regression and require the proof to fail | offline |

## Mutation controls (must stay red if reintroduced)

- Reuse of the production edit applicator for expected-byte construction → FPH-009
- Any partial plan escaping a refusal/failure/cancellation-shaped outcome → FPH-005
- Overlapping or target-escaping edits silently normalized → FPH-003
- Second-pass instability accepted as `Applied` → FPH-004
- New promoted family without generator disposition → FPH-001
- Non-deterministic seed/order dependence changing normalized receipts → FPH-007
- Random-byte rejection-dominant generation replacing structured subjects → FPH-001/FPH-007
- Generated Perl executed anywhere in the harness → FPH-009 (source-text pin bans process spawning)
- Panic-family, unchecked-indexing, or unsafe syntax added anywhere in the
  harness surface → FPH-009 source pin, with repository no-new ratchets as
  independent backstops
- A minimized counterexample discarded, made non-replayable, or omitted from
  its readable focused regression → FPH-010
- Focused/scheduled/release profiles sharing an unbounded or indistinguishable
  budget/receipt identity → FPH-007/FPH-010
- Wall-clock thresholds standing in for bounded work → FPH-007

## Non-proof residuals (named, not silently dropped)

- Scheduled/release workflow wiring and receipt consumption by #7147/#9749
  stay governed by those issues' consumers; this claim ships the three bounded
  profiles and normalized receipt producer without adding a workflow.
- Real statistical depth per family and crash-cluster triage workflow are
  runtime operations, not unit proof.
- Ordinary PR proof runs only the deterministic focused profile. Scheduled and
  release consumers use the larger explicit case/shrink profiles, publish the
  normalized receipt, and report timeout/missing/instrument failure as
  `NOT_PROVEN`. Enabling either consumer requires a measured LEM projection and
  owns any workflow change; this spec does not add a workflow.
- Trivia/opaque hashing (#7111/#7120), structural-preservation oracle
  (#8146), and cancellation checkpoints (#7140) convert FPH-008 dormancies;
  landing them is owned by those issues, not this spec.
