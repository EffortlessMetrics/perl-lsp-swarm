---
name: final-challenge
description: Mutably challenge the cumulative candidate before merge or after repair, focusing on realistic falsifiers, stale proof, claim drift, reachability, and residue without creating review-stage receipts.
user-invocable: false
---

# Final challenge

Challenge the current cumulative candidate while repairs remain allowed. Check repair-
introduced defects, stale or weakened proof, silent claim drift, duplicate workaround
residue, changed owners or production routes, unresolved security/external-truth/
packaging/migration risk, and simplification opportunities. A clean challenge is valid;
do not manufacture changes.

## Orchestration affordances

### Lane-root decisions

The lane root retains whether the cumulative candidate is ready for fixed review,
which findings are material, whether proof/claim/authority changed, which challenge
lenses are needed, and whether repair, simplification, issue return, claim split, or
`NOT_PROVEN` is honest.

### Useful review contexts

Use focused subagents, context forks, or an Agent Team only where useful for:

- repair-introduced defects and stale proof;
- claim/non-goal drift and unsupported PR prose;
- production-path and semantic-owner changes;
- compatibility/workaround/migration residue;
- external language/protocol/dependency/release truth;
- security, persistence, packaging, support, and rollback risk;
- final simplification opportunities.

Each reviewer receives the cumulative candidate, current claim, accepted prior finding
dispositions, exact changed seams, applicable authority, falsifiers, and a read-only
return boundary. Do not repeat still-current review merely because this skill was
entered.

### Mutation owner and join

One candidate writer integrates accepted mutable repairs. The lane root joins
contradictions/findings, verifies load-bearing seams, and decides the cumulative
challenge result. Subagent verdicts do not authorize formal review or merge.

### Return packet

Return candidate/head identity, cumulative claim/non-goals, prior repairs examined,
lenses/searched scope, material findings with evidence/falsifiers, affected proof/review
dimensions, contradictions/dispositions, limitations, and typed result.

## Runtime-local boundary

This is an attention shift, not a durable lifecycle stage. Do not compute a claim
digest, post a marker, create a review receipt, or write challenge state to a file.
Durable output is limited to useful localized findings, evidence-backed dispositions,
changed proof/claim/route facts, and the later cumulative GitHub review.

Keep subagent/Team topology, task progress, clean duplicate passes, temporary
experiments, raw output, and retries runtime-local.

After repair, revisit the affected finding, proof, and semantic seam. Do not restart the
entire challenge merely because the SHA changed. Broaden only when claim, production
path, authority, risk, rollback, or proof materially changed.

## Routes

- `CANDIDATE_READY_FOR_REVIEW` → `review-pr`
- `MUTABLE_FINDINGS_OPEN` → repair through `build-candidate`, then repeat affected proof and this challenge
- `PROOF_REVISE` → `prepare-proof`, then repeat affected candidate passes
- `SPLIT_CLAIM` → `prepare-issue` to narrow the claim and preserve the residual
- `MATERIAL_PREMISE_CHANGED` → `prepare-issue`
- `NOT_PROVEN` → preserve the missing evidence, authority, or instrument failure
