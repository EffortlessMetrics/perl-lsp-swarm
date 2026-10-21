# Implementation Checklist: #17237 — row-owned predicate discriminator participation facts

## Overview

Two phases. **Phase A is this PR** (spec only). **Phase B** is the implementing
PR and is blocked on the authorities listed under §Blockers; Phase B steps are
the contract for that PR, not work done here.

## Phase A — this PR

### Step A1: Spec bundle
- **File:** `.spec/17237-row-owned-predicate-discriminator/{context,acceptance,checklist}.md` (CREATE)
- **Change:** fact meaning, identity, ten falsifiers, input contract
- **Verify:** `git diff --check`; the three files exist; acceptance has the six required sections

### Step A2: Record the decision on the issues
- Comment on #17237 linking this bundle; comment on #7435 with the §Input contract below.

## Blockers for Phase B

| Authority | State at spec time | Needed from it |
|---|---|---|
| #9285 | open | changed predicate and required discriminator, canonical IDs |
| #9293 | open | exact relation/call identity for one test row |
| #7435 | open (itself blocked on #5242, #10238, #10244, #7423, #8929) | canonical argument→parameter bindings |
| #9274 / #9276 | open | v2 wire types, validation, canonical serialization |

Zero selected tests, missing #7435 bindings, or unavailable value facts is
`NOT_PROVEN`, not a pass.

## Input contract (what the fact reads, and nothing else)

From #7435, per binding: call entity and anchor; caller and callee
document/root/project/source generation; argument expression identity, anchor
and canonical ordinal; canonical parameter entity and position; binding
mode/disposition; value/type/shape evidence references; limitations for
alias/default/slurpy/prototype; completeness. Only `Complete` positional
bindings from an exact current target may produce `Distinguishes` or
`SameOutcome`.

From #9285, per change: change entity, logical owner, the changed
predicate/discriminator identity, and enough structure for a deterministic
old/new evaluation of the supported comparison set.

From #9293: the exact relation/call identity and the test/subtest owning it.

Anything not on this list is not read.

## Phase B change order (for the implementing PR)

### Step B1: Falsifiers first
- **File:** `crates/perl-ripr-facts/tests/discriminator_participation.rs` (CREATE)
- **Change:** F1–F10 from acceptance §Test-Grid, red against a stub that returns `Unknown`
- **Verify:** `cargo test -p perl-ripr-facts --all-targets --locked discriminator`

### Step B2: Fact type and identity
- **Change:** participation enum and fact struct; identity over the tuple in acceptance §Hazards; deterministic ordering
- **Depends on:** B1
- **Verify:** `cargo check -p perl-ripr-facts`

### Step B3: Evaluator over canonical facts
- **Change:** old/new evaluation for `>=`↔`>`/`<=`↔`<` and `==`↔`!=` over literal or canonical finite scalar values; whole-set rule; `Unknown` otherwise
- **Depends on:** B2, #7435, #9285
- **Verify:** F1, F2, F5 green

### Step B4: v2 projection
- **Change:** project through the v2 contract; no sidecar
- **Depends on:** B3, #9276
- **Verify:** `cargo xtask check-ripr-facts-schema`, `cargo xtask check-architecture`

### Step B5: Final verification
- **Verify:** `cargo fmt --all -- --check && cargo clippy -p perl-ripr-facts -p perl-semantic-facts -p perl-semantic-analyzer --all-targets --locked -- -D warnings && git diff --check`

## Scope boundary

Files IN scope for this PR: `.spec/17237-row-owned-predicate-discriminator/`.
Files OUT of scope: all crates, schema, generated artifacts, the existing
`discriminators` array in `emitter/relations.rs`, and any ripr-swarm file.

## Flags for the Phase B builder

- Do not extend the existing `discriminators` array; it is test-side expected values.
- Do not reuse `behavior_hint_for_hunk` / `extract_discriminator` (added-line text scan) as the old/new evaluator.
- The admitted-set cap value is a builder decision; over cap must be `Unknown`.
- Whether `Scoped` is emitted in the first cohort or only reserved is a builder decision; reserving it is acceptable, silently mapping it to `Distinguishes` is not.
