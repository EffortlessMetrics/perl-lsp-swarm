---
name: final-challenge
description: Explicit mutable review skill for challenging the repaired cumulative candidate immediately before formal review, with emphasis on repair-introduced defects, stale proof, claim drift, production reachability, and unnecessary residue.
---

# Final challenge

Review the cumulative repaired candidate while fixes are still expected.

Resolve the exact current head and normalized material claim/review-index digest before judgment:

```text
scripts/reviews/claim-digest --pr <n> [--repo owner/repo]
```

Check:

- defects introduced by review or CI repair;
- proof made stale or weakened by the repair;
- silent claim expansion or narrowing;
- duplicate compatibility or workaround residue;
- changed semantic owner, consumer, or production route;
- unresolved security, external-truth, packaging, or migration risk;
- opportunities for final simplification.

A clean challenge is valid.

## Runtime-local pass

This is the final mutable attention shift before fixed-candidate formal review. Do not create a `final-challenge` receipt, stage marker, schema, or second durable currentness authority.

The durable judgment is the submitted formal review and its exact candidate-and-claim-bound `review-run` receipt. If a later session resumes before a current formal review exists, rerun this bounded challenge and continue directly into `review-pr`. Repetition here is cheaper and safer than maintaining another stage-state protocol.

## Routes

- `CANDIDATE_FIXED_FOR_FORMAL_REVIEW` → pass the exact head and claim digest to `$review-pr`
- `MUTABLE_FINDINGS_OPEN` → repair through `$build-candidate`, then repeat affected proof and this challenge
- `PROOF_REVISE` → `$prepare-proof`, then repeat affected candidate passes
- `SPLIT_CLAIM` → `$prepare-issue` to narrow the current claim and preserve the independent residual claim
- `MATERIAL_PREMISE_CHANGED` → `$prepare-issue`
- `NOT_PROVEN` → preserve the exact missing subject identity, evidence, or instrument failure
