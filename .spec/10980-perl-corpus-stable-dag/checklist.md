# Implementation Checklist: #10980 — encode the stable perl-corpus authority DAG, conflict keys, and legacy exits

## Change order (compiles at each step)

### Step 1: Closed JSON Schema
- **File:** `schemas/perl_corpus_train.v1.schema.json` (CREATE)
- **Change:** Closed document/node schema: roles, dependency classes, release horizons,
  conflict-key pattern, subject/lineage/authority reference patterns, `legacy_exit`,
  `spec`, `stable_versus_mutable`, `revision_governance`.
- **Verify:** `cargo xtask perl-corpus-train check` (schema applied through `jsonschema`)

### Step 2: Checker, projections, explain-static
- **File:** `xtask/src/tasks/perl_corpus_train.rs` (CREATE); `xtask/src/tasks/mod.rs`,
  `xtask/src/main.rs` (MODIFY: module registration, `perl-corpus-train` subcommand)
- **Change:** `validate_document` with the 22 named reason codes; `render_projections`
  (JSON/Markdown/DOT/Mermaid, sorted, digest-bound); `render_explain_static`;
  `run_check` / `run_graph` / `run_explain_static`. Reuses
  `module_train::canonical_digest` and `native_neovim_train::canonical_form`.
- **Verify:** `cargo check -p xtask --all-targets --locked`

### Step 3: Falsifiers first
- **File:** `xtask/src/tasks/perl_corpus_train_tests.rs` (CREATE);
  `.spec/10980-perl-corpus-stable-dag/invalid/*.json` + `expected_errors.json` (CREATE)
- **Change:** The twelve #10980 falsifiers as in-memory mutations of the landed manifest,
  plus twenty small schema-valid invalid fixtures, one named law each, plus the schema
  fixture (`unknown_key_schema.json`).
- **Verify:** `cargo test -p xtask --locked perl_corpus_train`

### Step 4: Seed manifest and shuffled control
- **File:** `.spec/10980-perl-corpus-stable-dag/train.manifest.json`,
  `.spec/10980-perl-corpus-stable-dag/shuffled/train.manifest.json` (CREATE)
- **Change:** Every current #8826 controller and concrete leaf (106 nodes, 256 typed
  edges with header/comment provenance, 63 conflict keys, candidate lineages, exits).
- **Verify:** `cargo xtask perl-corpus-train check`

### Step 5: Generated projections
- **File:** `.spec/10980-perl-corpus-stable-dag/projections/train.graph.{json,md,dot,mmd}` (CREATE)
- **Change:** `cargo xtask perl-corpus-train graph`; committed bytes are freshness-checked.
- **Verify:** `cargo xtask perl-corpus-train graph --check`

### Step 6: Shared contract registration and policy
- **File:** `.spec/10858-train-edge-contract/adaptations.json`,
  `policy/non-rust-allowlist.toml` (MODIFY)
- **Change:** Three adaptation rows and one manifest entry; allowlist row for the schema.
- **Verify:** `cargo xtask check-train-edge-contract`; `cargo xtask non-rust check`

### Step 7: Final verification
- **Verify:** `cargo fmt -p xtask -- --check && cargo clippy -p xtask --all-targets --locked -- -D warnings && cargo test -p xtask --locked && git diff --check`

## Callers and consumers

- `perl_corpus_train::run_check` / `run_graph` / `run_explain_static` are called from
  `xtask/src/main.rs` (`Commands::PerlCorpusTrain`).
- `train.manifest.json` is read by `perl_corpus_train` and by
  `train_edge_contract::run` through `adaptations.json`.
- `module_train::canonical_digest` and `native_neovim_train::canonical_form` are used by
  `perl_corpus_train` (read-only reuse).

## Scope boundary

Files IN scope: the `.spec/10980-perl-corpus-stable-dag/` bundle (manifest, shuffled
control, invalid fixtures, projections, three markdown files),
`schemas/perl_corpus_train.v1.schema.json`, `xtask/src/tasks/perl_corpus_train.rs`,
`xtask/src/tasks/perl_corpus_train_tests.rs`, the three-line registrations in
`xtask/src/tasks/mod.rs` and `xtask/src/main.rs`,
`.spec/10858-train-edge-contract/adaptations.json`, `policy/non-rust-allowlist.toml`,
a pointer in `crates/perl-corpus/CLAUDE.md`.

Files OUT of scope: everything else — no `crates/perl-corpus` source or asset, no other
train bundle, no current-tree probe, frontier, live observation, spec compiler, packet
generator, scheduler, workflow, or GitHub state.

## Flags for builder

- Leaf headers naming an umbrella (#6980, #6985, #6989, #6716, #7009) are routed to the
  owning leaf with the original statement in `provenance`; do not add controller edges.
- Parallel siblings that the controller declares disjoint (#11580/#11034, the three CI
  routes, the three consumer families, the parser-accuracy strata, the #11030 children)
  must keep distinct exclusive keys; a shared key without an ordering path is a
  rejection, not a warning.
- Lineage rows must not carry status words (open/merged/closed/draft/landed); the
  checker scans for them.
- After any manifest edit: `cargo xtask perl-corpus-train graph` then `check`.

## Deterministic proof

Two consecutive runs of `cargo xtask perl-corpus-train graph` produce no diff; `check`
proves the shuffled control canonizes and projects byte-identically. The exact
canonical digest is printed inside `projections/train.graph.json` and `train.graph.md`.

## Not proven here

Current-tree state, readiness, candidate vacancy, packet correctness, and the semantic
correctness of every leaf reading beyond its cited header statement remain `not_proven`
and are owned by #10992, #11001, #11010, #11017, and the #8826 revision route.
