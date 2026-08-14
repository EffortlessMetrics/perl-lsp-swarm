---
name: build-candidate
description: Implement or complete one coherent candidate, harden its tests, simplify it, and challenge the actual result before publication.
argument-hint: "[issue, branch, or candidate]"
---

# Build candidate

Existing coherent work enters midstream; do not replay completed chronology. Before
creating another candidate, check only whether an equivalent current PR already
implements the same claim. Do not inspect sibling lanes, touched-file overlap, or nearby
symbols as a routine ownership check.

## Orchestration affordances

### Lane-root decisions

The lane root retains the material claim/non-goals and semantic owner, accepted
implementation latitude, proof sufficiency, risk/rollback boundary, finding
dispositions, material return-to-issue/proof decisions, and candidate sufficiency for PR
convergence.

### Useful execution and review contexts

Use subagents, context forks, an Ultracode workflow, or an Agent Team only where useful
for:

- implementation inside one admitted mutation boundary;
- owner/consumer verification;
- test hardening against the actual candidate;
- external language/protocol/dependency truth;
- production-path reachability;
- simplification and duplicate-authority review;
- specialist security, compatibility, lifecycle, packaging, persistence, performance,
  migration, or support review.

Give each child settled facts, exact claim/candidate identity, named skill where
applicable, mutation/read-only authority, falsifiers, proof budget, and return boundary.

### Mutation owner

One writer mutates the candidate branch/worktree at a time. Read-only reviewers and
oracles return evidence to that writer. A reviewer may become writer only through an
explicit reassignment; resulting mutation still returns through affected proof/review.

### Join predicate

Join into one candidate only when implementation satisfies the current claim without
expanding authority; discriminating proof is current; production consumers reach the
behavior or the limitation is explicit; test-hardening/simplification findings and
accepted review findings are dispositioned; unsupported behavior and risk/rollback are
honest; and no contradiction is hidden behind a subagent verdict.

### Return packet and local proof budget

Return candidate/head identity, changed behavior/seams, claim/non-goals, proof run and
not run, production-route evidence, findings/dispositions, limitations, risk/rollback,
current GitHub state, and typed result.

The writer runs formatting, diff hygiene, focused proof, and affected package/semantic
checks before publication. Broad workspace/platform/package/release proof remains
hosted or risk-selected; do not pay repository-wide CI cost after every edit.

## Flow

1. Establish or reuse the current candidate and writer.
2. Invoke `build-from-proof` where implementation is missing.
3. Invoke `improve-test-suite` against the actual candidate.
4. Invoke `simplify-candidate`; every changed revision returns through affected proof.
5. Invoke `review-candidate` against current issue/contract/product authorities.
6. Repair through the candidate writer and rerun affected proof/review.
7. Return the typed candidate disposition to the invoking flow. `CANDIDATE_READY` is the normal handoff for publication and convergence; this flow does not require a not-yet-installed outer endpoint to produce a complete candidate.

## GitHub boundary

Publish when implementation changes the accepted claim/authority/route; when a reusable
production-path or external-truth finding affects later work; when a prerequisite or
support/risk boundary changes; or when the candidate-wide proof/limitation summary is
ready for PR review.

Keep subagent/Team topology, writer/reviewer identities, task progress, temporary
experiments, raw build logs, retries, and routine local passes runtime-local. Do not
post one update per edit, test, agent, or normal skill transition.

## Routes

- `CANDIDATE_READY` → return candidate identity, current proof, claim boundary, and review result to the invoking flow; its normal next phase is PR convergence
- `CANDIDATE_FINDINGS_OPEN` → repair and repeat affected passes
- `WEAK_PROOF` → `prepare-proof`
- `MATERIAL_SCOPE_OR_AUTHORITY_CHANGE` → return the corrected premise to the invoking flow for issue preparation
- `NO_BUILD_SUBJECT` → return the no-build disposition for proportional publication/review
- `WRITER_COLLISION` / `UNSAFE_WORKTREE` → resolve the same-candidate mechanical hazard
- `BLOCKED` / `NOT_PROVEN` → preserve the exact boundary
