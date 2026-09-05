# File policy

Rust and `xtask` are the default implementation surfaces for this repository.
Non-Rust files are allowed where they serve a clear product, test, release, or
CI purpose, but those surfaces must not expand anonymously.

## Legitimate non-Rust surfaces

The policy recognizes these legitimate non-Rust areas:

- Perl fixtures and corpus data.
- Tree-sitter C/native parser bindings.
- VS Code extension surfaces.
- GitHub workflows and CI scripts.
- Generated evidence and default-branch publications.
- Release metadata, configuration, documentation, and assets.

## Active ledger

`policy/non-rust-allowlist.toml` is the active policy authority. Each entry
records its matcher, kind, language, surface, classification, owner, reason,
coverage, creation date, and review date. Broad globs also require
`broad_glob_reason`.

`cargo xtask non-rust inventory --check` validates that ledger, classifies the
current tracked tree, emits Markdown and JSON evidence under `target/policy/`,
warns on inherited unclassified debt, and rejects newly added unclassified
paths relative to the resolved merge baseline.

## Publication boundary

[`docs/policy/NON_RUST_INVENTORY.md`](policy/NON_RUST_INVENTORY.md) is a
human-readable default-branch publication. The post-merge publisher, or an
explicit `cargo xtask non-rust inventory --write`, may refresh it from one
current-tree scan.

Feature branches do not regenerate, stage, read, or byte-compare that
publication to establish merge validity. The allowlist plus the current-tree
evaluator own the verdict; the ignored `target/policy/non-rust-inventory.{md,json}`
files are the per-run evidence. The policy CI shard retains both projections
when the check produces them, including when the new-path ratchet fails.
