# Acceptance Criteria: #17237 — row-owned predicate discriminator participation facts

Spec-only. Every criterion below is discharged by the implementing PR; this PR
discharges only S1–S3.

## §Behavior

One fact per exact related-test **row** (change + logical owner + relation/call +
test/subtest + changed discriminator). `participation` is one of:

| Input / Condition | Participation | Notes |
|---|---|---|
| Admitted exact value makes old and new predicate outcomes differ (`>=`→`>` at 100, input `100`) | `Distinguishes` | Static source/value fact; not proof the test ran |
| Every admitted exact value represented by the row gives the same old/new outcome (inputs `101`, `99`) | `SameOutcome` | Needs the whole admitted set, never one member |
| Discrimination is exact only under a narrower explicit value/config/profile/platform/test-selection scope | `Scoped` | Cannot satisfy a broader downstream claim |
| Dynamic, partial, mixed-outcome, unsupported, stale, ambiguous, recovered, over-budget or unbound input | `Unknown` | Default. Never rewritten to `SameOutcome` |
| Call is separately impossible (#16436) or framework-skipped (#16438) | participation unchanged | Separate planes; the fact may exist for such a call |

`old_outcome` / `new_outcome` are present only when statically exact.

## §Hazards

| Class | Invariant | Surface | Required adversarial test |
|---|---|---|---|
| ID/ref-space collision | Fact identity binds project/root, generation, change, logical owner, relation/call, test/subtest, bindings, value witnesses, discriminator, producer/schema; equal rendered argument text never aliases two rows | fact identity construction | `f7_same_named_owner_other_root`, `f11_equal_text_different_callsite` |
| Bounds/overflow | N/A — no client-supplied indices; value witnesses are finite sets with an explicit admitted-set cap, over cap → `Unknown` | witness set construction | `f12_over_budget_value_set_is_unknown` |
| Protocol-safety | Missing, stale or malformed binding/value input yields `Unknown` with a typed limitation, never an empty or negative result | v2 projection | `f9_missing_binding_is_unknown` |
| Scanner literal/comment blindness | The exporter must not scan source text for predicates or inputs; all evidence comes from canonical facts | exporter | `f10_no_text_scan_in_exporter` (static check that the fact module imports no parser/lexer and reads no source) |
| Test-encodes-the-bug | Expected values come from the evaluator over canonical facts, not copied from the implementation's output; the fixture matrix is hand-derived | fixture goldens | review row: each golden carries its derivation |
| Coverage/measurement integrity | N/A — no coverage transform |  |  |

Subsystem defaults consulted: SUBSYSTEM_HAZARD_DEFAULTS.md has no RIPR-facts
section; rows above were derived from SPEC_UPDATE_CHECKLIST §8.

## §Contracts

| Contract | Source | How satisfied |
|---|---|---|
| Fail-closed actionability | ripr-swarm CLAUDE.md "Evidence and actionability" | `Unknown` default; `SameOutcome` only on whole admitted set; downstream `missing_input` requires `SameOutcome` + exact row ownership + no stronger limitation |
| Separate planes | #17237 semantic law | relation, activation, framework, participation, oracle relevance stay independent facts |
| No second binder / evaluator | #17237 inputs | consumes #7435 bindings and #9285 discriminator only; no source rescan |
| v2 wire and determinism | #9274 / #9276 | projected through the v2 contract; identity stable under packet ordering |
| `ripr-perl-facts-v1` unchanged | `fixtures/perl-verdict-corpus` pin | no change to v1 output; the fact is v2-only |

## §API-Shape

Names are implementation latitude; the semantic split is not.

| Item | Kind | Shape | Dup-risk (grep result) | Caller count |
|---|---|---|---|---|
| participation fact | struct | change/owner identity, relation/call/test/subtest identity, discriminator identity, binding refs, witness refs, `participation`, optional `old_outcome`/`new_outcome`, source/root/config/profile/generation identity, provenance/confidence/completeness/limitations | no type named `SameOutcome` or `Distinguishes` in `crates` (doc-comment prose only) | 0 (new) |
| `participation` | enum | `Distinguishes \| SameOutcome \| Scoped \| Unknown` | none found | 0 (new) |
| existing `discriminators` array | emitted array | test-side expected values from `is(...)`; **not touched** | exists in `emitter/relations.rs` | n/a |

## §Test-Grid

Falsifiers first (S2): these are written red before any positive path.

| # | Scenario | Kind | Test name | Invariant discharged |
|---|---|---|---|---|
| F1 | exact call + input `101` on a 100 boundary emits `Distinguishes` | negative | `f1_above_boundary_is_not_distinguishes` | no false `Distinguishes` |
| F2 | exact call + input `100` emits `SameOutcome` | negative | `f2_boundary_input_is_not_same_outcome` | no false `SameOutcome` |
| F3 | activation `Possible` treated as participation | negative | `f3_activation_possible_is_not_participation` | planes separate |
| F4 | activation `Impossible` rewritten as `SameOutcome` | negative | `f4_activation_impossible_is_not_same_outcome` | planes separate |
| F5 | one member of a mixed finite set makes the row exact | adversarial | `f5_mixed_set_never_first_member_exact` | whole-set rule |
| F6 | argument/parameter chosen by order or name, not canonical IDs | adversarial | `f6_binding_by_canonical_id_only` | identity |
| F7 | same-named owner/parameter in another package/root satisfies the fact | adversarial | `f7_same_named_owner_other_root` | identity |
| F8 | stale caller, callee, change, value or root generation stays current | state | `f8_stale_generation_is_unknown` | currentness |
| F9 | missing argument binding becomes an empty/negative result | negative | `f9_missing_binding_is_unknown` | fail closed |
| F10 | a text scan or private evaluator appears in the exporter | adversarial | `f10_no_text_scan_in_exporter` | no second authority |

Fixture matrix (positive and control), each with a hand-derived expectation:
`>=`→`>` with exact below/boundary/above inputs; `==`→`!=` with equal/non-equal
inputs; finite set all same-outcome; finite set mixed → `Unknown` or reviewed
`Scoped`; direct literal and canonical local-constant argument; two related
tests with different participation; one test with two calls carrying different
inputs; stale/mixed generation; ambiguous or dynamic call target; unsupported
binding mode; impossible activation and framework skip as separate-plane
controls; deterministic shuffled fact/input order.

## §Blast-Radius

| Consumer | Crate | Dependency type | Impact | Required update |
|---|---|---|---|---|
| ripr consumer (`missing_input`) | ripr-swarm | v2 packet reader | none until the implementing PR | ripr-swarm#5562 |
| `emit_relations_and_discriminators` | `perl-ripr-facts` | adjacent emitter | none — separate array | none |
| v2 schema | `perl-ripr-facts` | generated schema | none in this PR | #9276 gates the implementing PR |

Must-not-touch boundary in this PR: everything outside `.spec/17237-*`.

## Spec-PR acceptance (this PR)

- [ ] S1: `context.md`, `acceptance.md`, `checklist.md` present and consistent with #17237.
- [ ] S2: all ten falsifiers named with test names and the invariant each discharges.
- [ ] S3: input contract for #7435 and #9285 stated as field-level needs (checklist §Input contract).
