# Implementation Checklist: #11626 slice one — offline status + safe frontier

## Current state

- [x] Fail-closed manifest loader: strict schema, C01 structural laws,
      wording laws, pinned canonical digest (`10BA2619…C104FB`).
- [x] Typed current-tree state projection with independent implementation
      presence and typed reason codes.
- [x] `module-train status --tree HEAD` and `module-train next --tree HEAD`
      wired into the xtask CLI.
- [x] Frontier derivation from manifest data (typed edges, role rejection,
      controller-satisfaction law, binding-pending gate, ceilings-not-quotas).
- [x] 29 focused tests in `xtask/src/tasks/module_train_tests.rs`.
- [x] `.spec/11626-module-train-frontier/` bundle (this file's siblings).

## Proof (scoped; two-run deterministic)

```text
cargo test -p xtask --locked --bin xtask module_train
  -> test result: ok. 29 passed; 0 failed

cargo run -q -p xtask --locked -- module-train status --tree HEAD   (x2)
cargo run -q -p xtask --locked -- module-train next --tree HEAD      (x2)
  -> byte-identical within each command
  -> tree_head 112bc2cb2…; canonical_digest == pinned; match=yes
  -> ready_leaves: 4 (C02, E00A, M01, M07A)

cargo fmt -p xtask -- --check        -> clean
cargo clippy -p xtask --locked -- -D warnings
  -> zero findings in this PR's files; one pre-existing failure in
     tasks/gates.rs (lines_filter_map_ok) that fails identically on
     origin/main and is untouched here
```

## Known answers verified against C01's closeout

- C02 unblocked as next head (`ready`), C03 `blocked_hard` on C02.
- Case/work-packet bindings structurally pending; consumers (M00S, P11A,
  P11F) carry the typed pending reason, never satisfied.
- Controllers never enter the frontier; fan-in never enters; L09G cannot
  precede its admitted cutovers.

## Residuals (recorded on #11626; not proven here)

1. Per-node semantic implementation/retirement probes (E00/M/L09/P11
   families) — the largest remaining slice; presence stays `not_proven`
   until then.
2. `explain` static agent packet projection + `graph` + #11114 handoff.
3. Arbitrary-tree binding (`--tree HEAD` only) and JSON output.
4. Supersession / incomplete-current-tree projections (fail closed today).
5. C01's optional revision follow-up: adding C02 to
   `case_work_packet_bindings.consumers` (law already global; left to a
   future classified #11625 revision).

## Adoption / rollback

Adopt via the two CLI commands. Rollback = revert this PR; no other consumer
exists yet. A manifest semantic revision updates the digest pin and
re-derives via #11625's revision route.
