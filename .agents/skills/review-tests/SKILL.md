---
name: review-tests
description: Explicit atomic skill for adversarially challenging proposed or existing proof before implementation or before treating a candidate as adequately protected.
---

# Review tests

Try to falsify the proof.

Ask:

- What realistic incorrect implementation still passes?
- Is the oracle independent, or does it merely restate the code?
- Can the test pass vacuously or against an empty path?
- Is the opposite direction represented?
- Are stale, wrong-scope, failure, and recovery cases included where material?
- Does the proof exercise the claimed production seam?
- Is this the cheapest effective proof layer?
- What does the proof not establish?

A clean proof review is valid. Do not add broad tests merely to demonstrate effort.

## Orchestration

A test adversary, external oracle, production-path reviewer, or mutation-style analysis may run independently. Join into one proof conclusion.

## Routes

- `PROOF_READY` → `$build-candidate`
- `WEAK_PROOF` → `$spec-to-test`
- `REQUIREMENT_OR_OWNER_CHANGED` → `$prepare-issue`
- `NO_EXECUTABLE_PROOF_SUBJECT` → return to `$deliver-pr`
- `NOT_PROVEN` → preserve the missing oracle or instrument
