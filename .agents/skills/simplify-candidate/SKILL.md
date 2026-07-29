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

Preserve clarity, proof, rollback, and production behavior. A conclusion of `ALREADY_MINIMAL` is valid.

Any simplification that changes production code, behavior, configuration, generated output, or proof artifacts creates a new candidate. Route it through `$improve-test-suite` so affected proof executes on the simplified revision before candidate review. Pure analysis that makes no candidate change may proceed as `ALREADY_MINIMAL`.

## Routes

- `SIMPLIFIED` / `PROOF_CHANGED` → `$improve-test-suite`, then `$review-candidate` after current proof
- `ALREADY_MINIMAL` → `$review-candidate`
- `MATERIAL_DESIGN_CHANGE` → `$prepare-issue`
- `NOT_PROVEN` → preserve the missing candidate identity or proof result
