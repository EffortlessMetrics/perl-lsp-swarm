---
name: prepare-issue
description: Research, challenge, and plan a substantive concern before proof or implementation, or revisit a material premise discovered later.
argument-hint: "[issue number, PR number, or concern]"
---

# Prepare issue

Use this flow when the problem, owner, scope, proof seam, or implementation direction is not settled.

## Inputs

Use current source, live GitHub issues and PRs, accepted repository contracts, package guidance, and primary external authority where relevant.

## Orchestration

The main thread owns synthesis and issue updates. It may use subagents or Teams for independent source mapping, related-work search, reproduction, external truth, proof inventory, and vision challenge. Join contradictions into one current synthesis; do not count votes.

## Flow

1. Invoke `find-or-create-issue`.
2. Invoke `research-issue`.
3. Invoke `review-issue`.
4. Invoke `issue-to-plan`.
5. Invoke `research-plan` and `review-plan`.
6. Invoke `compile-spec` only when the decision needs durable cross-PR authority.
7. Continue to `prepare-proof` without routine sign-off.

## GitHub

Keep the issue body current and use comments for evidence, corrections, and alternatives. Use labels for stable classification and requested attention only. Do not use lifecycle labels or task completion as authority.

## Establishes

A current researched and vision-aligned plan adequate for proof design or a proportional no-proof route.

## Does not establish

Implementation, production reachability, formal review, or merge readiness.

## Routes

- `PLAN_READY` → `prepare-proof`
- `SPEC_REQUIRED` → `compile-spec`, then `prepare-proof`
- `MORE_RESEARCH_NEEDED` → `research-issue` or `research-plan`
- `MATERIAL_PREMISE_CHANGED` → repeat this flow
- `ALREADY_SATISFIED` / `SUPERSEDED` → return to `deliver-pr`
- `BLOCKED` / `NOT_PROVEN` → preserve the exact unresolved boundary
