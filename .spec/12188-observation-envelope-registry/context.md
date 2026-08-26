# Context: #12188 — normalized evidence observation contract and adapter registry

Parent/controller: #12177. Train row: COMP-PROFILE-E01. Depends on: #12186
(landed, PR #12427). Feeds: receipt-family adapters E02–E06, evidence-set
assembly E07, evaluator E08.

Maintained-profile evaluation needs one normalized, private-safe observation
envelope before heterogeneous canonical receipts can be compared honestly.
Without it, each receipt-family adapter would invent its own normalization
and flatten distinct source vocabulary (observed-red-but-complete,
instrument failure, zero work, accepted debt, conditional non-selection)
into pass/not-applicable, and claims like "unknown schema fails closed" or
"an adapter may narrow but never strengthen the source claim" would have no
executable home.

This claim defines, in `xtask/src/compiler_profile_observation.rs`:

1. The versioned envelope `CompilerProfileObservationV1` with
   `ObservationIdentity`, `CanonicalReceiptReference` (reference + digest,
   never payload), `CandidateSubjectIdentity` (8 closed dimensions, absent
   stays explicit `not_proven`), `ProducerAndSchemaIdentity`,
   `ObservationClass` (composing the landed `ClaimFamily` × `ProofClass`),
   and independent closed dispositions: `ObservationDisposition` (8
   variants), `CurrentnessDisposition`, `CompletenessDisposition`,
   `WorkDisposition`, `LimitationDisposition`, `ObservedClaimCeiling`
   (composing the landed `ClaimCeiling`), `InvalidationEvidence` (reusing
   the landed `InvalidationInput`), and `InstrumentAndTerminalState`.
2. The deterministic `ObservationAdapterRegistry` and
   `ObservationAdapterDescriptor`: stable adapter id/version, one accepted
   source family and inclusive schema range, source authority, emitted
   classes, provable subject dimensions, preserved fields, lossiness,
   source/observation claim ceilings, required currentness inputs,
   explicitly unsupported versions, and an optional `supersedes` migration
   relation.
3. The closure laws as executable behavior: private-safe free text (no
   host paths, issue/PR/workflow colour, log prose); envelope identity
   order-insensitive but content-sensitive (`identity()` over canonical
   semantic text); registry fingerprint independent of registration order;
   fail-closed selection for unknown/future/unsupported/ambiguous schema;
   narrowing-only claim ceilings; non-completed instruments and zero work
   can never be typed pass/not-applicable; accepted debt and closed
   non-claiming dispositions can never carry more than observed evidence.
4. Synthetic fixtures only — no concrete receipt-family adapter ships; the
   registry starts empty and E02–E06 register their own descriptors.

## Placement decision

The contract lands in `xtask` (`xtask/src/compiler_profile_observation.rs`),
exposed through `xtask/src/lib.rs`, beside the landed #12186 model it
composes with:

- every current consumer is xtask-level contract tooling (E02–E08); no crate
  below `xtask` consumes observation envelopes today;
- the issue's verification line targets `cargo test -p xtask --locked
  compiler_profile_observation`, which the library test target satisfies;
- composing the landed #12186 vocabulary from the same crate avoids a second
  type vocabulary; two canonical tags (`ClaimCeiling::tag`,
  `InvalidationKind::tag`) are widened to `pub` in
  `compiler_profile_contract.rs` so both contracts share one canonical-text
  idiom.

If a future consumer below `xtask` appears, the module can be lowered
together with `compiler_profile_contract` without changing the vocabulary.

## Parallel-lane boundary

A parallel lane owns the C02 initial-row inventory (#12330) in the same
crate. The module boundary is exact: this lane owns
`compiler_profile_observation.rs` (observation/envelope/registry); the
inventory lane owns row inventory. Shared files are additive-only: one
`pub mod` line in `xtask/src/lib.rs` and two `pub` widenings in
`compiler_profile_contract.rs`.
