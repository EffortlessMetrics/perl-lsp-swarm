---
name: review-plan
description: Explicit atomic skill for adversarially reviewing a researched plan for vision, authority, architecture, negative cases, proof quality, rollback, and unnecessary complexity.
---

# Review plan

Review the researched plan while it is still cheap to change.

Use the vision, authority, slice-boundary, external-truth, and test-economics lenses as applicable.

Test:

- wrong object, owner, consumer, or abstraction layer;
- false clear winner or missing alternative;
- poisoned or circular proof premise;
- missing negative, stale, error, or rollback cases;
- duplicate authority or premature framework;
- mismatch between the issue claim and intended PR boundary;
- whether a durable specification is actually warranted.

## Routes

- `PLAN_READY` → `$prepare-proof`
- `SPEC_REQUIRED` → `$compile-spec`
- `REVISE_PLAN` → `$issue-to-plan`, then repeat this review
- `MATERIAL_PREMISE_CHANGED` → `$prepare-issue`
- `NOT_PROVEN` → preserve the exact missing evidence
