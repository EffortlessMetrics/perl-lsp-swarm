---
name: build-candidate
description: Use when reviewed proof exists or a coherent implementation candidate needs completion, test hardening, simplification, production-path validation, and candidate-stage vision review.
---

# Build candidate

## Purpose

Produce one coherent candidate that satisfies the current claim, is protected by discriminating proof, is locally minimal, and remains aligned with the product vision.

## Entry

This flow may begin from reviewed proof, an existing branch, an existing candidate, or a candidate discovered midstream. Do not replay completed work merely to manufacture chronology.

Before creating another branch, check whether an equivalent current candidate already implements the same claim. Do not inspect sibling lanes, touched-file overlap, or nearby symbols as a routine ownership check.

## Orchestration affordances

### Lane-root decisions

The lane root retains:

- the material claim, non-goals, and semantic owner;
- which implementation latitude remains inside the accepted plan;
- proof sufficiency and accepted risk/rollback boundary;
- which review findings are valid and what disposition they require;
- when a discovery materially returns to issue or proof preparation;
- whether the candidate is coherent enough for PR convergence.

### Delegable work

Use focused workers where useful for:

- implementation inside one admitted mutation boundary;
- source/owner/consumer verification;
- test hardening against the actual candidate;
- external language/protocol/dependency truth;
- production-path reachability;
- simplification and duplicate-authority review;
- specialist security, compatibility, lifecycle, packaging, persistence, performance,
  migration, or support review.

A worker receives settled facts, exact claim/candidate identity, named skill where
applicable, mutation/read-only authority, falsifiers, proof budget, and return boundary.

### Mutation owner

One writer mutates the candidate branch/worktree at a time. Read-only reviewers and
oracles return evidence to that writer. A reviewer may become the writer only through
an explicit reassignment; the resulting mutation still returns through affected proof
and review.

### Join predicate

Join into one candidate only when:

- implementation satisfies the current claim without expanding authority;
- discriminating proof is current for changed seams;
- production consumers reach the changed behavior or the limitation is explicit;
- test hardening and simplification findings are dispositioned;
- accepted review findings are repaired through the writer;
- unsupported behavior, compatibility, risk, and rollback boundaries are honest;
- no material contradiction remains hidden behind a worker verdict.

### Return packet and local proof budget

Return candidate/head identity, changed behavior and seams, current claim/non-goals,
proof run and deliberately not run, production-route evidence, findings and
dispositions, limitations, risk/rollback, current GitHub state, and typed candidate
result.

The writer runs formatting, diff hygiene, focused proof, and affected package/semantic
checks before publication. Broad workspace, platform, package, or release proof remains
hosted or risk-selected; do not pay repository-wide CI cost after every edit.

## Procedure

1. Establish or reuse the current candidate branch/worktree and writer.
2. Run `$build-from-proof` for missing implementation.
3. Run `$improve-test-suite` against the actual candidate.
4. Run `$simplify-candidate`; every changed revision returns through affected proof.
5. Run `$review-candidate`, including candidate-stage vision alignment against current authorities.
6. Repair ordinary findings through the same candidate writer and repeat affected proof/review.
7. Return the typed candidate disposition to the invoking flow. `CANDIDATE_READY` is the normal handoff for publication/convergence; this flow does not require an unavailable outer endpoint to be installed in order to produce a complete candidate.

## GitHub boundary

Publish when implementation changes the accepted claim/authority/route, when a reusable
production-path or external-truth finding affects later work, when a prerequisite or
support/risk boundary changes, or when the candidate-wide proof/limitation summary is
ready for PR review.

Keep writer/reviewer identities, topology, task progress, temporary experiments, raw
build logs, retries, and routine local passes runtime-local. Do not post one update per
edit, test, agent, or normal skill transition.

## What this establishes

A locally coherent publication candidate within the stated claim.

## What this does not establish

Formal fixed-candidate review, current GitHub checks, review-thread convergence, merge authorization, or current-main reconciliation.

## Valid exits

- `CANDIDATE_READY` → return the candidate identity, current proof, claim boundary, and review result to the invoking flow; its normal next phase is PR convergence
- `CANDIDATE_FINDINGS_OPEN` → repair within this flow, then rerun affected passes
- `WEAK_PROOF` → `$prepare-proof`
- `MATERIAL_SCOPE_OR_AUTHORITY_CHANGE` → return the corrected premise to the invoking flow for issue preparation
- `NO_BUILD_SUBJECT` → return the no-build disposition for proportional publication/review
- `WRITER_COLLISION` / `UNSAFE_WORKTREE` → resolve the same-candidate mechanical hazard
- `BLOCKED` / `NOT_PROVEN` → preserve the exact boundary
