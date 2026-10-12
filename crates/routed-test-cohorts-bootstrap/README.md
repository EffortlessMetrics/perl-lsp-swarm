# routed-test-cohorts-bootstrap

Lightweight bootstrap for the staged routed-test-cohorts producer (#17623).

## Why this crate exists

Building `-p xtask --bin routed-test-cohorts` resolves and compiles xtask's
full dependency graph (parser, LSP core, tree-sitter, …) before the producer
can run. This crate builds the *same* producer from a minimal dependency set
(clap, color-eyre, serde_json, sha2, toml) so admission/bootstrap is cheap.

## Sharing mechanism (no divergence)

Producer logic is **not** copied or extracted. The `[[bin]]` target in
`Cargo.toml` uses Cargo's `path` attribute to point directly at the canonical
source, `../../xtask/src/bin/routed-test-cohorts.rs`. Both
`cargo build -p xtask --bin routed-test-cohorts` and
`cargo build -p routed-test-cohorts-bootstrap` compile that one file, so
package/feature/profile selection, cohort inventory, and evidence semantics
cannot drift. Any edit to the producer file changes both binaries.

This was preferred over extracting a shared library crate because it requires
zero changes to the producer source (preserving the six-package selection,
features, profiles, and inventory verbatim) and keeps a single owner for the
logic in xtask, where its tests and history already live.

## Admission

`scripts/cargo_admitted.py` prefers this bootstrap binary for
`--routed-test-cohorts` invocations when this crate is present, and falls back
to the exact xtask invocation when it is not. Removing this crate (or the
rewrite block in `cargo_admitted.py`) restores the original path.

## Status

Staged only. Complete workload fit is NOT_PROVEN until native exact-candidate
qualification on Linux.
