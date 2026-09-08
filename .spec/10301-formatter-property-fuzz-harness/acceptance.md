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

Implementation is blocked until #10237 and #10239 land the canonical
byte-native source/request/result/edit-plan seam and #7138 lands the applicable
strict plan/application authority. Current UTF-16 DTOs and
`native::edit_application` are research witnesses, not build targets.

| Row | Proposition | Proof | Status |
| --- | --- | --- | --- |
| FPH-001 | Every admitted construct family is a registry variant and every variant carries at least one generator/mutator disposition; promoting a family without a disposition fails the suite | `every_admitted_family_has_a_registered_disposition` (mutation control: deleting any single registry disposition turns this red) | offline |
| FPH-002 | Two runs of the same case through fresh formatter contexts produce identical disposition, reason, exact source/configuration identity, requested/admitted/widened targets, change summary, and byte-edit-plan digest | `two_fresh_runs_are_identical_typed_outcomes` | blocked: #10237/#10239 |
| FPH-003 | For every `Applied` outcome, a test-owned strict byte-plan applicator that shares no production mapping, clamping, construction, or application code reproduces the rendered bytes exactly; edits are ordered, pairwise non-overlapping, exact-source-bound, and contained in the admitted target including explicit widening provenance | `applied_plan_independently_applies_to_rendered_bytes`, `applied_edits_are_ordered_nonoverlapping_and_target_contained` | blocked: #10237/#10239/#7138 |
| FPH-004 | Second pass on the formatted output from a fresh context is a legitimate `NoChange` (`AlreadyFormatted`) with zero edits — idempotence | `second_pass_is_legitimate_nochange` | blocked: #10237/#10239 |
| FPH-005 | `Refused`/`FailedOrNotProven` outcomes never carry an applied or partial plan: `edits` is empty and the reason class is one of the stable refusal codes; deliberately invalid/recovered generated source maps only to typed refusals | `refusals_carry_no_plan_and_exact_reason_class` | blocked: #10237/#10239 |
| FPH-006 | Under `FinalNewline::Preserve`, LF/CRLF/bare-CR/mixed conventions are preserved. Under `Insert` or `Trim`, only the requested terminal-newline policy delta is permitted and evidence reports `ChangedByFormatter`; the unaffected body convention remains unchanged. Every emitted byte range belongs to the exact predecessor and adapter projection is outside this oracle | `line_endings_and_byte_geometry_survive_variants` (negative control: treating `Insert` on terminal CRLF as `Preserved` turns the test red) | blocked: #10237/#10239/#7138 |
| FPH-007 | Generation is bounded and receipted. RNG identity is `proptest 1.11.0` from the locked graph, `RngAlgorithm::ChaCha`, domain-separated 32-byte seed derivation `formatter-property-harness.v1`, and a versioned strategy fingerprint. The canonical corpus is ordered admitted-family exemplars plus checked-in minimized entries. Focused evaluates every exemplar once, every persisted entry once, and exactly 64 novel cases; scheduled/release replay exemplars and persistence once per profile, then run 16×256 / 64×256 novel cases. Receipts carry separate counts and ordered digests for exemplar, persisted-replay, novel, and total evaluations. The same seed set + corpus/strategy/lock identities reproduces the same ordered cases and receipt | `generated_case_receipt_is_deterministic_bounded_and_replayable` | offline |
| FPH-008 | Mandatory invariant rows use `pass | fail | not_proven`; profile aggregation is `fail` if any row fails, otherwise `not_proven` if any mandatory row is dormant/missing/stale/timed-out/crashed/instrument-failed, otherwise `pass`. A non-`pass` profile writes its receipt and makes the producer command non-zero; a green Cargo test cannot bless missing cancellation, structural-preservation, or protected-region evidence | `mandatory_not_proven_prevents_profile_pass`, `profile_result_lattice_is_fail_closed` (flip: the live profile becomes pass-capable only when #7140/#7101/#7104/#7111/#7120/#8146 mechanisms land) | blocked: #7140/#7101/#7104/#7111/#7120/#8146 |
| FPH-009 | The harness never reuses production edit application or substitutes an external oracle. `fph_policy_pins` recursively inventories the exact allowed support tree and scans every crate `src/**/*.rs` and `tests/**/*.rs` for harness ownership markers/checker definitions; any unlisted support file or second definition is red. It also bans process/Perl execution, producer-side expected-byte derivation, panic-family/unchecked-indexing/unsafe exceptions, and verifies the producer/validator entry points | `fph_policy_pins`, `adding_unlisted_harness_module_fails_inventory`, `adding_second_checker_fails_uniqueness` | offline |
| FPH-010 | Every counterexample shrinks through the pinned strategy into both its persisted replay identity and a readable focused regression before or with the fix; adding, removing, or reordering a persisted entry changes the corpus digest. Novel runners disable implicit persistence replay because the producer replays the canonical persistence set exactly once per profile; a fallible canonical writer atomically records a newly minimized case. Focused, scheduled, and release profiles have distinct bounded seed/case/shrink budgets and normalized receipt identities | `persisted_counterexample_replays_and_matches_focused_regression`, `execution_profiles_are_distinct_bounded_and_receipted` | offline |

## Profile result and consumer contract

`formatter_property_harness.v1` records every mandatory row plus aggregate
`pass | fail | not_proven`, exact candidate when required, profile, locked
proptest identity, RNG/seed-derivation/strategy identities, corpus identity,
and the four evaluation counts/digests. Focused, scheduled, and release invoke
the same test-owned producer. A separate test-owned validator checks schema,
candidate, profile, aggregate result, counts, and digests without trusting the
producer's exit status. `fail` and `not_proven` both exit non-zero after an
atomic receipt write. Neither can close #10301 or satisfy #7147/#9749.

## Mutation controls (must stay red if reintroduced)

- Reuse of the production edit applicator for expected-byte construction → FPH-009
- Any partial plan escaping a refusal/failure/cancellation-shaped outcome → FPH-005
- Overlapping or target-escaping edits silently normalized → FPH-003
- Second-pass instability accepted as `Applied` → FPH-004
- New promoted family without generator disposition → FPH-001
- Non-deterministic seed/order dependence changing normalized receipts → FPH-007
- RNG, locked proptest, seed derivation, strategy, corpus, or cardinality drift
  without a receipt identity change → FPH-007
- Random-byte rejection-dominant generation replacing structured subjects → FPH-001/FPH-007
- Generated Perl executed anywhere in the harness → FPH-009 (source-text pin bans process spawning)
- An unlisted support file or second checker added outside the scanned module →
  FPH-009
- Panic-family, unchecked-indexing, or unsafe syntax added anywhere in the
  harness surface → FPH-009 source pin, with repository no-new ratchets as
  independent backstops
- A minimized counterexample discarded, made non-replayable, or omitted from
  its readable focused regression → FPH-010
- Focused/scheduled/release profiles sharing an unbounded or indistinguishable
  budget/receipt identity → FPH-007/FPH-010
- Wall-clock thresholds standing in for bounded work → FPH-007
- Any mandatory `not_proven` row producing aggregate `pass` or zero exit →
  FPH-008
- Producer receipt accepted without the independent schema/candidate/profile/
  aggregate/count/digest validator → FPH-009

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
  until they land the aggregate is `not_proven` and #10301 cannot close.
- #10237, #10239, then the applicable #7138 plan/application authority are
  prerequisite wake events. No implementation may bind this harness to the
  transitional UTF-16 DTOs or production-unwired `native::edit_application`.
