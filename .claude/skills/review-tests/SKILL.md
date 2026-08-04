---
name: review-tests
description: Adversarially challenge proposed or existing proof for realistic wrong implementations, circular or vacuous oracles, production reachability, and economics.
user-invocable: false
---

# Review tests

Require observed execution evidence: the proof ran as an instrument, failed against current/wrong behavior for the intended reason, and relevant controls ran without vacuous failure. The review is directed at the applicable proof questions, falsifying of realistic wrong implementations, and verified by observed execution or competent authority. Then try to falsify the oracle using a test adversary, external authority, production-path review, or mutation-style analysis where useful.

A clean review is valid. An unexecuted draft or failed instrument is `NOT_PROVEN`, not proof-ready.

## Routes

- `PROOF_EXECUTION_OBSERVED_AND_ADEQUATE` → `build-candidate`
- `WEAK_PROOF` → `spec-to-test`
- `DRAFT_NOT_EXECUTED` / `INSTRUMENT_FAILURE` → `NOT_PROVEN`
- `REQUIREMENT_OR_OWNER_CHANGED` → `prepare-issue`
- `NO_EXECUTABLE_PROOF_SUBJECT` → `deliver-pr`
- `NOT_PROVEN` → preserve the missing oracle or instrument
