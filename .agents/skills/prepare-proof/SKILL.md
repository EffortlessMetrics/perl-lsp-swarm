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

## Proof-candidate boundary

Proof design, independent oracle challenge, realistic wrong-implementation construction, opposite-direction analysis, production-path review, and proof economics may be investigated independently when useful.

When tests, fixtures, schemas, or proof receipts require mutation, use the selected claim's current branch/worktree and one proof writer at a time. Read-only adversaries and oracles return evidence to that writer; they do not create competing proof candidates.

Do not inspect sibling worktrees or neighbouring PR overlap merely to reserve a proof surface.

## Procedure

1. Resolve current inputs and establish or reuse the current proof candidate/writer when proof artifacts require mutation.
2. Run `$spec-to-test` to design, locate, materialize, and execute the proof.
3. Run `$review-tests` against the observed execution and realistic wrong implementations.
4. Strengthen and re-execute the proof until adequate or a material premise changes.
5. Continue to `$build-candidate`.

## What this establishes

A proof boundary adequate to guide or protect the next candidate within its stated claim.

## What this does not establish

Implementation correctness, production reachability unless directly exercised, formal review, or merge readiness.

## Valid exits

- `PROOF_READY` → `$build-candidate`
- `WEAK_PROOF` → `$spec-to-test`, then repeat `$review-tests`
- `WRITER_COLLISION` / `UNSAFE_WORKTREE` → preserve the same-candidate mutation hazard
- `PLAN_CHANGED` / `MATERIAL_PREMISE_CHANGED` → `$prepare-issue`
- `MORE_ORACLE_RESEARCH` → research the named authority, then repeat this flow
- `NO_EXECUTABLE_PROOF_SUBJECT` → return to the invoking public flow for proportional candidate/claim review
- `ALREADY_PROVEN` → reference current proof and continue to `$build-candidate`
- `NOT_PROVEN` → name the unavailable or contradictory evidence
