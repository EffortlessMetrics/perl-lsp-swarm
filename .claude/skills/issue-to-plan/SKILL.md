---
name: issue-to-plan
description: Synthesize one researched issue into the current plan, scope, proof strategy, dependencies, risk, and next action.
user-invocable: false
---

# Issue to plan

Maintain one current issue synthesis and plan while preserving research history in comments. Identify outcome, owner/consumers, claim boundary, seams, scope, non-goals, proof, dependencies, risks, rollback, return conditions, and next action.

## Routes

- `PLAN_DRAFTED` → `research-plan`
- `MATERIAL_PREMISE_CHANGED` → `research-issue`
- `NO_IMPLEMENTATION_REQUIRED` → `deliver-pr`
