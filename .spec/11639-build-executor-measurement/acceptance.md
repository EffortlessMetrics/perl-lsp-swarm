# Acceptance: #11639 — deterministic executor-model measurement protocol

## Claim

One versioned deterministic measurement contract (`build_executor_measurement.v1`)
and a repository-owned harness exist in `xtask::build_measurement`. The harness can
prepare and execute one declared experiment cell through injected providers while
retaining exact subject, environment, cache, storage, process, and timing
identities, and renders deterministic JSON and human projections from one typed
result. No build command, cache path, wrapper, lock, executor, or policy behavior
changes.

## Explicit non-claims

- No executor model is recommended or selected.
- No native-POSIX or native-Windows observation matrix is captured here.
- `scripts/cargo-safe`, `justfile`, devplane paths, locks, and cache policy are
  byte-identical to `main@32a40405fb` (candidate basis; unchanged from the
  lane-start basis `f0c15033dd`).
- No production crate behavior changes; only `xtask` gains a library module.

## Acceptance predicates (each maps to executable proof)

1. Protocol version constant `build_executor_measurement.v1` appears in every
   rendered record.
2. All eight `execution_model` variants, three `workflow_class` variants, and
   seven `operation` variants are representable; workflow classes never collapse
   (distinct enum arms, no `From` collapse).
3. Subject identity fields (repository, commit, worktree, package, target,
   features, default-features, toolchain, profile, test-runner profile) are
   load-bearing: `satisfies_row` rejects any differing field.
4. Cache attribution requires fresh counter snapshots under one server/process
   identity; an intervening unrelated user forces `Unattributed` → NOT_PROVEN.
5. Timing decomposition separates preparation, admission/queue/lock wait,
   execution, reporting, and total wall; reconciliation rejects a record whose
   phases do not sum to total within the declared tolerance, and rejects a record
   missing the wait segment.
6. Every actual growth path records its own filesystem identity; disk admission
   on a different filesystem than the growth path is refused.
7. Missing lock primitive yields `LockObservation::unavailable` → NOT_PROVEN,
   never a locked-success; missing process/metrics instruments yield NOT_PROVEN,
   never zero.
8. Host profiles remain distinct; a `wsl_or_git_bash` record cannot satisfy a
   `native_windows` row.
9. Proof cells require observed selected work matching the expected work;
   zero-work with exit success is NOT_PROVEN.
10. Canonical cell identity is input-order independent (feature/path lists
    normalize deterministically).
11. Raw-evidence digest and normalized-interpretation digest are computed and
    kept separate.
12. JSON and human projections derive from one typed record; rendering twice is
    byte-identical.
13. Struct serialized shape matches
    `.ci/receipts/schemas/build-executor-measurement.v1.schema.json`
    (`required`/`properties` cross-check, `additionalProperties: false`).

## Proof commands

```bash
cargo fmt -p xtask -- --check
cargo test -p xtask --all-targets --locked build_measurement
cargo clippy -p xtask --all-targets --locked -- -D warnings
cargo xtask check-devex-docs
cargo run -p xtask -- non-rust check   # tracked non-Rust policy (inventory regenerated)
git diff --check
```

`cargo test -p xtask --all-targets --locked build_measurement` must match ≥1 test
(the falsifier suite under `build_measurement::`).
