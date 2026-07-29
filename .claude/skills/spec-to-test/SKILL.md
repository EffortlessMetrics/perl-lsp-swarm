---
name: spec-to-test
description: Translate the current plan or contract into the cheapest discriminating executable proof and its limitations.
user-invocable: false
---

# Spec to test

Design proof that fails against realistic wrong implementations, uses an independent oracle, includes material negative or opposite-direction controls, and exercises production composition when required.

## Routes

- `PROOF_DRAFTED` / `EXISTING_PROOF_FOUND` → `review-tests`
- `AMBIGUOUS_REQUIREMENT` / `MATERIAL_PREMISE_CHANGED` → `prepare-issue`
- `NO_EXECUTABLE_PROOF_SUBJECT` → `deliver-pr`
