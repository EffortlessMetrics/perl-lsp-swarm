---
name: improve-test-suite
description: Explicit atomic skill for hardening proof after implementation by finding realistic wrong candidates that still pass and moving proof to the cheapest effective layer.
---

# Improve test suite

Use the actual candidate to identify weaknesses the pre-build proof could not expose. The integrating candidate writer owns any test or fixture mutations; read-only adversaries return proposed cases and evidence.

Ask:

- What realistic incorrect implementation still passes?
- Did implementation create new branch, state, stale, scope, failure, or recovery paths?
- Does the proof exercise production composition where claimed?
- Are negative and opposite-direction controls present?
- Can broad or slow proof be replaced by a focused oracle without weakening the claim?
- Did the candidate reveal a material requirement or ownership error?

## Required execution boundary

Before returning `TEST_SUITE_HARDENED` or `ALREADY_ADEQUATE`:

1. execute the accepted pre-build proof against the actual candidate and observe the expected green result;
2. for each new or materially changed proof, observe it fail against the current pre-fix behavior, a controlled realistic wrong implementation, or an equivalent mutation, then pass against the actual candidate;
3. execute relevant negative and opposite-direction controls;
4. run `$review-tests` against the observed two-sided evidence to challenge oracle independence, non-vacuity, production reachability, and proof economics.

A test draft, unobserved command, circular assertion, instrument failure, or green-only result is `NOT_PROVEN`, not hardened proof. Existing unchanged proof may reuse current discrimination evidence, but it must still pass on the actual candidate.

Add or strengthen only proportionate discriminating proof.

## Routes

- `TEST_SUITE_HARDENED` / `ALREADY_ADEQUATE` → `$simplify-candidate`
- `PROOF_REVISE` → apply through the candidate writer, execute both sides, then `$review-tests`
- `WEAK_OR_CIRCULAR_ORACLE` → `$prepare-proof`
- `MATERIAL_REQUIREMENT_CHANGED` → `$prepare-issue`
- `NOT_PROVEN` → preserve the missing instrument or unobserved evidence
