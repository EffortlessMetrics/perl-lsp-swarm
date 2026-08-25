# Context: #11756 — Emacs exact-tree context resolver engine

## Problem

Each stable Emacs train node owns a semantic proposition, but a fresh coding
agent still needs broad repository archaeology to find where that proposition
lives on the exact current tree: which instructions apply, which checked spec
governs, which files/symbols implement the seam, which tests own the claim,
which nearby files look relevant but belong to another issue, and where to
stop when the mapping is stale or ambiguous. Prose issue bodies cannot carry
this without becoming a second scope authority, and ad-hoc grep/navigation
substitutes filename affinity for semantic identity.

## Why this approach

One deterministic offline projection — `emacs_node_context.v1` — recomputes a
bounded per-node packet from exactly three data inputs plus the tree itself:

```text
.spec/10918-emacs-train-graph/train.manifest.json   (E01, stable semantics)
.spec/11770-emacs-train-revisions/revisions.ledger.json (E01R, currency)
.spec/11756-emacs-context-engine/context.mappings.v1.json (population)
+ exact current tree (paths, symbols, digests, instructions)
```

This is the CTXENG engine slice of the E04 context plane (#11718): resolver
mechanics plus representative population. Nothing is cached — every run
re-derives — so a stale packet cannot be emitted, and every packet embeds the
digests (git commit/tree, manifest, ledger, mapping, per-file) that make reuse
across trees detectable.

## Current state (honest, as of this bundle)

- Engine landed as `cargo xtask integration emacs train context <node|issue>`
  (`--format json|markdown`) and `... contexts --check` over the full stable
  denominator.
- Population covers six representative nodes (CTXENG, E00, E01, E01R, H7777,
  H7778) and three explicit representative blockers (ADP_E, RUNCONF, REG).
  Every other stable node resolves to a precise typed `mapping_gap` blocker
  naming its population owner (#11757 substrate / #11758 projection).
- Full population remains with the population leaves; #11718 stays open until
  that fan-in completes.

## Authority and ownership

- Semantic scope authority stays with `emacs_train.v1` (#10918) and the leaf
  specs (#11717). Paths and symbols in a packet are navigation evidence only.
- Revision currency stays with the E01R ledger (#11770), consumed as data.
- Population content stays with #11757/#11758; engine defects return here.
- Live candidate/writer state stays with #10930; this engine performs no
  network access at all and the packet schema has no live-state fields.

## Durable laws consumed

The resolver enforces numbered fail-closed laws (see `resolve.rs`): schema and
identity validation with `deny_unknown_fields` (L01–L04), ledger-schema
identity (L05), mapping schema/status/bounds (L06–L09), exact-tree git binding
(L10), path normalization and scan bounds (L11), cross-node production-symbol
uniqueness (L12), minimum-context completeness (L13), exact-file write sets
(L14), client-family separation (L15), symbol anchoring at the exact path with
stale detection (L16), generated-surface existence without execution (L17),
role/kind separation so helpers/fixtures/schemas/generated output can never be
presented as production implementation (L18), and mandatory recomputed
`AGENTS.md` chains (L19).

## Encoding decisions and traceability

- Exit codes: `0` full context, `3` precise mapping-gap packet (still
  printed), `1` instrument/law failure — a blocker is a valid answer for a
  fresh agent but must be distinguishable by callers.
- Determinism: fixed field order, canonical sorting, and an in-process
  two-render comparison for every node inside `contexts --check`; the CI
  contract additionally renders one real node twice and diffs.
- Symbols anchor at their exact declared path with a bounded line scan; a
  same-named symbol elsewhere can never satisfy a mapping.
- The revision ledger is consumed through typed accessors over known
  reference fields, never re-derived or cloned, per its own consumption rule.

## Compatibility with the repository operating contract (`AGENTS.md`)

Focused proof only (`cargo test -p xtask --bin xtask emacs_train_context` and
`cargo run -p xtask -- integration emacs train contexts --check`); no
repository-wide checks are gated behind this engine. Production code follows
the no-`unwrap`/`expect`/`panic` hygiene; tests follow the same discipline.

## Open decisions respected, not decided

- OD1 (shared mechanics reuse gate, #10554): not decided here; the engine
  composes the T02 projection pattern without extracting shared machinery.
- Candidate vacancy, readiness and actor routing remain with #10930/#11719.

## Adoption, rollback, transfer and stop

- Adoption: population leaves extend `context.mappings.v1.json`; the engine
  and its laws are reused unchanged.
- Rollback: revert this bundle and module; contexts return to manual
  navigation. No product behavior depends on the engine.
- Transfer: tree movement invalidates packets through digests; re-derive,
  never patch a packet valid.
- Stop: stop before live GitHub observation, packet generation (#11719),
  product proof, or treating a context row as implementation truth.

## Links

- Parent plane: #11718 (E04 fan-in), decomposition note therein.
- Consumed: #10918 manifest, #11770 ledger, #11716 architecture bundle.
- Population leaves: #11757, #11758. Sibling precedent: T02 #11765 pattern.
