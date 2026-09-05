# Implementation Checklist: #10858 — shared typed train edge and claim-profile contracts

## Change order

This is a specification/data change plus one focused xtask validator. Each
step is reviewable without building product crates.

### Step 1: Write the fail-closed fixtures first

- **Files:** `fixtures/train_edge_contract/*` (valid documents, the shuffled
  canonical twin, and the invalid set with `expected_errors.json`).
- **Change:** encode the ten required #10858 fixtures and the negative
  controls as programme-neutral documents before any validator logic is
  trusted; each invalid document names the exact reason code it must produce.

### Step 2: Define the closed contract

- **File:** `schemas/train_edge_contract.v1.schema.json`.
- **Change:** pin the closed edge vocabulary, the four external stages, the
  base reason traits, the derives-from provenance, the claim-profile shape,
  and the four independent projection tracks as a draft-2020-12 schema.

### Step 3: Implement the deterministic validator

- **File:** `xtask/src/tasks/train_edge_contract.rs` (+ CLI wiring in
  `xtask/src/main.rs`, `xtask/src/tasks/mod.rs`).
- **Change:** closed-vocabulary validation with stable reason codes,
  canonical semantics, profile requirement expansion and eligibility
  evaluation, and the landed-manifest adaptation checks; falsifier suite
  covers every required fixture and negative control as unit tests.

### Step 4: Declare the landed-manifest adaptations

- **File:** `.spec/10858-train-edge-contract/adaptations.json`.
- **Change:** bind every dependency class of the three landed manifests to
  exactly one shared kind (external → `external_checkpoint` at
  `manual_authorization`, per the controller's explicit-authorization gate);
  unknown classes fail closed.

### Step 5: Run the focused proof

- `cargo test -p xtask --bin xtask train_edge_contract --locked`
- `cargo clippy -p xtask --all-targets --locked -- -D warnings`
- `cargo fmt -p xtask -- --check`
- `cargo xtask check-train-edge-contract` (manual run path)

## Verification notes

- The validator never rewrites manifest bytes; adaptation is read-only and
  lossless (target + provenance preserved, counts reported).
- The shuffled control proves array order never changes canonical semantics.
- The unknown-class mutation proves a future manifest class without a
  declared adaptation row fails closed instead of normalizing silently.
