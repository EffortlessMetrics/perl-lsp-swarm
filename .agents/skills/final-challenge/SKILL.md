---
name: final-challenge
description: Mutably challenge the cumulative candidate before merge or after repair, focusing on realistic falsifiers, stale proof, claim drift, reachability, and residue without creating review-stage receipts.
---

# Final challenge

Challenge the current cumulative candidate while repairs remain allowed.

Check:

- defects introduced by review or CI repair;
- proof made stale or weakened by the repair;
- silent claim expansion or narrowing;
- duplicate compatibility or workaround residue;
- changed semantic owner, consumer, or production route;
- unresolved security, external-truth, packaging, migration, or support risk;
- opportunities for simplification.

A clean challenge is valid. Do not manufacture a finding or edit.

This is a runtime-local attention shift, not a durable lifecycle stage. Do not compute a claim digest, post a `final-challenge` marker, or create a review receipt. The durable record is the useful GitHub review, findings, dispositions, and proof.

After a later repair, revisit the affected finding, proof, and semantic seam. Do not restart the entire challenge merely because the commit SHA changed. Broaden only when the repair materially changes the claim, production path, authority, risk, rollback, or proof.

## Routes

- `CANDIDATE_READY_FOR_REVIEW` → `$review-pr`
- `MUTABLE_FINDINGS_OPEN` → repair through `$build-candidate`, then repeat affected proof and this challenge
- `PROOF_REVISE` → `$prepare-proof`, then repeat affected candidate passes
- `SPLIT_CLAIM` → `$prepare-issue` to narrow the claim and preserve the residual
- `MATERIAL_PREMISE_CHANGED` → `$prepare-issue`
- `NOT_PROVEN` → preserve the missing evidence, authority, or instrument failure
