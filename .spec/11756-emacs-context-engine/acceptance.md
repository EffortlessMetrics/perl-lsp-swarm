# Acceptance Criteria: #11756 — Emacs exact-tree context resolver engine

## §Behavior

- `cargo xtask integration emacs train context <node|issue> [--format json|markdown]`
  recomputes one bounded `emacs_node_context.v1` packet from manifest +
  ledger + mapping + exact tree on every invocation; nothing is cached.
- `cargo xtask integration emacs train contexts --check` walks the full
  stable denominator: every node either resolves to a validated packet or
  yields a precise typed blocker; any law violation fails the check.
- Exit code `3` distinguishes a mapping-gap packet from a full context (`0`);
  instrument failures exit `1` without a packet.

## §Hazards

- Same-named symbol from the wrong crate/script: fails (L12/L16 — exact-path
  anchoring plus cross-node production uniqueness).
- Helper/schema/fixture/generated file as production implementation: fails
  (L18 role/kind law).
- Generated output as canonical input: fails (L17/L18).
- Applicable local `AGENTS.md` omitted: fails (L19 — chains are recomputed,
  never stored).
- Stale path/symbol after tree movement: fails (L16 — digests and anchors
  recompute from the current tree).
- Context for another tree/node/spec digest: detectable — the packet embeds
  git commit/tree and manifest/ledger/mapping/input digests (L10).
- Eglot material mapped into an lsp-mode node or vice versa: fails (L15).
- Ambiguous mapping turned into broad directory writes: fails (L14).
- Private paths/source/logs or unbounded output: fails (L11 — normalized
  relative paths, bounded scans, digests only, no source text embedded).
- Network/live GitHub access: impossible by construction; no such code path
  exists in the engine.

## §Contracts

- Packet schema `emacs_node_context.v1` version 1 as typed in
  `xtask/src/tasks/emacs_train_context/model.rs`.
- Mapping document `emacs_train_context_mappings.v1` version 1 at
  `.spec/11756-emacs-context-engine/context.mappings.v1.json`, extended by
  the population leaves #11757/#11758.
- Population ownership routing: substrate lanes -> #11757, projection lanes
  -> #11758, plane mechanics -> #11756.

## §API-Shape

- One nested command group `integration emacs train {context,contexts}`;
  no other integration subcommands exist yet and none are implied.

## §Test-Grid

- 22 falsifier/determinism tests in `xtask/src/tasks/emacs_train_context/tests.rs`
  over synthetic fixture trees (happy path, determinism, gap blocker, and one
  test per numbered law).
- CI contract `.github/workflows/emacs-train-context-contract.yml`: falsifier
  suite, denominator check, two-render determinism diff on the real tree,
  and one substrate node render.

## §Blast-Radius

- Additive xtask tooling and one new `.spec` bundle; no product crates, no
  schemas under `schemas/`, no shared tooling modified. The `integration`
  command group is new and empty of other subcommands.

## Proof

```bash
cargo test -p xtask --bin xtask emacs_train_context --locked
cargo run -p xtask --locked -- integration emacs train contexts --check
cargo run -p xtask --locked -- integration emacs train context CTXENG --format json
git diff --check
```

## Claim boundary

Landed: the deterministic resolver/renderer mechanics, the fail-closed law
set, and representative population (six mapped nodes, three representative
blockers, auto-routed blockers for the rest). NOT landed: full per-node
population (#11757/#11758), the E04 fan-in closeout (#11718), shared packet
generation (#11719), and any live-state observation (#10930). A context row
is navigation evidence, never implementation truth.

## Non-goals

- No global source index, call graph, or semantic code-search service.
- No Emacs packet schema (E06 #11719 consumes this engine instead).
- No manifest/spec ownership change, readiness derivation, candidate
  selection, product execution, support decision, or external mutation.
