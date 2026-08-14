---
name: review-tests
description: Explicit atomic skill for adversarially challenging proposed or existing proof before implementation or before treating a candidate as adequately protected.
---

# Review tests

Try to falsify the proof.

This is a directed, falsifying, and verified proof review: work the applicable
questions, actively seek a realistic wrong implementation that passes, and rely on
observed execution or competent authority rather than an impression or CI status.

Require observed execution evidence before declaring proof ready:

- the test or proof artifact executed successfully as an instrument;
- it failed against the current/wrong behavior or a controlled realistic wrong implementation for the intended reason;
- relevant controls executed and did not fail vacuously;
- the evidence identifies the fixture, command/instrument, and observed result.

## Narrowing a detector requires proof in both directions

When a change narrows a gate, lint, scanner, filter, or predicate to remove false
positives, silence is the expected outcome either way: the fix and an over-broad cut are
indistinguishable in CI. Require both directions before accepting it.

```text
the reported false positive no longer fires
a known true positive still fires against the narrowed detector
```

The second is load-bearing and is the one usually skipped. Supply it as a retention
control the narrowed detector must still catch — a real prior finding where one exists,
otherwise a constructed case matching the shape the detector owns. Where the narrowing
folds, joins, strips, or normalizes input before matching, construct the case that
survives that transformation, because that is where a narrowed detector goes silent.

The retained case must traverse the specific predicate or transformation being narrowed, not merely trigger an unaffected rule in the same scanner.

A detector that stops firing is not evidence that it works. Turning a noisy control into
a quiet one is worse than the false positives, since nothing downstream can tell the
difference.

Then ask:

- What realistic incorrect implementation still passes?
- Is the oracle independent, or does it merely restate the code?
- Can the test pass vacuously or against an empty path?
- Is the opposite direction represented?
- Are stale, wrong-scope, failure, and recovery cases included where material?
- Does the proof exercise the claimed production seam?
- Is this the cheapest effective proof layer?
- What does the proof not establish?

A clean proof review is valid. Do not add broad tests merely to demonstrate effort.

## Orchestration affordances

### Lane-root decisions

The lane root retains the proof-sufficiency judgment and decides whether findings require
proof repair, issue correction, candidate work, or an explicit `NOT_PROVEN` boundary.
A reviewer does not approve the claim or mutate proof merely by returning a clean
result.

### Useful read-only reviewers

Use differentiated read-only work where useful:

- test adversary constructing realistic wrong implementations;
- external-oracle reviewer;
- production-path tracer;
- denominator/schema/receipt/instrument-integrity reviewer;
- mutation-style or opposite-direction analysis;
- proof-economics reviewer.

These lenses may run independently. They must name searched scope, evidence, falsifiers,
and uncertainty. Repeated conclusions from one source are not separate evidence.

### Mutation owner and join

Reviewers are read-only by default. Accepted proof mutations return to the current proof
writer through `$spec-to-test` or `$prepare-proof`.

Join when the lane root can state:

- which realistic wrong implementations the proof excludes;
- which oracle and production seam the result uses;
- why controls make vacuity visible;
- what evidence conflicts or remains missing;
- whether the proof is adequate, weak, unavailable, or has no executable subject.

### Return packet

Return proof/candidate identity, reviewed instrument and execution, findings by affected
proof dimension, direct and contradictory evidence, realistic falsifiers, searched
scope, limitations, `NOT_PROVEN` boundary, recommended route, and stable overflow
references.

## GitHub boundary

Use an issue/PR comment or review finding when a proof defect, external oracle,
production-path fact, or `NOT_PROVEN` boundary will affect implementation, review,
support, or later resumption. Localized candidate-test findings may use inline review.

The integrating lane owner posts. A skill run that only answers a bounded proof question
returns file/line-anchored findings as evidence and does not write to GitHub.

Keep reviewer identity, topology, raw logs, temporary mutants, retries, and clean
routine results runtime-local. Do not post one summary per reviewer or one comment per
proof run.

## Routes

- `PROOF_EXECUTION_OBSERVED_AND_ADEQUATE` → `$build-candidate`
- `WEAK_PROOF` → `$spec-to-test`
- `DRAFT_NOT_EXECUTED` / `INSTRUMENT_FAILURE` → `NOT_PROVEN` with the missing execution evidence
- `REQUIREMENT_OR_OWNER_CHANGED` → `$prepare-issue`
- `NO_EXECUTABLE_PROOF_SUBJECT` → return to `$deliver-pr`
- `NOT_PROVEN` → preserve the missing oracle or instrument
