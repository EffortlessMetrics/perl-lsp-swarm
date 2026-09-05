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
frozen pointer document. It carries no counts and no rows, no command writes
it, and it never changes on `main`, so it cannot conflict on merge (#14688).
The merge check requires it to stay byte-identical to `main`; a branch that
regenerated it restores it with
`git checkout origin/main -- docs/policy/NON_RUST_INVENTORY.md`.

The inventory itself is evidence, not publication. The allowlist plus the
current-tree evaluator own the verdict; the ignored
`target/policy/non-rust-inventory.{md,json}` files are the per-run evidence,
and the policy CI shard uploads both as the `non-rust-inventory-<sha>`
artifact on every run, including runs on `main`, which is the default-branch
reference. The shard retains them when the check produces them, including
when the new-path ratchet fails.
