---
name: issue-to-plan
description: Synthesize one researched issue into the current plan, scope, proof strategy, dependencies, risk, and next action.
user-invocable: false
---

# Issue to plan

Maintain one current issue synthesis and plan while preserving research history in comments. Identify outcome, owner/consumers, claim boundary, seams, scope, non-goals, proof, dependencies, risks, rollback, return conditions, and next action.

A main/root or whole-flow agent assigned issue preparation, or another admitted issue integrator, may update GitHub. A read-only planner or reviewer returns the proposed issue delta and evidence through `READ_ONLY_RESULT`; it does not rewrite the issue itself.

## Routes

- `PLAN_DRAFTED` → `research-plan`
- `READ_ONLY_RESULT` → return the proposed issue delta and evidence to the issue integrator
- `MATERIAL_PREMISE_CHANGED` → `research-issue`
- `NO_IMPLEMENTATION_REQUIRED` → `deliver-pr`
