# Acceptance: #11138 direct-call resolution assessment (slice 1)

## Satisfied by this slice

- [x] One typed `DirectCallableResolutionAssessment` contract exists below diagnostics/LSP.
- [x] Evidence axes (call shape, target resolution, dynamic boundary, coverage) remain
  distinct inside the assessment record.
- [x] `exact_unresolved` is impossible from untyped empty optional/vector inputs in the
  derivation path covered by falsifiers.
- [x] Method-call syntax class cannot yield `exact_unresolved`.
- [x] External/unindexed coverage cannot collapse into `exact_unresolved`.
- [x] Deterministic assessment digest over subject and axis fields (ordered fields only).
- [x] Direct-call query family registered against the #8911 requirement registry shape.

## Deferred (later slices)

- [ ] Full positive binding observations from canonical workspace queries.
- [ ] Complete call-class denominator from accepted `SemanticQueryView` integration.
- [ ] Qualified-call classifier convergence and #11145 handoff fields.
- [ ] Full fixture matrix from the issue body.
