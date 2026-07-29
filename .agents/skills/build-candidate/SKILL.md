---
name: build-candidate
description: Use when reviewed proof exists or a coherent implementation candidate needs completion, test hardening, simplification, production-path validation, and candidate-stage vision review.
---

# Build candidate

## Purpose

Produce one coherent candidate that satisfies the current claim, is protected by discriminating proof, is locally minimal, and remains aligned with the product vision.

## Entry

This flow may begin from reviewed proof, an existing branch, an existing candidate, or a candidate discovered midstream. Do not replay completed work merely to manufacture chronology.

## Orchestration

Serialized:

- one integrating writer owns the contested branch, worktree, and semantic mutation surface.

Parallel where useful:

- current source and consumer verification;
- test adversary and proof economics;
- external truth;
- security, compatibility, parser/compiler, packaging, or production-path lenses.

Join into one hardened, simplified, challenged candidate.

## Procedure

1. Establish or reuse one integrating writer and candidate branch.
2. Run `$build-from-proof` for missing implementation.
3. Run `$improve-test-suite` against the actual candidate.
4. Run `$simplify-candidate`.
5. Run `$review-candidate`, including candidate-stage vision alignment.
6. Repair ordinary findings through the same writer and repeat affected proof/review.
7. Continue to `$finish-pr`.

## What this establishes

A locally coherent publication candidate within the stated claim.

## What this does not establish

Formal fixed-candidate review, current GitHub checks, review-thread convergence, merge authorization, or current-main reconciliation.

## Valid exits

- `CANDIDATE_READY` → `$finish-pr`
- `CANDIDATE_FINDINGS_OPEN` → repair within this flow, then rerun affected passes
- `WEAK_PROOF` → `$prepare-proof`
- `MATERIAL_SCOPE_OR_AUTHORITY_CHANGE` → `$prepare-issue`
- `NO_BUILD_SUBJECT` → return to `$deliver-pr` for proportional publication/review
- `BLOCKED` / `NOT_PROVEN` → preserve the exact boundary
