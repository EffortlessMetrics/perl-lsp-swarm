---
name: prepare-proof
description: Turn settled intent into the cheapest discriminating executable proof before implementation or candidate promotion.
argument-hint: "[issue, spec, or candidate]"
---

# Prepare proof

Use the issue plan, governing contract, current semantic owner, production path, existing tests, and independent external authority.

## Orchestration and mutation boundary

The main thread joins proof design, independent oracle challenge, realistic wrong-implementation analysis, opposite-direction controls, production-path review, and proof economics. Subagents or Teams may handle independent read-heavy questions.

Before `spec-to-test` creates or changes tests, fixtures, schemas, or proof receipts, establish or reuse one proof writer for the branch, worktree, and contested proof surface. Adversarial and oracle lanes remain read-only and return evidence to that writer. A separate production-code writer is not needed until the proof is adequate.

## Flow

1. Resolve current inputs and establish or reuse one proof writer when proof artifacts require mutation.
2. Invoke `spec-to-test` to materialize and execute the proof.
3. Invoke `review-tests` against the observed execution and realistic wrong implementations.
4. Strengthen and re-execute the proof until adequate.
5. Continue to `build-candidate` without routine approval.

## Routes

- `PROOF_READY` → `build-candidate`
- `WEAK_PROOF` → `spec-to-test`, then `review-tests`
- `WRITER_COLLISION` / `UNSAFE_WORKTREE` → preserve the exact mutation hazard
- `PLAN_CHANGED` / `MATERIAL_PREMISE_CHANGED` → `prepare-issue`
- `MORE_ORACLE_RESEARCH` → research, then repeat
- `NO_EXECUTABLE_PROOF_SUBJECT` → return to the invoking public flow for proportional candidate/claim review
- `ALREADY_PROVEN` → `build-candidate`
- `NOT_PROVEN` → preserve the missing evidence
