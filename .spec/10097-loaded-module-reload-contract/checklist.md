# Implementation Checklist: #10097 — loaded-module reload contract (R01)

## Change order

This is a contract/types/fixtures change inside `crates/perl-dap` plus one
ADR and this spec bundle. No runtime behavior changes.

### Step 1: Write the fail-closed fixtures first

- **Files:** `fixtures/classification/*` (fifteen documents reaching all
  thirteen dispositions), `fixtures/transactions/*` (six outcome/phase/
  generation-effect documents covering all four terminal kinds),
  `fixtures/negative_controls/*` + `expected_errors.json` (the ten
  negative controls with exact reason codes).
- **Change:** encode the corpus before the contract logic is trusted, so
  every disposition, effect, and refusal code is pinned by a document the
  implementation must reproduce.

### Step 2: Pin the closed vocabularies

- **File:** `schemas/loaded_module_reload.v1.schema.json`.
- **Change:** draft-2020-12 `$defs` for every closed vocabulary
  (dispositions, classifications, phases, causes, outcome kinds,
  generation effects, object kinds, invalidation dispositions, mechanisms,
  claims, surface violations, error codes, control ops) and the three
  document shapes. The schema describes the corpus, not a DAP wire format
  (#10138 owns wire).

### Step 3: Implement the typed contract module

- **Files:** `crates/perl-dap/src/reload/` (`subject.rs`, `eligibility.rs`,
  `transaction.rs`, `generation.rs`, `invalidation.rs`, `surface.rs`,
  `mechanism.rs`), module wiring in `crates/perl-dap/src/lib.rs`, and a
  `pub(crate)` single-authority re-export of the standard command list
  from `debug_adapter/dispatch.rs` for the collision check.
- **Change:** the six contract types plus supporting closed vocabularies
  and pure functions; no serde derives on the contract types (the wire
  format stays with #10138), no runtime wiring, no capability change.

### Step 4: Bind the corpus to the module

- **File:** `crates/perl-dap/src/reload/fixture_tests.rs`.
- **Change:** fixture-driven tests that run every classification,
  transaction, and negative-control document through the module, verify
  `expected_errors.json` covers exactly the controls, and check the
  schema's enums against the Rust vocabularies in exact sync.

### Step 5: Record the decision

- **Files:** `docs/adr/0046-loaded-module-reload-semantics.md` plus the
  ADR index row in `docs/adr/README.md`.
- **Change:** the decision record for subject identity, eligibility and
  precedence, the possibly-applied boundary, generation semantics, the
  invalidation table, protocol requirements, and mechanism limits, with
  live-tree evidence citations.

### Step 6: Run the focused proof

- `cargo fmt -p perl-dap -- --check`
- `cargo clippy -p perl-dap --lib --locked -- -D warnings`
- `cargo test -p perl-dap --lib reload --locked`
- `cargo test -p perl-dap --locked`

## Verification notes

- The fixture harness reads this bundle relative to `CARGO_MANIFEST_DIR`;
  it is in-repo proof (same shape as the `.spec/10690` crate-test
  precedent), not a published-package check.
- The schema-sync test makes the schema descriptive rather than
  duplicative: any vocabulary widened in the Rust enums without the schema
  (or vice versa) fails the sync test instead of drifting silently.
- The negative controls are wrong-candidate documents: each names the
  exact reason code the contract must produce, so an implementation that
  accepts basename-only identity, dirty-as-saved subjects, active frames,
  unsupported classes, compile-success-as-reload, raw client input, an
  invented capability, or availability-as-authority fails with a precise
  diff rather than passing vacuously.
