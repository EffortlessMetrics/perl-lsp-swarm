---
name: improve-test-suite
description: Harden proof against the actual implementation and move it to the cheapest effective layer without weakening the claim.
user-invocable: false
---

# Improve test suite

Find realistic wrong candidates that still pass, new failure/recovery paths, production-composition gaps, and overbroad proof. The integrating candidate writer owns test and fixture mutations; read-only adversaries return proposed cases and evidence.

Before declaring the suite hardened:

1. execute the accepted pre-build proof against the actual candidate and observe green;
2. for each new or materially changed proof, observe red against pre-fix behavior, a controlled realistic wrong implementation, or an equivalent mutation, then green against the candidate;
3. execute relevant negative and opposite-direction controls;
4. invoke `review-tests` against the observed two-sided evidence.

An unexecuted draft, green-only result, circular assertion, or instrument failure is `NOT_PROVEN`. Unchanged proof may reuse current discrimination evidence, but it must still pass on the actual candidate. Strengthen only proportionate discriminating evidence.

## Routes

- `TEST_SUITE_HARDENED` / `ALREADY_ADEQUATE` → `simplify-candidate`
- `PROOF_REVISE` → apply through the candidate writer, execute both sides, then `review-tests`
- `WEAK_OR_CIRCULAR_ORACLE` → `prepare-proof`
- `MATERIAL_REQUIREMENT_CHANGED` → `prepare-issue`
- `NOT_PROVEN` → preserve the missing instrument or unobserved evidence
