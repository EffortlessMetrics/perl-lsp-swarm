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

## Orchestration and mutation boundary

May run independently where useful:

- proof design;
- independent oracle challenge;
- realistic wrong-implementation construction;
- opposite-direction and non-vacuity analysis;
- production-path and test-economics review.

Those parallel lanes are read-only unless they are the admitted proof writer. Before `$spec-to-test` creates or changes tests, fixtures, schemas, or proof receipts, establish or reuse one proof writer owning the branch, worktree, and contested proof surface. One integrating proof writer applies the joined design; adversarial and oracle lanes return evidence rather than mutating concurrently.

Join into one proof boundary. A production-code writer is not required until the proof is adequate, but proof-artifact mutation always has one accountable writer.

## Procedure

1. Resolve current inputs and establish or reuse one proof writer when proof artifacts require mutation.
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
- `WRITER_COLLISION` / `UNSAFE_WORKTREE` → preserve the exact mutation hazard
- `PLAN_CHANGED` / `MATERIAL_PREMISE_CHANGED` → `$prepare-issue`
- `MORE_ORACLE_RESEARCH` → research the named authority, then repeat this flow
- `NO_EXECUTABLE_PROOF_SUBJECT` → return to the invoking public flow for proportional candidate/claim review
- `ALREADY_PROVEN` → reference current proof and continue to `$build-candidate`
- `NOT_PROVEN` → name the unavailable or contradictory evidence
