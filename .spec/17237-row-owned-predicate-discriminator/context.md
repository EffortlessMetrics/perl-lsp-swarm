# Context: #17237 — row-owned predicate discriminator participation facts

Parent producer programme: #4747. Train slot: RIPR-F04.
Authorities this fact consumes: #9285 (change/discriminator), #9293 (exact
relation), #7435 (call argument→parameter bindings), #9274 / #9276 (v2 wire).
Coordinates with, but never substitutes for: #16436 (activation), #16438
(framework). Downstream consumer: ripr-swarm#5562; the Perl row that needs it is
ripr-swarm#8979 (`perl-testmore-tier-far-from-boundary`).

This is a **spec-only** bundle. It settles the fact's meaning, identity,
falsifiers and input contract so the implementing PR starts from the negative
cases. It changes no production code, schema, or generated artifact.

## Problem

RIPR's `RelatedTestMiss::MissingInput` is narrower than "the test call can
execute". For a changed predicate such as `return $amount >= 100;`, a test can
have an exact static call path, be control-flow feasible, run under the
framework, and still not discriminate the change: it calls the owner with `101`,
where the old and new predicate agree. The boundary input `100` is the
load-bearing one.

No existing plane answers the row-local question *does this exact related-test
call supply inputs that make the old and new predicate outcomes differ?*:

```text
#9293   relation:   does this test call the changed owner?
#16436  activation: can this call be reached statically?
#16438  framework:  is it skipped / TODO-qualified?
#9285   change:     what predicate/discriminator changed?
#9296   relevance:  what sink does the oracle observe?
```

Without it ripr cannot truthfully emit `missing_input` for Perl, and copying a
finding-wide discriminator onto every related test would be false.

## Measured current state (perl-lsp-swarm `c4d186fe6`)

- `crates/perl-ripr-facts/src/emitter/changes.rs` infers `behavior_hint` and a
  discriminator **by scanning added-line text** (`behavior_hint_for_hunk`,
  `extract_discriminator`), and its own limitation text says
  `missing_discriminator` is always `null` in this slice. That is the
  change-plane (#9285) heuristic. It is not a canonical predicate fact and must
  not be reused as this fact's old/new evaluator.
- `emitter/relations.rs` collects parser-backed `is(...)` argument pairs
  (`collect_is_args` into `is_args`). Those are **test-side expected values**,
  not predicate participation. No wire array carries them:
  `emit_relations_and_discriminators` returns only relations and limitations,
  `is_args` is `#[allow(dead_code)]` and read only by tests since #5064, and
  `schemas/ripr-perl-facts-v1.schema.json` has no `discriminators` key. The
  field's doc says it is retained as input for "the discriminator work", so the
  name clash is the review hazard: this spec calls the new fact *participation*
  and does not build it from `is_args`.
- No canonical call argument→parameter binding, parameter entity, or value
  witness exists in code: #7435, #10238, #7423, #10244, #5242 are all open, and
  `grep ParameterEntityId|CallArgumentBinding crates` finds nothing.

So the positive path cannot be implemented yet without a second binder or a
private evaluator, which this issue forbids. Hence spec first.

## Why this approach

- Fix the meaning and the failure direction first (`Unknown` is the default;
  `SameOutcome` needs every admitted value). A wrong `SameOutcome` becomes a
  wrong actionable `missing_input` downstream, which is worse than missed
  advisories.
- Write the ten falsifiers as named cases now, so the implementing PR writes
  them red before any positive path.
- State the input contract against #7435 and #9285 as field-level needs, which
  doubles as their review checklist.

## Alternatives rejected

- **Implement a literal-only evaluator in the exporter now.** Rejected: that is
  falsifier 10 (a private predicate evaluator / text scan in the RIPR exporter)
  and a second argument binder.
- **Build it from the retained `is_args` collector.** Rejected: it is test-side
  expected values keyed by assertion, not row-owned predicate participation.
- **Derive `SameOutcome` from generic activation.** Rejected: falsifiers 3 and 4.
- **Wait for all authorities before writing anything.** Rejected: the semantics
  and falsifiers do not depend on them, and #7435's review benefits from the
  concrete consumer contract.

## Prior art / duplicates

Searched all of `crates` for `SameOutcome`, `Distinguishes`, `ParameterEntityId` and
`CallArgumentBinding`: no type or identifier; `Distinguishes` appears only in
unrelated doc-comment prose. `call_argument` appears only as unrelated local
names (completion, Dancer2 handler targets, parser tests). Not a duplicate of #16436, #16438, #9296, or #9293.

## Links

- Issue: #17237
- Consumer: ripr-swarm#5562, ripr-swarm#8979, ripr-swarm#5445
- Dependency issues: #9285, #9293, #7435, #9274, #9276
