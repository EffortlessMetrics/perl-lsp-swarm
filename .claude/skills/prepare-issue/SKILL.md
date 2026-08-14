---
name: prepare-issue
description: Research, challenge, and plan a substantive concern before proof or implementation, or revisit a material premise discovered later.
argument-hint: "[issue number, PR number, or concern]"
---

# Prepare issue

Use this flow when the problem, owner, scope, proof seam, or implementation direction is not settled.

## Inputs

Use current source, live GitHub issues and PRs, accepted repository contracts, package guidance, and primary external authority where relevant.

## Orchestration affordances

### Lane-root decisions

The lane root retains the accepted problem, whether the work should exist, semantic
owner and consumers, one coherent claim, scope/non-goals, risk/rollback, contradiction
resolution, current plan, and whether a durable cross-PR specification is warranted.

### Useful subagent work

Use focused subagents or an Agent Team only where useful for:

- source-owner and production-consumer mapping;
- related issue, PR, and landed-work search;
- reproduction and current proof inventory;
- external language/protocol/dependency truth;
- vision, duplication, slice, risk, and rollback challenge.

Give each child settled facts, exact authorities and scope, one question, falsifiers,
and a bounded return. Children do not choose the claim or update the issue independently.

### Mutation owner and join

One issue-body integrator owns durable synthesis and plan updates. Join evidence and
contradictions rather than counting agents.

The join is ready when the problem, owner, consumers, scope/non-goals, proof obligations,
prerequisites, and plan are current; material contradictions are resolved or explicitly
`NOT_PROVEN`; and the result supports proof design or an honest no-proof route.

### Return packet

Return the current claim, owner/consumers, accepted plan, evidence references,
contradictions and dispositions, proof obligations, limitations, material return-to-
issue conditions, and typed route result.

## Flow

1. Invoke `find-or-create-issue`.
2. Invoke `research-issue`.
3. Invoke `review-issue`.
4. Invoke `issue-to-plan`.
5. Invoke `research-plan` and `review-plan`.
6. Invoke `compile-spec` only when the decision needs durable cross-PR authority.
7. Continue to `prepare-proof` without routine sign-off.

## GitHub

Use comments for durable research, corrected assumptions, external truth, dependencies,
alternatives, and contradictions. Update the issue body when one current synthesis or
plan helps builders and later contexts. Record a route change only when the material
next judgment changes.

Keep subagent/Team topology, task state, retries, raw logs, provisional reasoning, and
routine skill transitions runtime-local. Labels classify stable area/risk/attention;
they do not prove lifecycle completion or lane ownership.

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
