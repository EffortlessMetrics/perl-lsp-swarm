---
name: issue-to-plan
description: Explicit atomic skill for synthesizing one researched issue into a current implementation plan, scope, non-goals, proof strategy, dependencies, risk, and next action.
---

# Issue to plan

Synthesize the researched concern into one current issue plan. Preserve research history in comments rather than rewriting it away.

## Durable write boundary

A root or operation agent assigned issue preparation, or another explicitly admitted issue integrator, may update the issue body. A read-only planner or reviewer must not mutate GitHub; return the proposed synthesis, exact fields to replace, and supporting evidence through `READ_ONLY_RESULT` so the issue integrator can apply it.

The plan should identify:

- the user and repository outcome;
- current owner and consumers;
- coherent acceptance-and-rollback claim;
- implementation seams and intended ownership changes;
- scope and non-goals;
- proof strategy and negative controls;
- dependencies, risks, and rollback;
- material return conditions;
- one current next action.

Do not over-specify exact code where investigation has not established it.

## Routes

- `PLAN_DRAFTED` → `$research-plan`
- `READ_ONLY_RESULT` → return the proposed issue delta and evidence to the issue integrator
- `MATERIAL_PREMISE_CHANGED` → `$research-issue`
- `NO_IMPLEMENTATION_REQUIRED` → return to `$deliver-pr`
