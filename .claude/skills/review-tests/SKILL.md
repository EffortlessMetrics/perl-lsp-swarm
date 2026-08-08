---
name: review-tests
description: Adversarially challenge proposed or existing proof for realistic wrong implementations, circular or vacuous oracles, production reachability, and economics.
user-invocable: false
---

# Review tests

Require observed execution evidence: the proof ran as an instrument, failed against
current/wrong behavior for the intended reason, and relevant controls ran without
vacuous failure. The review is directed at applicable proof questions, falsifying of
realistic wrong implementations, and verified by execution or competent authority.

A clean review is valid. An unexecuted draft or failed instrument is `NOT_PROVEN`, not
proof-ready.

## Orchestration affordances

### Lane-root decisions

The lane root retains proof sufficiency and decides whether findings require proof
repair, issue correction, candidate work, or an explicit `NOT_PROVEN` boundary. A
subagent verdict does not approve the claim.

### Useful review contexts

Use focused subagents or context forks where useful for:

- realistic wrong-implementation construction;
- external-oracle challenge;
- production-path tracing;
- denominator/schema/receipt/instrument integrity;
- mutation-style and opposite-direction analysis;
- proof economics.

Each reviewer names searched scope, evidence, falsifiers, and uncertainty. Repeated
conclusions from one source are not separate evidence.

### Mutation owner and join

Review contexts are read-only by default. Accepted proof changes return to the current
proof writer through `spec-to-test` or `prepare-proof`.

Join when the lane root can state which wrong implementations are excluded, which
oracle and production seam apply, how vacuity is controlled, what evidence conflicts or
remains absent, and whether the result is adequate, weak, unavailable, or has no
executable subject.

### Return packet

Return proof/candidate identity, reviewed instrument/execution, findings by proof
dimension, direct and contradictory evidence, realistic falsifiers, searched scope,
limitations, `NOT_PROVEN` boundary, recommended route, and stable overflow references.

## GitHub boundary

Post when a proof defect, external oracle, production-path fact, or `NOT_PROVEN`
boundary affects implementation, review, support, or later resumption. Localized
candidate-test findings may use inline review.

Keep reviewer identity/topology, raw logs, temporary mutants, retries, and clean routine
results runtime-local. Do not post one summary per reviewer or proof run.

## Routes

- `PROOF_EXECUTION_OBSERVED_AND_ADEQUATE` → `build-candidate`
- `WEAK_PROOF` → `spec-to-test`
- `DRAFT_NOT_EXECUTED` / `INSTRUMENT_FAILURE` → `NOT_PROVEN`
- `REQUIREMENT_OR_OWNER_CHANGED` → `prepare-issue`
- `NO_EXECUTABLE_PROOF_SUBJECT` → `deliver-pr`
- `NOT_PROVEN` → preserve the missing oracle or instrument
