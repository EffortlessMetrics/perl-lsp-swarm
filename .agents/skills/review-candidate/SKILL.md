---
name: review-candidate
description: Explicit mutable review skill for challenging the actual candidate for correctness, vision, authority, production reachability, compatibility, security, complexity, proof, and claim honesty before publication.
---

# Review candidate

Review the actual implementation while mutation is still expected.

## Authoritative inputs

Resolve current, claim-relative authority before reviewing:

- controlling issue and current synthesis/plan;
- governing specification, ADR, policy, or accepted invariant where applicable;
- current product vision, roadmap, architecture, and semantic owner/consumer sources relevant to the change;
- exact cumulative candidate identity and diff;
- current proof, execution results, and known limitations.

Do not substitute stale conversation or agent self-report for those sources. If the controlling claim or applicable authorities cannot be established reliably, return `NOT_PROVEN`; if investigation changes the premise, return the corrected boundary for issue preparation.

Use applicable lenses:

- candidate-mode vision alignment;
- authority alignment;
- production path;
- claim boundary;
- external truth;
- security, compatibility, parser/compiler, packaging, or performance risk;
- test economics.

Check the cumulative candidate, not only the latest edit. Verify that real user or protocol paths can reach the changed behavior and that the PR-sized claim remains coherent.

The review is directed at the applicable vision, authority, production path,
external-truth, claim, security/compatibility, complexity, proof, and rollback
questions. It is falsifying: identify realistic wrong behavior or residue that the
candidate should reject. It is verified through execution or competent authority,
not a diff impression, green CI, or an ungrounded delegate verdict.

A clean review is valid.

## Orchestration

Run differentiated read-only lenses in parallel when they improve detection. One integrating writer owns accepted repairs. Join findings into one candidate disposition rather than counting votes.

## Routes

- `CANDIDATE_READY` → return the candidate identity, material claim, current proof, and review result to the invoking flow for PR convergence
- `CANDIDATE_FINDINGS_OPEN` → repair through `$build-candidate`, then repeat affected proof and review
- `WEAK_PROOF` → `$prepare-proof`
- `MATERIAL_VISION_AUTHORITY_OR_SCOPE_CHANGE` → return the corrected premise to the invoking flow for issue preparation
- `NOT_PROVEN` → preserve the missing authority, evidence, or candidate identity
