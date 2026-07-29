---
name: prepare-proof
description: Turn settled intent into the cheapest discriminating executable proof before implementation or candidate promotion.
argument-hint: "[issue, spec, or candidate]"
---

# Prepare proof

Use the issue plan, governing contract, current semantic owner, production path, existing tests, and independent external authority.

## Orchestration

The main thread joins proof design, independent oracle challenge, realistic wrong-implementation analysis, opposite-direction controls, production-path review, and proof economics. Subagents or Teams may handle independent read-heavy questions.

## Flow

1. Invoke `spec-to-test`.
2. Invoke `review-tests`.
3. Strengthen the proof until adequate.
4. Continue to `build-candidate` without routine approval.

## Routes

- `PROOF_READY` → `build-candidate`
- `WEAK_PROOF` → `spec-to-test`, then `review-tests`
- `PLAN_CHANGED` / `MATERIAL_PREMISE_CHANGED` → `prepare-issue`
- `MORE_ORACLE_RESEARCH` → research, then repeat
- `NO_EXECUTABLE_PROOF_SUBJECT` → return to `deliver-pr`
- `ALREADY_PROVEN` → `build-candidate`
- `NOT_PROVEN` → preserve the missing evidence
