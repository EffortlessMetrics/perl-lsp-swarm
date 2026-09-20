# Checklist: #12186

- [x] Verify currentness: no existing PR for #12186; module is additive beside `xtask/src/tasks/compiler_profile.rs` (capability wire contract untouched).
- [x] Placement decision recorded (`context.md`): `xtask`, not `perl-core-harness-types`.
- [x] Define identity newtypes with validating constructors: `CompilerProfileId`, `CompilerProfileVersion`, `CompilerProfileDigest`, `CompilerProfileRowId`, `SubjectRef`, `WorkScope`.
- [x] Define closed dimensions: `ClaimFamily` (13), `ProofClass` (4), `SourceTier` (5), `RowDisposition` (5 closed states).
- [x] Define row components: `SubjectSelector`, `EvidenceRequirement`, `CompletenessRequirement`, `WorkRequirement`, `AllowedLimitation`, `LegacyExitRequirement`, `ClaimCeiling`, `InvalidationInput`, `OwnerAndWakeEvent`.
- [x] Define `CompilerProfileRow`, `CompilerProfileImport`, `CompilerProfileDefinition` with `validate`, `verify_import_closure`, `canonical_semantic_text`, `semantic_fingerprint`.
- [x] Write falsifier tests for all 15 issue falsifiers (see `inventory.md` mapping).
- [x] Add the four shape fixtures forming an exact import chain.
- [x] Focused proof green: `cargo test -p xtask --locked compiler_profile_contract`, `cargo clippy -p xtask --all-targets --locked -- -D warnings`, `cargo fmt -p xtask -- --check`, `git diff --check`.
- [ ] Successor initial-row inventory instantiates this vocabulary (follow-up issue, not this PR).
- [ ] #12187 manifest tooling serializes the inventory without reinterpreting it (follow-up, not this PR).

## Writer conflicts / rollback / stop conditions

- Single candidate branch `contract/12186-compiler-profile-identity-closure`; one writer.
- Rollback = drop the additive module + `.spec` packet + one `mod` line; no runtime state to unwind.
- Stop conditions honored: no checked row inventory, no manifest/serde/file syntax, no CLI, no receipt adaptation, no candidate evaluation or status, no compiler/provider/world/EIR/client implementation, no CI requiredness, no support/release/publication action.
