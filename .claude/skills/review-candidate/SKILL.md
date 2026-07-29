---
name: review-candidate
description: Mutably challenge the actual candidate for correctness, vision, authority, production reachability, compatibility, security, complexity, proof, and claim honesty before publication.
user-invocable: false
---

# Review candidate

Use differentiated lenses where useful. Review the cumulative candidate and verify real production reachability. One writer integrates accepted repairs. A clean review is valid.

## Routes

- `CANDIDATE_READY` → `finish-pr`
- `CANDIDATE_FINDINGS_OPEN` → repair through `build-candidate`, then repeat affected proof/review
- `WEAK_PROOF` → `prepare-proof`
- `MATERIAL_VISION_AUTHORITY_OR_SCOPE_CHANGE` → `prepare-issue`
- `NOT_PROVEN` → preserve the missing evidence or candidate identity
