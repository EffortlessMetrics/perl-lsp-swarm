---
name: spec-to-test
description: Explicit atomic skill for translating the current issue plan or durable contract into the cheapest discriminating executable proof and its stated limitations.
---

# Spec to test

Identify the behavior, failure, boundary, or invariant that must be distinguished.

Design the smallest proof that:

- fails against the known or realistic wrong implementation;
- uses an oracle independent of the implementation under test;
- includes negative, stale, opposite-direction, or non-vacuity controls where material;
- exercises the claimed production seam when component proof is insufficient;
- records what it establishes and what remains unproved.

Prefer focused unit or component proof when it is sufficient. Escalate to integration, packaged, real-workspace, or external-oracle proof only where the claim requires it.

## Routes

- `PROOF_DRAFTED` → `$review-tests`
- `EXISTING_PROOF_FOUND` → `$review-tests`
- `AMBIGUOUS_REQUIREMENT` / `MATERIAL_PREMISE_CHANGED` → `$prepare-issue`
- `NO_EXECUTABLE_PROOF_SUBJECT` → return to `$deliver-pr`
