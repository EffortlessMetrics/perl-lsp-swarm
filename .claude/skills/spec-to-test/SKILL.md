---
name: spec-to-test
description: Translate the current plan or contract into the cheapest discriminating executable proof and its limitations.
user-invocable: false
---

# Spec to test

Design and materialize proof that fails against realistic wrong implementations, uses an independent oracle, includes material negative or opposite-direction controls, and exercises production composition when required.

Before promotion, execute the proof against current/wrong behavior or another controlled realistic wrong implementation and observe the intended failure. Execute relevant controls to show the fixture and oracle themselves run. An unexecuted draft is `NOT_PROVEN`.

## Routes

- `PROOF_EXECUTED_RED` / `EXISTING_PROOF_EXECUTED` → `review-tests`
- `DRAFT_NOT_EXECUTED` / `INSTRUMENT_FAILURE` → `NOT_PROVEN`
- `AMBIGUOUS_REQUIREMENT` / `MATERIAL_PREMISE_CHANGED` → `prepare-issue`
- `NO_EXECUTABLE_PROOF_SUBJECT` → `deliver-pr`
