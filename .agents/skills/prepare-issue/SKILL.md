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

## Orchestration

May run independently where useful:

- source owner and consumer mapping;
- related issue and PR search;
- reproduction and current proof inventory;
- external-truth research;
- vision, duplication, and slice challenge.

Join those results into one current issue synthesis and plan. One issue-body integrator owns durable updates.

## Procedure

1. Run `$find-or-create-issue`.
2. Run `$research-issue` against current source and authority.
3. Run `$review-issue` to challenge whether the work should exist and in what shape.
4. Run `$issue-to-plan`.
5. Run `$research-plan` and `$review-plan`.
6. Run `$compile-spec` only when the decision should outlive one PR or govern several consumers.
7. Continue directly to `$prepare-proof` when the plan is ready.

## GitHub contract

Read the selected issue, narrowly related issues and PRs, and directly linked umbrellas or contracts. Preserve research and corrections in comments; keep one current usable synthesis and plan in the issue body. Use only stable classification/risk labels.

Do not apply lifecycle-completion labels or stop merely because an old marker, reviewer identity, or receipt is absent.

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
