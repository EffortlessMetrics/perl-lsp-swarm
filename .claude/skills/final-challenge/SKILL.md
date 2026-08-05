---
name: final-challenge
description: Mutably challenge the cumulative candidate before merge or after repair, focusing on realistic falsifiers, stale proof, claim drift, reachability, and residue without creating review-stage receipts.
user-invocable: false
---

# Final challenge

Challenge the current cumulative candidate while repairs remain allowed.

Check repair-introduced defects, stale or weakened proof, silent claim drift, duplicate workaround residue, changed owners or production routes, unresolved security/external-truth/packaging/migration risk, and opportunities for simplification. A clean challenge is valid; do not manufacture changes.

This is a runtime-local attention shift, not a durable lifecycle stage. Do not compute a claim digest, post a marker, or create a review receipt. The durable record is the useful GitHub review, findings, dispositions, and proof.

After a later repair, revisit the affected finding, proof, and semantic seam. Do not restart the entire challenge merely because the commit SHA changed. Broaden only when the repair materially changes the claim, production path, authority, risk, rollback, or proof.

## Routes

- `CANDIDATE_READY_FOR_REVIEW` → `review-pr`
- `MUTABLE_FINDINGS_OPEN` → repair through `build-candidate`, then repeat affected proof and this challenge
- `PROOF_REVISE` → `prepare-proof`, then repeat affected candidate passes
- `SPLIT_CLAIM` → `prepare-issue` to narrow the claim and preserve the residual
- `MATERIAL_PREMISE_CHANGED` → `prepare-issue`
- `NOT_PROVEN` → preserve the missing evidence, authority, or instrument failure
