---
name: improve-test-suite
description: Explicit atomic skill for hardening proof after implementation by finding realistic wrong candidates that still pass and moving proof to the cheapest effective layer.
---

# Improve test suite

Use the actual candidate to identify weaknesses the pre-build proof could not expose.

Ask:

- What realistic incorrect implementation still passes?
- Did implementation create new branch, state, stale, scope, failure, or recovery paths?
- Does the proof exercise production composition where claimed?
- Are negative and opposite-direction controls present?
- Can broad or slow proof be replaced by a focused oracle without weakening the claim?
- Did the candidate reveal a material requirement or ownership error?

## Orchestration affordances

### Lane-root decisions

The lane root retains the accepted candidate claim, proof-sufficiency judgment, which
new paths are material, proof-cost tradeoffs, and whether a discovery changes the
requirement/owner rather than merely strengthening tests.

### Delegable read-only questions

Use focused adversaries where useful to:

- construct realistic wrong candidates that still pass;
- inventory candidate-created branches, state transitions, stale/scope/failure/recovery
  paths;
- trace production composition and identify fixture-only shortcuts;
- identify negative and opposite-direction controls;
- compare focused versus broad proof economics;
- inspect whether a test change weakens a ratchet or merely makes red green.

### Mutation owner

The integrating candidate writer owns all accepted test, fixture, schema, and proof
mutations. Read-only adversaries return proposed cases and evidence; they do not create
competing proof/candidate branches.

### Join predicate

The suite is hardened only when:

- accepted pre-build proof passes on the actual candidate;
- each new/materially changed proof was observed red against pre-fix behavior, a
  controlled realistic wrong candidate, or equivalent mutation, then green on the
  candidate;
- negative/opposite-direction controls executed;
- `$review-tests` finds the oracle non-vacuous, independent enough, production-reachable,
  and proportionate;
- missing instruments or evidence remain explicit `NOT_PROVEN` boundaries.

### Return packet and proof budget

Return candidate/proof identity, newly covered wrong behaviors and paths, two-sided
execution evidence, controls, proof moved/removed, production-route findings, proof not
run, limitations, cost/remote boundary, and typed result.

Prefer focused tests and affected package/semantic proof. Broader suites run only when
new composition/risk requires them; do not publish a candidate repeatedly merely to
learn about defects that local adversarial work could expose.

## Required execution boundary

Before returning `TEST_SUITE_HARDENED` or `ALREADY_ADEQUATE`:

1. execute the accepted pre-build proof against the actual candidate and observe the expected green result;
2. for each new or materially changed proof, observe it fail against the current pre-fix behavior, a controlled realistic wrong implementation, or an equivalent mutation, then pass against the actual candidate;
3. execute relevant negative and opposite-direction controls;
4. run `$review-tests` against the observed two-sided evidence to challenge oracle independence, non-vacuity, production reachability, and proof economics.

A test draft, unobserved command, circular assertion, instrument failure, or green-only
result is `NOT_PROVEN`, not hardened proof. Existing unchanged proof may reuse current
discrimination evidence, but it must still pass on the actual candidate.

Add or strengthen only proportionate discriminating proof.

## GitHub boundary

Publish when hardening changes a durable proof obligation, reveals a claim/owner/route
error, provides a reusable falsifier, changes support/risk, or materially updates the
candidate proof/limitation summary.

Keep adversary identity, topology, temporary mutants, raw output, retries, routine clean
results, and per-test progress runtime-local. Link stable evidence; do not post one
comment per test or reviewer.

## Routes

- `TEST_SUITE_HARDENED` / `ALREADY_ADEQUATE` → `$simplify-candidate`
- `PROOF_REVISE` → apply through the candidate writer, execute both sides, then `$review-tests`
- `WEAK_OR_CIRCULAR_ORACLE` → `$prepare-proof`
- `MATERIAL_REQUIREMENT_CHANGED` → `$prepare-issue`
- `NOT_PROVEN` → preserve the missing instrument or unobserved evidence
