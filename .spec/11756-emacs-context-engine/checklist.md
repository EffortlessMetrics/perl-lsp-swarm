# Implementation Checklist: #11756 — Emacs exact-tree context resolver engine

## Change order

Tooling/data-only change: one xtask module, one population document, one CI
contract, this bundle. No product crate is touched.

### Step 1: Write the fail-closed fixtures first

- **File:** `xtask/src/tasks/emacs_train_context/tests.rs` (CREATE).
- **Change:** Synthetic-tree fixtures covering the happy path, per-node
  determinism, precise gap blockers, and one deliberately wrong input per
  numbered law (L01-L19), plus instruction-chain discovery, revision
  currency binding, and issue-number lookup.
- **Verify:** `cargo test -p xtask --bin xtask emacs_train_context --locked`
  — every law fixture must fail closed against the real resolver.

### Step 2: Implement the resolver/renderer laws

- **File:** `xtask/src/tasks/emacs_train_context/{model,digest,resolve,render,mod}.rs` (CREATE).
- **Change:** Typed manifest/mapping/packet models with
  `deny_unknown_fields`; SHA-256 digest binding incl. git identity; the
  fail-closed law set; deterministic json/markdown rendering; the
  `integration emacs train {context,contexts}` command surface wired in
  `xtask/src/main.rs` and `xtask/src/tasks/mod.rs`.
- **Verify:** focused clippy over the module; two-render determinism;
  unknown node ids list the denominator.

### Step 3: Ship the representative population document

- **File:** `.spec/11756-emacs-context-engine/context.mappings.v1.json` (CREATE).
- **Change:** Six representative mapped nodes (CTXENG self-mapping, E00/E01/
  E01R spec bundles, H7777/H7778 landed substrate with exact symbols) and
  three representative blockers (ADP_E, RUNCONF, REG) with owners.
- **Verify:** `cargo run -p xtask --locked -- integration emacs train
  contexts --check` — mapped nodes resolve, every other node yields its
  precise typed blocker, counts match the 55-node denominator.

### Step 4: Add the CI contract

- **File:** `.github/workflows/emacs-train-context-contract.yml` (CREATE).
- **Change:** Falsifier suite, denominator check, real-tree two-render
  determinism diff, one substrate render; path-triggered on every mapped
  surface so tree movement fails closed in CI.
- **Verify:** workflow syntax; `git diff --check`.

### Step 5: Compile the bundle

- **Files:** `context.md`, `acceptance.md`, `checklist.md` (CREATE).
- **Change:** problem, authority, durable laws, encoding decisions, claim
  boundary, non-goals per the checked-bundle pattern of #10918/#11770.
- **Verify:** `git diff --check`.

## Residual (explicitly not this PR)

- Full per-node population: #11757 (substrate/subjects/adapters/profiles/
  actual-host) and #11758 (root/public replay/registry/docs/certification).
- E04 fan-in closeout: #11718 stays open until both population leaves land.
- Packet adapter over shared contracts: #11719 (E06).
