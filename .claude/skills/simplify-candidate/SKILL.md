---
name: simplify-candidate
description: Remove unnecessary scaffolding, duplicate authority, overbroad APIs, repeated validation, dead compatibility, and one-use frameworks after the candidate works.
user-invocable: false
---

# Simplify candidate

Simplify without weakening proof, rollback, production behavior, or clarity. A clean
`ALREADY_MINIMAL` result is valid.

## Orchestration affordances

### Lane-root decisions

The lane root retains which complexity is necessary for the claim, whether a proposed
simplification changes public behavior/authority/support/rollback, and whether the
candidate remains inside the accepted design.

### Useful review contexts

Use focused subagents or context forks where useful for:

- semantic-owner and duplicate-validation mapping;
- public API and compatibility-residue audit;
- one-use abstraction/framework inventory;
- production-path reachability and test-only-path detection;
- state/identity derivation duplication;
- proof and rollback impact of proposed deletion.

Reviewers return specific removal/retention evidence and uncertainty. They do not mutate
the candidate or decide design changes independently.

### Mutation owner and join

One candidate writer applies accepted simplifications. Join when the lane root can
state which machinery was removed or retained, why the remaining structure is needed,
which semantic owner is canonical, and what proof/review dimensions changed.

Any simplification changing production code, behavior, configuration, generated output,
or proof artifacts creates a new candidate and returns through `improve-test-suite`.
Pure analysis may return `ALREADY_MINIMAL` without mutation.

### Return packet and proof budget

Return candidate identity, proposed/applied removals, canonical owner, retained
complexity rationale, changed seams, affected proof/review dimensions, proof run/not
run, limitations, and typed result.

Run formatting/diff hygiene and the smallest affected proof after mutation. Do not use
broad CI merely to explore a deletion; inspect owner/consumer edges and focused proof
first.

## GitHub boundary

Publish when simplification changes semantic ownership, public/support/rollback meaning,
removes durable compatibility/migration machinery, or materially changes the candidate
proof/limitation summary. Localized findings may be inline.

The lane root posts. Focused subagents and context forks return file/line-anchored
findings as evidence and do not write to GitHub themselves.

Keep subagent/Team topology, candidate-shaping experiments, discarded alternatives,
raw logs, retries, and clean `ALREADY_MINIMAL` analysis runtime-local unless the
conclusion resolves a durable existing concern.

## Routes

- `SIMPLIFIED` / `PROOF_CHANGED` → `improve-test-suite`, then `review-candidate` after current proof
- `ALREADY_MINIMAL` → `review-candidate`
- `MATERIAL_DESIGN_CHANGE` → `prepare-issue`
- `NOT_PROVEN` → preserve the missing candidate identity or proof result
