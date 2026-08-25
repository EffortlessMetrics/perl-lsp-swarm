# Context: #12186 — maintained compiler operating profile identity and closure types

Parent/controller: #12176. Train row: COMP-PROFILE-C01.

The compiler-profile program needs one dependency-neutral, in-memory domain
model for maintained compiler operating profiles before any checked row
inventory, manifest format, receipt adaptation, or candidate evaluation can
exist honestly. Without a single owned vocabulary, each successor (the
initial-row inventory, the C02 manifest tooling in #12187, the #12177
evidence/evaluation train) would invent its own subtly different profile,
row, and evidence types, and closure laws such as "imports preserve every
row and limitation" or "identity changes with any semantic field" would have
no executable home.

This claim defines, in `xtask/src/compiler_profile_contract.rs`:

1. The exact identity types (`CompilerProfileId`, `CompilerProfileVersion`,
   `CompilerProfileDigest`, `CompilerProfileRowId`) and the profile/row model
   (`CompilerProfileDefinition`, `CompilerProfileImport`, `CompilerProfileRow`).
2. Closed typed states that cannot be bypassed by omission: `RowDisposition`
   (required/conditional/optional/unsupported/not-applicable), and per-row
   mandatory subject, evidence, completeness, work, limitation, legacy-exit,
   claim-ceiling, invalidation, and owner/wake-event data.
3. Three independent closed dimensions that cannot cross-satisfy through
   constructors or validation: `ClaimFamily` (13 proposition families),
   `ProofClass` (4 proof axes), `SourceTier` (5 evidence stages).
4. The closure laws as executable behavior: conjunctive required rows,
   exact import closure (`verify_import_closure`) preserving identity,
   version, digest, rows, and limitations verbatim, and deterministic
   semantic fingerprints (`semantic_fingerprint`) that change with any
   semantic row field and are independent of row/insertion order.
5. Minimal in-memory shape fixtures for the four #12176 profile classes
   (`compiler_local_lexical.v1`, `compiler_static_project.v1`,
   `compiler_bounded_execution.v1`, `compiler_maintained_code_intelligence.v1`)
   proving representability and closure only — not the checked row inventory
   and not live product state.

## Placement decision

The model lands in `xtask` (`xtask/src/compiler_profile_contract.rs`),
exposed as library API through `xtask/src/lib.rs` — the same shape as the
`actual_host_receipt` contract module — because:

- every current consumer is xtask-level contract tooling: the successor
  initial-row inventory, the #12187 manifest tooling, and the #12177
  evidence train checks; no crate below `xtask` consumes the model today;
- `perl-core-harness-types` (the candidate lower home) is chartered for
  upstream Perl core harness receipt contracts — a different domain — and
  moving the compiler-profile model there would widen that crate's charter
  for no current reverse-dependent consumer;
- the issue's verification line targets `cargo test -p xtask --locked
  compiler_profile_contract`, which the library test target satisfies;
- the related `compiler_profile.v1` capability wire contract lives in
  `xtask/src/tasks/compiler_profile.rs`; this model is additive and mirrors
  its canonical-identity idiom (order-insensitive canonical text + sha256)
  without serde, which the issue excludes from the stable model;
- library placement keeps the pure contract vocabulary reachable to future
  integration tests and successor tooling without inventing a CLI surface
  (an issue non-goal).

If a future consumer below `xtask` appears, the module can be lowered into a
shared crate without changing the vocabulary.
