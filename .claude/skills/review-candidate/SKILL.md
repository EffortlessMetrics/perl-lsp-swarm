---
name: review-candidate
description: Mutably challenge the actual candidate for correctness, vision, authority, production reachability, compatibility, security, complexity, proof, and claim honesty before publication.
user-invocable: false
---

# Review candidate

Before reviewing, resolve the current controlling issue/plan, governing contract where
applicable, relevant product/architecture and semantic-owner sources, exact cumulative
candidate, and current proof with limitations. Stale conversation or subagent self-
report is not authority. Missing or contradictory authority is `NOT_PROVEN`; a changed
premise returns to issue preparation.

## Orchestration affordances

### Lane-root decisions

The lane root retains candidate sufficiency, finding dispositions, material
claim/authority/proof corrections, risk/rollback decisions, and whether the candidate
returns to build, proof, issue preparation, or PR convergence.

### Useful review contexts

Use focused subagents, context forks, or an Agent Team only where useful for:

- product/architecture and semantic-owner alignment;
- production-path and real-caller reachability;
- proof discrimination/evidence integrity;
- external language/protocol/dependency truth;
- claim/non-goal honesty and fallback/refusal behavior;
- simplification, duplicate authority, compatibility residue, and API surface;
- security, concurrency, lifecycle, persistence, packaging, migration, performance,
  support, and rollback risk.

Review the cumulative candidate. Each reviewer names exact scope, evidence, realistic
falsifier, uncertainty, and affected claim dimension. A subagent verdict is not
acceptance.

### Mutation owner and join

Review contexts are read-only by default. One candidate writer integrates accepted
repairs. If a reviewer is explicitly reassigned as writer, the resulting head returns
through affected proof/review.

Join when the lane root has verified load-bearing evidence, preserved/resolved material
contradictions, dispositioned findings, and can state what the candidate does and does
not prove.

### Return packet

Return candidate/head identity, material claim/non-goals, lenses and searched scope,
localized findings with severity/evidence/falsifier, contradictory evidence,
production-route/proof conclusions, accepted/refuted/follow-up dispositions,
limitations and `NOT_PROVEN` boundaries, recommended route, and typed result.

A clean review is valid. Do not create findings or edits merely to demonstrate effort.

## Review questions

Where applicable, check claim honesty, semantic/external correctness, proof
discrimination, production reachability, negative/fallback behavior, compatibility,
security, support, rollback, semantic ownership, duplicate authority, complexity,
residue, and remaining uncertainty.

The challenge is directed and falsifying; conclusions require execution or competent
authority—not a diff impression, green CI, or ungrounded delegate verdict.

## GitHub boundary

The lane root posts. Focused subagents, context forks, and Team members return
file/line-anchored findings as evidence and do not write to GitHub themselves.

The lane root uses inline review for localized findings and a PR comment/review summary
for cross-cutting claim, authority, proof, risk, or production-route conclusions.
Preserve an evidence-backed disposition before resolving substantive findings.

Keep reviewer/Team topology, raw exploration, temporary tests, duplicate clean reports,
retries, and routine progress runtime-local. One cumulative lane-root conclusion joins
useful evidence.

## Routes

- `CANDIDATE_READY` → return candidate identity, material claim, current proof, and review result to the invoking flow for PR convergence
- `CANDIDATE_FINDINGS_OPEN` → repair through `build-candidate`, then repeat affected proof/review
- `WEAK_PROOF` → `prepare-proof`
- `MATERIAL_VISION_AUTHORITY_OR_SCOPE_CHANGE` → return the corrected premise to the invoking flow for issue preparation
- `NOT_PROVEN` → preserve the missing authority, evidence, or candidate identity
