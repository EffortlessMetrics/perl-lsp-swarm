---
name: prepare-issue
description: Use when a substantive concern, issue, or implementation discovery lacks a current researched problem and plan, or when a material premise must be revisited before proof or implementation continues.
---

# Prepare issue

## Purpose

Turn a request, finding, or existing issue into a current, challenged plan that supports the next useful engineering decision.

## Use when

- no controlling issue exists for substantive work;
- the issue exists but its premise, owner, scope, proof seam, or plan is unsettled;
- implementation or review changed a material assumption;
- an existing candidate needs enough issue continuity to be resumed honestly.

## Do not use when

The claim is already reconciled, or the requested work is a genuinely mechanical correction whose scope and proof are already obvious.

## Authoritative inputs

- current `origin/main` and relevant package guidance;
- live GitHub issues and overlapping PRs;
- current semantic owners and consumers;
- accepted specifications or ADRs where present;
- primary external Perl, LSP, DAP, dependency, or packaging authority where relevant.

## Orchestration affordances

### Lane-root decisions

The lane root retains:

- the accepted problem and whether the work should exist;
- semantic owner and live consumers;
- one coherent claim, scope, non-goals, risk, and rollback boundary;
- contradiction resolution and the current plan;
- whether a durable cross-PR specification is warranted.

### Delegable read-only questions

Run independently where useful:

- source owner and production-consumer mapping;
- related issue, PR, and landed-work search;
- reproduction and current proof inventory;
- external language/protocol/dependency truth;
- vision, duplication, slice, risk, and rollback challenge.

Each worker receives settled facts, exact authorities and scope, one question, falsifiers,
and a bounded return. It does not choose the claim or update the issue independently.

### Mutation owner and join

One issue-body integrator owns durable synthesis and plan updates. Join worker returns as
evidence and contradictions, not votes.

The join is ready when:

- the problem, owner, consumers, scope, non-goals, and proof obligations are current;
- material contradictions are resolved or explicitly `NOT_PROVEN`;
- related work and prerequisites have a durable disposition;
- the plan is adequate for proof design or an honest no-proof route.

### Return packet

Return the current claim, owner/consumers, accepted plan, evidence references,
contradictions and dispositions, proof obligations, limitations, material return-to-
issue conditions, and the typed route result.

## Procedure

1. Run `$find-or-create-issue`.
2. Run `$research-issue` against current source and authority.
3. Run `$review-issue` to challenge whether the work should exist and in what shape.
4. Run `$issue-to-plan`.
5. Run `$research-plan` and `$review-plan`.
6. Run `$compile-spec` only when the decision should outlive one PR or govern several consumers.
7. Continue directly to `$prepare-proof` when the plan is ready.

## GitHub contract

Read the selected issue, narrowly related issues and PRs, and directly linked umbrellas or contracts.

Publish comments when research, corrected assumptions, external truth, dependencies,
alternatives, or contradictions will remain useful. Update the issue body when a durable
current synthesis or plan helps builders and later contexts. Post one route change only
when the material next judgment changes.

Keep worker identity, topology, task state, raw logs, provisional reasoning, and routine
skill transitions runtime-local. Use only stable classification/risk labels; do not use
labels or comments as lifecycle completion or lane ownership.

## What this establishes

A current source-grounded problem statement, vision-aligned scope, and plan adequate for proof design or an honest no-proof route.

## What this does not establish

Implementation correctness, proof quality, production reachability, review currentness, or merge readiness.

## Routes

- `PLAN_READY` → `$prepare-proof`
- `SPEC_REQUIRED` → `$compile-spec`, then `$prepare-proof`
- `MORE_RESEARCH_NEEDED` → `$research-issue` or `$research-plan`
- `MATERIAL_PREMISE_CHANGED` → repeat this flow with corrected authority
- `ALREADY_SATISFIED` or `SUPERSEDED` → return to `$deliver-pr` for reconciliation
- `BLOCKED` or `NOT_PROVEN` → preserve the exact dependency, decision, or missing evidence
