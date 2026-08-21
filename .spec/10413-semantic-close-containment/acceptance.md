# #10413 — Semantic close containment acceptance

## Candidate boundary

This PR accepts the CP00 evaluator/fixtures prerequisite: a trusted-base-safe standalone validator with deterministic offline proof. It does not claim live trusted enforcement. A separate follow-up must add and verify the base-owned `pull_request_target` workflow, including exact-base execution and exact candidate-head proof, before live enforcement is accepted.

## Stable acceptance rows

### CP00-001 — no terminal relation

A PR with no supported automatic closing relation returns `PASS_NOT_APPLICABLE` without issue/domain lookup.

### CP00-002 — phase/partial/slice contradiction

A terminal relation is rejected when the same issue's stable boundary says Phase N only, partial, slice only, or bounded slice. An issue whose own complete denominator is explicitly that phase remains valid at containment level.

### CP00-003 — explicitly unproved required work

A terminal relation is rejected when the stable claim boundary explicitly says full/complete/remaining issue work is not proved, established, or claimed.

### CP00-004 — remaining work names the same issue

A terminal relation is rejected when `Remaining work` or a recognized stable remaining-work heading points to the same issue.

### CP00-005 — controller packet reference

A controller/programme terminal relation requires a `Governing contract` section that explicitly references a semantic close packet for that issue. Presence passes containment only; CP03 decides sufficiency.

### CP00-006 — predecessor/successor identity

An issue line identifying the candidate PR as `Historical deletion` or `Historical predecessor` prevents that PR from terminally closing the surviving successor proposition.

### CP00-007 — proof-level contradiction

A terminal relation is rejected when the issue acceptance requires installed/public/packaged/presentation/release/actual-host proof and the PR's stable boundary explicitly excludes the same proof level.

### CP00-008 — ordinary atomic close

An atomic issue/PR with no listed contradiction returns `PASS_NO_HIGH_CONFIDENCE_CONTRADICTION`; the report states that semantic completion is not proved.

### CP00-009 — valid phase leaf

Issue #2624's complete denominator is Phase 1 advisory baseline establishment. A PR saying Phase 1 only may close that phase leaf at containment level.

### CP00-010 — relation independence

Multiple terminal relations produce independent rows. One contradictory row makes the aggregate red without changing valid rows' dispositions.

### CP00-011 — GitHub/instrument failure

A terminal relation whose issue cannot be fetched or decoded returns `NOT_PROVEN_GITHUB` or `INSTRUMENT_FAILURE` and fails the check. Failure never becomes pass.

### CP00-012 — untrusted text remains data

Candidate title/body, source lines, issue text, shell fragments, workflow expressions, and path-looking strings are never executed or used to construct commands or filesystem paths.

### CP00-013 — exact replacement relation

Each contradiction reports a non-terminal replacement: `Advances #N` for bounded progress or `Refs #N` for historical predecessor evidence.

### CP00-014 — explicit retirement mapping

Every CP00 rule names its CP03 replacement. CP00 fixtures replay against CP03/CP04 before the independent check is removed.

## Result vocabulary

```text
PASS_NOT_APPLICABLE
PASS_NO_HIGH_CONFIDENCE_CONTRADICTION
FAIL_PHASE_TERMINAL_RELATION
FAIL_EXPLICIT_UNPROVEN_REQUIRED_WORK
FAIL_REMAINING_WORK_SAME_ISSUE
FAIL_CONTROLLER_PACKET_MISSING
FAIL_PREDECESSOR_SUCCESSOR_COLLAPSE
FAIL_PROOF_LEVEL_CONTRADICTION
NOT_PROVEN_GITHUB
INSTRUMENT_FAILURE
```

## Exit contract

```text
0  pass / not applicable
2  supported high-confidence contradiction
3  GitHub or instrument evidence not proven
```

A containment pass is never a semantic-close receipt.

The CP00 rows and fixtures remain subject to the CP03 retirement mappings above. The follow-up workflow does not replace or weaken those mappings; it only supplies the live enforcement path after this prerequisite lands.
