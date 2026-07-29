---
name: build-candidate
description: Implement or complete a coherent candidate, harden its tests, simplify it, and challenge the actual result before publication.
argument-hint: "[issue, branch, or candidate]"
---

# Build candidate

One integrating writer owns contested mutation. The main thread may use subagents or Teams for source verification, test adversary work, external truth, and differentiated risk review.

## Flow

1. Establish or reuse the candidate and writer.
2. Invoke `build-from-proof` where implementation is missing.
3. Invoke `improve-test-suite` against the actual candidate.
4. Invoke `simplify-candidate`; every changed revision returns through affected proof.
5. Invoke `review-candidate` against current issue/contract/product authorities.
6. Repair through the integrating writer and rerun affected proof/review.
7. Return the typed candidate disposition to the invoking flow. `CANDIDATE_READY` is the normal handoff for publication and convergence; this flow does not require a not-yet-installed outer endpoint to produce a complete candidate.

Existing coherent work enters midstream; do not replay completed chronology.

## Routes

- `CANDIDATE_READY` → return candidate identity, current proof, claim boundary, and review result to the invoking flow; its normal next phase is PR convergence
- `CANDIDATE_FINDINGS_OPEN` → repair and repeat affected passes
- `WEAK_PROOF` → `prepare-proof`
- `MATERIAL_SCOPE_OR_AUTHORITY_CHANGE` → return the corrected premise to the invoking flow for issue preparation
- `NO_BUILD_SUBJECT` → return the no-build disposition for proportional publication/review
- `BLOCKED` / `NOT_PROVEN` → preserve the exact boundary
