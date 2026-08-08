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

Do not substitute stale conversation or agent self-report for those sources. If the
controlling claim or applicable authorities cannot be established reliably, return
`NOT_PROVEN`; if investigation changes the premise, return the corrected boundary for
issue preparation.

## Orchestration affordances

### Lane-root decisions

The lane root retains candidate sufficiency, finding dispositions, material
claim/authority/proof corrections, risk/rollback decisions, and whether the candidate
returns to build, proof, issue preparation, or PR convergence.

### Useful differentiated review

Use read-only reviewers where useful for:

- product/roadmap/architecture and semantic-owner alignment;
- production-path and real-caller reachability;
- proof discrimination and evidence integrity;
- external language/protocol/dependency truth;
- claim/non-goal honesty and unsupported fallback/refusal behavior;
- simplification, duplicate authority, compatibility residue, and API surface;
- security, concurrency, lifecycle, persistence, packaging, migration, performance,
  support, and rollback risk.

Review the cumulative candidate, not only the latest edit. Each reviewer names exact
scope, evidence, realistic falsifier, uncertainty, and affected claim dimension. A
subagent verdict is not acceptance.

### Mutation owner and join

Reviewers are read-only by default. One candidate writer integrates accepted repairs.
If a reviewer is explicitly reassigned as writer, the resulting head returns through
affected proof and review.

Join when the lane root has verified load-bearing evidence, preserved and resolved
material contradictions, dispositioned findings, and can state what the candidate does
and does not prove.

### Return packet

Return candidate/head identity, material claim/non-goals, lenses and scope used,
localized findings with severity/evidence/falsifier, contradictory evidence,
production-route and proof conclusions, accepted/refuted/follow-up dispositions,
limitations and `NOT_PROVEN` boundaries, recommended route, and typed result.

A clean review is valid. Do not create findings or edits merely to demonstrate effort.

## Review questions

Where applicable, check:

- claim honesty, semantic and external correctness;
- proof discrimination and evidence identity;
- production-path reachability and negative/fallback behavior;
- compatibility, security, support, and rollback;
- semantic owner, duplicate authority, unnecessary complexity, and residue;
- remaining uncertainty and whether the candidate remains one coherent PR-sized claim.

The review is directed and falsifying: identify realistic wrong behavior or residue that
the candidate should reject. Conclusions require execution or competent authority—not
a diff impression, green CI, or an ungrounded delegate verdict.

## GitHub boundary

The integrating lane owner posts. A skill run that only answers a bounded review
question returns file/line-anchored findings as evidence and does not write to GitHub.

Use inline review for localized candidate findings and a PR comment/review summary for
cross-cutting claim, authority, proof, risk, or production-route conclusions. Preserve
an evidence-backed disposition before resolving substantive findings.

Keep reviewer topology, raw exploration, temporary tests, duplicate clean reports,
retries, and routine review progress runtime-local. One cumulative lane-root conclusion
joins the useful evidence.

## Routes

- `CANDIDATE_READY` → return the candidate identity, material claim, current proof, and review result to the invoking flow for PR convergence
- `CANDIDATE_FINDINGS_OPEN` → repair through `$build-candidate`, then repeat affected proof and review
- `WEAK_PROOF` → `$prepare-proof`
- `MATERIAL_VISION_AUTHORITY_OR_SCOPE_CHANGE` → return the corrected premise to the invoking flow for issue preparation
- `NOT_PROVEN` → preserve the missing authority, evidence, or candidate identity
