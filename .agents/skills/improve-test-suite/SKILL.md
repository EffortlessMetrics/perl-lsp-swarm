---
name: improve-test-suite
description: Explicit atomic skill for hardening proof after implementation by finding realistic wrong candidates that still pass and moving proof to the cheapest effective layer.
---

# Improve test suite

Use the actual candidate to identify weaknesses the pre-build proof could not expose.

Ask:

- What realistic incorrect implementation still passes?
- Did implementation create new branch, state, stale, scope, failure, or recovery paths?
- Does the proof exercise production composition where claimed?
- Are negative and opposite-direction controls present?
- Can broad or slow proof be replaced by a focused oracle without weakening the claim?
- Did the candidate reveal a material requirement or ownership error?

Add or strengthen only proportionate discriminating proof.

## Routes

- `TEST_SUITE_HARDENED` / `ALREADY_ADEQUATE` → `$simplify-candidate`
- `WEAK_OR_CIRCULAR_ORACLE` → `$prepare-proof`
- `MATERIAL_REQUIREMENT_CHANGED` → `$prepare-issue`
- `NOT_PROVEN` → preserve the missing instrument
