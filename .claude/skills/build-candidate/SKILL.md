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
3. Invoke `improve-test-suite`.
4. Invoke `simplify-candidate`.
5. Invoke `review-candidate`, including candidate-stage vision review.
6. Repair through the integrating writer and rerun affected proof/review.
7. Continue to `finish-pr`.

Existing coherent work enters midstream; do not replay completed chronology.

## Routes

- `CANDIDATE_READY` → `finish-pr`
- `CANDIDATE_FINDINGS_OPEN` → repair and repeat affected passes
- `WEAK_PROOF` → `prepare-proof`
- `MATERIAL_SCOPE_OR_AUTHORITY_CHANGE` → `prepare-issue`
- `NO_BUILD_SUBJECT` → `deliver-pr`
- `BLOCKED` / `NOT_PROVEN` → preserve the exact boundary
