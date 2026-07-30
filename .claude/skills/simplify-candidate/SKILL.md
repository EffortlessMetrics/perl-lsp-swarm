---
name: simplify-candidate
description: Remove unnecessary scaffolding, duplicate authority, overbroad APIs, repeated validation, dead compatibility, and one-use frameworks after the candidate works.
user-invocable: false
---

# Simplify candidate

Simplify without weakening proof, rollback, production behavior, or clarity. A clean `ALREADY_MINIMAL` result is valid.

Any simplification that changes production code, behavior, configuration, generated output, or proof artifacts creates a new candidate. Route it through `improve-test-suite` so affected proof executes on the simplified revision before candidate review. Pure analysis that makes no candidate change may proceed as `ALREADY_MINIMAL`.

## Routes

- `SIMPLIFIED` / `PROOF_CHANGED` → `improve-test-suite`, then `review-candidate` after current proof
- `ALREADY_MINIMAL` → `review-candidate`
- `MATERIAL_DESIGN_CHANGE` → `prepare-issue`
- `NOT_PROVEN` → preserve the missing candidate identity or proof result
