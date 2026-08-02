---
name: review-candidate
description: Mutably challenge the actual candidate for correctness, vision, authority, production reachability, compatibility, security, complexity, proof, and claim honesty before publication.
user-invocable: false
---

# Review candidate

Before reviewing, resolve the current controlling issue and plan, governing specification/ADR/policy where applicable, relevant product/roadmap/architecture and semantic owner sources, exact cumulative candidate, and current proof with limitations. Do not substitute stale conversation or agent self-report. Missing or contradictory authority is `NOT_PROVEN`; a changed premise returns to issue preparation.

Use differentiated lenses where useful. Review the cumulative candidate and verify real production reachability. The review is directed at the declared claim, falsifying rather than merely descriptive, and verified against the relevant proof and source authority. Where applicable, check claim honesty, semantic/external correctness, proof discrimination, production reachability, negative/fallback behavior, compatibility/rollback, and remaining uncertainty. One writer integrates accepted repairs. A clean review is valid; do not manufacture findings or edits.

## Routes

- `CANDIDATE_READY` → return candidate identity, material claim, current proof, and review result to the invoking flow for PR convergence
- `CANDIDATE_FINDINGS_OPEN` → repair through `build-candidate`, then repeat affected proof/review
- `WEAK_PROOF` → `prepare-proof`
- `MATERIAL_VISION_AUTHORITY_OR_SCOPE_CHANGE` → return the corrected premise to the invoking flow for issue preparation
- `NOT_PROVEN` → preserve the missing authority, evidence, or candidate identity
