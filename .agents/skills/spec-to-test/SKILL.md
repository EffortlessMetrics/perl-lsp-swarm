---
name: spec-to-test
description: Explicit atomic skill for translating the current issue plan or durable contract into the cheapest discriminating executable proof and its stated limitations.
---

# Spec to test

Identify the behavior, failure, boundary, or invariant that must be distinguished.

Design and materialize the smallest proof that:

- fails against the known or realistic wrong implementation;
- uses an oracle independent of the implementation under test;
- includes negative, stale, opposite-direction, or non-vacuity controls where material;
- exercises the claimed production seam when component proof is insufficient;
- records what it establishes and what remains unproved.

Prefer focused unit or component proof when it is sufficient. Escalate to integration, packaged, real-workspace, or external-oracle proof only where the claim requires it.

Before promotion, execute the drafted proof at least once against the current/wrong behavior or another controlled wrong implementation and observe the discriminating failure. Also execute any control needed to establish that the fixture and oracle themselves run. A proposed test that has not executed is `NOT_PROVEN`, not proof-ready.

## Routes

- `PROOF_EXECUTED_RED` → `$review-tests`
- `EXISTING_PROOF_EXECUTED` → `$review-tests`
- `DRAFT_NOT_EXECUTED` / `INSTRUMENT_FAILURE` → `NOT_PROVEN` with the missing execution evidence
- `AMBIGUOUS_REQUIREMENT` / `MATERIAL_PREMISE_CHANGED` → `$prepare-issue`
- `NO_EXECUTABLE_PROOF_SUBJECT` → return to `$deliver-pr`
