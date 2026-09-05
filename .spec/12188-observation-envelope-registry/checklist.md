# Checklist: #12188

- [x] Verify currentness: no existing PR for #12188; module is additive beside `compiler_profile_contract.rs` (#12186 landed, PR #12427).
- [x] Placement decision recorded (`context.md`): `xtask`, composing the landed vocabulary.
- [x] Define identity newtypes: `ObservationDigest`, `ObservationIdentity`, `ReceiptFamily`, `ReceiptId`, `SchemaVersion`, `AdapterId`, `AdapterVersion`, `ProducerAndSchemaIdentity`, `CanonicalReceiptReference`, `AdapterIdentity`.
- [x] Define subject identity: `SubjectDimensionKind` (8 closed), `SubjectDimension` (proven/not_proven), `CandidateSubjectIdentity` (absent stays explicit `not_proven`).
- [x] Define independent closed dispositions: `ObservationClass`, `ObservationDisposition` (8), `CurrentnessDisposition`, `CompletenessDisposition`, `WorkDisposition`, `LimitationDisposition`, `ObservedClaimCeiling`, `InvalidationEvidence`, `InstrumentAndTerminalState`/`TerminalState`.
- [x] Define `CompilerProfileObservationV1` with `validate`, `canonical_semantic_text`, `identity`.
- [x] Define `ObservationAdapterDescriptor` + `ObservationAdapterRegistry` with `register`, `from_descriptors`, `select_adapter`, `validate_observation`, `canonical_text`, `semantic_fingerprint`.
- [x] Write falsifier tests for all 12 issue falsifiers (see `inventory.md` mapping).
- [x] Add synthetic fixtures only; the registry ships empty (no concrete adapter).
- [x] Focused proof green: `cargo test -p xtask --locked compiler_profile_observation`, `cargo test -p xtask --locked compiler_profile_contract` (shared-tag widening), `cargo clippy -p xtask --lib --locked -- -D warnings`, `cargo fmt -p xtask -- --check`, `git diff --check`.
- [ ] E02–E06 register concrete receipt-family adapters (follow-up issues, not this PR).
- [ ] E07 assembles evidence sets; E08 evaluates rows (follow-up issues, not this PR).

## Writer conflicts / rollback / stop conditions

- Single candidate branch `contract/12188-observation-envelope`; one writer.
- Parallel lane #12330 owns the row inventory module; shared files (`lib.rs` one `pub mod` line, two `pub` tag widenings in `compiler_profile_contract.rs`) are additive-only; first-merged wins, the other rebases.
- Rollback = drop the additive module + `.spec` packet + one `mod` line + two `pub` keywords; no runtime state to unwind.
- Stop conditions honored: no concrete receipt-family adapter, no manifest loading, no row evaluation, no receipt discovery from logs, no proof execution, no serde/file syntax, no CLI, no compiler/provider/support/release/publication behavior.
