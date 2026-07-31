---
name: review-plan
description: Adversarially review a researched plan for vision, authority, architecture, negative cases, proof, rollback, and unnecessary complexity.
user-invocable: false
---

# Review plan

Challenge the plan while it is cheap to change. Use relevant vision, authority, slice, external-truth, and test-economics lenses. A clean review is valid.

## Routes

- `PLAN_READY` → `prepare-proof`
- `SPEC_REQUIRED` → `compile-spec`
- `REVISE_PLAN` → `issue-to-plan`, then repeat
- `MATERIAL_PREMISE_CHANGED` → `prepare-issue`
- `NOT_PROVEN` → preserve the missing evidence
