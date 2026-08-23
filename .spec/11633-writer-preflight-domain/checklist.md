# Checklist: #11633 — typed writer-preflight subject and decision core

Implementation order per the issue's Codex execution packet:

- [x] Create the `.spec` decision table (this packet, `context.md`).
- [x] Add missing-evidence, cross-subject, ambient/executor, collision,
  unique-state, behind-only, and unrelated-load falsifiers
  (`shift_left_1`…`shift_left_14` tests plus decision-table tests).
- [x] Implement pure types, exhaustive decision core, and deterministic
  projections (`subject.rs`, `observation.rs`, `decision.rs`,
  `projection.rs`; `render_human`/`explain` derive from one object).
- [x] Reconcile every fact with its current owner (observation-model doc
  comments name #3957/#9548/#9542 lineage; provider ownership deferred to
  #11634 adapters, none implemented here).
- [x] Run fresh false-pass and authority-boundary reviews (see review map
  in `context.md`; executed against the merged candidate before publish).

Proof gates (all green on the candidate):

- [x] `cargo test -p xtask --lib --locked writer_preflight` — 44 passed.
- [x] `cargo fmt -p xtask -- --check` — clean.
- [x] `cargo clippy -p xtask --lib --locked -- -D warnings` — clean.

Boundaries respected:

- [x] No live preflight command, no Bash/PowerShell migration, no Just
  change, no writer allocation, no Git repair, no Cargo execution, no
  process cleanup, no scheduler, no model/session identity.
- [x] Sibling leaves (#11634 adapters, #11635 consumption, #11636 parity)
  not implemented here; their consumer seams are documented in module docs
  and this packet instead.
