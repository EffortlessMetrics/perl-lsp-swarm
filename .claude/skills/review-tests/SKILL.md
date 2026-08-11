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

## Narrowing a detector requires proof in both directions

When a change narrows a gate, lint, scanner, filter, or predicate to remove false
positives, silence is the expected outcome either way: the fix and an over-broad cut
look identical in CI. Require both directions before accepting it.

```text
the reported false positive no longer fires
a known true positive still fires against the narrowed detector
```

The second is the load-bearing one and is the one usually skipped. Supply it as a
retention control the narrowed detector must still catch — a real prior finding where
one exists, otherwise a constructed case matching the shape the detector owns. Where the
narrowing folds, joins, strips, or normalizes input before matching, construct the case
that survives the transformation, since that is where a narrowed detector goes silent.

The retained case must traverse the specific predicate or transformation being narrowed, not merely trigger an unaffected rule in the same scanner.

A detector that no longer fires is not evidence that it works. Converting a noisy
control into a quiet one is worse than the false positives, because nothing downstream
can tell the difference.

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

The lane root posts. Focused subagents and Team members return file/line-anchored proof
findings as evidence and do not write to GitHub themselves.

Keep reviewer identity/topology, raw logs, temporary mutants, retries, and clean routine
results runtime-local. Do not post one summary per reviewer or proof run.

## Routes

- `PROOF_EXECUTION_OBSERVED_AND_ADEQUATE` → `build-candidate`
- `WEAK_PROOF` → `spec-to-test`
- `DRAFT_NOT_EXECUTED` / `INSTRUMENT_FAILURE` → `NOT_PROVEN`
- `REQUIREMENT_OR_OWNER_CHANGED` → `prepare-issue`
- `NO_EXECUTABLE_PROOF_SUBJECT` → `deliver-pr`
- `NOT_PROVEN` → preserve the missing oracle or instrument
