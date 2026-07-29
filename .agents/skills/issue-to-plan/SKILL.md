---
name: issue-to-plan
description: Explicit atomic skill for synthesizing one researched issue into a current implementation plan, scope, non-goals, proof strategy, dependencies, risk, and next action.
---

# Issue to plan

Update the issue's current synthesis and current plan. Preserve research history in comments rather than rewriting it away.

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
- `MATERIAL_PREMISE_CHANGED` → `$research-issue`
- `NO_IMPLEMENTATION_REQUIRED` → return to `$deliver-pr`
