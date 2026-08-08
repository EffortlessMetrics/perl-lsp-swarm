---
name: simplify-candidate
description: Explicit atomic skill for removing unnecessary scaffolding, duplicate authority, overbroad APIs, repeated validation, dead compatibility, and one-use frameworks after the candidate works.
---

# Simplify candidate

Review the working candidate for complexity created while uncertainty was high.

Look for:

- duplicate semantic authority or validation;
- one-use traits, wrappers, registries, and frameworks;
- overbroad public APIs;
- repeated state or identity derivation;
- dead compatibility or migration scaffolding;
- test-only product paths;
- explanatory structure no longer needed by the code;
- opportunities to use an existing owner directly.

Preserve clarity, proof, rollback, and production behavior. A conclusion of
`ALREADY_MINIMAL` is valid.

## Orchestration affordances

### Lane-root decisions

The lane root retains which complexity is necessary for the claim, whether a proposed
simplification changes public behavior/authority/support/rollback, and whether the
candidate remains inside the accepted design.

### Delegable read-only questions

Use focused review where useful for:

- semantic-owner and duplicate-validation mapping;
- public API and compatibility-residue audit;
- one-use abstraction/framework inventory;
- production-path reachability and test-only-path detection;
- state/identity derivation duplication;
- proof and rollback impact of proposed deletions.

Reviewers return specific removal/retention evidence and uncertainty. They do not mutate
the candidate or decide design changes independently.

### Mutation owner and join

One candidate writer applies accepted simplifications. Join when the lane root can
state which machinery was removed or retained, why the remaining structure is needed,
which semantic owner is canonical, and what proof/review dimensions the change affects.

Any simplification that changes production code, behavior, configuration, generated
output, or proof artifacts creates a new candidate. Route it through
`$improve-test-suite` before candidate review. Pure analysis may return
`ALREADY_MINIMAL` without mutation.

### Return packet and proof budget

Return candidate identity, proposed/applied removals, canonical owner, retained
complexity rationale, changed seams, proof/review dimensions affected, proof run/not
run, limitations, and typed result.

Run formatting/diff hygiene and the smallest affected proof after mutation. Do not use
broad CI merely to explore whether a deletion is safe; inspect owner/consumer edges and
focused proof first.

## GitHub boundary

Publish when simplification changes semantic ownership, public/support/rollback meaning,
removes durable compatibility or migration machinery, or materially changes the
candidate proof/limitation summary. Localized review findings may be inline.

The integrating lane owner posts. A skill run that only answers a bounded
simplification question returns file/line-anchored findings as evidence and does not
write to GitHub.

Keep reviewer topology, candidate-shaping experiments, discarded alternatives, raw
logs, retries, and clean `ALREADY_MINIMAL` analysis runtime-local unless the conclusion
resolves a durable existing concern.

## Routes

- `SIMPLIFIED` / `PROOF_CHANGED` → `$improve-test-suite`, then `$review-candidate` after current proof
- `ALREADY_MINIMAL` → `$review-candidate`
- `MATERIAL_DESIGN_CHANGE` → `$prepare-issue`
- `NOT_PROVEN` → preserve the missing candidate identity or proof result
