---
name: improve-test-suite
description: Harden proof against the actual implementation and move it to the cheapest effective layer without weakening the claim.
user-invocable: false
---

# Improve test suite

Use the actual candidate to find realistic wrong candidates that still pass, new
failure/recovery paths, production-composition gaps, and overbroad proof.

## Orchestration affordances

### Lane-root decisions

The lane root retains the accepted claim, proof-sufficiency judgment, which new paths
are material, proof-cost tradeoffs, and whether a discovery changes the requirement or
owner rather than merely strengthening tests.

### Useful adversarial contexts

Use focused subagents or context forks where useful to:

- construct realistic wrong candidates that still pass;
- inventory candidate-created branches, state transitions, stale/scope/failure/recovery
  paths;
- trace production composition and fixture-only shortcuts;
- identify negative and opposite-direction controls;
- compare focused and broad proof economics;
- detect test weakening disguised as making a red gate green.

### Mutation owner

The integrating candidate writer owns accepted test, fixture, schema, and proof
mutations. Read-only adversaries return proposed cases and evidence; they do not create
competing proof/candidate branches.

### Join predicate

The suite is hardened only when accepted proof passes on the actual candidate; each
new/materially changed proof is observed red against pre-fix behavior, a controlled
wrong candidate, or equivalent mutation, then green on the candidate; relevant controls
execute; `review-tests` establishes non-vacuity, production reachability, and
proportionate economics; and missing evidence remains explicit `NOT_PROVEN`.

### Return packet and proof budget

Return candidate/proof identity, newly excluded wrong behaviors and paths, two-sided
execution evidence, controls, proof moved/removed, production-route findings, proof not
run, limitations, cost/remote boundary, and typed result.

Prefer focused and affected proof. Broader suites run only when new composition/risk
requires them; do not pay another hosted CI cycle for defects local adversarial work can
expose.

## Required execution

1. Execute accepted pre-build proof against the candidate and observe green.
2. For each new/materially changed proof, observe red against pre-fix behavior, a
   controlled wrong candidate, or equivalent mutation, then green against the candidate.
3. Execute relevant negative and opposite-direction controls.
4. Invoke `review-tests` against the observed two-sided evidence.

An unexecuted draft, green-only result, circular assertion, or instrument failure is
`NOT_PROVEN`. Unchanged proof may reuse current discrimination evidence, but it must
still pass on the actual candidate. Strengthen only proportionate discriminating proof.

## GitHub boundary

Publish when hardening changes a durable proof obligation, reveals a claim/owner/route
error, adds a reusable falsifier, changes support/risk, or materially updates the
candidate proof/limitation summary.

Keep subagent topology, temporary mutants, raw output, retries, routine clean results,
and per-test progress runtime-local. Link stable evidence; do not post one comment per
test or reviewer.

## Routes

- `TEST_SUITE_HARDENED` / `ALREADY_ADEQUATE` → `simplify-candidate`
- `PROOF_REVISE` → apply through the candidate writer, execute both sides, then `review-tests`
- `WEAK_OR_CIRCULAR_ORACLE` → `prepare-proof`
- `MATERIAL_REQUIREMENT_CHANGED` → `prepare-issue`
- `NOT_PROVEN` → preserve the missing instrument or unobserved evidence
