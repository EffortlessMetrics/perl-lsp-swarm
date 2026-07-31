---
name: final-challenge
description: Mutably challenge the repaired cumulative candidate immediately before formal review for repair defects, stale proof, claim drift, reachability, and residue.
user-invocable: false
---

# Final challenge

Resolve the exact current head and normalized material claim/review-index digest with `scripts/reviews/claim-digest --pr <n> [--repo owner/repo]`. Use differentiated read-only lenses where useful. A clean challenge is valid; do not manufacture changes.

This is a runtime-local attention shift, not a durable lifecycle state. Do not post a `final-challenge` marker or create another schema/helper. The durable judgment is the submitted formal review and its exact candidate-and-claim-bound `review-run` receipt.

If a later session resumes before a current formal review exists, rerun this bounded challenge and continue directly to `review-pr`. Repeating the pass is cheaper and safer than maintaining another stage-state protocol.

## Routes

- `CANDIDATE_FIXED_FOR_FORMAL_REVIEW` → pass the exact head and claim digest to `review-pr`
- `MUTABLE_FINDINGS_OPEN` → repair through `build-candidate`, then repeat affected proof and this challenge
- `PROOF_REVISE` → `prepare-proof`, then repeat affected candidate passes
- `SPLIT_CLAIM` → `prepare-issue` to narrow the current claim and preserve the residual claim
- `MATERIAL_PREMISE_CHANGED` → `prepare-issue`
- `NOT_PROVEN` → preserve the exact missing subject identity, evidence, or instrument failure
