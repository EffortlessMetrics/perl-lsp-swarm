---
name: review-tests
description: Adversarially challenge proposed or existing proof for realistic wrong implementations, circular or vacuous oracles, production reachability, and economics.
user-invocable: false
---

# Review tests

Try to falsify the proof. Use a test adversary, external oracle, production-path review, or mutation-style analysis when it changes the detection surface. A clean review is valid.

## Routes

- `PROOF_READY` → `build-candidate`
- `WEAK_PROOF` → `spec-to-test`
- `REQUIREMENT_OR_OWNER_CHANGED` → `prepare-issue`
- `NO_EXECUTABLE_PROOF_SUBJECT` → `deliver-pr`
- `NOT_PROVEN` → preserve the missing oracle or instrument
