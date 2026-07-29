---
name: prepare-proof
description: Use when intent is settled but executable proof is absent, weak, circular, too expensive, or no longer aligned with the current claim or production seam.
---

# Prepare proof

## Purpose

Turn the accepted issue plan or durable contract into the cheapest executable proof that discriminates the intended behavior from realistic incorrect implementations.

## Inputs

- controlling issue and current plan;
- governing specification or ADR where applicable;
- current semantic owner and production consumer path;
- existing tests, fixtures, receipts, and known regressions;
- independent external authority where the claim depends on Perl, LSP, DAP, packaging, or dependency behavior.

## Orchestration

May run independently where useful:

- proof design;
- independent oracle challenge;
- realistic wrong-implementation construction;
- opposite-direction and non-vacuity analysis;
- production-path and test-economics review.

Join into one proof boundary. No production writer is required until the proof is adequate.

## Procedure

1. Run `$spec-to-test` to design or locate the proof.
2. Run `$review-tests` against realistic wrong implementations.
3. Strengthen the proof until it is adequate or a material premise changes.
4. Continue to `$build-candidate`.

## What this establishes

A proof boundary adequate to guide or protect the next candidate within its stated claim.

## What this does not establish

Implementation correctness, production reachability unless directly exercised, formal review, or merge readiness.

## Valid exits

- `PROOF_READY` → `$build-candidate`
- `WEAK_PROOF` → `$spec-to-test`, then repeat `$review-tests`
- `PLAN_CHANGED` / `MATERIAL_PREMISE_CHANGED` → `$prepare-issue`
- `MORE_ORACLE_RESEARCH` → research the named authority, then repeat this flow
- `NO_EXECUTABLE_PROOF_SUBJECT` → return to `$deliver-pr` for proportional candidate/claim review
- `ALREADY_PROVEN` → reference current proof and continue to `$build-candidate`
- `NOT_PROVEN` → name the unavailable or contradictory evidence
