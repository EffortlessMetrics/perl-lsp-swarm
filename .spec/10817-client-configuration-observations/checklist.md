# Implementation Checklist: #10817 — adapt client and workspace configuration channels into typed observations

## Gate

### Step 0: Wake verification (blocking)

- **Check:** #10813 merged and current over the corrected #10807 denominator;
  #7010 consumes real JSON-RPC response envelopes and exposes exact terminal
  request-slot identity through `ServerRequestRegistry`; security owners
  #4997/#10917/#7479 have dispositions the adapter can record.
- **If unmet:** stop. Cutover stays `BLOCKED_BY_PREREQUISITE`; this packet
  remains the durable prep. Do not add an observation wrapper around post-parse
  values as a workaround.

## Change order after wake (compiles at each step)

### Step 1: Reconcile this packet against landed prerequisites

- **File:** `.spec/10817-client-configuration-observations/*` (UPDATE receipts)
- **Details:** refresh R1–R6 line receipts, field-cohort table, and fixture names
  against the actual #10813 type names and #7010 registry API.
- **Verify:** `rg -n "R[1-6]" .spec/10817-client-configuration-observations/`

### Step 2: Red provenance fixtures

- **File:** new test module near the adapter seam (name per landed contract)
- **Change:** write failing versions of F1 (`same_value_different_source...`),
  F2 (unscoped-not-trusted), F3 (root/slot/generation mismatch), F4
  (late/duplicate terminal-once) from acceptance.md §Test-Grid.
- **Verify:** `cargo test -p perl-lsp-rs --all-targets --locked <fixture>` fails for the right reason.

### Step 3: Initialization-options + didChange adapters

- **File:** perl-lsp-rs-core config observation surface (#10813 types);
  wiring in lifecycle/capabilities.rs:705-725 and runtime/workspace.rs:1311-1470
- **Details:** observation created before any effective-state call; raw parsers
  become forwarding parsers returning typed field observations for migrated fields.
- **Depends on:** Steps 0–2.
- **Verify:** Step 2 fixtures green; `cargo check -p perl-lsp-rs -p perl-lsp-rs-core`.

### Step 4: Unscoped/per-root response adapters over registry slots

- **File:** response entry replacing `$/perl-lsp/clientResponse` compatibility
  path (dispatch/routing.rs:243-246); pending tuple (runtime/types.rs:91-98)
  superseded by registry-bound slot identity
- **Details:** per-root observations bind connection/request id/slot index/root/
  generation; array order is transport detail only.
- **Depends on:** Step 3.
- **Verify:** remaining red fixtures from Step 2 green; stale/duplicate tests pass.

### Step 5: Cut high-risk cohorts first; remove direct mutation for them

- **Order:** AI enable/provider (#4997 disposition), externalIncludePaths
  (#4998), formatter engine (#5001), limits (#10917 disposition).
  testRunner family stays absent/migration-only (#11845).
- **Verify:** architecture recurrence check (step 8) fails any bypass.

### Step 6: Narrow consumptive compatibility projection (only if a consumer cannot move)

- **Constraints:** crate-private; every caller inventoried in PR body; cannot
  strengthen unauthorized/malformed/absent fields; carries observation/request/
  generation identity; removal owner named under #10387.
- **Verify:** caller inventory grep posted to the PR.

### Step 7: Redaction, determinism, envelope integration proof

- **Details:** F8/F9 plus exact-process capture proving real-envelope entry;
  run deterministic receipt generation twice byte-clean.
- **Depends on:** Steps 4–6.
- **Verify:** `cargo test -p perl-lsp-rs --all-targets --locked` twice.

### Step 8: Architecture recurrence checks + final verification

- **Verify:**

```bash
cargo test -p perl-lsp-rs-core --all-targets --locked configuration_observation
cargo test -p perl-lsp-rs --all-targets --locked workspace_configuration
cargo test -p perl-lsp-rs --all-targets --locked did_change_configuration
cargo clippy -p perl-lsp-rs-core -p perl-lsp-rs --all-targets --locked -- -D warnings
git diff --check
```

## Deterministic checking of this packet (valid now, before wake)

```bash
for f in context.md acceptance.md checklist.md; do if not exist ".spec\10817-client-configuration-observations\%f" exit /b 1; done
rg -c "CFGOBS-C(0[1-9]|1[0-7])" .spec/10817-client-configuration-observations/acceptance.md   # expect >= 17 rows
rg -n "main@ab3cece9d" .spec/10817-client-configuration-observations/context.md               # pinned evidence base
git diff --check
```

Run the structural checks twice against the unchanged tree; identical ordered
output and clean second run is the determinism proof. A missing tool or row is
`NOT_PROVEN`, never a green result.

## Callers and consumers

- Children #10898/#10909/#10917 consume the substrate this packet specifies;
  they must not start before Step 0 passes.
- #10386/#10387 consume observations without reparsing raw transport (C17).
- Review lenses after wake: transport/response, provenance/security,
  state-mutation, multi-root/currentness, boundary (per issue review plan).

## Scope boundary

Files IN scope: `.spec/10817-client-configuration-observations/context.md`,
`acceptance.md`, `checklist.md`.

Files OUT of scope: every production file named in the inventory until Step 0
passes; then only the surfaces Steps 2–8 name. Project/environment/probe
boundaries (#10818), accepted store, precedence/publication/consumer invalidation
(#10386/#10387), child bindings (#10898/#10909/#10917) remain out of scope.

## Flags for builder

- The #10807 denominator may rename source classes; re-map CFGOBS identities at
  Step 1 rather than freezing current catalog slices.
- `WORKSPACE_CONFIGURATION_REQUEST_TIMEOUT` (30s age bound) and the pending-count
  cap are useful bounded behavior; preserve their semantics in the registry-bound
  replacement unless #7010 supersedes them.
- Some clients send didChangeConfiguration settings without the top-level `perl`
  key (workspace.rs:1317-1322 dual-shape acceptance); the adapter must keep both
  shapes classified honestly rather than widening authority.
