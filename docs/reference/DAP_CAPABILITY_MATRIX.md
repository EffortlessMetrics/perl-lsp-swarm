# DAP Initialize Capability Inventory

Owner: #6688

This inventory records the exact fields emitted by the production `initialize`
response, the controlling Rust expression, and the value's wire shape. It is a
drift boundary, not a runtime-support verdict.

Current production emits **34** capability or capability-configuration fields:

- **33 boolean wire fields**;
- **1 array wire field**, `exceptionBreakpointFilters`;
- **33 standard DAP fields** present in the pinned upstream `Capabilities`
  definition;
- **1 project extension**, `supportsInlineValues`;
- **7 fixed false** fields;
- the remaining standard claims are mostly **catalog-derived, not
  backend-derived**.

The machine-readable source is `.ci/dap/capability-matrix.json`.

## Independent authority

The matrix cannot choose the source it validates. The checker independently
pins all three coordinates:

```text
crates/perl-dap/src/debug_adapter/process.rs
let capabilities = json!({
Capabilities
```

A different path, absolute path, escaping path, alternate anchor, or upstream
definition fails before row reconciliation. The anchor must occur exactly once.

The production object is parsed as a complete Rust token boundary. Every
top-level entry must be a quoted capability name followed by one canonical
identifier or the literal `false`. Same-line fields, line breaks, and comments
are accepted. Unconsumed fragments, complex expressions, duplicate fields,
unbalanced delimiters, or a malformed `json!` close fail rather than
disappearing from the inventory.

## Standard and extension classification

A standard row must exist in the `Capabilities.properties` object of the exact
upstream schema pinned by #6737. Its recorded `wire_type` must exactly equal the
upstream schema type. Membership alone is insufficient.

The integration contract also executes the production `initialize` handler and
compares the actual `serde_json::Value` kind for every emitted field with the
matrix. This catches a boolean expression changed to an array, an array changed
to a scalar, or any row whose runtime JSON shape no longer matches upstream.

`supportsInlineValues` is the single current capability extension. It must
remain absent from upstream, explicitly classified as a project extension,
boolean-shaped, and owned by #2374. It does not count toward standard DAP
conformance.

## Advertisement bases

### Fixed false

A fixed false row is present on the wire but explicitly unsupported. Its
expression must remain the literal `false`, and its wire shape must remain
boolean.

### Catalog-derived, not backend-derived

These fields are currently controlled by feature-catalog booleans or broad
derived booleans such as `supports_core`. The matrix records that mechanism
without asserting that the selected native or external backend can perform the
operation.

#6688's end state remains:

```text
frontend wire support
∩ selected backend implementation
∩ selected backend mode
∩ validated runtime/configuration prerequisites
∩ behavior-backed proof
= advertised capability set
```

## Candidate and receipt binding

The hosted gate verifies the exact candidate SHA before it executes repository
code and again before publication. It requires a clean tracked and untracked
tree after the Python falsifiers, Rust contract test, and validator.

Every authority subject is compared byte-for-byte with its Git object at that
candidate. The receipt records:

- candidate SHA;
- CI run ID and attempt;
- Git blob SHA-1 and SHA-256 for the matrix, production source, pinned authority
  manifest, workflow, guide, validator modules, falsifiers, and Rust contract;
- the exact production expression inventory;
- the upstream schema content identity;
- every row's classification, expression, basis, owner, matrix wire shape, and
  observed upstream type.

The artifact name and step summary are convenience surfaces. The durable JSON
contains the identity needed to distinguish receipts from different commits or
runs.

## Verification

```bash
python3 scripts/tests/test_dap_capability_matrix.py

cargo test -p perl-dap \
  --test dap_capability_matrix_contract \
  --locked

python3 scripts/ci/dap_capability_matrix.py \
  --root . \
  --matrix .ci/dap/capability-matrix.json \
  --authority-manifest .ci/dap/protocol-authority.json \
  --repository-sha "$(git rev-parse HEAD)" \
  --run-id 1 \
  --run-attempt 1 \
  --receipt target/receipts/dap-capability-matrix.json
```

The checker fails when:

- production emits a field without a row, or a row no longer exists in
  production;
- the canonical source coordinates, controlling expression, or wire shape
  drift;
- any production syntax fragment cannot be classified;
- a standard row is absent upstream or has a different upstream type;
- a project extension appears upstream and needs intentional reclassification;
- a fixed-false or extension invariant changes;
- a row lacks a bounded owner;
- repository code mutates a tracked or untracked candidate subject;
- the receipt cannot be bound to the candidate SHA and CI run.

## Next slices

1. Attach request, response, event, and backend methods to each row.
2. Record native and ptkdb-mode verdicts separately.
3. Add runtime and configuration prerequisites with explicit unsupported
   reasons.
4. Bind positive and negative behavior receipts from #6684 and #4786.
5. Generate `initialize` from the accepted intersection instead of maintaining
   catalog-derived booleans in parallel.

## Claim boundary

This inventory proves that the current wire surface is complete, structurally
consumed, wire-shape-checked, and candidate-bound relative to the pinned schema.
It does not prove that a catalog-derived true field is behaviorally supported.
Capability correction remains #6688; backend implementation remains
#4785/#4786; real-session proof remains #6684.
