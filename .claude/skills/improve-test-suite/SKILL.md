---
name: improve-test-suite
description: Harden proof against the actual implementation and move it to the cheapest effective layer without weakening the claim.
user-invocable: false
---

# Improve test suite

Find realistic wrong candidates that still pass, new failure/recovery paths, production-composition gaps, and overbroad proof. Strengthen only proportionate discriminating evidence.

## Routes

- `TEST_SUITE_HARDENED` / `ALREADY_ADEQUATE` → `simplify-candidate`
- `WEAK_OR_CIRCULAR_ORACLE` → `prepare-proof`
- `MATERIAL_REQUIREMENT_CHANGED` → `prepare-issue`
- `NOT_PROVEN` → preserve the missing instrument
