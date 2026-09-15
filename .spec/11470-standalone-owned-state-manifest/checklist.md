# Implementation Checklist: #11470 — standalone owned-state manifests and safe removal plans

## Gate

### Step 0: Scope verification (blocking, permanent)

- **Check:** this lane never deletes, moves, or mutates PATH/profile/registry/
  current-selection state. The validator parses documents only. Removal
  execution belongs to #11471/#11472; activation to #11417; scanning to
  #11425–#11430.
- **If violated:** stop and revert the offending change; the contract claim
  does not authorize filesystem effects.

## Landed in this bundle (already executed)

1. `.spec/11470-standalone-owned-state-manifest/{context,acceptance,checklist}.md`.
2. `schemas/standalone_owned_state.v1.schema.json`,
   `schemas/standalone_removal_plan.v1.schema.json`,
   `schemas/standalone_uninstall_result.v1.schema.json` — closed documents.
3. `fixtures/experience/install_owned_state/*.json` — six positive scenario
   manifests, four negative-control manifests (unknown role, unbounded
   identity, ambiguous running, traversing root), four plans (two valid by
   lifecycle policy, one all-preserve blocked plan, one stale-binding
   negative), two coherent results.
4. `xtask/examples/standalone_owned_state.rs` — checked validator: closed
   typed structs, manifest coherence rules (canonical absolute roots,
   identity bounds, running-state both-direction rule, total class→retention
   function), plan totality with enforced canonical order_index sequence and
   destructive-legality matrix with exact-currentness digests, exact
   postcondition populations, result vocabulary coherence plus exact
   result⇔plan⇔manifest binding reconciliation, canonical serialization.
5. Non-Rust registration for every new file plus regenerated inventory.

## Successor change order (platform lanes #11471/#11472)

### Step S1: Reconcile against landed prerequisites

- Replace reference-only fields once #11179/#11425–#11430 land real
  candidate/selection identities and #11467–#11469 define marker formats;
  re-map rows if role spellings changed there.

### Step S2: Bind the scanner

- Produce `standalone_owned_state.v1` from a real enumeration; the validator
  is the acceptance oracle. Incomplete walks must set
  `enumeration.complete=false` with reason — never guess.

### Step S3: Plan + execute under exact currentness

- Generate `standalone_removal_plan.v1`, verify binding immediately before
  execution, recompute digests per entry at removal time, and emit one
  coherent `standalone_uninstall_result.v1`. Any movement between plan and
  execution ⇒ refuse (`root_or_manifest_mismatch`) and replan.

## Deterministic checking of this bundle (valid now)

```bash
for f in context.md acceptance.md checklist.md; do [ -f ".spec/11470-standalone-owned-state-manifest/$f" ] || exit 1; done
rg -c "SOS-C(0[1-9]|1[0-6])" .spec/11470-standalone-owned-state-manifest/acceptance.md   # expect >= 16 contract rows
rg -c "^\| [0-9]+ \| " .spec/11470-standalone-owned-state-manifest/acceptance.md        # expect exactly 10 falsifier rows
rg -n "main@cce85d167" .spec/11470-standalone-owned-state-manifest/context.md            # pinned evidence base
cargo test -p xtask --example standalone_owned_state --locked                            # 21 focused tests
python -m unittest scripts/ci/test_standalone_contract_schemas.py                        # 18 schema-law checks
cargo run -q -p xtask --example standalone_owned_state --locked -- \
  --manifest fixtures/experience/install_owned_state/manifest_canonical_full_install.json \
  --plan fixtures/experience/install_owned_state/plan_full_removal.json \
  --result fixtures/experience/install_owned_state/result_partial_failure_retryable.json # combined binding run must pass
cargo run -q -p xtask --example standalone_owned_state --locked -- \
  --manifest fixtures/experience/install_owned_state/manifest_canonical_full_install.json \
  --plan fixtures/experience/install_owned_state/plan_invalid_stale_binding.json         # must fail naming root_or_manifest_mismatch
cargo fmt -p xtask -- --check
cargo clippy -p xtask --example standalone_owned_state --locked -- -D warnings
git diff --check
```

The falsifier-row count is 10 by construction; a different rg count than the
table means NOT_PROVEN — fix the pattern before trusting it. Structural
checks run twice against the unchanged tree; identical ordered output is the
determinism proof.

## Callers and consumers

- #11471 (POSIX) and #11472 (Windows) consume manifest+plan+result documents;
  they own process handling, permissions, and marker mutation.
- #11417 gates any `not_applicable`; until selected, results use
  `conditional_activation_not_selected`.

## Scope boundary

Files IN scope are listed in context.md §Scope boundary. Everything else —
especially `install.sh`, `scripts/install.sh`, `install.ps1`,
`policy/install-surface-registry.toml`, editor/client surfaces — stays
untouched by this claim.
